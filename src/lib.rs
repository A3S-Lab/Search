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

mod engine;
mod error;
mod query;
mod result;
mod aggregator;
mod search;

pub mod engines;

pub use engine::{Engine, EngineConfig, EngineCategory};
pub use error::{SearchError, Result};
pub use query::SearchQuery;
pub use result::{SearchResult, SearchResults, ResultType};
pub use aggregator::Aggregator;
pub use search::Search;
