//! Run templates: reusable (name, question) pairs for manual or scheduled runs.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::RunTemplate;

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    pub async fn create_run_template(
        &self,
        name: &str,
        question_template: &str,
        description: Option<&str>,
    ) -> Result<RunTemplate> {
        let run_template = RunTemplate {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            question_template: question_template.to_string(),
            description: description.map(ToOwned::to_owned),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO run_templates (id, name, question_template, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_template.id,
                run_template.name,
                run_template.question_template,
                run_template.description,
                encode_time(run_template.created_at),
                encode_time(run_template.updated_at),
            ],
        )?;

        self.get_run_template(&run_template.id)
            .await?
            .context("created run template missing after insert")
    }

    pub async fn get_run_template(&self, template_id: &str) -> Result<Option<RunTemplate>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM run_templates WHERE id = ?1",
            [template_id],
            map_run_template,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_run_templates(&self, limit: i64) -> Result<Vec<RunTemplate>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM run_templates ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([limit], map_run_template)?;
        collect_rows(rows)
    }

    pub async fn update_run_template(
        &self,
        template_id: &str,
        name: &str,
        question_template: &str,
        description: Option<&str>,
    ) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute(
            "UPDATE run_templates
             SET name = ?1, question_template = ?2, description = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                name,
                question_template,
                description,
                encode_time(Utc::now()),
                template_id,
            ],
        )?;
        Ok(affected > 0)
    }

    pub async fn delete_run_template(&self, template_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute("DELETE FROM run_templates WHERE id = ?1", [template_id])?;
        Ok(affected > 0)
    }
}

pub(crate) fn map_run_template(row: &Row<'_>) -> rusqlite::Result<RunTemplate> {
    Ok(RunTemplate {
        id: row.get("id")?,
        name: row.get("name")?,
        question_template: row.get("question_template")?,
        description: row.get("description")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}
