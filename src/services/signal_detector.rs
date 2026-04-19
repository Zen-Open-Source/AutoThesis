use crate::{app_state::AppState, models::ScanSignal};
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SignalDetectorOutput {
    pub signals: Vec<ScanSignal>,
}

/// Detect trading signals for a ticker by searching for recent news and analysis.
pub async fn detect_signals(state: &AppState, ticker: &str) -> Result<Vec<ScanSignal>> {
    let query = format!(
        "{} stock catalyst earnings news analyst upgrade downgrade insider trading valuation 2024 2025",
        ticker
    );

    let search_results = state.search.search(&query, 10).await?;

    let prompt = state.prompts.get("scanner_signal")?;
    let payload = serde_json::json!({
        "ticker": ticker,
        "search_results": search_results.iter().map(|r| serde_json::json!({
            "title": r.title,
            "url": r.url,
            "snippet": r.snippet,
        })).collect::<Vec<_>>(),
    });

    match state
        .llm
        .complete_json(
            "scanner_signal",
            prompt,
            &serde_json::to_string_pretty(&payload)?,
        )
        .await
    {
        Ok(value) => {
            let output: SignalDetectorOutput = serde_json::from_value(value)?;
            Ok(output.signals)
        }
        Err(_) => Ok(fallback_signals(ticker, &search_results)),
    }
}

fn fallback_signals(
    ticker: &str,
    search_results: &[crate::providers::search::SearchResultItem],
) -> Vec<ScanSignal> {
    if search_results.is_empty() {
        return vec![ScanSignal {
            signal_type: "coverage".to_string(),
            strength: 0.1,
            description: format!("No recent news found for {}", ticker),
            evidence: vec![],
        }];
    }

    vec![ScanSignal {
        signal_type: "coverage".to_string(),
        strength: 0.3,
        description: format!(
            "{} recent news articles found for {}",
            search_results.len(),
            ticker
        ),
        evidence: search_results
            .iter()
            .take(3)
            .filter_map(|r| r.title.clone())
            .collect(),
    }]
}

/// Calculate signal strength score from detected signals.
pub fn calculate_signal_strength(signals: &[ScanSignal]) -> f64 {
    if signals.is_empty() {
        return 0.0;
    }

    let weights: std::collections::HashMap<&str, f64> = [
        ("earnings_catalyst", 1.5),
        ("news_spike", 1.2),
        ("analyst_activity", 1.3),
        ("valuation_anomaly", 1.0),
        ("sector_momentum", 0.8),
        ("insider_activity", 1.1),
        ("coverage", 0.5),
    ]
    .into_iter()
    .collect();

    let total_weight: f64 = signals
        .iter()
        .map(|s| weights.get(s.signal_type.as_str()).copied().unwrap_or(1.0) * s.strength)
        .sum();

    let avg_weight = total_weight / signals.len() as f64;
    (avg_weight * 10.0).clamp(0.0, 10.0)
}

/// Calculate timing score based on signal freshness.
pub fn calculate_timing_score(signals: &[ScanSignal]) -> f64 {
    if signals.is_empty() {
        return 0.0;
    }

    // Assume signals are recent from search results
    // Higher scores for catalysts and analyst activity
    let has_catalyst = signals.iter().any(|s| s.signal_type == "earnings_catalyst");
    let has_analyst = signals.iter().any(|s| s.signal_type == "analyst_activity");
    let has_news = signals.iter().any(|s| s.signal_type == "news_spike");

    let mut score: f64 = 5.0;
    if has_catalyst {
        score += 2.0;
    }
    if has_analyst {
        score += 1.5;
    }
    if has_news {
        score += 1.0;
    }

    score.min(10.0)
}
