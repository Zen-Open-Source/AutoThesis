use crate::{
    app_state::AppState,
    models::{DashboardResponse, DashboardTickerRow},
    services::alerts::{self, TickerAlertSnapshot},
    status::RunStatus,
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::HashMap;

pub async fn build_watchlist_dashboard(
    state: &AppState,
    watchlist_id: &str,
) -> Result<DashboardResponse> {
    let watchlist = state
        .db
        .get_watchlist(watchlist_id)
        .await?
        .ok_or_else(|| anyhow!("watchlist not found: {watchlist_id}"))?;
    let watchlist_tickers = state.db.list_watchlist_tickers(watchlist_id).await?;
    let alert_rules = state.db.list_or_create_alert_rules(watchlist_id).await?;

    // --- Single batched pre-fetch replaces the per-ticker N+1. ---
    let ticker_ids: Vec<String> = watchlist_tickers
        .iter()
        .map(|wt| wt.ticker.clone())
        .collect();
    // Up to the two most-recent runs per ticker (latest + previous).
    let runs_by_ticker = state.db.recent_runs_for_tickers(&ticker_ids, 2).await?;
    // Collect run_ids so we can batch-fetch their scores / source freshness.
    let run_ids: Vec<String> = runs_by_ticker
        .values()
        .flat_map(|runs| runs.iter().map(|r| r.id.clone()))
        .collect();
    let scores_by_run = state.db.latest_scores_for_runs(&run_ids).await?;
    let source_ts_by_run = state.db.latest_source_timestamps_for_runs(&run_ids).await?;

    let mut rows = Vec::new();
    for watchlist_ticker in watchlist_tickers {
        let recent_runs: &[crate::models::Run] = runs_by_ticker
            .get(&watchlist_ticker.ticker)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let latest_run = recent_runs.first();
        let previous_run = recent_runs.get(1);

        let latest_score = latest_run.and_then(|run| scores_by_run.get(&run.id).copied());
        let previous_score = previous_run.and_then(|run| scores_by_run.get(&run.id).copied());
        let score_delta = match (latest_score, previous_score) {
            (Some(latest), Some(previous)) => Some(latest - previous),
            _ => None,
        };
        let trend = classify_trend(score_delta).to_string();

        let latest_source_timestamp =
            latest_run.and_then(|run| source_ts_by_run.get(&run.id).copied());
        let evidence_freshness = classify_freshness(latest_source_timestamp);
        let previous_evidence_freshness = previous_run
            .map(|run| classify_freshness(source_ts_by_run.get(&run.id).copied()))
            .unwrap_or_else(|| "no_evidence".to_string());

        let latest_status = latest_run
            .map(|run| run.status.clone())
            .unwrap_or_else(|| "no_data".to_string());
        let decision_state = classify_decision(&latest_status, latest_score, &evidence_freshness);
        let previous_decision_state = previous_run.map(|run| {
            classify_decision(&run.status, previous_score, &previous_evidence_freshness)
        });

        let snapshot = TickerAlertSnapshot {
            ticker: watchlist_ticker.ticker.clone(),
            latest_run_id: latest_run.map(|run| run.id.clone()),
            latest_status: latest_status.clone(),
            latest_score,
            previous_score,
            evidence_freshness: evidence_freshness.clone(),
            decision_state: decision_state.clone(),
            previous_decision_state,
        };
        alerts::evaluate_ticker_snapshot(state, watchlist_id, &alert_rules, &snapshot).await?;

        rows.push(DashboardTickerRow {
            ticker: watchlist_ticker.ticker,
            latest_run_id: latest_run.map(|run| run.id.clone()),
            latest_status,
            latest_score,
            previous_score,
            score_delta,
            trend,
            summary: latest_run.and_then(|run| run.summary.clone()),
            evidence_freshness,
            decision_state,
            active_alert_count: 0,
            last_run_updated_at: latest_run.map(|run| run.updated_at),
        });
    }

    let active_alerts = state
        .db
        .list_thesis_alerts(watchlist_id, Some("active"))
        .await?;
    let mut active_alert_counts: HashMap<String, i64> = HashMap::new();
    for alert in &active_alerts {
        *active_alert_counts.entry(alert.ticker.clone()).or_insert(0) += 1;
    }
    for row in &mut rows {
        row.active_alert_count = active_alert_counts.get(&row.ticker).copied().unwrap_or(0);
    }

    Ok(DashboardResponse {
        watchlist,
        rows,
        active_alerts,
        generated_at: Utc::now(),
    })
}

fn classify_trend(score_delta: Option<f64>) -> &'static str {
    match score_delta {
        Some(delta) if delta > 0.3 => "improving",
        Some(delta) if delta < -0.3 => "deteriorating",
        Some(_) => "flat",
        None => "unknown",
    }
}

fn classify_freshness(latest_source_timestamp: Option<chrono::DateTime<Utc>>) -> String {
    match latest_source_timestamp {
        Some(timestamp) => {
            let days_old = (Utc::now() - timestamp).num_days();
            if days_old <= 7 {
                "fresh".to_string()
            } else if days_old <= 30 {
                "recent".to_string()
            } else {
                "stale".to_string()
            }
        }
        None => "no_evidence".to_string(),
    }
}

fn classify_decision(
    latest_status: &str,
    latest_score: Option<f64>,
    evidence_freshness: &str,
) -> String {
    if latest_status == "no_data" {
        return "no_coverage".to_string();
    }
    match RunStatus::parse(latest_status) {
        Some(RunStatus::Queued | RunStatus::Running) => "researching".to_string(),
        Some(RunStatus::Failed | RunStatus::Cancelled) => "attention".to_string(),
        _ => match latest_score {
            Some(score) if score >= 8.0 && evidence_freshness == "fresh" => {
                "high_conviction".to_string()
            }
            Some(score) if score >= 7.0 => "watch".to_string(),
            Some(score) if score >= 6.0 => "mixed".to_string(),
            Some(_) => "low_conviction".to_string(),
            None => "needs_review".to_string(),
        },
    }
}
