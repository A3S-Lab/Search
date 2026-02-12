//! Sogou search engine implementation.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::fetcher::PageFetcher;
use crate::{
    Engine, EngineCategory, EngineConfig, HttpFetcher, Result, SearchError, SearchQuery,
    SearchResult,
};

/// Sogou search engine (搜狗).
pub struct Sogou {
    config: EngineConfig,
    fetcher: Arc<dyn PageFetcher>,
}

impl Sogou {
    /// Creates a new Sogou engine with a default HTTP fetcher.
    pub fn new() -> Self {
        Self::with_fetcher(Arc::new(HttpFetcher::new()))
    }

    /// Creates a new Sogou engine with a custom page fetcher.
    pub fn with_fetcher(fetcher: Arc<dyn PageFetcher>) -> Self {
        Self {
            config: EngineConfig {
                name: "Sogou".to_string(),
                shortcut: "sogou".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.0,
                timeout: 5,
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
}

impl Default for Sogou {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Engine for Sogou {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.sogou.com/web?query={}",
            urlencoding::encode(&query.query)
        );

        let html = self.fetcher.fetch(&url).await?;

        self.parse_results(&html)
    }
}

impl Sogou {
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse("div.vrwrap, div.rb")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let title_selector = Selector::parse("h3 a, .vr-title a")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let snippet_selector = Selector::parse(".str-text, .str_info, .space-txt")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title_elem = element.select(&title_selector).next();

            if let Some(title_elem) = title_elem {
                let title = title_elem.text().collect::<String>().trim().to_string();
                let raw_url = title_elem.value().attr("href").unwrap_or_default();

                // Sogou returns relative redirect URLs like /link?url=...
                let url = if raw_url.starts_with('/') {
                    format!("https://www.sogou.com{}", raw_url)
                } else {
                    raw_url.to_string()
                };

                let content = element
                    .select(&snippet_selector)
                    .next()
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();

                if !url.is_empty() && !title.is_empty() {
                    results.push(SearchResult::new(url, title, content));
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpFetcher;

    #[test]
    fn test_sogou_new() {
        let engine = Sogou::new();
        assert_eq!(engine.config.name, "Sogou");
        assert_eq!(engine.config.shortcut, "sogou");
        assert_eq!(engine.config.weight, 1.0);
    }

    #[test]
    fn test_sogou_with_fetcher() {
        let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = Sogou::with_fetcher(fetcher);
        assert_eq!(engine.name(), "Sogou");
    }

    #[test]
    fn test_sogou_default() {
        let engine = Sogou::default();
        assert_eq!(engine.name(), "Sogou");
    }

    #[test]
    fn test_sogou_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Sogou".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = Sogou::new().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Sogou");
    }

    #[test]
    fn test_sogou_engine_trait() {
        let engine = Sogou::new();
        assert_eq!(engine.name(), "Sogou");
        assert_eq!(engine.shortcut(), "sogou");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_sogou_parse_results_empty() {
        let engine = Sogou::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_sogou_parse_results_with_data() {
        let engine = Sogou::new();
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="/link?url=abc123">Rust Programming</a></h3>
            <div class="str-text">A systems programming language.</div>
        </div>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://example.com/page">Example Page</a></h3>
            <div class="str_info">Some description here.</div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming");
        assert_eq!(results[0].url, "https://www.sogou.com/link?url=abc123");
        assert_eq!(results[0].content, "A systems programming language.");
        assert_eq!(results[1].title, "Example Page");
        assert_eq!(results[1].url, "https://example.com/page");
    }

    #[test]
    fn test_sogou_parse_results_relative_url() {
        let engine = Sogou::new();
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3><a href="/link?url=xyz789">Test Result</a></h3>
            <div class="space-txt">Test snippet.</div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.sogou.com/link?url=xyz789");
    }
}
