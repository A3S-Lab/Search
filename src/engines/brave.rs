//! Brave search engine implementation.

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Brave search engine.
pub struct Brave {
    config: EngineConfig,
    client: Client,
}

impl Brave {
    /// Creates a new Brave engine.
    pub fn new() -> Self {
        Self {
            config: EngineConfig {
                name: "Brave".to_string(),
                shortcut: "brave".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.0,
                timeout: 5,
                enabled: true,
                paging: true,
                safesearch: true,
            },
            client: Client::builder()
                .user_agent("Mozilla/5.0 (compatible; a3s-search/0.1)")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Creates with custom configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }
}

impl Default for Brave {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Engine for Brave {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://search.brave.com/search?q={}",
            urlencoding::encode(&query.query)
        );

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        self.parse_results(&html)
    }
}

impl Brave {
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector =
            Selector::parse(r#"div.snippet[data-type="web"]"#)
                .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let title_selector = Selector::parse(".search-snippet-title")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let desc_selector = Selector::parse(".generic-snippet .content, .snippet-description")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let url_selector = Selector::parse(r#"a[href^="http"]"#)
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let url = element
                .select(&url_selector)
                .next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or_default()
                .to_string();

            let content = element
                .select(&desc_selector)
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brave_new() {
        let engine = Brave::new();
        assert_eq!(engine.config.name, "Brave");
        assert_eq!(engine.config.shortcut, "brave");
        assert_eq!(engine.config.weight, 1.0);
    }

    #[test]
    fn test_brave_default() {
        let engine = Brave::default();
        assert_eq!(engine.name(), "Brave");
    }

    #[test]
    fn test_brave_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Brave".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = Brave::new().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Brave");
    }

    #[test]
    fn test_brave_engine_trait() {
        let engine = Brave::new();
        assert_eq!(engine.name(), "Brave");
        assert_eq!(engine.shortcut(), "brave");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_brave_parse_results_empty() {
        let engine = Brave::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_brave_parse_results_with_data() {
        let engine = Brave::new();
        let html = r#"
        <html><body>
        <div class="snippet" data-type="web">
            <a href="https://www.rust-lang.org/" class="search-snippet-title">Rust Programming Language</a>
            <div class="generic-snippet"><div class="content">A systems programming language focused on safety.</div></div>
        </div>
        <div class="snippet" data-type="web">
            <a href="https://doc.rust-lang.org/book/" class="search-snippet-title">The Rust Book</a>
            <div class="snippet-description">Official Rust programming guide.</div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].content, "A systems programming language focused on safety.");
        assert_eq!(results[1].title, "The Rust Book");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
        assert_eq!(results[1].content, "Official Rust programming guide.");
    }

    #[test]
    fn test_brave_parse_results_skips_non_web() {
        let engine = Brave::new();
        let html = r#"
        <html><body>
        <div class="snippet" data-type="video">
            <a href="https://example.com/video" class="search-snippet-title">A Video</a>
        </div>
        <div class="snippet" data-type="web">
            <a href="https://example.com/page" class="search-snippet-title">A Page</a>
        </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "A Page");
    }
}
