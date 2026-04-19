use crate::{app_state::AppState, models::BatchJobRunWithDetails, status::RunStatus};
use anyhow::Result;

pub async fn sync_batch_jobs_for_run(state: &AppState, run_id: &str) -> Result<()> {
    let batch_job_ids = state.db.list_batch_job_ids_for_run(run_id).await?;
    for batch_job_id in batch_job_ids {
        sync_single_batch_job(state, &batch_job_id).await?;
    }
    Ok(())
}

async fn sync_single_batch_job(state: &AppState, batch_job_id: &str) -> Result<()> {
    let batch_job_runs = state.db.list_batch_job_runs(batch_job_id).await?;
    if batch_job_runs.is_empty() {
        state
            .db
            .update_batch_job_status(batch_job_id, RunStatus::Queued.as_str())
            .await?;
        return Ok(());
    }

    let mut completed_count = 0usize;
    let mut failed_count = 0usize;
    let mut has_running = false;
    let mut has_queued = false;

    for batch_job_run in &batch_job_runs {
        // Any status the DB returns that we don't recognise (e.g. a future
        // "retrying" state) is treated as in-flight so we don't prematurely
        // finalise the batch.
        let status = batch_job_run
            .run
            .as_ref()
            .and_then(|run| RunStatus::parse(&run.status))
            .unwrap_or(RunStatus::Queued);
        match status {
            RunStatus::Completed => completed_count += 1,
            RunStatus::Failed | RunStatus::Cancelled => failed_count += 1,
            RunStatus::Queued => has_queued = true,
            RunStatus::Running => has_running = true,
        }
    }

    let all_terminal = completed_count + failed_count == batch_job_runs.len();
    if all_terminal {
        let status = if failed_count == 0 {
            RunStatus::Completed.as_str()
        } else if completed_count == 0 {
            RunStatus::Failed.as_str()
        } else {
            // Domain-specific composite status that doesn't map to RunStatus.
            "failed_partial"
        };
        let summary = build_batch_summary(&batch_job_runs, completed_count, failed_count);
        state
            .db
            .finalize_batch_job(batch_job_id, status, Some(&summary))
            .await?;
    } else {
        let status = if has_running {
            RunStatus::Running
        } else if has_queued {
            RunStatus::Queued
        } else {
            RunStatus::Running
        };
        state
            .db
            .update_batch_job_status(batch_job_id, status.as_str())
            .await?;
    }
    Ok(())
}

fn build_batch_summary(
    batch_job_runs: &[BatchJobRunWithDetails],
    completed_count: usize,
    failed_count: usize,
) -> String {
    let total = batch_job_runs.len();
    let pending_count = total.saturating_sub(completed_count + failed_count);
    format!(
        "Completed: {completed_count} | Failed/Cancelled: {failed_count} | Pending: {pending_count} | Total: {total}"
    )
}
