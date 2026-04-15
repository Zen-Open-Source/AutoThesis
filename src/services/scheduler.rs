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

/// Tracks active runs to limit concurrency.
pub type ActiveRunTracker = Arc<Mutex<HashSet<String>>>;

/// Start the scheduler background loop.
pub fn start_scheduler(state: AppState) -> ActiveRunTracker {
    let active_runs: ActiveRunTracker = Arc::new(Mutex::new(HashSet::new()));
    let active_runs_clone = active_runs.clone();
    let check_interval = state.config.scheduler_check_interval_secs;

    if !state.config.scheduler_enabled {
        info!("scheduler disabled via configuration");
        return active_runs;
    }

    info!(
        check_interval_secs = check_interval,
        max_concurrent = state.config.scheduler_max_concurrent_runs,
        min_ticker_age_hours = state.config.scheduler_min_ticker_age_hours,
        "starting scheduler"
    );

    tokio::spawn(async move {
        loop {
            if let Err(error) = run_scheduler_tick(&state, &active_runs_clone).await {
                error!(error = %error, "scheduler tick failed");
            }

            tokio::time::sleep(std::time::Duration::from_secs(check_interval)).await;
        }
    });

    active_runs
}

/// Run a single scheduler tick.
async fn run_scheduler_tick(state: &AppState, active_runs: &ActiveRunTracker) -> Result<()> {
    let due_watchlists = state.db.get_watchlists_due_for_refresh().await?;

    if due_watchlists.is_empty() {
        return Ok(());
    }

    info!(count = due_watchlists.len(), "processing due watchlists");

    for (watchlist, schedule) in due_watchlists {
        if let Err(error) =
            process_watchlist_refresh(state, active_runs, &watchlist, &schedule).await
        {
            error!(
                watchlist_id = %watchlist.id,
                watchlist_name = %watchlist.name,
                error = %error,
                "watchlist refresh failed"
            );
        }
    }

    Ok(())
}

/// Process refresh for a single watchlist.
async fn process_watchlist_refresh(
    state: &AppState,
    active_runs: &ActiveRunTracker,
    watchlist: &crate::models::Watchlist,
    schedule: &crate::models::WatchlistSchedule,
) -> Result<()> {
    let tickers = state.db.list_watchlist_tickers(&watchlist.id).await?;

    if tickers.is_empty() {
        info!(watchlist_id = %watchlist.id, "watchlist has no tickers, skipping");
        return Ok(());
    }

    let mut runs_spawned = 0;
    let max_concurrent = state.config.scheduler_max_concurrent_runs;
    let min_age_hours = state.config.scheduler_min_ticker_age_hours;

    // Get count of currently active runs
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

    for watchlist_ticker in tickers {
        if runs_spawned >= available_slots {
            break;
        }

        // Check if ticker has an active run
        let has_active_run = {
            let guard = active_runs.lock().await;
            guard.contains(&watchlist_ticker.ticker)
        };

        if has_active_run {
            continue;
        }

        // Check if ticker was recently refreshed
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

        // Check if there's already a pending scheduled run
        let pending = state
            .db
            .get_pending_scheduled_run_for_ticker(&watchlist.id, &watchlist_ticker.ticker)
            .await?;

        if pending.is_some() {
            continue;
        }

        match spawn_scheduled_run(
            state,
            active_runs,
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
            }
        }
    }

    if runs_spawned > 0 {
        state
            .db
            .mark_watchlist_refreshed(&watchlist.id, schedule.refresh_interval_hours)
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

    tokio::spawn(async move {
        let result = orchestrator::execute_run(state_clone.clone(), run_id.clone()).await;

        {
            let mut guard = active_runs_clone.lock().await;
            guard.remove(&ticker_clone);
        }

        let success = result.is_ok();
        if let Err(error) = &result {
            error!(%run_id, error = %error, "scheduled run failed");
        }

        if let Err(error) = state_clone
            .db
            .update_scheduled_run_completed(&scheduled_run_id, success)
            .await
        {
            error!(%scheduled_run_id, error = %error, "failed to update scheduled run status");
        }
    });

    Ok(scheduled_run)
}

/// Manually trigger a refresh for all tickers in a watchlist.
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
        });

    let tickers = state.db.list_watchlist_tickers(watchlist_id).await?;
    let mut spawned = 0i64;

    let active_runs: ActiveRunTracker = Arc::new(Mutex::new(HashSet::new()));

    for watchlist_ticker in &tickers {
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
        .mark_watchlist_refreshed(watchlist_id, schedule.refresh_interval_hours)
        .await?;

    info!(
        watchlist_id = %watchlist_id,
        tickers_total = tickers.len(),
        tickers_spawned = spawned,
        "manual watchlist refresh triggered"
    );

    Ok(spawned)
}
