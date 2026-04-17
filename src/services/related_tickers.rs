use crate::{
    app_state::AppState,
    models::{RelatedTicker, RelatedTickersResponse, TickerUniverse},
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use tracing::info;

/// Discover related tickers for a research run.
pub async fn discover_related_tickers(
    state: &AppState,
    run_id: &str,
) -> Result<RelatedTickersResponse> {
    let run = state
        .db
        .get_run(run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("run not found: {}", run_id))?;

    let primary_ticker = run.ticker.clone();
    info!(%run_id, %primary_ticker, "discovering related tickers");

    // Get primary ticker's info
    let primary_info = state.db.get_ticker_universe(&primary_ticker).await?;

    // Collect all related ticker candidates with their scores
    let mut candidates: HashMap<String, CandidateInfo> = HashMap::new();

    // 1. Same sector tickers (weight: 0.5)
    if let Some(ref info) = primary_info {
        if let Some(ref sector) = info.sector {
            add_sector_matches(state, sector, &primary_ticker, &mut candidates).await?;
        }
    }

    // 2. Mentions in sources (weight: 0.3)
    add_source_mentions(state, run_id, &primary_ticker, &mut candidates).await?;

    // 3. Mentions in thesis (weight: 0.2)
    if let Some(ref memo) = run.final_memo_markdown {
        add_thesis_mentions(memo, &primary_ticker, &mut candidates);
    }

    // Batch-fetch universe + latest-run data to avoid N+1 DB calls.
    let candidate_tickers: Vec<String> = candidates.keys().cloned().collect();
    let universe_map = state
        .db
        .get_ticker_universe_batch(&candidate_tickers)
        .await?;
    let latest_runs = state.db.latest_runs_for_tickers(&candidate_tickers).await?;

    // Convert candidates to RelatedTicker and enrich with research status
    let mut related_tickers: Vec<RelatedTicker> = Vec::new();
    for (ticker, candidate) in candidates.iter() {
        let ticker_info = universe_map.get(ticker).cloned();
        let latest_run = latest_runs.get(ticker).cloned();

        let latest_conviction = if let Some(run) = &latest_run {
            state
                .db
                .get_latest_iteration_evaluation_score(&run.id)
                .await?
        } else {
            None
        };

        let context = candidate.context.clone().or_else(|| {
            ticker_info.as_ref().and_then(|info| {
                if let (Some(sector), Some(primary_sector)) = (
                    info.sector.as_ref(),
                    primary_info.as_ref().and_then(|p| p.sector.as_ref()),
                ) {
                    if sector == primary_sector {
                        Some(format!("Same sector: {sector}"))
                    } else {
                        info.sector.clone()
                    }
                } else {
                    info.sector.clone()
                }
            })
        });

        related_tickers.push(RelatedTicker {
            ticker: ticker.clone(),
            name: ticker_info.as_ref().and_then(|t| t.name.clone()),
            sector: ticker_info.as_ref().and_then(|t| t.sector.clone()),
            industry: ticker_info.as_ref().and_then(|t| t.industry.clone()),
            relationship_type: candidate.relationship_type.clone(),
            relevance_score: candidate.relevance_score,
            context,
            has_research: latest_run.is_some(),
            latest_conviction,
            latest_run_id: latest_run.as_ref().map(|r| r.id.clone()),
            latest_run_status: latest_run.as_ref().map(|r| r.status.clone()),
        });
    }

    // Sort by relevance score descending, limit to top 10
    related_tickers.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    related_tickers.truncate(10);

    info!(
        %run_id,
        %primary_ticker,
        count = related_tickers.len(),
        "discovered related tickers"
    );

    Ok(RelatedTickersResponse {
        primary_ticker,
        related_tickers,
    })
}

#[derive(Debug, Clone)]
struct CandidateInfo {
    relationship_type: String,
    relevance_score: f64,
    context: Option<String>,
    mention_count: i32,
}

async fn add_sector_matches(
    state: &AppState,
    sector: &str,
    primary_ticker: &str,
    candidates: &mut HashMap<String, CandidateInfo>,
) -> Result<()> {
    let sector_tickers = state
        .db
        .list_ticker_universe(true, Some(sector), None, None)
        .await?;

    for ticker_info in sector_tickers {
        if ticker_info.ticker == primary_ticker {
            continue;
        }

        let context = build_sector_context(&ticker_info, sector);
        let score = 5.0; // Base score for sector match

        candidates
            .entry(ticker_info.ticker.clone())
            .and_modify(|c| {
                c.relevance_score = (c.relevance_score + score).min(10.0);
                if c.relationship_type != "same_sector" {
                    c.relationship_type = "same_sector".to_string();
                }
            })
            .or_insert(CandidateInfo {
                relationship_type: "same_sector".to_string(),
                relevance_score: score,
                context: Some(context),
                mention_count: 0,
            });
    }

    Ok(())
}

fn build_sector_context(ticker_info: &TickerUniverse, sector: &str) -> String {
    if let Some(ref industry) = ticker_info.industry {
        format!("{} - {}", sector, industry)
    } else {
        sector.to_string()
    }
}

async fn add_source_mentions(
    state: &AppState,
    run_id: &str,
    primary_ticker: &str,
    candidates: &mut HashMap<String, CandidateInfo>,
) -> Result<()> {
    let sources = state.db.list_sources_for_run(run_id).await?;

    // Get valid tickers for validation
    let valid_tickers = get_valid_tickers(state).await?;

    for source in sources {
        let text = match (&source.raw_text, &source.excerpt) {
            (Some(raw), _) => raw,
            (None, Some(excerpt)) => excerpt,
            (None, None) => continue,
        };

        let mentions = extract_ticker_mentions(text, &valid_tickers, primary_ticker);
        for ticker in mentions {
            let score = 3.0; // Weight for source mention

            candidates
                .entry(ticker.clone())
                .and_modify(|c| {
                    c.mention_count += 1;
                    c.relevance_score = (c.relevance_score + score * 0.5).min(10.0);
                    if c.relationship_type == "mentioned_in_thesis" {
                        c.relationship_type = "mentioned_in_sources".to_string();
                    }
                })
                .or_insert(CandidateInfo {
                    relationship_type: "mentioned_in_sources".to_string(),
                    relevance_score: score,
                    context: Some("Mentioned in research sources".to_string()),
                    mention_count: 1,
                });
        }
    }

    Ok(())
}

fn add_thesis_mentions(
    thesis: &str,
    primary_ticker: &str,
    candidates: &mut HashMap<String, CandidateInfo>,
) {
    // Simple regex-free extraction: look for uppercase ticker-like patterns
    // This is a basic heuristic - could be enhanced with NLP
    let words: Vec<&str> = thesis.split_whitespace().collect();

    for word in words {
        // Check if word looks like a ticker (2-5 uppercase letters, possibly with punctuation)
        let cleaned = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
        if is_potential_ticker(cleaned) {
            let ticker = cleaned.to_uppercase();
            if ticker == primary_ticker {
                continue;
            }

            let score = 2.0; // Weight for thesis mention

            candidates
                .entry(ticker.clone())
                .and_modify(|c| {
                    c.mention_count += 1;
                    c.relevance_score = (c.relevance_score + score * 0.3).min(10.0);
                })
                .or_insert(CandidateInfo {
                    relationship_type: "mentioned_in_thesis".to_string(),
                    relevance_score: score,
                    context: Some("Mentioned in thesis memo".to_string()),
                    mention_count: 1,
                });
        }
    }
}

fn is_potential_ticker(s: &str) -> bool {
    let len = s.len();
    if !(2..=5).contains(&len) {
        return false;
    }
    s.chars().all(|c| c.is_ascii_uppercase())
}

async fn get_valid_tickers(state: &AppState) -> Result<HashSet<String>> {
    let tickers = state
        .db
        .list_ticker_universe(true, None, None, None)
        .await?;
    Ok(tickers.into_iter().map(|t| t.ticker).collect())
}

fn extract_ticker_mentions(
    text: &str,
    valid_tickers: &HashSet<String>,
    primary_ticker: &str,
) -> Vec<String> {
    let mut mentions = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();

    for word in words {
        let cleaned = word
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_uppercase();

        if cleaned.len() >= 2
            && cleaned.len() <= 5
            && cleaned.chars().all(|c| c.is_ascii_uppercase())
            && valid_tickers.contains(&cleaned)
            && cleaned != primary_ticker
        {
            mentions.push(cleaned);
        }
    }

    mentions.sort();
    mentions.dedup();
    mentions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tickers(set: &[&str]) -> HashSet<String> {
        set.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn is_potential_ticker_accepts_2_to_5_uppercase() {
        assert!(is_potential_ticker("AA"));
        assert!(is_potential_ticker("NVDA"));
        assert!(is_potential_ticker("GOOGL"));
        assert!(!is_potential_ticker("A"));
        assert!(!is_potential_ticker("GOOGLE")); // 6 chars
        assert!(!is_potential_ticker("nvda"));
        assert!(!is_potential_ticker("N4"));
    }

    #[test]
    fn extract_ticker_mentions_filters_to_valid_set_and_excludes_primary() {
        let valid = tickers(&["NVDA", "AMD", "MSFT"]);
        let text = "NVDA outpaces AMD and MSFT, while NVDA stays primary. GOOG not in set.";
        let mentions = extract_ticker_mentions(text, &valid, "NVDA");
        assert_eq!(mentions, vec!["AMD".to_string(), "MSFT".to_string()]);
    }

    #[test]
    fn extract_ticker_mentions_handles_punctuation() {
        let valid = tickers(&["NVDA", "AMD"]);
        let text = "(AMD) — look at the report.";
        let mentions = extract_ticker_mentions(text, &valid, "NVDA");
        assert_eq!(mentions, vec!["AMD".to_string()]);
    }

    #[test]
    fn extract_ticker_mentions_dedupes_and_sorts() {
        let valid = tickers(&["AAPL", "MSFT", "AMD"]);
        let text = "MSFT AMD AMD AAPL MSFT";
        let mentions = extract_ticker_mentions(text, &valid, "NVDA");
        assert_eq!(
            mentions,
            vec!["AAPL".to_string(), "AMD".to_string(), "MSFT".to_string()]
        );
    }

    #[test]
    fn add_thesis_mentions_scores_known_like_tickers_and_skips_primary() {
        let mut candidates: HashMap<String, CandidateInfo> = HashMap::new();
        let thesis = "Compare NVDA to AMD, MSFT and GOOG, not lowercase 'nvda'.";
        add_thesis_mentions(thesis, "NVDA", &mut candidates);
        assert!(candidates.contains_key("AMD"));
        assert!(candidates.contains_key("MSFT"));
        assert!(candidates.contains_key("GOOG"));
        assert!(!candidates.contains_key("NVDA"));
    }
}
