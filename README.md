# AutoThesis.finance

AutoThesis.finance is a local Rust web app for iterative stock research. Enter a ticker, start a run, and the app will search, extract evidence, critique its own memo, and publish a final research memo with sources and iteration history.

The project is inspired by Andrej Karpathy's `autoresearch` pattern: instead of generating a memo in one pass, AutoThesis runs a closed loop of planning, searching, reading, writing, critiquing, and refining over multiple iterations.

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

## Features

### Single Stock Research

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

### Multi-Ticker Comparison

Compare multiple stocks side-by-side:

1. Navigate to the **Comparisons** page.
2. Enter a comparison name and select multiple tickers (e.g., `NVDA`, `AMD`, `INTC`).
3. Optionally customize the comparison question.
4. Click **Create Comparison**.
5. Watch each ticker run its own research iteration.
6. View the completed comparison with side-by-side memos.

### Dark Mode

AutoThesis supports both light and dark themes:

- The app defaults to your system preference (`prefers-color-scheme`).
- Click the sun/moon icon in the header to manually toggle themes.
- Your preference is saved in browser localStorage and persists across sessions.

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
