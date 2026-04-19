use crate::{
    cancellation::CancellationRegistry,
    config::Config,
    db::Database,
    providers::{
        fetch::{ReqwestWebFetcher, WebFetcher},
        llm::LlmProvider,
        openai::OpenAiProvider,
        price::PriceProvider,
        search::SearchProvider,
        tavily::TavilySearchProvider,
    },
};
use anyhow::{anyhow, Context, Result};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct PromptStore {
    prompts: Arc<HashMap<String, String>>,
}

impl PromptStore {
    pub fn load_default() -> Result<Self> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts");
        let mut prompts = HashMap::new();

        for name in [
            "planner",
            "search_query_writer",
            "reader",
            "synthesizer",
            "critic",
            "evaluator",
            "scanner_signal",
            "preliminary_thesis",
        ] {
            let path = base.join(format!("{name}.md"));
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read prompt file {}", path.display()))?;
            prompts.insert(name.to_string(), content);
        }

        Ok(Self {
            prompts: Arc::new(prompts),
        })
    }

    pub fn get(&self, name: &str) -> Result<&str> {
        self.prompts
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| anyhow!("missing prompt: {name}"))
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
    pub llm: Arc<dyn LlmProvider>,
    pub search: Arc<dyn SearchProvider>,
    pub fetcher: Arc<dyn WebFetcher>,
    pub prompts: PromptStore,
    pub price_provider: PriceProvider,
    pub cancellation: CancellationRegistry,
    /// Global cap on concurrently-executing orchestrator runs. Every route
    /// that spawns a background research run (manual create, retry, batch,
    /// comparison, scanner promotion, scheduler, watchlist refresh trigger,
    /// dashboard refresh) must acquire a permit from this semaphore before
    /// kicking off the orchestrator. See `crate::services::orchestrator::spawn_bounded_run`.
    pub run_semaphore: Arc<Semaphore>,
}

impl AppState {
    pub fn new(
        config: Config,
        db: Database,
        llm: Arc<dyn LlmProvider>,
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn WebFetcher>,
        prompts: PromptStore,
        price_provider: PriceProvider,
    ) -> Self {
        let run_semaphore = Arc::new(Semaphore::new(config.max_concurrent_runs));
        Self {
            config: Arc::new(config),
            db,
            llm,
            search,
            fetcher,
            prompts,
            price_provider,
            cancellation: CancellationRegistry::new(),
            run_semaphore,
        }
    }

    pub async fn from_config(config: Config) -> Result<Self> {
        let db = Database::connect(&config.database_url).await?;

        // Seed S&P 500 ticker universe if empty
        let seed_count = db.seed_sp500_universe().await?;
        if seed_count > 0 {
            tracing::info!(count = seed_count, "seeded S&P 500 ticker universe");
        }

        let prompts = PromptStore::load_default()?;

        if config.openai_api_key.trim().is_empty() {
            return Err(anyhow!("OPENAI_API_KEY is required"));
        }
        if config.search_api_key.trim().is_empty() {
            return Err(anyhow!("SEARCH_API_KEY is required"));
        }

        let llm = Arc::new(OpenAiProvider::new(
            config.openai_api_key.clone(),
            config.openai_model.clone(),
            config.openai_base_url.clone(),
        )?);

        let search: Arc<dyn SearchProvider> = match config.search_provider.as_str() {
            "tavily" => Arc::new(TavilySearchProvider::new(config.search_api_key.clone())?),
            other => return Err(anyhow!("unsupported SEARCH_PROVIDER: {other}")),
        };

        let fetcher = Arc::new(ReqwestWebFetcher::new()?);
        let price_provider = PriceProvider::new()?;

        Ok(Self::new(
            config,
            db,
            llm,
            search,
            fetcher,
            prompts,
            price_provider,
        ))
    }
}
