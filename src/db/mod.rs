//! Database module.
//!
//! The `Database` type is defined here and each domain-specific set of
//! methods lives in its own submodule (see `mod` declarations below). All
//! submodules share the connection pool, migrations, and the small set of
//! row-mapping/time-encoding helpers exposed at the bottom of this file.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, Error as RusqliteError, OptionalExtension, Row};
use std::{fs, path::PathBuf, sync::Arc};

mod batches;
mod bookmarks;
mod comparisons;
mod llm;
mod portfolios;
mod prices;
mod research;
mod run_templates;
mod runs;
mod scanner;
mod scheduled_runs;
mod search;
mod source_quality;
mod thesis;
mod ticker_universe;
mod watchlists;

pub use search::{EvidenceNoteInsert, RankedInsert};

pub(crate) type SqlitePool = Pool<SqliteConnectionManager>;
pub(crate) type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

#[derive(Clone)]
pub struct Database {
    pool: Arc<SqlitePool>,
}

#[derive(Debug)]
struct SqlitePragmaSetup;

impl CustomizeConnection<Connection, RusqliteError> for SqlitePragmaSetup {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), RusqliteError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(())
    }
}

impl Database {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let path = normalize_database_url(database_url);
        let manager = if path == ":memory:" {
            SqliteConnectionManager::memory()
        } else {
            SqliteConnectionManager::file(&path)
        };
        let manager = manager.with_init(|_| Ok(()));

        // For in-memory databases every new connection is a distinct DB, so we
        // must cap the pool at 1 to preserve schema + data across calls.
        let is_memory = path == ":memory:";
        let max_size = if is_memory { 1 } else { 8 };

        let pool = Pool::builder()
            .max_size(max_size)
            .connection_customizer(Box::new(SqlitePragmaSetup))
            .build(manager)
            .context("failed to build sqlite connection pool")?;

        let database = Self {
            pool: Arc::new(pool),
        };
        database.run_migrations()?;
        Ok(database)
    }

    pub(crate) fn open_connection(&self) -> Result<PooledConn> {
        self.pool
            .get()
            .context("failed to acquire sqlite connection from pool")
    }

    fn run_migrations(&self) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (version TEXT PRIMARY KEY)",
            [],
        )?;

        let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sql/migrations");
        let mut entries = fs::read_dir(&migrations_dir)
            .with_context(|| {
                format!(
                    "failed to read migrations directory {}",
                    migrations_dir.display()
                )
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            if !entry.file_type()?.is_file() {
                continue;
            }
            let version = entry.file_name().to_string_lossy().to_string();
            let already_applied = conn
                .query_row(
                    "SELECT version FROM schema_migrations WHERE version = ?1",
                    [version.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if already_applied.is_some() {
                continue;
            }

            let sql = fs::read_to_string(entry.path())?;
            conn.execute_batch(&sql)?;
            conn.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [version.as_str()],
            )?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared helpers used by every db submodule.
// ---------------------------------------------------------------------------

fn normalize_database_url(database_url: &str) -> String {
    if database_url == "sqlite::memory:" || database_url == ":memory:" {
        ":memory:".to_string()
    } else if let Some(stripped) = database_url.strip_prefix("sqlite://") {
        stripped.to_string()
    } else if let Some(stripped) = database_url.strip_prefix("sqlite:") {
        stripped.to_string()
    } else {
        database_url.to_string()
    }
}

pub(crate) fn encode_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

pub(crate) fn option_time(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(encode_time)
}

pub(crate) fn parse_time(value: String) -> rusqlite::Result<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

pub(crate) fn parse_time_opt(s: &str) -> Option<DateTime<Utc>> {
    parse_time(s.to_string()).ok()
}

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}
