use crate::{
    app_state::AppState,
    models::{EvidenceNoteRecord, SourceRecord},
};
use anyhow::Result;
use chrono::Utc;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub async fn synthesize(
    state: &AppState,
    ticker: &str,
    question: &str,
    iteration_number: i64,
    previous_draft: Option<&str>,
    previous_critique: Option<&str>,
    sources: &[SourceRecord],
    notes: &[EvidenceNoteRecord],
) -> Result<String> {
    let prompt = state.prompts.get("synthesizer")?;
    let payload = json!({
        "research_date": Utc::now().date_naive().to_string(),
        "ticker": ticker,
        "question": question,
        "iteration_number": iteration_number,
        "previous_draft": previous_draft,
        "previous_critique": previous_critique,
        "sources": sources,
        "evidence_notes": notes,
    });
    state
        .llm
        .complete_markdown(
            "synthesizer",
            prompt,
            &serde_json::to_string_pretty(&payload)?,
        )
        .await
}
