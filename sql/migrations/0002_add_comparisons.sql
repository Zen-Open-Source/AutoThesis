CREATE TABLE IF NOT EXISTS comparisons (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    question TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    final_comparison_html TEXT,
    summary TEXT
);

CREATE TABLE IF NOT EXISTS comparison_runs (
    id TEXT PRIMARY KEY,
    comparison_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    sort_order INTEGER NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(comparison_id) REFERENCES comparisons(id) ON DELETE CASCADE,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    UNIQUE(comparison_id, run_id)
);

CREATE INDEX IF NOT EXISTS idx_comparison_runs_comparison_id ON comparison_runs(comparison_id);
CREATE INDEX IF NOT EXISTS idx_comparison_runs_run_id ON comparison_runs(run_id);
