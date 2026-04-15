use crate::providers::llm::LlmProvider;
use crate::utils::retry_with_backoff;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone)]
pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("failed to build reqwest client")?;

        let base_url =
            base_url.unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());

        Ok(Self {
            client,
            api_key,
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
        let request = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: user_prompt.to_string(),
                }],
            }],
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart {
                    text: system_prompt.to_string(),
                }],
            }),
            generation_config: Some(GeminiGenerationConfig {
                temperature: 0.2,
                response_mime_type: if json_mode {
                    Some("application/json".to_string())
                } else {
                    None
                },
            }),
        };

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url.trim_end_matches('/'),
            self.model,
            self.api_key
        );
        let client = self.client.clone();

        retry_with_backoff(
            || async {
                let response = client.post(&url).json(&request).send().await?;
                let response = response.error_for_status()?;
                let body: GeminiResponse = response.json().await?;
                let text = body
                    .candidates
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("Gemini returned no candidates"))?
                    .content
                    .parts
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("Gemini returned no parts"))?
                    .text;
                Ok(text)
            },
            3,
        )
        .await
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
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
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    system_instruction: Option<GeminiSystemInstruction>,
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiPart>,
}
