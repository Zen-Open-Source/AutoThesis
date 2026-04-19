//! Runs, iterations, events, and iteration-detail composition.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Row};
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::{EventRecord, Iteration, IterationDetail, Run};

use super::{collect_rows, encode_time, parse_time, parse_time_opt, Database};

impl Database {
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

    /// Status-only projection of `get_run`. Used by the orchestrator's
    /// cancellation-fallback hot path so we don't materialise an entire
    /// `Run` row just to read one column.
    pub async fn get_run_status(&self, run_id: &str) -> Result<Option<String>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT status FROM runs WHERE id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
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

    /// Batch variant of [`Database::get_latest_iteration_evaluation_score`].
    /// Returns a map from run_id to score for the latest iteration with an
    /// `evaluation_json` payload. Missing runs / runs with no evaluated
    /// iteration are simply absent from the map.
    pub async fn latest_scores_for_runs(
        &self,
        run_ids: &[String],
    ) -> Result<HashMap<String, f64>> {
        if run_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.open_connection()?;
        let placeholders: String = (0..run_ids.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT i.run_id, i.evaluation_json
             FROM iterations i
             INNER JOIN (
                SELECT run_id, MAX(iteration_number) AS max_n
                FROM iterations
                WHERE run_id IN ({placeholders}) AND evaluation_json IS NOT NULL
                GROUP BY run_id
             ) latest ON latest.run_id = i.run_id AND latest.max_n = i.iteration_number"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            run_ids.iter().map(|r| r as &dyn rusqlite::ToSql).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (run_id, raw) = row?;
            if let Some(raw) = raw {
                if let Some(score) = serde_json::from_str::<serde_json::Value>(&raw)
                    .ok()
                    .and_then(|value| value.get("score").and_then(|s| s.as_f64()))
                {
                    map.insert(run_id, score);
                }
            }
        }
        Ok(map)
    }

    /// Batch variant of [`Database::get_latest_source_timestamp_for_run`].
    /// Returns a map of run_id to the most recent source timestamp.
    pub async fn latest_source_timestamps_for_runs(
        &self,
        run_ids: &[String],
    ) -> Result<HashMap<String, DateTime<Utc>>> {
        if run_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.open_connection()?;
        let placeholders: String = (0..run_ids.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT run_id, MAX(COALESCE(published_at, created_at)) AS latest
             FROM sources
             WHERE run_id IN ({placeholders})
             GROUP BY run_id"
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            run_ids.iter().map(|r| r as &dyn rusqlite::ToSql).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (run_id, raw) = row?;
            if let Some(raw) = raw {
                if let Some(ts) = parse_time_opt(&raw) {
                    map.insert(run_id, ts);
                }
            }
        }
        Ok(map)
    }

    /// Fetch up to `limit` most-recent runs for each ticker in one query.
    /// Used by the dashboard which needs the latest two runs per ticker.
    pub async fn recent_runs_for_tickers(
        &self,
        tickers: &[String],
        limit: i64,
    ) -> Result<HashMap<String, Vec<Run>>> {
        if tickers.is_empty() || limit <= 0 {
            return Ok(HashMap::new());
        }
        let conn = self.open_connection()?;
        let placeholders: String = (0..tickers.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let limit_placeholder = tickers.len() + 1;
        let sql = format!(
            "SELECT id, ticker, question, status, created_at, updated_at,
                    final_iteration_number, final_memo_markdown, final_memo_html, summary
             FROM (
                 SELECT r.*,
                        ROW_NUMBER() OVER (PARTITION BY ticker ORDER BY created_at DESC) AS rn
                 FROM runs r
                 WHERE ticker IN ({placeholders})
             ) WHERE rn <= ?{limit_placeholder}
             ORDER BY ticker, created_at DESC"
        );
        let mut params: Vec<&dyn rusqlite::ToSql> =
            tickers.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
        params.push(&limit as &dyn rusqlite::ToSql);
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), map_run)?;
        let mut map: HashMap<String, Vec<Run>> = HashMap::new();
        for row in rows {
            let run = row?;
            map.entry(run.ticker.clone()).or_default().push(run);
        }
        Ok(map)
    }

    /// Fetch the single most-recent run for each ticker in one query. Used
    /// by the ticker universe / watchlist read paths.
    pub async fn latest_runs_for_tickers(
        &self,
        tickers: &[String],
    ) -> Result<HashMap<String, Run>> {
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
}

pub(crate) fn map_run(row: &Row<'_>) -> rusqlite::Result<Run> {
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

pub(crate) fn map_iteration(row: &Row<'_>) -> rusqlite::Result<Iteration> {
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

pub(crate) fn map_event(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
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
