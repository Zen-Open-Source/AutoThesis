use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    markdown::render_markdown,
    models::{
        ComparisonDetail, ComparisonRunWithDetails, EvaluatorOutput, EventRecord, Iteration,
        IterationSummary, Run, SearchQueryRecord, SourceRecord,
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
}

#[derive(Template)]
#[template(path = "run.html")]
struct RunTemplate {
    run: Run,
    events: Vec<EventRecord>,
    iterations: Vec<IterationSummary>,
    final_memo_html: Option<String>,
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
    Ok(Html(
        IndexTemplate { runs }
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
    let html = RunTemplate {
        final_memo_html: run.final_memo_html.clone(),
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

// Comparison templates
#[derive(Template)]
#[template(path = "comparisons.html")]
struct ComparisonsTemplate {
    comparisons: Vec<crate::models::Comparison>,
}

#[derive(Clone)]
struct ComparisonRunView {
    ticker: String,
    status: String,
    final_memo_html: Option<String>,
    summary: Option<String>,
}

#[derive(Template)]
#[template(path = "comparison.html")]
struct ComparisonTemplate {
    comparison: crate::models::Comparison,
    comparison_runs: Vec<ComparisonRunView>,
}

pub async fn comparisons_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let comparisons = state.db.list_comparisons(25).await.map_err(AppError::from)?;
    Ok(Html(
        ComparisonsTemplate { comparisons }
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

    let run_views: Vec<ComparisonRunView> = comparison_runs
        .into_iter()
        .map(|cr| ComparisonRunView {
            ticker: cr.ticker,
            status: cr.run.as_ref().map(|r| r.status.clone()).unwrap_or_else(|| "pending".to_string()),
            final_memo_html: cr.run.as_ref().and_then(|r| r.final_memo_html.clone()),
            summary: cr.run.as_ref().and_then(|r| r.summary.clone()),
        })
        .collect();

    let html = ComparisonTemplate {
        comparison,
        comparison_runs: run_views,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(html))
}
