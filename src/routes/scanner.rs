use crate::{
    app_state::AppState,
    error::AppError,
    models::{
        CreateScanRunRequest, CreateScanRunResponse, PromoteOpportunityRequest,
        PromoteOpportunityResponse, ScanOpportunityDetail, ScanRunDetail,
    },
    services::scanner,
    utils::{normalize_ticker, AppResult},
};
use axum::{
    extract::{Path, State},
    Json,
};
use tracing::error;

pub async fn create_scan_run(
    State(state): State<AppState>,
    Json(payload): Json<CreateScanRunRequest>,
) -> AppResult<Json<CreateScanRunResponse>> {
    let scan_run = scanner::start_scan(&state, payload.config_id.as_deref())
        .await
        .map_err(AppError::from)?;

    Ok(Json(CreateScanRunResponse {
        scan_run_id: scan_run.id,
        status: scan_run.status,
    }))
}

pub async fn list_scan_runs(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<crate::models::ScanRun>>> {
    let scan_runs = state.db.list_scan_runs(10).await.map_err(AppError::from)?;
    Ok(Json(scan_runs))
}

pub async fn get_scan_run(
    Path(scan_run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<ScanRunDetail>> {
    let scan_run = state
        .db
        .get_scan_run(&scan_run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let opportunities = state
        .db
        .list_scan_opportunities_for_run(&scan_run_id)
        .await
        .map_err(AppError::from)?;

    Ok(Json(ScanRunDetail {
        scan_run,
        opportunities,
    }))
}

pub async fn get_scan_opportunity(
    Path(opportunity_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<ScanOpportunityDetail>> {
    let opportunity = state
        .db
        .get_scan_opportunity(&opportunity_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let signals: Vec<crate::models::ScanSignal> =
        serde_json::from_str(&opportunity.signals_json).unwrap_or_default();

    let ticker_info = state
        .db
        .get_ticker_universe(&opportunity.ticker)
        .await
        .map_err(AppError::from)?;

    let existing_run = if let Some(ref run_id) = opportunity.promoted_to_run_id {
        state.db.get_run(run_id).await.map_err(AppError::from)?
    } else {
        None
    };

    Ok(Json(ScanOpportunityDetail {
        opportunity,
        signals,
        ticker_info,
        existing_run,
    }))
}

pub async fn promote_opportunity(
    Path(opportunity_id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<PromoteOpportunityRequest>,
) -> AppResult<Json<PromoteOpportunityResponse>> {
    let opportunity = state
        .db
        .get_scan_opportunity(&opportunity_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    if opportunity.promoted_to_run_id.is_some() {
        return Err(AppError::BadRequest(
            "opportunity already promoted".to_string(),
        ));
    }

    let question = payload.question.unwrap_or_else(|| {
        format!(
            "What is the current bull and bear case for {}?",
            opportunity.ticker
        )
    });

    let run = state
        .db
        .create_run(&opportunity.ticker, &question)
        .await
        .map_err(AppError::from)?;

    state
        .db
        .insert_event(&run.id, None, "queued", "Run queued from scanner", None)
        .await
        .map_err(AppError::from)?;

    state
        .db
        .promote_scan_opportunity(&opportunity_id, &run.id)
        .await
        .map_err(AppError::from)?;

    // Spawn the research run
    let state_clone = state.clone();
    let run_id = run.id.clone();
    tokio::spawn(async move {
        if let Err(error) =
            crate::services::orchestrator::execute_run(state_clone, run_id.clone()).await
        {
            error!(%run_id, error = %error, "background orchestrator failed");
        }
    });

    Ok(Json(PromoteOpportunityResponse {
        run_id: run.id,
        status: "queued".to_string(),
    }))
}

pub async fn dismiss_opportunity(
    Path(opportunity_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    let dismissed = state
        .db
        .dismiss_scan_opportunity(&opportunity_id)
        .await
        .map_err(AppError::from)?;

    if !dismissed {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({
        "opportunity_id": opportunity_id,
        "status": "dismissed",
    })))
}

pub async fn get_scanner_dashboard(
    State(state): State<AppState>,
) -> AppResult<Json<crate::models::ScannerDashboard>> {
    let dashboard = scanner::build_scanner_dashboard(&state)
        .await
        .map_err(AppError::from)?;
    Ok(Json(dashboard))
}

pub async fn list_ticker_universe(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<crate::models::TickerUniverse>>> {
    let tickers = state
        .db
        .list_ticker_universe(true, None, None, None)
        .await
        .map_err(AppError::from)?;
    Ok(Json(tickers))
}

pub async fn add_ticker_to_universe(
    State(state): State<AppState>,
    Json(payload): Json<AddTickerRequest>,
) -> AppResult<Json<crate::models::TickerUniverse>> {
    let ticker = normalize_ticker(&payload.ticker)?;

    let ticker_entry = state
        .db
        .upsert_ticker_universe(&ticker, payload.name.as_deref(), None, None, None, false)
        .await
        .map_err(AppError::from)?;

    Ok(Json(ticker_entry))
}

#[derive(Debug, serde::Deserialize)]
pub struct AddTickerRequest {
    pub ticker: String,
    pub name: Option<String>,
}
