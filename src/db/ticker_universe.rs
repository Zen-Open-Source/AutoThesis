//! Ticker universe: the set of symbols available for research and scanning,
//! along with metadata (sector/industry/market cap) used for filtering.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use std::{collections::HashMap, fs, path::PathBuf};
use uuid::Uuid;

use crate::models::TickerUniverse;

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    pub async fn upsert_ticker_universe(
        &self,
        ticker: &str,
        name: Option<&str>,
        sector: Option<&str>,
        industry: Option<&str>,
        market_cap_billion: Option<f64>,
        is_sp500: bool,
    ) -> Result<TickerUniverse> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO ticker_universe (id, ticker, name, sector, industry, market_cap_billion, is_sp500, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9)
             ON CONFLICT(ticker) DO UPDATE SET
               name = excluded.name,
               sector = excluded.sector,
               industry = excluded.industry,
               market_cap_billion = excluded.market_cap_billion,
               is_sp500 = excluded.is_sp500,
               updated_at = excluded.updated_at",
            params![
                id,
                ticker,
                name,
                sector,
                industry,
                market_cap_billion,
                if is_sp500 { 1 } else { 0 },
                encode_time(now),
                encode_time(now),
            ],
        )?;

        self.get_ticker_universe(ticker)
            .await?
            .context("ticker universe entry missing after upsert")
    }

    /// Batch lookup of `TickerUniverse` rows keyed by ticker. Avoids an N+1
    /// sequence of `get_ticker_universe` calls.
    pub async fn get_ticker_universe_batch(
        &self,
        tickers: &[String],
    ) -> Result<HashMap<String, TickerUniverse>> {
        if tickers.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.open_connection()?;
        let placeholders: String = (0..tickers.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT * FROM ticker_universe WHERE ticker IN ({placeholders})");
        let params: Vec<&dyn rusqlite::ToSql> =
            tickers.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), map_ticker_universe)?;
        let mut map = HashMap::new();
        for row in rows {
            let entry = row?;
            map.insert(entry.ticker.clone(), entry);
        }
        Ok(map)
    }

    pub async fn get_ticker_universe(&self, ticker: &str) -> Result<Option<TickerUniverse>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM ticker_universe WHERE ticker = ?1",
            [ticker],
            map_ticker_universe,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_ticker_universe(
        &self,
        active_only: bool,
        sector_filter: Option<&str>,
        min_market_cap: Option<f64>,
        max_market_cap: Option<f64>,
    ) -> Result<Vec<TickerUniverse>> {
        let conn = self.open_connection()?;
        let mut sql = "SELECT * FROM ticker_universe WHERE 1=1".to_string();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if active_only {
            sql.push_str(" AND is_active = 1");
        }
        if let Some(sector) = sector_filter {
            sql.push_str(" AND sector = ?");
            params_vec.push(Box::new(sector.to_string()));
        }
        if let Some(min) = min_market_cap {
            sql.push_str(" AND market_cap_billion >= ?");
            params_vec.push(Box::new(min));
        }
        if let Some(max) = max_market_cap {
            sql.push_str(" AND market_cap_billion <= ?");
            params_vec.push(Box::new(max));
        }
        sql.push_str(" ORDER BY ticker ASC");

        let params: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params.as_slice(), map_ticker_universe)?;
        collect_rows(rows)
    }

    pub async fn count_ticker_universe(&self, active_only: bool) -> Result<i64> {
        let conn = self.open_connection()?;
        let count = if active_only {
            conn.query_row(
                "SELECT COUNT(*) FROM ticker_universe WHERE is_active = 1",
                [],
                |row| row.get(0),
            )?
        } else {
            conn.query_row("SELECT COUNT(*) FROM ticker_universe", [], |row| row.get(0))?
        };
        Ok(count)
    }

    pub async fn set_ticker_universe_active(&self, ticker: &str, is_active: bool) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE ticker_universe SET is_active = ?1, updated_at = ?2 WHERE ticker = ?3",
            params![
                if is_active { 1 } else { 0 },
                encode_time(Utc::now()),
                ticker
            ],
        )?;
        Ok(affected > 0)
    }

    pub async fn seed_sp500_universe(&self) -> Result<i64> {
        let count = self.count_ticker_universe(false).await?;
        if count > 0 {
            return Ok(0);
        }

        let seed_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sql/seed_sp500.sql");
        let seed_sql =
            fs::read_to_string(&seed_path).context("failed to read S&P 500 seed file")?;
        let conn = self.open_connection()?;
        conn.execute_batch(&seed_sql)?;

        self.count_ticker_universe(false).await
    }
}

pub(crate) fn map_ticker_universe(row: &Row<'_>) -> rusqlite::Result<TickerUniverse> {
    Ok(TickerUniverse {
        id: row.get("id")?,
        ticker: row.get("ticker")?,
        name: row.get("name")?,
        sector: row.get("sector")?,
        industry: row.get("industry")?,
        market_cap_billion: row.get("market_cap_billion")?,
        is_sp500: row.get::<_, i64>("is_sp500")? > 0,
        is_active: row.get::<_, i64>("is_active")? > 0,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}
