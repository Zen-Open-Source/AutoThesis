//! Multi-model research panel: providers, model runs, model comparisons,
//! per-provider quality aggregates.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::models::{LlmProvider, ModelComparison, ModelQualityScore, ModelRun};

use super::{collect_rows, encode_time, parse_time, Database};

impl Database {
    #[allow(clippy::too_many_arguments)]
    pub async fn create_llm_provider(
        &self,
        name: &str,
        provider_type: &str,
        api_key_encrypted: Option<&str>,
        model: &str,
        base_url: Option<&str>,
        is_enabled: bool,
        is_default: bool,
        priority: i64,
        config_json: Option<&str>,
    ) -> Result<LlmProvider> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO llm_providers (id, name, provider_type, api_key_encrypted, model, base_url, is_enabled, is_default, priority, config_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id, name, provider_type, api_key_encrypted, model, base_url,
                if is_enabled { 1 } else { 0 }, if is_default { 1 } else { 0 },
                priority, config_json, encode_time(now), encode_time(now),
            ],
        )?;
        self.get_llm_provider(&id)
            .await?
            .context("created provider missing")
    }

    pub async fn get_llm_provider(&self, provider_id: &str) -> Result<Option<LlmProvider>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM llm_providers WHERE id = ?1",
            [provider_id],
            map_llm_provider,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_llm_providers(&self, enabled_only: bool) -> Result<Vec<LlmProvider>> {
        let conn = self.open_connection()?;
        let sql = if enabled_only {
            "SELECT * FROM llm_providers WHERE is_enabled = 1 ORDER BY priority DESC, name ASC"
        } else {
            "SELECT * FROM llm_providers ORDER BY priority DESC, name ASC"
        };
        let mut statement = conn.prepare(sql)?;
        let rows = statement.query_map([], map_llm_provider)?;
        collect_rows(rows)
    }

    pub async fn delete_llm_provider(&self, provider_id: &str) -> Result<bool> {
        let conn = self.open_connection()?;
        let affected = conn.execute("DELETE FROM llm_providers WHERE id = ?1", [provider_id])?;
        Ok(affected > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_model_run(
        &self,
        run_id: &str,
        provider_id: &str,
        iteration_number: Option<i64>,
        output_type: &str,
        output_content: Option<&str>,
        tokens_used: Option<i64>,
        latency_ms: Option<i64>,
        cost_estimate: Option<f64>,
        quality_score: Option<f64>,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<ModelRun> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO model_runs (id, run_id, provider_id, iteration_number, output_type, output_content, tokens_used, latency_ms, cost_estimate, quality_score, status, error_message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                id, run_id, provider_id, iteration_number, output_type, output_content,
                tokens_used, latency_ms, cost_estimate, quality_score, status, error_message,
                encode_time(now),
            ],
        )?;
        self.get_model_run(&id).await?.context("model run missing")
    }

    pub async fn get_model_run(&self, id: &str) -> Result<Option<ModelRun>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM model_runs WHERE id = ?1",
            [id],
            map_model_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_model_runs_for_run(&self, run_id: &str) -> Result<Vec<ModelRun>> {
        let conn = self.open_connection()?;
        let mut statement =
            conn.prepare("SELECT * FROM model_runs WHERE run_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map([run_id], map_model_run)?;
        collect_rows(rows)
    }

    pub async fn create_model_comparison(
        &self,
        run_id: &str,
        comparison_type: &str,
        winner_provider_id: Option<&str>,
        comparison_json: &str,
        similarity_score: Option<f64>,
        key_differences: Option<&str>,
    ) -> Result<ModelComparison> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO model_comparisons (id, run_id, comparison_type, winner_provider_id, comparison_json, similarity_score, key_differences, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, run_id, comparison_type, winner_provider_id, comparison_json, similarity_score, key_differences, encode_time(now)],
        )?;
        self.get_model_comparison(&id)
            .await?
            .context("comparison missing")
    }

    pub async fn get_model_comparison(&self, id: &str) -> Result<Option<ModelComparison>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM model_comparisons WHERE id = ?1",
            [id],
            map_model_comparison,
        )
        .optional()
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_model_quality_score(
        &self,
        provider_id: &str,
        total_runs: i64,
        successful_runs: i64,
        avg_quality_score: Option<f64>,
        avg_latency_ms: Option<f64>,
        total_tokens: i64,
        total_cost: f64,
        accuracy_score: Option<f64>,
    ) -> Result<ModelQualityScore> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO model_quality_scores (id, provider_id, total_runs, successful_runs, avg_quality_score, avg_latency_ms, total_tokens, total_cost, accuracy_score, last_run_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(provider_id) DO UPDATE SET
               total_runs = excluded.total_runs, successful_runs = excluded.successful_runs,
               avg_quality_score = excluded.avg_quality_score, avg_latency_ms = excluded.avg_latency_ms,
               total_tokens = excluded.total_tokens, total_cost = excluded.total_cost,
               accuracy_score = excluded.accuracy_score, last_run_at = excluded.last_run_at, updated_at = excluded.updated_at",
            params![id, provider_id, total_runs, successful_runs, avg_quality_score, avg_latency_ms, total_tokens, total_cost, accuracy_score, encode_time(now), encode_time(now), encode_time(now)],
        )?;
        self.get_model_quality_score(provider_id)
            .await?
            .context("quality score missing")
    }

    pub async fn get_model_quality_score(
        &self,
        provider_id: &str,
    ) -> Result<Option<ModelQualityScore>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "SELECT * FROM model_quality_scores WHERE provider_id = ?1",
            [provider_id],
            map_model_quality_score,
        )
        .optional()
        .map_err(Into::into)
    }

    pub async fn list_model_quality_scores(&self) -> Result<Vec<ModelQualityScore>> {
        let conn = self.open_connection()?;
        let mut statement = conn.prepare(
            "SELECT * FROM model_quality_scores ORDER BY accuracy_score DESC NULLS LAST",
        )?;
        let rows = statement.query_map([], map_model_quality_score)?;
        collect_rows(rows)
    }
}

pub(crate) fn map_llm_provider(row: &Row<'_>) -> rusqlite::Result<LlmProvider> {
    Ok(LlmProvider {
        id: row.get("id")?,
        name: row.get("name")?,
        provider_type: row.get("provider_type")?,
        api_key_encrypted: row.get("api_key_encrypted")?,
        model: row.get("model")?,
        base_url: row.get("base_url")?,
        is_enabled: row.get::<_, i64>("is_enabled")? > 0,
        is_default: row.get::<_, i64>("is_default")? > 0,
        priority: row.get("priority")?,
        config_json: row.get("config_json")?,
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}

pub(crate) fn map_model_run(row: &Row<'_>) -> rusqlite::Result<ModelRun> {
    Ok(ModelRun {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        provider_id: row.get("provider_id")?,
        iteration_number: row.get("iteration_number")?,
        output_type: row.get("output_type")?,
        output_content: row.get("output_content")?,
        tokens_used: row.get("tokens_used")?,
        latency_ms: row.get("latency_ms")?,
        cost_estimate: row.get("cost_estimate")?,
        quality_score: row.get("quality_score")?,
        status: row.get("status")?,
        error_message: row.get("error_message")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_model_comparison(row: &Row<'_>) -> rusqlite::Result<ModelComparison> {
    Ok(ModelComparison {
        id: row.get("id")?,
        run_id: row.get("run_id")?,
        comparison_type: row.get("comparison_type")?,
        winner_provider_id: row.get("winner_provider_id")?,
        comparison_json: row.get("comparison_json")?,
        similarity_score: row.get("similarity_score")?,
        key_differences: row.get("key_differences")?,
        created_at: parse_time(row.get("created_at")?)?,
    })
}

pub(crate) fn map_model_quality_score(row: &Row<'_>) -> rusqlite::Result<ModelQualityScore> {
    Ok(ModelQualityScore {
        id: row.get("id")?,
        provider_id: row.get("provider_id")?,
        total_runs: row.get("total_runs")?,
        successful_runs: row.get("successful_runs")?,
        avg_quality_score: row.get("avg_quality_score")?,
        avg_latency_ms: row.get("avg_latency_ms")?,
        total_tokens: row.get("total_tokens")?,
        total_cost: row.get("total_cost")?,
        accuracy_score: row.get("accuracy_score")?,
        last_run_at: row
            .get::<_, Option<String>>("last_run_at")?
            .and_then(|s| parse_time(s).ok()),
        created_at: parse_time(row.get("created_at")?)?,
        updated_at: parse_time(row.get("updated_at")?)?,
    })
}
