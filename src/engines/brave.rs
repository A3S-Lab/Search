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

        let result_selector = Selector::parse("#results .snippet").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let title_selector = Selector::parse(".snippet-title").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let desc_selector = Selector::parse(".snippet-description").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let url_selector = Selector::parse("a").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;

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
}
