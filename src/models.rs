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
    pub active_alert_count: i64,
    pub last_run_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub watchlist_id: String,
    pub rule_type: String,
    pub threshold: Option<f64>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThesisAlert {
    pub id: String,
    pub watchlist_id: String,
    pub ticker: String,
    pub run_id: String,
    #[serde(rename = "type")]
    pub alert_type: String,
    pub severity: String,
    pub message: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardResponse {
    pub watchlist: Watchlist,
    pub rows: Vec<DashboardTickerRow>,
    pub active_alerts: Vec<ThesisAlert>,
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

// Scanner models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerUniverse {
    pub id: String,
    pub ticker: String,
    pub name: Option<String>,
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub market_cap_billion: Option<f64>,
    pub is_sp500: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub universe_filter: String,
    pub sector_filter: Option<String>,
    pub min_market_cap: Option<f64>,
    pub max_market_cap: Option<f64>,
    pub max_opportunities: i64,
    pub signal_weights_json: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRun {
    pub id: String,
    pub config_id: Option<String>,
    pub status: String,
    pub tickers_scanned: i64,
    pub opportunities_found: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOpportunity {
    pub id: String,
    pub scan_run_id: String,
    pub ticker: String,
    pub overall_score: f64,
    pub signal_strength_score: f64,
    pub thesis_quality_score: Option<f64>,
    pub coverage_gap_score: f64,
    pub timing_score: f64,
    pub signals_json: String,
    pub preliminary_thesis_markdown: Option<String>,
    pub preliminary_thesis_html: Option<String>,
    pub key_catalysts: Option<String>,
    pub risk_factors: Option<String>,
    pub promoted_to_run_id: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSignal {
    #[serde(rename = "type")]
    pub signal_type: String,
    pub strength: f64,
    pub description: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOpportunityDetail {
    pub opportunity: ScanOpportunity,
    pub signals: Vec<ScanSignal>,
    pub ticker_info: Option<TickerUniverse>,
    pub existing_run: Option<Run>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRunDetail {
    pub scan_run: ScanRun,
    pub opportunities: Vec<ScanOpportunity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerDashboard {
    pub latest_scan_run: Option<ScanRun>,
    pub top_opportunities: Vec<ScanOpportunity>,
    pub total_tickers_in_universe: i64,
    pub active_config: Option<ScannerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScanRunRequest {
    pub config_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScanRunResponse {
    pub scan_run_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteOpportunityRequest {
    pub question: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteOpportunityResponse {
    pub run_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalDetectorOutput {
    pub ticker: String,
    pub signals: Vec<ScanSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreliminaryThesisOutput {
    pub thesis_markdown: String,
    pub key_catalysts: String,
    pub risk_factors: String,
    pub quality_score: f64,
}

// Multi-Model Research Panel models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProvider {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub api_key_encrypted: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
    pub is_enabled: bool,
    pub is_default: bool,
    pub priority: i64,
    pub config_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRun {
    pub id: String,
    pub run_id: String,
    pub provider_id: String,
    pub iteration_number: Option<i64>,
    pub output_type: String,
    pub output_content: Option<String>,
    pub tokens_used: Option<i64>,
    pub latency_ms: Option<i64>,
    pub cost_estimate: Option<f64>,
    pub quality_score: Option<f64>,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelComparison {
    pub id: String,
    pub run_id: String,
    pub comparison_type: String,
    pub winner_provider_id: Option<String>,
    pub comparison_json: String,
    pub similarity_score: Option<f64>,
    pub key_differences: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQualityScore {
    pub id: String,
    pub provider_id: String,
    pub total_runs: i64,
    pub successful_runs: i64,
    pub avg_quality_score: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub accuracy_score: Option<f64>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLlmProviderRequest {
    pub name: String,
    pub provider_type: String,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
    pub is_enabled: Option<bool>,
    pub is_default: Option<bool>,
    pub priority: Option<i64>,
    pub config_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLlmProviderRequest {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub is_enabled: Option<bool>,
    pub is_default: Option<bool>,
    pub priority: Option<i64>,
    pub config_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModelRunRequest {
    pub provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiModelRunResponse {
    pub run_id: String,
    pub model_runs: Vec<ModelRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelComparisonDetail {
    pub comparison: ModelComparison,
    pub model_runs: Vec<ModelRun>,
    pub providers: Vec<LlmProvider>,
}

// Thesis Performance Tracking models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSnapshot {
    pub id: String,
    pub ticker: String,
    pub price_date: chrono::NaiveDate,
    pub open_price: f64,
    pub close_price: f64,
    pub high_price: Option<f64>,
    pub low_price: Option<f64>,
    pub volume: Option<i64>,
    pub adjusted_close: Option<f64>,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThesisOutcome {
    pub id: String,
    pub run_id: String,
    pub ticker: String,
    pub thesis_date: chrono::NaiveDate,
    pub thesis_price: f64,
    pub return_1d: Option<f64>,
    pub return_7d: Option<f64>,
    pub return_30d: Option<f64>,
    pub return_90d: Option<f64>,
    pub return_180d: Option<f64>,
    pub return_365d: Option<f64>,
    pub price_1d: Option<f64>,
    pub price_7d: Option<f64>,
    pub price_30d: Option<f64>,
    pub price_90d: Option<f64>,
    pub price_180d: Option<f64>,
    pub price_365d: Option<f64>,
    pub thesis_direction: Option<String>,
    pub thesis_correct_1d: Option<bool>,
    pub thesis_correct_7d: Option<bool>,
    pub thesis_correct_30d: Option<bool>,
    pub thesis_correct_90d: Option<bool>,
    pub notes: Option<String>,
    pub last_updated: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThesisAccuracy {
    pub id: String,
    pub ticker: Option<String>,
    pub provider_id: Option<String>,
    pub time_horizon: String,
    pub total_theses: i64,
    pub correct_theses: i64,
    pub accuracy_rate: Option<f64>,
    pub avg_return: Option<f64>,
    pub median_return: Option<f64>,
    pub best_return: Option<f64>,
    pub worst_return: Option<f64>,
    pub sharpe_ratio: Option<f64>,
    pub win_rate: Option<f64>,
    pub avg_holding_days: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceTrackingJob {
    pub id: String,
    pub job_type: String,
    pub target_date: chrono::NaiveDate,
    pub tickers_json: String,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub prices_fetched: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceDashboard {
    pub overall_accuracy: Option<f64>,
    pub total_theses_tracked: i64,
    pub accuracy_by_horizon: Vec<ThesisAccuracy>,
    pub accuracy_by_ticker: Vec<ThesisAccuracy>,
    pub accuracy_by_model: Vec<ThesisAccuracy>,
    pub recent_outcomes: Vec<ThesisOutcome>,
    pub top_performers: Vec<ThesisOutcome>,
    pub worst_performers: Vec<ThesisOutcome>,
}

// Evidence Quality Scoring models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReputation {
    pub id: String,
    pub domain: String,
    pub reputation_score: f64,
    pub total_citations: i64,
    pub successful_citations: i64,
    pub failed_citations: i64,
    pub avg_evidence_quality: Option<f64>,
    pub source_type: Option<String>,
    pub bias_rating: Option<String>,
    pub reliability_tier: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceOutcome {
    pub id: String,
    pub evidence_note_id: String,
    pub run_id: String,
    pub ticker: String,
    pub claim_type: Option<String>,
    pub claim_text: Option<String>,
    pub outcome_type: String,
    pub outcome_date: chrono::NaiveDate,
    pub price_at_claim: Option<f64>,
    pub price_at_outcome: Option<f64>,
    pub return_since_claim: Option<f64>,
    pub was_correct: bool,
    pub confidence_at_claim: Option<f64>,
    pub outcome_notes: Option<String>,
    pub verified_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQualityMetric {
    pub id: String,
    pub source_id: String,
    pub domain: Option<String>,
    pub quality_score: Option<f64>,
    pub relevance_score: Option<f64>,
    pub timeliness_score: Option<f64>,
    pub authority_score: Option<f64>,
    pub citation_count: i64,
    pub last_cited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainReliabilityHistory {
    pub id: String,
    pub domain: String,
    pub recorded_date: chrono::NaiveDate,
    pub reliability_score: f64,
    pub sample_size: i64,
    pub success_rate: Option<f64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEvidenceOutcomeRequest {
    pub evidence_note_id: String,
    pub outcome_type: String,
    pub outcome_date: chrono::NaiveDate,
    pub was_correct: bool,
    pub outcome_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceQualityDashboard {
    pub top_domains: Vec<SourceReputation>,
    pub worst_domains: Vec<SourceReputation>,
    pub recent_evidence_outcomes: Vec<EvidenceOutcome>,
    pub overall_success_rate: Option<f64>,
    pub total_evidence_tracked: i64,
}

// Historical Analytics models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThesisHistory {
    pub id: String,
    pub run_id: String,
    pub ticker: String,
    pub thesis_date: chrono::NaiveDate,
    pub thesis_markdown: String,
    pub thesis_html: Option<String>,
    pub executive_summary: Option<String>,
    pub bull_case: Option<String>,
    pub bear_case: Option<String>,
    pub key_catalysts: Option<String>,
    pub key_risks: Option<String>,
    pub conviction_level: Option<String>,
    pub thesis_direction: Option<String>,
    pub model_provider_id: Option<String>,
    pub signals_json: Option<String>,
    pub iteration_number: Option<i64>,
    pub archived_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEffectiveness {
    pub id: String,
    pub signal_type: String,
    pub signal_date: chrono::NaiveDate,
    pub ticker: String,
    pub signal_strength: f64,
    pub signal_description: Option<String>,
    pub outcome_type: Option<String>,
    pub return_7d: Option<f64>,
    pub return_30d: Option<f64>,
    pub return_90d: Option<f64>,
    pub was_predictive: Option<bool>,
    pub thesis_run_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEffectivenessStats {
    pub id: String,
    pub signal_type: String,
    pub total_signals: i64,
    pub predictive_signals: i64,
    pub predictive_rate: Option<f64>,
    pub avg_return_7d: Option<f64>,
    pub avg_return_30d: Option<f64>,
    pub avg_return_90d: Option<f64>,
    pub best_return_90d: Option<f64>,
    pub worst_return_90d: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchAnalytics {
    pub id: String,
    pub analytics_date: chrono::NaiveDate,
    pub total_runs: i64,
    pub total_theses: i64,
    pub avg_conviction: Option<f64>,
    pub avg_iteration_count: Option<f64>,
    pub avg_source_count: Option<f64>,
    pub avg_evidence_count: Option<f64>,
    pub avg_quality_score: Option<f64>,
    pub thesis_accuracy_30d: Option<f64>,
    pub thesis_accuracy_90d: Option<f64>,
    pub top_performing_ticker: Option<String>,
    pub worst_performing_ticker: Option<String>,
    pub best_model_provider_id: Option<String>,
    pub model_accuracy_ranking_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerResearchSummary {
    pub id: String,
    pub ticker: String,
    pub first_research_date: Option<chrono::NaiveDate>,
    pub last_research_date: Option<chrono::NaiveDate>,
    pub total_research_runs: i64,
    pub avg_conviction: Option<f64>,
    pub avg_quality_score: Option<f64>,
    pub thesis_accuracy_30d: Option<f64>,
    pub thesis_accuracy_90d: Option<f64>,
    pub total_return_all_time: Option<f64>,
    pub best_return_90d: Option<f64>,
    pub worst_return_90d: Option<f64>,
    pub research_frequency: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformanceHistory {
    pub id: String,
    pub provider_id: String,
    pub recorded_date: chrono::NaiveDate,
    pub total_runs: i64,
    pub successful_runs: i64,
    pub avg_quality_score: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub accuracy_score: Option<f64>,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsDashboard {
    pub latest_analytics: Option<ResearchAnalytics>,
    pub signal_effectiveness: Vec<SignalEffectivenessStats>,
    pub top_tickers: Vec<TickerResearchSummary>,
    pub model_rankings: Vec<ModelQualityScore>,
    pub recent_thesis_history: Vec<ThesisHistory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerHistoryDetail {
    pub summary: TickerResearchSummary,
    pub thesis_history: Vec<ThesisHistory>,
    pub outcomes: Vec<ThesisOutcome>,
    pub signal_history: Vec<SignalEffectiveness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistSchedule {
    pub watchlist_id: String,
    pub refresh_enabled: bool,
    pub refresh_interval_hours: i64,
    pub last_refresh_at: Option<DateTime<Utc>>,
    pub next_refresh_at: Option<DateTime<Utc>>,
    pub refresh_template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledRun {
    pub id: String,
    pub watchlist_id: String,
    pub ticker: String,
    pub run_id: String,
    pub scheduled_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWatchlistScheduleRequest {
    pub enabled: bool,
    pub interval_hours: i64,
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistScheduleResponse {
    pub watchlist: Watchlist,
    pub schedule: WatchlistSchedule,
    pub scheduled_runs: Vec<ScheduledRun>,
}
