use crate::error::AppError;
pub use crate::error::AppResult;
use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

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

/// Maximum accepted length for a user-supplied question template.
/// Anything longer is truncated before being merged with system prompts.
pub const MAX_QUESTION_LENGTH: usize = 2_000;

/// Sanitize a user-supplied question / template: strip C0/C1 control characters
/// (except tab / newline / carriage return), collapse whitespace, and cap
/// length. This limits the blast radius of prompt-injection or jailbreak
/// attempts that rely on unusual control characters or excessive content.
pub fn sanitize_question(raw: &str) -> String {
    let filtered: String = raw
        .chars()
        .filter(|c| {
            *c == '\n' || *c == '\r' || *c == '\t' || (!c.is_control() && !is_bidi_override(*c))
        })
        .collect();
    let mut truncated: String = filtered.chars().take(MAX_QUESTION_LENGTH).collect();
    // Ensure trailing whitespace is trimmed after truncation to avoid dangling
    // partial sentences being glued to the system prompt.
    while truncated.ends_with(|c: char| c.is_whitespace()) {
        truncated.pop();
    }
    truncated
}

fn is_bidi_override(c: char) -> bool {
    matches!(
        c,
        '\u{202A}' // LRE
        | '\u{202B}' // RLE
        | '\u{202C}' // PDF
        | '\u{202D}' // LRO
        | '\u{202E}' // RLO
        | '\u{2066}' // LRI
        | '\u{2067}' // RLI
        | '\u{2068}' // FSI
        | '\u{2069}' // PDI
    )
}

/// Render a question template with ticker substitution.
///
/// The template is sanitized first so that stored templates can't smuggle
/// control characters or unbounded content into the LLM system prompt.
pub fn render_question_for_ticker(question_template: &str, ticker: &str) -> String {
    let sanitized = sanitize_question(question_template);
    if sanitized.contains("{ticker}") {
        sanitized.replace("{ticker}", ticker)
    } else {
        format!("{ticker}: {sanitized}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ticker_uppercases_and_trims() {
        assert_eq!(normalize_ticker(" nvda ").unwrap(), "NVDA");
        assert_eq!(normalize_ticker("brk.b").unwrap(), "BRK.B");
    }

    #[test]
    fn normalize_ticker_rejects_invalid_characters() {
        assert!(normalize_ticker("").is_err());
        assert!(normalize_ticker("NV DA").is_err());
        assert!(normalize_ticker("NV!DA").is_err());
    }

    #[test]
    fn normalize_tickers_dedupes_preserving_order() {
        let input = vec!["nvda".into(), "MSFT".into(), "nvda".into(), "aapl".into()];
        assert_eq!(
            normalize_tickers(input).unwrap(),
            vec!["NVDA", "MSFT", "AAPL"]
        );
    }

    #[test]
    fn render_question_substitutes_or_prefixes() {
        assert_eq!(
            render_question_for_ticker("Analyze {ticker}", "NVDA"),
            "Analyze NVDA"
        );
        assert_eq!(
            render_question_for_ticker("Earnings outlook", "NVDA"),
            "NVDA: Earnings outlook"
        );
    }

    #[test]
    fn sanitize_question_strips_control_chars_and_caps_length() {
        let raw = "Normal\u{202E}hidden\u{0007}bell";
        let clean = sanitize_question(raw);
        assert!(!clean.contains('\u{202E}'));
        assert!(!clean.contains('\u{0007}'));

        let huge = "a".repeat(MAX_QUESTION_LENGTH * 2);
        let clean = sanitize_question(&huge);
        assert!(clean.len() <= MAX_QUESTION_LENGTH);
    }

    #[test]
    fn sanitize_question_preserves_common_whitespace() {
        let raw = "line1\nline2\tcol2";
        assert_eq!(sanitize_question(raw), "line1\nline2\tcol2");
    }
}
