use crate::{
    app_state::AppState,
    error::AppError,
    models::{Comparison, ComparisonDetail, CreateComparisonRequest, CreateComparisonResponse},
    services::orchestrator,
    utils::{normalize_ticker, render_question_for_ticker, sanitize_question, AppResult},
};
use axum::{
    extract::{Path, State},
    response::Json as AxumJson,
};
use std::collections::HashSet;
use tracing::error;

pub async fn create_comparison(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateComparisonRequest>,
) -> AppResult<AxumJson<CreateComparisonResponse>> {
    let comparison_name = payload.name.trim();
    if comparison_name.is_empty() {
        return Err(AppError::BadRequest(
            "comparison name is required".to_string(),
        ));
    }

    let normalized_tickers: Vec<String> = payload
        .tickers
        .iter()
        .map(|t| normalize_ticker(t))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = HashSet::new();
    let tickers: Vec<String> = normalized_tickers
        .into_iter()
        .filter(|ticker| seen.insert(ticker.clone()))
        .collect();

    if tickers.len() < 2 {
        return Err(AppError::BadRequest(
            "at least two unique tickers are required".to_string(),
        ));
    }

    // Use custom question, or selected template, or fallback default.
    let question_template = if let Some(question) = payload
        .question
        .as_deref()
        .map(str::trim)
        .filter(|question| !question.is_empty())
    {
        sanitize_question(question)
    } else if let Some(template_id) = payload
        .template_id
        .as_deref()
        .map(str::trim)
        .filter(|template_id| !template_id.is_empty())
    {
        let template = state
            .db
            .get_run_template(template_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::BadRequest("template_id was not found".to_string()))?;
        template.question_template
    } else {
        "Compare these stocks across valuation, growth, and risk factors".to_string()
    };

    // Create the comparison record
    let comparison = state
        .db
        .create_comparison(comparison_name, &question_template)
        .await
        .map_err(AppError::from)?;

    // Create individual runs for each ticker and link them to the comparison
    for (idx, ticker) in tickers.iter().enumerate() {
        let ticker_question = render_question_for_ticker(&question_template, ticker);
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
) -> AppResult<AxumJson<Vec<Comparison>>> {
    let comparisons = state
        .db
        .list_comparisons(25)
        .await
        .map_err(AppError::from)?;
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
