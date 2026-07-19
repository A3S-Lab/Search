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
//!
//! provider "tavily" {
//!   api_key            = env("TAVILY_API_KEY")
//!   search_depth       = "advanced"
//!   include_answer     = "advanced"
//!   include_raw_content = "markdown"
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use a3s_acl::ast::{Document, Value};
use a3s_acl::parse;

use crate::providers::ProviderEngine;
use crate::{Engine, EngineConfig, HealthConfig, SearchError};

mod provider;

pub use provider::{ProviderEntry, ProviderSettings};

const MAX_ACL_EXACT_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Top-level search configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SearchConfig {
    /// Default timeout in seconds for all engines.
    pub timeout: u64,

    /// Health monitor configuration.
    pub health: Option<HealthEntry>,

    /// Engine configurations keyed by shortcut.
    pub engines: HashMap<String, EngineEntry>,

    /// Typed native provider configurations keyed by provider identifier.
    pub providers: HashMap<String, ProviderEntry>,
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
    /// Creates an empty configuration with the documented global defaults.
    pub fn new() -> Self {
        Self {
            timeout: 10,
            health: None,
            engines: HashMap::new(),
            providers: HashMap::new(),
        }
    }

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
        let Self {
            mut timeout,
            mut health,
            mut engines,
            mut providers,
        } = Self::new();

        for block in &doc.blocks {
            match block.name.as_str() {
                "timeout" => {
                    if let Some(v) = block.attributes.get("value") {
                        timeout = value_as_u64(v, "timeout.value")?;
                        if timeout == 0 {
                            return Err(config_value_error(
                                "timeout.value",
                                "an integer greater than zero",
                            ));
                        }
                    }
                }
                "health" => {
                    let max_failures = block
                        .attributes
                        .get("max_failures")
                        .map(|value| value_as_u32(value, "health.max_failures"))
                        .transpose()?
                        .unwrap_or(3);
                    if max_failures == 0 {
                        return Err(config_value_error(
                            "health.max_failures",
                            "an integer greater than zero",
                        ));
                    }
                    health = Some(HealthEntry {
                        max_failures,
                        suspend_seconds: block
                            .attributes
                            .get("suspend_seconds")
                            .map(|value| value_as_u64(value, "health.suspend_seconds"))
                            .transpose()?
                            .unwrap_or(60),
                    });
                }
                "engine" => {
                    // Engine blocks have the engine shortcut as a label
                    if let Some(shortcut) = block.labels.first() {
                        let prefix = format!("engine \"{shortcut}\"");
                        let enabled = block
                            .attributes
                            .get("enabled")
                            .map(|value| value_as_bool(value, &format!("{prefix}.enabled")))
                            .transpose()?
                            .unwrap_or(true);
                        let weight = block
                            .attributes
                            .get("weight")
                            .map(|value| value_as_f64(value, &format!("{prefix}.weight")))
                            .transpose()?
                            .unwrap_or(1.0);
                        if !weight.is_finite() || weight <= 0.0 {
                            return Err(config_value_error(
                                &format!("{prefix}.weight"),
                                "a finite number greater than zero",
                            ));
                        }
                        let engine_timeout = block
                            .attributes
                            .get("timeout")
                            .map(|value| value_as_u64(value, &format!("{prefix}.timeout")))
                            .transpose()?;
                        if engine_timeout == Some(0) {
                            return Err(config_value_error(
                                &format!("{prefix}.timeout"),
                                "an integer greater than zero",
                            ));
                        }
                        let entry = EngineEntry {
                            enabled,
                            weight,
                            timeout: engine_timeout,
                        };
                        if engines.insert(shortcut.clone(), entry).is_some() {
                            return Err(SearchError::Parse(format!(
                                "duplicate engine block for \"{shortcut}\""
                            )));
                        }
                    }
                }
                "provider" => {
                    let (provider, entry) = provider::parse_provider_block(block)?;
                    if providers.insert(provider.clone(), entry).is_some() {
                        return Err(SearchError::Parse(format!(
                            "duplicate provider block for \"{provider}\""
                        )));
                    }
                }
                _ => {
                    // Ignore unknown blocks
                }
            }
        }

        if let Some(duplicate) = providers
            .keys()
            .find(|provider| engines.contains_key(provider.as_str()))
        {
            return Err(SearchError::Parse(format!(
                "search source \"{duplicate}\" cannot be configured as both an engine and a provider"
            )));
        }

        Ok(Self {
            timeout,
            health,
            engines,
            providers,
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
        let mut engines: Vec<_> = self
            .engines
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .map(|(shortcut, _)| shortcut.as_str())
            .collect();
        engines.sort_unstable();
        engines
    }

    /// Returns enabled native provider identifiers in deterministic order.
    pub fn enabled_providers(&self) -> Vec<&str> {
        let mut providers: Vec<_> = self
            .providers
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .map(|(provider, _)| provider.as_str())
            .collect();
        providers.sort_unstable();
        providers
    }

    /// Returns all enabled engine and provider identifiers.
    pub fn enabled_sources(&self) -> Vec<&str> {
        let mut sources = self.enabled_engines();
        sources.extend(self.enabled_providers());
        sources.sort_unstable();
        sources
    }

    /// Returns the configuration entry for an engine shortcut or known alias.
    pub fn engine_entry(&self, shortcut: &str) -> Option<&EngineEntry> {
        self.engines.get(shortcut).or_else(|| {
            engine_aliases(shortcut)
                .iter()
                .find_map(|alias| self.engines.get(*alias))
        })
    }

    /// Returns a typed native provider entry.
    pub fn provider_entry(&self, provider: &str) -> Option<&ProviderEntry> {
        self.providers.get(provider)
    }

    /// Creates a configured native provider engine.
    ///
    /// Returns `None` when the provider is not present in this configuration.
    pub fn create_provider_engine(&self, provider: &str) -> crate::Result<Option<ProviderEngine>> {
        let Some(entry) = self.provider_entry(provider) else {
            return Ok(None);
        };
        let engine = entry.create_engine()?;
        let config = self.apply_engine_config(engine.config().clone());
        Ok(Some(engine.with_config(config)))
    }

    /// Applies top-level and per-engine ACL settings to an `EngineConfig`.
    ///
    /// The top-level `timeout` becomes the default timeout for every configured
    /// search run. Per-engine `timeout` overrides it when present.
    pub fn apply_engine_config(&self, mut config: EngineConfig) -> EngineConfig {
        config.timeout = self.timeout;

        if let Some(entry) = self.engine_entry(&config.shortcut) {
            config.enabled = entry.enabled;
            config.weight = entry.weight;
            if let Some(timeout) = entry.timeout {
                config.timeout = timeout;
            }
        } else if let Some(entry) = self.provider_entry(&config.shortcut) {
            config.enabled = entry.enabled;
            config.weight = entry.weight;
            if let Some(timeout) = entry.timeout {
                config.timeout = timeout;
            }
        }

        config
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn engine_aliases(shortcut: &str) -> &'static [&'static str] {
    match shortcut {
        "ddg" => &["duckduckgo"],
        "duckduckgo" => &["ddg"],
        "wiki" => &["wikipedia"],
        "wikipedia" => &["wiki"],
        "g" => &["google"],
        "google" => &["g"],
        "360" => &["so360"],
        "so360" => &["360"],
        _ => &[],
    }
}

/// Extract a lossless u64 from an ACL value.
fn value_as_u64(value: &Value, attribute: &str) -> crate::Result<u64> {
    match value {
        Value::Number(number)
            if number.is_finite()
                && *number >= 0.0
                && number.fract() == 0.0
                && *number <= MAX_ACL_EXACT_INTEGER =>
        {
            Ok(*number as u64)
        }
        _ => Err(config_value_error(attribute, "a non-negative integer")),
    }
}

/// Extract a lossless u32 from an ACL value.
fn value_as_u32(value: &Value, attribute: &str) -> crate::Result<u32> {
    match value {
        Value::Number(number)
            if number.is_finite()
                && *number >= 0.0
                && number.fract() == 0.0
                && *number <= f64::from(u32::MAX) =>
        {
            Ok(*number as u32)
        }
        _ => Err(config_value_error(attribute, "a non-negative integer")),
    }
}

/// Extract a f64 from a Value.
fn value_as_f64(value: &Value, attribute: &str) -> crate::Result<f64> {
    match value {
        Value::Number(number) => Ok(*number),
        _ => Err(config_value_error(attribute, "a number")),
    }
}

/// Extract a bool from a Value.
fn value_as_bool(value: &Value, attribute: &str) -> crate::Result<bool> {
    match value {
        Value::Bool(value) => Ok(*value),
        _ => Err(config_value_error(attribute, "a boolean")),
    }
}

fn config_value_error(attribute: &str, expected: &str) -> SearchError {
    SearchError::Parse(format!("attribute \"{attribute}\" must be {expected}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_uses_documented_empty_defaults() {
        let config = SearchConfig::new();

        assert_eq!(config.timeout, 10);
        assert!(config.health.is_none());
        assert!(config.engines.is_empty());
        assert!(config.providers.is_empty());
    }

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

    #[test]
    fn test_engine_entry_uses_aliases() {
        let config = SearchConfig::parse(
            r#"
            engine "google" {
                enabled = false
                weight = 2.0
            }
            "#,
        )
        .unwrap();

        let entry = config.engine_entry("g").unwrap();
        assert!(!entry.enabled);
        assert_eq!(entry.weight, 2.0);
    }

    #[test]
    fn test_apply_engine_config_uses_top_level_timeout() {
        let config = SearchConfig::parse(
            r#"
            timeout {
                value = 12
            }
            "#,
        )
        .unwrap();
        let engine = EngineConfig {
            name: "DuckDuckGo".to_string(),
            shortcut: "ddg".to_string(),
            timeout: 5,
            ..Default::default()
        };

        let applied = config.apply_engine_config(engine);
        assert_eq!(applied.timeout, 12);
        assert!(applied.enabled);
        assert_eq!(applied.weight, 1.0);
    }

    #[test]
    fn test_apply_engine_config_uses_engine_override() {
        let config = SearchConfig::parse(
            r#"
            timeout {
                value = 12
            }

            engine "ddg" {
                enabled = false
                weight = 1.7
                timeout = 3
            }
            "#,
        )
        .unwrap();
        let engine = EngineConfig {
            name: "DuckDuckGo".to_string(),
            shortcut: "ddg".to_string(),
            timeout: 5,
            ..Default::default()
        };

        let applied = config.apply_engine_config(engine);
        assert_eq!(applied.timeout, 3);
        assert!(!applied.enabled);
        assert_eq!(applied.weight, 1.7);
    }

    #[test]
    fn strict_top_level_and_engine_values_reject_lossy_or_unsafe_configuration() {
        for acl in [
            r#"timeout { value = -1 }"#,
            r#"timeout { value = 1.5 }"#,
            r#"timeout { value = 9007199254740992 }"#,
            r#"timeout { value = 0 }"#,
            r#"health { max_failures = 0 }"#,
            r#"health { max_failures = 1.5 }"#,
            r#"health { max_failures = "3" }"#,
            r#"health { suspend_seconds = -1 }"#,
            r#"engine "ddg" { enabled = "true" }"#,
            r#"engine "ddg" { weight = 0 }"#,
            r#"engine "ddg" { weight = -1 }"#,
            r#"engine "ddg" { weight = "1" }"#,
            r#"engine "ddg" { timeout = 0 }"#,
            r#"engine "ddg" { timeout = 1.5 }"#,
            r#"engine "ddg" {} engine "ddg" {}"#,
        ] {
            assert!(SearchConfig::parse(acl).is_err(), "{acl}");
        }
    }
}
