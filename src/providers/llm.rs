use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete_json(
        &self,
        prompt_name: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<Value>;

    async fn complete_markdown(
        &self,
        prompt_name: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String>;
}
