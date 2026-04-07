use crate::{app_state::AppState, models::ComparisonRunWithDetails};
use anyhow::Result;

pub async fn sync_comparisons_for_run(state: &AppState, run_id: &str) -> Result<()> {
    let comparison_ids = state.db.list_comparison_ids_for_run(run_id).await?;
    for comparison_id in comparison_ids {
        sync_single_comparison(state, &comparison_id).await?;
    }
    Ok(())
}

async fn sync_single_comparison(state: &AppState, comparison_id: &str) -> Result<()> {
    let comparison_runs = state.db.list_comparison_runs(comparison_id).await?;
    if comparison_runs.is_empty() {
        state
            .db
            .update_comparison_status(comparison_id, "queued")
            .await?;
        return Ok(());
    }

    let mut completed_count = 0usize;
    let mut failed_count = 0usize;
    let mut has_running = false;
    let mut has_queued = false;

    for comparison_run in &comparison_runs {
        let status = comparison_run
            .run
            .as_ref()
            .map(|run| run.status.as_str())
            .unwrap_or("queued");
        match status {
            "completed" => completed_count += 1,
            "failed" => failed_count += 1,
            "queued" => has_queued = true,
            _ => has_running = true,
        }
    }

    let all_terminal = completed_count + failed_count == comparison_runs.len();
    if all_terminal {
        let (status, summary, final_html) =
            build_terminal_rollup(&comparison_runs, completed_count, failed_count);
        state
            .db
            .finalize_comparison(comparison_id, status, &final_html, summary.as_deref())
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
            .update_comparison_status(comparison_id, status)
            .await?;
    }

    Ok(())
}

fn build_terminal_rollup(
    comparison_runs: &[ComparisonRunWithDetails],
    completed_count: usize,
    failed_count: usize,
) -> (&'static str, Option<String>, String) {
    let status = if failed_count == 0 {
        "completed"
    } else if completed_count == 0 {
        "failed"
    } else {
        "failed_partial"
    };

    let summary_parts = comparison_runs
        .iter()
        .map(|comparison_run| {
            let run_status = comparison_run
                .run
                .as_ref()
                .map(|run| run.status.as_str())
                .unwrap_or("queued");
            let run_summary = comparison_run
                .run
                .as_ref()
                .and_then(|run| run.summary.as_deref())
                .unwrap_or("No summary available");
            format!(
                "{} ({}) — {}",
                comparison_run.ticker, run_status, run_summary
            )
        })
        .collect::<Vec<_>>();
    let summary = if summary_parts.is_empty() {
        None
    } else {
        Some(summary_parts.join(" | "))
    };

    let sections = comparison_runs
        .iter()
        .map(|comparison_run| {
            let run_status = comparison_run
                .run
                .as_ref()
                .map(|run| run.status.as_str())
                .unwrap_or("queued");
            let run_summary = comparison_run
                .run
                .as_ref()
                .and_then(|run| run.summary.as_deref())
                .unwrap_or("No summary available");
            let run_html = comparison_run
                .run
                .as_ref()
                .and_then(|run| run.final_memo_html.as_deref())
                .unwrap_or("<p>Final memo not available.</p>");
            format!(
                "<article><h3>{}</h3><p><strong>Status:</strong> {}</p><p>{}</p><div>{}</div></article>",
                escape_html(&comparison_run.ticker),
                escape_html(run_status),
                escape_html(run_summary),
                run_html
            )
        })
        .collect::<Vec<_>>();
    let final_html = format!("<section>{}</section>", sections.join(""));

    (status, summary, final_html)
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
