# AutoThesis.finance

AutoThesis is a Rust-first local web app that runs an iterative research loop for public equities. Enter a ticker, let the system search, extract evidence, critique its own memo, and publish a final investment memo with inspectable iterations and sources.

## What is implemented

- Rust `axum` web app with server-rendered HTML via `askama`
- SQLite persistence with SQL migrations
- Iterative research loop with planner, search, reader, synthesizer, critic, and evaluator stages
- OpenAI-backed LLM provider and Tavily-backed search provider
- Source fetching and text extraction via `reqwest` and `scraper`
- Inspectable runs, events, iterations, sources, evidence notes, and final memos
- Tests for DB persistence, API endpoints, and an end-to-end mocked happy path

## What is not implemented

- Brokerage or trading integrations
- Authentication, billing, or multi-user accounts
- Backtesting or portfolio optimization
- Advanced SEC parsing beyond normal web retrieval

## Prerequisites

- Rust stable (`cargo`, `rustc`)
- An OpenAI API key
- A search API key for Tavily

## Environment setup

1. Copy `.env.example` to `.env`.
2. Fill in:
   - `OPENAI_API_KEY`
   - `SEARCH_API_KEY`
3. Optionally adjust `MAX_ITERATIONS`, `MAX_SOURCES_PER_ITERATION`, or the model name.

## Local run

```bash
cargo run
```

Then open `http://127.0.0.1:3000`.

## Migrations

Migrations are applied automatically on startup from `sql/migrations`.

## Test commands

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## Sample tickers and questions

- `NVDA` — What is the current bull and bear case for NVDA, and what would need to be true for the valuation to make sense?
- `AMZN` — What are the most important risks and catalysts for AMZN over the next 12 to 24 months?
- `COST` — What is the current bull and bear case for COST?

## Architecture overview

- `src/routes`: JSON API and server-rendered pages
- `src/services`: planner/search/reader/synthesizer/critic/evaluator/orchestrator
- `src/providers`: OpenAI, Tavily, and page fetching integrations
- `src/db.rs`: persistence layer and query helpers
- `templates/`: Askama HTML templates
- `sql/migrations/`: SQLite schema
- `prompts/`: versioned prompt files used by the agent loop
