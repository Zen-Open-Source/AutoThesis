use crate::{
    app_state::AppState,
    models::{PlannerOutput, SearchQueryRecord},
    providers::search::SearchResultItem,
    services::source_ranker::{rank_search_result, RankedSearchResult},
};
use anyhow::Result;
use serde_json::json;
use std::{cmp::Ordering, collections::HashSet};
use tokio::task::JoinSet;

/// Cap on parallel search provider calls per iteration. Tavily and most
/// search APIs rate-limit bursts; 4 concurrent in-flight queries is a
/// reasonable compromise that yields a big latency win over serial calls.
const MAX_CONCURRENT_SEARCHES: usize = 4;

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
    let search_results = search_queries_parallel(state, queries, max_results_per_query).await?;

    let mut ranked: Vec<(String, RankedSearchResult)> = search_results
        .into_iter()
        .flat_map(|(query_id, items)| {
            items
                .into_iter()
                .map(move |item| (query_id.clone(), rank_search_result(item)))
        })
        .collect();

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

/// Execute all provider searches in parallel, bounded by
/// `MAX_CONCURRENT_SEARCHES`. Any single query failure aborts the batch;
/// that mirrors the previous sequential behaviour and preserves existing
/// error-handling semantics for the caller.
async fn search_queries_parallel(
    state: &AppState,
    queries: &[SearchQueryRecord],
    max_results_per_query: usize,
) -> Result<Vec<(String, Vec<SearchResultItem>)>> {
    type QueryOutcome = (usize, Result<(String, Vec<SearchResultItem>)>);
    let mut set: JoinSet<QueryOutcome> = JoinSet::new();
    let mut results: Vec<Option<(String, Vec<SearchResultItem>)>> =
        (0..queries.len()).map(|_| None).collect();
    let mut next_to_spawn = 0;
    let mut first_error: Option<anyhow::Error> = None;

    let spawn = |set: &mut JoinSet<_>, idx: usize| {
        let provider = state.search.clone();
        let query = queries[idx].clone();
        let query_id = query.id.clone();
        let query_text = query.query_text.clone();
        set.spawn(async move {
            let res = provider
                .search(&query_text, max_results_per_query)
                .await
                .map(|items| (query_id, items));
            (idx, res)
        });
    };

    while next_to_spawn < queries.len() && set.len() < MAX_CONCURRENT_SEARCHES {
        spawn(&mut set, next_to_spawn);
        next_to_spawn += 1;
    }

    while let Some(join_result) = set.join_next().await {
        match join_result {
            Ok((idx, Ok(value))) => {
                results[idx] = Some(value);
            }
            Ok((_idx, Err(error))) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            Err(join_error) => {
                if first_error.is_none() {
                    first_error =
                        Some(anyhow::anyhow!("search task failed: {join_error}"));
                }
            }
        }
        if first_error.is_none() && next_to_spawn < queries.len() {
            spawn(&mut set, next_to_spawn);
            next_to_spawn += 1;
        }
    }

    if let Some(error) = first_error {
        return Err(error);
    }

    Ok(results.into_iter().flatten().collect())
}
