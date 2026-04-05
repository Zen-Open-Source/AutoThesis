use crate::{
    app_state::AppState,
    db::Database,
    models::{EvidenceNoteInput, ReaderOutput, SourceRecord},
    services::source_ranker::classify_source,
};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::json;

pub async fn hydrate_sources(
    state: &AppState,
    run_id: &str,
    iteration_id: &str,
    sources: &[SourceRecord],
) -> Result<Vec<SourceRecord>> {
    let mut hydrated = Vec::new();

    for source in sources {
        match state.fetcher.fetch(&source.url).await {
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
        if let Some(updated) = state
            .db
            .list_sources(iteration_id)
            .await?
            .into_iter()
            .find(|candidate| candidate.id == source.id)
        {
            hydrated.push(updated);
        }
    }

    Ok(hydrated)
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
    for note in notes {
        db.insert_evidence_note(
            iteration_id,
            &note.source_id,
            &note.note_markdown,
            Some(&note.claim_type),
        )
        .await?;
    }
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
