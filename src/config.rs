//! Configuration file loading for search engine setup.
//!
//! Uses ACL (Agent Configuration Language) as the configuration format.
//!
//! ## Example ACL
//!
//! ```acl
//! timeout {
//!   value = 10
//! }
//!
//! health {
//!   max_failures    = 3
//!   suspend_seconds = 60
//! }
//!
//! engine "ddg" {
//!   enabled = true
//!   weight  = 1.0
//! }
//!
//! engine "brave" {
//!   enabled = true
//!   weight  = 1.2
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use a3s_acl::ast::{Document, Value};
use a3s_acl::parse;

use crate::{HealthConfig, SearchError};

/// Top-level search configuration.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Default timeout in seconds for all engines.
    pub timeout: u64,

    /// Health monitor configuration.
    pub health: Option<HealthEntry>,

    /// Engine configurations keyed by shortcut.
    pub engines: HashMap<String, EngineEntry>,
}

/// Health monitor configuration entry.
#[derive(Debug, Clone)]
pub struct HealthEntry {
    /// Number of consecutive failures before suspending.
    pub max_failures: u32,

    /// Suspension duration in seconds.
    pub suspend_seconds: u64,
}

/// Per-engine configuration entry.
#[derive(Debug, Clone)]
pub struct EngineEntry {
    /// Whether the engine is enabled.
    pub enabled: bool,

    /// Weight for ranking (higher = more influence).
    pub weight: f64,

    /// Per-engine timeout override in seconds.
    pub timeout: Option<u64>,
}

impl SearchConfig {
    /// Loads a configuration from an ACL file.
    pub fn load(path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| {
            SearchError::Other(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;

        Self::parse(&content)
    }

    /// Parses configuration from an ACL string.
    pub fn parse(content: &str) -> crate::Result<Self> {
        let doc = parse(content)
            .map_err(|e| SearchError::Parse(format!("Failed to parse ACL config: {}", e)))?;

        Self::from_document(&doc)
    }

    /// Converts an ACL Document to SearchConfig.
    fn from_document(doc: &Document) -> crate::Result<Self> {
        let mut timeout = 10u64;
        let mut health = None;
        let mut engines: HashMap<String, EngineEntry> = HashMap::new();

        for block in &doc.blocks {
            match block.name.as_str() {
                "timeout" => {
                    if let Some(v) = block.attributes.get("value") {
                        timeout = value_as_u64(v).unwrap_or(10);
                    }
                }
                "health" => {
                    health = Some(HealthEntry {
                        max_failures: block
                            .attributes
                            .get("max_failures")
                            .and_then(value_as_u32)
                            .unwrap_or(3),
                        suspend_seconds: block
                            .attributes
                            .get("suspend_seconds")
                            .and_then(value_as_u64)
                            .unwrap_or(60),
                    });
                }
                "engine" => {
                    // Engine blocks have the engine shortcut as a label
                    if let Some(shortcut) = block.labels.first() {
                        engines.insert(
                            shortcut.clone(),
                            EngineEntry {
                                enabled: block
                                    .attributes
                                    .get("enabled")
                                    .and_then(value_as_bool)
                                    .unwrap_or(true),
                                weight: block
                                    .attributes
                                    .get("weight")
                                    .and_then(value_as_f64)
                                    .unwrap_or(1.0),
                                timeout: block.attributes.get("timeout").and_then(value_as_u64),
                            },
                        );
                    }
                }
                _ => {
                    // Ignore unknown blocks
                }
            }
        }

        Ok(Self {
            timeout,
            health,
            engines,
        })
    }

    /// Converts the health entry to a `HealthConfig`.
    pub fn health_config(&self) -> HealthConfig {
        match &self.health {
            Some(h) => HealthConfig {
                max_failures: h.max_failures,
                suspend_duration: Duration::from_secs(h.suspend_seconds),
            },
            None => HealthConfig::default(),
        }
    }

    /// Returns the list of enabled engine shortcuts.
    pub fn enabled_engines(&self) -> Vec<&str> {
        self.engines
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .map(|(shortcut, _)| shortcut.as_str())
            .collect()
    }
}

/// Extract a u64 from a Value.
fn value_as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => Some(*n as u64),
        _ => None,
    }
}

/// Extract a u32 from a Value.
fn value_as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => Some(*n as u32),
        _ => None,
    }
}

/// Extract a f64 from a Value.
fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        _ => None,
    }
}

/// Extract a bool from a Value.
fn value_as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_acl_basic() {
        let acl = r#"
            timeout {
                value = 10
            }

            health {
                max_failures = 5
                suspend_seconds = 120
            }

            engine "ddg" {
                enabled = true
                weight  = 1.0
            }

            engine "brave" {
                enabled = true
                weight  = 1.2
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        assert_eq!(config.timeout, 10);

        let health = config.health.unwrap();
        assert_eq!(health.max_failures, 5);
        assert_eq!(health.suspend_seconds, 120);

        assert_eq!(config.engines.len(), 2);
        assert!(config.engines.contains_key("ddg"));
        assert!(config.engines.contains_key("brave"));
        assert_eq!(config.engines["ddg"].weight, 1.0);
        assert_eq!(config.engines["brave"].weight, 1.2);
    }

    #[test]
    fn test_parse_acl_minimal() {
        let acl = r#"
            engine "ddg" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        assert_eq!(config.timeout, 10); // default
        assert!(config.health.is_none());
        assert_eq!(config.engines.len(), 1);
        assert!(config.engines["ddg"].enabled);
        assert_eq!(config.engines["ddg"].weight, 1.0); // default
    }

    #[test]
    fn test_parse_acl_disabled_engine() {
        let acl = r#"
            engine "ddg" {
                enabled = false
                weight  = 0.5
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        assert!(!config.engines["ddg"].enabled);
        assert_eq!(config.engines["ddg"].weight, 0.5);
    }

    #[test]
    fn test_parse_acl_engine_timeout_override() {
        let acl = r#"
            timeout {
                value = 5
            }

            engine "wiki" {
                enabled = true
                timeout = 15
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        assert_eq!(config.timeout, 5);
        assert_eq!(config.engines["wiki"].timeout, Some(15));
    }

    #[test]
    fn test_health_config_conversion() {
        let acl = r#"
            health {
                max_failures = 5
                suspend_seconds = 120
            }

            engine "ddg" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        let health_config = config.health_config();
        assert_eq!(health_config.max_failures, 5);
        assert_eq!(health_config.suspend_duration, Duration::from_secs(120));
    }

    #[test]
    fn test_health_config_default_when_missing() {
        let acl = r#"
            engine "ddg" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        let health_config = config.health_config();
        assert_eq!(health_config.max_failures, 3);
        assert_eq!(health_config.suspend_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_enabled_engines() {
        let acl = r#"
            engine "ddg" {
                enabled = true
            }

            engine "brave" {
                enabled = false
            }

            engine "wiki" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        let enabled = config.enabled_engines();
        assert_eq!(enabled.len(), 2);
        assert!(enabled.contains(&"ddg"));
        assert!(enabled.contains(&"wiki"));
        assert!(!enabled.contains(&"brave"));
    }

    #[test]
    fn test_invalid_acl() {
        let result = SearchConfig::parse("{{{{invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_acl_all_engines() {
        let acl = r#"
            timeout {
                value = 8
            }

            engine "ddg" {
                enabled = true
                weight  = 1.0
            }

            engine "brave" {
                enabled = true
                weight  = 1.1
            }

            engine "bing" {
                enabled = true
                weight  = 1.0
            }

            engine "wiki" {
                enabled = true
                weight  = 0.8
            }

            engine "sogou" {
                enabled = false
            }

            engine "360" {
                enabled = false
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        assert_eq!(config.engines.len(), 6);
        assert_eq!(config.enabled_engines().len(), 4);
    }

    #[test]
    fn test_engine_entry_defaults() {
        let acl = r#"
            engine "ddg" {}
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        let entry = &config.engines["ddg"];
        assert!(entry.enabled); // default true
        assert_eq!(entry.weight, 1.0); // default 1.0
        assert!(entry.timeout.is_none());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = SearchConfig::load("/nonexistent/path/config.acl");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read config file"));
    }
}
