use crate::utils::retry_with_backoff;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// How long a current-price result stays fresh in the in-memory cache. Short
/// enough to feel live on a dashboard yet long enough to absorb bursts
/// (dashboard refresh + alerts evaluate + portfolio render all hit the same
/// ticker within milliseconds). Intraday movement inside this window is
/// acceptable for the callers that use the PriceProvider.
const PRICE_CACHE_TTL: Duration = Duration::from_secs(30);
/// Max attempts for Yahoo Finance retries. Yahoo's public chart API returns
/// the occasional 5xx under load; three tries with jittered exponential
/// backoff recovers the vast majority of those without user-visible errors.
const PRICE_FETCH_ATTEMPTS: u32 = 3;

#[derive(Clone)]
pub struct PriceProvider {
    client: reqwest::Client,
    current_cache: Arc<Mutex<HashMap<String, (Instant, PriceData)>>>,
}

#[derive(Debug, Clone)]
pub struct PriceData {
    pub ticker: String,
    pub date: chrono::NaiveDate,
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: i64,
    pub adjusted_close: f64,
}

impl PriceProvider {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("AutoThesis/1.0")
            .build()
            .context("failed to build reqwest client")?;

        Ok(Self {
            client,
            current_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Fetch current price for a ticker using Yahoo Finance API. Served from
    /// a 30-second in-memory cache to absorb duplicate calls from the same
    /// dashboard render and retried with exponential backoff on transient
    /// upstream errors.
    pub async fn get_current_price(&self, ticker: &str) -> Result<PriceData> {
        let key = ticker.to_uppercase();
        if let Some(cached) = self.cached_current(&key) {
            return Ok(cached);
        }
        // URL-encode the ticker so symbols like `BRK.B` / `BF-B` behave and
        // callers can't inject path segments.
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d",
            urlencoding_encode(ticker)
        );

        let body: YahooChartResponse = retry_with_backoff(
            || async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .context("failed to fetch price from Yahoo Finance")?;
                if !response.status().is_success() {
                    return Err(anyhow!(
                        "Yahoo Finance returned status {}",
                        response.status()
                    ));
                }
                response
                    .json::<YahooChartResponse>()
                    .await
                    .context("failed to parse Yahoo Finance response")
            },
            PRICE_FETCH_ATTEMPTS,
        )
        .await?;

        let chart = body
            .chart
            .result
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no chart data for ticker {}", ticker))?;

        let meta = chart.meta;
        let timestamp = chart
            .timestamp
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no timestamp"))?;
        let quote = chart
            .indicators
            .quote
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no quote data"))?;

        let close = quote
            .close
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no close price"))?;
        let open = quote
            .open
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no open price"))?;
        let high = quote
            .high
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no high price"))?;
        let low = quote
            .low
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no low price"))?;
        let volume = quote
            .volume
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no volume"))?;

        let date = DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.date_naive())
            .unwrap_or_else(|| Utc::now().date_naive());

        let data = PriceData {
            ticker: key.clone(),
            date,
            open,
            close,
            high,
            low,
            volume: volume.trunc() as i64,
            adjusted_close: meta.regular_market_price.unwrap_or(close),
        };
        self.store_current(key, data.clone());
        Ok(data)
    }

    fn cached_current(&self, key: &str) -> Option<PriceData> {
        let guard = self.current_cache.lock().ok()?;
        let (ts, data) = guard.get(key)?;
        if ts.elapsed() <= PRICE_CACHE_TTL {
            Some(data.clone())
        } else {
            None
        }
    }

    fn store_current(&self, key: String, data: PriceData) {
        if let Ok(mut guard) = self.current_cache.lock() {
            guard.insert(key, (Instant::now(), data));
        }
    }

    /// Fetch historical prices for a ticker
    pub async fn get_historical_prices(
        &self,
        ticker: &str,
        start_date: chrono::NaiveDate,
        end_date: chrono::NaiveDate,
    ) -> Result<Vec<PriceData>> {
        let period1 = start_date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("invalid start_date time components"))?
            .and_local_timezone(Utc)
            .single()
            .ok_or_else(|| anyhow!("ambiguous local timezone for start_date"))?
            .timestamp();
        let period2 = end_date
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| anyhow!("invalid end_date time components"))?
            .and_local_timezone(Utc)
            .single()
            .ok_or_else(|| anyhow!("ambiguous local timezone for end_date"))?
            .timestamp();

        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
            urlencoding_encode(ticker),
            period1,
            period2
        );

        let body: YahooChartResponse = retry_with_backoff(
            || async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .context("failed to fetch historical prices from Yahoo Finance")?;
                if !response.status().is_success() {
                    return Err(anyhow!(
                        "Yahoo Finance returned status {}",
                        response.status()
                    ));
                }
                response
                    .json::<YahooChartResponse>()
                    .await
                    .context("failed to parse Yahoo Finance response")
            },
            PRICE_FETCH_ATTEMPTS,
        )
        .await?;

        let chart = body
            .chart
            .result
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no chart data for ticker {}", ticker))?;

        let quote = chart
            .indicators
            .quote
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no quote data"))?;

        let mut prices = Vec::new();
        for (i, &timestamp) in chart.timestamp.iter().enumerate() {
            if i >= quote.close.len()
                || i >= quote.open.len()
                || i >= quote.high.len()
                || i >= quote.low.len()
                || i >= quote.volume.len()
            {
                continue;
            }

            let close = quote.close[i];
            let open = quote.open[i];
            let high = quote.high[i];
            let low = quote.low[i];
            let volume = quote.volume[i];

            if close.is_nan() || open.is_nan() {
                continue;
            }

            let date = DateTime::from_timestamp(timestamp, 0)
                .map(|dt| dt.date_naive())
                .unwrap_or_else(|| Utc::now().date_naive());

            prices.push(PriceData {
                ticker: ticker.to_uppercase(),
                date,
                open,
                close,
                high,
                low,
                volume: volume.trunc() as i64,
                adjusted_close: close,
            });
        }

        Ok(prices)
    }
}

/// Minimal URL path-segment encoder that preserves alphanumerics, dot, dash,
/// underscore, and tilde (the unreserved set per RFC 3986) and percent-encodes
/// everything else. We avoid pulling in the full `percent-encoding` crate for
/// what amounts to ticker symbols.
fn urlencoding_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let b = *byte;
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct YahooChartResponse {
    chart: YahooChart,
}

#[derive(Debug, Deserialize)]
struct YahooChart {
    result: Vec<YahooResult>,
}

#[derive(Debug, Deserialize)]
struct YahooResult {
    meta: YahooMeta,
    timestamp: Vec<i64>,
    indicators: YahooIndicators,
}

#[derive(Debug, Deserialize)]
struct YahooMeta {
    #[serde(rename = "regularMarketPrice")]
    regular_market_price: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct YahooIndicators {
    quote: Vec<YahooQuote>,
}

#[derive(Debug, Deserialize)]
struct YahooQuote {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
}
