use crate::{
    app_state::AppState,
    markdown::render_markdown,
    models::{PreliminaryThesisOutput, ScanSignal},
};
use anyhow::Result;

/// Generate a preliminary thesis for a ticker based on detected signals.
pub async fn generate_preliminary_thesis(
    state: &AppState,
    ticker: &str,
    signals: &[ScanSignal],
) -> Result<PreliminaryThesisOutput> {
    let prompt = state.prompts.get("preliminary_thesis")?;
    let payload = serde_json::json!({
        "ticker": ticker,
        "signals": signals,
        "research_date": chrono::Utc::now().date_naive().to_string(),
    });

    match state
        .llm
        .complete_json(
            "preliminary_thesis",
            prompt,
            &serde_json::to_string_pretty(&payload)?,
        )
        .await
    {
        Ok(value) => {
            let output: PreliminaryThesisOutput = serde_json::from_value(value)?;
            Ok(output)
        }
        Err(_) => Ok(fallback_thesis(ticker, signals)),
    }
}

fn fallback_thesis(ticker: &str, signals: &[ScanSignal]) -> PreliminaryThesisOutput {
    let signal_summary: String = signals
        .iter()
        .map(|s| {
            format!(
                "- {}: {} (strength: {:.1})",
                s.signal_type, s.description, s.strength
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let thesis = format!(
        "# Preliminary Thesis for {}\n\n## Summary\n\nThis is a preliminary thesis generated for scanning purposes. Further research is recommended.\n\n## Detected Signals\n\n{}\n\n## Next Steps\n\nRun a full research iteration to develop a comprehensive thesis.",
        ticker,
        signal_summary
    );

    let key_catalysts = signals
        .iter()
        .filter(|s| s.signal_type == "earnings_catalyst" || s.signal_type == "analyst_activity")
        .map(|s| s.description.clone())
        .collect::<Vec<_>>()
        .join("; ");

    let risk_factors = "Insufficient data for risk assessment. Full research required.".to_string();

    PreliminaryThesisOutput {
        thesis_markdown: thesis,
        key_catalysts: if key_catalysts.is_empty() {
            "None identified in scan".to_string()
        } else {
            key_catalysts
        },
        risk_factors,
        quality_score: 3.0,
    }
}

/// Render preliminary thesis to HTML.
pub fn render_thesis_html(markdown: &str) -> String {
    render_markdown(markdown)
}
