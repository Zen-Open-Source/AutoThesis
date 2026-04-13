use crate::{
    app_state::AppState,
    error::{AppError, AppResult},
    markdown::render_markdown,
    models::{
        BatchJob, Bookmark, EvaluatorOutput, EventRecord, Iteration, IterationSummary, Run,
        RunTemplate, SearchQueryRecord, SourceRecord, Watchlist,
    },
    services::dashboard,
};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::Html,
};
use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
pub struct DashboardPageQuery {
    pub watchlist_id: Option<String>,
}

#[derive(Clone)]
struct DashboardRowView {
    ticker: String,
    latest_status: String,
    latest_score_text: String,
    score_delta_text: String,
    trend: String,
    evidence_freshness: String,
    decision_state: String,
    has_summary: bool,
    summary: String,
    has_latest_run: bool,
    latest_run_id: String,
    active_alert_count: i64,
    last_run_updated_at_text: String,
}

#[derive(Clone)]
struct DashboardAlertView {
    id: String,
    ticker: String,
    alert_type: String,
    severity: String,
    message: String,
    created_at_text: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    watchlists: Vec<Watchlist>,
    run_templates: Vec<RunTemplate>,
    has_selected_watchlist: bool,
    selected_watchlist_id: String,
    selected_watchlist_name: String,
    generated_at: String,
    active_alerts: Vec<DashboardAlertView>,
    rows: Vec<DashboardRowView>,
    schedule_enabled: bool,
    schedule_interval_hours: i64,
    schedule_template_id: String,
    last_refresh_at: String,
    next_refresh_at: String,
}

pub async fn dashboard_index(
    State(state): State<AppState>,
    Query(query): Query<DashboardPageQuery>,
) -> AppResult<Html<String>> {
    let watchlists = state
        .db
        .list_watchlists(200)
        .await
        .map_err(AppError::from)?;
    let run_templates = state
        .db
        .list_run_templates(200)
        .await
        .map_err(AppError::from)?;

    if watchlists.is_empty() {
        let html = DashboardTemplate {
            watchlists,
            run_templates,
            has_selected_watchlist: false,
            selected_watchlist_id: String::new(),
            selected_watchlist_name: String::new(),
            generated_at: String::new(),
            active_alerts: Vec::new(),
            rows: Vec::new(),
            schedule_enabled: false,
            schedule_interval_hours: 168,
            schedule_template_id: String::new(),
            last_refresh_at: String::new(),
            next_refresh_at: String::new(),
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?;
        return Ok(Html(html));
    }

    let selected_watchlist = query
        .watchlist_id
        .as_deref()
        .and_then(|watchlist_id| {
            watchlists
                .iter()
                .find(|watchlist| watchlist.id == watchlist_id)
        })
        .cloned()
        .unwrap_or_else(|| watchlists[0].clone());

    let dashboard = dashboard::build_watchlist_dashboard(&state, &selected_watchlist.id)
        .await
        .map_err(AppError::from)?;

    let rows = dashboard
        .rows
        .into_iter()
        .map(|row| DashboardRowView {
            ticker: row.ticker,
            latest_status: row.latest_status,
            latest_score_text: format_optional_score(row.latest_score),
            score_delta_text: format_optional_delta(row.score_delta),
            trend: row.trend,
            evidence_freshness: row.evidence_freshness,
            decision_state: row.decision_state,
            has_summary: row
                .summary
                .as_ref()
                .map(|summary| !summary.is_empty())
                .unwrap_or(false),
            summary: row.summary.unwrap_or_default(),
            has_latest_run: row.latest_run_id.is_some(),
            latest_run_id: row.latest_run_id.unwrap_or_default(),
            active_alert_count: row.active_alert_count,
            last_run_updated_at_text: row
                .last_run_updated_at
                .map(|timestamp| timestamp.to_rfc3339())
                .unwrap_or_else(|| "n/a".to_string()),
        })
        .collect::<Vec<_>>();
    let active_alerts = dashboard
        .active_alerts
        .into_iter()
        .map(|alert| DashboardAlertView {
            id: alert.id,
            ticker: alert.ticker,
            alert_type: alert.alert_type,
            severity: alert.severity,
            message: alert.message,
            created_at_text: alert.created_at.to_rfc3339(),
        })
        .collect::<Vec<_>>();

    // Fetch schedule data
    let schedule = state
        .db
        .get_watchlist_schedule(&selected_watchlist.id)
        .await
        .map_err(AppError::from)?;

    let (
        schedule_enabled,
        schedule_interval_hours,
        schedule_template_id,
        last_refresh_at,
        next_refresh_at,
    ) = if let Some(s) = schedule {
        (
            s.refresh_enabled,
            s.refresh_interval_hours,
            s.refresh_template_id.unwrap_or_default(),
            s.last_refresh_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
            s.next_refresh_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        )
    } else {
        (false, 168, String::new(), String::new(), String::new())
    };

    let html = DashboardTemplate {
        watchlists,
        run_templates,
        has_selected_watchlist: true,
        selected_watchlist_id: selected_watchlist.id,
        selected_watchlist_name: selected_watchlist.name,
        generated_at: dashboard.generated_at.to_rfc3339(),
        active_alerts,
        rows,
        schedule_enabled,
        schedule_interval_hours,
        schedule_template_id,
        last_refresh_at,
        next_refresh_at,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;

    Ok(Html(html))
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

fn format_optional_score(score: Option<f64>) -> String {
    score
        .map(|value| format!("{value:.1}"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_optional_delta(score_delta: Option<f64>) -> String {
    score_delta
        .map(|value| format!("{value:+.1}"))
        .unwrap_or_else(|| "n/a".to_string())
}

// Scanner pages

#[derive(Clone)]
struct ScannerOpportunityView {
    id: String,
    ticker: String,
    overall_score_text: String,
    signal_strength_text: String,
    coverage_gap_text: String,
    timing_score_text: String,
    has_key_catalysts: bool,
    key_catalysts: String,
    has_risk_factors: bool,
    risk_factors: String,
    status: String,
    has_promoted_run: bool,
    #[allow(dead_code)]
    promoted_to_run_id: String,
}

#[derive(Template)]
#[template(path = "scanner.html")]
struct ScannerTemplate {
    has_latest_scan_run: bool,
    #[allow(dead_code)]
    latest_scan_run_id: String,
    latest_scan_run_status: String,
    latest_scan_run_tickers_scanned: i64,
    latest_scan_run_opportunities_found: i64,
    latest_scan_run_started_at_text: String,
    has_latest_scan_run_completed_at: bool,
    latest_scan_run_completed_at_text: String,
    has_latest_scan_run_error: bool,
    latest_scan_run_error_message: String,
    top_opportunities: Vec<ScannerOpportunityView>,
    total_tickers_in_universe: i64,
    has_scan_running: bool,
}

#[derive(Clone)]
struct ScanSignalView {
    signal_type: String,
    strength_text: String,
    description: String,
    evidence: Vec<String>,
}

#[derive(Template)]
#[template(path = "scanner_opportunity.html")]
struct ScannerOpportunityTemplate {
    opportunity: ScannerOpportunityDetailView,
    signals: Vec<ScanSignalView>,
    has_thesis: bool,
    thesis_html: String,
    has_ticker_name: bool,
    ticker_name: String,
    has_ticker_sector: bool,
    ticker_sector: String,
    has_existing_run: bool,
    existing_run_id: String,
    existing_run_status: String,
}

#[derive(Clone)]
struct ScannerOpportunityDetailView {
    id: String,
    ticker: String,
    overall_score_text: String,
    signal_strength_text: String,
    thesis_quality_text: String,
    coverage_gap_text: String,
    timing_score_text: String,
    status: String,
    has_promoted_run: bool,
    promoted_to_run_id: String,
}

pub async fn scanner_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let dashboard = crate::services::scanner::build_scanner_dashboard(&state)
        .await
        .map_err(AppError::from)?;

    let (
        has_latest_scan_run,
        latest_scan_run_id,
        latest_scan_run_status,
        latest_scan_run_tickers_scanned,
        latest_scan_run_opportunities_found,
        latest_scan_run_started_at_text,
        has_latest_scan_run_completed_at,
        latest_scan_run_completed_at_text,
        has_latest_scan_run_error,
        latest_scan_run_error_message,
    ) = if let Some(run) = dashboard.latest_scan_run {
        (
            true,
            run.id,
            run.status.clone(),
            run.tickers_scanned,
            run.opportunities_found,
            run.started_at.map(format_timestamp).unwrap_or_default(),
            run.completed_at.is_some(),
            run.completed_at.map(format_timestamp).unwrap_or_default(),
            run.error_message.is_some(),
            run.error_message.unwrap_or_default(),
        )
    } else {
        (
            false,
            String::new(),
            String::new(),
            0,
            0,
            String::new(),
            false,
            String::new(),
            false,
            String::new(),
        )
    };

    let top_opportunities: Vec<ScannerOpportunityView> = dashboard
        .top_opportunities
        .into_iter()
        .map(|opp| ScannerOpportunityView {
            id: opp.id,
            ticker: opp.ticker,
            overall_score_text: format_optional_score(Some(opp.overall_score)),
            signal_strength_text: format_optional_score(Some(opp.signal_strength_score)),
            coverage_gap_text: format_optional_score(Some(opp.coverage_gap_score)),
            timing_score_text: format_optional_score(Some(opp.timing_score)),
            has_key_catalysts: opp.key_catalysts.is_some(),
            key_catalysts: opp.key_catalysts.unwrap_or_default(),
            has_risk_factors: opp.risk_factors.is_some(),
            risk_factors: opp.risk_factors.unwrap_or_default(),
            status: opp.status,
            has_promoted_run: opp.promoted_to_run_id.is_some(),
            promoted_to_run_id: opp.promoted_to_run_id.unwrap_or_default(),
        })
        .collect();

    let has_scan_running = has_latest_scan_run
        && (latest_scan_run_status == "running" || latest_scan_run_status == "queued");

    let html = ScannerTemplate {
        has_latest_scan_run,
        latest_scan_run_id,
        latest_scan_run_status,
        latest_scan_run_tickers_scanned,
        latest_scan_run_opportunities_found,
        latest_scan_run_started_at_text,
        has_latest_scan_run_completed_at,
        latest_scan_run_completed_at_text,
        has_latest_scan_run_error,
        latest_scan_run_error_message,
        top_opportunities,
        total_tickers_in_universe: dashboard.total_tickers_in_universe,
        has_scan_running,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(html))
}

pub async fn scanner_opportunity_detail(
    Path(opportunity_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Html<String>> {
    let opportunity = state
        .db
        .get_scan_opportunity(&opportunity_id)
        .await
        .map_err(AppError::from)?
        .ok_or(AppError::NotFound)?;

    let signals: Vec<crate::models::ScanSignal> =
        serde_json::from_str(&opportunity.signals_json).unwrap_or_default();

    let signal_views: Vec<ScanSignalView> = signals
        .into_iter()
        .map(|s| ScanSignalView {
            signal_type: s.signal_type,
            strength_text: format_optional_score(Some(s.strength * 10.0)),
            description: s.description,
            evidence: s.evidence,
        })
        .collect();

    let ticker_info = state
        .db
        .get_ticker_universe(&opportunity.ticker)
        .await
        .map_err(AppError::from)?;

    let existing_run = if let Some(ref run_id) = opportunity.promoted_to_run_id {
        state.db.get_run(run_id).await.map_err(AppError::from)?
    } else {
        None
    };

    let opportunity_view = ScannerOpportunityDetailView {
        id: opportunity.id.clone(),
        ticker: opportunity.ticker.clone(),
        overall_score_text: format_optional_score(Some(opportunity.overall_score)),
        signal_strength_text: format_optional_score(Some(opportunity.signal_strength_score)),
        thesis_quality_text: format_optional_score(opportunity.thesis_quality_score),
        coverage_gap_text: format_optional_score(Some(opportunity.coverage_gap_score)),
        timing_score_text: format_optional_score(Some(opportunity.timing_score)),
        status: opportunity.status.clone(),
        has_promoted_run: opportunity.promoted_to_run_id.is_some(),
        promoted_to_run_id: opportunity.promoted_to_run_id.clone().unwrap_or_default(),
    };

    let has_thesis = opportunity.preliminary_thesis_html.is_some();
    let thesis_html = opportunity.preliminary_thesis_html.unwrap_or_default();

    let (has_ticker_name, ticker_name, has_ticker_sector, ticker_sector) =
        if let Some(ref info) = ticker_info {
            (
                info.name.is_some(),
                info.name.clone().unwrap_or_default(),
                info.sector.is_some(),
                info.sector.clone().unwrap_or_default(),
            )
        } else {
            (false, String::new(), false, String::new())
        };

    let (has_existing_run, existing_run_id, existing_run_status) = if let Some(run) = existing_run {
        (true, run.id, run.status)
    } else {
        (false, String::new(), String::new())
    };

    let html = ScannerOpportunityTemplate {
        opportunity: opportunity_view,
        signals: signal_views,
        has_thesis,
        thesis_html,
        has_ticker_name,
        ticker_name,
        has_ticker_sector,
        ticker_sector,
        has_existing_run,
        existing_run_id,
        existing_run_status,
    }
    .render()
    .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(html))
}

fn format_timestamp(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M UTC").to_string()
}

// Portfolio pages

#[derive(Template)]
#[template(path = "portfolios.html")]
struct PortfoliosTemplate {
    portfolios: Vec<PortfolioView>,
}

#[derive(Clone)]
struct PortfolioView {
    id: String,
    name: String,
    has_description: bool,
    description: String,
    position_count: i32,
    cash_balance_text: String,
    updated_at_text: String,
}

#[derive(Template)]
#[template(path = "portfolio.html")]
struct PortfolioDetailTemplate {
    portfolio: PortfolioView,
    positions: Vec<PositionView>,
    summary: PortfolioSummaryView,
    transactions: Vec<TransactionView>,
}

#[derive(Clone)]
struct PositionView {
    id: String,
    ticker: String,
    shares_text: String,
    cost_basis_text: String,
    total_cost_text: String,
    current_price_text: String,
    market_value_text: String,
    gain_loss_text: String,
    gain_loss_pct_text: String,
    allocation_pct_text: String,
    conviction_text: String,
    conviction_alignment: String,
    conviction_badge_class: String,
    has_latest_run: bool,
    latest_run_id: String,
    latest_run_status: String,
    opened_at_text: String,
    has_notes: bool,
    notes: String,
}

#[derive(Clone)]
struct PortfolioSummaryView {
    total_market_value_text: String,
    total_cost_text: String,
    total_gain_loss_text: String,
    total_gain_loss_pct_text: String,
    cash_balance_text: String,
    total_value_text: String,
}

#[derive(Clone)]
struct TransactionView {
    id: String,
    ticker: String,
    transaction_type: String,
    shares_text: String,
    price_per_share_text: String,
    total_amount_text: String,
    executed_at_text: String,
    has_notes: bool,
    notes: String,
}

pub async fn portfolios_index(State(state): State<AppState>) -> AppResult<Html<String>> {
    let portfolios = state
        .db
        .list_portfolios(100)
        .await
        .map_err(AppError::from)?;

    let mut portfolio_views = Vec::new();
    for portfolio in portfolios {
        let positions = state
            .db
            .list_active_positions(&portfolio.id)
            .await
            .map_err(AppError::from)?;
        portfolio_views.push(PortfolioView {
            id: portfolio.id,
            name: portfolio.name,
            has_description: portfolio.description.is_some(),
            description: portfolio.description.unwrap_or_default(),
            position_count: positions.len() as i32,
            cash_balance_text: format_currency(portfolio.cash_balance),
            updated_at_text: format_timestamp(portfolio.updated_at),
        });
    }

    Ok(Html(
        PortfoliosTemplate {
            portfolios: portfolio_views,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

pub async fn portfolio_detail(
    Path(portfolio_id): Path<String>,
    State(state): State<AppState>,
) -> AppResult<Html<String>> {
    let detail = crate::services::portfolio::build_portfolio_detail(&state, &portfolio_id)
        .await
        .map_err(AppError::from)?;

    let portfolio_view = PortfolioView {
        id: detail.portfolio.id.clone(),
        name: detail.portfolio.name.clone(),
        has_description: detail.portfolio.description.is_some(),
        description: detail.portfolio.description.clone().unwrap_or_default(),
        position_count: detail.positions.len() as i32,
        cash_balance_text: format_currency(detail.portfolio.cash_balance),
        updated_at_text: format_timestamp(detail.portfolio.updated_at),
    };

    let position_views: Vec<PositionView> = detail
        .positions
        .into_iter()
        .map(|p| {
            let conviction_badge_class = match p.conviction_alignment.as_str() {
                "aligned" => "badge-success",
                "moderate" => "badge-warning",
                "mismatch" => "badge-danger",
                "low_conviction" => "badge-danger",
                _ => "badge-secondary",
            };
            PositionView {
                id: p.position.id,
                ticker: p.position.ticker,
                shares_text: format_shares(p.position.shares),
                cost_basis_text: format_currency(p.position.cost_basis_per_share),
                total_cost_text: format_currency(p.position.total_cost),
                current_price_text: p
                    .current_price
                    .map(format_currency)
                    .unwrap_or_else(|| "-".to_string()),
                market_value_text: p
                    .market_value
                    .map(format_currency)
                    .unwrap_or_else(|| "-".to_string()),
                gain_loss_text: p
                    .gain_loss
                    .map(|g| format_currency_signed(g))
                    .unwrap_or_else(|| "-".to_string()),
                gain_loss_pct_text: p
                    .gain_loss_pct
                    .map(|g| format!("{:.1}%", g))
                    .unwrap_or_else(|| "-".to_string()),
                allocation_pct_text: p
                    .allocation_pct
                    .map(|a| format!("{:.1}%", a))
                    .unwrap_or_else(|| "-".to_string()),
                conviction_text: p
                    .latest_conviction
                    .map(|c| format!("{:.1}/10", c))
                    .unwrap_or_else(|| "No thesis".to_string()),
                conviction_alignment: p.conviction_alignment.clone(),
                conviction_badge_class: conviction_badge_class.to_string(),
                has_latest_run: p.latest_run_id.is_some(),
                latest_run_id: p.latest_run_id.unwrap_or_default(),
                latest_run_status: p.latest_run_status.unwrap_or_default(),
                opened_at_text: p.position.opened_at.to_string(),
                has_notes: p.position.notes.is_some(),
                notes: p.position.notes.unwrap_or_default(),
            }
        })
        .collect();

    let summary_view = PortfolioSummaryView {
        total_market_value_text: format_currency(detail.summary.total_market_value),
        total_cost_text: format_currency(detail.summary.total_cost),
        total_gain_loss_text: format_currency_signed(detail.summary.total_gain_loss),
        total_gain_loss_pct_text: format!("{:.1}%", detail.summary.total_gain_loss_pct),
        cash_balance_text: format_currency(detail.summary.cash_balance),
        total_value_text: format_currency(detail.summary.total_value),
    };

    let transaction_views: Vec<TransactionView> = detail
        .recent_transactions
        .into_iter()
        .map(|t| TransactionView {
            id: t.id,
            ticker: t.ticker,
            transaction_type: t.transaction_type,
            shares_text: format_shares(t.shares),
            price_per_share_text: format_currency(t.price_per_share),
            total_amount_text: format_currency(t.total_amount),
            executed_at_text: t.executed_at.to_string(),
            has_notes: t.notes.is_some(),
            notes: t.notes.unwrap_or_default(),
        })
        .collect();

    Ok(Html(
        PortfolioDetailTemplate {
            portfolio: portfolio_view,
            positions: position_views,
            summary: summary_view,
            transactions: transaction_views,
        }
        .render()
        .map_err(|error| AppError::Internal(error.into()))?,
    ))
}

fn format_currency(amount: f64) -> String {
    format!("${:.2}", amount)
}

fn format_currency_signed(amount: f64) -> String {
    if amount >= 0.0 {
        format!("+${:.2}", amount)
    } else {
        format!("-${:.2}", amount.abs())
    }
}

fn format_shares(shares: f64) -> String {
    if shares == shares.floor() {
        format!("{:.0}", shares)
    } else {
        format!("{:.2}", shares)
    }
}
