use crate::{
    app_state::AppState,
    models::{EvidenceNoteRecord, SourceRecord},
};
use anyhow::Result;
use serde_json::json;

pub async fn critique(
    state: &AppState,
    ticker: &str,
    question: &str,
    draft_markdown: &str,
    sources: &[SourceRecord],
    notes: &[EvidenceNoteRecord],
) -> Result<String> {
    let prompt = state.prompts.get("critic")?;
    let payload = json!({
        "ticker": ticker,
        "question": question,
        "draft_markdown": draft_markdown,
        "sources": sources,
        "evidence_notes": notes,
    });
    state
        .llm
        .complete_markdown("critic", prompt, &serde_json::to_string_pretty(&payload)?)
        .await
}
