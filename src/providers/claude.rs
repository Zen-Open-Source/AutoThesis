use crate::providers::llm::LlmProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone)]
pub struct ClaudeProvider {
    client: reqwest::Client,
    model: String,
    base_url: String,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&api_key).context("failed to build api key header")?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(120))
            .build()
            .context("failed to build reqwest client")?;

        let base_url = base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string());

        Ok(Self {
            client,
            model,
            base_url,
        })
    }

    async fn chat(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        json_mode: bool,
    ) -> Result<String> {
        let messages = vec![ClaudeMessage {
            role: "user".to_string(),
            content: user_prompt.to_string(),
        }];

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system: system_prompt.to_string(),
            messages,
            temperature: 0.2,
        };

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let mut last_error = None;
        for attempt in 0..3 {
            let response = self.client.post(&url).json(&request).send().await;
            match response {
                Ok(response) => match response.error_for_status() {
                    Ok(success) => {
                        let body: ClaudeResponse = success.json().await?;
                        let content = body
                            .content
                            .into_iter()
                            .next()
                            .ok_or_else(|| anyhow!("Claude returned no content"))?
                            .text;
                        if json_mode {
                            // Try to extract JSON from the response
                            let json_start = content.find('{');
                            let json_end = content.rfind('}');
                            if let (Some(start), Some(end)) = (json_start, json_end) {
                                return Ok(content[start..=end].to_string());
                            }
                        }
                        return Ok(content);
                    }
                    Err(error) => last_error = Some(anyhow!(error)),
                },
                Err(error) => last_error = Some(anyhow!(error)),
            }

            if attempt < 2 {
                sleep(Duration::from_millis(500 * (attempt + 1) as u64)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Claude request failed")))
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete_json(
        &self,
        _prompt_name: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<Value> {
        let content = self.chat(system_prompt, user_prompt, true).await?;
        let value = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse json response: {content}"))?;
        Ok(value)
    }

    async fn complete_markdown(
        &self,
        _prompt_name: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        self.chat(system_prompt, user_prompt, false).await
    }
}

#[derive(Debug, Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ClaudeMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Debug, Deserialize)]
struct ClaudeContent {
    text: String,
}
