//! Configuration file loading for search engine setup.
//!
//! Uses HCL (HashiCorp Configuration Language) as the configuration format.
//!
//! ## Example HCL
//!
//! ```hcl
//! timeout = 10
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

use serde::Deserialize;

use crate::{HealthConfig, SearchError};

/// Top-level search configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    /// Default timeout in seconds for all engines.
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Health monitor configuration.
    #[serde(default)]
    pub health: Option<HealthEntry>,

    /// Engine configurations keyed by shortcut.
    #[serde(default, rename = "engine")]
    pub engines: HashMap<String, EngineEntry>,
}

/// Health monitor configuration entry.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthEntry {
    /// Number of consecutive failures before suspending.
    #[serde(default = "default_max_failures")]
    pub max_failures: u32,

    /// Suspension duration in seconds.
    #[serde(default = "default_suspend_seconds")]
    pub suspend_seconds: u64,
}

/// Per-engine configuration entry.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineEntry {
    /// Whether the engine is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Weight for ranking (higher = more influence).
    #[serde(default = "default_weight")]
    pub weight: f64,

    /// Per-engine timeout override in seconds.
    pub timeout: Option<u64>,
}

fn default_timeout() -> u64 {
    10
}

fn default_max_failures() -> u32 {
    3
}

fn default_suspend_seconds() -> u64 {
    60
}

fn default_enabled() -> bool {
    true
}

fn default_weight() -> f64 {
    1.0
}

impl SearchConfig {
    /// Loads a configuration from an HCL file.
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

    /// Parses configuration from an HCL string.
    pub fn parse(content: &str) -> crate::Result<Self> {
        hcl::from_str(content)
            .map_err(|e| SearchError::Parse(format!("Failed to parse HCL config: {}", e)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hcl_basic() {
        let hcl = r#"
            timeout = 10

            health {
                max_failures    = 5
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

        let config = SearchConfig::parse(hcl).unwrap();
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
    fn test_parse_hcl_minimal() {
        let hcl = r#"
            engine "ddg" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
        assert_eq!(config.timeout, 10); // default
        assert!(config.health.is_none());
        assert_eq!(config.engines.len(), 1);
        assert!(config.engines["ddg"].enabled);
        assert_eq!(config.engines["ddg"].weight, 1.0); // default
    }

    #[test]
    fn test_parse_hcl_disabled_engine() {
        let hcl = r#"
            engine "ddg" {
                enabled = false
                weight  = 0.5
            }
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
        assert!(!config.engines["ddg"].enabled);
        assert_eq!(config.engines["ddg"].weight, 0.5);
    }

    #[test]
    fn test_parse_hcl_engine_timeout_override() {
        let hcl = r#"
            timeout = 5

            engine "wiki" {
                enabled = true
                timeout = 15
            }
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
        assert_eq!(config.timeout, 5);
        assert_eq!(config.engines["wiki"].timeout, Some(15));
    }

    #[test]
    fn test_health_config_conversion() {
        let hcl = r#"
            health {
                max_failures    = 5
                suspend_seconds = 120
            }

            engine "ddg" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
        let health_config = config.health_config();
        assert_eq!(health_config.max_failures, 5);
        assert_eq!(health_config.suspend_duration, Duration::from_secs(120));
    }

    #[test]
    fn test_health_config_default_when_missing() {
        let hcl = r#"
            engine "ddg" {
                enabled = true
            }
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
        let health_config = config.health_config();
        assert_eq!(health_config.max_failures, 3);
        assert_eq!(health_config.suspend_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_enabled_engines() {
        let hcl = r#"
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

        let config = SearchConfig::parse(hcl).unwrap();
        let enabled = config.enabled_engines();
        assert_eq!(enabled.len(), 2);
        assert!(enabled.contains(&"ddg"));
        assert!(enabled.contains(&"wiki"));
        assert!(!enabled.contains(&"brave"));
    }

    #[test]
    fn test_invalid_hcl() {
        let result = SearchConfig::parse("{{{{invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_hcl_all_engines() {
        let hcl = r#"
            timeout = 8

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

        let config = SearchConfig::parse(hcl).unwrap();
        assert_eq!(config.engines.len(), 6);
        assert_eq!(config.enabled_engines().len(), 4);
    }

    #[test]
    fn test_engine_entry_defaults() {
        let hcl = r#"
            engine "ddg" {}
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
        let entry = &config.engines["ddg"];
        assert!(entry.enabled); // default true
        assert_eq!(entry.weight, 1.0); // default 1.0
        assert!(entry.timeout.is_none());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = SearchConfig::load("/nonexistent/path/config.hcl");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read config file"));
    }
}
