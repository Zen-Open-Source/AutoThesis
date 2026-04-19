//! Scanner: scan configs, scan runs, opportunities, and signal effectiveness.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{ScanOpportunity, ScanRun, ScannerConfig, SignalEffectiveness};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
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
}

pub(crate) fn map_scanner_config(row: &Row<'_>) -> rusqlite::Result<ScannerConfig> {
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

pub(crate) fn map_scan_run(row: &Row<'_>) -> rusqlite::Result<ScanRun> {
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

pub(crate) fn map_scan_opportunity(row: &Row<'_>) -> rusqlite::Result<ScanOpportunity> {
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

pub(crate) fn map_signal_effectiveness(row: &Row<'_>) -> rusqlite::Result<SignalEffectiveness> {
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
