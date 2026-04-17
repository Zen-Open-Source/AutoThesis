use crate::models::{
    AlertRule, BatchJob, BatchJobRun, BatchJobRunWithDetails, Bookmark, Comparison, ComparisonRun,
    ComparisonRunWithDetails, EventRecord, EvidenceNoteRecord, EvidenceOutcome, Iteration,
    IterationDetail, LlmProvider, ModelComparison, ModelQualityScore, ModelRun, Portfolio,
    Position, PriceSnapshot, ResearchAnalytics, Run, RunTemplate, ScanOpportunity, ScanRun,
    ScannerConfig, ScheduledRun, SearchQueryRecord, SearchResultRecord, SignalEffectiveness,
    SourceAnnotation, SourceRecord, SourceReputation, ThesisAccuracy, ThesisAlert, ThesisHistory,
    ThesisOutcome, TickerResearchSummary, TickerUniverse, Transaction, Watchlist,
    WatchlistSchedule, WatchlistTicker,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, Error as RusqliteError, OptionalExtension, Row};
use std::{fs, path::PathBuf, sync::Arc};
use uuid::Uuid;

pub(crate) type SqlitePool = Pool<SqliteConnectionManager>;
pub(crate) type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

#[derive(Clone)]
pub struct Database {
    pool: Arc<SqlitePool>,
}

#[derive(Debug)]
struct SqlitePragmaSetup;

impl CustomizeConnection<Connection, RusqliteError> for SqlitePragmaSetup {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), RusqliteError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(())
    }
}

const ALERT_RULE_SCORE_DROP: &str = "score_drop";
const ALERT_RULE_FRESHNESS_STALE: &str = "freshness_stale";
const ALERT_RULE_DECISION_DOWNGRADE: &str = "decision_downgrade";

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let path = normalize_database_url(database_url);
        let manager = if path == ":memory:" {
            SqliteConnectionManager::memory()
        } else {
            SqliteConnectionManager::file(&path)
        };
        let manager = manager.with_init(|_| Ok(()));

        // For in-memory databases every new connection is a distinct DB, so we
        // must cap the pool at 1 to preserve schema + data across calls.
        let is_memory = path == ":memory:";
        let max_size = if is_memory { 1 } else { 8 };

        let pool = Pool::builder()
            .max_size(max_size)
            .connection_customizer(Box::new(SqlitePragmaSetup))
            .build(manager)
            .context("failed to build sqlite connection pool")?;

        let database = Self {
            pool: Arc::new(pool),
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

    pub async fn reset_run_for_retry(&self, run_id: &str) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM source_annotations WHERE run_id = ?1", [run_id])?;
        tx.execute("DELETE FROM iterations WHERE run_id = ?1", [run_id])?;
        tx.execute("DELETE FROM sources WHERE run_id = ?1", [run_id])?;
        tx.execute("DELETE FROM events WHERE run_id = ?1", [run_id])?;
        tx.execute(
            "UPDATE runs
             SET status = ?1, updated_at = ?2, final_iteration_number = NULL, final_memo_markdown = NULL, final_memo_html = NULL, summary = NULL
             WHERE id = ?3",
            params!["queued", encode_time(Utc::now()), run_id],
        )?;
        tx.commit()?;
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

    fn open_connection(&self) -> Result<PooledConn> {
        self.pool
            .get()
            .context("failed to acquire sqlite connection from pool")
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

    async fn get_watchlist_ticker(
        &self,
        watchlist_id: &str,
        ticker: &str,
    ) -> Result<Option<WatchlistTicker>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM watchlist_tickers WHERE watchlist_id = ?1 AND ticker = ?2",
            params![watchlist_id, ticker],
            map_watchlist_ticker,
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

    pub async fn create_batch_job(&self, name: &str, question_template: &str) -> Result<BatchJob> {
        let batch_job = BatchJob {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            question_template: question_template.to_string(),
            status: "building".to_string(),
            summary: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO batch_jobs (id, name, question_template, status, summary, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                batch_job.id,
                batch_job.name,
                batch_job.question_template,
                batch_job.status,
                batch_job.summary,
                encode_time(batch_job.created_at),
                encode_time(batch_job.updated_at),
            ],
        )?;

        self.get_batch_job(&batch_job.id)
            .await?
            .context("created batch job missing after insert")
    }

    pub async fn get_batch_job(&self, batch_job_id: &str) -> Result<Option<BatchJob>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM batch_jobs WHERE id = ?1",
            [batch_job_id],
            map_batch_job,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_batch_jobs(&self, limit: i64) -> Result<Vec<BatchJob>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM batch_jobs ORDER BY created_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_batch_job)?;
        collect_rows(rows)
    }

    pub async fn add_run_to_batch_job(
        &self,
        batch_job_id: &str,
        run_id: &str,
        ticker: &str,
        sort_order: i64,
    ) -> Result<BatchJobRun> {
        let batch_job_run = BatchJobRun {
            id: Uuid::new_v4().to_string(),
            batch_job_id: batch_job_id.to_string(),
            run_id: run_id.to_string(),
            ticker: ticker.to_string(),
            sort_order,
            created_at: Utc::now(),
        };
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO batch_job_runs (id, batch_job_id, run_id, ticker, sort_order, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                batch_job_run.id,
                batch_job_run.batch_job_id,
                batch_job_run.run_id,
                batch_job_run.ticker,
                batch_job_run.sort_order,
                encode_time(batch_job_run.created_at),
            ],
        )?;
        Ok(batch_job_run)
    }

    pub async fn list_batch_job_runs(
        &self,
        batch_job_id: &str,
    ) -> Result<Vec<BatchJobRunWithDetails>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT
                bjr.id AS bjr_id,
                bjr.batch_job_id AS bjr_batch_job_id,
                bjr.run_id AS bjr_run_id,
                bjr.ticker AS bjr_ticker,
                bjr.sort_order AS bjr_sort_order,
                bjr.created_at AS bjr_created_at,
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
             FROM batch_job_runs bjr
             LEFT JOIN runs r ON bjr.run_id = r.id
             WHERE bjr.batch_job_id = ?1
             ORDER BY bjr.sort_order ASC, bjr.created_at ASC",
        )?;

        let rows = statement.query_map([batch_job_id], |row| {
            let batch_job_run = BatchJobRun {
                id: row.get("bjr_id")?,
                batch_job_id: row.get("bjr_batch_job_id")?,
                run_id: row.get("bjr_run_id")?,
                ticker: row.get("bjr_ticker")?,
                sort_order: row.get("bjr_sort_order")?,
                created_at: parse_time(row.get("bjr_created_at")?)?,
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

            Ok(BatchJobRunWithDetails {
                id: batch_job_run.id,
                batch_job_id: batch_job_run.batch_job_id,
                run_id: batch_job_run.run_id,
                ticker: batch_job_run.ticker,
                sort_order: batch_job_run.sort_order,
                created_at: batch_job_run.created_at,
                run,
            })
        })?;

        collect_rows(rows)
    }

    pub async fn list_batch_job_ids_for_run(&self, run_id: &str) -> Result<Vec<String>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT batch_job_id FROM batch_job_runs WHERE run_id = ?1")?;
        let rows = statement.query_map([run_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    pub async fn update_batch_job_status(&self, batch_job_id: &str, status: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE batch_jobs SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, encode_time(Utc::now()), batch_job_id],
        )?;
        Ok(())
    }

    pub async fn finalize_batch_job(
        &self,
        batch_job_id: &str,
        status: &str,
        summary: Option<&str>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE batch_jobs SET status = ?1, summary = ?2, updated_at = ?3 WHERE id = ?4",
            params![status, summary, encode_time(Utc::now()), batch_job_id],
        )?;
        Ok(())
    }

    pub async fn create_run_template(
        &self,
        name: &str,
        question_template: &str,
        description: Option<&str>,
    ) -> Result<RunTemplate> {
        let run_template = RunTemplate {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            question_template: question_template.to_string(),
            description: description.map(ToOwned::to_owned),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO run_templates (id, name, question_template, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_template.id,
                run_template.name,
                run_template.question_template,
                run_template.description,
                encode_time(run_template.created_at),
                encode_time(run_template.updated_at),
            ],
        )?;

        self.get_run_template(&run_template.id)
            .await?
            .context("created run template missing after insert")
    }

    pub async fn get_run_template(&self, template_id: &str) -> Result<Option<RunTemplate>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM run_templates WHERE id = ?1",
            [template_id],
            map_run_template,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_run_templates(&self, limit: i64) -> Result<Vec<RunTemplate>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM run_templates ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_run_template)?;
        collect_rows(rows)
    }

    pub async fn update_run_template(
        &self,
        template_id: &str,
        name: &str,
        question_template: &str,
        description: Option<&str>,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE run_templates
             SET name = ?1, question_template = ?2, description = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                name,
                question_template,
                description,
                encode_time(Utc::now()),
                template_id,
            ],
        )?;
        Ok(affected > 0)
    }

    pub async fn delete_run_template(&self, template_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute("DELETE FROM run_templates WHERE id = ?1", [template_id])?;
        Ok(affected > 0)
    }

    pub async fn create_watchlist(&self, name: &str) -> Result<Watchlist> {
        let watchlist = Watchlist {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO watchlists (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                watchlist.id,
                watchlist.name,
                encode_time(watchlist.created_at),
                encode_time(watchlist.updated_at),
            ],
        )?;

        self.get_watchlist(&watchlist.id)
            .await?
            .context("created watchlist missing after insert")
    }

    pub async fn get_watchlist(&self, watchlist_id: &str) -> Result<Option<Watchlist>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM watchlists WHERE id = ?1",
            [watchlist_id],
            map_watchlist,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_watchlists(&self, limit: i64) -> Result<Vec<Watchlist>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM watchlists ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_watchlist)?;
        collect_rows(rows)
    }

    pub async fn update_watchlist_name(&self, watchlist_id: &str, name: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE watchlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, encode_time(Utc::now()), watchlist_id],
        )?;
        Ok(affected > 0)
    }

    pub async fn delete_watchlist(&self, watchlist_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute("DELETE FROM watchlists WHERE id = ?1", [watchlist_id])?;
        Ok(affected > 0)
    }

    pub async fn replace_watchlist_tickers(
        &self,
        watchlist_id: &str,
        tickers: &[String],
    ) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM watchlist_tickers WHERE watchlist_id = ?1",
            [watchlist_id],
        )?;
        for (index, ticker) in tickers.iter().enumerate() {
            tx.execute(
                "INSERT INTO watchlist_tickers (id, watchlist_id, ticker, sort_order, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    Uuid::new_v4().to_string(),
                    watchlist_id,
                    ticker,
                    index as i64,
                    encode_time(Utc::now()),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub async fn list_watchlist_tickers(&self, watchlist_id: &str) -> Result<Vec<WatchlistTicker>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM watchlist_tickers WHERE watchlist_id = ?1 ORDER BY sort_order ASC, created_at ASC",
        )?;
        let rows = statement.query_map([watchlist_id], map_watchlist_ticker)?;
        collect_rows(rows)
    }

    pub async fn list_watchlist_ids_for_ticker(&self, ticker: &str) -> Result<Vec<String>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT watchlist_id FROM watchlist_tickers WHERE ticker = ?1 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map([ticker], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    pub async fn list_or_create_alert_rules(&self, watchlist_id: &str) -> Result<Vec<AlertRule>> {
        let rules = self.list_alert_rules(watchlist_id).await?;
        if !rules.is_empty() {
            return Ok(rules);
        }

        self.create_alert_rule(watchlist_id, ALERT_RULE_SCORE_DROP, Some(0.8), true)
            .await?;
        self.create_alert_rule(watchlist_id, ALERT_RULE_FRESHNESS_STALE, None, true)
            .await?;
        self.create_alert_rule(watchlist_id, ALERT_RULE_DECISION_DOWNGRADE, None, true)
            .await?;
        self.list_alert_rules(watchlist_id).await
    }

    pub async fn list_alert_rules(&self, watchlist_id: &str) -> Result<Vec<AlertRule>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare("SELECT * FROM alert_rules WHERE watchlist_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map([watchlist_id], map_alert_rule)?;
        collect_rows(rows)
    }

    async fn create_alert_rule(
        &self,
        watchlist_id: &str,
        rule_type: &str,
        threshold: Option<f64>,
        enabled: bool,
    ) -> Result<AlertRule> {
        let rule = AlertRule {
            id: Uuid::new_v4().to_string(),
            watchlist_id: watchlist_id.to_string(),
            rule_type: rule_type.to_string(),
            threshold,
            enabled,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT OR IGNORE INTO alert_rules (id, watchlist_id, rule_type, threshold, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                rule.id,
                rule.watchlist_id,
                rule.rule_type,
                rule.threshold,
                if rule.enabled { 1 } else { 0 },
                encode_time(rule.created_at),
                encode_time(rule.updated_at),
            ],
        )?;

        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM alert_rules WHERE watchlist_id = ?1 AND rule_type = ?2 LIMIT 1",
            params![watchlist_id, rule_type],
            map_alert_rule,
        )
        .optional()?
        .context("created alert rule missing after insert")
    }

    pub async fn list_thesis_alerts(
        &self,
        watchlist_id: &str,
        status: Option<&str>,
    ) -> Result<Vec<ThesisAlert>> {
        let conn = self.open_connection()?;
        if let Some(status) = status {
            let mut statement = conn.prepare(
                "SELECT * FROM thesis_alerts
                 WHERE watchlist_id = ?1 AND status = ?2
                 ORDER BY created_at DESC",
            )?;
            let rows = statement.query_map(params![watchlist_id, status], map_thesis_alert)?;
            return collect_rows(rows);
        }

        let mut statement = conn.prepare(
            "SELECT * FROM thesis_alerts
             WHERE watchlist_id = ?1
             ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([watchlist_id], map_thesis_alert)?;
        collect_rows(rows)
    }

    pub async fn create_thesis_alert_if_absent(
        &self,
        watchlist_id: &str,
        ticker: &str,
        run_id: &str,
        alert_type: &str,
        severity: &str,
        message: &str,
    ) -> Result<ThesisAlert> {
        let alert_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT OR IGNORE INTO thesis_alerts
             (id, watchlist_id, ticker, run_id, alert_type, severity, message, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                alert_id,
                watchlist_id,
                ticker,
                run_id,
                alert_type,
                severity,
                message,
                "active",
                encode_time(now),
                encode_time(now),
            ],
        )?;

        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM thesis_alerts
             WHERE watchlist_id = ?1 AND ticker = ?2 AND alert_type = ?3 AND run_id = ?4
             LIMIT 1",
            params![watchlist_id, ticker, alert_type, run_id],
            map_thesis_alert,
        )
        .optional()?
        .context("thesis alert missing after insert")
    }

    pub async fn update_thesis_alert_status(&self, alert_id: &str, status: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE thesis_alerts SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, encode_time(Utc::now()), alert_id],
        )?;
        Ok(affected > 0)
    }

    pub async fn add_ticker_to_watchlist(
        &self,
        watchlist_id: &str,
        ticker: &str,
        sort_order: i64,
    ) -> Result<WatchlistTicker> {
        let id = Uuid::new_v4().to_string();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO watchlist_tickers (id, watchlist_id, ticker, sort_order, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(watchlist_id, ticker) DO NOTHING",
            params![
                id,
                watchlist_id,
                ticker,
                sort_order,
                encode_time(Utc::now())
            ],
        )?;
        self.get_watchlist_ticker(watchlist_id, ticker)
            .await?
            .context("watchlist ticker missing after insert")
    }

    pub async fn remove_ticker_from_watchlist(
        &self,
        watchlist_id: &str,
        ticker: &str,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "DELETE FROM watchlist_tickers WHERE watchlist_id = ?1 AND ticker = ?2",
            params![watchlist_id, ticker],
        )?;
        Ok(affected > 0)
    }

    pub async fn list_runs_for_ticker(&self, ticker: &str, limit: i64) -> Result<Vec<Run>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM runs WHERE ticker = ?1 ORDER BY created_at DESC LIMIT ?2")?;
        let rows = statement.query_map(params![ticker, limit], map_run)?;
        collect_rows(rows)
    }

    pub async fn get_latest_iteration_evaluation_score(&self, run_id: &str) -> Result<Option<f64>> {
        let conn = self.open_connection()?;
        let evaluation_json = conn
            .query_row(
                "SELECT evaluation_json
                 FROM iterations
                 WHERE run_id = ?1 AND evaluation_json IS NOT NULL
                 ORDER BY iteration_number DESC
                 LIMIT 1",
                [run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        Ok(evaluation_json.and_then(|raw| {
            serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|value| value.get("score").and_then(|score| score.as_f64()))
        }))
    }

    pub async fn get_latest_source_timestamp_for_run(
        &self,
        run_id: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        let conn = self.open_connection()?;
        let raw_timestamp = conn
            .query_row(
                "SELECT COALESCE(published_at, created_at)
                 FROM sources
                 WHERE run_id = ?1
                 ORDER BY COALESCE(published_at, created_at) DESC
                 LIMIT 1",
                [run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        match raw_timestamp {
            Some(raw) => Ok(Some(parse_time(raw)?)),
            None => Ok(None),
        }
    }

    // Scanner database methods

    pub async fn upsert_ticker_universe(
        &self,
        ticker: &str,
        name: Option<&str>,
        sector: Option<&str>,
        industry: Option<&str>,
        market_cap_billion: Option<f64>,
        is_sp500: bool,
    ) -> Result<TickerUniverse> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO ticker_universe (id, ticker, name, sector, industry, market_cap_billion, is_sp500, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9)
             ON CONFLICT(ticker) DO UPDATE SET
               name = excluded.name,
               sector = excluded.sector,
               industry = excluded.industry,
               market_cap_billion = excluded.market_cap_billion,
               is_sp500 = excluded.is_sp500,
               updated_at = excluded.updated_at",
            params![
                id,
                ticker,
                name,
                sector,
                industry,
                market_cap_billion,
                if is_sp500 { 1 } else { 0 },
                encode_time(now),
                encode_time(now),
            ],
        )?;

        self.get_ticker_universe(ticker)
            .await?
            .context("ticker universe entry missing after upsert")
    }

    /// Batch lookup of `TickerUniverse` rows keyed by ticker. Avoids an N+1
    /// sequence of `get_ticker_universe` calls when enriching many candidates
    /// at once (e.g. related-tickers discovery).
    pub async fn get_ticker_universe_batch(
        &self,
        tickers: &[String],
    ) -> Result<std::collections::HashMap<String, TickerUniverse>> {
        use std::collections::HashMap;
        if tickers.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.open_connection()?;
        let placeholders: String = (0..tickers.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT * FROM ticker_universe WHERE ticker IN ({placeholders})");
        let params: Vec<&dyn rusqlite::ToSql> =
            tickers.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), map_ticker_universe)?;
        let mut map = HashMap::new();
        for row in rows {
            let entry = row?;
            map.insert(entry.ticker.clone(), entry);
        }
        Ok(map)
    }

    /// Batch lookup: for each ticker, fetch its latest run (if any). Single
    /// SQL query using a grouped sub-select.
    pub async fn latest_runs_for_tickers(
        &self,
        tickers: &[String],
    ) -> Result<std::collections::HashMap<String, Run>> {
        use std::collections::HashMap;
        if tickers.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.open_connection()?;
        let placeholders: String = (0..tickers.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT r.* FROM runs r
             INNER JOIN (
                SELECT ticker, MAX(created_at) AS max_created_at
                FROM runs
                WHERE ticker IN ({placeholders})
                GROUP BY ticker
             ) latest ON latest.ticker = r.ticker AND latest.max_created_at = r.created_at"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            tickers.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), map_run)?;
        let mut map = HashMap::new();
        for row in rows {
            let run = row?;
            map.insert(run.ticker.clone(), run);
        }
        Ok(map)
    }

    pub async fn get_ticker_universe(&self, ticker: &str) -> Result<Option<TickerUniverse>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM ticker_universe WHERE ticker = ?1",
            [ticker],
            map_ticker_universe,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_ticker_universe(
        &self,
        active_only: bool,
        sector_filter: Option<&str>,
        min_market_cap: Option<f64>,
        max_market_cap: Option<f64>,
    ) -> Result<Vec<TickerUniverse>> {
        let conn = self.open_connection()?;
        let mut sql = "SELECT * FROM ticker_universe WHERE 1=1".to_string();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if active_only {
            sql.push_str(" AND is_active = 1");
        }
        if let Some(sector) = sector_filter {
            sql.push_str(" AND sector = ?");
            params_vec.push(Box::new(sector.to_string()));
        }
        if let Some(min) = min_market_cap {
            sql.push_str(" AND market_cap_billion >= ?");
            params_vec.push(Box::new(min));
        }
        if let Some(max) = max_market_cap {
            sql.push_str(" AND market_cap_billion <= ?");
            params_vec.push(Box::new(max));
        }
        sql.push_str(" ORDER BY ticker ASC");

        let params: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), map_ticker_universe)?;
        collect_rows(rows)
    }

    pub async fn count_ticker_universe(&self, active_only: bool) -> Result<i64> {
        let conn = self.open_connection()?;
        let count = if active_only {
            conn.query_row(
                "SELECT COUNT(*) FROM ticker_universe WHERE is_active = 1",
                [],
                |row| row.get(0),
            )?
        } else {
            conn.query_row("SELECT COUNT(*) FROM ticker_universe", [], |row| row.get(0))?
        };
        Ok(count)
    }

    pub async fn set_ticker_universe_active(&self, ticker: &str, is_active: bool) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE ticker_universe SET is_active = ?1, updated_at = ?2 WHERE ticker = ?3",
            params![
                if is_active { 1 } else { 0 },
                encode_time(Utc::now()),
                ticker
            ],
        )?;
        Ok(affected > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_scanner_config(
        &self,
        name: &str,
        description: Option<&str>,
        universe_filter: &str,
        sector_filter: Option<&str>,
        min_market_cap: Option<f64>,
        max_market_cap: Option<f64>,
        max_opportunities: i64,
        signal_weights_json: Option<&str>,
    ) -> Result<ScannerConfig> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO scanner_configs
             (id, name, description, universe_filter, sector_filter, min_market_cap, max_market_cap, max_opportunities, signal_weights_json, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11)",
            params![
                id,
                name,
                description,
                universe_filter,
                sector_filter,
                min_market_cap,
                max_market_cap,
                max_opportunities,
                signal_weights_json,
                encode_time(now),
                encode_time(now),
            ],
        )?;

        self.get_scanner_config(&id)
            .await?
            .context("scanner config missing after insert")
    }

    pub async fn get_scanner_config(&self, config_id: &str) -> Result<Option<ScannerConfig>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM scanner_configs WHERE id = ?1",
            [config_id],
            map_scanner_config,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn get_default_scanner_config(&self) -> Result<Option<ScannerConfig>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM scanner_configs WHERE is_active = 1 ORDER BY created_at ASC LIMIT 1",
            [],
            map_scanner_config,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_scanner_configs(&self) -> Result<Vec<ScannerConfig>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM scanner_configs ORDER BY created_at ASC")?;
        let rows = statement.query_map([], map_scanner_config)?;
        collect_rows(rows)
    }

    pub async fn create_scan_run(&self, config_id: Option<&str>) -> Result<ScanRun> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO scan_runs (id, config_id, status, tickers_scanned, opportunities_found, created_at, updated_at)
             VALUES (?1, ?2, 'queued', 0, 0, ?3, ?4)",
            params![id, config_id, encode_time(now), encode_time(now)],
        )?;

        self.get_scan_run(&id)
            .await?
            .context("scan run missing after insert")
    }

    pub async fn get_scan_run(&self, scan_run_id: &str) -> Result<Option<ScanRun>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM scan_runs WHERE id = ?1",
            [scan_run_id],
            map_scan_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn set_scan_run_status(&self, scan_run_id: &str, status: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE scan_runs SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, encode_time(Utc::now()), scan_run_id],
        )?;
        Ok(())
    }

    pub async fn update_scan_run_progress(
        &self,
        scan_run_id: &str,
        tickers_scanned: i64,
        opportunities_found: i64,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE scan_runs SET tickers_scanned = ?1, opportunities_found = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                tickers_scanned,
                opportunities_found,
                encode_time(Utc::now()),
                scan_run_id
            ],
        )?;
        Ok(())
    }

    pub async fn complete_scan_run(
        &self,
        scan_run_id: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let status = if error_message.is_some() {
            "failed"
        } else {
            "completed"
        };
        conn.execute(
            "UPDATE scan_runs SET status = ?1, completed_at = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5",
            params![
                status,
                encode_time(Utc::now()),
                error_message,
                encode_time(Utc::now()),
                scan_run_id
            ],
        )?;
        Ok(())
    }

    pub async fn list_scan_runs(&self, limit: i64) -> Result<Vec<ScanRun>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM scan_runs ORDER BY created_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_scan_run)?;
        collect_rows(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_scan_opportunity(
        &self,
        scan_run_id: &str,
        ticker: &str,
        overall_score: f64,
        signal_strength_score: f64,
        thesis_quality_score: Option<f64>,
        coverage_gap_score: f64,
        timing_score: f64,
        signals_json: &str,
        preliminary_thesis_markdown: Option<&str>,
        preliminary_thesis_html: Option<&str>,
        key_catalysts: Option<&str>,
        risk_factors: Option<&str>,
    ) -> Result<ScanOpportunity> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO scan_opportunities
             (id, scan_run_id, ticker, overall_score, signal_strength_score, thesis_quality_score, coverage_gap_score, timing_score, signals_json, preliminary_thesis_markdown, preliminary_thesis_html, key_catalysts, risk_factors, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'new', ?14, ?15)",
            params![
                id,
                scan_run_id,
                ticker,
                overall_score,
                signal_strength_score,
                thesis_quality_score,
                coverage_gap_score,
                timing_score,
                signals_json,
                preliminary_thesis_markdown,
                preliminary_thesis_html,
                key_catalysts,
                risk_factors,
                encode_time(now),
                encode_time(now),
            ],
        )?;

        self.get_scan_opportunity(&id)
            .await?
            .context("scan opportunity missing after insert")
    }

    pub async fn get_scan_opportunity(
        &self,
        opportunity_id: &str,
    ) -> Result<Option<ScanOpportunity>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM scan_opportunities WHERE id = ?1",
            [opportunity_id],
            map_scan_opportunity,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_scan_opportunities_for_run(
        &self,
        scan_run_id: &str,
    ) -> Result<Vec<ScanOpportunity>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM scan_opportunities WHERE scan_run_id = ?1 ORDER BY overall_score DESC",
        )?;
        let rows = statement.query_map([scan_run_id], map_scan_opportunity)?;
        collect_rows(rows)
    }

    pub async fn list_top_scan_opportunities(&self, limit: i64) -> Result<Vec<ScanOpportunity>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM scan_opportunities
             WHERE status = 'new'
             ORDER BY overall_score DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], map_scan_opportunity)?;
        collect_rows(rows)
    }

    pub async fn promote_scan_opportunity(
        &self,
        opportunity_id: &str,
        run_id: &str,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE scan_opportunities SET promoted_to_run_id = ?1, status = 'promoted', updated_at = ?2 WHERE id = ?3",
            params![run_id, encode_time(Utc::now()), opportunity_id],
        )?;
        Ok(affected > 0)
    }

    pub async fn dismiss_scan_opportunity(&self, opportunity_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE scan_opportunities SET status = 'dismissed', updated_at = ?1 WHERE id = ?2",
            params![encode_time(Utc::now()), opportunity_id],
        )?;
        Ok(affected > 0)
    }

    pub async fn seed_sp500_universe(&self) -> Result<i64> {
        let count = self.count_ticker_universe(false).await?;
        if count > 0 {
            return Ok(0);
        }

        let seed_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sql/seed_sp500.sql");
        let seed_sql =
            fs::read_to_string(&seed_path).context("failed to read S&P 500 seed file")?;
        let conn = self.open_connection()?;
        conn.execute_batch(&seed_sql)?;

        self.count_ticker_universe(false).await
    }

    // Multi-Model Research Panel database methods

    #[allow(clippy::too_many_arguments)]
    pub async fn create_llm_provider(
        &self,
        name: &str,
        provider_type: &str,
        api_key_encrypted: Option<&str>,
        model: &str,
        base_url: Option<&str>,
        is_enabled: bool,
        is_default: bool,
        priority: i64,
        config_json: Option<&str>,
    ) -> Result<LlmProvider> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO llm_providers (id, name, provider_type, api_key_encrypted, model, base_url, is_enabled, is_default, priority, config_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id, name, provider_type, api_key_encrypted, model, base_url,
                if is_enabled { 1 } else { 0 }, if is_default { 1 } else { 0 },
                priority, config_json, encode_time(now), encode_time(now),
            ],
        )?;
        self.get_llm_provider(&id)
            .await?
            .context("created provider missing")
    }

    pub async fn get_llm_provider(&self, provider_id: &str) -> Result<Option<LlmProvider>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM llm_providers WHERE id = ?1",
            [provider_id],
            map_llm_provider,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_llm_providers(&self, enabled_only: bool) -> Result<Vec<LlmProvider>> {
        let conn = self.open_connection()?;
        let sql = if enabled_only {
            "SELECT * FROM llm_providers WHERE is_enabled = 1 ORDER BY priority DESC, name ASC"
        } else {
            "SELECT * FROM llm_providers ORDER BY priority DESC, name ASC"
        };
        let mut statement = conn.prepare(sql)?;
        let rows = statement.query_map([], map_llm_provider)?;
        collect_rows(rows)
    }

    pub async fn delete_llm_provider(&self, provider_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute("DELETE FROM llm_providers WHERE id = ?1", [provider_id])?;
        Ok(affected > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_model_run(
        &self,
        run_id: &str,
        provider_id: &str,
        iteration_number: Option<i64>,
        output_type: &str,
        output_content: Option<&str>,
        tokens_used: Option<i64>,
        latency_ms: Option<i64>,
        cost_estimate: Option<f64>,
        quality_score: Option<f64>,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<ModelRun> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO model_runs (id, run_id, provider_id, iteration_number, output_type, output_content, tokens_used, latency_ms, cost_estimate, quality_score, status, error_message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                id, run_id, provider_id, iteration_number, output_type, output_content,
                tokens_used, latency_ms, cost_estimate, quality_score, status, error_message,
                encode_time(now),
            ],
        )?;
        self.get_model_run(&id).await?.context("model run missing")
    }

    pub async fn get_model_run(&self, id: &str) -> Result<Option<ModelRun>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM model_runs WHERE id = ?1",
            [id],
            map_model_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_model_runs_for_run(&self, run_id: &str) -> Result<Vec<ModelRun>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM model_runs WHERE run_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map([run_id], map_model_run)?;
        collect_rows(rows)
    }

    pub async fn create_model_comparison(
        &self,
        run_id: &str,
        comparison_type: &str,
        winner_provider_id: Option<&str>,
        comparison_json: &str,
        similarity_score: Option<f64>,
        key_differences: Option<&str>,
    ) -> Result<ModelComparison> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO model_comparisons (id, run_id, comparison_type, winner_provider_id, comparison_json, similarity_score, key_differences, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, run_id, comparison_type, winner_provider_id, comparison_json, similarity_score, key_differences, encode_time(now)],
        )?;
        self.get_model_comparison(&id)
            .await?
            .context("comparison missing")
    }

    pub async fn get_model_comparison(&self, id: &str) -> Result<Option<ModelComparison>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM model_comparisons WHERE id = ?1",
            [id],
            map_model_comparison,
        )
        .optional()
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_model_quality_score(
        &self,
        provider_id: &str,
        total_runs: i64,
        successful_runs: i64,
        avg_quality_score: Option<f64>,
        avg_latency_ms: Option<f64>,
        total_tokens: i64,
        total_cost: f64,
        accuracy_score: Option<f64>,
    ) -> Result<ModelQualityScore> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO model_quality_scores (id, provider_id, total_runs, successful_runs, avg_quality_score, avg_latency_ms, total_tokens, total_cost, accuracy_score, last_run_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(provider_id) DO UPDATE SET
               total_runs = excluded.total_runs, successful_runs = excluded.successful_runs,
               avg_quality_score = excluded.avg_quality_score, avg_latency_ms = excluded.avg_latency_ms,
               total_tokens = excluded.total_tokens, total_cost = excluded.total_cost,
               accuracy_score = excluded.accuracy_score, last_run_at = excluded.last_run_at, updated_at = excluded.updated_at",
            params![id, provider_id, total_runs, successful_runs, avg_quality_score, avg_latency_ms, total_tokens, total_cost, accuracy_score, encode_time(now), encode_time(now), encode_time(now)],
        )?;
        self.get_model_quality_score(provider_id)
            .await?
            .context("quality score missing")
    }

    pub async fn get_model_quality_score(
        &self,
        provider_id: &str,
    ) -> Result<Option<ModelQualityScore>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM model_quality_scores WHERE provider_id = ?1",
            [provider_id],
            map_model_quality_score,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_model_quality_scores(&self) -> Result<Vec<ModelQualityScore>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM model_quality_scores ORDER BY accuracy_score DESC NULLS LAST",
        )?;
        let rows = statement.query_map([], map_model_quality_score)?;
        collect_rows(rows)
    }

    // Performance Tracking database methods

    #[allow(clippy::too_many_arguments)]
    pub async fn create_price_snapshot(
        &self,
        ticker: &str,
        price_date: chrono::NaiveDate,
        open_price: f64,
        close_price: f64,
        high_price: Option<f64>,
        low_price: Option<f64>,
        volume: Option<i64>,
        adjusted_close: Option<f64>,
        source: &str,
    ) -> Result<PriceSnapshot> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO price_snapshots (id, ticker, price_date, open_price, close_price, high_price, low_price, volume, adjusted_close, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![id, ticker, price_date.to_string(), open_price, close_price, high_price, low_price, volume, adjusted_close, source, encode_time(now)],
        )?;
        self.get_price_snapshot_by_ticker_date(ticker, price_date)
            .await?
            .context("price snapshot missing")
    }

    pub async fn get_price_snapshot_by_ticker_date(
        &self,
        ticker: &str,
        price_date: chrono::NaiveDate,
    ) -> Result<Option<PriceSnapshot>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM price_snapshots WHERE ticker = ?1 AND price_date = ?2",
            params![ticker, price_date.to_string()],
            map_price_snapshot,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn get_latest_price_snapshot(&self, ticker: &str) -> Result<Option<PriceSnapshot>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM price_snapshots WHERE ticker = ?1 ORDER BY price_date DESC LIMIT 1",
            [ticker],
            map_price_snapshot,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn create_thesis_outcome(
        &self,
        run_id: &str,
        ticker: &str,
        thesis_date: chrono::NaiveDate,
        thesis_price: f64,
    ) -> Result<ThesisOutcome> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO thesis_outcomes (id, run_id, ticker, thesis_date, thesis_price, last_updated, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, run_id, ticker, thesis_date.to_string(), thesis_price, encode_time(now), encode_time(now)],
        )?;
        self.get_thesis_outcome(&id)
            .await?
            .context("thesis outcome missing")
    }

    pub async fn get_thesis_outcome(&self, id: &str) -> Result<Option<ThesisOutcome>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM thesis_outcomes WHERE id = ?1",
            [id],
            map_thesis_outcome,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn get_thesis_outcome_for_run(&self, run_id: &str) -> Result<Option<ThesisOutcome>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM thesis_outcomes WHERE run_id = ?1",
            [run_id],
            map_thesis_outcome,
        )
        .optional()
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_thesis_outcome_returns(
        &self,
        id: &str,
        return_1d: Option<f64>,
        return_7d: Option<f64>,
        return_30d: Option<f64>,
        return_90d: Option<f64>,
        price_1d: Option<f64>,
        price_7d: Option<f64>,
        price_30d: Option<f64>,
        price_90d: Option<f64>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE thesis_outcomes SET return_1d = ?1, return_7d = ?2, return_30d = ?3, return_90d = ?4, price_1d = ?5, price_7d = ?6, price_30d = ?7, price_90d = ?8, last_updated = ?9 WHERE id = ?10",
            params![return_1d, return_7d, return_30d, return_90d, price_1d, price_7d, price_30d, price_90d, encode_time(Utc::now()), id],
        )?;
        Ok(())
    }

    pub async fn list_thesis_outcomes_for_ticker(
        &self,
        ticker: &str,
    ) -> Result<Vec<ThesisOutcome>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare("SELECT * FROM thesis_outcomes WHERE ticker = ?1 ORDER BY thesis_date DESC")?;
        let rows = statement.query_map([ticker], map_thesis_outcome)?;
        collect_rows(rows)
    }

    pub async fn list_recent_thesis_outcomes(&self, limit: i64) -> Result<Vec<ThesisOutcome>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM thesis_outcomes ORDER BY thesis_date DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_thesis_outcome)?;
        collect_rows(rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_thesis_accuracy(
        &self,
        ticker: Option<&str>,
        provider_id: Option<&str>,
        time_horizon: &str,
        total_theses: i64,
        correct_theses: i64,
        accuracy_rate: Option<f64>,
        avg_return: Option<f64>,
    ) -> Result<ThesisAccuracy> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO thesis_accuracy (id, ticker, provider_id, time_horizon, total_theses, correct_theses, accuracy_rate, avg_return, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(ticker, provider_id, time_horizon) DO UPDATE SET
               total_theses = excluded.total_theses, correct_theses = excluded.correct_theses,
               accuracy_rate = excluded.accuracy_rate, avg_return = excluded.avg_return, updated_at = excluded.updated_at",
            params![id, ticker, provider_id, time_horizon, total_theses, correct_theses, accuracy_rate, avg_return, encode_time(now), encode_time(now)],
        )?;
        self.get_thesis_accuracy(ticker, provider_id, time_horizon)
            .await?
            .context("accuracy missing")
    }

    pub async fn get_thesis_accuracy(
        &self,
        ticker: Option<&str>,
        provider_id: Option<&str>,
        time_horizon: &str,
    ) -> Result<Option<ThesisAccuracy>> {
        let conn = self.open_connection()?;
        conn.query_row("SELECT * FROM thesis_accuracy WHERE ticker IS ?1 AND provider_id IS ?2 AND time_horizon = ?3", params![ticker, provider_id, time_horizon], map_thesis_accuracy)
            .optional().map_err(Into::into)
    }

    pub async fn list_thesis_accuracy_by_horizon(&self) -> Result<Vec<ThesisAccuracy>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare("SELECT * FROM thesis_accuracy WHERE ticker IS NULL AND provider_id IS NULL ORDER BY time_horizon")?;
        let rows = statement.query_map([], map_thesis_accuracy)?;
        collect_rows(rows)
    }

    // Evidence Quality database methods

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

    // Historical Analytics database methods

    pub async fn create_thesis_history(
        &self,
        run_id: &str,
        ticker: &str,
        thesis_date: chrono::NaiveDate,
        thesis_markdown: &str,
        model_provider_id: Option<&str>,
    ) -> Result<ThesisHistory> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO thesis_history (id, run_id, ticker, thesis_date, thesis_markdown, model_provider_id, archived_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, run_id, ticker, thesis_date.to_string(), thesis_markdown, model_provider_id, encode_time(now)],
        )?;
        self.get_thesis_history(&id)
            .await?
            .context("thesis history missing")
    }

    pub async fn get_thesis_history(&self, id: &str) -> Result<Option<ThesisHistory>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM thesis_history WHERE id = ?1",
            [id],
            map_thesis_history,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_thesis_history_for_ticker(
        &self,
        ticker: &str,
        limit: i64,
    ) -> Result<Vec<ThesisHistory>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM thesis_history WHERE ticker = ?1 ORDER BY thesis_date DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![ticker, limit], map_thesis_history)?;
        collect_rows(rows)
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

    pub async fn create_signal_effectiveness(
        &self,
        signal_type: &str,
        signal_date: chrono::NaiveDate,
        ticker: &str,
        signal_strength: f64,
        return_30d: Option<f64>,
        was_predictive: Option<bool>,
    ) -> Result<SignalEffectiveness> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO signal_effectiveness (id, signal_type, signal_date, ticker, signal_strength, return_30d, was_predictive, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, signal_type, signal_date.to_string(), ticker, signal_strength, return_30d, was_predictive.map(|b| if b { 1 } else { 0 }), encode_time(now)],
        )?;
        self.get_signal_effectiveness(&id)
            .await?
            .context("signal effectiveness missing")
    }

    pub async fn get_signal_effectiveness(&self, id: &str) -> Result<Option<SignalEffectiveness>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM signal_effectiveness WHERE id = ?1",
            [id],
            map_signal_effectiveness,
        )
        .optional()
        .map_err(Into::into)
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

fn map_batch_job(row: &Row<'_>) -> rusqlite::Result<BatchJob> {
    Ok(BatchJob {
        id: row.get("id")?,
        name: row.get("name")?,
        question_template: row.get("question_template")?,
        status: row.get("status")?,
        summary: row.get("summary")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_run_template(row: &Row<'_>) -> rusqlite::Result<RunTemplate> {
    Ok(RunTemplate {
        id: row.get("id")?,
        name: row.get("name")?,
        question_template: row.get("question_template")?,
        description: row.get("description")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_alert_rule(row: &Row<'_>) -> rusqlite::Result<AlertRule> {
    Ok(AlertRule {
        id: row.get("id")?,
        watchlist_id: row.get("watchlist_id")?,
        rule_type: row.get("rule_type")?,
        threshold: row.get("threshold")?,
        enabled: row.get::<_, i64>("enabled")? > 0,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_thesis_alert(row: &Row<'_>) -> rusqlite::Result<ThesisAlert> {
    Ok(ThesisAlert {
        id: row.get("id")?,
        watchlist_id: row.get("watchlist_id")?,
        ticker: row.get("ticker")?,
        run_id: row.get("run_id")?,
        alert_type: row.get("alert_type")?,
        severity: row.get("severity")?,
        message: row.get("message")?,
        status: row.get("status")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_watchlist(row: &Row<'_>) -> rusqlite::Result<Watchlist> {
    Ok(Watchlist {
        id: row.get("id")?,
        name: row.get("name")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_watchlist_ticker(row: &Row<'_>) -> rusqlite::Result<WatchlistTicker> {
    Ok(WatchlistTicker {
        id: row.get("id")?,
        watchlist_id: row.get("watchlist_id")?,
        ticker: row.get("ticker")?,
        sort_order: row.get("sort_order")?,
        created_at: parse_time(row.get("created_at")?)?,
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

fn map_ticker_universe(row: &Row<'_>) -> rusqlite::Result<TickerUniverse> {
    Ok(TickerUniverse {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        name: row.get("name")?,
        sector: row.get("sector")?,
        industry: row.get("industry")?,
        market_cap_billion: row.get("market_cap_billion")?,
        is_sp500: row.get::<_, i64>("is_sp500")? > 0,
        is_active: row.get::<_, i64>("is_active")? > 0,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_scanner_config(row: &Row<'_>) -> rusqlite::Result<ScannerConfig> {
    Ok(ScannerConfig {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        universe_filter: row.get("universe_filter")?,
        sector_filter: row.get("sector_filter")?,
        min_market_cap: row.get("min_market_cap")?,
        max_market_cap: row.get("max_market_cap")?,
        max_opportunities: row.get("max_opportunities")?,
        signal_weights_json: row.get("signal_weights_json")?,
        is_active: row.get::<_, i64>("is_active")? > 0,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_scan_run(row: &Row<'_>) -> rusqlite::Result<ScanRun> {
    Ok(ScanRun {
        id: row.get("id")?,
        config_id: row.get("config_id")?,
        status: row.get("status")?,
        tickers_scanned: row.get("tickers_scanned")?,
        opportunities_found: row.get("opportunities_found")?,
        started_at: row
            .get::<_, Option<String>>("started_at")?
            .and_then(|s| parse_time(s).ok()),
        completed_at: row
            .get::<_, Option<String>>("completed_at")?
            .and_then(|s| parse_time(s).ok()),
        error_message: row.get("error_message")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_scan_opportunity(row: &Row<'_>) -> rusqlite::Result<ScanOpportunity> {
    Ok(ScanOpportunity {
        id: row.get("id")?,
        scan_run_id: row.get("scan_run_id")?,
        ticker: row.get("ticker")?,
        overall_score: row.get("overall_score")?,
        signal_strength_score: row.get("signal_strength_score")?,
        thesis_quality_score: row.get("thesis_quality_score")?,
        coverage_gap_score: row.get("coverage_gap_score")?,
        timing_score: row.get("timing_score")?,
        signals_json: row.get("signals_json")?,
        preliminary_thesis_markdown: row.get("preliminary_thesis_markdown")?,
        preliminary_thesis_html: row.get("preliminary_thesis_html")?,
        key_catalysts: row.get("key_catalysts")?,
        risk_factors: row.get("risk_factors")?,
        promoted_to_run_id: row.get("promoted_to_run_id")?,
        status: row.get("status")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_llm_provider(row: &Row<'_>) -> rusqlite::Result<LlmProvider> {
    Ok(LlmProvider {
        id: row.get("id")?,
        name: row.get("name")?,
        provider_type: row.get("provider_type")?,
        api_key_encrypted: row.get("api_key_encrypted")?,
        model: row.get("model")?,
        base_url: row.get("base_url")?,
        is_enabled: row.get::<_, i64>("is_enabled")? > 0,
        is_default: row.get::<_, i64>("is_default")? > 0,
        priority: row.get("priority")?,
        config_json: row.get("config_json")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_model_run(row: &Row<'_>) -> rusqlite::Result<ModelRun> {
    Ok(ModelRun {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        provider_id: row.get("provider_id")?,
        iteration_number: row.get("iteration_number")?,
        output_type: row.get("output_type")?,
        output_content: row.get("output_content")?,
        tokens_used: row.get("tokens_used")?,
        latency_ms: row.get("latency_ms")?,
        cost_estimate: row.get("cost_estimate")?,
        quality_score: row.get("quality_score")?,
        status: row.get("status")?,
        error_message: row.get("error_message")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_model_comparison(row: &Row<'_>) -> rusqlite::Result<ModelComparison> {
    Ok(ModelComparison {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        comparison_type: row.get("comparison_type")?,
        winner_provider_id: row.get("winner_provider_id")?,
        comparison_json: row.get("comparison_json")?,
        similarity_score: row.get("similarity_score")?,
        key_differences: row.get("key_differences")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_model_quality_score(row: &Row<'_>) -> rusqlite::Result<ModelQualityScore> {
    Ok(ModelQualityScore {
        id: row.get("id")?,
        provider_id: row.get("provider_id")?,
        total_runs: row.get("total_runs")?,
        successful_runs: row.get("successful_runs")?,
        avg_quality_score: row.get("avg_quality_score")?,
        avg_latency_ms: row.get("avg_latency_ms")?,
        total_tokens: row.get("total_tokens")?,
        total_cost: row.get("total_cost")?,
        accuracy_score: row.get("accuracy_score")?,
        last_run_at: row
            .get::<_, Option<String>>("last_run_at")?
            .and_then(|s| parse_time(s).ok()),
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_price_snapshot(row: &Row<'_>) -> rusqlite::Result<PriceSnapshot> {
    Ok(PriceSnapshot {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        price_date: row
            .get::<_, String>("price_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        open_price: row.get("open_price")?,
        close_price: row.get("close_price")?,
        high_price: row.get("high_price")?,
        low_price: row.get("low_price")?,
        volume: row.get("volume")?,
        adjusted_close: row.get("adjusted_close")?,
        source: row.get("source")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_thesis_outcome(row: &Row<'_>) -> rusqlite::Result<ThesisOutcome> {
    Ok(ThesisOutcome {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        ticker: row.get("ticker")?,
        thesis_date: row
            .get::<_, String>("thesis_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        thesis_price: row.get("thesis_price")?,
        return_1d: row.get("return_1d")?,
        return_7d: row.get("return_7d")?,
        return_30d: row.get("return_30d")?,
        return_90d: row.get("return_90d")?,
        return_180d: row.get("return_180d")?,
        return_365d: row.get("return_365d")?,
        price_1d: row.get("price_1d")?,
        price_7d: row.get("price_7d")?,
        price_30d: row.get("price_30d")?,
        price_90d: row.get("price_90d")?,
        price_180d: row.get("price_180d")?,
        price_365d: row.get("price_365d")?,
        thesis_direction: row.get("thesis_direction")?,
        thesis_correct_1d: row
            .get::<_, Option<i64>>("thesis_correct_1d")?
            .map(|v| v > 0),
        thesis_correct_7d: row
            .get::<_, Option<i64>>("thesis_correct_7d")?
            .map(|v| v > 0),
        thesis_correct_30d: row
            .get::<_, Option<i64>>("thesis_correct_30d")?
            .map(|v| v > 0),
        thesis_correct_90d: row
            .get::<_, Option<i64>>("thesis_correct_90d")?
            .map(|v| v > 0),
        notes: row.get("notes")?,
        last_updated: parse_time(row.get("last_updated")?)?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_thesis_accuracy(row: &Row<'_>) -> rusqlite::Result<ThesisAccuracy> {
    Ok(ThesisAccuracy {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        provider_id: row.get("provider_id")?,
        time_horizon: row.get("time_horizon")?,
        total_theses: row.get("total_theses")?,
        correct_theses: row.get("correct_theses")?,
        accuracy_rate: row.get("accuracy_rate")?,
        avg_return: row.get("avg_return")?,
        median_return: row.get("median_return")?,
        best_return: row.get("best_return")?,
        worst_return: row.get("worst_return")?,
        sharpe_ratio: row.get("sharpe_ratio")?,
        win_rate: row.get("win_rate")?,
        avg_holding_days: row.get("avg_holding_days")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_source_reputation(row: &Row<'_>) -> rusqlite::Result<SourceReputation> {
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

fn map_evidence_outcome(row: &Row<'_>) -> rusqlite::Result<EvidenceOutcome> {
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

fn map_thesis_history(row: &Row<'_>) -> rusqlite::Result<ThesisHistory> {
    Ok(ThesisHistory {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        ticker: row.get("ticker")?,
        thesis_date: row
            .get::<_, String>("thesis_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        thesis_markdown: row.get("thesis_markdown")?,
        thesis_html: row.get("thesis_html")?,
        executive_summary: row.get("executive_summary")?,
        bull_case: row.get("bull_case")?,
        bear_case: row.get("bear_case")?,
        key_catalysts: row.get("key_catalysts")?,
        key_risks: row.get("key_risks")?,
        conviction_level: row.get("conviction_level")?,
        thesis_direction: row.get("thesis_direction")?,
        model_provider_id: row.get("model_provider_id")?,
        signals_json: row.get("signals_json")?,
        iteration_number: row.get("iteration_number")?,
        archived_at: parse_time(row.get("archived_at")?)?,
    })
}

fn map_ticker_research_summary(row: &Row<'_>) -> rusqlite::Result<TickerResearchSummary> {
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

fn map_signal_effectiveness(row: &Row<'_>) -> rusqlite::Result<SignalEffectiveness> {
    Ok(SignalEffectiveness {
        id: row.get("id")?,
        signal_type: row.get("signal_type")?,
        signal_date: row
            .get::<_, String>("signal_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        ticker: row.get("ticker")?,
        signal_strength: row.get("signal_strength")?,
        signal_description: row.get("signal_description")?,
        outcome_type: row.get("outcome_type")?,
        return_7d: row.get("return_7d")?,
        return_30d: row.get("return_30d")?,
        return_90d: row.get("return_90d")?,
        was_predictive: row.get::<_, Option<i64>>("was_predictive")?.map(|v| v > 0),
        thesis_run_id: row.get("thesis_run_id")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn map_research_analytics(row: &Row<'_>) -> rusqlite::Result<ResearchAnalytics> {
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

// ==================== Scheduler Methods ====================

impl Database {
    pub async fn get_watchlists_due_for_refresh(
        &self,
    ) -> Result<Vec<(Watchlist, WatchlistSchedule)>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM watchlists WHERE refresh_enabled = 1 AND next_refresh_at <= ?1",
        )?;
        let now = encode_time(Utc::now());
        let rows = statement.query_map([&now], |row| {
            let watchlist = map_watchlist(row)?;
            let schedule = WatchlistSchedule {
                watchlist_id: watchlist.id.clone(),
                refresh_enabled: row.get::<_, i64>("refresh_enabled")? > 0,
                refresh_interval_hours: row.get("refresh_interval_hours")?,
                last_refresh_at: row
                    .get::<_, Option<String>>("last_refresh_at")?
                    .and_then(|s| parse_time_opt(&s)),
                next_refresh_at: row
                    .get::<_, Option<String>>("next_refresh_at")?
                    .and_then(|s| parse_time_opt(&s)),
                refresh_template_id: row.get("refresh_template_id")?,
            };
            Ok((watchlist, schedule))
        })?;
        collect_rows(rows)
    }

    pub async fn get_watchlist_schedule(
        &self,
        watchlist_id: &str,
    ) -> Result<Option<WatchlistSchedule>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT id, refresh_enabled, refresh_interval_hours, last_refresh_at, next_refresh_at, refresh_template_id FROM watchlists WHERE id = ?1",
            [watchlist_id],
            |row| {
                Ok(WatchlistSchedule {
                    watchlist_id: row.get("id")?,
                    refresh_enabled: row.get::<_, i64>("refresh_enabled")? > 0,
                    refresh_interval_hours: row.get("refresh_interval_hours")?,
                    last_refresh_at: row.get::<_, Option<String>>("last_refresh_at")?.and_then(|s| parse_time_opt(&s)),
                    next_refresh_at: row.get::<_, Option<String>>("next_refresh_at")?.and_then(|s| parse_time_opt(&s)),
                    refresh_template_id: row.get("refresh_template_id")?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn update_watchlist_schedule(
        &self,
        watchlist_id: &str,
        enabled: bool,
        interval_hours: i64,
        template_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let now = Utc::now();
        let next_refresh_at = if enabled {
            Some(encode_time(now + chrono::Duration::hours(interval_hours)))
        } else {
            None
        };

        conn.execute(
            "UPDATE watchlists SET refresh_enabled = ?1, refresh_interval_hours = ?2, refresh_template_id = ?3, next_refresh_at = ?4, updated_at = ?5 WHERE id = ?6",
            params![
                if enabled { 1i64 } else { 0i64 },
                interval_hours,
                template_id,
                next_refresh_at,
                encode_time(now),
                watchlist_id,
            ],
        )?;
        Ok(())
    }

    pub async fn mark_watchlist_refreshed(
        &self,
        watchlist_id: &str,
        interval_hours: i64,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let now = Utc::now();
        let next_refresh_at = encode_time(now + chrono::Duration::hours(interval_hours));

        conn.execute(
            "UPDATE watchlists SET last_refresh_at = ?1, next_refresh_at = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                encode_time(now),
                next_refresh_at,
                encode_time(now),
                watchlist_id,
            ],
        )?;
        Ok(())
    }

    pub async fn create_scheduled_run(
        &self,
        watchlist_id: &str,
        ticker: &str,
        run_id: &str,
    ) -> Result<ScheduledRun> {
        let scheduled_run = ScheduledRun {
            id: Uuid::new_v4().to_string(),
            watchlist_id: watchlist_id.to_string(),
            ticker: ticker.to_string(),
            run_id: run_id.to_string(),
            scheduled_at: Utc::now(),
            started_at: None,
            completed_at: None,
            status: "pending".to_string(),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO scheduled_runs (id, watchlist_id, ticker, run_id, scheduled_at, started_at, completed_at, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                scheduled_run.id,
                scheduled_run.watchlist_id,
                scheduled_run.ticker,
                scheduled_run.run_id,
                encode_time(scheduled_run.scheduled_at),
                scheduled_run.started_at.map(encode_time),
                scheduled_run.completed_at.map(encode_time),
                scheduled_run.status,
                encode_time(scheduled_run.created_at),
            ],
        )?;

        Ok(scheduled_run)
    }

    pub async fn update_scheduled_run_started(&self, scheduled_run_id: &str) -> Result<()> {
        let conn = self.open_connection()?;
        let now = encode_time(Utc::now());
        conn.execute(
            "UPDATE scheduled_runs SET started_at = ?1, status = 'running' WHERE id = ?2",
            params![now, scheduled_run_id],
        )?;
        Ok(())
    }

    pub async fn update_scheduled_run_completed(
        &self,
        scheduled_run_id: &str,
        success: bool,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let now = encode_time(Utc::now());
        let status = if success { "completed" } else { "failed" };
        conn.execute(
            "UPDATE scheduled_runs SET completed_at = ?1, status = ?2 WHERE id = ?3",
            params![now, status, scheduled_run_id],
        )?;
        Ok(())
    }

    pub async fn list_scheduled_runs(
        &self,
        watchlist_id: &str,
        limit: i64,
    ) -> Result<Vec<ScheduledRun>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM scheduled_runs WHERE watchlist_id = ?1 ORDER BY scheduled_at DESC LIMIT ?2"
        )?;
        let rows = statement.query_map(params![watchlist_id, limit], map_scheduled_run)?;
        collect_rows(rows)
    }

    pub async fn get_pending_scheduled_run_for_ticker(
        &self,
        watchlist_id: &str,
        ticker: &str,
    ) -> Result<Option<ScheduledRun>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM scheduled_runs WHERE watchlist_id = ?1 AND ticker = ?2 AND status IN ('pending', 'running')",
            params![watchlist_id, ticker],
            map_scheduled_run,
        )
        .optional()
        .map_err(Into::into)
    }
}

fn map_scheduled_run(row: &Row<'_>) -> rusqlite::Result<ScheduledRun> {
    Ok(ScheduledRun {
        id: row.get("id")?,
        watchlist_id: row.get("watchlist_id")?,
        ticker: row.get("ticker")?,
        run_id: row.get("run_id")?,
        scheduled_at: parse_time(row.get("scheduled_at")?)?,
        started_at: row
            .get::<_, Option<String>>("started_at")?
            .and_then(|s| parse_time_opt(&s)),
        completed_at: row
            .get::<_, Option<String>>("completed_at")?
            .and_then(|s| parse_time_opt(&s)),
        status: row.get("status")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

fn parse_time_opt(s: &str) -> Option<DateTime<Utc>> {
    parse_time(s.to_string()).ok()
}

// Portfolio database methods

impl Database {
    pub async fn list_portfolios(&self, limit: i64) -> Result<Vec<Portfolio>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM portfolios ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_portfolio)?;
        collect_rows(rows)
    }

    pub async fn get_portfolio(&self, portfolio_id: &str) -> Result<Option<Portfolio>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM portfolios WHERE id = ?1",
            [portfolio_id],
            map_portfolio,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn create_portfolio(
        &self,
        name: &str,
        description: Option<&str>,
        cash_balance: f64,
    ) -> Result<Portfolio> {
        let portfolio = Portfolio {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            cash_balance,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO portfolios (id, name, description, cash_balance, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                portfolio.id,
                portfolio.name,
                portfolio.description,
                portfolio.cash_balance,
                encode_time(portfolio.created_at),
                encode_time(portfolio.updated_at),
            ],
        )?;

        Ok(portfolio)
    }

    pub async fn update_portfolio(
        &self,
        portfolio_id: &str,
        name: &str,
        description: Option<&str>,
        cash_balance: f64,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let rows = conn.execute(
            "UPDATE portfolios SET name = ?1, description = ?2, cash_balance = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, description, cash_balance, encode_time(Utc::now()), portfolio_id],
        )?;
        Ok(rows > 0)
    }

    pub async fn delete_portfolio(&self, portfolio_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let rows = conn.execute("DELETE FROM portfolios WHERE id = ?1", [portfolio_id])?;
        Ok(rows > 0)
    }

    pub async fn list_positions(&self, portfolio_id: &str) -> Result<Vec<Position>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare("SELECT * FROM positions WHERE portfolio_id = ?1 ORDER BY opened_at DESC")?;
        let rows = statement.query_map([portfolio_id], map_position)?;
        collect_rows(rows)
    }

    /// Batch-count active positions per portfolio in a single query.
    /// Used by the portfolios index page to avoid N `list_active_positions`
    /// round-trips.
    pub async fn count_active_positions_by_portfolio(
        &self,
    ) -> Result<std::collections::HashMap<String, i64>> {
        use std::collections::HashMap;
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT portfolio_id, COUNT(*) FROM positions WHERE is_active = 1 GROUP BY portfolio_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (portfolio_id, count) = row?;
            map.insert(portfolio_id, count);
        }
        Ok(map)
    }

    pub async fn list_active_positions(&self, portfolio_id: &str) -> Result<Vec<Position>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM positions WHERE portfolio_id = ?1 AND is_active = 1 ORDER BY opened_at DESC",
        )?;
        let rows = statement.query_map([portfolio_id], map_position)?;
        collect_rows(rows)
    }

    pub async fn get_position(&self, position_id: &str) -> Result<Option<Position>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM positions WHERE id = ?1",
            [position_id],
            map_position,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn create_position(
        &self,
        portfolio_id: &str,
        ticker: &str,
        shares: f64,
        cost_basis_per_share: f64,
        opened_at: chrono::NaiveDate,
        notes: Option<&str>,
    ) -> Result<Position> {
        let position = Position {
            id: Uuid::new_v4().to_string(),
            portfolio_id: portfolio_id.to_string(),
            ticker: ticker.to_uppercase(),
            shares,
            cost_basis_per_share,
            total_cost: shares * cost_basis_per_share,
            opened_at,
            closed_at: None,
            notes: notes.map(|s| s.to_string()),
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO positions (id, portfolio_id, ticker, shares, cost_basis_per_share, total_cost, opened_at, closed_at, notes, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                position.id,
                position.portfolio_id,
                position.ticker,
                position.shares,
                position.cost_basis_per_share,
                position.total_cost,
                position.opened_at.to_string(),
                position.closed_at.map(|d| d.to_string()),
                position.notes,
                position.is_active as i32,
                encode_time(position.created_at),
                encode_time(position.updated_at),
            ],
        )?;

        // Also create a transaction record
        self.create_transaction(
            portfolio_id,
            ticker,
            "buy",
            shares,
            cost_basis_per_share,
            opened_at,
            notes,
        )
        .await?;

        Ok(position)
    }

    pub async fn update_position(
        &self,
        position_id: &str,
        shares: f64,
        cost_basis_per_share: f64,
        notes: Option<&str>,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let total_cost = shares * cost_basis_per_share;
        let rows = conn.execute(
            "UPDATE positions SET shares = ?1, cost_basis_per_share = ?2, total_cost = ?3, notes = ?4, updated_at = ?5 WHERE id = ?6",
            params![shares, cost_basis_per_share, total_cost, notes, encode_time(Utc::now()), position_id],
        )?;
        Ok(rows > 0)
    }

    pub async fn close_position(
        &self,
        position_id: &str,
        closed_at: chrono::NaiveDate,
        notes: Option<&str>,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let rows = conn.execute(
            "UPDATE positions SET is_active = 0, closed_at = ?1, notes = ?2, updated_at = ?3 WHERE id = ?4",
            params![closed_at.to_string(), notes, encode_time(Utc::now()), position_id],
        )?;
        Ok(rows > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_transaction(
        &self,
        portfolio_id: &str,
        ticker: &str,
        transaction_type: &str,
        shares: f64,
        price_per_share: f64,
        executed_at: chrono::NaiveDate,
        notes: Option<&str>,
    ) -> Result<Transaction> {
        let transaction = Transaction {
            id: Uuid::new_v4().to_string(),
            portfolio_id: portfolio_id.to_string(),
            ticker: ticker.to_uppercase(),
            transaction_type: transaction_type.to_string(),
            shares,
            price_per_share,
            total_amount: shares * price_per_share,
            executed_at,
            notes: notes.map(|s| s.to_string()),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO transactions (id, portfolio_id, ticker, transaction_type, shares, price_per_share, total_amount, executed_at, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                transaction.id,
                transaction.portfolio_id,
                transaction.ticker,
                transaction.transaction_type,
                transaction.shares,
                transaction.price_per_share,
                transaction.total_amount,
                transaction.executed_at.to_string(),
                transaction.notes,
                encode_time(transaction.created_at),
            ],
        )?;

        Ok(transaction)
    }

    pub async fn list_transactions(
        &self,
        portfolio_id: &str,
        limit: i64,
    ) -> Result<Vec<Transaction>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM transactions WHERE portfolio_id = ?1 ORDER BY executed_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![portfolio_id, limit], map_transaction)?;
        collect_rows(rows)
    }
}

fn map_portfolio(row: &Row<'_>) -> rusqlite::Result<Portfolio> {
    Ok(Portfolio {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        cash_balance: row.get("cash_balance")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_position(row: &Row<'_>) -> rusqlite::Result<Position> {
    let opened_at_str: String = row.get("opened_at")?;
    let closed_at_str: Option<String> = row.get("closed_at")?;

    Ok(Position {
        id: row.get("id")?,
        portfolio_id: row.get("portfolio_id")?,
        ticker: row.get("ticker")?,
        shares: row.get("shares")?,
        cost_basis_per_share: row.get("cost_basis_per_share")?,
        total_cost: row.get("total_cost")?,
        opened_at: chrono::NaiveDate::parse_from_str(&opened_at_str, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().date_naive()),
        closed_at: closed_at_str
            .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        notes: row.get("notes")?,
        is_active: row.get::<_, i32>("is_active")? == 1,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

fn map_transaction(row: &Row<'_>) -> rusqlite::Result<Transaction> {
    let executed_at_str: String = row.get("executed_at")?;

    Ok(Transaction {
        id: row.get("id")?,
        portfolio_id: row.get("portfolio_id")?,
        ticker: row.get("ticker")?,
        transaction_type: row.get("transaction_type")?,
        shares: row.get("shares")?,
        price_per_share: row.get("price_per_share")?,
        total_amount: row.get("total_amount")?,
        executed_at: chrono::NaiveDate::parse_from_str(&executed_at_str, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().date_naive()),
        notes: row.get("notes")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}
