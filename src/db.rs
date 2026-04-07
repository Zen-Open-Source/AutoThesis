use crate::models::{
    Bookmark, Comparison, ComparisonRun, ComparisonRunWithDetails, EventRecord, EvidenceNoteRecord,
    Iteration, IterationDetail, Run, SearchQueryRecord, SearchResultRecord, SourceAnnotation,
    SourceRecord,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::{fs, path::PathBuf};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    database_url: String,
}

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let database = Self {
            database_url: database_url.to_string(),
        };
        database.run_migrations()?;
        Ok(database)
    }

    pub async fn create_run(&self, ticker: &str, question: &str) -> Result<Run> {
        let run = Run {
            id: Uuid::new_v4().to_string(),
            ticker: ticker.to_string(),
            question: question.to_string(),
            status: "queued".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            final_iteration_number: None,
            final_memo_markdown: None,
            final_memo_html: None,
            summary: None,
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO runs (id, ticker, question, status, created_at, updated_at, final_iteration_number, final_memo_markdown, final_memo_html, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                run.id,
                run.ticker,
                run.question,
                run.status,
                encode_time(run.created_at),
                encode_time(run.updated_at),
                run.final_iteration_number,
                run.final_memo_markdown,
                run.final_memo_html,
                run.summary,
            ],
        )?;

        self.get_run(&run.id)
            .await?
            .context("created run missing after insert")
    }

    pub async fn list_runs(&self, limit: i64) -> Result<Vec<Run>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare("SELECT * FROM runs ORDER BY created_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_run)?;
        collect_rows(rows)
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<Run>> {
        let conn = self.open_connection()?;
        conn.query_row("SELECT * FROM runs WHERE id = ?1", [run_id], map_run)
            .optional()
            .map_err(Into::into)
    }

    pub async fn set_run_status(&self, run_id: &str, status: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE runs SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, encode_time(Utc::now()), run_id],
        )?;
        Ok(())
    }

    pub async fn finalize_run(
        &self,
        run_id: &str,
        final_iteration_number: i64,
        final_memo_markdown: &str,
        final_memo_html: &str,
        summary: Option<&str>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE runs
             SET status = ?1, updated_at = ?2, final_iteration_number = ?3, final_memo_markdown = ?4, final_memo_html = ?5, summary = ?6
             WHERE id = ?7",
            params![
                "completed",
                encode_time(Utc::now()),
                final_iteration_number,
                final_memo_markdown,
                final_memo_html,
                summary,
                run_id,
            ],
        )?;
        Ok(())
    }

    pub async fn create_iteration(&self, run_id: &str, iteration_number: i64) -> Result<Iteration> {
        let iteration = Iteration {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            iteration_number,
            status: "running".to_string(),
            plan_markdown: None,
            draft_markdown: None,
            critique_markdown: None,
            evaluation_json: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO iterations (id, run_id, iteration_number, status, plan_markdown, draft_markdown, critique_markdown, evaluation_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                iteration.id,
                iteration.run_id,
                iteration.iteration_number,
                iteration.status,
                iteration.plan_markdown,
                iteration.draft_markdown,
                iteration.critique_markdown,
                iteration.evaluation_json,
                encode_time(iteration.created_at),
                encode_time(iteration.updated_at),
            ],
        )?;

        self.get_iteration_by_number(run_id, iteration_number)
            .await?
            .context("created iteration missing after insert")
    }

    pub async fn get_iteration_by_number(
        &self,
        run_id: &str,
        iteration_number: i64,
    ) -> Result<Option<Iteration>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM iterations WHERE run_id = ?1 AND iteration_number = ?2",
            params![run_id, iteration_number],
            map_iteration,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_iterations(&self, run_id: &str) -> Result<Vec<Iteration>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare("SELECT * FROM iterations WHERE run_id = ?1 ORDER BY iteration_number ASC")?;
        let rows = statement.query_map([run_id], map_iteration)?;
        collect_rows(rows)
    }

    pub async fn update_iteration_plan(
        &self,
        iteration_id: &str,
        plan_markdown: &str,
    ) -> Result<()> {
        self.update_iteration_field(iteration_id, "plan_markdown", plan_markdown)
    }

    pub async fn update_iteration_draft(
        &self,
        iteration_id: &str,
        draft_markdown: &str,
    ) -> Result<()> {
        self.update_iteration_field(iteration_id, "draft_markdown", draft_markdown)
    }

    pub async fn update_iteration_critique(
        &self,
        iteration_id: &str,
        critique_markdown: &str,
    ) -> Result<()> {
        self.update_iteration_field(iteration_id, "critique_markdown", critique_markdown)
    }

    pub async fn update_iteration_evaluation(
        &self,
        iteration_id: &str,
        evaluation_json: &str,
    ) -> Result<()> {
        self.update_iteration_field(iteration_id, "evaluation_json", evaluation_json)
    }

    pub async fn set_iteration_status(&self, iteration_id: &str, status: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE iterations SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, encode_time(Utc::now()), iteration_id],
        )?;
        Ok(())
    }

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

    pub async fn list_evidence_notes(&self, iteration_id: &str) -> Result<Vec<EvidenceNoteRecord>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM evidence_notes WHERE iteration_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map([iteration_id], map_evidence_note)?;
        collect_rows(rows)
    }

    pub async fn insert_event(
        &self,
        run_id: &str,
        iteration_id: Option<&str>,
        event_type: &str,
        message: &str,
        payload_json: Option<&str>,
    ) -> Result<EventRecord> {
        let event = EventRecord {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            iteration_id: iteration_id.map(ToOwned::to_owned),
            event_type: event_type.to_string(),
            message: message.to_string(),
            payload_json: payload_json.map(ToOwned::to_owned),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO events (id, run_id, iteration_id, event_type, message, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id,
                event.run_id,
                event.iteration_id,
                event.event_type,
                event.message,
                event.payload_json,
                encode_time(event.created_at),
            ],
        )?;

        Ok(event)
    }

    pub async fn list_events(&self, run_id: &str) -> Result<Vec<EventRecord>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM events WHERE run_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map([run_id], map_event)?;
        collect_rows(rows)
    }

    pub async fn get_iteration_detail(
        &self,
        run_id: &str,
        iteration_number: i64,
    ) -> Result<Option<IterationDetail>> {
        let Some(iteration) = self
            .get_iteration_by_number(run_id, iteration_number)
            .await?
        else {
            return Ok(None);
        };

        Ok(Some(IterationDetail {
            search_queries: self.list_search_queries(&iteration.id).await?,
            search_results: self.list_search_results(&iteration.id).await?,
            sources: self.list_sources(&iteration.id).await?,
            evidence_notes: self.list_evidence_notes(&iteration.id).await?,
            iteration,
        }))
    }

    pub async fn upsert_bookmark(
        &self,
        entity_type: &str,
        entity_id: &str,
        title: &str,
        note: Option<&str>,
        target_path: &str,
    ) -> Result<Bookmark> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO bookmarks (id, entity_type, entity_id, title, note, target_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(entity_type, entity_id)
             DO UPDATE SET
                title = excluded.title,
                note = excluded.note,
                target_path = excluded.target_path,
                updated_at = excluded.updated_at",
            params![
                id,
                entity_type,
                entity_id,
                title,
                note,
                target_path,
                encode_time(now),
                encode_time(now),
            ],
        )?;
        self.get_bookmark_by_entity(entity_type, entity_id)
            .await?
            .context("bookmark missing after upsert")
    }

    pub async fn list_bookmarks(&self, limit: i64) -> Result<Vec<Bookmark>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM bookmarks ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_bookmark)?;
        collect_rows(rows)
    }

    pub async fn delete_bookmark(&self, entity_type: &str, entity_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "DELETE FROM bookmarks WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
        )?;
        Ok(affected > 0)
    }

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

    fn update_iteration_field(
        &self,
        iteration_id: &str,
        field_name: &str,
        value: &str,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let query =
            format!("UPDATE iterations SET {field_name} = ?1, updated_at = ?2 WHERE id = ?3");
        conn.execute(
            &query,
            params![value, encode_time(Utc::now()), iteration_id],
        )?;
        Ok(())
    }

    fn open_connection(&self) -> Result<Connection> {
        let path = normalize_database_url(&self.database_url);
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(conn)
    }

    fn run_migrations(&self) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version TEXT PRIMARY KEY)",
            [],
        )?;

        let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sql/migrations");
        let mut entries = fs::read_dir(&migrations_dir)
            .with_context(|| {
                format!(
                    "failed to read migrations directory {}",
                    migrations_dir.display()
                )
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            if !entry.file_type()?.is_file() {
                continue;
            }
            let version = entry.file_name().to_string_lossy().to_string();
            let already_applied = conn
                .query_row(
                    "SELECT version FROM schema_migrations WHERE version = ?1",
                    [version.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if already_applied.is_some() {
                continue;
            }

            let sql = fs::read_to_string(entry.path())?;
            conn.execute_batch(&sql)?;
            conn.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [version.as_str()],
            )?;
        }

        Ok(())
    }

    async fn get_bookmark_by_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<Option<Bookmark>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM bookmarks WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
            map_bookmark,
        )
        .optional()
        .map_err(Into::into)
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
}

fn normalize_database_url(database_url: &str) -> String {
    if database_url == "sqlite::memory:" || database_url == ":memory:" {
        ":memory:".to_string()
    } else if let Some(stripped) = database_url.strip_prefix("sqlite://") {
        stripped.to_string()
    } else if let Some(stripped) = database_url.strip_prefix("sqlite:") {
        stripped.to_string()
    } else {
        database_url.to_string()
    }
}

fn encode_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn option_time(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(encode_time)
}

fn parse_time(value: String) -> rusqlite::Result<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn map_run(row: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        question: row.get("question")?,
        status: row.get("status")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
        final_iteration_number: row.get("final_iteration_number")?,
        final_memo_markdown: row.get("final_memo_markdown")?,
        final_memo_html: row.get("final_memo_html")?,
        summary: row.get("summary")?,
    })
}

fn map_iteration(row: &Row<'_>) -> rusqlite::Result<Iteration> {
    Ok(Iteration {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        iteration_number: row.get("iteration_number")?,
        status: row.get("status")?,
        plan_markdown: row.get("plan_markdown")?,
        draft_markdown: row.get("draft_markdown")?,
        critique_markdown: row.get("critique_markdown")?,
        evaluation_json: row.get("evaluation_json")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_search_query(row: &Row<'_>) -> rusqlite::Result<SearchQueryRecord> {
    Ok(SearchQueryRecord {
        id: row.get("id")?,
        iteration_id: row.get("iteration_id")?,
        query_text: row.get("query_text")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_search_result(row: &Row<'_>) -> rusqlite::Result<SearchResultRecord> {
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

fn map_source(row: &Row<'_>) -> rusqlite::Result<SourceRecord> {
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

fn map_evidence_note(row: &Row<'_>) -> rusqlite::Result<EvidenceNoteRecord> {
    Ok(EvidenceNoteRecord {
        id: row.get("id")?,
        iteration_id: row.get("iteration_id")?,
        source_id: row.get("source_id")?,
        note_markdown: row.get("note_markdown")?,
        claim_type: row.get("claim_type")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_event(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
    Ok(EventRecord {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        iteration_id: row.get("iteration_id")?,
        event_type: row.get("event_type")?,
        message: row.get("message")?,
        payload_json: row.get("payload_json")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

// Comparison methods
impl Database {
    pub async fn create_comparison(&self, name: &str, question: &str) -> Result<Comparison> {
        let comparison = Comparison {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            question: question.to_string(),
            status: "building".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            final_comparison_html: None,
            summary: None,
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO comparisons (id, name, question, status, created_at, updated_at, final_comparison_html, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                comparison.id,
                comparison.name,
                comparison.question,
                comparison.status,
                encode_time(comparison.created_at),
                encode_time(comparison.updated_at),
                comparison.final_comparison_html,
                comparison.summary,
            ],
        )?;

        self.get_comparison(&comparison.id)
            .await?
            .context("created comparison missing after insert")
    }

    pub async fn get_comparison(&self, comparison_id: &str) -> Result<Option<Comparison>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM comparisons WHERE id = ?1",
            [comparison_id],
            map_comparison,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_comparisons(&self, limit: i64) -> Result<Vec<Comparison>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM comparisons ORDER BY created_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_comparison)?;
        collect_rows(rows)
    }

    pub async fn add_run_to_comparison(
        &self,
        comparison_id: &str,
        run_id: &str,
        ticker: &str,
        sort_order: i64,
    ) -> Result<ComparisonRun> {
        let comparison_run = ComparisonRun {
            id: Uuid::new_v4().to_string(),
            comparison_id: comparison_id.to_string(),
            run_id: run_id.to_string(),
            ticker: ticker.to_string(),
            sort_order,
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO comparison_runs (id, comparison_id, run_id, ticker, sort_order, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                comparison_run.id,
                comparison_run.comparison_id,
                comparison_run.run_id,
                comparison_run.ticker,
                comparison_run.sort_order,
                encode_time(comparison_run.created_at),
            ],
        )?;

        Ok(comparison_run)
    }

    pub async fn list_comparison_runs(
        &self,
        comparison_id: &str,
    ) -> Result<Vec<ComparisonRunWithDetails>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT
                cr.id AS cr_id,
                cr.comparison_id AS cr_comparison_id,
                cr.run_id AS cr_run_id,
                cr.ticker AS cr_ticker,
                cr.sort_order AS cr_sort_order,
                cr.created_at AS cr_created_at,
                r.id AS run_id,
                r.ticker AS run_ticker,
                r.question AS run_question,
                r.status AS run_status,
                r.created_at AS run_created_at,
                r.updated_at AS run_updated_at,
                r.final_iteration_number AS run_final_iteration_number,
                r.final_memo_markdown AS run_final_memo_markdown,
                r.final_memo_html AS run_final_memo_html,
                r.summary AS run_summary
             FROM comparison_runs cr
             LEFT JOIN runs r ON cr.run_id = r.id
             WHERE cr.comparison_id = ?1
             ORDER BY cr.sort_order ASC, cr.created_at ASC",
        )?;

        let rows = statement.query_map([comparison_id], |row| {
            let comparison_run = ComparisonRun {
                id: row.get("cr_id")?,
                comparison_id: row.get("cr_comparison_id")?,
                run_id: row.get("cr_run_id")?,
                ticker: row.get("cr_ticker")?,
                sort_order: row.get("cr_sort_order")?,
                created_at: parse_time(row.get("cr_created_at")?)?,
            };

            let run_id: Option<String> = row.get("run_id")?;
            let run = if run_id.is_some() {
                Some(Run {
                    id: row.get("run_id")?,
                    ticker: row.get("run_ticker")?,
                    question: row.get("run_question")?,
                    status: row.get("run_status")?,
                    created_at: parse_time(row.get("run_created_at")?)?,
                    updated_at: parse_time(row.get("run_updated_at")?)?,
                    final_iteration_number: row.get("run_final_iteration_number")?,
                    final_memo_markdown: row.get("run_final_memo_markdown")?,
                    final_memo_html: row.get("run_final_memo_html")?,
                    summary: row.get("run_summary")?,
                })
            } else {
                None
            };

            Ok(ComparisonRunWithDetails {
                id: comparison_run.id,
                comparison_id: comparison_run.comparison_id,
                run_id: comparison_run.run_id,
                ticker: comparison_run.ticker,
                sort_order: comparison_run.sort_order,
                created_at: comparison_run.created_at,
                run,
            })
        })?;

        collect_rows(rows)
    }

    pub async fn list_comparison_ids_for_run(&self, run_id: &str) -> Result<Vec<String>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT comparison_id FROM comparison_runs WHERE run_id = ?1")?;
        let rows = statement.query_map([run_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    pub async fn update_comparison_status(&self, comparison_id: &str, status: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE comparisons SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, encode_time(Utc::now()), comparison_id],
        )?;
        Ok(())
    }

    pub async fn finalize_comparison(
        &self,
        comparison_id: &str,
        status: &str,
        final_comparison_html: &str,
        summary: Option<&str>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE comparisons
             SET status = ?1, updated_at = ?2, final_comparison_html = ?3, summary = ?4
             WHERE id = ?5",
            params![
                status,
                encode_time(Utc::now()),
                final_comparison_html,
                summary,
                comparison_id,
            ],
        )?;
        Ok(())
    }

    pub async fn delete_comparison(&self, comparison_id: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute("DELETE FROM comparisons WHERE id = ?1", [comparison_id])?;
        Ok(())
    }
}

fn map_comparison(row: &Row<'_>) -> rusqlite::Result<Comparison> {
    Ok(Comparison {
        id: row.get("id")?,
        name: row.get("name")?,
        question: row.get("question")?,
        status: row.get("status")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
        final_comparison_html: row.get("final_comparison_html")?,
        summary: row.get("summary")?,
    })
}

fn map_bookmark(row: &Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get("id")?,
        entity_type: row.get("entity_type")?,
        entity_id: row.get("entity_id")?,
        title: row.get("title")?,
        note: row.get("note")?,
        target_path: row.get("target_path")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_source_annotation(row: &Row<'_>) -> rusqlite::Result<SourceAnnotation> {
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
