//! Shared base for HTML-scraping search engines.
//!
//! Most search engines follow the same pattern: build a URL, fetch HTML,
//! parse results with CSS selectors. This module eliminates that boilerplate
//! by providing a generic `HtmlEngine<P>` that delegates only the
//! engine-specific parts (URL building and HTML parsing) to a `HtmlParser` trait.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::fetcher::PageFetcher;
use crate::{Engine, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Parse a CSS selector string, returning a `SearchError::Parse` on failure.
pub fn selector(css: &str) -> Result<Selector> {
    Selector::parse(css)
        .map_err(|e| SearchError::Parse(format!("Invalid CSS selector '{}': {:?}", css, e)))
}

/// Structural markers used to distinguish a real search response from an
/// HTTP-success challenge, interstitial, or unrelated page.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SearchResponseSpec {
    /// Human-readable engine name used in diagnostics.
    pub engine: &'static str,
    /// Selectors for result items that the parser understands.
    pub result_selectors: &'static [&'static str],
    /// Selectors for explicit empty states or an empty result container.
    pub empty_selectors: &'static [&'static str],
    /// Selectors for CAPTCHA, consent, verification, or other interruptions.
    pub challenge_selectors: &'static [&'static str],
}

/// Validates the protocol-level structure of an HTML search response.
///
/// Challenge markers take precedence because some providers render an
/// interstitial over a partially populated result document. A response is
/// accepted only when it contains a known result item or a legitimate empty
/// state; an unrelated 2xx page is reported as parser drift.
pub(crate) fn validate_search_response(html: &str, spec: SearchResponseSpec) -> Result<()> {
    let document = Html::parse_document(html);

    if matches_any(&document, spec.challenge_selectors)? {
        return Err(SearchError::Challenge(format!(
            "{} returned a CAPTCHA, challenge, or consent page instead of search results",
            spec.engine
        )));
    }

    if matches_any(&document, spec.result_selectors)?
        || matches_any(&document, spec.empty_selectors)?
    {
        return Ok(());
    }

    Err(SearchError::InvalidResponse(format!(
        "{} response did not contain recognized result or empty-state structure",
        spec.engine
    )))
}

fn matches_any(document: &Html, selectors: &[&str]) -> Result<bool> {
    for css in selectors {
        let selector = selector(css)?;
        if document.select(&selector).next().is_some() {
            return Ok(true);
        }
    }
    Ok(false)
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

    const TEST_SPEC: SearchResponseSpec = SearchResponseSpec {
        engine: "Test Search",
        result_selectors: &[".result"],
        empty_selectors: &[".no-results", "#results:empty"],
        challenge_selectors: &["#challenge-form"],
    };

    #[test]
    fn response_validation_accepts_results_and_explicit_empty_state() {
        assert!(validate_search_response(
            r#"<main><article class="result"></article></main>"#,
            TEST_SPEC,
        )
        .is_ok());
        assert!(
            validate_search_response(r#"<main><p class="no-results"></p></main>"#, TEST_SPEC,)
                .is_ok()
        );
    }

    #[test]
    fn response_validation_prioritizes_challenge_over_stale_results() {
        let error = validate_search_response(
            r#"<main><article class="result"></article><form id="challenge-form"></form></main>"#,
            TEST_SPEC,
        )
        .unwrap_err();

        assert_eq!(error.kind(), "challenge");
        assert!(error.is_transient());
    }

    #[test]
    fn response_validation_rejects_unrecognized_success_page() {
        let error = validate_search_response(
            r#"<html><body><main id="homepage"></main></body></html>"#,
            TEST_SPEC,
        )
        .unwrap_err();

        assert_eq!(error.kind(), "invalid_response");
        assert!(!error.is_transient());
    }
}
