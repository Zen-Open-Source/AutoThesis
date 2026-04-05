# AutoThesis.finance

AutoThesis.finance is a local Rust web app for iterative stock research. Enter a ticker, start a run, and the app will search, extract evidence, critique its own memo, and publish a final research memo with sources and iteration history.

## Requirements

- Rust stable (`cargo`, `rustc`)
- An OpenAI API key
- A Tavily API key

## Setup

1. Copy the example environment file:

   ```bash
   cp .env.example .env
   ```

2. Add your API keys to `.env`:

   ```bash
   OPENAI_API_KEY=...
   SEARCH_API_KEY=...
   ```

3. Optionally adjust:
   - `OPENAI_MODEL`
   - `MAX_ITERATIONS`
   - `MAX_SOURCES_PER_ITERATION`
   - `APP_HOST`
   - `APP_PORT`

## Run locally

Start the app with:

```bash
cargo run
```

Then open:

```text
http://127.0.0.1:3000
```

Database migrations are applied automatically on startup.

## How to use

1. Open the homepage.
2. Enter a stock ticker such as `NVDA`, `AMZN`, `TSLA`, or `COST`.
3. Optionally customize the research question.
4. Click **Start Research**.
5. Watch the run progress through multiple iterations.
6. Open the completed memo and inspect:
   - executive summary
   - bull case
   - bear case
   - risks
   - known unknowns
   - sources
   - iteration history

## Example questions

- What is the current bull and bear case for `NVDA`?
- What would need to be true for the current valuation of `AMZN` to make sense?
- What are the most important risks and catalysts for `COST` over the next 12 to 24 months?

## Environment variables

See `.env.example` for the full list. The main ones are:

- `APP_HOST`
- `APP_PORT`
- `DATABASE_URL`
- `OPENAI_API_KEY`
- `OPENAI_MODEL`
- `OPENAI_BASE_URL`
- `SEARCH_API_KEY`
- `SEARCH_PROVIDER`
- `MAX_ITERATIONS`
- `MAX_SOURCES_PER_ITERATION`
- `RUST_LOG`

## Development checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
