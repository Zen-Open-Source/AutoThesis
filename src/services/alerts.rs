use crate::{app_state::AppState, models::AlertRule, status::RunStatus};
use anyhow::Result;
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct TickerAlertSnapshot {
    pub ticker: String,
    pub latest_run_id: Option<String>,
    pub latest_status: String,
    pub latest_score: Option<f64>,
    pub previous_score: Option<f64>,
    pub evidence_freshness: String,
    pub decision_state: String,
    pub previous_decision_state: Option<String>,
}

pub async fn evaluate_watchlists_for_ticker(state: &AppState, ticker: &str) -> Result<()> {
    let watchlist_ids = state.db.list_watchlist_ids_for_ticker(ticker).await?;
    for watchlist_id in watchlist_ids {
        evaluate_watchlist_ticker(state, &watchlist_id, ticker).await?;
    }
    Ok(())
}

pub async fn evaluate_watchlist_ticker(
    state: &AppState,
    watchlist_id: &str,
    ticker: &str,
) -> Result<()> {
    let rules = state.db.list_or_create_alert_rules(watchlist_id).await?;
    let snapshot = build_snapshot(state, ticker).await?;
    evaluate_ticker_snapshot(state, watchlist_id, &rules, &snapshot).await
}

pub async fn evaluate_ticker_snapshot(
    state: &AppState,
    watchlist_id: &str,
    rules: &[AlertRule],
    snapshot: &TickerAlertSnapshot,
) -> Result<()> {
    if should_skip_status(&snapshot.latest_status) {
        return Ok(());
    }
    let Some(run_id) = snapshot.latest_run_id.as_deref() else {
        return Ok(());
    };

    for rule in rules {
        if !rule.enabled {
            continue;
        }

        match rule.rule_type.as_str() {
            "score_drop" => {
                let threshold = rule.threshold.unwrap_or(0.8);
                if let (Some(latest), Some(previous)) =
                    (snapshot.latest_score, snapshot.previous_score)
                {
                    let drop = previous - latest;
                    if drop >= threshold {
                        let severity = if drop >= 2.0 { "critical" } else { "warning" };
                        let message = format!(
                            "{} score dropped by {:.1} points ({:.1} → {:.1})",
                            snapshot.ticker, drop, previous, latest
                        );
                        state
                            .db
                            .create_thesis_alert_if_absent(
                                watchlist_id,
                                &snapshot.ticker,
                                run_id,
                                "score_drop",
                                severity,
                                &message,
                            )
                            .await?;
                    }
                }
            }
            "freshness_stale" => {
                if snapshot.evidence_freshness == "stale" {
                    let message =
                        format!("{} evidence is stale (older than 30 days)", snapshot.ticker);
                    state
                        .db
                        .create_thesis_alert_if_absent(
                            watchlist_id,
                            &snapshot.ticker,
                            run_id,
                            "freshness_stale",
                            "warning",
                            &message,
                        )
                        .await?;
                }
            }
            "decision_downgrade" => {
                if is_decision_downgrade(
                    snapshot.previous_decision_state.as_deref(),
                    &snapshot.decision_state,
                ) {
                    let severity = if snapshot.decision_state == "attention" {
                        "critical"
                    } else {
                        "warning"
                    };
                    let from = snapshot
                        .previous_decision_state
                        .as_deref()
                        .unwrap_or("unknown")
                        .replace('_', " ");
                    let to = snapshot.decision_state.replace('_', " ");
                    let message = format!(
                        "{} decision downgraded from {} to {}",
                        snapshot.ticker, from, to
                    );
                    state
                        .db
                        .create_thesis_alert_if_absent(
                            watchlist_id,
                            &snapshot.ticker,
                            run_id,
                            "decision_downgrade",
                            severity,
                            &message,
                        )
                        .await?;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn build_snapshot(state: &AppState, ticker: &str) -> Result<TickerAlertSnapshot> {
    let recent_runs = state.db.list_runs_for_ticker(ticker, 2).await?;
    let latest_run = recent_runs.first().cloned();
    let previous_run = recent_runs.get(1).cloned();

    let latest_score = if let Some(run) = latest_run.as_ref() {
        state
            .db
            .get_latest_iteration_evaluation_score(&run.id)
            .await?
    } else {
        None
    };
    let previous_score = if let Some(run) = previous_run.as_ref() {
        state
            .db
            .get_latest_iteration_evaluation_score(&run.id)
            .await?
    } else {
        None
    };

    let latest_freshness = if let Some(run) = latest_run.as_ref() {
        let latest_source_timestamp = state
            .db
            .get_latest_source_timestamp_for_run(&run.id)
            .await?;
        classify_freshness(latest_source_timestamp)
    } else {
        "no_evidence".to_string()
    };

    let latest_status = latest_run
        .as_ref()
        .map(|run| run.status.clone())
        .unwrap_or_else(|| "no_data".to_string());
    let decision_state = classify_decision(&latest_status, latest_score, &latest_freshness);

    let previous_decision_state = if let Some(run) = previous_run.as_ref() {
        let previous_freshness = classify_freshness(
            state
                .db
                .get_latest_source_timestamp_for_run(&run.id)
                .await?,
        );
        let previous_decision = classify_decision(&run.status, previous_score, &previous_freshness);
        Some(previous_decision)
    } else {
        None
    };

    Ok(TickerAlertSnapshot {
        ticker: ticker.to_string(),
        latest_run_id: latest_run.as_ref().map(|run| run.id.clone()),
        latest_status,
        latest_score,
        previous_score,
        evidence_freshness: latest_freshness,
        decision_state,
        previous_decision_state,
    })
}

fn should_skip_status(status: &str) -> bool {
    if status == "no_data" {
        return true;
    }
    matches!(
        RunStatus::parse(status),
        Some(RunStatus::Queued | RunStatus::Running)
    )
}

fn is_decision_downgrade(previous: Option<&str>, current: &str) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    matches!(previous, "high_conviction" | "watch")
        && matches!(current, "mixed" | "low_conviction" | "attention")
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
