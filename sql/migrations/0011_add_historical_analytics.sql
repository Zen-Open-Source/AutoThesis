-- Historical Analytics Tables

-- Thesis history archive
CREATE TABLE IF NOT EXISTS thesis_history (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    thesis_date DATE NOT NULL,
    thesis_markdown TEXT NOT NULL,
    thesis_html TEXT,
    executive_summary TEXT,
    bull_case TEXT,
    bear_case TEXT,
    key_catalysts TEXT,
    key_risks TEXT,
    conviction_level TEXT,
    thesis_direction TEXT,
    model_provider_id TEXT,
    signals_json TEXT,
    iteration_number INTEGER,
    archived_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    FOREIGN KEY(model_provider_id) REFERENCES llm_providers(id) ON DELETE SET NULL
);

-- Signal effectiveness tracking
CREATE TABLE IF NOT EXISTS signal_effectiveness (
    id TEXT PRIMARY KEY,
    signal_type TEXT NOT NULL,
    signal_date DATE NOT NULL,
    ticker TEXT NOT NULL,
    signal_strength REAL NOT NULL,
    signal_description TEXT,
    outcome_type TEXT,
    return_7d REAL,
    return_30d REAL,
    return_90d REAL,
    was_predictive INTEGER DEFAULT 0,
    thesis_run_id TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(thesis_run_id) REFERENCES runs(id) ON DELETE SET NULL,
    UNIQUE(signal_type, signal_date, ticker)
);

-- Aggregated signal effectiveness stats
CREATE TABLE IF NOT EXISTS signal_effectiveness_stats (
    id TEXT PRIMARY KEY,
    signal_type TEXT NOT NULL,
    total_signals INTEGER NOT NULL DEFAULT 0,
    predictive_signals INTEGER NOT NULL DEFAULT 0,
    predictive_rate REAL,
    avg_return_7d REAL,
    avg_return_30d REAL,
    avg_return_90d REAL,
    best_return_90d REAL,
    worst_return_90d REAL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE(signal_type)
);

-- Research analytics aggregated stats
CREATE TABLE IF NOT EXISTS research_analytics (
    id TEXT PRIMARY KEY,
    analytics_date DATE NOT NULL,
    total_runs INTEGER NOT NULL DEFAULT 0,
    total_theses INTEGER NOT NULL DEFAULT 0,
    avg_conviction REAL,
    avg_iteration_count REAL,
    avg_source_count REAL,
    avg_evidence_count REAL,
    avg_quality_score REAL,
    thesis_accuracy_30d REAL,
    thesis_accuracy_90d REAL,
    top_performing_ticker TEXT,
    worst_performing_ticker TEXT,
    best_model_provider_id TEXT,
    model_accuracy_ranking_json TEXT,
    created_at DATETIME NOT NULL,
    UNIQUE(analytics_date)
);

-- Ticker research history summary
CREATE TABLE IF NOT EXISTS ticker_research_summary (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    first_research_date DATE,
    last_research_date DATE,
    total_research_runs INTEGER NOT NULL DEFAULT 0,
    avg_conviction REAL,
    avg_quality_score REAL,
    thesis_accuracy_30d REAL,
    thesis_accuracy_90d REAL,
    total_return_all_time REAL,
    best_return_90d REAL,
    worst_return_90d REAL,
    research_frequency TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

-- Model performance over time
CREATE TABLE IF NOT EXISTS model_performance_history (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    recorded_date DATE NOT NULL,
    total_runs INTEGER NOT NULL DEFAULT 0,
    successful_runs INTEGER NOT NULL DEFAULT 0,
    avg_quality_score REAL,
    avg_latency_ms REAL,
    accuracy_score REAL,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost REAL NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(provider_id) REFERENCES llm_providers(id) ON DELETE CASCADE,
    UNIQUE(provider_id, recorded_date)
);

-- Indexes for historical analytics
CREATE INDEX IF NOT EXISTS idx_thesis_history_ticker ON thesis_history(ticker);
CREATE INDEX IF NOT EXISTS idx_thesis_history_date ON thesis_history(thesis_date);
CREATE INDEX IF NOT EXISTS idx_thesis_history_provider ON thesis_history(model_provider_id);
CREATE INDEX IF NOT EXISTS idx_signal_effectiveness_type ON signal_effectiveness(signal_type);
CREATE INDEX IF NOT EXISTS idx_signal_effectiveness_ticker ON signal_effectiveness(ticker);
CREATE INDEX IF NOT EXISTS idx_signal_effectiveness_date ON signal_effectiveness(signal_date);
CREATE INDEX IF NOT EXISTS idx_signal_effectiveness_stats_type ON signal_effectiveness_stats(signal_type);
CREATE INDEX IF NOT EXISTS idx_research_analytics_date ON research_analytics(analytics_date);
CREATE INDEX IF NOT EXISTS idx_ticker_research_summary_ticker ON ticker_research_summary(ticker);
CREATE INDEX IF NOT EXISTS idx_model_performance_provider ON model_performance_history(provider_id);
CREATE INDEX IF NOT EXISTS idx_model_performance_date ON model_performance_history(recorded_date);
