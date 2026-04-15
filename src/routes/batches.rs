use crate::{
    app_state::AppState,
    config::default_question_for_ticker,
    error::AppError,
    models::{BatchJob, BatchJobDetail, CreateBatchJobRequest, CreateBatchJobResponse},
    services::orchestrator,
    utils::{normalize_ticker, render_question_for_ticker, AppResult},
};
use axum::{
    extract::{Path, State},
    response::Json as AxumJson,
};
use std::collections::HashSet;
use tracing::error;

pub async fn create_batch_job(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateBatchJobRequest>,
) -> AppResult<AxumJson<CreateBatchJobResponse>> {
    let batch_name = payload.name.trim();
    if batch_name.is_empty() {
        return Err(AppError::BadRequest("batch name is required".to_string()));
    }

    let normalized_tickers: Vec<String> = payload
        .tickers
        .iter()
        .map(|ticker| normalize_ticker(ticker))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = HashSet::new();
    let tickers: Vec<String> = normalized_tickers
        .into_iter()
        .filter(|ticker| seen.insert(ticker.clone()))
        .collect();
    if tickers.is_empty() {
        return Err(AppError::BadRequest(
            "at least one unique ticker is required".to_string(),
        ));
    }

    let question_template = resolve_question_template(&state, &payload).await?;
    let batch_job = state
        .db
        .create_batch_job(batch_name, &question_template)
        .await
        .map_err(AppError::from)?;

    for (index, ticker) in tickers.iter().enumerate() {
        let ticker_question = render_question_for_ticker(&question_template, ticker);
        let run = state
            .db
            .create_run(ticker, &ticker_question)
            .await
            .map_err(AppError::from)?;
        state
            .db
            .insert_event(&run.id, None, "queued", "Run queued", None)
            .await
            .map_err(AppError::from)?;
        state
            .db
            .add_run_to_batch_job(&batch_job.id, &run.id, ticker, index as i64)
            .await
            .map_err(AppError::from)?;

        let state_for_task = state.clone();
        let run_id = run.id.clone();
        tokio::spawn(async move {
            if let Err(error) = orchestrator::execute_run(state_for_task, run_id.clone()).await {
                error!(%run_id, error = %error, "background orchestrator failed");
            }
        });
    }

    state
        .db
        .update_batch_job_status(&batch_job.id, "queued")
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(CreateBatchJobResponse {
        batch_job_id: batch_job.id,
        status: "queued".to_string(),
    }))
}

pub async fn list_batch_jobs(State(state): State<AppState>) -> AppResult<AxumJson<Vec<BatchJob>>> {
    let batch_jobs = state.db.list_batch_jobs(25).await.map_err(AppError::from)?;
    Ok(AxumJson(batch_jobs))
}

pub async fn get_batch_job(
    Path(batch_job_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<BatchJobDetail>> {
    let batch_job = state
        .db
        .get_batch_job(&batch_job_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    let batch_job_runs = state
        .db
        .list_batch_job_runs(&batch_job_id)
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(BatchJobDetail {
        batch_job,
        batch_job_runs,
    }))
}

async fn resolve_question_template(
    state: &AppState,
    payload: &CreateBatchJobRequest,
) -> AppResult<String> {
    if let Some(question_template) = payload
        .question_template
        .as_deref()
        .map(str::trim)
        .filter(|question_template| !question_template.is_empty())
    {
        return Ok(question_template.to_string());
    }

    if let Some(template_id) = payload
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
        return Ok(template.question_template);
    }

    Ok(default_question_for_ticker("{ticker}"))
}
