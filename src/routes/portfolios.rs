use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    models::{
        ClosePositionRequest, CreatePortfolioRequest, CreatePositionRequest, Portfolio,
        PortfolioDetail, UpdatePortfolioRequest,
    },
    services::portfolio,
};
use axum::{
    extract::{Path, State},
    response::Json as AxumJson,
};
use chrono::Utc;

pub async fn list_portfolios(State(state): State<AppState>) -> AppResult<AxumJson<Vec<Portfolio>>> {
    let portfolios = state
        .db
        .list_portfolios(100)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(portfolios))
}

pub async fn create_portfolio(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreatePortfolioRequest>,
) -> AppResult<AxumJson<Portfolio>> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "portfolio name is required".to_string(),
        ));
    }

    let portfolio = state
        .db
        .create_portfolio(
            name,
            payload.description.as_deref(),
            payload.cash_balance.unwrap_or(0.0),
        )
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(portfolio))
}

pub async fn get_portfolio(
    Path(portfolio_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<PortfolioDetail>> {
    let detail = portfolio::build_portfolio_detail(&state, &portfolio_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(detail))
}

pub async fn update_portfolio(
    Path(portfolio_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<UpdatePortfolioRequest>,
) -> AppResult<AxumJson<PortfolioDetail>> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "portfolio name is required".to_string(),
        ));
    }

    let updated = state
        .db
        .update_portfolio(
            &portfolio_id,
            name,
            payload.description.as_deref(),
            payload.cash_balance,
        )
        .await
        .map_err(AppError::from)?;
    if !updated {
        return Err(AppError::NotFound);
    }

    let detail = portfolio::build_portfolio_detail(&state, &portfolio_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(detail))
}

pub async fn delete_portfolio(
    Path(portfolio_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let deleted = state
        .db
        .delete_portfolio(&portfolio_id)
        .await
        .map_err(AppError::from)?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(AxumJson(serde_json::json!({ "deleted": true })))
}

pub async fn create_position(
    Path(portfolio_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<CreatePositionRequest>,
) -> AppResult<AxumJson<PortfolioDetail>> {
    let ticker = payload.ticker.trim().to_uppercase();
    if ticker.is_empty() {
        return Err(AppError::BadRequest("ticker is required".to_string()));
    }
    if !ticker
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(AppError::BadRequest(
            "ticker must contain only letters, numbers, '.' or '-'".to_string(),
        ));
    }
    if payload.shares <= 0.0 {
        return Err(AppError::BadRequest("shares must be positive".to_string()));
    }
    if payload.cost_basis_per_share <= 0.0 {
        return Err(AppError::BadRequest(
            "cost_basis_per_share must be positive".to_string(),
        ));
    }

    let opened_at = payload.opened_at.unwrap_or_else(|| Utc::now().date_naive());

    state
        .db
        .create_position(
            &portfolio_id,
            &ticker,
            payload.shares,
            payload.cost_basis_per_share,
            opened_at,
            payload.notes.as_deref(),
        )
        .await
        .map_err(AppError::from)?;

    let detail = portfolio::build_portfolio_detail(&state, &portfolio_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(detail))
}

pub async fn close_position(
    Path(position_id): Path<String>,
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<ClosePositionRequest>,
) -> AppResult<AxumJson<serde_json::Value>> {
    let position = state
        .db
        .get_position(&position_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    if !position.is_active {
        return Err(AppError::BadRequest(
            "position is already closed".to_string(),
        ));
    }

    let closed = state
        .db
        .close_position(&position_id, payload.closed_at, payload.notes.as_deref())
        .await
        .map_err(AppError::from)?;
    if !closed {
        return Err(AppError::NotFound);
    }

    // Create a sell transaction
    if let Ok(price_data) = state
        .price_provider
        .get_current_price(&position.ticker)
        .await
    {
        let _ = state
            .db
            .create_transaction(
                &position.portfolio_id,
                &position.ticker,
                "sell",
                position.shares,
                price_data.close,
                payload.closed_at,
                None,
            )
            .await;
    }

    Ok(AxumJson(serde_json::json!({ "closed": true })))
}

pub async fn evaluate_conviction(
    Path(portfolio_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<AxumJson<serde_json::Value>> {
    portfolio::evaluate_portfolio_conviction(&state, &portfolio_id)
        .await
        .map_err(AppError::from)?;
    Ok(AxumJson(serde_json::json!({ "evaluated": true })))
}
