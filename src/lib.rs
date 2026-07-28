//! # a3s-search
//!
//! An extensible web search library with conventional engines and native
//! third-party provider APIs.
//!
//! This library provides a framework for aggregating search results from multiple
//! search engines, with support for:
//!
//! - Async parallel search execution
//! - Result deduplication and merging
//! - Configurable ranking algorithms
//! - Extensible engine interface
//! - Provider-neutral [`SearchProvider`](providers::SearchProvider) extensions
//! - Native AnySearch and Tavily integrations
//! - Rich answers, full text, images, relevance, usage, and request reports
//! - Dynamic proxy IP pool for anti-crawler protection
//!
//! ## Example
//!
//! ```rust,no_run
//! use a3s_search::{Search, SearchQuery, engines::DuckDuckGo};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut search = Search::new();
//!     search.add_engine(DuckDuckGo::new());
//!
//!     let query = SearchQuery::new("rust programming");
//!     let results = search.search(query).await?;
//!
//!     for result in results.items() {
//!         println!("{}: {}", result.title, result.url);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Using Proxy
//!
//! ```rust,no_run
//! use a3s_search::{Search, SearchQuery, engines::DuckDuckGo, HttpFetcher, PageFetcher};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let fetcher: Arc<dyn PageFetcher> = Arc::new(
//!         HttpFetcher::with_proxy("http://proxy1.example.com:8080")?
//!     );
//!
//!     let mut search = Search::new();
//!     search.add_engine(DuckDuckGo::with_fetcher(
//!         a3s_search::engines::DuckDuckGoParser, fetcher,
//!     ));
//!
//!     let query = SearchQuery::new("rust programming");
//!     let results = search.search(query).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Using Dynamic Proxy Pool
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use a3s_search::{Search, SearchQuery, PooledHttpFetcher, PageFetcher};
//! use a3s_search::engines::{DuckDuckGo, DuckDuckGoParser};
//! use a3s_search::proxy::{ProxyPool, ProxyProvider, ProxyConfig, spawn_auto_refresh};
//!
//! // Implement your own provider to fetch proxies from any source
//! struct MyProxyProvider { /* ... */ }
//!
//! #[async_trait::async_trait]
//! impl ProxyProvider for MyProxyProvider {
//!     async fn fetch_proxies(&self) -> a3s_search::Result<Vec<ProxyConfig>> {
//!         // Fetch from your proxy API, database, etc.
//!         Ok(vec![ProxyConfig::new("10.0.0.1", 8080)])
//!     }
//!     fn refresh_interval(&self) -> std::time::Duration {
//!         std::time::Duration::from_secs(60) // refresh every minute
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let pool = Arc::new(ProxyPool::with_provider(MyProxyProvider { /* ... */ }));
//!     let _refresh_handle = spawn_auto_refresh(Arc::clone(&pool));
//!
//!     let fetcher: Arc<dyn PageFetcher> = Arc::new(PooledHttpFetcher::new(Arc::clone(&pool)));
//!
//!     let mut search = Search::new();
//!     search.add_engine(DuckDuckGo::with_fetcher(DuckDuckGoParser, fetcher));
//!
//!     let query = SearchQuery::new("rust programming");
//!     let results = search.search(query).await?;
//!     Ok(())
//! }
//! ```

mod aggregator;
mod bulkhead;
mod circuit;
mod coalescer;
mod config;
mod engine;
mod enrich;
mod error;
mod extract;
mod fetcher;
mod fetcher_http;
mod health;
mod html_engine;
pub mod metrics;
pub mod proxy;
mod quality;
mod query;
mod result;
mod retry_budget;
mod search;

pub mod engines;
pub mod providers;

#[cfg(feature = "headless")]
pub mod browser;

#[cfg(feature = "headless")]
pub use a3s_use_browser;

pub use aggregator::Aggregator;
pub use bulkhead::{
    Bulkhead, BulkheadConfig, BulkheadPermit, BulkheadRejection, BulkheadRejectionKind,
    BulkheadSnapshot,
};
pub use circuit::{
    CircuitBreaker, CircuitBreakerConfig, CircuitOpen, CircuitPermit, CircuitSnapshot,
    CircuitState, CircuitWindowConfig,
};
pub use coalescer::{SearchCoalescer, SearchCoalescerConfig, SearchCoalescerSnapshot};
pub use config::{EngineEntry, HealthEntry, ProviderEntry, ProviderSettings, SearchConfig};
pub use engine::{Engine, EngineCategory, EngineConfig, EngineOutput};
pub use enrich::enrich_full_text;
pub use error::{ProviderError, ProviderErrorKind, Result, SearchError};
pub use extract::extract_main_text;
pub use fetcher::{PageFetcher, WaitStrategy};
pub use fetcher_http::{HttpFetcher, PooledHttpFetcher};
pub use health::{HealthConfig, HealthMonitor};
pub use html_engine::{selector, HtmlEngine, HtmlParser};
pub use metrics::{Metrics, MetricsSnapshot, TimingGuard};
pub use quality::{
    query_match_score, SearchCascade, SearchQuality, SearchQualityFloor, SearchTierDecision,
    SearchTierReport,
};
pub use query::{SafeSearch, SearchQuery, TimeRange};
pub use result::{
    EngineFailure, EngineOutcome, EngineOutcomeKind, ResultType, SearchImage, SearchReport,
    SearchResult, SearchResults, SearchUsage,
};
pub use retry_budget::{RetryBudget, RetryBudgetConfig, RetryBudgetSnapshot};
pub use search::Search;

#[cfg(feature = "headless")]
pub use browser::{
    BrowserBackend, BrowserFetcher, BrowserPool, BrowserPoolConfig, BrowserProvider,
};
