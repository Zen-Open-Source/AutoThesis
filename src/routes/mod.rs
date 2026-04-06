pub mod comparisons;
pub mod health;
pub mod pages;
pub mod runs;

use crate::app_state::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{services::ServeDir, trace::TraceLayer};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(pages::index))
        .route("/runs/:id", get(pages::run_detail))
        .route(
            "/runs/:id/iterations/:iteration_number",
            get(pages::iteration_detail),
        )
        .route("/comparisons", get(pages::comparisons_index))
        .route("/comparisons/:id", get(pages::comparison_detail))
        .route("/healthz", get(health::healthz))
        .route("/api/runs", post(runs::create_run).get(runs::list_runs))
        .route("/api/runs/:id", get(runs::get_run))
        .route("/api/runs/:id/events", get(runs::get_events))
        .route("/api/runs/:id/iterations", get(runs::list_iterations))
        .route(
            "/api/runs/:id/iterations/:iteration_number",
            get(runs::get_iteration),
        )
        .route("/api/runs/:id/final", get(runs::get_final))
        .route(
            "/api/comparisons",
            post(comparisons::create_comparison).get(comparisons::list_comparisons),
        )
        .route(
            "/api/comparisons/:id",
            get(comparisons::get_comparison).delete(comparisons::delete_comparison),
        )
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
