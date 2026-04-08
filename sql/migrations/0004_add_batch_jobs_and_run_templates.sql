CREATE TABLE IF NOT EXISTS batch_jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    question_template TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

CREATE TABLE IF NOT EXISTS batch_job_runs (
    id TEXT PRIMARY KEY,
    batch_job_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    sort_order INTEGER NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(batch_job_id) REFERENCES batch_jobs(id) ON DELETE CASCADE,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    UNIQUE(batch_job_id, run_id)
);

CREATE TABLE IF NOT EXISTS run_templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    question_template TEXT NOT NULL,
    description TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE(name)
);

CREATE INDEX IF NOT EXISTS idx_batch_jobs_created_at ON batch_jobs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_batch_job_runs_batch_job_id ON batch_job_runs(batch_job_id);
CREATE INDEX IF NOT EXISTS idx_batch_job_runs_run_id ON batch_job_runs(run_id);
CREATE INDEX IF NOT EXISTS idx_run_templates_updated_at ON run_templates(updated_at DESC);
