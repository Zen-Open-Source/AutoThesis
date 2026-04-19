use crate::{
    app_state::AppState, config::default_question_for_ticker, models::ScheduledRun,
    services::orchestrator,
};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Tickers with an in-flight scheduled run. The scheduler uses this to keep
/// total concurrency under `scheduler_max_concurrent_runs` and to avoid
/// enqueueing a second scheduled run for a ticker that already has one in
/// progress.
pub type ActiveRunTracker = Arc<Mutex<HashSet<String>>>;

/// How often (in scheduler ticks) to sweep for stuck `scheduled_runs` rows
/// whose underlying `runs` row already reached a terminal status.
const REAP_EVERY_N_TICKS: u64 = 10;

/// If `next_refresh_at` is more than `refresh_interval_hours * this` in the
/// past we log a warning, but we still only run one batch of work. This
/// prevents the scheduler from firing a flood of refreshes after the process
/// has been offline for a long time.
const CATCH_UP_WARN_MULTIPLIER: i64 = 4;

/// Start the scheduler background loop.
pub fn start_scheduler(state: AppState) -> ActiveRunTracker {
    let active_runs = state.active_scheduled_runs.clone();
    let check_interval = state.config.scheduler_check_interval_secs;

    if !state.config.scheduler_enabled {
        info!("scheduler disabled via configuration");
        return active_runs.clone();
    }

    info!(
        check_interval_secs = check_interval,
        max_concurrent = state.config.scheduler_max_concurrent_runs,
        min_ticker_age_hours = state.config.scheduler_min_ticker_age_hours,
        "starting scheduler"
    );

    let state_clone = state.clone();
    tokio::spawn(async move {
        // Startup: reconcile any scheduled_runs left in `running` by a crash.
        match state_clone.db.reap_stuck_scheduled_runs().await {
            Ok(n) if n > 0 => info!(reaped = n, "reaped stuck scheduled runs on startup"),
            Ok(_) => {}
            Err(error) => error!(%error, "startup stuck-run reap failed"),
        }

        let mut tick: u64 = 0;
        loop {
            if let Err(error) = run_scheduler_tick(&state_clone).await {
                error!(error = %error, "scheduler tick failed");
            }

            tick = tick.wrapping_add(1);
            if tick % REAP_EVERY_N_TICKS == 0 {
                match state_clone.db.reap_stuck_scheduled_runs().await {
                    Ok(n) if n > 0 => info!(reaped = n, "reaped stuck scheduled runs"),
                    Ok(_) => {}
                    Err(error) => error!(%error, "periodic stuck-run reap failed"),
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(check_interval)).await;
        }
    });

    state.active_scheduled_runs.clone()
}

/// Run a single scheduler tick.
async fn run_scheduler_tick(state: &AppState) -> Result<()> {
    let due_watchlists = state.db.get_watchlists_due_for_refresh().await?;

    if due_watchlists.is_empty() {
        return Ok(());
    }

    info!(count = due_watchlists.len(), "processing due watchlists");

    for (watchlist, schedule) in due_watchlists {
        // Catch-up guard: if this watchlist is significantly overdue we still
        // only run one batch, but we surface it in the logs so the operator
        // knows the system is recovering from downtime.
        if let Some(next) = schedule.next_refresh_at {
            let overdue_hours = (Utc::now() - next).num_hours();
            let threshold = schedule
                .refresh_interval_hours
                .saturating_mul(CATCH_UP_WARN_MULTIPLIER);
            if overdue_hours > threshold && threshold > 0 {
                warn!(
                    watchlist_id = %watchlist.id,
                    overdue_hours,
                    interval_hours = schedule.refresh_interval_hours,
                    "watchlist is significantly overdue (catching up one batch only)"
                );
            }
        }

        if let Err(error) = process_watchlist_refresh(state, &watchlist, &schedule).await {
            error!(
                watchlist_id = %watchlist.id,
                watchlist_name = %watchlist.name,
                error = %error,
                "watchlist refresh failed"
            );
            // Apply backoff so we don't retry every tick on a broken watchlist.
            if let Err(record_err) = state
                .db
                .record_watchlist_refresh_failure(
                    &watchlist.id,
                    schedule.refresh_interval_hours,
                    &error.to_string(),
                )
                .await
            {
                error!(
                    watchlist_id = %watchlist.id,
                    error = %record_err,
                    "failed to record watchlist refresh failure"
                );
            }
        }
    }

    Ok(())
}

/// Process refresh for a single watchlist.
async fn process_watchlist_refresh(
    state: &AppState,
    watchlist: &crate::models::Watchlist,
    schedule: &crate::models::WatchlistSchedule,
) -> Result<()> {
    let tickers = state.db.list_watchlist_tickers(&watchlist.id).await?;

    if tickers.is_empty() {
        info!(watchlist_id = %watchlist.id, "watchlist has no tickers, skipping");
        // An empty watchlist isn't really a "failure" - clear the failure
        // state and push `next_refresh_at` one interval out so we try again
        // later (maybe the user added tickers).
        state
            .db
            .record_watchlist_refresh_success(&watchlist.id, schedule.refresh_interval_hours)
            .await?;
        return Ok(());
    }

    let active_runs = state.active_scheduled_runs.clone();
    let max_concurrent = state.config.scheduler_max_concurrent_runs;
    let min_age_hours = state.config.scheduler_min_ticker_age_hours;

    let active_count = active_runs.lock().await.len();
    let available_slots = max_concurrent.saturating_sub(active_count);

    if available_slots == 0 {
        info!(
            watchlist_id = %watchlist.id,
            active = active_count,
            max = max_concurrent,
            "max concurrent runs reached, skipping"
        );
        return Ok(());
    }

    let mut runs_spawned = 0usize;
    let mut spawn_errors: Vec<String> = Vec::new();

    for watchlist_ticker in tickers {
        if runs_spawned >= available_slots {
            break;
        }

        let has_active_run = {
            let guard = active_runs.lock().await;
            guard.contains(&watchlist_ticker.ticker)
        };
        if has_active_run {
            continue;
        }

        let recent_runs = state
            .db
            .list_runs_for_ticker(&watchlist_ticker.ticker, 1)
            .await?;
        if let Some(latest_run) = recent_runs.first() {
            let hours_since_update = (Utc::now() - latest_run.updated_at).num_hours();
            if hours_since_update < min_age_hours {
                continue;
            }
        }

        let pending = state
            .db
            .get_pending_scheduled_run_for_ticker(&watchlist.id, &watchlist_ticker.ticker)
            .await?;
        if pending.is_some() {
            continue;
        }

        match spawn_scheduled_run(
            state,
            &active_runs,
            &watchlist.id,
            &watchlist_ticker.ticker,
            schedule,
        )
        .await
        {
            Ok(scheduled_run) => {
                info!(
                    watchlist_id = %watchlist.id,
                    ticker = %watchlist_ticker.ticker,
                    run_id = %scheduled_run.run_id,
                    "spawned scheduled run"
                );
                runs_spawned += 1;
            }
            Err(error) => {
                warn!(
                    watchlist_id = %watchlist.id,
                    ticker = %watchlist_ticker.ticker,
                    error = %error,
                    "failed to spawn scheduled run"
                );
                spawn_errors.push(format!("{}: {error}", watchlist_ticker.ticker));
            }
        }
    }

    if runs_spawned > 0 {
        state
            .db
            .record_watchlist_refresh_success(&watchlist.id, schedule.refresh_interval_hours)
            .await?;
    } else if !spawn_errors.is_empty() {
        // Every attempted spawn failed (or we had nothing spawnable *and*
        // there were errors). Back off so we don't hammer a broken provider.
        state
            .db
            .record_watchlist_refresh_failure(
                &watchlist.id,
                schedule.refresh_interval_hours,
                &spawn_errors.join("; "),
            )
            .await?;
    } else {
        // Nothing to spawn, but nothing failed either (all tickers filtered
        // by dedup / recency). Treat as a no-op success so we try again at
        // the normal cadence rather than staying due forever.
        state
            .db
            .record_watchlist_refresh_success(&watchlist.id, schedule.refresh_interval_hours)
            .await?;
    }

    Ok(())
}

/// Spawn a scheduled run for a ticker.
async fn spawn_scheduled_run(
    state: &AppState,
    active_runs: &ActiveRunTracker,
    watchlist_id: &str,
    ticker: &str,
    schedule: &crate::models::WatchlistSchedule,
) -> Result<ScheduledRun> {
    let question = if let Some(template_id) = &schedule.refresh_template_id {
        if let Some(template) = state.db.get_run_template(template_id).await? {
            if template.question_template.contains("{ticker}") {
                template.question_template.replace("{ticker}", ticker)
            } else {
                format!("{ticker}: {}", template.question_template)
            }
        } else {
            default_question_for_ticker(ticker)
        }
    } else {
        default_question_for_ticker(ticker)
    };

    let run = state.db.create_run(ticker, &question).await?;

    let scheduled_run = state
        .db
        .create_scheduled_run(watchlist_id, ticker, &run.id)
        .await?;

    state
        .db
        .update_scheduled_run_started(&scheduled_run.id)
        .await?;

    {
        let mut guard = active_runs.lock().await;
        guard.insert(ticker.to_string());
    }

    let state_clone = state.clone();
    let run_id = run.id.clone();
    let scheduled_run_id = scheduled_run.id.clone();
    let active_runs_clone = active_runs.clone();
    let ticker_clone = ticker.to_string();

    // Bounded by the global run semaphore. We do NOT call `spawn_bounded_run`
    // here directly because the scheduled-run bookkeeping (per-ticker active
    // set and scheduled_runs row update) has to run in the same task as the
    // orchestrator itself.
    tokio::spawn(async move {
        let semaphore = state_clone.run_semaphore.clone();
        let permit = match semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                warn!(%run_id, "run semaphore closed, dropping scheduled run");
                return;
            }
        };

        let result = orchestrator::execute_run(state_clone.clone(), run_id.clone()).await;
        drop(permit);

        {
            let mut guard = active_runs_clone.lock().await;
            guard.remove(&ticker_clone);
        }

        let success = result.is_ok();
        let error_message = result.as_ref().err().map(|e| e.to_string());
        if let Some(ref msg) = error_message {
            error!(%run_id, error = %msg, "scheduled run failed");
        }

        if let Err(error) = state_clone
            .db
            .update_scheduled_run_completed(&scheduled_run_id, success, error_message.as_deref())
            .await
        {
            error!(%scheduled_run_id, error = %error, "failed to update scheduled run status");
        }
    });

    Ok(scheduled_run)
}

/// Manually trigger a refresh for all tickers in a watchlist. Participates in
/// the same `active_scheduled_runs` tracker as the background loop so two
/// concurrent triggers (or a trigger firing while a tick is in progress) can
/// neither double-run a ticker nor exceed `scheduler_max_concurrent_runs`.
pub async fn trigger_watchlist_refresh(state: &AppState, watchlist_id: &str) -> Result<i64> {
    let _watchlist = state
        .db
        .get_watchlist(watchlist_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("watchlist not found: {}", watchlist_id))?;

    let schedule = state
        .db
        .get_watchlist_schedule(watchlist_id)
        .await?
        .unwrap_or_else(|| crate::models::WatchlistSchedule {
            watchlist_id: watchlist_id.to_string(),
            refresh_enabled: true,
            refresh_interval_hours: 168,
            last_refresh_at: None,
            next_refresh_at: None,
            refresh_template_id: None,
            consecutive_failures: 0,
            last_failure_at: None,
            last_failure_reason: None,
        });

    let tickers = state.db.list_watchlist_tickers(watchlist_id).await?;
    let max_concurrent = state.config.scheduler_max_concurrent_runs;
    let active_runs = state.active_scheduled_runs.clone();

    let mut spawned = 0i64;
    for watchlist_ticker in &tickers {
        // Respect the global concurrency cap even for manual triggers.
        if active_runs.lock().await.len() >= max_concurrent {
            break;
        }
        // Skip tickers already running.
        if active_runs.lock().await.contains(&watchlist_ticker.ticker) {
            continue;
        }

        if spawn_scheduled_run(
            state,
            &active_runs,
            watchlist_id,
            &watchlist_ticker.ticker,
            &schedule,
        )
        .await
        .is_ok()
        {
            spawned += 1;
        }
    }

    state
        .db
        .record_watchlist_refresh_success(watchlist_id, schedule.refresh_interval_hours)
        .await?;

    info!(
        watchlist_id = %watchlist_id,
        tickers_total = tickers.len(),
        tickers_spawned = spawned,
        "manual watchlist refresh triggered"
    );

    Ok(spawned)
}
