//! Bing China search engine implementation.
//!
//! Bing's normal HTML result page may redirect automated clients to the home page
//! or a CAPTCHA. The RSS endpoint returns the same public search results as stable,
//! server-rendered XML and therefore does not require a headless browser.

use super::bing::{build_bing_rss_url, parse_bing_response, validate_bing_response};
use crate::html_engine::{HtmlEngine, HtmlParser};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};

/// Bing China RSS/HTML response parser.
pub struct BingChinaParser;

/// Bing China search engine (必应中国).
pub type BingChina = HtmlEngine<BingChinaParser>;

impl BingChina {
    /// Creates a new Bing China engine with the given page fetcher.
    pub fn new(fetcher: std::sync::Arc<dyn crate::PageFetcher>) -> Self {
        HtmlEngine::with_fetcher(BingChinaParser, fetcher)
    }
}

impl HtmlParser for BingChinaParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Bing China".to_string(),
            shortcut: "bing_cn".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 10,
            enabled: true,
            paging: true,
            safesearch: true,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        build_bing_rss_url(query)
    }

    fn validate(&self, response: &str) -> Result<()> {
        validate_bing_response(response)
    }

    fn parse(&self, response: &str) -> Result<Vec<SearchResult>> {
        parse_bing_response(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetcher_http::HttpFetcher;
    use crate::Engine;
    use std::sync::Arc;

    fn make_bing_china() -> BingChina {
        BingChina::new(Arc::new(HttpFetcher::new()))
    }

    #[test]
    fn test_bing_china_new() {
        let engine = make_bing_china();
        assert_eq!(engine.config().name, "Bing China");
        assert_eq!(engine.config().shortcut, "bing_cn");
        assert_eq!(engine.config().categories, vec![EngineCategory::General]);
        assert_eq!(engine.config().weight, 1.0);
        assert_eq!(engine.config().timeout, 10);
        assert!(engine.config().enabled);
        assert!(engine.config().paging);
        assert!(engine.config().safesearch);
    }

    #[test]
    fn test_bing_china_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Bing".to_string(),
            shortcut: "cbing".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = make_bing_china().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom Bing");
        assert_eq!(engine.config().shortcut, "cbing");
        assert_eq!(engine.config().weight, 1.5);
    }

    #[test]
    fn test_bing_china_engine_trait() {
        let engine = make_bing_china();
        assert_eq!(engine.name(), "Bing China");
        assert_eq!(engine.shortcut(), "bing_cn");
        assert_eq!(engine.weight(), 1.0);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_parse_results_empty_html() {
        let parser = BingChinaParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_build_url_uses_rss_endpoint() {
        let parser = BingChinaParser;
        let url = parser.build_url(&SearchQuery::new("巴威 2020 台风"));
        assert!(url.starts_with("https://www.bing.com/search?"));
        assert!(url.contains("format=rss"));
        assert!(url.contains("%E5%B7%B4%E5%A8%81"));
    }

    #[test]
    fn test_parse_rss_results() {
        let parser = BingChinaParser;
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
            <rss version="2.0"><channel>
              <title>Bing: test</title>
              <item>
                <title>Typhoon &quot;Bavi&quot;</title>
                <link>https://example.com/bavi</link>
                <description>Landfall wind reached 35 m/s.</description>
                <pubDate>Thu, 27 Aug 2020 00:00:00 GMT</pubDate>
              </item>
            </channel></rss>"#;
        let results = parser.parse(xml).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Typhoon \"Bavi\"");
        assert_eq!(results[0].url, "https://example.com/bavi");
        assert_eq!(results[0].content, "Landfall wind reached 35 m/s.");
        assert_eq!(
            results[0].published_date.as_deref(),
            Some("Thu, 27 Aug 2020 00:00:00 GMT")
        );
    }

    #[test]
    fn test_rejects_bing_home_page_without_results() {
        let parser = BingChinaParser;
        let html = r#"<html><body><header id="b_header"></header></body></html>"#;
        let error = parser.parse(html).unwrap_err();
        assert!(error.to_string().contains("home page"));
    }

    #[test]
    fn test_parse_results_with_results() {
        let parser = BingChinaParser;
        let html = r#"
            <html>
            <body>
                <ol id="b_results">
                    <li class="b_algo">
                        <h2><a href="https://www.rust-lang.org/">Rust Programming Language</a></h2>
                        <div class="b_caption"><p>A language empowering everyone.</p></div>
                    </li>
                    <li class="b_algo">
                        <h2><a href="https://doc.rust-lang.org/book/">The Rust Book</a></h2>
                        <div class="b_caption"><p>The official Rust book.</p></div>
                    </li>
                </ol>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].content, "A language empowering everyone.");
        assert_eq!(results[1].title, "The Rust Book");
    }

    #[test]
    fn test_parse_results_skips_non_http_urls() {
        let parser = BingChinaParser;
        let html = r#"
            <html>
            <body>
                <li class="b_algo">
                    <h2><a href="javascript:void(0)">Bad Link</a></h2>
                </li>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        let parser = BingChinaParser;
        let html = r#"
            <html>
            <body>
                <li class="b_algo">
                    <div class="b_caption"><p>No title element</p></div>
                </li>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_algo_slug() {
        let parser = BingChinaParser;
        let html = r#"
            <html>
            <body>
                <li class="b_algo">
                    <h2><a href="https://example.com">Example</a></h2>
                    <div class="b_algoSlug">Snippet from algo slug.</div>
                </li>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Snippet from algo slug.");
    }
}
