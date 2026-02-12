//! Baidu search engine implementation using headless browser.
//!
//! This engine requires the `headless` feature because Baidu's search results
//! page relies on JavaScript rendering that plain HTTP requests cannot handle.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::fetcher::PageFetcher;
use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Baidu search engine (百度).
///
/// Requires a `PageFetcher` (typically a `BrowserFetcher`) to render
/// Baidu's JavaScript-heavy result pages.
pub struct Baidu {
    config: EngineConfig,
    fetcher: Arc<dyn PageFetcher>,
}

impl Baidu {
    /// Creates a new Baidu engine with the given page fetcher.
    pub fn new(fetcher: Arc<dyn PageFetcher>) -> Self {
        Self {
            config: EngineConfig {
                name: "Baidu".to_string(),
                shortcut: "baidu".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.0,
                timeout: 10,
                enabled: true,
                paging: true,
                safesearch: false,
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

        let result_selector = Selector::parse("div.result, div.c-container")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let title_selector = Selector::parse("h3 a, .t a")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let snippet_selector =
            Selector::parse(".c-abstract, .c-span-last, .content-right_8Zs40")
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

            if !url.is_empty() && !title.is_empty() {
                results.push(SearchResult::new(url, title, content));
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl Engine for Baidu {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.baidu.com/s?wd={}",
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

    fn make_baidu() -> Baidu {
        Baidu::new(Arc::new(HttpFetcher::new()))
    }

    #[test]
    fn test_baidu_new() {
        let engine = make_baidu();
        assert_eq!(engine.config.name, "Baidu");
        assert_eq!(engine.config.shortcut, "baidu");
        assert_eq!(engine.config.categories, vec![EngineCategory::General]);
        assert_eq!(engine.config.weight, 1.0);
        assert_eq!(engine.config.timeout, 10);
        assert!(engine.config.enabled);
        assert!(engine.config.paging);
        assert!(!engine.config.safesearch);
    }

    #[test]
    fn test_baidu_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Baidu".to_string(),
            shortcut: "cbaidu".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = make_baidu().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Baidu");
        assert_eq!(engine.shortcut(), "cbaidu");
        assert_eq!(engine.weight(), 1.5);
    }

    #[test]
    fn test_baidu_engine_trait() {
        let engine = make_baidu();
        assert_eq!(engine.name(), "Baidu");
        assert_eq!(engine.shortcut(), "baidu");
        assert_eq!(engine.weight(), 1.0);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_parse_results_empty_html() {
        let engine = make_baidu();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_results() {
        let engine = make_baidu();
        let html = r#"
            <html>
            <body>
                <div class="c-container">
                    <h3><a href="https://www.rust-lang.org/">Rust 编程语言</a></h3>
                    <div class="c-abstract">一门赋予每个人构建可靠软件能力的语言。</div>
                </div>
                <div class="result">
                    <h3><a href="https://doc.rust-lang.org/book/">Rust 程序设计语言</a></h3>
                    <div class="c-abstract">Rust 官方教程。</div>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust 编程语言");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].content, "一门赋予每个人构建可靠软件能力的语言。");
        assert_eq!(results[1].title, "Rust 程序设计语言");
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        let engine = make_baidu();
        let html = r#"
            <html>
            <body>
                <div class="c-container">
                    <div class="c-abstract">No title here</div>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_empty_url() {
        let engine = make_baidu();
        let html = r#"
            <html>
            <body>
                <div class="c-container">
                    <h3><a href="">Empty URL</a></h3>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }
}
