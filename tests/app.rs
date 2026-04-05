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
        };

        let database = Database::connect(&database_url).await?;
        let prompts = PromptStore::load_default()?;
        let state = AppState::new(
            config,
            database,
            Arc::new(MockLlmProvider),
            Arc::new(MockSearchProvider::new(include_failure)),
            Arc::new(MockFetcher::new(include_failure)),
            prompts,
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
