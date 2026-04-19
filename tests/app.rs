use anyhow::{anyhow, Result};
use async_trait::async_trait;
use autothesis::{
    app_state::{AppState, PromptStore},
    build_app,
    config::Config,
    db::Database,
    models::{CreateRunRequest, PlannerOutput, ReaderOutput, SearchQueryOutput},
    providers::{
        fetch::{FetchedPage, WebFetcher},
        llm::LlmProvider,
        price::PriceProvider,
        search::{SearchProvider, SearchResultItem},
    },
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

#[tokio::test]
async fn db_persists_run_iteration_and_events() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let run = ctx.state.db.create_run("NVDA", "Test question").await?;
    let iteration = ctx.state.db.create_iteration(&run.id, 1).await?;
    ctx.state
        .db
        .insert_search_query(&iteration.id, "nvda earnings release")
        .await?;
    ctx.state
        .db
        .insert_event(&run.id, Some(&iteration.id), "test_event", "hello", None)
        .await?;

    let stored_run = ctx
        .state
        .db
        .get_run(&run.id)
        .await?
        .expect("run should exist");
    let iterations = ctx.state.db.list_iterations(&run.id).await?;
    let events = ctx.state.db.list_events(&run.id).await?;

    assert_eq!(stored_run.ticker, "NVDA");
    assert_eq!(iterations.len(), 1);
    assert_eq!(events.len(), 1);
    Ok(())
}

#[tokio::test]
async fn create_run_endpoint_queues_and_completes_run() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&CreateRunRequest {
                    ticker: "nvda".to_string(),
                    question: None,
                    template_id: None,
                })?))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let payload: Value = serde_json::from_slice(&body)?;
    let run_id = payload["run_id"].as_str().expect("run id");

    let run = wait_for_run_completion(&ctx.state, run_id).await?;
    assert_eq!(run.status, "completed");
    assert_eq!(run.ticker, "NVDA");
    Ok(())
}

#[tokio::test]
async fn end_to_end_run_completes_three_iterations_and_persists_artifacts() -> Result<()> {
    let ctx = TestContext::new(3, false).await?;

    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&CreateRunRequest {
                    ticker: "NVDA".to_string(),
                    question: Some("What is the bull and bear case?".to_string()),
                    template_id: None,
                })?))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let payload: Value = serde_json::from_slice(&body)?;
    let run_id = payload["run_id"].as_str().expect("run id");

    let run = wait_for_run_completion(&ctx.state, run_id).await?;
    let iterations = ctx.state.db.list_iterations(run_id).await?;
    let detail = ctx
        .state
        .db
        .get_iteration_detail(run_id, 3)
        .await?
        .expect("iteration detail");
    let sources = ctx.state.db.list_sources(&detail.iteration.id).await?;
    let all_events = ctx.state.db.list_events(run_id).await?;

    assert_eq!(run.status, "completed");
    assert_eq!(iterations.len(), 3);
    assert!(run
        .final_memo_markdown
        .as_deref()
        .unwrap_or_default()
        .contains("# Executive Summary"));
    assert!(run
        .final_memo_markdown
        .as_deref()
        .unwrap_or_default()
        .contains("## Bull Case"));
    assert!(run
        .final_memo_markdown
        .as_deref()
        .unwrap_or_default()
        .contains("## Bear Case"));
    assert!(run
        .final_memo_markdown
        .as_deref()
        .unwrap_or_default()
        .contains("## Risks"));
    assert!(run
        .final_memo_markdown
        .as_deref()
        .unwrap_or_default()
        .contains("## Known Unknowns"));
    assert!(run
        .final_memo_markdown
        .as_deref()
        .unwrap_or_default()
        .contains("## Sources"));
    assert!(run.final_memo_html.is_some());
    assert!(iterations
        .iter()
        .all(|iteration| iteration.critique_markdown.is_some()));
    assert!(sources.len() >= 2);
    assert!(all_events
        .iter()
        .any(|event| event.event_type == "run_completed"));
    Ok(())
}

#[tokio::test]
async fn source_fetch_failure_is_recorded_and_run_still_completes() -> Result<()> {
    let ctx = TestContext::new(2, true).await?;

    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&CreateRunRequest {
                    ticker: "AMZN".to_string(),
                    question: None,
                    template_id: None,
                })?))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let payload: Value = serde_json::from_slice(&body)?;
    let run_id = payload["run_id"].as_str().expect("run id");

    let run = wait_for_run_completion(&ctx.state, run_id).await?;
    let events = ctx.state.db.list_events(run_id).await?;

    assert_eq!(run.status, "completed");
    assert!(events
        .iter()
        .any(|event| event.event_type == "source_fetch_failed"));
    Ok(())
}

#[tokio::test]
async fn comparison_endpoint_updates_terminal_status_and_rollup() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/comparisons")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "MegaCap Pair",
                    "tickers": ["AAPL", "MSFT"],
                    "question": "Compare growth and valuation"
                }))?))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let payload: Value = serde_json::from_slice(&body)?;
    let comparison_id = payload["comparison_id"].as_str().expect("comparison id");

    let comparison = wait_for_comparison_terminal(&ctx.state, comparison_id).await?;
    assert_eq!(comparison.status, "completed");
    assert!(comparison.summary.is_some());
    assert!(comparison.final_comparison_html.is_some());

    let detail_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/comparisons/{comparison_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_body = detail_response.into_body().collect().await?.to_bytes();
    let detail_payload: Value = serde_json::from_slice(&detail_body)?;
    assert_eq!(
        detail_payload["comparison_runs"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
        2
    );
    Ok(())
}

#[tokio::test]
async fn bookmarks_and_source_annotations_endpoints_work() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let run = ctx
        .state
        .db
        .create_run("NVDA", "Bookmark and annotation test")
        .await?;
    let iteration = ctx.state.db.create_iteration(&run.id, 1).await?;
    let source = ctx
        .state
        .db
        .insert_source(
            &run.id,
            Some(&iteration.id),
            "https://example.com/nvda",
            Some("NVDA source"),
            Some("example.com"),
            Some("Test excerpt"),
            Some(9.0),
            Some("news"),
        )
        .await?;
    let run_id = run.id.clone();
    let source_id = source.id.clone();

    let create_bookmark_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/bookmarks")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "entity_type": "run",
                    "entity_id": run_id,
                    "title": "NVDA run",
                    "note": "important",
                    "target_path": format!("/runs/{}", run.id)
                }))?))?,
        )
        .await?;
    assert_eq!(create_bookmark_response.status(), StatusCode::OK);

    let list_bookmarks_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/bookmarks")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_bookmarks_response.status(), StatusCode::OK);
    let list_bookmarks_body = list_bookmarks_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let bookmarks_payload: Value = serde_json::from_slice(&list_bookmarks_body)?;
    assert_eq!(
        bookmarks_payload
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        1
    );

    let create_annotation_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sources/{source_id}/annotations"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "run_id": run.id.clone(),
                    "selected_text": "Revenue accelerated 28%",
                    "annotation_markdown": "Strong acceleration heading into Q2",
                    "tag": "growth"
                }))?))?,
        )
        .await?;
    assert_eq!(create_annotation_response.status(), StatusCode::OK);
    let create_annotation_body = create_annotation_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let annotation_payload: Value = serde_json::from_slice(&create_annotation_body)?;
    let annotation_id = annotation_payload["id"].as_str().expect("annotation id");

    let list_annotations_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/sources/{source_id}/annotations"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_annotations_response.status(), StatusCode::OK);
    let list_annotations_body = list_annotations_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let annotations_payload: Value = serde_json::from_slice(&list_annotations_body)?;
    assert_eq!(
        annotations_payload
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        1
    );

    let delete_annotation_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/sources/{}/annotations?annotation_id={}",
                    source_id, annotation_id
                ))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(delete_annotation_response.status(), StatusCode::OK);

    let delete_bookmark_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/bookmarks?entity_type=run&entity_id={}",
                    run.id
                ))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(delete_bookmark_response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn run_template_crud_and_run_creation_work() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let create_template_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/run-templates")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Template A",
                    "question_template": "What are the key risks for {ticker}?",
                    "description": "risk template"
                }))?))?,
        )
        .await?;
    assert_eq!(create_template_response.status(), StatusCode::OK);
    let create_template_body = create_template_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let template_payload: Value = serde_json::from_slice(&create_template_body)?;
    let template_id = template_payload["id"].as_str().expect("template id");

    let create_run_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "ticker": "NVDA",
                    "question": null,
                    "template_id": template_id
                }))?))?,
        )
        .await?;
    assert_eq!(create_run_response.status(), StatusCode::OK);
    let create_run_body = create_run_response.into_body().collect().await?.to_bytes();
    let run_payload: Value = serde_json::from_slice(&create_run_body)?;
    let run_id = run_payload["run_id"].as_str().expect("run id");
    let completed_run = wait_for_run_completion(&ctx.state, run_id).await?;
    assert!(completed_run.question.contains("NVDA"));
    assert!(completed_run.question.contains("key risks"));

    let update_template_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/run-templates/{template_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Template A Updated",
                    "question_template": "What catalysts matter most for {ticker}?",
                    "description": "updated"
                }))?))?,
        )
        .await?;
    assert_eq!(update_template_response.status(), StatusCode::OK);

    let list_templates_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/run-templates")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_templates_response.status(), StatusCode::OK);
    let list_templates_body = list_templates_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let templates_payload: Value = serde_json::from_slice(&list_templates_body)?;
    assert_eq!(
        templates_payload
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        1
    );

    let delete_template_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/run-templates/{template_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(delete_template_response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn batch_job_endpoint_creates_runs_and_reaches_terminal_state() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let create_batch_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/batches")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Tech Batch",
                    "tickers": ["AAPL", "MSFT"],
                    "question_template": "What matters most for {ticker} in the next 12 months?",
                    "template_id": null
                }))?))?,
        )
        .await?;
    assert_eq!(create_batch_response.status(), StatusCode::OK);
    let create_batch_body = create_batch_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let batch_payload: Value = serde_json::from_slice(&create_batch_body)?;
    let batch_job_id = batch_payload["batch_job_id"]
        .as_str()
        .expect("batch job id");

    let batch_job = wait_for_batch_job_terminal(&ctx.state, batch_job_id).await?;
    assert_eq!(batch_job.status, "completed");
    assert!(batch_job.summary.is_some());

    let get_batch_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/batches/{batch_job_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(get_batch_response.status(), StatusCode::OK);
    let get_batch_body = get_batch_response.into_body().collect().await?.to_bytes();
    let get_batch_payload: Value = serde_json::from_slice(&get_batch_body)?;
    assert_eq!(
        get_batch_payload["batch_job_runs"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn run_cancel_and_retry_controls_work() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let run = ctx
        .state
        .db
        .create_run("NVDA", "Cancel and retry test")
        .await?;

    let cancel_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/runs/{}/cancel", run.id))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(cancel_response.status(), StatusCode::OK);
    let cancelled_run = ctx
        .state
        .db
        .get_run(&run.id)
        .await?
        .expect("cancelled run should exist");
    assert_eq!(cancelled_run.status, "cancelled");

    let retry_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/runs/{}/retry", run.id))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(retry_response.status(), StatusCode::OK);
    let retried_run = wait_for_run_completion(&ctx.state, &run.id).await?;
    assert_eq!(retried_run.status, "completed");
    Ok(())
}

#[tokio::test]
async fn watchlist_crud_endpoints_work() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let create_watchlist_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/watchlists")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Core Watchlist",
                    "tickers": ["NVDA", "AAPL"]
                }))?))?,
        )
        .await?;
    assert_eq!(create_watchlist_response.status(), StatusCode::OK);
    let create_watchlist_body = create_watchlist_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let watchlist_payload: Value = serde_json::from_slice(&create_watchlist_body)?;
    let watchlist_id = watchlist_payload["watchlist"]["id"]
        .as_str()
        .expect("watchlist id")
        .to_string();
    assert_eq!(
        watchlist_payload["tickers"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        2
    );

    let add_ticker_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/watchlists/{watchlist_id}/tickers"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "ticker": "MSFT"
                }))?))?,
        )
        .await?;
    assert_eq!(add_ticker_response.status(), StatusCode::OK);
    let add_ticker_body = add_ticker_response.into_body().collect().await?.to_bytes();
    let add_ticker_payload: Value = serde_json::from_slice(&add_ticker_body)?;
    assert_eq!(
        add_ticker_payload["tickers"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        3
    );

    let remove_ticker_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/watchlists/{watchlist_id}/tickers/MSFT"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(remove_ticker_response.status(), StatusCode::OK);
    let remove_ticker_body = remove_ticker_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let remove_ticker_payload: Value = serde_json::from_slice(&remove_ticker_body)?;
    assert_eq!(
        remove_ticker_payload["tickers"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        2
    );

    let update_watchlist_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/watchlists/{watchlist_id}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Core Watchlist Updated",
                    "tickers": ["TSLA", "META"]
                }))?))?,
        )
        .await?;
    assert_eq!(update_watchlist_response.status(), StatusCode::OK);
    let update_watchlist_body = update_watchlist_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let update_watchlist_payload: Value = serde_json::from_slice(&update_watchlist_body)?;
    assert_eq!(
        update_watchlist_payload["watchlist"]["name"]
            .as_str()
            .unwrap_or_default(),
        "Core Watchlist Updated"
    );
    assert_eq!(
        update_watchlist_payload["tickers"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        2
    );

    let list_watchlists_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/watchlists")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_watchlists_response.status(), StatusCode::OK);
    let list_watchlists_body = list_watchlists_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let list_watchlists_payload: Value = serde_json::from_slice(&list_watchlists_body)?;
    assert_eq!(
        list_watchlists_payload
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        1
    );

    let delete_watchlist_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/watchlists/{watchlist_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(delete_watchlist_response.status(), StatusCode::OK);

    let get_deleted_watchlist_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/watchlists/{watchlist_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        get_deleted_watchlist_response.status(),
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test]
async fn dashboard_payload_and_refresh_action_work() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let create_watchlist_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/watchlists")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Dashboard Watchlist",
                    "tickers": ["NVDA"]
                }))?))?,
        )
        .await?;
    assert_eq!(create_watchlist_response.status(), StatusCode::OK);
    let create_watchlist_body = create_watchlist_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let watchlist_payload: Value = serde_json::from_slice(&create_watchlist_body)?;
    let watchlist_id = watchlist_payload["watchlist"]["id"]
        .as_str()
        .expect("watchlist id")
        .to_string();

    let create_template_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/run-templates")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Dashboard Template",
                    "question_template": "What changed most for {ticker}?",
                    "description": null
                }))?))?,
        )
        .await?;
    assert_eq!(create_template_response.status(), StatusCode::OK);
    let create_template_body = create_template_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let template_payload: Value = serde_json::from_slice(&create_template_body)?;
    let template_id = template_payload["id"].as_str().expect("template id");

    let initial_dashboard_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/dashboard?watchlist_id={watchlist_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(initial_dashboard_response.status(), StatusCode::OK);
    let initial_dashboard_body = initial_dashboard_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let initial_dashboard_payload: Value = serde_json::from_slice(&initial_dashboard_body)?;
    assert_eq!(
        initial_dashboard_payload["rows"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        initial_dashboard_payload["rows"][0]["latest_status"]
            .as_str()
            .unwrap_or_default(),
        "no_data"
    );

    let refresh_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/dashboard/refresh")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "watchlist_id": watchlist_id,
                    "ticker": "NVDA",
                    "template_id": template_id,
                    "question": null
                }))?))?,
        )
        .await?;
    assert_eq!(refresh_response.status(), StatusCode::OK);
    let refresh_body = refresh_response.into_body().collect().await?.to_bytes();
    let refresh_payload: Value = serde_json::from_slice(&refresh_body)?;
    let run_id = refresh_payload["run_id"].as_str().expect("run id");
    let completed_run = wait_for_run_completion(&ctx.state, run_id).await?;
    assert_eq!(completed_run.status, "completed");

    let dashboard_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/dashboard?watchlist_id={watchlist_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(dashboard_response.status(), StatusCode::OK);
    let dashboard_body = dashboard_response.into_body().collect().await?.to_bytes();
    let dashboard_payload: Value = serde_json::from_slice(&dashboard_body)?;
    assert_eq!(
        dashboard_payload["rows"][0]["latest_status"]
            .as_str()
            .unwrap_or_default(),
        "completed"
    );
    assert!(dashboard_payload["rows"][0]["latest_score"].is_number());
    assert_eq!(
        dashboard_payload["rows"][0]["latest_run_id"]
            .as_str()
            .unwrap_or_default(),
        run_id
    );
    Ok(())
}

#[tokio::test]
async fn alerts_endpoints_work_and_dashboard_exposes_active_alerts() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let create_watchlist_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/watchlists")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({
                    "name": "Alert Watchlist",
                    "tickers": ["NVDA"]
                }))?))?,
        )
        .await?;
    assert_eq!(create_watchlist_response.status(), StatusCode::OK);
    let create_watchlist_body = create_watchlist_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let watchlist_payload: Value = serde_json::from_slice(&create_watchlist_body)?;
    let watchlist_id = watchlist_payload["watchlist"]["id"]
        .as_str()
        .expect("watchlist id")
        .to_string();

    let run_one = ctx.state.db.create_run("NVDA", "Initial thesis").await?;
    let iteration_one = ctx.state.db.create_iteration(&run_one.id, 1).await?;
    ctx.state
        .db
        .update_iteration_evaluation(&iteration_one.id, r#"{"score":8.4}"#)
        .await?;
    ctx.state
        .db
        .set_iteration_status(&iteration_one.id, "completed")
        .await?;
    ctx.state
        .db
        .finalize_run(
            &run_one.id,
            1,
            "# Executive Summary\nInitial",
            "<h1>Executive Summary</h1><p>Initial</p>",
            Some("Initial"),
        )
        .await?;

    let run_two = ctx.state.db.create_run("NVDA", "Updated thesis").await?;
    let iteration_two = ctx.state.db.create_iteration(&run_two.id, 1).await?;
    ctx.state
        .db
        .update_iteration_evaluation(&iteration_two.id, r#"{"score":5.0}"#)
        .await?;
    ctx.state
        .db
        .set_iteration_status(&iteration_two.id, "completed")
        .await?;
    ctx.state
        .db
        .finalize_run(
            &run_two.id,
            1,
            "# Executive Summary\nUpdated",
            "<h1>Executive Summary</h1><p>Updated</p>",
            Some("Updated"),
        )
        .await?;

    let dashboard_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/dashboard?watchlist_id={watchlist_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(dashboard_response.status(), StatusCode::OK);
    let dashboard_body = dashboard_response.into_body().collect().await?.to_bytes();
    let dashboard_payload: Value = serde_json::from_slice(&dashboard_body)?;
    let active_alerts = dashboard_payload["active_alerts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!active_alerts.is_empty());
    assert!(
        dashboard_payload["rows"][0]["active_alert_count"]
            .as_i64()
            .unwrap_or_default()
            >= 1
    );

    let alert_id = active_alerts[0]["id"]
        .as_str()
        .expect("alert id")
        .to_string();
    let dismiss_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/alerts/{alert_id}/dismiss"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(dismiss_response.status(), StatusCode::OK);

    let active_alerts_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/alerts?watchlist_id={watchlist_id}&status=active"
                ))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(active_alerts_response.status(), StatusCode::OK);
    let active_alerts_body = active_alerts_response
        .into_body()
        .collect()
        .await?
        .to_bytes();
    let active_alerts_payload: Value = serde_json::from_slice(&active_alerts_body)?;
    assert!(
        active_alerts_payload
            .as_array()
            .map(Vec::len)
            .unwrap_or_default()
            < active_alerts.len()
    );

    let all_alerts_response = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/alerts?watchlist_id={watchlist_id}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(all_alerts_response.status(), StatusCode::OK);
    let all_alerts_body = all_alerts_response.into_body().collect().await?.to_bytes();
    let all_alerts_payload: Value = serde_json::from_slice(&all_alerts_body)?;
    let snooze_target = all_alerts_payload
        .as_array()
        .and_then(|alerts| {
            alerts
                .iter()
                .find(|alert| alert["status"].as_str().unwrap_or_default() == "active")
        })
        .and_then(|alert| alert["id"].as_str())
        .map(str::to_string);

    if let Some(snooze_alert_id) = snooze_target {
        let snooze_response = ctx
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/alerts/{snooze_alert_id}/snooze"))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(snooze_response.status(), StatusCode::OK);
    }

    Ok(())
}

#[tokio::test]
async fn scheduler_records_backoff_on_failure() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let watchlist = ctx.state.db.create_watchlist("Failing watchlist").await?;

    // Kick off an initial schedule; next_refresh_at will be interval hours out.
    ctx.state
        .db
        .update_watchlist_schedule(&watchlist.id, true, 2, None)
        .await?;

    // Record a failure and capture the new next_refresh_at the DB produces.
    let next_after_1 = ctx
        .state
        .db
        .record_watchlist_refresh_failure(&watchlist.id, 2, "test reason")
        .await?;
    let schedule1 = ctx
        .state
        .db
        .get_watchlist_schedule(&watchlist.id)
        .await?
        .expect("schedule");

    assert_eq!(schedule1.consecutive_failures, 1);
    assert_eq!(
        schedule1.last_failure_reason.as_deref(),
        Some("test reason")
    );
    assert!(schedule1.last_failure_at.is_some());

    // First failure: backoff = interval * 2^1 = 4 hours. The stored
    // next_refresh_at should match what the method returned.
    assert_eq!(schedule1.next_refresh_at, Some(next_after_1));

    // Second failure should push the window further out than the first.
    let next_after_2 = ctx
        .state
        .db
        .record_watchlist_refresh_failure(&watchlist.id, 2, "still failing")
        .await?;
    let schedule2 = ctx
        .state
        .db
        .get_watchlist_schedule(&watchlist.id)
        .await?
        .expect("schedule");
    assert_eq!(schedule2.consecutive_failures, 2);
    assert!(next_after_2 > next_after_1);

    // A success clears the failure counters.
    ctx.state
        .db
        .record_watchlist_refresh_success(&watchlist.id, 2)
        .await?;
    let schedule3 = ctx
        .state
        .db
        .get_watchlist_schedule(&watchlist.id)
        .await?
        .expect("schedule");
    assert_eq!(schedule3.consecutive_failures, 0);
    assert!(schedule3.last_failure_at.is_none());
    assert!(schedule3.last_failure_reason.is_none());

    Ok(())
}

#[tokio::test]
async fn scheduler_reaps_stuck_scheduled_runs() -> Result<()> {
    let ctx = TestContext::new(1, false).await?;

    let watchlist = ctx.state.db.create_watchlist("Reaper test").await?;
    ctx.state
        .db
        .replace_watchlist_tickers(&watchlist.id, &["NVDA".to_string()])
        .await?;

    // Create a real run, drive it to completion, but leave the corresponding
    // scheduled_run row stuck in `running`.
    let run = ctx.state.db.create_run("NVDA", "stuck test").await?;
    let scheduled = ctx
        .state
        .db
        .create_scheduled_run(&watchlist.id, "NVDA", &run.id)
        .await?;
    ctx.state
        .db
        .update_scheduled_run_started(&scheduled.id)
        .await?;

    // Mark the run itself as completed (simulating an orchestrator task that
    // finished before the scheduler could record completion, e.g. a crash).
    ctx.state.db.set_run_status(&run.id, "completed").await?;

    let before = ctx
        .state
        .db
        .get_pending_scheduled_run_for_ticker(&watchlist.id, "NVDA")
        .await?;
    assert!(
        before.is_some(),
        "scheduled_run should be in pending/running before reap"
    );

    let reaped = ctx.state.db.reap_stuck_scheduled_runs().await?;
    assert_eq!(reaped, 1);

    let after = ctx
        .state
        .db
        .get_pending_scheduled_run_for_ticker(&watchlist.id, "NVDA")
        .await?;
    assert!(
        after.is_none(),
        "scheduled_run should no longer be pending/running after reap"
    );

    let runs = ctx.state.db.list_scheduled_runs(&watchlist.id, 5).await?;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "completed");

    Ok(())
}

struct TestContext {
    _temp_dir: TempDir,
    state: AppState,
    app: axum::Router,
}

impl TestContext {
    async fn new(max_iterations: u32, include_failure: bool) -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let database_path = temp_dir.path().join("autothesis-test.db");
        let database_url = format!("sqlite://{}", database_path.display());
        let config = Config {
            host: "127.0.0.1".to_string(),
            port: 3000,
            database_url: database_url.clone(),
            openai_api_key: "test-openai-key".to_string(),
            openai_model: "test-model".to_string(),
            openai_base_url: "https://example.com".to_string(),
            search_api_key: "test-search-key".to_string(),
            search_provider: "mock".to_string(),
            max_iterations,
            max_sources_per_iteration: 4,
            max_concurrent_runs: 5,
            scheduler_enabled: false,
            scheduler_check_interval_secs: 60,
            scheduler_max_concurrent_runs: 3,
            scheduler_min_ticker_age_hours: 24,
        };

        let database = Database::connect(&database_url).await?;
        let prompts = PromptStore::load_default()?;
        let price_provider = PriceProvider::new()?;
        let state = AppState::new(
            config,
            database,
            Arc::new(MockLlmProvider),
            Arc::new(MockSearchProvider::new(include_failure)),
            Arc::new(MockFetcher::new(include_failure)),
            prompts,
            price_provider,
        );
        let app = build_app(state.clone());

        Ok(Self {
            _temp_dir: temp_dir,
            state,
            app,
        })
    }
}

#[derive(Default)]
struct MockLlmProvider;

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn complete_json(
        &self,
        prompt_name: &str,
        _system_prompt: &str,
        user_prompt: &str,
    ) -> Result<Value> {
        let payload: Value = serde_json::from_str(user_prompt)?;
        match prompt_name {
            "planner" => {
                let ticker = payload["ticker"].as_str().unwrap_or("TICKER");
                Ok(serde_json::to_value(PlannerOutput {
                    research_goal: format!("Evaluate the bull and bear case for {ticker}."),
                    subquestions: vec![
                        "What are the main revenue drivers?".to_string(),
                        "What is the strongest bear argument?".to_string(),
                    ],
                    evidence_needed: vec![
                        "Latest earnings release".to_string(),
                        "Investor presentation".to_string(),
                    ],
                    priority_order: vec![
                        "Primary sources".to_string(),
                        "Counterarguments".to_string(),
                    ],
                })?)
            }
            "search_query_writer" => {
                let ticker = payload["ticker"].as_str().unwrap_or("TICKER");
                let iteration = payload["iteration_number"].as_i64().unwrap_or(1);
                Ok(serde_json::to_value(SearchQueryOutput {
                    queries: vec![
                        format!("{ticker} iteration {iteration} earnings release"),
                        format!("{ticker} iteration {iteration} investor presentation"),
                    ],
                })?)
            }
            "reader" => {
                let notes_json = payload["sources"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .map(|source| {
                        json!({
                            "source_id": source["source_id"],
                            "note_markdown": format!(
                                "- Fact: Evidence from {}.
                        - Inference: The source suggests durable demand.",
                                source["url"].as_str().unwrap_or("unknown source")
                            ),
                            "claim_type": "fact"
                        })
                    })
                    .collect::<Vec<_>>();
                let notes = serde_json::from_value(Value::Array(notes_json))?;
                Ok(serde_json::to_value(ReaderOutput { notes })?)
            }
            "evaluator" => {
                let current = payload["current_draft"].as_str().unwrap_or_default();
                let score = if current.contains("Iteration 3") {
                    8.8
                } else {
                    7.2
                };
                Ok(json!({
                    "improved": true,
                    "score": score,
                    "rubric": {
                        "evidence_coverage": score,
                        "source_quality": score,
                        "balance": score,
                        "specificity": score,
                        "decision_usefulness": score
                    },
                    "reasoning": "The newer draft adds more evidence and balance.",
                    "continue": true
                }))
            }
            other => Err(anyhow!("unexpected json prompt: {other}")),
        }
    }

    async fn complete_markdown(
        &self,
        prompt_name: &str,
        _system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let payload: Value = serde_json::from_str(user_prompt)?;
        match prompt_name {
            "synthesizer" => {
                let ticker = payload["ticker"].as_str().unwrap_or("TICKER");
                let question = payload["question"].as_str().unwrap_or("Question");
                let iteration = payload["iteration_number"].as_i64().unwrap_or(1);
                let sources = payload["sources"].as_array().cloned().unwrap_or_default();
                let first_source = sources
                    .first()
                    .and_then(|source| source["id"].as_str())
                    .unwrap_or("source-1");
                let source_lines = sources
                    .iter()
                    .map(|source| {
                        format!(
                            "- {} — {}",
                            source["id"].as_str().unwrap_or("source"),
                            source["url"].as_str().unwrap_or("https://example.com")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    );
                Ok(format!(
                    "Research date: {}
Ticker: {}
Question: {}
Disclaimer: This is research support and not financial advice.

# Executive Summary

Fact: Iteration {} consolidates the latest evidence. [source: {}]

## Business Overview

Fact: The company operates a scaled business supported by primary sources. [source: {}]

## Bull Case

Inference: Demand durability and product strength support upside. [source: {}]

## Bear Case

Inference: Valuation and execution risk remain meaningful. [source: {}]

## Valuation Assumptions

Open question: The current valuation requires sustained growth and margin execution. [source: {}]

## Catalysts

Fact: Upcoming earnings and product cycles could change the thesis. [source: {}]

## Risks

Fact: Competition, macro softness, and missed guidance are key risks. [source: {}]

## Known Unknowns

Open question: The durability of the next twelve months of demand is still uncertain. [source: {}]

## What Would Change My Mind

Inference: A material slowdown in demand or weaker margins would reduce confidence. [source: {}]

## Sources

{}",
                    Utc::now().date_naive(),
                    ticker,
                    question,
                    iteration,
                    first_source,
                    first_source,
                    first_source,
                    first_source,
                    first_source,
                    first_source,
                    first_source,
                    first_source,
                    first_source,
                    source_lines,
                ))
            }
            "critic" => Ok("# Overall Assessment

The memo is directionally useful but can still add counterarguments.

## Unsupported Claims

- Tighten valuation support.

## Missing Counterarguments

- Add a sharper bear argument.

## Missing Source Coverage

- Add one more primary source.

## Follow-up Research Tasks

- Search for the latest shareholder letter.
- Search for the latest earnings transcript."
                .to_string()),
            other => Err(anyhow!("unexpected markdown prompt: {other}")),
        }
    }
}

struct MockSearchProvider {
    counter: AtomicUsize,
    include_failure: bool,
}

impl MockSearchProvider {
    fn new(include_failure: bool) -> Self {
        Self {
            counter: AtomicUsize::new(0),
            include_failure,
        }
    }
}

#[async_trait]
impl SearchProvider for MockSearchProvider {
    async fn search(&self, query: &str, _max_results: usize) -> Result<Vec<SearchResultItem>> {
        let call = self.counter.fetch_add(1, Ordering::SeqCst);
        let base = format!("https://example.com/{call}");
        let maybe_fail = if self.include_failure && call == 0 {
            "https://example.com/fail-source".to_string()
        } else {
            format!("{base}/c")
        };
        Ok(vec![
            SearchResultItem {
                title: Some(format!("{} primary", query)),
                url: format!("{base}/a"),
                snippet: Some("Primary source evidence".to_string()),
                score: Some(9.5),
                source_type: Some("ir".to_string()),
                published_at: None,
            },
            SearchResultItem {
                title: Some(format!("{} transcript", query)),
                url: format!("{base}/b"),
                snippet: Some("Transcript evidence".to_string()),
                score: Some(9.0),
                source_type: Some("transcript".to_string()),
                published_at: None,
            },
            SearchResultItem {
                title: Some(format!("{} media", query)),
                url: maybe_fail,
                snippet: Some("Media evidence".to_string()),
                score: Some(20.0),
                source_type: Some("sec".to_string()),
                published_at: None,
            },
        ])
    }
}

struct MockFetcher {
    include_failure: bool,
}

impl MockFetcher {
    fn new(include_failure: bool) -> Self {
        Self { include_failure }
    }
}

#[async_trait]
impl WebFetcher for MockFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        if self.include_failure && url.contains("fail-source") {
            return Err(anyhow!("simulated fetch failure"));
        }
        Ok(FetchedPage {
            url: url.to_string(),
            title: Some(format!("Title for {url}")),
            domain: Some("example.com".to_string()),
            text: format!(
                "Evidence captured from {} at {}. Revenue drivers, risks, and catalysts were discussed.",
                url,
                Utc::now()
            ),
        })
    }
}

async fn wait_for_run_completion(
    state: &AppState,
    run_id: &str,
) -> Result<autothesis::models::Run> {
    for _ in 0..100 {
        let run = state
            .db
            .get_run(run_id)
            .await?
            .expect("run exists while polling");
        if run.status == "completed" {
            return Ok(run);
        }
        if run.status == "failed" {
            return Err(anyhow!("run failed during test"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!("timed out waiting for run completion"))
}

async fn wait_for_comparison_terminal(
    state: &AppState,
    comparison_id: &str,
) -> Result<autothesis::models::Comparison> {
    for _ in 0..100 {
        let comparison = state
            .db
            .get_comparison(comparison_id)
            .await?
            .expect("comparison exists while polling");
        if comparison.status == "completed"
            || comparison.status == "failed"
            || comparison.status == "failed_partial"
        {
            return Ok(comparison);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!("timed out waiting for comparison completion"))
}

async fn wait_for_batch_job_terminal(
    state: &AppState,
    batch_job_id: &str,
) -> Result<autothesis::models::BatchJob> {
    for _ in 0..100 {
        let batch_job = state
            .db
            .get_batch_job(batch_job_id)
            .await?
            .expect("batch job exists while polling");
        if batch_job.status == "completed"
            || batch_job.status == "failed"
            || batch_job.status == "failed_partial"
        {
            return Ok(batch_job);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!("timed out waiting for batch completion"))
}
