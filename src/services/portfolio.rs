use crate::{
    app_state::AppState,
    models::{PortfolioDetail, PortfolioSummary, PositionWithDetails},
};
use anyhow::Result;
use std::collections::HashMap;

pub async fn build_portfolio_detail(
    state: &AppState,
    portfolio_id: &str,
) -> Result<PortfolioDetail> {
    let portfolio = state
        .db
        .get_portfolio(portfolio_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("portfolio not found: {}", portfolio_id))?;

    let positions = state.db.list_active_positions(portfolio_id).await?;
    let mut positions_with_details = Vec::new();

    // Collect all tickers for batch price fetching
    let tickers: Vec<&str> = positions.iter().map(|p| p.ticker.as_str()).collect();

    // Fetch current prices
    let mut prices: HashMap<String, f64> = HashMap::new();
    for ticker in &tickers {
        if let Ok(price_data) = state.price_provider.get_current_price(ticker).await {
            prices.insert(ticker.to_uppercase(), price_data.close);
        }
    }

    // Calculate total market value for allocation percentages
    let mut total_market_value = 0.0;
    let mut position_values: Vec<(&crate::models::Position, Option<f64>, Option<f64>)> = Vec::new();

    for position in &positions {
        let current_price = prices.get(&position.ticker).copied();
        let market_value = current_price.map(|p| p * position.shares);
        if let Some(mv) = market_value {
            total_market_value += mv;
        }
        position_values.push((position, current_price, market_value));
    }

    // Build position details with conviction alignment
    for (position, current_price, market_value) in position_values {
        let gain_loss = market_value.map(|mv| mv - position.total_cost);
        let gain_loss_pct = gain_loss.and_then(|gl| {
            if position.total_cost.abs() > 0.0 {
                Some(gl / position.total_cost * 100.0)
            } else {
                None
            }
        });

        let allocation_pct = if total_market_value > 0.0 {
            market_value.map(|mv| mv / total_market_value * 100.0)
        } else {
            None
        };

        // Get latest thesis conviction for this ticker
        let recent_runs = state.db.list_runs_for_ticker(&position.ticker, 1).await?;
        let latest_run = recent_runs.first();

        let (latest_conviction, latest_run_id, latest_run_status) = if let Some(run) = latest_run {
            let score = state
                .db
                .get_latest_iteration_evaluation_score(&run.id)
                .await?;
            (score, Some(run.id.clone()), Some(run.status.clone()))
        } else {
            (None, None, None)
        };

        let conviction_alignment =
            classify_conviction_alignment(latest_conviction, position.is_active);

        positions_with_details.push(PositionWithDetails {
            position: position.clone(),
            current_price,
            market_value,
            gain_loss,
            gain_loss_pct,
            allocation_pct,
            latest_conviction,
            conviction_alignment,
            latest_run_id,
            latest_run_status,
        });
    }

    let total_cost: f64 = positions.iter().map(|p| p.total_cost).sum();
    let total_gain_loss = total_market_value - total_cost;
    let total_gain_loss_pct = if total_cost > 0.0 {
        total_gain_loss / total_cost * 100.0
    } else {
        0.0
    };

    let summary = PortfolioSummary {
        total_market_value,
        total_cost,
        total_gain_loss,
        total_gain_loss_pct,
        cash_balance: portfolio.cash_balance,
        total_value: total_market_value + portfolio.cash_balance,
    };

    let recent_transactions = state.db.list_transactions(portfolio_id, 20).await?;

    Ok(PortfolioDetail {
        portfolio,
        positions: positions_with_details,
        summary,
        recent_transactions,
    })
}

fn classify_conviction_alignment(conviction: Option<f64>, is_active: bool) -> String {
    if !is_active {
        return "closed".to_string();
    }

    match conviction {
        Some(score) if score >= 7.0 => "aligned".to_string(),
        Some(score) if score >= 6.0 => "moderate".to_string(),
        Some(score) if score >= 5.0 => "mismatch".to_string(),
        Some(_) => "low_conviction".to_string(),
        None => "no_thesis".to_string(),
    }
}

pub async fn evaluate_portfolio_conviction(state: &AppState, portfolio_id: &str) -> Result<()> {
    let positions = state.db.list_active_positions(portfolio_id).await?;

    for position in positions {
        let recent_runs = state.db.list_runs_for_ticker(&position.ticker, 1).await?;
        if let Some(run) = recent_runs.first() {
            let conviction = state
                .db
                .get_latest_iteration_evaluation_score(&run.id)
                .await?;

            // Create alert if conviction is low but position is still held
            if let Some(score) = conviction {
                if score < 6.0 {
                    let severity = if score < 5.0 { "critical" } else { "warning" };
                    let message = format!(
                        "Position {} has low thesis conviction ({:.1}/10). Consider reviewing or closing.",
                        position.ticker, score
                    );

                    // Check if there's a watchlist associated with this ticker
                    let watchlist_ids = state
                        .db
                        .list_watchlist_ids_for_ticker(&position.ticker)
                        .await?;
                    for watchlist_id in watchlist_ids {
                        state
                            .db
                            .create_thesis_alert_if_absent(
                                &watchlist_id,
                                &position.ticker,
                                &run.id,
                                "conviction_mismatch",
                                severity,
                                &message,
                            )
                            .await?;
                    }
                }
            }
        }
    }

    Ok(())
}
