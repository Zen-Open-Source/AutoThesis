//! Bookmarks - user-authored pointers to runs, iterations, and sources.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::Bookmark;

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    pub async fn upsert_bookmark(
        &self,
        entity_type: &str,
        entity_id: &str,
        title: &str,
        note: Option<&str>,
        target_path: &str,
    ) -> Result<Bookmark> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO bookmarks (id, entity_type, entity_id, title, note, target_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(entity_type, entity_id)
             DO UPDATE SET
                title = excluded.title,
                note = excluded.note,
                target_path = excluded.target_path,
                updated_at = excluded.updated_at",
            params![
                id,
                entity_type,
                entity_id,
                title,
                note,
                target_path,
                encode_time(now),
                encode_time(now),
            ],
        )?;
        self.get_bookmark_by_entity(entity_type, entity_id)
            .await?
            .context("bookmark missing after upsert")
    }

    pub async fn list_bookmarks(&self, limit: i64) -> Result<Vec<Bookmark>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM bookmarks ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_bookmark)?;
        collect_rows(rows)
    }

    pub async fn delete_bookmark(&self, entity_type: &str, entity_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "DELETE FROM bookmarks WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
        )?;
        Ok(affected > 0)
    }

    async fn get_bookmark_by_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<Option<Bookmark>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM bookmarks WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
            map_bookmark,
        )
        .optional()
        .map_err(Into::into)
    }
}

pub(crate) fn map_bookmark(row: &Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get("id")?,
        entity_type: row.get("entity_type")?,
        entity_id: row.get("entity_id")?,
        title: row.get("title")?,
        note: row.get("note")?,
        target_path: row.get("target_path")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}
