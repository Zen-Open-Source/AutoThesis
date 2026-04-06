use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    models::{Comparison, ComparisonDetail, CreateComparisonRequest, CreateComparisonResponse},
    services::orchestrator,
};
use axum::{
    extract::{Path, State},
    response::Json as AxumJson,
};
use tracing::error;

pub async fn create_comparison(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateComparisonRequest>,
) -> AppResult<AxumJson<CreateComparisonResponse>> {
    // Normalize tickers
    let tickers: Vec<String> = payload
        .tickers
        .iter()
        .map(|t| normalize_ticker(t))
        .collect::<Result<Vec<_>, _>>()?;

    if tickers.is_empty() {
        return Err(AppError::BadRequest("at least one ticker is required".to_string()));
    }

    // Use default question if not provided
    let question = payload
        .question
        .unwrap_or_else(|| "Compare these stocks across valuation, growth, and risk factors".to_string());

    // Create the comparison record
    let comparison = state
        .db
        .create_comparison(&payload.name, &question)
        .await
        .map_err(AppError::from)?;

    // Create individual runs for each ticker and link them to the comparison
    for (idx, ticker) in tickers.iter().enumerate() {
        let ticker_question = format!("{}: {}", ticker, question);
        let run = state
            .db
            .create_run(ticker, &ticker_question)
            .await
            .map_err(AppError::from)?;
        
        state
            .db
            .add_run_to_comparison(&comparison.id, &run.id, ticker, idx as i64)
            .await
            .map_err(AppError::from)?;

        // Spawn background task for this run
        let state_for_task = state.clone();
        let run_id = run.id.clone();
        tokio::spawn(async move {
            if let Err(error) = orchestrator::execute_run(state_for_task, run_id.clone()).await {
                error!(%run_id, error = %error, "background orchestrator failed");
            }
        });
    }

    // Update comparison status to queued
    state
        .db
        .update_comparison_status(&comparison.id, "queued")
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(CreateComparisonResponse {
        comparison_id: comparison.id,
        status: "queued".to_string(),
    }))
}

pub async fn list_comparisons(
    State(state): State<AppState>,
) -> AppResult<AxumJson<Vec<crate::models::Comparison>>> {
    let comparisons = state.db.list_comparisons(25).await.map_err(AppError::from)?;
    Ok(AxumJson(comparisons))
}

pub async fn get_comparison(
    Path(comparison_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<ComparisonDetail>> {
    let comparison = state
        .db
        .get_comparison(&comparison_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let comparison_runs = state
        .db
        .list_comparison_runs(&comparison_id)
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(ComparisonDetail {
        comparison,
        comparison_runs,
    }))
}

pub async fn delete_comparison(
    Path(comparison_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    state
        .db
        .delete_comparison(&comparison_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(serde_json::json!({ "deleted": true })))
}

fn normalize_ticker(raw: &str) -> AppResult<String> {
    let cleaned = raw.trim().to_uppercase();
    if cleaned.is_empty() {
        return Err(AppError::BadRequest("ticker is required".to_string()));
    }
    if !cleaned
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(AppError::BadRequest(
            "ticker must contain only letters, numbers, '.' or '-'".to_string(),
        ));
    }
    Ok(cleaned)
}
