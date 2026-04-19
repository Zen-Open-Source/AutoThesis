//! Comparisons: multi-run side-by-side thesis comparisons.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{Comparison, ComparisonRun, ComparisonRunWithDetails, Run};

use super::{collect_rows, encode_time, parse_time, Database};

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

pub(crate) fn map_comparison(row: &Row<'_>) -> rusqlite::Result<Comparison> {
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
