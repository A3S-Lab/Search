//! Sogou search engine implementation.

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Sogou search engine (搜狗).
pub struct Sogou {
    config: EngineConfig,
    client: Client,
}

impl Sogou {
    /// Creates a new Sogou engine.
    pub fn new() -> Self {
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

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        self.parse_results(&html)
    }
}

impl Sogou {
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse("div.vrwrap, div.rb").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let title_selector = Selector::parse("h3 a, .vr-title a").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let snippet_selector = Selector::parse(".str-text, .str_info, .space-txt").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;

        let mut results = Vec::new();

        for element in document.select(&result_selector) {
            let title_elem = element.select(&title_selector).next();

            if let Some(title_elem) = title_elem {
                let title = title_elem.text().collect::<String>().trim().to_string();
                let url = title_elem.value().attr("href").unwrap_or_default().to_string();

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
    fn test_sogou_new() {
        let engine = Sogou::new();
        assert_eq!(engine.config.name, "Sogou");
        assert_eq!(engine.config.shortcut, "sogou");
        assert_eq!(engine.config.weight, 1.0);
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
}
