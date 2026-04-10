-- Ticker universe: S&P 500 and custom tickers to scan
CREATE TABLE IF NOT EXISTS ticker_universe (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    name TEXT,
    sector TEXT,
    industry TEXT,
    market_cap_billion REAL,
    is_sp500 INTEGER NOT NULL DEFAULT 0,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

-- Scanner configurations
CREATE TABLE IF NOT EXISTS scanner_configs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    universe_filter TEXT NOT NULL DEFAULT 'sp500',
    sector_filter TEXT,
    min_market_cap REAL,
    max_market_cap REAL,
    max_opportunities INTEGER NOT NULL DEFAULT 20,
    signal_weights_json TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

-- Scan runs: individual scan executions
CREATE TABLE IF NOT EXISTS scan_runs (
    id TEXT PRIMARY KEY,
    config_id TEXT,
    status TEXT NOT NULL DEFAULT 'queued',
    tickers_scanned INTEGER DEFAULT 0,
    opportunities_found INTEGER DEFAULT 0,
    started_at DATETIME,
    completed_at DATETIME,
    error_message TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(config_id) REFERENCES scanner_configs(id) ON DELETE SET NULL
);

-- Scan opportunities: detected thesis opportunities
CREATE TABLE IF NOT EXISTS scan_opportunities (
    id TEXT PRIMARY KEY,
    scan_run_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    overall_score REAL NOT NULL,
    signal_strength_score REAL NOT NULL,
    thesis_quality_score REAL,
    coverage_gap_score REAL NOT NULL,
    timing_score REAL NOT NULL,
    signals_json TEXT NOT NULL,
    preliminary_thesis_markdown TEXT,
    preliminary_thesis_html TEXT,
    key_catalysts TEXT,
    risk_factors TEXT,
    promoted_to_run_id TEXT,
    status TEXT NOT NULL DEFAULT 'new',
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(scan_run_id) REFERENCES scan_runs(id) ON DELETE CASCADE,
    FOREIGN KEY(promoted_to_run_id) REFERENCES runs(id) ON DELETE SET NULL,
    UNIQUE(scan_run_id, ticker)
);

-- Indexes for scanner tables
CREATE INDEX IF NOT EXISTS idx_ticker_universe_ticker ON ticker_universe(ticker);
CREATE INDEX IF NOT EXISTS idx_ticker_universe_is_active ON ticker_universe(is_active);
CREATE INDEX IF NOT EXISTS idx_ticker_universe_sector ON ticker_universe(sector);
CREATE INDEX IF NOT EXISTS idx_scanner_configs_is_active ON scanner_configs(is_active);
CREATE INDEX IF NOT EXISTS idx_scan_runs_status ON scan_runs(status);
CREATE INDEX IF NOT EXISTS idx_scan_runs_created_at ON scan_runs(created_at);
CREATE INDEX IF NOT EXISTS idx_scan_opportunities_scan_run_id ON scan_opportunities(scan_run_id);
CREATE INDEX IF NOT EXISTS idx_scan_opportunities_ticker ON scan_opportunities(ticker);
CREATE INDEX IF NOT EXISTS idx_scan_opportunities_score ON scan_opportunities(overall_score);
CREATE INDEX IF NOT EXISTS idx_scan_opportunities_status ON scan_opportunities(status);
