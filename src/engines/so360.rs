//! 360 Search engine implementation.

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// 360 Search engine (360搜索).
pub struct So360 {
    config: EngineConfig,
    client: Client,
}

impl So360 {
    /// Creates a new 360 Search engine.
    pub fn new() -> Self {
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

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

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
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_so360_new() {
        let engine = So360::new();
        assert_eq!(engine.config.name, "360 Search");
        assert_eq!(engine.config.shortcut, "360");
        assert_eq!(engine.config.weight, 1.0);
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
}
