use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub ticker: String,
    pub question: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub final_iteration_number: Option<i64>,
    pub final_memo_markdown: Option<String>,
    pub final_memo_html: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Iteration {
    pub id: String,
    pub run_id: String,
    pub iteration_number: i64,
    pub status: String,
    pub plan_markdown: Option<String>,
    pub draft_markdown: Option<String>,
    pub critique_markdown: Option<String>,
    pub evaluation_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueryRecord {
    pub id: String,
    pub iteration_id: String,
    pub query_text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultRecord {
    pub id: String,
    pub iteration_id: String,
    pub query_id: String,
    pub title: Option<String>,
    pub url: String,
    pub snippet: Option<String>,
    pub rank_score: Option<f64>,
    pub source_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: String,
    pub run_id: String,
    pub iteration_id: Option<String>,
    pub url: String,
    pub title: Option<String>,
    pub domain: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub source_type: Option<String>,
    pub raw_text: Option<String>,
    pub excerpt: Option<String>,
    pub quality_score: Option<f64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceNoteRecord {
    pub id: String,
    pub iteration_id: String,
    pub source_id: String,
    pub note_markdown: String,
    pub claim_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub run_id: String,
    pub iteration_id: Option<String>,
    pub event_type: String,
    pub message: String,
    pub payload_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunRequest {
    pub ticker: String,
    pub question: Option<String>,
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunResponse {
    pub run_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub id: String,
    pub name: String,
    pub question: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub final_comparison_html: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonRun {
    pub id: String,
    pub comparison_id: String,
    pub run_id: String,
    pub ticker: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonRunWithDetails {
    pub id: String,
    pub comparison_id: String,
    pub run_id: String,
    pub ticker: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub run: Option<Run>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateComparisonRequest {
    pub name: String,
    pub tickers: Vec<String>,
    pub question: Option<String>,
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateComparisonResponse {
    pub comparison_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonDetail {
    pub comparison: Comparison,
    pub comparison_runs: Vec<ComparisonRunWithDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJob {
    pub id: String,
    pub name: String,
    pub question_template: String,
    pub status: String,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJobRun {
    pub id: String,
    pub batch_job_id: String,
    pub run_id: String,
    pub ticker: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJobRunWithDetails {
    pub id: String,
    pub batch_job_id: String,
    pub run_id: String,
    pub ticker: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub run: Option<Run>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchJobDetail {
    pub batch_job: BatchJob,
    pub batch_job_runs: Vec<BatchJobRunWithDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watchlist {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistTicker {
    pub id: String,
    pub watchlist_id: String,
    pub ticker: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistDetail {
    pub watchlist: Watchlist,
    pub tickers: Vec<WatchlistTicker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWatchlistRequest {
    pub name: String,
    pub tickers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWatchlistRequest {
    pub name: String,
    pub tickers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddWatchlistTickerRequest {
    pub ticker: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardTickerRow {
    pub ticker: String,
    pub latest_run_id: Option<String>,
    pub latest_status: String,
    pub latest_score: Option<f64>,
    pub previous_score: Option<f64>,
    pub score_delta: Option<f64>,
    pub trend: String,
    pub summary: Option<String>,
    pub evidence_freshness: String,
    pub decision_state: String,
    pub last_run_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardResponse {
    pub watchlist: Watchlist,
    pub rows: Vec<DashboardTickerRow>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBatchJobRequest {
    pub name: String,
    pub tickers: Vec<String>,
    pub question_template: Option<String>,
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBatchJobResponse {
    pub batch_job_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTemplate {
    pub id: String,
    pub name: String,
    pub question_template: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunTemplateRequest {
    pub name: String,
    pub question_template: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRunTemplateRequest {
    pub name: String,
    pub question_template: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardRefreshRequest {
    pub watchlist_id: String,
    pub ticker: String,
    pub template_id: Option<String>,
    pub question: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub note: Option<String>,
    pub target_path: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBookmarkRequest {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub note: Option<String>,
    pub target_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAnnotation {
    pub id: String,
    pub source_id: String,
    pub run_id: String,
    pub selected_text: String,
    pub annotation_markdown: String,
    pub tag: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSourceAnnotationRequest {
    pub run_id: String,
    pub selected_text: String,
    pub annotation_markdown: String,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalMemoResponse {
    pub run_id: String,
    pub status: String,
    pub final_iteration_number: Option<i64>,
    pub final_memo_markdown: Option<String>,
    pub final_memo_html: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationSummary {
    pub iteration_number: i64,
    pub status: String,
    pub evaluation_score: Option<f64>,
    pub created_at: DateTime<Utc>,
}

impl IterationSummary {
    pub fn from_iteration(iteration: &Iteration) -> Self {
        let evaluation_score = iteration
            .evaluation_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<EvaluatorOutput>(raw).ok())
            .map(|evaluation| evaluation.score);

        Self {
            iteration_number: iteration.iteration_number,
            status: iteration.status.clone(),
            evaluation_score,
            created_at: iteration.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationDetail {
    pub iteration: Iteration,
    pub search_queries: Vec<SearchQueryRecord>,
    pub search_results: Vec<SearchResultRecord>,
    pub sources: Vec<SourceRecord>,
    pub evidence_notes: Vec<EvidenceNoteRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerOutput {
    pub research_goal: String,
    pub subquestions: Vec<String>,
    pub evidence_needed: Vec<String>,
    pub priority_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueryOutput {
    pub queries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceNoteInput {
    pub source_id: String,
    pub note_markdown: String,
    pub claim_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderOutput {
    pub notes: Vec<EvidenceNoteInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationRubric {
    pub evidence_coverage: f64,
    pub source_quality: f64,
    pub balance: f64,
    pub specificity: f64,
    pub decision_usefulness: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatorOutput {
    pub improved: bool,
    pub score: f64,
    pub rubric: EvaluationRubric,
    pub reasoning: String,
    #[serde(rename = "continue")]
    pub should_continue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonEnvelope {
    pub value: Value,
}
