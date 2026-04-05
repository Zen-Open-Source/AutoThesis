CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL,
    question TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    final_iteration_number INTEGER,
    final_memo_markdown TEXT,
    final_memo_html TEXT,
    summary TEXT
);

CREATE TABLE IF NOT EXISTS iterations (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    iteration_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    plan_markdown TEXT,
    draft_markdown TEXT,
    critique_markdown TEXT,
    evaluation_json TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    UNIQUE(run_id, iteration_number)
);

CREATE TABLE IF NOT EXISTS search_queries (
    id TEXT PRIMARY KEY,
    iteration_id TEXT NOT NULL,
    query_text TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(iteration_id) REFERENCES iterations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS search_results (
    id TEXT PRIMARY KEY,
    iteration_id TEXT NOT NULL,
    query_id TEXT NOT NULL,
    title TEXT,
    url TEXT NOT NULL,
    snippet TEXT,
    rank_score REAL,
    source_type TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(iteration_id) REFERENCES iterations(id) ON DELETE CASCADE,
    FOREIGN KEY(query_id) REFERENCES search_queries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sources (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    iteration_id TEXT,
    url TEXT NOT NULL,
    title TEXT,
    domain TEXT,
    published_at DATETIME,
    source_type TEXT,
    raw_text TEXT,
    excerpt TEXT,
    quality_score REAL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    FOREIGN KEY(iteration_id) REFERENCES iterations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS evidence_notes (
    id TEXT PRIMARY KEY,
    iteration_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    note_markdown TEXT NOT NULL,
    claim_type TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(iteration_id) REFERENCES iterations(id) ON DELETE CASCADE,
    FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    iteration_id TEXT,
    event_type TEXT NOT NULL,
    message TEXT NOT NULL,
    payload_json TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    FOREIGN KEY(iteration_id) REFERENCES iterations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_iterations_run_id ON iterations(run_id);
CREATE INDEX IF NOT EXISTS idx_search_queries_iteration_id ON search_queries(iteration_id);
CREATE INDEX IF NOT EXISTS idx_search_results_iteration_id ON search_results(iteration_id);
CREATE INDEX IF NOT EXISTS idx_sources_run_id ON sources(run_id);
CREATE INDEX IF NOT EXISTS idx_sources_iteration_id ON sources(iteration_id);
CREATE INDEX IF NOT EXISTS idx_evidence_notes_iteration_id ON evidence_notes(iteration_id);
CREATE INDEX IF NOT EXISTS idx_events_run_id ON events(run_id);
