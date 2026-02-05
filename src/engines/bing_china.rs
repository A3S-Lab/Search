//! Bing China search engine implementation.

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Bing China search engine (必应中国).
pub struct BingChina {
    config: EngineConfig,
    client: Client,
}

impl BingChina {
    /// Creates a new Bing China engine.
    pub fn new() -> Self {
        Self {
            config: EngineConfig {
                name: "Bing China".to_string(),
                shortcut: "bing_cn".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.0,
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

impl Default for BingChina {
    fn default() -> Self {
        Self::new()
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

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        self.parse_results(&html)
    }
}

impl BingChina {
    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let result_selector = Selector::parse("li.b_algo").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let title_selector = Selector::parse("h2 a").map_err(|e| {
            SearchError::Parse(format!("Failed to parse selector: {:?}", e))
        })?;
        let snippet_selector = Selector::parse(".b_caption p, .b_algoSlug").map_err(|e| {
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

                if !url.is_empty() && !title.is_empty() && url.starts_with("http") {
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
    fn test_bing_china_new() {
        let engine = BingChina::new();
        assert_eq!(engine.config.name, "Bing China");
        assert_eq!(engine.config.shortcut, "bing_cn");
        assert_eq!(engine.config.weight, 1.0);
    }

    #[test]
    fn test_bing_china_default() {
        let engine = BingChina::default();
        assert_eq!(engine.name(), "Bing China");
    }

    #[test]
    fn test_bing_china_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Bing".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = BingChina::new().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Bing");
    }

    #[test]
    fn test_bing_china_engine_trait() {
        let engine = BingChina::new();
        assert_eq!(engine.name(), "Bing China");
        assert_eq!(engine.shortcut(), "bing_cn");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_bing_china_parse_results_empty() {
        let engine = BingChina::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }
}
