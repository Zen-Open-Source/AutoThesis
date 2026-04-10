use crate::{
    app_state::AppState,
    models::{ScanOpportunity, TickerUniverse},
};
use anyhow::Result;

/// Calculate coverage gap score - higher if no existing thesis exists.
pub async fn calculate_coverage_gap_score(state: &AppState, ticker: &str) -> Result<f64> {
    let existing_runs = state.db.list_runs_for_ticker(ticker, 1).await?;

    if existing_runs.is_empty() {
        // No existing thesis - high coverage gap score
        return Ok(10.0);
    }

    // Check if the existing run is stale (older than 30 days)
    if let Some(latest_run) = existing_runs.first() {
        let days_old = (chrono::Utc::now() - latest_run.updated_at).num_days();
        if days_old > 30 {
            return Ok(8.0);
        }
        if days_old > 14 {
            return Ok(5.0);
        }
        return Ok(2.0);
    }

    Ok(0.0)
}

/// Calculate overall opportunity score from component scores.
pub fn calculate_overall_score(
    signal_strength: f64,
    thesis_quality: Option<f64>,
    coverage_gap: f64,
    timing: f64,
) -> f64 {
    let thesis_quality_score = thesis_quality.unwrap_or(5.0);

    // Weighted average of component scores
    let weighted_score = (signal_strength * 0.30)
        + (thesis_quality_score * 0.25)
        + (coverage_gap * 0.25)
        + (timing * 0.20);

    weighted_score.min(10.0).max(0.0)
}

/// Rank opportunities by overall score.
pub fn rank_opportunities(opportunities: &mut [ScanOpportunity]) {
    opportunities.sort_by(|a, b| {
        b.overall_score
            .partial_cmp(&a.overall_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Filter opportunities to top N by score.
pub fn filter_top_opportunities(
    opportunities: Vec<ScanOpportunity>,
    max_count: usize,
) -> Vec<ScanOpportunity> {
    let mut sorted = opportunities;
    rank_opportunities(&mut sorted);
    sorted.into_iter().take(max_count).collect()
}

/// Check if ticker meets minimum criteria for opportunity.
pub fn meets_minimum_criteria(
    signal_strength: f64,
    coverage_gap: f64,
    min_signal_strength: f64,
    min_coverage_gap: f64,
) -> bool {
    signal_strength >= min_signal_strength && coverage_gap >= min_coverage_gap
}

/// Calculate market cap score - preference for mid to large cap.
pub fn calculate_market_cap_score(ticker_info: Option<&TickerUniverse>) -> f64 {
    let Some(info) = ticker_info else {
        return 5.0; // Unknown market cap
    };

    let Some(market_cap) = info.market_cap_billion else {
        return 5.0;
    };

    // Mega cap (>200B): moderate score (well covered)
    // Large cap (50-200B): high score (good liquidity, less coverage)
    // Mid cap (10-50B): highest score (potential alpha)
    // Small cap (<10B): lower score (higher risk, less liquid)

    if market_cap > 200.0 {
        6.0
    } else if market_cap > 50.0 {
        8.0
    } else if market_cap > 10.0 {
        10.0
    } else {
        4.0
    }
}

/// Calculate sector diversification score.
pub fn calculate_sector_score(existing_sectors: &[String], ticker_sector: Option<&str>) -> f64 {
    let Some(sector) = ticker_sector else {
        return 5.0;
    };

    // Higher score for sectors not already in opportunities
    let sector_count = existing_sectors.iter().filter(|s| s.as_str() == sector).count();

    if sector_count == 0 {
        10.0 // New sector - diversification bonus
    } else if sector_count == 1 {
        7.0 // One existing - still good
    } else {
        4.0 // Multiple existing - concentration risk
    }
}
