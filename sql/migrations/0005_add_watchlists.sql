CREATE TABLE IF NOT EXISTS watchlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

CREATE TABLE IF NOT EXISTS watchlist_tickers (
    id TEXT PRIMARY KEY,
    watchlist_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    sort_order INTEGER NOT NULL,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(watchlist_id) REFERENCES watchlists(id) ON DELETE CASCADE,
    UNIQUE(watchlist_id, ticker)
);

CREATE INDEX IF NOT EXISTS idx_watchlists_updated_at ON watchlists(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_watchlist_tickers_watchlist_id ON watchlist_tickers(watchlist_id);
