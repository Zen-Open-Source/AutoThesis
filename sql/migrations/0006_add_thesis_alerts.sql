CREATE TABLE IF NOT EXISTS alert_rules (
    id TEXT PRIMARY KEY,
    watchlist_id TEXT NOT NULL,
    rule_type TEXT NOT NULL,
    threshold REAL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(watchlist_id) REFERENCES watchlists(id) ON DELETE CASCADE,
    UNIQUE(watchlist_id, rule_type)
);

CREATE TABLE IF NOT EXISTS thesis_alerts (
    id TEXT PRIMARY KEY,
    watchlist_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    run_id TEXT NOT NULL,
    alert_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    message TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(watchlist_id) REFERENCES watchlists(id) ON DELETE CASCADE,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    UNIQUE(watchlist_id, ticker, alert_type, run_id)
);

CREATE INDEX IF NOT EXISTS idx_alert_rules_watchlist_id ON alert_rules(watchlist_id);
CREATE INDEX IF NOT EXISTS idx_alert_rules_watchlist_id_enabled ON alert_rules(watchlist_id, enabled);
CREATE INDEX IF NOT EXISTS idx_thesis_alerts_watchlist_id_status ON thesis_alerts(watchlist_id, status);
CREATE INDEX IF NOT EXISTS idx_thesis_alerts_watchlist_ticker_status ON thesis_alerts(watchlist_id, ticker, status);
