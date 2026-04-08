use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    markdown::render_markdown,
    models::{
        BatchJob, Bookmark, EvaluatorOutput, EventRecord, Iteration, IterationSummary, Run,
        RunTemplate, SearchQueryRecord, SourceRecord,
    },
};
use askama::Template;
use axum::{
    extract::{Path, State},
    response::Html,
};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    runs: Vec<Run>,
    run_templates: Vec<RunTemplate>,
}

#[derive(Template)]
#[template(path = "run.html")]
struct RunDetailTemplate {
    run: Run,
    events: Vec<EventRecord>,
    iterations: Vec<IterationSummary>,
    final_memo_html: Option<String>,
    can_cancel: bool,
    can_retry: bool,
}

#[derive(Clone)]
struct EvidenceNoteView {
    claim_type: Option<String>,
    note_html: String,
}

struct IterationPageDetail {
    iteration: Iteration,
    search_queries: Vec<SearchQueryRecord>,
    sources: Vec<SourceRecord>,
    evidence_notes: Vec<EvidenceNoteView>,
}

#[derive(Template)]
#[template(path = "iteration.html")]
struct IterationTemplate {
    run: Run,
    iteration: IterationPageDetail,
    plan_html: String,
    draft_html: String,
    critique_html: String,
    evaluation: Option<EvaluatorOutput>,
}

pub async fn index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let runs = state.db.list_runs(10).await.map_err(AppError::from)?;
    let run_templates = state
        .db
        .list_run_templates(200)
        .await
        .map_err(AppError::from)?;
    Ok(Html(
        IndexTemplate {
            runs,
            run_templates,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

pub async fn run_detail(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Html<String>> {
    let run = state
        .db
        .get_run(&run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    let events = state
        .db
        .list_events(&run_id)
        .await
        .map_err(AppError::from)?;
    let iterations = state
        .db
        .list_iterations(&run_id)
        .await
        .map_err(AppError::from)?
        .iter()
        .map(IterationSummary::from_iteration)
        .collect();
    let html = RunDetailTemplate {
        final_memo_html: run.final_memo_html.clone(),
        can_cancel: run.status == "queued" || run.status == "running",
        can_retry: run.status == "failed" || run.status == "cancelled",
        run,
        events,
        iterations,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(html))
}

pub async fn iteration_detail(
    Path((run_id, iteration_number)): Path<(String, i64)>,
    State(state): State<AppState>,
) -> AppResult<Html<String>> {
    let run = state
        .db
        .get_run(&run_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    let detail = state
        .db
        .get_iteration_detail(&run_id, iteration_number)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    let evaluation = detail
        .iteration
        .evaluation_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<EvaluatorOutput>(raw).ok());
    let page_detail = IterationPageDetail {
        evidence_notes: detail
            .evidence_notes
            .iter()
            .map(|note| EvidenceNoteView {
                claim_type: note.claim_type.clone(),
                note_html: render_markdown(&note.note_markdown),
            })
            .collect(),
        iteration: detail.iteration.clone(),
        search_queries: detail.search_queries.clone(),
        sources: detail.sources.clone(),
    };
    let html = IterationTemplate {
        run,
        iteration: page_detail,
        plan_html: render_markdown(detail.iteration.plan_markdown.as_deref().unwrap_or("")),
        draft_html: render_markdown(detail.iteration.draft_markdown.as_deref().unwrap_or("")),
        critique_html: render_markdown(detail.iteration.critique_markdown.as_deref().unwrap_or("")),
        evaluation,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(html))
}

#[derive(Template)]
#[template(path = "bookmarks.html")]
struct BookmarksTemplate {
    run_bookmarks: Vec<Bookmark>,
    comparison_bookmarks: Vec<Bookmark>,
    source_bookmarks: Vec<Bookmark>,
}

pub async fn bookmarks_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let bookmarks = state.db.list_bookmarks(500).await.map_err(AppError::from)?;
    let mut run_bookmarks = Vec::new();
    let mut comparison_bookmarks = Vec::new();
    let mut source_bookmarks = Vec::new();
    for bookmark in bookmarks {
        match bookmark.entity_type.as_str() {
            "run" => run_bookmarks.push(bookmark),
            "comparison" => comparison_bookmarks.push(bookmark),
            "source" => source_bookmarks.push(bookmark),
            _ => {}
        }
    }
    Ok(Html(
        BookmarksTemplate {
            run_bookmarks,
            comparison_bookmarks,
            source_bookmarks,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

#[derive(Template)]
#[template(path = "run_templates.html")]
struct RunTemplatesTemplate {
    templates: Vec<RunTemplate>,
}

pub async fn run_templates_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let templates = state
        .db
        .list_run_templates(500)
        .await
        .map_err(AppError::from)?;
    Ok(Html(
        RunTemplatesTemplate { templates }
            .render()
            .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

#[derive(Template)]
#[template(path = "batches.html")]
struct BatchesTemplate {
    batch_jobs: Vec<BatchJob>,
    run_templates: Vec<RunTemplate>,
}

#[derive(Clone)]
struct BatchRunView {
    run_id: String,
    has_run: bool,
    ticker: String,
    status: String,
    has_summary: bool,
    summary: String,
}

#[derive(Template)]
#[template(path = "batch.html")]
struct BatchTemplate {
    batch_job: BatchJob,
    batch_runs: Vec<BatchRunView>,
    has_pending_runs: bool,
}

pub async fn batches_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let batch_jobs = state.db.list_batch_jobs(25).await.map_err(AppError::from)?;
    let run_templates = state
        .db
        .list_run_templates(200)
        .await
        .map_err(AppError::from)?;
    Ok(Html(
        BatchesTemplate {
            batch_jobs,
            run_templates,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

pub async fn batch_detail(
    Path(batch_job_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Html<String>> {
    let batch_job = state
        .db
        .get_batch_job(&batch_job_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;
    let batch_job_runs = state
        .db
        .list_batch_job_runs(&batch_job_id)
        .await
        .map_err(AppError::from)?;

    let batch_runs: Vec<BatchRunView> = batch_job_runs
        .into_iter()
        .map(|batch_job_run| {
            let run_id = batch_job_run
                .run
                .as_ref()
                .map(|run| run.id.clone())
                .unwrap_or_default();
            let summary = batch_job_run
                .run
                .as_ref()
                .and_then(|run| run.summary.clone())
                .unwrap_or_default();
            BatchRunView {
                run_id,
                has_run: batch_job_run.run.is_some(),
                ticker: batch_job_run.ticker,
                status: batch_job_run
                    .run
                    .as_ref()
                    .map(|run| run.status.clone())
                    .unwrap_or_else(|| "pending".to_string()),
                has_summary: !summary.is_empty(),
                summary,
            }
        })
        .collect();
    let has_pending_runs = batch_runs.iter().any(|run| {
        run.status != "completed"
            && run.status != "failed"
            && run.status != "failed_partial"
            && run.status != "cancelled"
    });

    Ok(Html(
        BatchTemplate {
            batch_job,
            batch_runs,
            has_pending_runs,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

// Comparison templates
#[derive(Template)]
#[template(path = "comparisons.html")]
struct ComparisonsTemplate {
    comparisons: Vec<crate::models::Comparison>,
    run_templates: Vec<RunTemplate>,
}

#[derive(Clone)]
struct ComparisonRunView {
    run_id: String,
    has_run: bool,
    ticker: String,
    status: String,
    has_final_memo: bool,
    final_memo_html: String,
    has_summary: bool,
    summary: String,
}

#[derive(Template)]
#[template(path = "comparison.html")]
struct ComparisonTemplate {
    comparison: crate::models::Comparison,
    has_comparison_summary: bool,
    comparison_summary: String,
    has_final_comparison_html: bool,
    final_comparison_html: String,
    comparison_runs: Vec<ComparisonRunView>,
    has_pending_runs: bool,
}

pub async fn comparisons_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let comparisons = state
        .db
        .list_comparisons(25)
        .await
        .map_err(AppError::from)?;
    let run_templates = state
        .db
        .list_run_templates(200)
        .await
        .map_err(AppError::from)?;
    Ok(Html(
        ComparisonsTemplate {
            comparisons,
            run_templates,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

pub async fn comparison_detail(
    Path(comparison_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Html<String>> {
    let comparison = state
        .db
        .get_comparison(&comparison_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let comparison_runs = state
        .db
        .list_comparison_runs(&comparison_id)
        .await
        .map_err(AppError::from)?;
    let comparison_summary = comparison.summary.clone().unwrap_or_default();
    let final_comparison_html = comparison.final_comparison_html.clone().unwrap_or_default();

    let run_views: Vec<ComparisonRunView> = comparison_runs
        .into_iter()
        .map(|cr| {
            let run_id = cr
                .run
                .as_ref()
                .map(|run| run.id.clone())
                .unwrap_or_default();
            let final_memo_html = cr
                .run
                .as_ref()
                .and_then(|r| r.final_memo_html.clone())
                .unwrap_or_default();
            let summary = cr
                .run
                .as_ref()
                .and_then(|r| r.summary.clone())
                .unwrap_or_default();
            ComparisonRunView {
                run_id,
                has_run: cr.run.is_some(),
                ticker: cr.ticker,
                status: cr
                    .run
                    .as_ref()
                    .map(|r| r.status.clone())
                    .unwrap_or_else(|| "pending".to_string()),
                has_final_memo: !final_memo_html.is_empty(),
                final_memo_html,
                has_summary: !summary.is_empty(),
                summary,
            }
        })
        .collect();
    let has_pending_runs = run_views.iter().any(|run| {
        run.status != "completed" && run.status != "failed" && run.status != "cancelled"
    });

    let html = ComparisonTemplate {
        comparison,
        has_comparison_summary: !comparison_summary.is_empty(),
        comparison_summary,
        has_final_comparison_html: !final_comparison_html.is_empty(),
        final_comparison_html,
        comparison_runs: run_views,
        has_pending_runs,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(html))
}
