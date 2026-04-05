use crate::{app_state::AppState, models::PlannerOutput};
use anyhow::Result;
use serde_json::json;

pub async fn build_plan(
    state: &AppState,
    ticker: &str,
    question: &str,
    previous_critique: Option<&str>,
    iteration_number: i64,
) -> Result<PlannerOutput> {
    let prompt = state.prompts.get("planner")?;
    let payload = json!({
        "ticker": ticker,
        "question": question,
        "iteration_number": iteration_number,
        "previous_critique": previous_critique,
    });
    let value = state
        .llm
        .complete_json("planner", prompt, &serde_json::to_string_pretty(&payload)?)
        .await?;
    Ok(serde_json::from_value(value)?)
}

pub fn plan_to_markdown(plan: &PlannerOutput) -> String {
    format!(
        "# Research Goal

{}

## Subquestions
{}

## Evidence Needed
{}

## Priority Order
{}",
        plan.research_goal,
        bullets(&plan.subquestions),
        bullets(&plan.evidence_needed),
        bullets(&plan.priority_order),
    )
}

fn bullets(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}
