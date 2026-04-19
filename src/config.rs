use anyhow::{bail, Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub openai_base_url: String,
    pub search_api_key: String,
    pub search_provider: String,
    pub max_iterations: u32,
    pub max_sources_per_iteration: usize,
    /// Global cap on concurrent orchestrator runs across every entry point
    /// (manual create, retry, batch, comparison, scanner promote, scheduled
    /// refresh). Enforced by `AppState::run_semaphore`.
    pub max_concurrent_runs: usize,
    pub scheduler_enabled: bool,
    pub scheduler_check_interval_secs: u64,
    pub scheduler_max_concurrent_runs: usize,
    pub scheduler_min_ticker_age_hours: i64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let host = env_or("APP_HOST", "127.0.0.1");
        let port = env_or("APP_PORT", "3000")
            .parse()
            .context("APP_PORT must be a valid u16")?;
        let database_url = env_or("DATABASE_URL", "sqlite://autothesis.db");
        let openai_api_key = env_or("OPENAI_API_KEY", "");
        let openai_model = env_or("OPENAI_MODEL", "gpt-4.1-mini");
        let openai_base_url = env_or("OPENAI_BASE_URL", "https://api.openai.com/v1");
        let search_api_key = env_or("SEARCH_API_KEY", "");
        let search_provider = env_or("SEARCH_PROVIDER", "tavily");
        let max_iterations = env_or("MAX_ITERATIONS", "3")
            .parse()
            .context("MAX_ITERATIONS must be a valid u32")?;
        let max_sources_per_iteration = env_or("MAX_SOURCES_PER_ITERATION", "8")
            .parse()
            .context("MAX_SOURCES_PER_ITERATION must be a valid usize")?;
        let max_concurrent_runs = env_or("MAX_CONCURRENT_RUNS", "5")
            .parse()
            .context("MAX_CONCURRENT_RUNS must be a valid usize")?;
        let scheduler_enabled = env_or("SCHEDULER_ENABLED", "true")
            .parse()
            .context("SCHEDULER_ENABLED must be a valid boolean")?;
        let scheduler_check_interval_secs =
            env_or("SCHEDULER_CHECK_INTERVAL_SECS", "60")
                .parse()
                .context("SCHEDULER_CHECK_INTERVAL_SECS must be a valid u64")?;
        let scheduler_max_concurrent_runs = env_or("SCHEDULER_MAX_CONCURRENT_RUNS", "3")
            .parse()
            .context("SCHEDULER_MAX_CONCURRENT_RUNS must be a valid usize")?;
        let scheduler_min_ticker_age_hours = env_or("SCHEDULER_MIN_TICKER_AGE_HOURS", "24")
            .parse()
            .context("SCHEDULER_MIN_TICKER_AGE_HOURS must be a valid i64")?;

        if max_iterations == 0 {
            bail!("MAX_ITERATIONS must be at least 1");
        }
        if max_sources_per_iteration == 0 {
            bail!("MAX_SOURCES_PER_ITERATION must be at least 1");
        }
        if max_concurrent_runs == 0 {
            bail!("MAX_CONCURRENT_RUNS must be at least 1");
        }
        if scheduler_check_interval_secs == 0 {
            bail!("SCHEDULER_CHECK_INTERVAL_SECS must be at least 1");
        }
        if scheduler_max_concurrent_runs == 0 {
            bail!("SCHEDULER_MAX_CONCURRENT_RUNS must be at least 1");
        }

        Ok(Self {
            host,
            port,
            database_url,
            openai_api_key,
            openai_model,
            openai_base_url,
            search_api_key,
            search_provider,
            max_iterations,
            max_sources_per_iteration,
            max_concurrent_runs,
            scheduler_enabled,
            scheduler_check_interval_secs,
            scheduler_max_concurrent_runs,
            scheduler_min_ticker_age_hours,
        })
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub fn default_question_for_ticker(ticker: &str) -> String {
    format!(
        "What is the current bull and bear case for {ticker}, and what would need to be true for the valuation to make sense?"
    )
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}
