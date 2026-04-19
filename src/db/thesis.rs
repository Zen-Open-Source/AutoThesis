//! Long-running thesis tracking: outcomes (realized P&L), accuracy aggregates,
//! and historical archived versions of theses.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{ThesisAccuracy, ThesisHistory, ThesisOutcome};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
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
}

pub(crate) fn map_thesis_outcome(row: &Row<'_>) -> rusqlite::Result<ThesisOutcome> {
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

pub(crate) fn map_thesis_accuracy(row: &Row<'_>) -> rusqlite::Result<ThesisAccuracy> {
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

pub(crate) fn map_thesis_history(row: &Row<'_>) -> rusqlite::Result<ThesisHistory> {
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
