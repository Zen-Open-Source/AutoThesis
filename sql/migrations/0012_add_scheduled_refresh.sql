-- Add schedule columns to watchlists table
ALTER TABLE watchlists ADD COLUMN refresh_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE watchlists ADD COLUMN refresh_interval_hours INTEGER NOT NULL DEFAULT 168;
ALTER TABLE watchlists ADD COLUMN last_refresh_at DATETIME;
ALTER TABLE watchlists ADD COLUMN next_refresh_at DATETIME;
ALTER TABLE watchlists ADD COLUMN refresh_template_id TEXT REFERENCES run_templates(id) ON DELETE SET NULL;

-- Track scheduled runs history
CREATE TABLE IF NOT EXISTS scheduled_runs (
    id TEXT PRIMARY KEY,
    watchlist_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    run_id TEXT NOT NULL,
    scheduled_at DATETIME NOT NULL,
    started_at DATETIME,
    completed_at DATETIME,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at DATETIME NOT NULL,
    FOREIGN KEY(watchlist_id) REFERENCES watchlists(id) ON DELETE CASCADE,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE
);

-- Indexes for scheduler queries
CREATE INDEX IF NOT EXISTS idx_watchlists_refresh_enabled ON watchlists(refresh_enabled);
CREATE INDEX IF NOT EXISTS idx_watchlists_next_refresh_at ON watchlists(next_refresh_at);
CREATE INDEX IF NOT EXISTS idx_scheduled_runs_watchlist_id ON scheduled_runs(watchlist_id);
CREATE INDEX IF NOT EXISTS idx_scheduled_runs_status ON scheduled_runs(status);
CREATE INDEX IF NOT EXISTS idx_scheduled_runs_scheduled_at ON scheduled_runs(scheduled_at);
