use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    models::ThesisAlert,
};
use axum::{
    extract::{Path, Query, State},
    response::Json as AxumJson,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AlertQuery {
    pub watchlist_id: String,
    pub status: Option<String>,
}

pub async fn list_alerts(
    State(state): State<AppState>,
    Query(query): Query<AlertQuery>,
) -> AppResult<AxumJson<Vec<ThesisAlert>>> {
    let watchlist_id = query.watchlist_id.trim();
    if watchlist_id.is_empty() {
        return Err(AppError::BadRequest("watchlist_id is required".to_string()));
    }
    ensure_watchlist_exists(&state, watchlist_id).await?;

    let status = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|status| !status.is_empty());
    let alerts = state
        .db
        .list_thesis_alerts(watchlist_id, status)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(alerts))
}

pub async fn dismiss_alert(
    Path(alert_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let updated = state
        .db
        .update_thesis_alert_status(&alert_id, "dismissed")
        .await
        .map_err(AppError::from)?;
    if !updated {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({
        "updated": true,
        "status": "dismissed"
    })))
}

pub async fn snooze_alert(
    Path(alert_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let updated = state
        .db
        .update_thesis_alert_status(&alert_id, "snoozed")
        .await
        .map_err(AppError::from)?;
    if !updated {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({
        "updated": true,
        "status": "snoozed"
    })))
}

async fn ensure_watchlist_exists(state: &AppState, watchlist_id: &str) -> AppResult<()> {
    if state
        .db
        .get_watchlist(watchlist_id)
        .await
        .map_err(AppError::from)?
        .is_none()
    {
        return Err(AppError::NotFound);
    }
    Ok(())
}
