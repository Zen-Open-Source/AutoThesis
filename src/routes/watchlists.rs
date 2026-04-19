use crate::{
    app_state::AppState,
    config::default_question_for_ticker,
    error::AppError,
    models::{
        AddWatchlistTickerRequest, CreateRunResponse, CreateWatchlistRequest,
        DashboardRefreshRequest, DashboardResponse, UpdateWatchlistRequest, Watchlist,
        WatchlistDetail,
    },
    services::{dashboard, orchestrator},
    utils::{
        normalize_ticker, normalize_tickers, render_question_for_ticker, sanitize_question,
        AppResult,
    },
};
use axum::{
    extract::{Path, Query, State},
    response::Json as AxumJson,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    pub watchlist_id: Option<String>,
}

pub async fn list_watchlists(State(state): State<AppState>) -> AppResult<AxumJson<Vec<Watchlist>>> {
    let watchlists = state
        .db
        .list_watchlists(200)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(watchlists))
}

pub async fn create_watchlist(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreateWatchlistRequest>,
) -> AppResult<AxumJson<WatchlistDetail>> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "watchlist name is required".to_string(),
        ));
    }

    let watchlist = state
        .db
        .create_watchlist(name)
        .await
        .map_err(AppError::from)?;
    let tickers = normalize_tickers(payload.tickers.unwrap_or_default())?;
    state
        .db
        .replace_watchlist_tickers(&watchlist.id, &tickers)
        .await
        .map_err(AppError::from)?;
    let detail = fetch_watchlist_detail(&state, &watchlist.id).await?;
    Ok(AxumJson(detail))
}

pub async fn get_watchlist(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<WatchlistDetail>> {
    let detail = fetch_watchlist_detail(&state, &watchlist_id).await?;
    Ok(AxumJson(detail))
}

pub async fn update_watchlist(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<UpdateWatchlistRequest>,
) -> AppResult<AxumJson<WatchlistDetail>> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "watchlist name is required".to_string(),
        ));
    }

    let updated = state
        .db
        .update_watchlist_name(&watchlist_id, name)
        .await
        .map_err(AppError::from)?;
    if !updated {
        return Err(AppError::NotFound);
    }

    let tickers = normalize_tickers(payload.tickers)?;
    state
        .db
        .replace_watchlist_tickers(&watchlist_id, &tickers)
        .await
        .map_err(AppError::from)?;
    let detail = fetch_watchlist_detail(&state, &watchlist_id).await?;
    Ok(AxumJson(detail))
}

pub async fn delete_watchlist(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let deleted = state
        .db
        .delete_watchlist(&watchlist_id)
        .await
        .map_err(AppError::from)?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({ "deleted": true })))
}

pub async fn add_watchlist_ticker(
    Path(watchlist_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<AddWatchlistTickerRequest>,
) -> AppResult<AxumJson<WatchlistDetail>> {
    ensure_watchlist_exists(&state, &watchlist_id).await?;
    let ticker = normalize_ticker(&payload.ticker)?;
    let existing = state
        .db
        .list_watchlist_tickers(&watchlist_id)
        .await
        .map_err(AppError::from)?;
    state
        .db
        .add_ticker_to_watchlist(&watchlist_id, &ticker, existing.len() as i64)
        .await
        .map_err(AppError::from)?;
    let detail = fetch_watchlist_detail(&state, &watchlist_id).await?;
    Ok(AxumJson(detail))
}

pub async fn remove_watchlist_ticker(
    Path((watchlist_id, ticker)): Path<(String, String)>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<WatchlistDetail>> {
    ensure_watchlist_exists(&state, &watchlist_id).await?;
    let ticker = normalize_ticker(&ticker)?;
    let removed = state
        .db
        .remove_ticker_from_watchlist(&watchlist_id, &ticker)
        .await
        .map_err(AppError::from)?;
    if !removed {
        return Err(AppError::NotFound);
    }

    // Re-sequence sort_order after removal.
    let remaining = state
        .db
        .list_watchlist_tickers(&watchlist_id)
        .await
        .map_err(AppError::from)?;
    let reordered = remaining
        .iter()
        .map(|item| item.ticker.clone())
        .collect::<Vec<_>>();
    state
        .db
        .replace_watchlist_tickers(&watchlist_id, &reordered)
        .await
        .map_err(AppError::from)?;

    let detail = fetch_watchlist_detail(&state, &watchlist_id).await?;
    Ok(AxumJson(detail))
}

pub async fn get_dashboard(
    State(state): State<AppState>,
    Query(query): Query<DashboardQuery>,
) -> AppResult<AxumJson<DashboardResponse>> {
    let watchlist_id = if let Some(watchlist_id) = query
        .watchlist_id
        .as_deref()
        .map(str::trim)
        .filter(|watchlist_id| !watchlist_id.is_empty())
    {
        watchlist_id.to_string()
    } else {
        let watchlists = state.db.list_watchlists(1).await.map_err(AppError::from)?;
        let first = watchlists
            .first()
            .ok_or_else(|| AppError::BadRequest("no watchlists exist yet".to_string()))?;
        first.id.clone()
    };

    ensure_watchlist_exists(&state, &watchlist_id).await?;
    let response = dashboard::build_watchlist_dashboard(&state, &watchlist_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(response))
}

pub async fn refresh_dashboard_ticker(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<DashboardRefreshRequest>,
) -> AppResult<AxumJson<CreateRunResponse>> {
    let watchlist_id = payload.watchlist_id.trim();
    if watchlist_id.is_empty() {
        return Err(AppError::BadRequest("watchlist_id is required".to_string()));
    }
    ensure_watchlist_exists(&state, watchlist_id).await?;
    let ticker = normalize_ticker(&payload.ticker)?;

    let existing_tickers = state
        .db
        .list_watchlist_tickers(watchlist_id)
        .await
        .map_err(AppError::from)?;
    if !existing_tickers.iter().any(|item| item.ticker == ticker) {
        state
            .db
            .add_ticker_to_watchlist(watchlist_id, &ticker, existing_tickers.len() as i64)
            .await
            .map_err(AppError::from)?;
    }

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

    orchestrator::spawn_bounded_run(state, run.id.clone());
    Ok(AxumJson(CreateRunResponse {
        run_id: run.id,
        status: "queued".to_string(),
    }))
}

async fn fetch_watchlist_detail(
    state: &AppState,
    watchlist_id: &str,
) -> AppResult<WatchlistDetail> {
    let watchlist = state
        .db
        .get_watchlist(watchlist_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    let tickers = state
        .db
        .list_watchlist_tickers(watchlist_id)
        .await
        .map_err(AppError::from)?;
    Ok(WatchlistDetail { watchlist, tickers })
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


