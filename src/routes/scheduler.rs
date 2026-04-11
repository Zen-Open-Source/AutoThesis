use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    models::{UpdateWatchlistScheduleRequest, WatchlistScheduleResponse},
    services::scheduler,
};
use axum::{
    extract::{Path, State},
    response::Json as AxumJson,
};
use serde_json::json;

pub async fn get_watchlist_schedule(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<WatchlistScheduleResponse>> {
    let watchlist = state
        .db
        .get_watchlist(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let schedule = state
        .db
        .get_watchlist_schedule(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .unwrap_or_else(|| crate::models::WatchlistSchedule {
            watchlist_id: watchlist_id.clone(),
            refresh_enabled: false,
            refresh_interval_hours: 168,
            last_refresh_at: None,
            next_refresh_at: None,
            refresh_template_id: None,
        });

    let scheduled_runs = state
        .db
        .list_scheduled_runs(&watchlist_id, 20)
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(WatchlistScheduleResponse {
        watchlist,
        schedule,
        scheduled_runs,
    }))
}

pub async fn update_watchlist_schedule(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<UpdateWatchlistScheduleRequest>,
) -> AppResult<AxumJson<WatchlistScheduleResponse>> {
    // Verify watchlist exists
    let watchlist = state
        .db
        .get_watchlist(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    // Validate template if provided
    if let Some(template_id) = &payload.template_id {
        let template = state
            .db
            .get_run_template(template_id)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| AppError::BadRequest("template not found".to_string()))?;
        if template.id.is_empty() {
            return Err(AppError::BadRequest("invalid template".to_string()));
        }
    }

    // Validate interval
    if payload.interval_hours < 1 {
        return Err(AppError::BadRequest(
            "interval must be at least 1 hour".to_string(),
        ));
    }

    state
        .db
        .update_watchlist_schedule(
            &watchlist_id,
            payload.enabled,
            payload.interval_hours,
            payload.template_id.as_deref(),
        )
        .await
        .map_err(AppError::from)?;

    let schedule = state
        .db
        .get_watchlist_schedule(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("schedule not found after update")))?;

    let scheduled_runs = state
        .db
        .list_scheduled_runs(&watchlist_id, 20)
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(WatchlistScheduleResponse {
        watchlist,
        schedule,
        scheduled_runs,
    }))
}

pub async fn pause_watchlist_schedule(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    // Verify watchlist exists
    let _watchlist = state
        .db
        .get_watchlist(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let schedule = state
        .db
        .get_watchlist_schedule(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::BadRequest("schedule not configured".to_string()))?;

    state
        .db
        .update_watchlist_schedule(
            &watchlist_id,
            false,
            schedule.refresh_interval_hours,
            schedule.refresh_template_id.as_deref(),
        )
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(json!({ "paused": true })))
}

pub async fn resume_watchlist_schedule(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    // Verify watchlist exists
    let _watchlist = state
        .db
        .get_watchlist(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let schedule = state
        .db
        .get_watchlist_schedule(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .unwrap_or_else(|| crate::models::WatchlistSchedule {
            watchlist_id: watchlist_id.clone(),
            refresh_enabled: false,
            refresh_interval_hours: 168,
            last_refresh_at: None,
            next_refresh_at: None,
            refresh_template_id: None,
        });

    state
        .db
        .update_watchlist_schedule(
            &watchlist_id,
            true,
            schedule.refresh_interval_hours,
            schedule.refresh_template_id.as_deref(),
        )
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(json!({ "resumed": true })))
}

pub async fn trigger_refresh(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    // Verify watchlist exists
    let _watchlist = state
        .db
        .get_watchlist(&watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let spawned = scheduler::trigger_watchlist_refresh(&state, &watchlist_id)
        .await
        .map_err(AppError::from)?;

    Ok(AxumJson(json!({
        "triggered": true,
        "runs_spawned": spawned
    })))
}
