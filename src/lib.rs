//! # a3s-search
//!
//! An embeddable meta search engine library inspired by SearXNG.
//!
//! This library provides a framework for aggregating search results from multiple
//! search engines, with support for:
//!
//! - Async parallel search execution
//! - Result deduplication and merging
//! - Configurable ranking algorithms
//! - Extensible engine interface
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
mod config;
mod engine;
mod error;
mod fetcher;
mod fetcher_http;
mod health;
mod html_engine;
pub mod proxy;
mod query;
mod result;
mod search;

pub mod engines;

#[cfg(feature = "obscura")]
pub mod browser_obscura;

#[cfg(feature = "obscura")]
pub mod browser_setup_obs;

pub use aggregator::Aggregator;
pub use config::{EngineEntry, HealthEntry, SearchConfig};
pub use engine::{Engine, EngineCategory, EngineConfig};
pub use error::{Result, SearchError};
pub use fetcher::{PageFetcher, WaitStrategy};
pub use fetcher_http::{
    clear_search_http_metrics_callback, set_search_http_metrics_callback, HttpFetcher,
    PooledHttpFetcher, SearchHttpMetricsCallback, SearchHttpMetricsRecord,
};
pub use health::{HealthConfig, HealthMonitor};
pub use html_engine::{selector, HtmlEngine, HtmlParser};
pub use query::{SafeSearch, SearchQuery, TimeRange};
pub use result::{ResultType, SearchResult, SearchResults};
pub use search::Search;

#[cfg(feature = "obscura")]
pub use browser_obscura::{ObscuraFetcher, ObscuraPool, ObscuraPoolConfig};

#[cfg(feature = "obscura")]
pub use browser_setup_obs::{DownloadPhase, DownloadProgress, DownloadProgressCallback};
