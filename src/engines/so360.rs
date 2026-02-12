//! 360 Search engine implementation.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::fetcher::PageFetcher;
use crate::{
    Engine, EngineCategory, EngineConfig, HttpFetcher, Result, SearchError, SearchQuery,
    SearchResult,
};

/// 360 Search engine (360搜索).
pub struct So360 {
    config: EngineConfig,
    fetcher: Arc<dyn PageFetcher>,
}

impl So360 {
    /// Creates a new 360 Search engine with a default HTTP fetcher.
    pub fn new() -> Self {
        Self::with_fetcher(Arc::new(HttpFetcher::new()))
    }

    /// Creates a new 360 Search engine with a custom page fetcher.
    pub fn with_fetcher(fetcher: Arc<dyn PageFetcher>) -> Self {
        Self {
            config: EngineConfig {
                name: "360 Search".to_string(),
                shortcut: "360".to_string(),
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

impl Default for So360 {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Engine for So360 {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.so.com/s?q={}",
            urlencoding::encode(&query.query)
        );

        let html = self.fetcher.fetch(&url).await?;

        self.parse_results(&html)
    }
}

impl So360 {
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse("li.res-list")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let title_selector = Selector::parse("h3 a")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let snippet_selector = Selector::parse(".res-desc, .res-rich")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title_elem = element.select(&title_selector).next();

            if let Some(title_elem) = title_elem {
                let title = title_elem.text().collect::<String>().trim().to_string();

                // 360 Search stores the real URL in data-mdurl, falling back to href
                let url = title_elem
                    .value()
                    .attr("data-mdurl")
                    .or_else(|| title_elem.value().attr("href"))
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
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpFetcher;

    #[test]
    fn test_so360_new() {
        let engine = So360::new();
        assert_eq!(engine.config.name, "360 Search");
        assert_eq!(engine.config.shortcut, "360");
        assert_eq!(engine.config.weight, 1.0);
    }

    #[test]
    fn test_so360_with_fetcher() {
        let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = So360::with_fetcher(fetcher);
        assert_eq!(engine.name(), "360 Search");
    }

    #[test]
    fn test_so360_default() {
        let engine = So360::default();
        assert_eq!(engine.name(), "360 Search");
    }

    #[test]
    fn test_so360_with_config() {
        let custom_config = EngineConfig {
            name: "Custom 360".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = So360::new().with_config(custom_config);
        assert_eq!(engine.name(), "Custom 360");
    }

    #[test]
    fn test_so360_engine_trait() {
        let engine = So360::new();
        assert_eq!(engine.name(), "360 Search");
        assert_eq!(engine.shortcut(), "360");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_so360_parse_results_empty() {
        let engine = So360::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_so360_parse_results_with_data_mdurl() {
        let engine = So360::new();
        let html = r#"
        <html><body>
        <li class="res-list">
            <h3><a href="https://www.so.com/link?m=redirect_url" data-mdurl="https://www.rust-lang.org/">Rust Programming Language</a></h3>
            <div class="res-desc">A systems programming language focused on safety.</div>
        </li>
        <li class="res-list">
            <h3><a href="https://www.so.com/link?m=redirect_url2" data-mdurl="https://doc.rust-lang.org/book/">The Rust Book</a></h3>
            <div class="res-rich">Official Rust programming guide.</div>
        </li>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            results[0].content,
            "A systems programming language focused on safety."
        );
        assert_eq!(results[1].title, "The Rust Book");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
        assert_eq!(results[1].content, "Official Rust programming guide.");
    }

    #[test]
    fn test_so360_parse_results_fallback_to_href() {
        let engine = So360::new();
        let html = r#"
        <html><body>
        <li class="res-list">
            <h3><a href="https://example.com/page">Example Page</a></h3>
            <div class="res-desc">A page without data-mdurl.</div>
        </li>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
    }
}
