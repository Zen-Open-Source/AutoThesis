use crate::providers::search::{SearchProvider, SearchResultItem};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone)]
pub struct TavilySearchProvider {
    client: reqwest::Client,
    api_key: String,
}

impl TavilySearchProvider {
    pub fn new(api_key: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(45))
            .build()
            .context("failed to build tavily client")?;

        Ok(Self { client, api_key })
    }
}

#[async_trait]
impl SearchProvider for TavilySearchProvider {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResultItem>> {
        let request = TavilyRequest {
            api_key: self.api_key.clone(),
            query: query.to_string(),
            max_results,
            search_depth: "advanced".to_string(),
            include_answer: false,
            include_raw_content: false,
        };

        let mut last_error = None;
        for attempt in 0..3 {
            let response = self
                .client
                .post("https://api.tavily.com/search")
                .json(&request)
                .send()
                .await;

            match response {
                Ok(response) => match response.error_for_status() {
                    Ok(success) => {
                        let body: TavilyResponse = success.json().await?;
                        let items = body
                            .results
                            .into_iter()
                            .map(|item| SearchResultItem {
                                title: item.title,
                                url: item.url,
                                snippet: item.content,
                                score: item.score,
                                source_type: None,
                                published_at: item
                                    .published_date
                                    .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                                    .map(|value| value.with_timezone(&Utc)),
                            })
                            .collect();
                        return Ok(items);
                    }
                    Err(error) => last_error = Some(anyhow!(error)),
                },
                Err(error) => last_error = Some(anyhow!(error)),
            }

            if attempt < 2 {
                sleep(Duration::from_millis(500 * (attempt + 1) as u64)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Tavily request failed")))
    }
}

#[derive(Debug, Serialize)]
struct TavilyRequest {
    api_key: String,
    query: String,
    max_results: usize,
    search_depth: String,
    include_answer: bool,
    include_raw_content: bool,
}

#[derive(Debug, Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: Option<String>,
    url: String,
    content: Option<String>,
    score: Option<f64>,
    published_date: Option<String>,
}
