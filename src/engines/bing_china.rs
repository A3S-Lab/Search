//! Bing China search engine implementation using headless browser.
//!
//! This engine requires the `headless` feature because Bing China's search results
//! page relies on JavaScript rendering that plain HTTP requests cannot handle.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::fetcher::PageFetcher;
use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Bing China search engine (必应中国).
///
/// Requires a `PageFetcher` (typically a `BrowserFetcher`) to render
/// Bing China's JavaScript-heavy result pages.
pub struct BingChina {
    config: EngineConfig,
    fetcher: Arc<dyn PageFetcher>,
}

impl BingChina {
    /// Creates a new Bing China engine with the given page fetcher.
    pub fn new(fetcher: Arc<dyn PageFetcher>) -> Self {
        Self {
            config: EngineConfig {
                name: "Bing China".to_string(),
                shortcut: "bing_cn".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.0,
                timeout: 10,
                enabled: true,
                paging: true,
                safesearch: true,
            },
            fetcher,
        }
    }

    /// Creates with custom configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse("li.b_algo")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let title_selector = Selector::parse("h2 a")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let snippet_selector = Selector::parse(".b_caption p, .b_algoSlug")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title_elem = match element.select(&title_selector).next() {
                Some(el) => el,
                None => continue,
            };

            let title = title_elem.text().collect::<String>().trim().to_string();
            let url = title_elem
                .value()
                .attr("href")
                .unwrap_or_default()
                .to_string();

            let content = element
                .select(&snippet_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if !url.is_empty() && !title.is_empty() && url.starts_with("http") {
                results.push(SearchResult::new(url, title, content));
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl Engine for BingChina {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://cn.bing.com/search?q={}",
            urlencoding::encode(&query.query)
        );

        let html = self.fetcher.fetch(&url).await?;
        self.parse_results(&html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetcher_http::HttpFetcher;

    fn make_bing_china() -> BingChina {
        BingChina::new(Arc::new(HttpFetcher::new()))
    }

    #[test]
    fn test_bing_china_new() {
        let engine = make_bing_china();
        assert_eq!(engine.config.name, "Bing China");
        assert_eq!(engine.config.shortcut, "bing_cn");
        assert_eq!(engine.config.categories, vec![EngineCategory::General]);
        assert_eq!(engine.config.weight, 1.0);
        assert_eq!(engine.config.timeout, 10);
        assert!(engine.config.enabled);
        assert!(engine.config.paging);
        assert!(engine.config.safesearch);
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
        assert_eq!(engine.name(), "Custom Bing");
        assert_eq!(engine.shortcut(), "cbing");
        assert_eq!(engine.weight(), 1.5);
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
        let engine = make_bing_china();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_results() {
        let engine = make_bing_china();
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
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].content, "A language empowering everyone.");
        assert_eq!(results[1].title, "The Rust Book");
    }

    #[test]
    fn test_parse_results_skips_non_http_urls() {
        let engine = make_bing_china();
        let html = r#"
            <html>
            <body>
                <li class="b_algo">
                    <h2><a href="javascript:void(0)">Bad Link</a></h2>
                </li>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        let engine = make_bing_china();
        let html = r#"
            <html>
            <body>
                <li class="b_algo">
                    <div class="b_caption"><p>No title element</p></div>
                </li>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_algo_slug() {
        let engine = make_bing_china();
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
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Snippet from algo slug.");
    }
}
