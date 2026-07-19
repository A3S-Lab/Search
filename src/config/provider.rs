//! Typed ACL configuration for native search providers.

use a3s_acl::ast::Block;

use crate::providers::{
    AnySearchConfig, AnySearchProvider, BuiltinProvider, ProviderEngine, TavilyConfig,
    TavilyProvider,
};
use crate::{Engine, Result, SearchError};

mod anysearch;
mod common;
mod tavily;

use common::{
    config_error, optional_bool, optional_number, optional_u64, reject_unknown_attributes,
};

#[cfg(test)]
use common::acl_object_to_json_map;

const COMMON_ATTRIBUTES: &[&str] = &[
    "enabled",
    "weight",
    "timeout",
    "endpoint",
    "api_key",
    "http_timeout",
    "max_response_bytes",
];

const ANYSEARCH_ATTRIBUTES: &[&str] = &["max_results", "domain", "sub_domain", "sub_domain_params"];

const TAVILY_ATTRIBUTES: &[&str] = &[
    "project",
    "search_depth",
    "chunks_per_source",
    "max_results",
    "topic",
    "include_answer",
    "include_raw_content",
    "include_domains",
    "exclude_domains",
    "start_date",
    "end_date",
    "country",
    "auto_parameters",
    "exact_match",
    "include_usage",
    "include_images",
    "include_image_descriptions",
    "include_favicon",
    "safe_search",
];

const MAX_JSON_DEPTH: usize = 32;

/// Provider-specific configuration parsed from an ACL `provider` block.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ProviderSettings {
    /// Native AnySearch configuration.
    AnySearch(AnySearchConfig),
    /// Native Tavily configuration.
    Tavily(TavilyConfig),
}

impl ProviderSettings {
    /// Returns the stable provider identifier.
    pub const fn id(&self) -> &'static str {
        match self {
            Self::AnySearch(_) => "anysearch",
            Self::Tavily(_) => "tavily",
        }
    }

    /// Creates an engine backed by the configured provider.
    pub fn create_engine(&self) -> Result<ProviderEngine> {
        match self {
            Self::AnySearch(config) => {
                Ok(ProviderEngine::new(AnySearchProvider::new(config.clone())?))
            }
            Self::Tavily(config) => Ok(ProviderEngine::new(TavilyProvider::new(config.clone())?)),
        }
    }
}

/// One typed provider entry from search configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProviderEntry {
    /// Whether the provider participates in configured searches.
    pub enabled: bool,
    /// Ranking influence for results returned by this provider.
    pub weight: f64,
    /// Per-provider orchestration timeout in seconds.
    pub timeout: Option<u64>,
    /// Provider-specific validated settings.
    pub settings: ProviderSettings,
}

impl ProviderEntry {
    /// Returns the stable provider identifier.
    pub const fn id(&self) -> &'static str {
        self.settings.id()
    }

    /// Creates a provider engine with this entry's common engine settings.
    pub fn create_engine(&self) -> Result<ProviderEngine> {
        let engine = self.settings.create_engine()?;
        let mut config = engine.config().clone();
        config.enabled = self.enabled;
        config.weight = self.weight;
        if let Some(timeout) = self.timeout {
            config.timeout = timeout;
        }
        Ok(engine.with_config(config))
    }
}

pub(super) fn parse_provider_block(block: &Block) -> Result<(String, ProviderEntry)> {
    let provider = match block.labels.as_slice() {
        [provider] => provider.as_str(),
        _ => {
            return Err(SearchError::Parse(
                "provider blocks require exactly one provider identifier label".to_string(),
            ));
        }
    };
    if !block.blocks.is_empty() {
        return Err(config_error(
            provider,
            "nested blocks are not supported; use typed attributes",
        ));
    }

    let builtin = BuiltinProvider::from_id(provider).ok_or_else(|| {
        config_error(
            provider,
            "unknown provider; supported providers are anysearch and tavily",
        )
    })?;
    let provider_attributes = match builtin {
        BuiltinProvider::AnySearch => ANYSEARCH_ATTRIBUTES,
        BuiltinProvider::Tavily => TAVILY_ATTRIBUTES,
    };
    reject_unknown_attributes(block, provider, provider_attributes)?;

    let enabled = optional_bool(block, provider, "enabled")?.unwrap_or(true);
    let weight = optional_number(block, provider, "weight")?.unwrap_or(1.0);
    if !weight.is_finite() || weight <= 0.0 {
        return Err(config_error(
            provider,
            "attribute \"weight\" must be a finite number greater than zero",
        ));
    }
    let timeout = optional_u64(block, provider, "timeout")?;
    if timeout == Some(0) {
        return Err(config_error(
            provider,
            "attribute \"timeout\" must be greater than zero",
        ));
    }

    let settings = match builtin {
        BuiltinProvider::AnySearch => {
            ProviderSettings::AnySearch(anysearch::parse(block, provider)?)
        }
        BuiltinProvider::Tavily => ProviderSettings::Tavily(tavily::parse(block, provider)?),
    };

    Ok((
        provider.to_string(),
        ProviderEntry {
            enabled,
            weight,
            timeout,
            settings,
        },
    ))
}

#[cfg(test)]
mod tests {
    use a3s_acl::ast::Value;
    use serde_json::Value as JsonValue;

    use super::*;
    use crate::providers::{ProviderAuthentication, ProviderReadiness};
    use crate::SearchConfig;

    #[test]
    fn parses_typed_provider_blocks_and_redacts_static_credentials() {
        let config = SearchConfig::parse(
            r#"
            provider "anysearch" {
                enabled = true
                weight = 1.3
                timeout = 8
                api_key = "any-secret"
                max_results = 10
                domain = "code"
                sub_domain = "code.doc"
                sub_domain_params = {
                    library = "tokio"
                    nested = {
                        enabled = true
                        values = [1, 2, null]
                    }
                }
            }

            provider "tavily" {
                api_key = "tvly-secret"
                project = env("TAVILY_PROJECT")
                search_depth = "advanced"
                chunks_per_source = 3
                max_results = 15
                topic = "general"
                include_answer = "advanced"
                include_raw_content = "markdown"
                include_domains = ["docs.rs", "rust-lang.org"]
                exclude_domains = ["example.com"]
                start_date = "2026-01-01"
                end_date = "2026-07-20"
                country = "united states"
                auto_parameters = true
                exact_match = false
                include_usage = true
                include_images = true
                include_image_descriptions = true
                include_favicon = true
                safe_search = true
            }
            "#,
        )
        .unwrap();

        assert_eq!(config.providers.len(), 2);
        let anysearch = config.provider_entry("anysearch").unwrap();
        assert_eq!(anysearch.weight, 1.3);
        assert_eq!(anysearch.timeout, Some(8));
        assert_eq!(anysearch.id(), "anysearch");
        assert!(matches!(
            anysearch.create_engine().unwrap().readiness(),
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Authenticated
            }
        ));
        let debug = format!("{config:?}");
        assert!(!debug.contains("any-secret"));
        assert!(!debug.contains("tvly-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn env_credentials_and_keyless_defaults_have_typed_readiness() {
        let config = SearchConfig::parse(
            r#"
            provider "anysearch" {
                api_key = null
            }
            provider "tavily" {
                api_key = env("A3S_SEARCH_CONFIG_TEST_KEY_THAT_MUST_NOT_EXIST")
            }
            "#,
        )
        .unwrap();

        assert!(config
            .provider_entry("anysearch")
            .unwrap()
            .create_engine()
            .unwrap()
            .readiness()
            .is_ready());
        assert!(config
            .provider_entry("tavily")
            .unwrap()
            .create_engine()
            .unwrap()
            .readiness()
            .is_ready());
    }

    #[test]
    fn provider_validation_rejects_typos_types_and_cross_field_errors() {
        for acl in [
            r#"provider "tavily" { max_result = 10 }"#,
            r#"provider "tavily" { max_results = "10" }"#,
            r#"provider "tavily" { http_timeout = 9007199254740992 }"#,
            r#"provider "tavily" { http_timeout = 18446744073709551616 }"#,
            r#"provider "anysearch" { max_results = 11 }"#,
            r#"provider "anysearch" { tag = "web.general" }"#,
            r#"provider "anysearch" { sub_domain = "code.doc" }"#,
            r#"provider "tavily" { chunks_per_source = 2 }"#,
            r#"provider "tavily" { include_image_descriptions = true }"#,
            r#"provider "tavily" { topic = "news" country = "united states" }"#,
            r#"provider "tavily" { start_date = "2026-07-20" end_date = "2026-01-01" }"#,
            r#"provider "unknown" {}"#,
            r#"provider "tavily" { api_key = env("bad-name") }"#,
        ] {
            assert!(SearchConfig::parse(acl).is_err(), "{acl}");
        }
    }

    #[test]
    fn acl_objects_convert_recursively_without_evaluating_calls() {
        let value = Value::Object(vec![
            ("string".to_string(), Value::String("value".to_string())),
            (
                "list".to_string(),
                Value::List(vec![Value::Number(1.0), Value::Bool(true), Value::Null]),
            ),
        ]);
        let converted = acl_object_to_json_map("anysearch", "sub_domain_params", &value).unwrap();

        assert_eq!(converted["string"], JsonValue::String("value".to_string()));
        assert_eq!(converted["list"], serde_json::json!([1.0, true, null]));
        assert!(acl_object_to_json_map(
            "anysearch",
            "sub_domain_params",
            &Value::Object(vec![(
                "secret".to_string(),
                Value::Call("env".to_string(), vec![Value::String("SECRET".to_string())])
            )])
        )
        .is_err());
    }
}
