pub mod app_state;
pub mod config;
pub mod db;
pub mod error;
pub mod markdown;
pub mod models;
pub mod providers;
pub mod routes;
pub mod services;

use anyhow::Result;
use app_state::AppState;
use axum::Router;
use config::Config;

pub async fn build_state_from_env() -> Result<AppState> {
    let config = Config::from_env()?;
    AppState::from_config(config).await
}

pub fn build_app(state: AppState) -> Router {
    routes::router(state)
}
