use crate::{
    app_state::AppState,
    db::{Database, EvidenceNoteInsert},
    models::{EvidenceNoteInput, ReaderOutput, SourceRecord},
    providers::fetch::FetchedPage,
    services::source_ranker::classify_source,
};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use tokio::task::JoinSet;

/// Maximum number of concurrent source fetches per iteration. Bounded to
/// avoid stampeding outbound servers with bursts of requests while still
/// letting iterations finish noticeably faster than the old serial loop.
const MAX_CONCURRENT_FETCHES: usize = 6;

pub async fn hydrate_sources(
    state: &AppState,
    run_id: &str,
    iteration_id: &str,
    sources: &[SourceRecord],
) -> Result<Vec<SourceRecord>> {
    // Fetch phase: run up to MAX_CONCURRENT_FETCHES HTTP requests in parallel
    // and collect results in source order. DB writes follow in a second pass
    // so that we stay on one SQLite connection at a time and keep event
    // ordering deterministic.
    let fetch_results = fetch_pages_parallel(state, sources).await;

    let mut hydrated = Vec::with_capacity(sources.len());
    for (source, fetched) in sources.iter().zip(fetch_results.into_iter()) {
        match fetched {
            Ok(page) => {
                let source_type = classify_source(
                    page.domain.as_deref().unwrap_or_default(),
                    page.title.as_deref().unwrap_or_default(),
                );
                let excerpt = excerpt_from_text(&page.text, 400);
                state
                    .db
                    .update_source_content(
                        &source.id,
                        page.title.as_deref().or(source.title.as_deref()),
                        page.domain.as_deref().or(source.domain.as_deref()),
                        Some(&page.text),
                        Some(&excerpt),
                        source.quality_score,
                        Some(&source_type),
                        Some(Utc::now()),
                    )
                    .await?;
                state
                    .db
                    .insert_event(
                        run_id,
                        Some(iteration_id),
                        "source_fetched",
                        &format!("Fetched {}", source.url),
                        None,
                    )
                    .await?;
            }
            Err(error) => {
                let payload = serde_json::to_string(&json!({
                    "url": source.url,
                    "error": error.to_string(),
                }))?;
                state
                    .db
                    .insert_event(
                        run_id,
                        Some(iteration_id),
                        "source_fetch_failed",
                        &format!("Failed to fetch {}", source.url),
                        Some(&payload),
                    )
                    .await?;
            }
        }
        if let Some(updated) = state.db.get_source(&source.id).await? {
            hydrated.push(updated);
        }
    }

    Ok(hydrated)
}

/// Fetch all source URLs in parallel, capped at MAX_CONCURRENT_FETCHES.
/// Results are returned in the same order as the input `sources` slice.
async fn fetch_pages_parallel(
    state: &AppState,
    sources: &[SourceRecord],
) -> Vec<anyhow::Result<FetchedPage>> {
    let mut set: JoinSet<(usize, anyhow::Result<FetchedPage>)> = JoinSet::new();
    let mut results: Vec<Option<anyhow::Result<FetchedPage>>> =
        (0..sources.len()).map(|_| None).collect();
    let mut next_to_spawn = 0;

    while next_to_spawn < sources.len() && set.len() < MAX_CONCURRENT_FETCHES {
        let idx = next_to_spawn;
        let fetcher = state.fetcher.clone();
        let url = sources[idx].url.clone();
        set.spawn(async move { (idx, fetcher.fetch(&url).await) });
        next_to_spawn += 1;
    }

    while let Some(join_result) = set.join_next().await {
        let (idx, res) = match join_result {
            Ok(tuple) => tuple,
            Err(join_error) => {
                // A JoinError is either a panic or a cancellation - surface
                // it as a fetch error so the caller still records the event.
                let idx = next_to_spawn.saturating_sub(1);
                (idx, Err(anyhow::anyhow!("fetch task failed: {join_error}")))
            }
        };
        results[idx] = Some(res);
        if next_to_spawn < sources.len() {
            let spawn_idx = next_to_spawn;
            let fetcher = state.fetcher.clone();
            let url = sources[spawn_idx].url.clone();
            set.spawn(async move { (spawn_idx, fetcher.fetch(&url).await) });
            next_to_spawn += 1;
        }
    }

    results
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| Err(anyhow::anyhow!("fetch result missing"))))
        .collect()
}

pub async fn extract_evidence_notes(
    state: &AppState,
    ticker: &str,
    question: &str,
    sources: &[SourceRecord],
) -> Result<Vec<EvidenceNoteInput>> {
    let prompt = state.prompts.get("reader")?;
    let documents = sources
        .iter()
        .map(SourceForPrompt::from)
        .collect::<Vec<_>>();
    let payload = json!({
        "ticker": ticker,
        "question": question,
        "sources": documents,
    });

    match state
        .llm
        .complete_json("reader", prompt, &serde_json::to_string_pretty(&payload)?)
        .await
    {
        Ok(value) => {
            let output: ReaderOutput = serde_json::from_value(value)?;
            Ok(output.notes)
        }
        Err(_) => Ok(fallback_notes(sources)),
    }
}

pub async fn persist_notes(
    db: &Database,
    iteration_id: &str,
    notes: &[EvidenceNoteInput],
) -> Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    let rows: Vec<EvidenceNoteInsert<'_>> = notes
        .iter()
        .map(|note| EvidenceNoteInsert {
            source_id: note.source_id.as_str(),
            note_markdown: note.note_markdown.as_str(),
            claim_type: Some(note.claim_type.as_str()),
        })
        .collect();
    db.insert_evidence_notes_batch(iteration_id, &rows).await?;
    Ok(())
}

fn fallback_notes(sources: &[SourceRecord]) -> Vec<EvidenceNoteInput> {
    sources
        .iter()
        .map(|source| {
            let content = source
                .excerpt
                .as_deref()
                .or(source.raw_text.as_deref())
                .unwrap_or("No extractable text was available.");
            let note = format!(
                "- Fact: {}
- Open question: Validate this source directly if the memo depends on detailed numbers.",
                excerpt_from_text(content, 220)
            );
            EvidenceNoteInput {
                source_id: source.id.clone(),
                note_markdown: note,
                claim_type: "fact".to_string(),
            }
        })
        .collect()
}

fn excerpt_from_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[derive(Debug, Serialize)]
struct SourceForPrompt {
    source_id: String,
    title: Option<String>,
    url: String,
    source_type: Option<String>,
    text: String,
}

impl From<&SourceRecord> for SourceForPrompt {
    fn from(source: &SourceRecord) -> Self {
        let text = source
            .raw_text
            .clone()
            .or_else(|| source.excerpt.clone())
            .unwrap_or_else(|| "No extractable text available.".to_string());
        Self {
            source_id: source.id.clone(),
            title: source.title.clone(),
            url: source.url.clone(),
            source_type: source.source_type.clone(),
            text: excerpt_from_text(&text, 4_000),
        }
    }
}
