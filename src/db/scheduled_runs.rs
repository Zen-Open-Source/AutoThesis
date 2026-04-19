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
use crate::status::RunStatus;

use super::{collect_rows, encode_time, parse_time, parse_time_opt, Database};

/// Upper bound for the exponential backoff window applied to a watchlist
/// after `record_watchlist_refresh_failure`. Regardless of how many
/// consecutive failures have piled up, the next attempt will be at most
/// this far out so we still try periodically after an outage recovers.
const MAX_BACKOFF_HOURS: i64 = 24;

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
            let schedule = map_schedule_from_watchlist_row(row, &watchlist.id)?;
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
            "SELECT id, refresh_enabled, refresh_interval_hours, last_refresh_at, next_refresh_at, refresh_template_id, consecutive_failures, last_failure_at, last_failure_reason FROM watchlists WHERE id = ?1",
            [watchlist_id],
            |row| {
                let id: String = row.get("id")?;
                map_schedule_from_watchlist_row(row, &id)
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

    /// Record a successful refresh: clear the failure counter, stamp
    /// `last_refresh_at`, and schedule the next attempt one interval out.
    pub async fn record_watchlist_refresh_success(
        &self,
        watchlist_id: &str,
        interval_hours: i64,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let now = Utc::now();
        let next_refresh_at = encode_time(now + chrono::Duration::hours(interval_hours));

        conn.execute(
            "UPDATE watchlists SET
                last_refresh_at = ?1,
                next_refresh_at = ?2,
                consecutive_failures = 0,
                last_failure_at = NULL,
                last_failure_reason = NULL,
                updated_at = ?3
             WHERE id = ?4",
            params![
                encode_time(now),
                next_refresh_at,
                encode_time(now),
                watchlist_id,
            ],
        )?;
        Ok(())
    }

    /// Record a failed refresh: increment the failure counter, stamp the
    /// reason, and push `next_refresh_at` out using exponential backoff
    /// (`interval * 2^failures`, capped at `MAX_BACKOFF_HOURS`). Returns the
    /// newly-scheduled `next_refresh_at` so callers can log it.
    pub async fn record_watchlist_refresh_failure(
        &self,
        watchlist_id: &str,
        interval_hours: i64,
        reason: &str,
    ) -> Result<chrono::DateTime<Utc>> {
        let conn = self.open_connection()?;
        let now = Utc::now();

        // Read current failure count so we can compute a new backoff window.
        let current: i64 = conn
            .query_row(
                "SELECT consecutive_failures FROM watchlists WHERE id = ?1",
                [watchlist_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let next_failures = current.saturating_add(1);

        let shift = next_failures.clamp(0, 10) as u32;
        let backoff_hours = interval_hours
            .saturating_mul(1i64.checked_shl(shift).unwrap_or(i64::MAX))
            .min(MAX_BACKOFF_HOURS);
        let backoff_hours = backoff_hours.max(1);
        let next_refresh_at = now + chrono::Duration::hours(backoff_hours);

        conn.execute(
            "UPDATE watchlists SET
                consecutive_failures = ?1,
                last_failure_at = ?2,
                last_failure_reason = ?3,
                next_refresh_at = ?4,
                updated_at = ?5
             WHERE id = ?6",
            params![
                next_failures,
                encode_time(now),
                reason,
                encode_time(next_refresh_at),
                encode_time(now),
                watchlist_id,
            ],
        )?;
        Ok(next_refresh_at)
    }

    /// Back-compat shim kept for any caller still invoking the old name;
    /// delegates to `record_watchlist_refresh_success`.
    pub async fn mark_watchlist_refreshed(
        &self,
        watchlist_id: &str,
        interval_hours: i64,
    ) -> Result<()> {
        self.record_watchlist_refresh_success(watchlist_id, interval_hours)
            .await
    }

    /// Reconcile any `scheduled_runs` rows stuck in `pending`/`running`
    /// whose underlying `runs` row has already reached a terminal status.
    /// Returns the number of rows reconciled. Intended to be called at
    /// startup and periodically from the scheduler tick.
    pub async fn reap_stuck_scheduled_runs(&self) -> Result<i64> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT sr.id, r.status
             FROM scheduled_runs sr
             JOIN runs r ON r.id = sr.run_id
             WHERE sr.status IN ('pending', 'running')
               AND r.status IN ('completed', 'failed', 'cancelled')",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let rows: Vec<(String, String)> = collect_rows(rows)?;

        let now = encode_time(Utc::now());
        let mut reaped = 0i64;
        for (scheduled_run_id, run_status) in rows {
            let terminal = RunStatus::parse(&run_status).unwrap_or(RunStatus::Failed);
            let new_status = match terminal {
                RunStatus::Completed => "completed",
                RunStatus::Cancelled => "cancelled",
                _ => "failed",
            };
            let message = match terminal {
                RunStatus::Completed => None,
                other => Some(format!("reaped: underlying run ended as {other}")),
            };
            conn.execute(
                "UPDATE scheduled_runs
                 SET status = ?1, completed_at = COALESCE(completed_at, ?2), error_message = COALESCE(error_message, ?3)
                 WHERE id = ?4",
                params![new_status, now, message, scheduled_run_id],
            )?;
            reaped += 1;
        }
        Ok(reaped)
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
            error_message: None,
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
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.open_connection()?;
        let now = encode_time(Utc::now());
        let status = if success { "completed" } else { "failed" };
        conn.execute(
            "UPDATE scheduled_runs SET completed_at = ?1, status = ?2, error_message = ?3 WHERE id = ?4",
            params![now, status, error_message, scheduled_run_id],
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
        error_message: row.get("error_message")?,
    })
}

/// Shared mapper for pulling a `WatchlistSchedule` out of any SELECT that
/// includes the watchlist's schedule columns. `watchlist_id` is passed
/// separately because different call sites select the id as either `id`
/// or as part of a `watchlists.*` splat.
fn map_schedule_from_watchlist_row(
    row: &Row<'_>,
    watchlist_id: &str,
) -> rusqlite::Result<WatchlistSchedule> {
    Ok(WatchlistSchedule {
        watchlist_id: watchlist_id.to_string(),
        refresh_enabled: row.get::<_, i64>("refresh_enabled")? > 0,
        refresh_interval_hours: row.get("refresh_interval_hours")?,
        last_refresh_at: row
            .get::<_, Option<String>>("last_refresh_at")?
            .and_then(|s| parse_time_opt(&s)),
        next_refresh_at: row
            .get::<_, Option<String>>("next_refresh_at")?
            .and_then(|s| parse_time_opt(&s)),
        refresh_template_id: row.get("refresh_template_id")?,
        consecutive_failures: row.get("consecutive_failures")?,
        last_failure_at: row
            .get::<_, Option<String>>("last_failure_at")?
            .and_then(|s| parse_time_opt(&s)),
        last_failure_reason: row.get("last_failure_reason")?,
    })
}
