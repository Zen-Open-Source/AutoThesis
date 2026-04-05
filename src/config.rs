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

        if max_iterations == 0 {
            bail!("MAX_ITERATIONS must be at least 1");
        }
        if max_sources_per_iteration == 0 {
            bail!("MAX_SOURCES_PER_ITERATION must be at least 1");
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
