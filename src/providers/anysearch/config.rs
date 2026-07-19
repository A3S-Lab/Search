//! Typed AnySearch routing and provider configuration.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use url::Url;

use super::super::http::validate_provider_endpoint;
use super::super::{CredentialSource, ProviderHttpConfig};
use super::PROVIDER_ID;
use crate::{ProviderError, ProviderErrorKind, Result, SearchError};

pub(super) const DEFAULT_ENDPOINT: &str = "https://api.anysearch.com/mcp";
const MAX_SUB_DOMAIN_PARAM_DEPTH: usize = 16;
const MAX_SUB_DOMAIN_PARAM_NODES: usize = 1_024;
const MAX_SUB_DOMAIN_PARAM_OBJECT_FIELDS: usize = 128;
const MAX_SUB_DOMAIN_PARAM_ARRAY_ITEMS: usize = 256;
const MAX_SUB_DOMAIN_PARAM_KEY_CHARS: usize = 128;
const MAX_SUB_DOMAIN_PARAM_STRING_CHARS: usize = 16 * 1_024;
const MAX_SUB_DOMAIN_PARAM_TOTAL_TEXT_CHARS: usize = 64 * 1_024;

/// AnySearch vertical domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AnySearchDomain {
    /// General web search.
    General,
    /// Resource search.
    Resource,
    /// Public social-media search.
    SocialMedia,
    /// Finance search.
    Finance,
    /// Academic search.
    Academic,
    /// Legal search.
    Legal,
    /// Health search.
    Health,
    /// Business search.
    Business,
    /// Security search.
    Security,
    /// Intellectual-property search.
    Ip,
    /// Software and documentation search.
    Code,
    /// Energy search.
    Energy,
    /// Environment search.
    Environment,
    /// Agriculture search.
    Agriculture,
    /// Travel search.
    Travel,
    /// Film search.
    Film,
    /// Gaming search.
    Gaming,
}

impl AnySearchDomain {
    /// Returns the wire-format domain name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Resource => "resource",
            Self::SocialMedia => "social_media",
            Self::Finance => "finance",
            Self::Academic => "academic",
            Self::Legal => "legal",
            Self::Health => "health",
            Self::Business => "business",
            Self::Security => "security",
            Self::Ip => "ip",
            Self::Code => "code",
            Self::Energy => "energy",
            Self::Environment => "environment",
            Self::Agriculture => "agriculture",
            Self::Travel => "travel",
            Self::Film => "film",
            Self::Gaming => "gaming",
        }
    }
}

impl fmt::Display for AnySearchDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validated AnySearch `{domain}.{sub_domain}` routing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AnySearchSubDomain(String);

impl AnySearchSubDomain {
    /// Parses and normalizes a sub-domain routing key.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into().trim().to_ascii_lowercase();
        let Some((domain, sub_domain)) = value.split_once('.') else {
            return Err(invalid_config(
                "AnySearch sub_domain must use the {domain}.{sub_domain} format",
            ));
        };
        if sub_domain.contains('.')
            || !valid_route_segment(domain)
            || !valid_route_segment(sub_domain)
        {
            return Err(invalid_config(
                "AnySearch sub_domain must use the {domain}.{sub_domain} format",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the normalized routing key.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn domain(&self) -> &str {
        self.0.split_once('.').map_or("", |(domain, _)| domain)
    }
}

impl fmt::Display for AnySearchSubDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AnySearchSubDomain {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

fn valid_route_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 64
        && segment.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
}

/// Typed AnySearch request defaults and credentials.
#[derive(Debug, Clone)]
pub struct AnySearchConfig {
    pub(super) endpoint: Url,
    pub(super) api_key: CredentialSource,
    pub(super) max_results: u8,
    pub(super) domain: Option<AnySearchDomain>,
    pub(super) sub_domain: Option<AnySearchSubDomain>,
    pub(super) sub_domain_params: BTreeMap<String, Value>,
    pub(super) http: ProviderHttpConfig,
}

impl AnySearchConfig {
    /// Creates the official AnySearch MCP configuration.
    ///
    /// `ANYSEARCH_API_KEY` is optional. When it is absent, requests use
    /// AnySearch's documented anonymous mode.
    pub fn new() -> Result<Self> {
        let endpoint = Url::parse(DEFAULT_ENDPOINT).map_err(|_| {
            ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::InvalidRequest,
                "built-in AnySearch endpoint is invalid",
            )
        })?;
        Ok(Self {
            endpoint,
            api_key: CredentialSource::environment("ANYSEARCH_API_KEY"),
            max_results: 10,
            domain: None,
            sub_domain: None,
            sub_domain_params: BTreeMap::new(),
            http: ProviderHttpConfig::default(),
        })
    }

    /// Replaces the API endpoint.
    pub fn with_endpoint(mut self, endpoint: Url) -> Result<Self> {
        validate_provider_endpoint(PROVIDER_ID, &endpoint)?;
        self.endpoint = endpoint;
        Ok(self)
    }

    /// Replaces the credential source.
    pub fn with_api_key(mut self, api_key: CredentialSource) -> Self {
        self.api_key = api_key;
        self
    }

    /// Sets the maximum result count in the documented `1..=10` range.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=10).contains(&max_results) {
            return Err(invalid_config(
                "AnySearch max_results must be between 1 and 10",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Sets an optional vertical domain.
    pub fn with_domain(mut self, domain: AnySearchDomain) -> Self {
        self.domain = Some(domain);
        self
    }

    /// Sets an optional vertical sub-domain.
    pub fn with_sub_domain(mut self, sub_domain: AnySearchSubDomain) -> Self {
        self.sub_domain = Some(sub_domain);
        self
    }

    /// Replaces the vertical sub-domain parameters.
    pub fn with_sub_domain_params(mut self, params: BTreeMap<String, Value>) -> Self {
        self.sub_domain_params = params;
        self
    }

    /// Replaces shared HTTP safety limits.
    pub fn with_http_config(mut self, http: ProviderHttpConfig) -> Self {
        self.http = http;
        self
    }

    /// Returns the configured endpoint.
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Returns the maximum result count.
    pub const fn max_results(&self) -> u8 {
        self.max_results
    }

    /// Returns the configured vertical domain.
    pub const fn domain(&self) -> Option<AnySearchDomain> {
        self.domain
    }

    /// Returns the configured vertical sub-domain.
    pub fn sub_domain(&self) -> Option<&AnySearchSubDomain> {
        self.sub_domain.as_ref()
    }

    /// Returns the configured vertical routing parameters.
    pub fn sub_domain_params(&self) -> &BTreeMap<String, Value> {
        &self.sub_domain_params
    }

    pub(super) fn validate(&self) -> Result<()> {
        if let Some(sub_domain) = &self.sub_domain {
            let Some(domain) = self.domain else {
                return Err(invalid_config(
                    "AnySearch sub_domain requires a matching domain",
                ));
            };
            if sub_domain.domain() != domain.as_str() {
                return Err(invalid_config(
                    "AnySearch sub_domain prefix must match the configured domain",
                ));
            }
        }
        if !self.sub_domain_params.is_empty() && self.sub_domain.is_none() {
            return Err(invalid_config(
                "AnySearch sub_domain_params requires sub_domain",
            ));
        }
        validate_sub_domain_params(&self.sub_domain_params)?;
        Ok(())
    }
}

fn validate_sub_domain_params(params: &BTreeMap<String, Value>) -> Result<()> {
    if params.len() > MAX_SUB_DOMAIN_PARAM_OBJECT_FIELDS {
        return Err(invalid_config(
            "AnySearch sub_domain_params contains too many object fields",
        ));
    }

    let mut nodes = 0usize;
    let mut total_text_chars = 0usize;
    let mut stack: Vec<_> = params.values().map(|value| (value, 1usize)).collect();
    for key in params.keys() {
        validate_param_key(key)?;
        total_text_chars = total_text_chars.saturating_add(key.chars().count());
    }

    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > MAX_SUB_DOMAIN_PARAM_NODES {
            return Err(invalid_config(
                "AnySearch sub_domain_params contains too many values",
            ));
        }
        if depth > MAX_SUB_DOMAIN_PARAM_DEPTH {
            return Err(invalid_config(
                "AnySearch sub_domain_params exceeds the maximum nesting depth",
            ));
        }

        match value {
            Value::String(value) => {
                let chars = value.chars().count();
                if chars > MAX_SUB_DOMAIN_PARAM_STRING_CHARS {
                    return Err(invalid_config(
                        "AnySearch sub_domain_params contains an oversized string",
                    ));
                }
                total_text_chars = total_text_chars.saturating_add(chars);
            }
            Value::Array(values) => {
                if values.len() > MAX_SUB_DOMAIN_PARAM_ARRAY_ITEMS {
                    return Err(invalid_config(
                        "AnySearch sub_domain_params contains an oversized array",
                    ));
                }
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > MAX_SUB_DOMAIN_PARAM_OBJECT_FIELDS {
                    return Err(invalid_config(
                        "AnySearch sub_domain_params contains too many object fields",
                    ));
                }
                for (key, value) in values {
                    validate_param_key(key)?;
                    total_text_chars = total_text_chars.saturating_add(key.chars().count());
                    stack.push((value, depth + 1));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }

        if total_text_chars > MAX_SUB_DOMAIN_PARAM_TOTAL_TEXT_CHARS {
            return Err(invalid_config(
                "AnySearch sub_domain_params contains too much text",
            ));
        }
    }

    Ok(())
}

fn validate_param_key(key: &str) -> Result<()> {
    if key.trim().is_empty()
        || key.chars().count() > MAX_SUB_DOMAIN_PARAM_KEY_CHARS
        || key.chars().any(char::is_control)
    {
        return Err(invalid_config(
            "AnySearch sub_domain_params keys must be non-empty and bounded",
        ));
    }
    Ok(())
}

fn invalid_config(message: &str) -> SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidRequest, message).into()
}
