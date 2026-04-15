use crate::error::AppError;
use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

pub type AppResult<T> = Result<T, AppError>;

/// Normalize a ticker symbol to uppercase with validation.
pub fn normalize_ticker(raw: &str) -> AppResult<String> {
    let cleaned = raw.trim().to_uppercase();
    if cleaned.is_empty() {
        return Err(AppError::BadRequest("ticker is required".to_string()));
    }
    if !cleaned
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(AppError::BadRequest(
            "ticker must contain only letters, numbers, '.' or '-'".to_string(),
        ));
    }
    Ok(cleaned)
}

/// Normalize and deduplicate a list of tickers.
pub fn normalize_tickers(raw_tickers: Vec<String>) -> AppResult<Vec<String>> {
    let mut seen = HashSet::new();
    let mut tickers = Vec::new();
    for raw_ticker in raw_tickers {
        let ticker = normalize_ticker(&raw_ticker)?;
        if seen.insert(ticker.clone()) {
            tickers.push(ticker);
        }
    }
    Ok(tickers)
}

/// Render a question template with ticker substitution.
pub fn render_question_for_ticker(question_template: &str, ticker: &str) -> String {
    if question_template.contains("{ticker}") {
        question_template.replace("{ticker}", ticker)
    } else {
        format!("{ticker}: {question_template}")
    }
}

/// Retry an async operation with exponential backoff.
pub async fn retry_with_backoff<F, Fut, T>(mut operation: F, max_attempts: u32) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut last_error = None;
    for attempt in 0..max_attempts {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) => {
                last_error = Some(error);
                if attempt < max_attempts - 1 {
                    sleep(Duration::from_millis(500 * (attempt + 1) as u64)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("operation failed after {} attempts", max_attempts)))
}
