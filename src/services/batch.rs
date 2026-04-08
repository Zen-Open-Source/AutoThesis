use crate::{app_state::AppState, models::BatchJobRunWithDetails};
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
            .update_batch_job_status(batch_job_id, "queued")
            .await?;
        return Ok(());
    }

    let mut completed_count = 0usize;
    let mut failed_count = 0usize;
    let mut has_running = false;
    let mut has_queued = false;

    for batch_job_run in &batch_job_runs {
        let status = batch_job_run
            .run
            .as_ref()
            .map(|run| run.status.as_str())
            .unwrap_or("queued");
        match status {
            "completed" => completed_count += 1,
            "failed" | "cancelled" => failed_count += 1,
            "queued" => has_queued = true,
            _ => has_running = true,
        }
    }

    let all_terminal = completed_count + failed_count == batch_job_runs.len();
    if all_terminal {
        let status = if failed_count == 0 {
            "completed"
        } else if completed_count == 0 {
            "failed"
        } else {
            "failed_partial"
        };
        let summary = build_batch_summary(&batch_job_runs, completed_count, failed_count);
        state
            .db
            .finalize_batch_job(batch_job_id, status, Some(&summary))
            .await?;
    } else {
        let status = if has_running {
            "running"
        } else if has_queued {
            "queued"
        } else {
            "running"
        };
        state
            .db
            .update_batch_job_status(batch_job_id, status)
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
