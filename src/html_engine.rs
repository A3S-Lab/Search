//! Shared base for HTML-scraping search engines.
//!
//! Most search engines follow the same pattern: build a URL, fetch HTML,
//! parse results with CSS selectors. This module eliminates that boilerplate
//! by providing a generic `HtmlEngine<P>` that delegates only the
//! engine-specific parts (URL building and HTML parsing) to a `HtmlParser` trait.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::Selector;

use crate::fetcher::PageFetcher;
use crate::{Engine, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Parse a CSS selector string, returning a `SearchError::Parse` on failure.
pub fn selector(css: &str) -> Result<Selector> {
    Selector::parse(css)
        .map_err(|e| SearchError::Parse(format!("Invalid CSS selector '{}': {:?}", css, e)))
}

/// Engine-specific logic for HTML-scraping search engines.
///
/// Implement this trait to define how a search URL is built and how
/// the returned HTML is parsed into results. All boilerplate (config,
/// fetcher, `Engine` trait impl) is handled by `HtmlEngine<P>`.
pub trait HtmlParser: Send + Sync {
    /// Returns the default `EngineConfig` for this engine.
    fn default_config() -> EngineConfig;

    /// Builds the search URL from the query.
    fn build_url(&self, query: &SearchQuery) -> String;

    /// Validates the fetched HTML before parsing.
    ///
    /// Override this to detect error pages (e.g., CAPTCHAs, bot blocks).
    /// Returns `Ok(())` if the HTML is valid, or an error to abort parsing.
    fn validate(&self, _html: &str) -> Result<()> {
        Ok(())
    }

    /// Parses the fetched HTML into search results.
    fn parse(&self, html: &str) -> Result<Vec<SearchResult>>;
}

/// Generic base for all HTML-scraping search engines.
///
/// Combines an `EngineConfig`, a `PageFetcher`, and a `HtmlParser`
/// implementation. The `Engine` trait is automatically implemented.
pub struct HtmlEngine<P: HtmlParser> {
    config: EngineConfig,
    fetcher: Arc<dyn PageFetcher>,
    pub(crate) parser: P,
}

impl<P: HtmlParser> HtmlEngine<P> {
    /// Creates a new engine with a custom page fetcher.
    pub fn with_fetcher(parser: P, fetcher: Arc<dyn PageFetcher>) -> Self {
        Self {
            config: P::default_config(),
            fetcher,
            parser,
        }
    }

    /// Overrides the engine configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }
}

#[async_trait]
impl<P: HtmlParser> Engine for HtmlEngine<P> {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = self.parser.build_url(query);
        let html = self.fetcher.fetch(&url).await?;
        self.parser.validate(&html)?;
        self.parser.parse(&html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_valid() {
        let sel = selector("div.g");
        assert!(sel.is_ok());
    }

    #[test]
    fn test_selector_complex() {
        let sel = selector("div.snippet[data-type=\"web\"]");
        assert!(sel.is_ok());
    }

    #[test]
    fn test_selector_invalid() {
        let sel = selector("[[[invalid");
        assert!(sel.is_err());
        let err = sel.unwrap_err().to_string();
        assert!(err.contains("Invalid CSS selector"));
    }
}
