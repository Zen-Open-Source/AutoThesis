//! Scheduled refresh runs driven by `watchlists` with a `refresh_interval_hours`.
//!
//! The watchlist row itself carries the schedule columns (for simplicity), so
//! `get_watchlist_schedule`/`update_watchlist_schedule` manipulate watchlists,
//! while scheduled_runs holds the historical per-execution records.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{ScheduledRun, Watchlist, WatchlistSchedule};

use super::{collect_rows, encode_time, parse_time, parse_time_opt, Database};

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
            let watchlist = super::watchlists::map_watchlist(row)?;
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

pub(crate) fn map_scheduled_run(row: &Row<'_>) -> rusqlite::Result<ScheduledRun> {
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
