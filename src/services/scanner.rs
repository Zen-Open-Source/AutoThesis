use crate::{
    app_state::AppState,
    models::{PreliminaryThesisOutput, ScanOpportunity, ScanRun, ScannerConfig, TickerUniverse},
    services::{opportunity_ranker, preliminary_thesis, signal_detector},
};
use anyhow::Result;
use serde_json;
use tracing::{error, info};

/// Execute a scan run across the ticker universe.
pub async fn execute_scan(state: AppState, scan_run_id: String) -> Result<()> {
    let result = execute_scan_inner(&state, &scan_run_id).await;

    if let Err(ref error) = result {
        error!(%scan_run_id, error = %error, "scan run failed");
        let _ = state
            .db
            .complete_scan_run(&scan_run_id, Some(&error.to_string()))
            .await;
        return result;
    }

    let _ = state.db.complete_scan_run(&scan_run_id, None).await;
    result
}

async fn execute_scan_inner(state: &AppState, scan_run_id: &str) -> Result<()> {
    info!(%scan_run_id, "starting scan run");

    state.db.set_scan_run_status(scan_run_id, "running").await?;

    let scan_run = state.db.get_scan_run(scan_run_id).await?;
    let config = if let Some(run) = &scan_run {
        if let Some(config_id) = &run.config_id {
            state.db.get_scanner_config(config_id).await?
        } else {
            None
        }
    } else {
        None
    };

    let tickers = build_ticker_list(state, config.as_ref()).await?;
    info!(%scan_run_id, ticker_count = tickers.len(), "ticker list built");

    let mut opportunities = Vec::new();
    let mut tickers_scanned = 0i64;

    for ticker_info in &tickers {
        match process_ticker(state, scan_run_id, ticker_info).await {
            Ok(Some(opportunity)) => {
                opportunities.push(opportunity);
            }
            Ok(None) => {}
            Err(error) => {
                info!(ticker = %ticker_info.ticker, error = %error, "ticker scan failed");
            }
        }
        tickers_scanned += 1;

        if tickers_scanned % 10 == 0 {
            let _ = state
                .db
                .update_scan_run_progress(scan_run_id, tickers_scanned, opportunities.len() as i64)
                .await;
        }
    }

    let max_opportunities = config
        .as_ref()
        .map(|c| c.max_opportunities as usize)
        .unwrap_or(20);
    opportunities = opportunity_ranker::filter_top_opportunities(opportunities, max_opportunities);

    for opportunity in &opportunities {
        let _ = persist_opportunity(state, scan_run_id, opportunity).await;
    }

    state
        .db
        .update_scan_run_progress(scan_run_id, tickers_scanned, opportunities.len() as i64)
        .await?;

    info!(
        %scan_run_id,
        tickers_scanned,
        opportunities_found = opportunities.len(),
        "scan run completed"
    );

    Ok(())
}

async fn build_ticker_list(
    state: &AppState,
    config: Option<&ScannerConfig>,
) -> Result<Vec<TickerUniverse>> {
    let sector_filter = config.and_then(|c| c.sector_filter.as_deref());
    let min_market_cap = config.and_then(|c| c.min_market_cap);
    let max_market_cap = config.and_then(|c| c.max_market_cap);

    state
        .db
        .list_ticker_universe(true, sector_filter, min_market_cap, max_market_cap)
        .await
}

async fn process_ticker(
    state: &AppState,
    scan_run_id: &str,
    ticker_info: &TickerUniverse,
) -> Result<Option<ScanOpportunity>> {
    let ticker = &ticker_info.ticker;

    let signals = signal_detector::detect_signals(state, ticker).await?;

    let signal_strength = signal_detector::calculate_signal_strength(&signals);

    let coverage_gap = opportunity_ranker::calculate_coverage_gap_score(state, ticker).await?;

    if !opportunity_ranker::meets_minimum_criteria(signal_strength, coverage_gap, 3.0, 5.0) {
        return Ok(None);
    }

    let timing = signal_detector::calculate_timing_score(&signals);

    let PreliminaryThesisOutput {
        thesis_markdown,
        key_catalysts,
        risk_factors,
        quality_score,
    } = preliminary_thesis::generate_preliminary_thesis(state, ticker, &signals).await?;

    let thesis_html = preliminary_thesis::render_thesis_html(&thesis_markdown);

    let overall_score = opportunity_ranker::calculate_overall_score(
        signal_strength,
        Some(quality_score),
        coverage_gap,
        timing,
    );

    let opportunity = ScanOpportunity {
        id: uuid::Uuid::new_v4().to_string(),
        scan_run_id: scan_run_id.to_string(),
        ticker: ticker.clone(),
        overall_score,
        signal_strength_score: signal_strength,
        thesis_quality_score: Some(quality_score),
        coverage_gap_score: coverage_gap,
        timing_score: timing,
        signals_json: serde_json::to_string(&signals)?,
        preliminary_thesis_markdown: Some(thesis_markdown),
        preliminary_thesis_html: Some(thesis_html),
        key_catalysts: Some(key_catalysts),
        risk_factors: Some(risk_factors),
        promoted_to_run_id: None,
        status: "new".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    Ok(Some(opportunity))
}

async fn persist_opportunity(
    state: &AppState,
    scan_run_id: &str,
    opportunity: &ScanOpportunity,
) -> Result<()> {
    state
        .db
        .create_scan_opportunity(
            scan_run_id,
            &opportunity.ticker,
            opportunity.overall_score,
            opportunity.signal_strength_score,
            opportunity.thesis_quality_score,
            opportunity.coverage_gap_score,
            opportunity.timing_score,
            &opportunity.signals_json,
            opportunity.preliminary_thesis_markdown.as_deref(),
            opportunity.preliminary_thesis_html.as_deref(),
            opportunity.key_catalysts.as_deref(),
            opportunity.risk_factors.as_deref(),
        )
        .await?;
    Ok(())
}

/// Start a new scan run.
pub async fn start_scan(state: &AppState, config_id: Option<&str>) -> Result<ScanRun> {
    let scan_run = state.db.create_scan_run(config_id).await?;

    // Scan runs are not orchestrator runs (no LLM research cycle per scan),
    // so they do not acquire the run semaphore. Spawn directly.
    let state_clone = state.clone();
    let scan_run_id = scan_run.id.clone();
    tokio::spawn(async move {
        if let Err(error) = execute_scan(state_clone, scan_run_id.clone()).await {
            error!(%scan_run_id, error = %error, "background scan failed");
        }
    });

    Ok(scan_run)
}

/// Get scanner dashboard data.
pub async fn build_scanner_dashboard(state: &AppState) -> Result<crate::models::ScannerDashboard> {
    let latest_scan_run = state.db.list_scan_runs(1).await?.into_iter().next();

    let top_opportunities = if let Some(ref scan_run) = latest_scan_run {
        state
            .db
            .list_scan_opportunities_for_run(&scan_run.id)
            .await?
    } else {
        state.db.list_top_scan_opportunities(10).await?
    };

    let total_tickers = state.db.count_ticker_universe(true).await?;
    let active_config = state.db.get_default_scanner_config().await?;

    Ok(crate::models::ScannerDashboard {
        latest_scan_run,
        top_opportunities,
        total_tickers_in_universe: total_tickers,
        active_config,
    })
}
