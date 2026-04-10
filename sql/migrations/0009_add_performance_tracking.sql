-- Thesis Performance Tracking Tables

-- Price snapshots at key moments
CREATE TABLE IF NOT EXISTS price_snapshots (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL,
    price_date DATE NOT NULL,
    open_price REAL NOT NULL,
    close_price REAL NOT NULL,
    high_price REAL,
    low_price REAL,
    volume INTEGER,
    adjusted_close REAL,
    source TEXT NOT NULL DEFAULT 'yahoo',
    created_at DATETIME NOT NULL,
    UNIQUE(ticker, price_date)
);

-- Thesis outcomes - tracking returns over time
CREATE TABLE IF NOT EXISTS thesis_outcomes (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    thesis_date DATE NOT NULL,
    thesis_price REAL NOT NULL,
    return_1d REAL,
    return_7d REAL,
    return_30d REAL,
    return_90d REAL,
    return_180d REAL,
    return_365d REAL,
    price_1d REAL,
    price_7d REAL,
    price_30d REAL,
    price_90d REAL,
    price_180d REAL,
    price_365d REAL,
    thesis_direction TEXT,
    thesis_correct_1d INTEGER,
    thesis_correct_7d INTEGER,
    thesis_correct_30d INTEGER,
    thesis_correct_90d INTEGER,
    notes TEXT,
    last_updated DATETIME NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    UNIQUE(run_id)
);

-- Aggregated accuracy metrics
CREATE TABLE IF NOT EXISTS thesis_accuracy (
    id TEXT PRIMARY KEY,
    ticker TEXT,
    provider_id TEXT,
    time_horizon TEXT NOT NULL,
    total_theses INTEGER NOT NULL DEFAULT 0,
    correct_theses INTEGER NOT NULL DEFAULT 0,
    accuracy_rate REAL,
    avg_return REAL,
    median_return REAL,
    best_return REAL,
    worst_return REAL,
    sharpe_ratio REAL,
    win_rate REAL,
    avg_holding_days REAL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE(ticker, provider_id, time_horizon)
);

-- Price tracking jobs
CREATE TABLE IF NOT EXISTS price_tracking_jobs (
    id TEXT PRIMARY KEY,
    job_type TEXT NOT NULL,
    target_date DATE NOT NULL,
    tickers_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    started_at DATETIME,
    completed_at DATETIME,
    error_message TEXT,
    prices_fetched INTEGER DEFAULT 0,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

-- Indexes for performance tracking
CREATE INDEX IF NOT EXISTS idx_price_snapshots_ticker ON price_snapshots(ticker);
CREATE INDEX IF NOT EXISTS idx_price_snapshots_date ON price_snapshots(price_date);
CREATE INDEX IF NOT EXISTS idx_thesis_outcomes_ticker ON thesis_outcomes(ticker);
CREATE INDEX IF NOT EXISTS idx_thesis_outcomes_date ON thesis_outcomes(thesis_date);
CREATE INDEX IF NOT EXISTS idx_thesis_accuracy_ticker ON thesis_accuracy(ticker);
CREATE INDEX IF NOT EXISTS idx_thesis_accuracy_provider ON thesis_accuracy(provider_id);
CREATE INDEX IF NOT EXISTS idx_price_tracking_jobs_status ON price_tracking_jobs(status);
