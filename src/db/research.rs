//! Research analytics: per-ticker research summaries, evidence outcomes, and
//! daily research analytics snapshots.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{EvidenceOutcome, ResearchAnalytics, TickerResearchSummary};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    pub async fn create_evidence_outcome(
        &self,
        evidence_note_id: &str,
        run_id: &str,
        ticker: &str,
        outcome_type: &str,
        outcome_date: chrono::NaiveDate,
        was_correct: bool,
    ) -> Result<EvidenceOutcome> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO evidence_outcomes (id, evidence_note_id, run_id, ticker, outcome_type, outcome_date, was_correct, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, evidence_note_id, run_id, ticker, outcome_type, outcome_date.to_string(), if was_correct { 1 } else { 0 }, encode_time(now)],
        )?;
        self.get_evidence_outcome(&id)
            .await?
            .context("evidence outcome missing")
    }

    pub async fn get_evidence_outcome(&self, id: &str) -> Result<Option<EvidenceOutcome>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM evidence_outcomes WHERE id = ?1",
            [id],
            map_evidence_outcome,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn upsert_ticker_research_summary(
        &self,
        ticker: &str,
        total_research_runs: i64,
        avg_quality_score: Option<f64>,
        thesis_accuracy_90d: Option<f64>,
    ) -> Result<TickerResearchSummary> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO ticker_research_summary (id, ticker, total_research_runs, avg_quality_score, thesis_accuracy_90d, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(ticker) DO UPDATE SET
               total_research_runs = excluded.total_research_runs, avg_quality_score = excluded.avg_quality_score,
               thesis_accuracy_90d = excluded.thesis_accuracy_90d, updated_at = excluded.updated_at",
            params![id, ticker, total_research_runs, avg_quality_score, thesis_accuracy_90d, encode_time(now), encode_time(now)],
        )?;
        self.get_ticker_research_summary(ticker)
            .await?
            .context("ticker summary missing")
    }

    pub async fn get_ticker_research_summary(
        &self,
        ticker: &str,
    ) -> Result<Option<TickerResearchSummary>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM ticker_research_summary WHERE ticker = ?1",
            [ticker],
            map_ticker_research_summary,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_top_ticker_research_summaries(
        &self,
        limit: i64,
    ) -> Result<Vec<TickerResearchSummary>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare("SELECT * FROM ticker_research_summary ORDER BY thesis_accuracy_90d DESC NULLS LAST LIMIT ?1")?;
        let rows = statement.query_map([limit], map_ticker_research_summary)?;
        collect_rows(rows)
    }

    pub async fn create_research_analytics(
        &self,
        analytics_date: chrono::NaiveDate,
        total_runs: i64,
        total_theses: i64,
        avg_quality_score: Option<f64>,
        thesis_accuracy_30d: Option<f64>,
        thesis_accuracy_90d: Option<f64>,
    ) -> Result<ResearchAnalytics> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO research_analytics (id, analytics_date, total_runs, total_theses, avg_quality_score, thesis_accuracy_30d, thesis_accuracy_90d, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, analytics_date.to_string(), total_runs, total_theses, avg_quality_score, thesis_accuracy_30d, thesis_accuracy_90d, encode_time(now)],
        )?;
        self.get_research_analytics_by_id(&id)
            .await?
            .context("research analytics missing")
    }

    pub async fn get_research_analytics_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ResearchAnalytics>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM research_analytics WHERE id = ?1",
            [id],
            map_research_analytics,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn get_latest_research_analytics(&self) -> Result<Option<ResearchAnalytics>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM research_analytics ORDER BY analytics_date DESC LIMIT 1",
            [],
            map_research_analytics,
        )
        .optional()
        .map_err(Into::into)
    }
}

pub(crate) fn map_evidence_outcome(row: &Row<'_>) -> rusqlite::Result<EvidenceOutcome> {
    Ok(EvidenceOutcome {
        id: row.get("id")?,
        evidence_note_id: row.get("evidence_note_id")?,
        run_id: row.get("run_id")?,
        ticker: row.get("ticker")?,
        claim_type: row.get("claim_type")?,
        claim_text: row.get("claim_text")?,
        outcome_type: row.get("outcome_type")?,
        outcome_date: row
            .get::<_, String>("outcome_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        price_at_claim: row.get("price_at_claim")?,
        price_at_outcome: row.get("price_at_outcome")?,
        return_since_claim: row.get("return_since_claim")?,
        was_correct: row.get::<_, i64>("was_correct")? > 0,
        confidence_at_claim: row.get("confidence_at_claim")?,
        outcome_notes: row.get("outcome_notes")?,
        verified_by: row.get("verified_by")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_ticker_research_summary(
    row: &Row<'_>,
) -> rusqlite::Result<TickerResearchSummary> {
    Ok(TickerResearchSummary {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        first_research_date: row
            .get::<_, Option<String>>("first_research_date")?
            .and_then(|s| s.parse().ok()),
        last_research_date: row
            .get::<_, Option<String>>("last_research_date")?
            .and_then(|s| s.parse().ok()),
        total_research_runs: row.get("total_research_runs")?,
        avg_conviction: row.get("avg_conviction")?,
        avg_quality_score: row.get("avg_quality_score")?,
        thesis_accuracy_30d: row.get("thesis_accuracy_30d")?,
        thesis_accuracy_90d: row.get("thesis_accuracy_90d")?,
        total_return_all_time: row.get("total_return_all_time")?,
        best_return_90d: row.get("best_return_90d")?,
        worst_return_90d: row.get("worst_return_90d")?,
        research_frequency: row.get("research_frequency")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

pub(crate) fn map_research_analytics(row: &Row<'_>) -> rusqlite::Result<ResearchAnalytics> {
    Ok(ResearchAnalytics {
        id: row.get("id")?,
        analytics_date: row
            .get::<_, String>("analytics_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        total_runs: row.get("total_runs")?,
        total_theses: row.get("total_theses")?,
        avg_conviction: row.get("avg_conviction")?,
        avg_iteration_count: row.get("avg_iteration_count")?,
        avg_source_count: row.get("avg_source_count")?,
        avg_evidence_count: row.get("avg_evidence_count")?,
        avg_quality_score: row.get("avg_quality_score")?,
        thesis_accuracy_30d: row.get("thesis_accuracy_30d")?,
        thesis_accuracy_90d: row.get("thesis_accuracy_90d")?,
        top_performing_ticker: row.get("top_performing_ticker")?,
        worst_performing_ticker: row.get("worst_performing_ticker")?,
        best_model_provider_id: row.get("best_model_provider_id")?,
        model_accuracy_ranking_json: row.get("model_accuracy_ranking_json")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}
