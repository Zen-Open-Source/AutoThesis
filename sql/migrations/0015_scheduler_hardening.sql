-- Track refresh failures so the scheduler can apply exponential backoff.
ALTER TABLE watchlists ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE watchlists ADD COLUMN last_failure_at DATETIME;
ALTER TABLE watchlists ADD COLUMN last_failure_reason TEXT;

-- Capture why a scheduled run ended up in `failed` (or was reaped).
ALTER TABLE scheduled_runs ADD COLUMN error_message TEXT;

-- Useful for the stuck-run reaper's join from scheduled_runs -> runs.
CREATE INDEX IF NOT EXISTS idx_scheduled_runs_run_id ON scheduled_runs(run_id);
