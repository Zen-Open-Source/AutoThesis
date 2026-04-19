//! Portfolios, open/closed positions, and transactions (paper-trading).

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::{Portfolio, Position, Transaction};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    pub async fn list_portfolios(&self, limit: i64) -> Result<Vec<Portfolio>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM portfolios ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_portfolio)?;
        collect_rows(rows)
    }

    pub async fn get_portfolio(&self, portfolio_id: &str) -> Result<Option<Portfolio>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM portfolios WHERE id = ?1",
            [portfolio_id],
            map_portfolio,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn create_portfolio(
        &self,
        name: &str,
        description: Option<&str>,
        cash_balance: f64,
    ) -> Result<Portfolio> {
        let portfolio = Portfolio {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            cash_balance,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO portfolios (id, name, description, cash_balance, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                portfolio.id,
                portfolio.name,
                portfolio.description,
                portfolio.cash_balance,
                encode_time(portfolio.created_at),
                encode_time(portfolio.updated_at),
            ],
        )?;

        Ok(portfolio)
    }

    pub async fn update_portfolio(
        &self,
        portfolio_id: &str,
        name: &str,
        description: Option<&str>,
        cash_balance: f64,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let rows = conn.execute(
            "UPDATE portfolios SET name = ?1, description = ?2, cash_balance = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, description, cash_balance, encode_time(Utc::now()), portfolio_id],
        )?;
        Ok(rows > 0)
    }

    pub async fn delete_portfolio(&self, portfolio_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let rows = conn.execute("DELETE FROM portfolios WHERE id = ?1", [portfolio_id])?;
        Ok(rows > 0)
    }

    pub async fn list_positions(&self, portfolio_id: &str) -> Result<Vec<Position>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare("SELECT * FROM positions WHERE portfolio_id = ?1 ORDER BY opened_at DESC")?;
        let rows = statement.query_map([portfolio_id], map_position)?;
        collect_rows(rows)
    }

    /// Batch-count active positions per portfolio in a single query.
    /// Used by the portfolios index page to avoid N `list_active_positions`
    /// round-trips.
    pub async fn count_active_positions_by_portfolio(&self) -> Result<HashMap<String, i64>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT portfolio_id, COUNT(*) FROM positions WHERE is_active = 1 GROUP BY portfolio_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (portfolio_id, count) = row?;
            map.insert(portfolio_id, count);
        }
        Ok(map)
    }

    pub async fn list_active_positions(&self, portfolio_id: &str) -> Result<Vec<Position>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM positions WHERE portfolio_id = ?1 AND is_active = 1 ORDER BY opened_at DESC",
        )?;
        let rows = statement.query_map([portfolio_id], map_position)?;
        collect_rows(rows)
    }

    pub async fn get_position(&self, position_id: &str) -> Result<Option<Position>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM positions WHERE id = ?1",
            [position_id],
            map_position,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn create_position(
        &self,
        portfolio_id: &str,
        ticker: &str,
        shares: f64,
        cost_basis_per_share: f64,
        opened_at: chrono::NaiveDate,
        notes: Option<&str>,
    ) -> Result<Position> {
        let position = Position {
            id: Uuid::new_v4().to_string(),
            portfolio_id: portfolio_id.to_string(),
            ticker: ticker.to_uppercase(),
            shares,
            cost_basis_per_share,
            total_cost: shares * cost_basis_per_share,
            opened_at,
            closed_at: None,
            notes: notes.map(|s| s.to_string()),
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO positions (id, portfolio_id, ticker, shares, cost_basis_per_share, total_cost, opened_at, closed_at, notes, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                position.id,
                position.portfolio_id,
                position.ticker,
                position.shares,
                position.cost_basis_per_share,
                position.total_cost,
                position.opened_at.to_string(),
                position.closed_at.map(|d| d.to_string()),
                position.notes,
                position.is_active as i32,
                encode_time(position.created_at),
                encode_time(position.updated_at),
            ],
        )?;

        self.create_transaction(
            portfolio_id,
            ticker,
            "buy",
            shares,
            cost_basis_per_share,
            opened_at,
            notes,
        )
        .await?;

        Ok(position)
    }

    pub async fn update_position(
        &self,
        position_id: &str,
        shares: f64,
        cost_basis_per_share: f64,
        notes: Option<&str>,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let total_cost = shares * cost_basis_per_share;
        let rows = conn.execute(
            "UPDATE positions SET shares = ?1, cost_basis_per_share = ?2, total_cost = ?3, notes = ?4, updated_at = ?5 WHERE id = ?6",
            params![shares, cost_basis_per_share, total_cost, notes, encode_time(Utc::now()), position_id],
        )?;
        Ok(rows > 0)
    }

    pub async fn close_position(
        &self,
        position_id: &str,
        closed_at: chrono::NaiveDate,
        notes: Option<&str>,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let rows = conn.execute(
            "UPDATE positions SET is_active = 0, closed_at = ?1, notes = ?2, updated_at = ?3 WHERE id = ?4",
            params![closed_at.to_string(), notes, encode_time(Utc::now()), position_id],
        )?;
        Ok(rows > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_transaction(
        &self,
        portfolio_id: &str,
        ticker: &str,
        transaction_type: &str,
        shares: f64,
        price_per_share: f64,
        executed_at: chrono::NaiveDate,
        notes: Option<&str>,
    ) -> Result<Transaction> {
        let transaction = Transaction {
            id: Uuid::new_v4().to_string(),
            portfolio_id: portfolio_id.to_string(),
            ticker: ticker.to_uppercase(),
            transaction_type: transaction_type.to_string(),
            shares,
            price_per_share,
            total_amount: shares * price_per_share,
            executed_at,
            notes: notes.map(|s| s.to_string()),
            created_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO transactions (id, portfolio_id, ticker, transaction_type, shares, price_per_share, total_amount, executed_at, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                transaction.id,
                transaction.portfolio_id,
                transaction.ticker,
                transaction.transaction_type,
                transaction.shares,
                transaction.price_per_share,
                transaction.total_amount,
                transaction.executed_at.to_string(),
                transaction.notes,
                encode_time(transaction.created_at),
            ],
        )?;

        Ok(transaction)
    }

    pub async fn list_transactions(
        &self,
        portfolio_id: &str,
        limit: i64,
    ) -> Result<Vec<Transaction>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM transactions WHERE portfolio_id = ?1 ORDER BY executed_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![portfolio_id, limit], map_transaction)?;
        collect_rows(rows)
    }
}

pub(crate) fn map_portfolio(row: &Row<'_>) -> rusqlite::Result<Portfolio> {
    Ok(Portfolio {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        cash_balance: row.get("cash_balance")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

pub(crate) fn map_position(row: &Row<'_>) -> rusqlite::Result<Position> {
    let opened_at_str: String = row.get("opened_at")?;
    let closed_at_str: Option<String> = row.get("closed_at")?;

    Ok(Position {
        id: row.get("id")?,
        portfolio_id: row.get("portfolio_id")?,
        ticker: row.get("ticker")?,
        shares: row.get("shares")?,
        cost_basis_per_share: row.get("cost_basis_per_share")?,
        total_cost: row.get("total_cost")?,
        opened_at: chrono::NaiveDate::parse_from_str(&opened_at_str, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().date_naive()),
        closed_at: closed_at_str
            .and_then(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        notes: row.get("notes")?,
        is_active: row.get::<_, i32>("is_active")? == 1,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

pub(crate) fn map_transaction(row: &Row<'_>) -> rusqlite::Result<Transaction> {
    let executed_at_str: String = row.get("executed_at")?;

    Ok(Transaction {
        id: row.get("id")?,
        portfolio_id: row.get("portfolio_id")?,
        ticker: row.get("ticker")?,
        transaction_type: row.get("transaction_type")?,
        shares: row.get("shares")?,
        price_per_share: row.get("price_per_share")?,
        total_amount: row.get("total_amount")?,
        executed_at: chrono::NaiveDate::parse_from_str(&executed_at_str, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Utc::now().date_naive()),
        notes: row.get("notes")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}
