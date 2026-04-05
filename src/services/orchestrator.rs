use crate::{
    app_state::AppState,
    markdown::render_markdown,
    models::{EvaluatorOutput, EvidenceNoteRecord, Run, SourceRecord},
    services::{critic, evaluator, planner, reader, search, synthesizer},
};
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use tracing::{error, info};
use url::Url;

pub async fn execute_run(state: AppState, run_id: String) -> Result<()> {
    let result = execute_run_inner(state.clone(), &run_id).await;
    if let Err(error) = result {
        error!(%run_id, error = %error, "run failed");
        let _ = state.db.set_run_status(&run_id, "failed").await;
        let _ = state
            .db
            .insert_event(
                &run_id,
                None,
                "run_failed",
                &format!("Run failed: {error}"),
                None,
            )
            .await;
        return Err(error);
    }
    Ok(())
}

async fn execute_run_inner(state: AppState, run_id: &str) -> Result<()> {
    let run = state
        .db
        .get_run(run_id)
        .await?
        .ok_or_else(|| anyhow!("run not found: {run_id}"))?;

    info!(%run_id, ticker = %run.ticker, "starting run");
    state.db.set_run_status(run_id, "running").await?;
    state
        .db
        .insert_event(run_id, None, "run_started", "Research run started", None)
        .await?;

    let mut previous_draft: Option<String> = None;
    let mut previous_critique: Option<String> = None;
    let mut latest_draft = String::new();
    let mut latest_evaluation: Option<EvaluatorOutput> = None;

    for iteration_number in 1..=state.config.max_iterations as i64 {
        let iteration = state.db.create_iteration(run_id, iteration_number).await?;
        state
            .db
            .insert_event(
                run_id,
                Some(&iteration.id),
                "iteration_started",
                &format!("Running iteration {iteration_number}"),
                None,
            )
            .await?;

        let plan = planner::build_plan(
            &state,
            &run.ticker,
            &run.question,
            previous_critique.as_deref(),
            iteration_number,
        )
        .await
        .with_context(|| format!("failed to build plan for iteration {iteration_number}"))?;
        let plan_markdown = planner::plan_to_markdown(&plan);
        state
            .db
            .update_iteration_plan(&iteration.id, &plan_markdown)
            .await?;
        state
            .db
            .insert_event(
                run_id,
                Some(&iteration.id),
                "plan_ready",
                &format!("Planner completed for iteration {iteration_number}"),
                None,
            )
            .await?;

        let query_texts = search::generate_queries(
            &state,
            &run.ticker,
            &run.question,
            &plan,
            previous_critique.as_deref(),
            iteration_number,
        )
        .await?;
        let mut stored_queries = Vec::new();
        for query_text in query_texts {
            stored_queries.push(
                state
                    .db
                    .insert_search_query(&iteration.id, &query_text)
                    .await?,
            );
        }
        state
            .db
            .insert_event(
                run_id,
                Some(&iteration.id),
                "searching",
                &format!("Searching {} queries", stored_queries.len()),
                None,
            )
            .await?;

        let ranked_results = search::search_and_rank(
            &state,
            &stored_queries,
            5,
            state.config.max_sources_per_iteration,
        )
        .await?;

        let mut inserted_sources = Vec::new();
        for (query_id, result) in ranked_results {
            state
                .db
                .insert_search_result(
                    &iteration.id,
                    &query_id,
                    result.title.as_deref(),
                    &result.url,
                    result.snippet.as_deref(),
                    Some(result.rank_score),
                    Some(&result.source_type),
                )
                .await?;

            let domain = Url::parse(&result.url)
                .ok()
                .and_then(|parsed| parsed.domain().map(|value| value.to_string()));
            let source = state
                .db
                .insert_source(
                    run_id,
                    Some(&iteration.id),
                    &result.url,
                    result.title.as_deref(),
                    domain.as_deref(),
                    result.snippet.as_deref(),
                    Some(result.rank_score),
                    Some(&result.source_type),
                )
                .await?;
            inserted_sources.push(source);
        }
        state
            .db
            .insert_event(
                run_id,
                Some(&iteration.id),
                "sources_selected",
                &format!("Selected {} sources", inserted_sources.len()),
                None,
            )
            .await?;

        let hydrated_sources =
            reader::hydrate_sources(&state, run_id, &iteration.id, &inserted_sources).await?;
        let note_inputs =
            reader::extract_evidence_notes(&state, &run.ticker, &run.question, &hydrated_sources)
                .await?;
        reader::persist_notes(&state.db, &iteration.id, &note_inputs).await?;
        let persisted_notes = state.db.list_evidence_notes(&iteration.id).await?;

        latest_draft = synthesizer::synthesize(
            &state,
            &run.ticker,
            &run.question,
            iteration_number,
            previous_draft.as_deref(),
            previous_critique.as_deref(),
            &hydrated_sources,
            &persisted_notes,
        )
        .await?;
        state
            .db
            .update_iteration_draft(&iteration.id, &latest_draft)
            .await?;
        state
            .db
            .insert_event(
                run_id,
                Some(&iteration.id),
                "draft_written",
                &format!("Draft {} written", iteration_number),
                None,
            )
            .await?;

        let critique_markdown = critic::critique(
            &state,
            &run.ticker,
            &run.question,
            &latest_draft,
            &hydrated_sources,
            &persisted_notes,
        )
        .await?;
        state
            .db
            .update_iteration_critique(&iteration.id, &critique_markdown)
            .await?;

        let evaluation = evaluator::evaluate(
            &state,
            previous_draft.as_deref(),
            &latest_draft,
            &critique_markdown,
            &hydrated_sources,
            &persisted_notes,
        )
        .await?;
        let evaluation_json = serde_json::to_string_pretty(&evaluation)?;
        state
            .db
            .update_iteration_evaluation(&iteration.id, &evaluation_json)
            .await?;
        state
            .db
            .set_iteration_status(&iteration.id, "completed")
            .await?;

        let payload = serde_json::to_string(&json!({
            "score": evaluation.score,
            "continue": evaluation.should_continue,
        }))?;
        state
            .db
            .insert_event(
                run_id,
                Some(&iteration.id),
                "iteration_completed",
                &format!(
                    "Iteration {iteration_number} completed with score {:.1}",
                    evaluation.score
                ),
                Some(&payload),
            )
            .await?;

        previous_draft = Some(latest_draft.clone());
        previous_critique = Some(critique_markdown);
        latest_evaluation = Some(evaluation);
    }

    let final_html = render_markdown(&latest_draft);
    let summary = summarize_memo(&latest_draft);
    state
        .db
        .finalize_run(
            run_id,
            state.config.max_iterations as i64,
            &latest_draft,
            &final_html,
            summary.as_deref(),
        )
        .await?;

    let payload = serde_json::to_string(&json!({
        "final_score": latest_evaluation.as_ref().map(|evaluation| evaluation.score),
    }))?;
    state
        .db
        .insert_event(
            run_id,
            None,
            "run_completed",
            "Research run completed",
            Some(&payload),
        )
        .await?;
    Ok(())
}

fn summarize_memo(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .skip_while(|line| !line.trim().starts_with("# Executive Summary"))
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
}

#[allow(dead_code)]
fn _unused_to_keep_imports(_run: &Run, _sources: &[SourceRecord], _notes: &[EvidenceNoteRecord]) {}
