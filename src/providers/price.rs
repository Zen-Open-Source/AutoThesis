use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone)]
pub struct PriceProvider {
    client: reqwest::Client,
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

        Ok(Self { client })
    }

    /// Fetch current price for a ticker using Yahoo Finance API
    pub async fn get_current_price(&self, ticker: &str) -> Result<PriceData> {
        // Use Yahoo Finance query API
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d",
            ticker
        );

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

        let body: YahooChartResponse = response
            .json()
            .await
            .context("failed to parse Yahoo Finance response")?;

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

        Ok(PriceData {
            ticker: ticker.to_uppercase(),
            date,
            open,
            close,
            high,
            low,
            volume: volume as i64,
            adjusted_close: meta.regular_market_price.unwrap_or(close),
        })
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
            .unwrap()
            .and_local_timezone(Utc)
            .unwrap()
            .timestamp();
        let period2 = end_date
            .and_hms_opt(23, 59, 59)
            .unwrap()
            .and_local_timezone(Utc)
            .unwrap()
            .timestamp();

        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
            ticker, period1, period2
        );

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

        let body: YahooChartResponse = response
            .json()
            .await
            .context("failed to parse Yahoo Finance response")?;

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
                volume: volume as i64,
                adjusted_close: close,
            });
        }

        Ok(prices)
    }
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
