pub mod alerts;
pub mod batches;
pub mod bookmarks;
pub mod comparisons;
pub mod health;
pub mod pages;
pub mod run_templates;
pub mod runs;
pub mod scanner;
pub mod watchlists;

use crate::app_state::AppState;
use axum::{
    routing::{get, post, put},
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
        .route("/batches", get(pages::batches_index))
        .route("/batches/:id", get(pages::batch_detail))
        .route("/dashboard", get(pages::dashboard_index))
        .route("/bookmarks", get(pages::bookmarks_index))
        .route("/templates", get(pages::run_templates_index))
        .route("/scanner", get(pages::scanner_index))
        .route(
            "/scanner/opportunities/:id",
            get(pages::scanner_opportunity_detail),
        )
        .route("/healthz", get(health::healthz))
        .route("/api/runs", post(runs::create_run).get(runs::list_runs))
        .route("/api/runs/:id", get(runs::get_run))
        .route("/api/runs/:id/cancel", post(runs::cancel_run))
        .route("/api/runs/:id/retry", post(runs::retry_run))
        .route("/api/runs/:id/events", get(runs::get_events))
        .route("/api/runs/:id/iterations", get(runs::list_iterations))
        .route(
            "/api/runs/:id/iterations/:iteration_number",
            get(runs::get_iteration),
        )
        .route("/api/runs/:id/final", get(runs::get_final))
        .route(
            "/api/batches",
            post(batches::create_batch_job).get(batches::list_batch_jobs),
        )
        .route("/api/batches/:id", get(batches::get_batch_job))
        .route(
            "/api/run-templates",
            get(run_templates::list_run_templates).post(run_templates::create_run_template),
        )
        .route(
            "/api/run-templates/:id",
            put(run_templates::update_run_template).delete(run_templates::delete_run_template),
        )
        .route(
            "/api/watchlists",
            get(watchlists::list_watchlists).post(watchlists::create_watchlist),
        )
        .route(
            "/api/watchlists/:id",
            get(watchlists::get_watchlist)
                .put(watchlists::update_watchlist)
                .delete(watchlists::delete_watchlist),
        )
        .route(
            "/api/watchlists/:id/tickers",
            post(watchlists::add_watchlist_ticker),
        )
        .route(
            "/api/watchlists/:id/tickers/:ticker",
            axum::routing::delete(watchlists::remove_watchlist_ticker),
        )
        .route("/api/alerts", get(alerts::list_alerts))
        .route("/api/alerts/:id/dismiss", post(alerts::dismiss_alert))
        .route("/api/alerts/:id/snooze", post(alerts::snooze_alert))
        .route("/api/dashboard", get(watchlists::get_dashboard))
        .route(
            "/api/dashboard/refresh",
            post(watchlists::refresh_dashboard_ticker),
        )
        .route(
            "/api/comparisons",
            post(comparisons::create_comparison).get(comparisons::list_comparisons),
        )
        .route(
            "/api/comparisons/:id",
            get(comparisons::get_comparison).delete(comparisons::delete_comparison),
        )
        .route(
            "/api/bookmarks",
            get(bookmarks::list_bookmarks)
                .post(bookmarks::upsert_bookmark)
                .delete(bookmarks::delete_bookmark),
        )
        .route(
            "/api/sources/:source_id/annotations",
            get(bookmarks::list_source_annotations)
                .post(bookmarks::create_source_annotation)
                .delete(bookmarks::delete_source_annotation),
        )
        .route("/api/scanner", get(scanner::get_scanner_dashboard))
        .route(
            "/api/scanner/runs",
            post(scanner::create_scan_run).get(scanner::list_scan_runs),
        )
        .route("/api/scanner/runs/:id", get(scanner::get_scan_run))
        .route(
            "/api/scanner/opportunities/:id",
            get(scanner::get_scan_opportunity),
        )
        .route(
            "/api/scanner/opportunities/:id/promote",
            post(scanner::promote_opportunity),
        )
        .route(
            "/api/scanner/opportunities/:id/dismiss",
            post(scanner::dismiss_opportunity),
        )
        .route(
            "/api/scanner/universe",
            get(scanner::list_ticker_universe).post(scanner::add_ticker_to_universe),
        )
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
