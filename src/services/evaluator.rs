use crate::{
    app_state::AppState,
    models::{EvaluationRubric, EvaluatorOutput, EvidenceNoteRecord, SourceRecord},
};
use anyhow::Result;
use serde_json::{json, Value};

pub async fn evaluate(
    state: &AppState,
    previous_draft: Option<&str>,
    current_draft: &str,
    critique: &str,
    sources: &[SourceRecord],
    notes: &[EvidenceNoteRecord],
) -> Result<EvaluatorOutput> {
    if previous_draft.is_none() {
        return Ok(EvaluatorOutput {
            improved: true,
            score: 6.0,
            rubric: EvaluationRubric {
                evidence_coverage: 6.0,
                source_quality: 6.0,
                balance: 6.0,
                specificity: 6.0,
                decision_usefulness: 6.0,
            },
            reasoning: "Baseline iteration established the initial memo and remaining gaps should be iterated on.".to_string(),
            should_continue: true,
        });
    }

    let prompt = state.prompts.get("evaluator")?;
    let payload = json!({
        "previous_draft": previous_draft,
        "current_draft": current_draft,
        "critique": critique,
        "sources": sources,
        "evidence_notes": notes,
    });
    let value = state
        .llm
        .complete_json(
            "evaluator",
            prompt,
            &serde_json::to_string_pretty(&payload)?,
        )
        .await?;
    parse_evaluator_output(value)
}

pub fn parse_evaluator_output(value: Value) -> Result<EvaluatorOutput> {
    let mut output: EvaluatorOutput = serde_json::from_value(value)?;
    output.score = output.score.clamp(0.0, 10.0);
    output.rubric.evidence_coverage = output.rubric.evidence_coverage.clamp(0.0, 10.0);
    output.rubric.source_quality = output.rubric.source_quality.clamp(0.0, 10.0);
    output.rubric.balance = output.rubric.balance.clamp(0.0, 10.0);
    output.rubric.specificity = output.rubric.specificity.clamp(0.0, 10.0);
    output.rubric.decision_usefulness = output.rubric.decision_usefulness.clamp(0.0, 10.0);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::parse_evaluator_output;
    use serde_json::json;

    #[test]
    fn parses_and_clamps_evaluator_output() {
        let output = parse_evaluator_output(json!({
            "improved": true,
            "score": 11.2,
            "rubric": {
                "evidence_coverage": 12.0,
                "source_quality": 9.0,
                "balance": 8.0,
                "specificity": -2.0,
                "decision_usefulness": 7.5
            },
            "reasoning": "More balanced.",
            "continue": true
        }))
        .expect("should parse");

        assert_eq!(output.score, 10.0);
        assert_eq!(output.rubric.evidence_coverage, 10.0);
        assert_eq!(output.rubric.specificity, 0.0);
        assert!(output.should_continue);
    }
}
