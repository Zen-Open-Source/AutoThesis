//! Historical price snapshots (end-of-day OHLCV keyed by ticker + date).

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::PriceSnapshot;

use super::{encode_time, parse_time, Database};

impl Database {
    #[allow(clippy::too_many_arguments)]
    pub async fn create_price_snapshot(
        &self,
        ticker: &str,
        price_date: chrono::NaiveDate,
        open_price: f64,
        close_price: f64,
        high_price: Option<f64>,
        low_price: Option<f64>,
        volume: Option<i64>,
        adjusted_close: Option<f64>,
        source: &str,
    ) -> Result<PriceSnapshot> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO price_snapshots (id, ticker, price_date, open_price, close_price, high_price, low_price, volume, adjusted_close, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![id, ticker, price_date.to_string(), open_price, close_price, high_price, low_price, volume, adjusted_close, source, encode_time(now)],
        )?;
        self.get_price_snapshot_by_ticker_date(ticker, price_date)
            .await?
            .context("price snapshot missing")
    }

    pub async fn get_price_snapshot_by_ticker_date(
        &self,
        ticker: &str,
        price_date: chrono::NaiveDate,
    ) -> Result<Option<PriceSnapshot>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM price_snapshots WHERE ticker = ?1 AND price_date = ?2",
            params![ticker, price_date.to_string()],
            map_price_snapshot,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn get_latest_price_snapshot(&self, ticker: &str) -> Result<Option<PriceSnapshot>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM price_snapshots WHERE ticker = ?1 ORDER BY price_date DESC LIMIT 1",
            [ticker],
            map_price_snapshot,
        )
        .optional()
        .map_err(Into::into)
    }
}

pub(crate) fn map_price_snapshot(row: &Row<'_>) -> rusqlite::Result<PriceSnapshot> {
    Ok(PriceSnapshot {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        price_date: row
            .get::<_, String>("price_date")?
            .parse()
            .ok()
            .unwrap_or_default(),
        open_price: row.get("open_price")?,
        close_price: row.get("close_price")?,
        high_price: row.get("high_price")?,
        low_price: row.get("low_price")?,
        volume: row.get("volume")?,
        adjusted_close: row.get("adjusted_close")?,
        source: row.get("source")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}
