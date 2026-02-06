//! Search engine trait and configuration.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Result, SearchQuery, SearchResult};

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
    /// Display name of the engine.
    pub name: String,
    /// Short identifier (e.g., "ddg" for DuckDuckGo).
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

/// Trait for implementing search engines.
///
/// Each search engine must implement this trait to be used with the meta search.
#[async_trait]
pub trait Engine: Send + Sync {
    /// Returns the engine configuration.
    fn config(&self) -> &EngineConfig;

    /// Performs a search and returns results.
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;

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
}
