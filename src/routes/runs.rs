use crate::{
    app_state::AppState,
    config::default_question_for_ticker,
    error::AppError,
    models::{CreateRunRequest, CreateRunResponse, FinalMemoResponse, IterationSummary},
    services::{batch, comparison, orchestrator, related_tickers},
    status::RunStatus,
    utils::{normalize_ticker, render_question_for_ticker, sanitize_question, AppResult},
};
use axum::{
    extract::{Path, State},
    Json,
};
use tracing::error;

pub async fn create_run(
    State(state): State<AppState>,
    Json(payload): Json<CreateRunRequest>,
) -> AppResult<Json<CreateRunResponse>> {
    let ticker = normalize_ticker(&payload.ticker)?;
    let question = if let Some(question) = payload
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
        render_question_for_ticker(&template.question_template, &ticker)
    } else {
        default_question_for_ticker(&ticker)
    };

    let run = state
        .db
        .create_run(&ticker, &question)
        .await
        .map_err(AppError::from)?;
    state
        .db
        .insert_event(&run.id, None, "queued", "Run queued", None)
        .await
        .map_err(AppError::from)?;

    spawn_run(state.clone(), run.id.clone());

    Ok(Json(CreateRunResponse {
        run_id: run.id,
        status: "queued".to_string(),
    }))
}

pub async fn cancel_run(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    let run = state
        .db
        .get_run(&run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let parsed_status = RunStatus::parse(&run.status);
    if matches!(
        parsed_status,
        Some(RunStatus::Completed | RunStatus::Failed)
    ) {
        return Err(AppError::BadRequest(
            "completed or failed runs cannot be cancelled".to_string(),
        ));
    }

    if parsed_status != Some(RunStatus::Cancelled) {
        // Flip the in-process cancellation flag so the orchestrator notices
        // at its next checkpoint without waiting on a DB poll.
        state.cancellation.cancel(&run_id);
        state
            .db
            .set_run_status(&run_id, RunStatus::Cancelled.as_str())
            .await
            .map_err(AppError::from)?;
        state
            .db
            .insert_event(
                &run_id,
                None,
                "run_cancelled",
                "Run cancelled by user",
                None,
            )
            .await
            .map_err(AppError::from)?;
    }

    let _ = comparison::sync_comparisons_for_run(&state, &run_id).await;
    let _ = batch::sync_batch_jobs_for_run(&state, &run_id).await;

    Ok(Json(serde_json::json!({
        "run_id": run_id,
        "status": "cancelled",
    })))
}

pub async fn retry_run(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<CreateRunResponse>> {
    let run = state
        .db
        .get_run(&run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    if !matches!(
        RunStatus::parse(&run.status),
        Some(RunStatus::Failed | RunStatus::Cancelled)
    ) {
        return Err(AppError::BadRequest(
            "only failed or cancelled runs can be retried".to_string(),
        ));
    }

    state
        .db
        .reset_run_for_retry(&run_id)
        .await
        .map_err(AppError::from)?;
    state
        .db
        .insert_event(&run_id, None, "queued", "Run queued for retry", None)
        .await
        .map_err(AppError::from)?;

    let _ = comparison::sync_comparisons_for_run(&state, &run_id).await;
    let _ = batch::sync_batch_jobs_for_run(&state, &run_id).await;
    spawn_run(state.clone(), run_id.clone());

    Ok(Json(CreateRunResponse {
        run_id,
        status: "queued".to_string(),
    }))
}

pub async fn list_runs(State(state): State<AppState>) -> AppResult<Json<Vec<crate::models::Run>>> {
    let runs = state.db.list_runs(25).await.map_err(AppError::from)?;
    Ok(Json(runs))
}

pub async fn get_run(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<crate::models::Run>> {
    let run = state
        .db
        .get_run(&run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    Ok(Json(run))
}

pub async fn get_events(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<crate::models::EventRecord>>> {
    ensure_run_exists(&state, &run_id).await?;
    let events = state
        .db
        .list_events(&run_id)
        .await
        .map_err(AppError::from)?;
    Ok(Json(events))
}

pub async fn list_iterations(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<IterationSummary>>> {
    ensure_run_exists(&state, &run_id).await?;
    let iterations = state
        .db
        .list_iterations(&run_id)
        .await
        .map_err(AppError::from)?;
    let summaries = iterations
        .iter()
        .map(IterationSummary::from_iteration)
        .collect();
    Ok(Json(summaries))
}

pub async fn get_iteration(
    Path((run_id, iteration_number)): Path<(String, i64)>,
    State(state): State<AppState>,
) -> AppResult<Json<crate::models::IterationDetail>> {
    let detail = state
        .db
        .get_iteration_detail(&run_id, iteration_number)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    Ok(Json(detail))
}

pub async fn get_final(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<FinalMemoResponse>> {
    let run = state
        .db
        .get_run(&run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    Ok(Json(FinalMemoResponse {
        run_id: run.id,
        status: run.status,
        final_iteration_number: run.final_iteration_number,
        final_memo_markdown: run.final_memo_markdown,
        final_memo_html: run.final_memo_html,
        summary: run.summary,
    }))
}

async fn ensure_run_exists(state: &AppState, run_id: &str) -> AppResult<()> {
    if state
        .db
        .get_run(run_id)
        .await
        .map_err(AppError::from)?
        .is_none()
    {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn spawn_run(state: AppState, run_id: String) {
    tokio::spawn(async move {
        if let Err(error) = orchestrator::execute_run(state, run_id.clone()).await {
            error!(%run_id, error = %error, "background orchestrator failed");
        }
    });
}

pub async fn get_related_tickers(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Json<crate::models::RelatedTickersResponse>> {
    ensure_run_exists(&state, &run_id).await?;
    let response = related_tickers::discover_related_tickers(&state, &run_id)
        .await
        .map_err(AppError::from)?;
    Ok(Json(response))
}
