-- Multi-Model Research Panel Tables

-- LLM provider configurations
CREATE TABLE IF NOT EXISTS llm_providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    provider_type TEXT NOT NULL,
    api_key_encrypted TEXT,
    model TEXT NOT NULL,
    base_url TEXT,
    is_enabled INTEGER NOT NULL DEFAULT 1,
    is_default INTEGER NOT NULL DEFAULT 0,
    priority INTEGER NOT NULL DEFAULT 0,
    config_json TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE(name)
);

-- Individual model runs (outputs from specific models)
CREATE TABLE IF NOT EXISTS model_runs (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    iteration_number INTEGER,
    output_type TEXT NOT NULL,
    output_content TEXT,
    tokens_used INTEGER,
    latency_ms INTEGER,
    cost_estimate REAL,
    quality_score REAL,
    status TEXT NOT NULL DEFAULT 'completed',
    error_message TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    FOREIGN KEY(provider_id) REFERENCES llm_providers(id) ON DELETE SET NULL,
    UNIQUE(run_id, provider_id, iteration_number, output_type)
);

-- Model comparison results
CREATE TABLE IF NOT EXISTS model_comparisons (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    comparison_type TEXT NOT NULL,
    winner_provider_id TEXT,
    comparison_json TEXT NOT NULL,
    similarity_score REAL,
    key_differences TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
    FOREIGN KEY(winner_provider_id) REFERENCES llm_providers(id) ON DELETE SET NULL
);

-- Aggregated model quality scores
CREATE TABLE IF NOT EXISTS model_quality_scores (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    total_runs INTEGER NOT NULL DEFAULT 0,
    successful_runs INTEGER NOT NULL DEFAULT 0,
    avg_quality_score REAL,
    avg_latency_ms REAL,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost REAL NOT NULL DEFAULT 0,
    accuracy_score REAL,
    last_run_at DATETIME,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(provider_id) REFERENCES llm_providers(id) ON DELETE CASCADE,
    UNIQUE(provider_id)
);

-- Indexes for multi-model tables
CREATE INDEX IF NOT EXISTS idx_llm_providers_type ON llm_providers(provider_type);
CREATE INDEX IF NOT EXISTS idx_llm_providers_enabled ON llm_providers(is_enabled);
CREATE INDEX IF NOT EXISTS idx_model_runs_run_id ON model_runs(run_id);
CREATE INDEX IF NOT EXISTS idx_model_runs_provider_id ON model_runs(provider_id);
CREATE INDEX IF NOT EXISTS idx_model_comparisons_run_id ON model_comparisons(run_id);
CREATE INDEX IF NOT EXISTS idx_model_quality_scores_provider ON model_quality_scores(provider_id);
