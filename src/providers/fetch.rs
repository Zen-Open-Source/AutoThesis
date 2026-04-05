use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use scraper::{Html, Selector};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub url: String,
    pub title: Option<String>,
    pub domain: Option<String>,
    pub text: String,
}

#[async_trait]
pub trait WebFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedPage>;
}

#[derive(Clone)]
pub struct ReqwestWebFetcher {
    client: reqwest::Client,
}

impl ReqwestWebFetcher {
    pub fn new() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("AutoThesis/0.1 (+https://autothesis.finance)"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build reqwest client")?;

        Ok(Self { client })
    }
}

#[async_trait]
impl WebFetcher for ReqwestWebFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let body = response.text().await?;
        let document = Html::parse_document(&body);
        let title_selector = Selector::parse("title").ok();
        let title = title_selector
            .as_ref()
            .and_then(|selector| document.select(selector).next())
            .map(|node| collapse_whitespace(&node.text().collect::<Vec<_>>().join(" ")))
            .filter(|value| !value.is_empty());

        let text =
            collapse_whitespace(&document.root_element().text().collect::<Vec<_>>().join(" "));
        let domain = Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.domain().map(|value| value.to_string()));

        Ok(FetchedPage {
            url: url.to_string(),
            title,
            domain,
            text: truncate_chars(&text, 16_000),
        })
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
