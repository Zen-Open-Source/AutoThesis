-- Evidence Quality Scoring Tables

-- Source reputation scores
CREATE TABLE IF NOT EXISTS source_reputation (
    id TEXT PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    reputation_score REAL NOT NULL DEFAULT 5.0,
    total_citations INTEGER NOT NULL DEFAULT 0,
    successful_citations INTEGER NOT NULL DEFAULT 0,
    failed_citations INTEGER NOT NULL DEFAULT 0,
    avg_evidence_quality REAL,
    source_type TEXT,
    bias_rating TEXT,
    reliability_tier TEXT,
    notes TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

-- Evidence outcomes - did this evidence prove correct?
CREATE TABLE IF NOT EXISTS evidence_outcomes (
    id TEXT PRIMARY KEY,
    evidence_note_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    ticker TEXT NOT NULL,
    claim_type TEXT,
    claim_text TEXT,
    outcome_type TEXT NOT NULL,
    outcome_date DATE NOT NULL,
    price_at_claim REAL,
    price_at_outcome REAL,
    return_since_claim REAL,
    was_correct INTEGER NOT NULL DEFAULT 0,
    confidence_at_claim REAL,
    outcome_notes TEXT,
    verified_by TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(evidence_note_id) REFERENCES evidence_notes(id) ON DELETE CASCADE,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE
);

-- Detailed source quality metrics
CREATE TABLE IF NOT EXISTS source_quality_metrics (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    domain TEXT,
    quality_score REAL,
    relevance_score REAL,
    timeliness_score REAL,
    authority_score REAL,
    citation_count INTEGER NOT NULL DEFAULT 0,
    last_cited_at DATETIME,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE CASCADE
);

-- Domain reliability history
CREATE TABLE IF NOT EXISTS domain_reliability_history (
    id TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    recorded_date DATE NOT NULL,
    reliability_score REAL NOT NULL,
    sample_size INTEGER NOT NULL DEFAULT 0,
    success_rate REAL,
    created_at DATETIME NOT NULL,
    UNIQUE(domain, recorded_date)
);

-- Indexes for evidence quality
CREATE INDEX IF NOT EXISTS idx_source_reputation_domain ON source_reputation(domain);
CREATE INDEX IF NOT EXISTS idx_source_reputation_score ON source_reputation(reputation_score);
CREATE INDEX IF NOT EXISTS idx_evidence_outcomes_evidence_id ON evidence_outcomes(evidence_note_id);
CREATE INDEX IF NOT EXISTS idx_evidence_outcomes_ticker ON evidence_outcomes(ticker);
CREATE INDEX IF NOT EXISTS idx_evidence_outcomes_outcome_type ON evidence_outcomes(outcome_type);
CREATE INDEX IF NOT EXISTS idx_source_quality_metrics_source ON source_quality_metrics(source_id);
CREATE INDEX IF NOT EXISTS idx_source_quality_metrics_domain ON source_quality_metrics(domain);
CREATE INDEX IF NOT EXISTS idx_domain_reliability_domain ON domain_reliability_history(domain);
