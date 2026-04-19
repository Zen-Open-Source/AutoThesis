//! Source-level annotations and per-domain reputation scoring.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{SourceAnnotation, SourceReputation};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    pub async fn create_source_annotation(
        &self,
        source_id: &str,
        run_id: &str,
        selected_text: &str,
        annotation_markdown: &str,
        tag: Option<&str>,
    ) -> Result<SourceAnnotation> {
        let annotation = SourceAnnotation {
            id: Uuid::new_v4().to_string(),
            source_id: source_id.to_string(),
            run_id: run_id.to_string(),
            selected_text: selected_text.to_string(),
            annotation_markdown: annotation_markdown.to_string(),
            tag: tag.map(ToOwned::to_owned),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO source_annotations (id, source_id, run_id, selected_text, annotation_markdown, tag, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                annotation.id,
                annotation.source_id,
                annotation.run_id,
                annotation.selected_text,
                annotation.annotation_markdown,
                annotation.tag,
                encode_time(annotation.created_at),
                encode_time(annotation.updated_at),
            ],
        )?;
        self.get_source_annotation(&annotation.id)
            .await?
            .context("source annotation missing after insert")
    }

    pub async fn list_source_annotations(&self, source_id: &str) -> Result<Vec<SourceAnnotation>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM source_annotations WHERE source_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([source_id], map_source_annotation)?;
        collect_rows(rows)
    }

    pub async fn delete_source_annotation(
        &self,
        source_id: &str,
        annotation_id: &str,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "DELETE FROM source_annotations WHERE source_id = ?1 AND id = ?2",
            params![source_id, annotation_id],
        )?;
        Ok(affected > 0)
    }

    async fn get_source_annotation(&self, annotation_id: &str) -> Result<Option<SourceAnnotation>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM source_annotations WHERE id = ?1",
            [annotation_id],
            map_source_annotation,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn upsert_source_reputation(
        &self,
        domain: &str,
        reputation_score: f64,
        total_citations: i64,
        successful_citations: i64,
        failed_citations: i64,
    ) -> Result<SourceReputation> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO source_reputation (id, domain, reputation_score, total_citations, successful_citations, failed_citations, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(domain) DO UPDATE SET
               reputation_score = excluded.reputation_score, total_citations = excluded.total_citations,
               successful_citations = excluded.successful_citations, failed_citations = excluded.failed_citations, updated_at = excluded.updated_at",
            params![id, domain, reputation_score, total_citations, successful_citations, failed_citations, encode_time(now), encode_time(now)],
        )?;
        self.get_source_reputation(domain)
            .await?
            .context("source reputation missing")
    }

    pub async fn get_source_reputation(&self, domain: &str) -> Result<Option<SourceReputation>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM source_reputation WHERE domain = ?1",
            [domain],
            map_source_reputation,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_top_source_reputations(&self, limit: i64) -> Result<Vec<SourceReputation>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare("SELECT * FROM source_reputation ORDER BY reputation_score DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_source_reputation)?;
        collect_rows(rows)
    }
}

pub(crate) fn map_source_annotation(row: &Row<'_>) -> rusqlite::Result<SourceAnnotation> {
    Ok(SourceAnnotation {
        id: row.get("id")?,
        source_id: row.get("source_id")?,
        run_id: row.get("run_id")?,
        selected_text: row.get("selected_text")?,
        annotation_markdown: row.get("annotation_markdown")?,
        tag: row.get("tag")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

pub(crate) fn map_source_reputation(row: &Row<'_>) -> rusqlite::Result<SourceReputation> {
    Ok(SourceReputation {
        id: row.get("id")?,
        domain: row.get("domain")?,
        reputation_score: row.get("reputation_score")?,
        total_citations: row.get("total_citations")?,
        successful_citations: row.get("successful_citations")?,
        failed_citations: row.get("failed_citations")?,
        avg_evidence_quality: row.get("avg_evidence_quality")?,
        source_type: row.get("source_type")?,
        bias_rating: row.get("bias_rating")?,
        reliability_tier: row.get("reliability_tier")?,
        notes: row.get("notes")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}
