use anyhow::Result;
use autothesis::{build_app, build_state_from_env, services::scheduler};
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let state = build_state_from_env().await?;

    // Start the scheduler background loop
    let _active_runs = scheduler::start_scheduler(state.clone());

    let address = state.config.address();
    let listener = TcpListener::bind(&address).await?;
    info!(%address, "listening");
    axum::serve(listener, build_app(state)).await?;
    Ok(())
}
