//! Search queries, search results, sources, and evidence notes.
//!
//! These four tables form the evidence-gathering path of an iteration and
//! share the batch-insert helpers used by the orchestrator's write-heavy
//! phase.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{EvidenceNoteRecord, SearchQueryRecord, SearchResultRecord, SourceRecord};

use super::{collect_rows, encode_time, option_time, parse_time, Database};

/// Input type for `Database::insert_search_results_and_sources_batch`.
/// Declared here (rather than in `services::source_ranker`) so the DB layer
/// can offer a single-transaction batch insert without depending on a
/// service-layer type.
#[derive(Debug, Clone)]
pub struct RankedInsert {
    pub query_id: String,
    pub title: Option<String>,
    pub url: String,
    pub domain: Option<String>,
    pub snippet: Option<String>,
    pub rank_score: f64,
    pub source_type: String,
}

/// Borrowed input for `Database::insert_evidence_notes_batch` so callers
/// don't have to clone note strings just to hand them to us.
#[derive(Debug, Clone, Copy)]
pub struct EvidenceNoteInsert<'a> {
    pub source_id: &'a str,
    pub note_markdown: &'a str,
    pub claim_type: Option<&'a str>,
}

impl Database {
    pub async fn insert_search_query(
        &self,
        iteration_id: &str,
        query_text: &str,
    ) -> Result<SearchQueryRecord> {
        let record = SearchQueryRecord {
            id: Uuid::new_v4().to_string(),
            iteration_id: iteration_id.to_string(),
            query_text: query_text.to_string(),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO search_queries (id, iteration_id, query_text, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![record.id, record.iteration_id, record.query_text, encode_time(record.created_at)],
        )?;

        Ok(record)
    }

    pub async fn list_search_queries(&self, iteration_id: &str) -> Result<Vec<SearchQueryRecord>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM search_queries WHERE iteration_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map([iteration_id], map_search_query)?;
        collect_rows(rows)
    }

    /// Insert many search queries for the same iteration under a single
    /// transaction. SQLite issues one fsync per transaction, so folding N
    /// inserts into one drops commit latency roughly in proportion to N.
    pub async fn insert_search_queries_batch(
        &self,
        iteration_id: &str,
        query_texts: &[String],
    ) -> Result<Vec<SearchQueryRecord>> {
        if query_texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        let mut records = Vec::with_capacity(query_texts.len());
        {
            let mut statement = tx.prepare(
                "INSERT INTO search_queries (id, iteration_id, query_text, created_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for query_text in query_texts {
                let record = SearchQueryRecord {
                    id: Uuid::new_v4().to_string(),
                    iteration_id: iteration_id.to_string(),
                    query_text: query_text.clone(),
                    created_at: Utc::now(),
                };
                statement.execute(params![
                    record.id,
                    record.iteration_id,
                    record.query_text,
                    encode_time(record.created_at),
                ])?;
                records.push(record);
            }
        }
        tx.commit()?;
        Ok(records)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_search_result(
        &self,
        iteration_id: &str,
        query_id: &str,
        title: Option<&str>,
        url: &str,
        snippet: Option<&str>,
        rank_score: Option<f64>,
        source_type: Option<&str>,
    ) -> Result<SearchResultRecord> {
        let record = SearchResultRecord {
            id: Uuid::new_v4().to_string(),
            iteration_id: iteration_id.to_string(),
            query_id: query_id.to_string(),
            title: title.map(ToOwned::to_owned),
            url: url.to_string(),
            snippet: snippet.map(ToOwned::to_owned),
            rank_score,
            source_type: source_type.map(ToOwned::to_owned),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO search_results (id, iteration_id, query_id, title, url, snippet, rank_score, source_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.id,
                record.iteration_id,
                record.query_id,
                record.title,
                record.url,
                record.snippet,
                record.rank_score,
                record.source_type,
                encode_time(record.created_at),
            ],
        )?;

        Ok(record)
    }

    pub async fn list_search_results(&self, iteration_id: &str) -> Result<Vec<SearchResultRecord>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM search_results WHERE iteration_id = ?1 ORDER BY rank_score DESC, created_at ASC",
        )?;
        let rows = statement.query_map([iteration_id], map_search_result)?;
        collect_rows(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_source(
        &self,
        run_id: &str,
        iteration_id: Option<&str>,
        url: &str,
        title: Option<&str>,
        domain: Option<&str>,
        excerpt: Option<&str>,
        quality_score: Option<f64>,
        source_type: Option<&str>,
    ) -> Result<SourceRecord> {
        let source = SourceRecord {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            iteration_id: iteration_id.map(ToOwned::to_owned),
            url: url.to_string(),
            title: title.map(ToOwned::to_owned),
            domain: domain.map(ToOwned::to_owned),
            published_at: None,
            source_type: source_type.map(ToOwned::to_owned),
            raw_text: None,
            excerpt: excerpt.map(ToOwned::to_owned),
            quality_score,
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO sources (id, run_id, iteration_id, url, title, domain, published_at, source_type, raw_text, excerpt, quality_score, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                source.id,
                source.run_id,
                source.iteration_id,
                source.url,
                source.title,
                source.domain,
                option_time(source.published_at),
                source.source_type,
                source.raw_text,
                source.excerpt,
                source.quality_score,
                encode_time(source.created_at),
            ],
        )?;

        Ok(source)
    }

    /// Insert a whole iteration's ranked search results and corresponding
    /// source rows in one transaction. Collapses what used to be two
    /// round-trips per ranked result (plus one fsync each) into one commit.
    pub async fn insert_search_results_and_sources_batch(
        &self,
        run_id: &str,
        iteration_id: &str,
        items: &[RankedInsert],
    ) -> Result<Vec<SourceRecord>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        let mut sources = Vec::with_capacity(items.len());
        {
            let mut insert_search = tx.prepare(
                "INSERT INTO search_results (id, iteration_id, query_id, title, url, snippet, rank_score, source_type, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut insert_source = tx.prepare(
                "INSERT INTO sources (id, run_id, iteration_id, url, title, domain, published_at, source_type, raw_text, excerpt, quality_score, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;

            for item in items {
                let now = Utc::now();
                let search_id = Uuid::new_v4().to_string();
                insert_search.execute(params![
                    search_id,
                    iteration_id,
                    item.query_id,
                    item.title,
                    item.url,
                    item.snippet,
                    item.rank_score,
                    item.source_type,
                    encode_time(now),
                ])?;

                let source = SourceRecord {
                    id: Uuid::new_v4().to_string(),
                    run_id: run_id.to_string(),
                    iteration_id: Some(iteration_id.to_string()),
                    url: item.url.clone(),
                    title: item.title.clone(),
                    domain: item.domain.clone(),
                    published_at: None,
                    source_type: Some(item.source_type.clone()),
                    raw_text: None,
                    excerpt: item.snippet.clone(),
                    quality_score: Some(item.rank_score),
                    created_at: now,
                };
                insert_source.execute(params![
                    source.id,
                    source.run_id,
                    source.iteration_id,
                    source.url,
                    source.title,
                    source.domain,
                    option_time(source.published_at),
                    source.source_type,
                    source.raw_text,
                    source.excerpt,
                    source.quality_score,
                    encode_time(source.created_at),
                ])?;
                sources.push(source);
            }
        }
        tx.commit()?;
        Ok(sources)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_source_content(
        &self,
        source_id: &str,
        title: Option<&str>,
        domain: Option<&str>,
        raw_text: Option<&str>,
        excerpt: Option<&str>,
        quality_score: Option<f64>,
        source_type: Option<&str>,
        published_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE sources
             SET title = ?1, domain = ?2, raw_text = ?3, excerpt = ?4, quality_score = ?5, source_type = ?6, published_at = ?7
             WHERE id = ?8",
            params![
                title,
                domain,
                raw_text,
                excerpt,
                quality_score,
                source_type,
                option_time(published_at),
                source_id,
            ],
        )?;
        Ok(())
    }

    pub async fn list_sources(&self, iteration_id: &str) -> Result<Vec<SourceRecord>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM sources WHERE iteration_id = ?1 ORDER BY quality_score DESC, created_at ASC",
        )?;
        let rows = statement.query_map([iteration_id], map_source)?;
        collect_rows(rows)
    }

    pub async fn get_source(&self, source_id: &str) -> Result<Option<SourceRecord>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM sources WHERE id = ?1",
            [source_id],
            map_source,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_sources_for_run(&self, run_id: &str) -> Result<Vec<SourceRecord>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM sources WHERE run_id = ?1 ORDER BY quality_score DESC, created_at ASC",
        )?;
        let rows = statement.query_map([run_id], map_source)?;
        collect_rows(rows)
    }

    pub async fn insert_evidence_note(
        &self,
        iteration_id: &str,
        source_id: &str,
        note_markdown: &str,
        claim_type: Option<&str>,
    ) -> Result<EvidenceNoteRecord> {
        let note = EvidenceNoteRecord {
            id: Uuid::new_v4().to_string(),
            iteration_id: iteration_id.to_string(),
            source_id: source_id.to_string(),
            note_markdown: note_markdown.to_string(),
            claim_type: claim_type.map(ToOwned::to_owned),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO evidence_notes (id, iteration_id, source_id, note_markdown, claim_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                note.id,
                note.iteration_id,
                note.source_id,
                note.note_markdown,
                note.claim_type,
                encode_time(note.created_at),
            ],
        )?;

        Ok(note)
    }

    /// Insert every evidence note for an iteration in one transaction.
    pub async fn insert_evidence_notes_batch(
        &self,
        iteration_id: &str,
        notes: &[EvidenceNoteInsert<'_>],
    ) -> Result<Vec<EvidenceNoteRecord>> {
        if notes.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        let mut records = Vec::with_capacity(notes.len());
        {
            let mut statement = tx.prepare(
                "INSERT INTO evidence_notes (id, iteration_id, source_id, note_markdown, claim_type, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for note in notes {
                let record = EvidenceNoteRecord {
                    id: Uuid::new_v4().to_string(),
                    iteration_id: iteration_id.to_string(),
                    source_id: note.source_id.to_string(),
                    note_markdown: note.note_markdown.to_string(),
                    claim_type: note.claim_type.map(ToOwned::to_owned),
                    created_at: Utc::now(),
                };
                statement.execute(params![
                    record.id,
                    record.iteration_id,
                    record.source_id,
                    record.note_markdown,
                    record.claim_type,
                    encode_time(record.created_at),
                ])?;
                records.push(record);
            }
        }
        tx.commit()?;
        Ok(records)
    }

    pub async fn list_evidence_notes(&self, iteration_id: &str) -> Result<Vec<EvidenceNoteRecord>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM evidence_notes WHERE iteration_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map([iteration_id], map_evidence_note)?;
        collect_rows(rows)
    }
}

pub(crate) fn map_search_query(row: &Row<'_>) -> rusqlite::Result<SearchQueryRecord> {
    Ok(SearchQueryRecord {
        id: row.get("id")?,
        iteration_id: row.get("iteration_id")?,
        query_text: row.get("query_text")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_search_result(row: &Row<'_>) -> rusqlite::Result<SearchResultRecord> {
    Ok(SearchResultRecord {
        id: row.get("id")?,
        iteration_id: row.get("iteration_id")?,
        query_id: row.get("query_id")?,
        title: row.get("title")?,
        url: row.get("url")?,
        snippet: row.get("snippet")?,
        rank_score: row.get("rank_score")?,
        source_type: row.get("source_type")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_source(row: &Row<'_>) -> rusqlite::Result<SourceRecord> {
    let published_at = row
        .get::<_, Option<String>>("published_at")?
        .map(parse_time)
        .transpose()?;
    Ok(SourceRecord {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        iteration_id: row.get("iteration_id")?,
        url: row.get("url")?,
        title: row.get("title")?,
        domain: row.get("domain")?,
        published_at,
        source_type: row.get("source_type")?,
        raw_text: row.get("raw_text")?,
        excerpt: row.get("excerpt")?,
        quality_score: row.get("quality_score")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_evidence_note(row: &Row<'_>) -> rusqlite::Result<EvidenceNoteRecord> {
    Ok(EvidenceNoteRecord {
        id: row.get("id")?,
        iteration_id: row.get("iteration_id")?,
        source_id: row.get("source_id")?,
        note_markdown: row.get("note_markdown")?,
        claim_type: row.get("claim_type")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}
