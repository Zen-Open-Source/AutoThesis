use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    models::{Bookmark, CreateBookmarkRequest, CreateSourceAnnotationRequest, SourceAnnotation},
};
use axum::{
    extract::{Path, Query, State},
    response::Json as AxumJson,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DeleteBookmarkQuery {
    pub entity_type: String,
    pub entity_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteSourceAnnotationQuery {
    pub annotation_id: String,
}

pub async fn list_bookmarks(State(state): State<AppState>) -> AppResult<AxumJson<Vec<Bookmark>>> {
    let bookmarks = state.db.list_bookmarks(200).await.map_err(AppError::from)?;
    Ok(AxumJson(bookmarks))
}

pub async fn upsert_bookmark(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateBookmarkRequest>,
) -> AppResult<AxumJson<Bookmark>> {
    let entity_type = normalize_bookmark_entity_type(&payload.entity_type)?;
    let entity_id = payload.entity_id.trim();
    let title = payload.title.trim();
    let target_path = payload.target_path.trim();
    let note = payload
        .note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty());

    if entity_id.is_empty() {
        return Err(AppError::BadRequest("entity_id is required".to_string()));
    }
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".to_string()));
    }
    if target_path.is_empty() {
        return Err(AppError::BadRequest("target_path is required".to_string()));
    }

    let bookmark = state
        .db
        .upsert_bookmark(entity_type, entity_id, title, note, target_path)
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(bookmark))
}

pub async fn delete_bookmark(
    State(state): State<AppState>,
    Query(query): Query<DeleteBookmarkQuery>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let entity_type = normalize_bookmark_entity_type(&query.entity_type)?;
    let entity_id = query.entity_id.trim();
    if entity_id.is_empty() {
        return Err(AppError::BadRequest("entity_id is required".to_string()));
    }

    let deleted = state
        .db
        .delete_bookmark(entity_type, entity_id)
        .await
        .map_err(AppError::from)?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({ "deleted": true })))
}

pub async fn list_source_annotations(
    Path(source_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<Vec<SourceAnnotation>>> {
    let annotations = state
        .db
        .list_source_annotations(&source_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(annotations))
}

pub async fn create_source_annotation(
    Path(source_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateSourceAnnotationRequest>,
) -> AppResult<AxumJson<SourceAnnotation>> {
    let run_id = payload.run_id.trim();
    let selected_text = payload.selected_text.trim();
    let annotation_markdown = payload.annotation_markdown.trim();
    let tag = payload
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty());

    if run_id.is_empty() {
        return Err(AppError::BadRequest("run_id is required".to_string()));
    }
    if selected_text.is_empty() {
        return Err(AppError::BadRequest(
            "selected_text is required".to_string(),
        ));
    }
    if annotation_markdown.is_empty() {
        return Err(AppError::BadRequest(
            "annotation_markdown is required".to_string(),
        ));
    }

    let annotation = state
        .db
        .create_source_annotation(&source_id, run_id, selected_text, annotation_markdown, tag)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(annotation))
}

pub async fn delete_source_annotation(
    Path(source_id): Path<String>,
    State(state): State<AppState>,
    Query(query): Query<DeleteSourceAnnotationQuery>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let annotation_id = query.annotation_id.trim();
    if annotation_id.is_empty() {
        return Err(AppError::BadRequest(
            "annotation_id is required".to_string(),
        ));
    }
    let deleted = state
        .db
        .delete_source_annotation(&source_id, annotation_id)
        .await
        .map_err(AppError::from)?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({ "deleted": true })))
}

fn normalize_bookmark_entity_type(raw: &str) -> AppResult<&str> {
    let normalized = raw.trim().to_lowercase();
    match normalized.as_str() {
        "run" => Ok("run"),
        "comparison" => Ok("comparison"),
        "source" => Ok("source"),
        _ => Err(AppError::BadRequest(
            "entity_type must be run, comparison, or source".to_string(),
        )),
    }
}
