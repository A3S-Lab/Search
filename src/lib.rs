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
//! ## Using Proxy Pool
//!
//! ```rust,no_run
//! use a3s_search::{Search, SearchQuery, engines::DuckDuckGo};
//! use a3s_search::proxy::{ProxyPool, ProxyConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create a proxy pool with multiple proxies
//!     let proxy_pool = ProxyPool::with_proxies(vec![
//!         ProxyConfig::new("proxy1.example.com", 8080),
//!         ProxyConfig::new("proxy2.example.com", 8080),
//!     ]);
//!
//!     let mut search = Search::new();
//!     search.set_proxy_pool(proxy_pool);
//!     search.add_engine(DuckDuckGo::new());
//!
//!     let query = SearchQuery::new("rust programming");
//!     let results = search.search(query).await?;
//!
//!     Ok(())
//! }
//! ```

mod aggregator;
mod engine;
mod error;
mod fetcher;
mod fetcher_http;
pub mod proxy;
mod query;
mod result;
mod search;

pub mod engines;

#[cfg(feature = "headless")]
pub mod browser;

#[cfg(feature = "headless")]
pub mod browser_setup;

pub use aggregator::Aggregator;
pub use engine::{Engine, EngineCategory, EngineConfig};
pub use error::{Result, SearchError};
pub use fetcher::{PageFetcher, WaitStrategy};
pub use fetcher_http::HttpFetcher;
pub use query::SearchQuery;
pub use result::{ResultType, SearchResult, SearchResults};
pub use search::Search;

#[cfg(feature = "headless")]
pub use browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig};
