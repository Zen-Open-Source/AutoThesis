//! Batch jobs: run the same question against a set of tickers.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{BatchJob, BatchJobRun, BatchJobRunWithDetails, Run};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
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
}

pub(crate) fn map_batch_job(row: &Row<'_>) -> rusqlite::Result<BatchJob> {
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
