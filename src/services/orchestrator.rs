use crate::{
    app_state::AppState,
    markdown::render_markdown,
    models::EvaluatorOutput,
    services::{
        alerts, batch, comparison, critic, evaluator, planner, reader, search, synthesizer,
    },
    status::RunStatus,
};
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use tracing::{error, info, warn};
use url::Url;

#[derive(Debug, thiserror::Error)]
#[error("run cancelled")]
struct RunCancelled;

/// Spawn an orchestrator run in the background, bounded by the global
/// `AppState::run_semaphore` concurrency cap. Every code path that starts a
/// research run (manual create, retry, batch, comparison, scanner promote,
/// scheduler, watchlist refresh trigger, dashboard refresh) should go
/// through this helper so that `config.max_concurrent_runs` is enforced
/// globally, not just per-entry-point.
pub fn spawn_bounded_run(state: AppState, run_id: String) {
    tokio::spawn(async move {
        // Clone the semaphore Arc out so we don't hold `state` while awaiting.
        let semaphore = state.run_semaphore.clone();
        let permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                warn!(%run_id, "run semaphore closed, dropping run");
                return;
            }
        };
        let result = execute_run(state, run_id.clone()).await;
        drop(permit);
        if let Err(error) = result {
            error!(%run_id, error = %error, "background orchestrator failed");
        }
    });
}

pub async fn execute_run(state: AppState, run_id: String) -> Result<()> {
    state.cancellation.register(&run_id);
    let result = execute_run_inner(state.clone(), &run_id).await;
    state.cancellation.clear(&run_id);
    if let Err(error) = result {
        if error.downcast_ref::<RunCancelled>().is_some() {
            info!(%run_id, "run cancelled before completion");
            let _ = state
                .db
                .set_run_status(&run_id, RunStatus::Cancelled.as_str())
                .await;
            let _ = sync_thesis_alerts_for_run(&state, &run_id).await;
            let _ = comparison::sync_comparisons_for_run(&state, &run_id).await;
            let _ = batch::sync_batch_jobs_for_run(&state, &run_id).await;
            return Ok(());
        }

        error!(%run_id, error = %error, "run failed");
        let _ = state
            .db
            .set_run_status(&run_id, RunStatus::Failed.as_str())
            .await;
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
        let _ = sync_thesis_alerts_for_run(&state, &run_id).await;
        let _ = comparison::sync_comparisons_for_run(&state, &run_id).await;
        let _ = batch::sync_batch_jobs_for_run(&state, &run_id).await;
        return Err(error);
    }
    let _ = sync_thesis_alerts_for_run(&state, &run_id).await;
    let _ = comparison::sync_comparisons_for_run(&state, &run_id).await;
    let _ = batch::sync_batch_jobs_for_run(&state, &run_id).await;
    Ok(())
}

async fn execute_run_inner(state: AppState, run_id: &str) -> Result<()> {
    let run = state
        .db
        .get_run(run_id)
        .await?
        .ok_or_else(|| anyhow!("run not found: {run_id}"))?;
    ensure_run_not_cancelled(&state, run_id).await?;

    info!(%run_id, ticker = %run.ticker, "starting run");
    state
        .db
        .set_run_status(run_id, RunStatus::Running.as_str())
        .await?;
    let _ = comparison::sync_comparisons_for_run(&state, run_id).await;
    let _ = batch::sync_batch_jobs_for_run(&state, run_id).await;
    state
        .db
        .insert_event(run_id, None, "run_started", "Research run started", None)
        .await?;

    let mut previous_draft: Option<String> = None;
    let mut previous_critique: Option<String> = None;
    let mut latest_draft = String::new();
    let mut latest_evaluation: Option<EvaluatorOutput> = None;

    for iteration_number in 1..=state.config.max_iterations as i64 {
        ensure_run_not_cancelled(&state, run_id).await?;
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

        ensure_run_not_cancelled(&state, run_id).await?;
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

        ensure_run_not_cancelled(&state, run_id).await?;
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

        ensure_run_not_cancelled(&state, run_id).await?;
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

        ensure_run_not_cancelled(&state, run_id).await?;
        let hydrated_sources =
            reader::hydrate_sources(&state, run_id, &iteration.id, &inserted_sources).await?;
        let note_inputs =
            reader::extract_evidence_notes(&state, &run.ticker, &run.question, &hydrated_sources)
                .await?;
        reader::persist_notes(&state.db, &iteration.id, &note_inputs).await?;
        let persisted_notes = state.db.list_evidence_notes(&iteration.id).await?;

        ensure_run_not_cancelled(&state, run_id).await?;
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

        ensure_run_not_cancelled(&state, run_id).await?;
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

        ensure_run_not_cancelled(&state, run_id).await?;
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

    ensure_run_not_cancelled(&state, run_id).await?;
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

async fn ensure_run_not_cancelled(state: &AppState, run_id: &str) -> Result<()> {
    // Fast path: in-process cancellation flag (set synchronously by the
    // cancel route), avoids a DB round-trip on every iteration step.
    if state.cancellation.is_cancelled(run_id) {
        return Err(RunCancelled.into());
    }
    // Fallback: if the flag was never registered (e.g. a run was requeued
    // by an external mechanism) or is unknown, fall back to the DB status
    // so cancellation still eventually kicks in. Use the lightweight
    // status-only query to avoid materializing the entire run row on every
    // orchestrator checkpoint.
    let status = state
        .db
        .get_run_status(run_id)
        .await?
        .ok_or_else(|| anyhow!("run not found while checking status: {run_id}"))?;
    if RunStatus::parse(&status) == Some(RunStatus::Cancelled) {
        return Err(RunCancelled.into());
    }
    Ok(())
}

async fn sync_thesis_alerts_for_run(state: &AppState, run_id: &str) -> Result<()> {
    if let Some(run) = state.db.get_run(run_id).await? {
        alerts::evaluate_watchlists_for_ticker(state, &run.ticker).await?;
    }
    Ok(())
}
