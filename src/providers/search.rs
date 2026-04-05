use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub title: Option<String>,
    pub url: String,
    pub snippet: Option<String>,
    pub score: Option<f64>,
    pub source_type: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResultItem>>;
}
