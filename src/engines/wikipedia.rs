//! Wikipedia search engine implementation.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::{Engine, EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};

/// Wikipedia search engine using the MediaWiki API.
pub struct Wikipedia {
    config: EngineConfig,
    client: Client,
    language: String,
}

impl Wikipedia {
    /// Creates a new Wikipedia engine.
    pub fn new() -> Self {
        Self {
            config: EngineConfig {
                name: "Wikipedia".to_string(),
                shortcut: "wiki".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.2,
                timeout: 5,
                enabled: true,
                paging: false,
                safesearch: false,
            },
            client: Client::builder()
                .user_agent("Mozilla/5.0 (compatible; a3s-search/0.1)")
                .build()
                .expect("Failed to create HTTP client"),
            language: "en".to_string(),
        }
    }

    /// Sets the Wikipedia language.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Creates with custom configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }
}

impl Default for Wikipedia {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct WikiResponse {
    query: Option<WikiQuery>,
}

#[derive(Deserialize)]
struct WikiQuery {
    search: Vec<WikiSearchResult>,
}

#[derive(Deserialize)]
struct WikiSearchResult {
    title: String,
    snippet: String,
    #[allow(dead_code)]
    pageid: u64,
}

#[async_trait]
impl Engine for Wikipedia {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://{}.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&format=json&srlimit=10",
            self.language,
            urlencoding::encode(&query.query)
        );

        let response = self.client.get(&url).send().await?;
        let wiki_response: WikiResponse = response.json().await?;

        let results = wiki_response
            .query
            .map(|q| {
                q.search
                    .into_iter()
                    .map(|item| {
                        let url = format!(
                            "https://{}.wikipedia.org/wiki/{}",
                            self.language,
                            item.title.replace(' ', "_")
                        );
                        let content = strip_html_tags(&item.snippet);
                        SearchResult::new(url, item.title, content)
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wikipedia_new() {
        let engine = Wikipedia::new();
        assert_eq!(engine.config.name, "Wikipedia");
        assert_eq!(engine.config.shortcut, "wiki");
        assert_eq!(engine.config.weight, 1.2);
        assert_eq!(engine.language, "en");
    }

    #[test]
    fn test_wikipedia_default() {
        let engine = Wikipedia::default();
        assert_eq!(engine.name(), "Wikipedia");
    }

    #[test]
    fn test_wikipedia_with_language() {
        let engine = Wikipedia::new().with_language("zh");
        assert_eq!(engine.language, "zh");
    }

    #[test]
    fn test_wikipedia_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Wiki".to_string(),
            weight: 2.0,
            ..Default::default()
        };
        let engine = Wikipedia::new().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Wiki");
        assert_eq!(engine.weight(), 2.0);
    }

    #[test]
    fn test_wikipedia_engine_trait() {
        let engine = Wikipedia::new();
        assert_eq!(engine.name(), "Wikipedia");
        assert_eq!(engine.shortcut(), "wiki");
        assert_eq!(engine.weight(), 1.2);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_strip_html_tags_simple() {
        let html = "<b>bold</b> text";
        assert_eq!(strip_html_tags(html), "bold text");
    }

    #[test]
    fn test_strip_html_tags_nested() {
        let html = "<div><span>nested</span></div>";
        assert_eq!(strip_html_tags(html), "nested");
    }

    #[test]
    fn test_strip_html_tags_no_tags() {
        let html = "plain text";
        assert_eq!(strip_html_tags(html), "plain text");
    }

    #[test]
    fn test_strip_html_tags_empty() {
        let html = "";
        assert_eq!(strip_html_tags(html), "");
    }

    #[test]
    fn test_strip_html_tags_only_tags() {
        let html = "<br><hr>";
        assert_eq!(strip_html_tags(html), "");
    }

    #[test]
    fn test_strip_html_tags_with_attributes() {
        let html = r#"<a href="url">link</a>"#;
        assert_eq!(strip_html_tags(html), "link");
    }

    #[test]
    fn test_wiki_response_deserialization_with_results() {
        let json = r#"{
            "query": {
                "search": [
                    {"title": "Rust (programming language)", "snippet": "<span class=\"searchmatch\">Rust</span> is a language", "pageid": 12345},
                    {"title": "Rust", "snippet": "Rust is an iron oxide", "pageid": 67890}
                ]
            }
        }"#;
        let response: WikiResponse = serde_json::from_str(json).unwrap();
        let query = response.query.unwrap();
        assert_eq!(query.search.len(), 2);
        assert_eq!(query.search[0].title, "Rust (programming language)");
        assert_eq!(query.search[1].title, "Rust");
    }

    #[test]
    fn test_wiki_response_deserialization_empty_results() {
        let json = r#"{"query": {"search": []}}"#;
        let response: WikiResponse = serde_json::from_str(json).unwrap();
        let query = response.query.unwrap();
        assert!(query.search.is_empty());
    }

    #[test]
    fn test_wiki_response_deserialization_no_query() {
        let json = r#"{}"#;
        let response: WikiResponse = serde_json::from_str(json).unwrap();
        assert!(response.query.is_none());
    }

    #[test]
    fn test_strip_html_tags_mixed_content() {
        let html = "Hello <b>world</b>, this is <i>a</i> test";
        assert_eq!(strip_html_tags(html), "Hello world, this is a test");
    }

    #[test]
    fn test_strip_html_tags_unclosed_tag() {
        let html = "Hello <b>world";
        assert_eq!(strip_html_tags(html), "Hello world");
    }

    #[test]
    fn test_wikipedia_with_language_zh() {
        let engine = Wikipedia::new().with_language("zh");
        assert_eq!(engine.language, "zh");
        assert_eq!(engine.name(), "Wikipedia");
    }
}
