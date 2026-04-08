use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    models::{CreateRunTemplateRequest, RunTemplate, UpdateRunTemplateRequest},
};
use axum::{
    extract::{Path, State},
    response::Json as AxumJson,
};

pub async fn list_run_templates(
    State(state): State<AppState>,
) -> AppResult<AxumJson<Vec<RunTemplate>>> {
    let templates = state
        .db
        .list_run_templates(200)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(templates))
}

pub async fn create_run_template(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateRunTemplateRequest>,
) -> AppResult<AxumJson<RunTemplate>> {
    let name = payload.name.trim();
    let question_template = payload.question_template.trim();
    let description = payload
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty());

    if name.is_empty() {
        return Err(AppError::BadRequest(
            "template name is required".to_string(),
        ));
    }
    if question_template.is_empty() {
        return Err(AppError::BadRequest(
            "question_template is required".to_string(),
        ));
    }

    let template = state
        .db
        .create_run_template(name, question_template, description)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(template))
}

pub async fn update_run_template(
    Path(template_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<UpdateRunTemplateRequest>,
) -> AppResult<AxumJson<RunTemplate>> {
    let name = payload.name.trim();
    let question_template = payload.question_template.trim();
    let description = payload
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty());

    if name.is_empty() {
        return Err(AppError::BadRequest(
            "template name is required".to_string(),
        ));
    }
    if question_template.is_empty() {
        return Err(AppError::BadRequest(
            "question_template is required".to_string(),
        ));
    }

    let updated = state
        .db
        .update_run_template(&template_id, name, question_template, description)
        .await
        .map_err(AppError::from)?;
    if !updated {
        return Err(AppError::NotFound);
    }
    let template = state
        .db
        .get_run_template(&template_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    Ok(AxumJson(template))
}

pub async fn delete_run_template(
    Path(template_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let deleted = state
        .db
        .delete_run_template(&template_id)
        .await
        .map_err(AppError::from)?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({ "deleted": true })))
}
