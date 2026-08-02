//! Search engine trait and configuration.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Result, SearchImage, SearchQuery, SearchReport, SearchResult};

/// Categories for search engines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineCategory {
    #[default]
    General,
    Images,
    Videos,
    News,
    Maps,
    Music,
    Files,
    Science,
    Social,
}

/// Configuration for a search engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Stable logical source name used for display, provenance, and rank fusion.
    ///
    /// Multiple transport variants for the same upstream source must share
    /// this name and use distinct [`Self::shortcut`] values.
    pub name: String,
    /// Short identifier for one selectable source transport (for example,
    /// `ddg` for DuckDuckGo over HTTP).
    pub shortcut: String,
    /// Categories this engine belongs to.
    pub categories: Vec<EngineCategory>,
    /// Weight for ranking (higher = more influence).
    #[serde(default = "default_weight")]
    pub weight: f64,
    /// Request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Whether the engine is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Whether pagination is supported.
    #[serde(default)]
    pub paging: bool,
    /// Whether safe search is supported.
    #[serde(default)]
    pub safesearch: bool,
}

fn default_weight() -> f64 {
    1.0
}

fn default_timeout() -> u64 {
    5
}

fn default_enabled() -> bool {
    true
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            shortcut: String::new(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 5,
            enabled: true,
            paging: false,
            safesearch: false,
        }
    }
}

/// Rich output from one engine execution.
///
/// Existing engines can continue implementing [`Engine::search`]; the default
/// [`Engine::search_output`] implementation wraps those results. Provider
/// adapters override `search_output` to return direct answers, suggestions, and
/// structured request reports without encoding them as synthetic web results.
#[derive(Debug, Clone, Default)]
pub struct EngineOutput {
    /// Web or media results returned by the engine.
    pub results: Vec<SearchResult>,
    /// Query suggestions returned by the engine.
    pub suggestions: Vec<String>,
    /// Direct answers returned by the engine.
    pub answers: Vec<String>,
    /// Query-related images returned by the engine.
    pub images: Vec<SearchImage>,
    /// Structured execution reports.
    pub reports: Vec<SearchReport>,
}

impl EngineOutput {
    /// Creates output from ordinary search results.
    pub fn new(results: Vec<SearchResult>) -> Self {
        Self {
            results,
            ..Self::default()
        }
    }

    /// Adds a direct answer.
    pub fn with_answer(mut self, answer: impl Into<String>) -> Self {
        self.answers.push(answer.into());
        self
    }

    /// Adds a query suggestion.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Adds a query-related image.
    pub fn with_image(mut self, image: SearchImage) -> Self {
        crate::result::merge_image(&mut self.images, image);
        self
    }

    /// Adds a structured execution report.
    pub fn with_report(mut self, report: SearchReport) -> Self {
        self.reports.push(report);
        self
    }
}

impl From<Vec<SearchResult>> for EngineOutput {
    fn from(results: Vec<SearchResult>) -> Self {
        Self::new(results)
    }
}

/// Trait for implementing search engines.
///
/// Each search engine must implement this trait to be used with the meta search.
#[async_trait]
pub trait Engine: Send + Sync {
    /// Returns the engine configuration.
    fn config(&self) -> &EngineConfig;

    /// Performs a search and returns results.
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;

    /// Performs a search and returns rich engine output.
    ///
    /// The default preserves source compatibility for existing engines.
    async fn search_output(&self, query: &SearchQuery) -> Result<EngineOutput> {
        self.search(query).await.map(EngineOutput::new)
    }

    /// Returns the engine name.
    fn name(&self) -> &str {
        &self.config().name
    }

    /// Returns the engine shortcut.
    fn shortcut(&self) -> &str {
        &self.config().shortcut
    }

    /// Returns the engine weight.
    fn weight(&self) -> f64 {
        self.config().weight
    }

    /// Returns whether the engine is enabled.
    fn is_enabled(&self) -> bool {
        self.config().enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_category_default() {
        let default: EngineCategory = Default::default();
        assert_eq!(default, EngineCategory::General);
    }

    #[test]
    fn test_engine_category_variants() {
        let categories = vec![
            EngineCategory::General,
            EngineCategory::Images,
            EngineCategory::Videos,
            EngineCategory::News,
            EngineCategory::Maps,
            EngineCategory::Music,
            EngineCategory::Files,
            EngineCategory::Science,
            EngineCategory::Social,
        ];
        assert_eq!(categories.len(), 9);
    }

    #[test]
    fn test_engine_config_default() {
        let config = EngineConfig::default();
        assert_eq!(config.name, "");
        assert_eq!(config.shortcut, "");
        assert_eq!(config.categories, vec![EngineCategory::General]);
        assert_eq!(config.weight, 1.0);
        assert_eq!(config.timeout, 5);
        assert!(config.enabled);
        assert!(!config.paging);
        assert!(!config.safesearch);
    }

    #[test]
    fn test_engine_config_custom() {
        let config = EngineConfig {
            name: "Test Engine".to_string(),
            shortcut: "test".to_string(),
            categories: vec![EngineCategory::Images, EngineCategory::Videos],
            weight: 2.0,
            timeout: 10,
            enabled: false,
            paging: true,
            safesearch: true,
        };
        assert_eq!(config.name, "Test Engine");
        assert_eq!(config.shortcut, "test");
        assert_eq!(config.weight, 2.0);
        assert_eq!(config.timeout, 10);
        assert!(!config.enabled);
        assert!(config.paging);
        assert!(config.safesearch);
    }

    #[test]
    fn test_engine_config_serialization() {
        let config = EngineConfig {
            name: "Test".to_string(),
            shortcut: "t".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"name\":\"Test\""));
        assert!(json.contains("\"shortcut\":\"t\""));
    }

    #[test]
    fn test_engine_config_deserialization() {
        let json = r#"{"name":"Test","shortcut":"t","categories":["general"]}"#;
        let config: EngineConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "Test");
        assert_eq!(config.shortcut, "t");
        assert_eq!(config.weight, 1.0); // default
        assert_eq!(config.timeout, 5); // default
        assert!(config.enabled); // default
    }

    #[test]
    fn test_engine_category_serialization() {
        let category = EngineCategory::Images;
        let json = serde_json::to_string(&category).unwrap();
        assert_eq!(json, "\"images\"");
    }

    #[test]
    fn test_engine_category_deserialization() {
        let json = "\"videos\"";
        let category: EngineCategory = serde_json::from_str(json).unwrap();
        assert_eq!(category, EngineCategory::Videos);
    }

    #[test]
    fn test_engine_category_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(EngineCategory::General);
        set.insert(EngineCategory::Images);
        set.insert(EngineCategory::General); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_engine_output_builders() {
        let output = EngineOutput::new(vec![SearchResult::new(
            "https://example.com",
            "Example",
            "Snippet",
        )])
        .with_answer("42")
        .with_suggestion("rust async")
        .with_image(SearchImage::new("https://example.com/image.png"))
        .with_report(SearchReport::new("provider"));

        assert_eq!(output.results.len(), 1);
        assert_eq!(output.answers, vec!["42"]);
        assert_eq!(output.suggestions, vec!["rust async"]);
        assert_eq!(output.images.len(), 1);
        assert_eq!(output.reports.len(), 1);
    }
}
