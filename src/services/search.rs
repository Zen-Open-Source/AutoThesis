use crate::{
    app_state::AppState,
    models::{PlannerOutput, SearchQueryRecord},
    services::source_ranker::{rank_search_result, RankedSearchResult},
};
use anyhow::Result;
use serde_json::json;
use std::{cmp::Ordering, collections::HashSet};

pub async fn generate_queries(
    state: &AppState,
    ticker: &str,
    question: &str,
    plan: &PlannerOutput,
    previous_critique: Option<&str>,
    iteration_number: i64,
) -> Result<Vec<String>> {
    let prompt = state.prompts.get("search_query_writer")?;
    let payload = json!({
        "ticker": ticker,
        "question": question,
        "iteration_number": iteration_number,
        "plan": plan,
        "previous_critique": previous_critique,
    });
    let value = state
        .llm
        .complete_json(
            "search_query_writer",
            prompt,
            &serde_json::to_string_pretty(&payload)?,
        )
        .await?;
    let output: crate::models::SearchQueryOutput = serde_json::from_value(value)?;
    Ok(output
        .queries
        .into_iter()
        .filter(|query| !query.trim().is_empty())
        .collect())
}

pub async fn search_and_rank(
    state: &AppState,
    queries: &[SearchQueryRecord],
    max_results_per_query: usize,
    max_total_sources: usize,
) -> Result<Vec<(String, RankedSearchResult)>> {
    let mut ranked = Vec::new();

    for query in queries {
        let results = state
            .search
            .search(&query.query_text, max_results_per_query)
            .await?;
        for result in results {
            ranked.push((query.id.clone(), rank_search_result(result)));
        }
    }

    ranked.sort_by(|a, b| {
        b.1.rank_score
            .partial_cmp(&a.1.rank_score)
            .unwrap_or(Ordering::Equal)
    });

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for entry in ranked {
        if seen.insert(entry.1.url.clone()) {
            deduped.push(entry);
        }
        if deduped.len() >= max_total_sources {
            break;
        }
    }

    Ok(deduped)
}
