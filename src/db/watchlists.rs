//! Watchlists, their tickers, per-watchlist alert rules, and thesis alerts.
//!
//! Thesis alerts live here (rather than in `thesis.rs`) because they are
//! raised relative to a watchlist and evaluated on every dashboard refresh;
//! `thesis.rs` is reserved for the longer-term outcome / accuracy / history
//! tracking tables.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{AlertRule, ThesisAlert, Watchlist, WatchlistTicker};

use super::{collect_rows, encode_time, parse_time, Database};

const ALERT_RULE_SCORE_DROP: &str = "score_drop";
const ALERT_RULE_FRESHNESS_STALE: &str = "freshness_stale";
const ALERT_RULE_DECISION_DOWNGRADE: &str = "decision_downgrade";

impl Database {
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
}

pub(crate) fn map_watchlist(row: &Row<'_>) -> rusqlite::Result<Watchlist> {
    Ok(Watchlist {
        id: row.get("id")?,
        name: row.get("name")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

pub(crate) fn map_watchlist_ticker(row: &Row<'_>) -> rusqlite::Result<WatchlistTicker> {
    Ok(WatchlistTicker {
        id: row.get("id")?,
        watchlist_id: row.get("watchlist_id")?,
        ticker: row.get("ticker")?,
        sort_order: row.get("sort_order")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_alert_rule(row: &Row<'_>) -> rusqlite::Result<AlertRule> {
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

pub(crate) fn map_thesis_alert(row: &Row<'_>) -> rusqlite::Result<ThesisAlert> {
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
