-- Indexes to speed up common query patterns: scheduler polling, dashboard
-- rollups, related-ticker discovery and watchlist refresh paths all filter
-- `runs` by ticker / status / updated_at; events are always fetched per
-- run_id ordered by created_at.

CREATE INDEX IF NOT EXISTS idx_runs_ticker ON runs(ticker);
CREATE INDEX IF NOT EXISTS idx_runs_status_updated_at ON runs(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_runs_created_at ON runs(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_events_run_created_at ON events(run_id, created_at);

CREATE INDEX IF NOT EXISTS idx_sources_run_quality ON sources(run_id, quality_score DESC);
CREATE INDEX IF NOT EXISTS idx_sources_iteration_quality ON sources(iteration_id, quality_score DESC);

CREATE INDEX IF NOT EXISTS idx_search_results_iteration_rank ON search_results(iteration_id, rank_score DESC);
