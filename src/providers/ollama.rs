use crate::providers::llm::LlmProvider;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone)]
pub struct OllamaProvider {
    client: reqwest::Client,
    model: String,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(model: String, base_url: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(300)) // Local models can be slower
            .build()
            .context("failed to build reqwest client")?;

        let base_url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());

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
        let request = OllamaRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                OllamaMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            stream: false,
            format: if json_mode {
                Some("json".to_string())
            } else {
                None
            },
            options: Some(OllamaOptions { temperature: 0.2 }),
        };

        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));

        let mut last_error = None;
        for attempt in 0..3 {
            let response = self.client.post(&url).json(&request).send().await;
            match response {
                Ok(response) => match response.error_for_status() {
                    Ok(success) => {
                        let body: OllamaResponse = success.json().await?;
                        return Ok(body.message.content);
                    }
                    Err(error) => last_error = Some(anyhow!(error)),
                },
                Err(error) => last_error = Some(anyhow!(error)),
            }

            if attempt < 2 {
                sleep(Duration::from_millis(500 * (attempt + 1) as u64)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Ollama request failed")))
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
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
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}
