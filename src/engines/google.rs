//! Google search engine implementation (via scraping).

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Google search engine.
pub struct Google {
    config: EngineConfig,
    client: Client,
}

impl Google {
    /// Creates a new Google engine.
    pub fn new() -> Self {
        Self {
            config: EngineConfig {
                name: "Google".to_string(),
                shortcut: "g".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.5,
                timeout: 5,
                enabled: true,
                paging: true,
                safesearch: true,
            },
            client: Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
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

impl Default for Google {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Engine for Google {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.google.com/search?q={}&hl=en",
            urlencoding::encode(&query.query)
        );

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        self.parse_results(&html)
    }
}

impl Google {
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse("div.g").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let title_selector = Selector::parse("h3").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let link_selector = Selector::parse("a").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let snippet_selector = Selector::parse("div[data-sncf]").map_err(|e| {
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
                .select(&link_selector)
                .next()
                .and_then(|e| e.value().attr("href"))
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_google_new() {
        let engine = Google::new();
        assert_eq!(engine.config.name, "Google");
        assert_eq!(engine.config.shortcut, "g");
        assert_eq!(engine.config.weight, 1.5);
    }

    #[test]
    fn test_google_default() {
        let engine = Google::default();
        assert_eq!(engine.name(), "Google");
    }

    #[test]
    fn test_google_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Google".to_string(),
            weight: 2.0,
            ..Default::default()
        };
        let engine = Google::new().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Google");
    }

    #[test]
    fn test_google_engine_trait() {
        let engine = Google::new();
        assert_eq!(engine.name(), "Google");
        assert_eq!(engine.shortcut(), "g");
        assert_eq!(engine.weight(), 1.5);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_google_parse_results_empty() {
        let engine = Google::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }
}
