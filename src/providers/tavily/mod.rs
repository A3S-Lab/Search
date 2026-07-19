//! Native Tavily Search API provider.

use std::collections::BTreeMap;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;

use super::http::{
    bearer_header, insert_header, secret_header, validate_provider_endpoint, ProviderHttpClient,
};
use super::metadata::sanitize_provider_metadata;
use super::protocol::{non_empty, sanitize_provider_text_with_secrets};
use super::{
    ProviderCapabilities, ProviderDescriptor, ProviderReadiness, ProviderReport, ProviderRequest,
    ProviderResponse, SearchProvider,
};
use crate::{ProviderError, ProviderErrorKind, Result, SafeSearch, SearchUsage};

mod config;
mod error;
mod request;
mod response;
mod types;

pub use config::TavilyConfig;
pub use types::{
    TavilyAnswer, TavilyCountry, TavilyDate, TavilyRawContent, TavilySearchDepth, TavilyTopic,
};

use error::{invalid_request, tavily_error};
use request::{time_range_name, TavilyRequest};
use response::{adapt_images, adapt_results, FlexibleNumber, TavilyResponse, TavilyUsage};

const PROVIDER_ID: &str = "tavily";
const PROJECT_HEADER: &str = "x-project-id";
const ACCESS_MODE_HEADER: &str = "x-tavily-access-mode";

/// Native Rust implementation of the Tavily Search API.
#[derive(Debug)]
pub struct TavilyProvider {
    config: TavilyConfig,
    client: ProviderHttpClient,
}

impl TavilyProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: TavilyConfig) -> Result<Self> {
        config.validate()?;
        validate_provider_endpoint(PROVIDER_ID, &config.endpoint)?;
        let client = ProviderHttpClient::new(PROVIDER_ID, config.http)?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `TAVILY_API_KEY` and optional
    /// `TAVILY_PROJECT`.
    pub fn from_env() -> Result<Self> {
        Self::new(TavilyConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for TavilyProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "Tavily",
            "https://www.tavily.com/",
            ProviderCapabilities::new()
                .with_anonymous(true)
                .with_safe_search(true)
                .with_time_range(true)
                .with_answers(true)
                .with_images(true)
                .with_full_text(true)
                .with_usage(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        let api_key = match self.config.api_key.resolve(PROVIDER_ID) {
            Ok(Some(api_key)) => api_key,
            Ok(None) => {
                return ProviderReadiness::Ready {
                    authentication: super::ProviderAuthentication::Anonymous,
                };
            }
            Err(_) => return ProviderReadiness::InvalidCredential,
        };
        if bearer_header(PROVIDER_ID, &api_key).is_err() {
            return ProviderReadiness::InvalidCredential;
        }

        match self.config.project.resolve(PROVIDER_ID) {
            Ok(Some(project))
                if secret_header(PROVIDER_ID, project.expose().to_string()).is_err() =>
            {
                ProviderReadiness::InvalidCredential
            }
            Ok(_) => ProviderReadiness::Ready {
                authentication: super::ProviderAuthentication::Authenticated,
            },
            Err(_) => ProviderReadiness::InvalidCredential,
        }
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        if request.query.trim().is_empty() {
            return Err(invalid_request("Tavily query must not be empty"));
        }
        let api_key = self.config.api_key.resolve(PROVIDER_ID)?;
        let authenticated = api_key.is_some();
        let project = if authenticated {
            self.config.project.resolve(PROVIDER_ID)?
        } else {
            None
        };
        let mut secrets = Vec::new();
        if let Some(api_key) = api_key.as_ref() {
            secrets.push(api_key.expose());
        }
        if let Some(project) = project.as_ref() {
            secrets.push(project.expose());
        }
        let safe_search = self.config.safe_search == Some(true)
            || matches!(
                request.safe_search,
                SafeSearch::Moderate | SafeSearch::Strict
            );
        if safe_search && !authenticated {
            return Err(ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::Permission,
                "Tavily safe_search requires an authenticated enterprise plan",
            )
            .into());
        }
        if safe_search && !self.config.search_depth.supports_safe_search() {
            return Err(ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::InvalidRequest,
                "Tavily safe_search is not supported for fast or ultra-fast depth",
            )
            .into());
        }
        let request_search_depth = self.config.request_search_depth(safe_search);
        let request_topic = self.config.request_topic();

        let mut headers = HeaderMap::new();
        if let Some(api_key) = api_key.as_ref() {
            headers.insert(AUTHORIZATION, bearer_header(PROVIDER_ID, api_key)?);
            if let Some(project) = project.as_ref() {
                insert_header(
                    PROVIDER_ID,
                    &mut headers,
                    PROJECT_HEADER,
                    secret_header(PROVIDER_ID, project.expose().to_string())?,
                )?;
            }
        } else {
            insert_header(
                PROVIDER_ID,
                &mut headers,
                ACCESS_MODE_HEADER,
                HeaderValue::from_static("keyless"),
            )?;
        }

        let payload = TavilyRequest {
            query: request.query.trim(),
            search_depth: request_search_depth,
            chunks_per_source: self.config.chunks_per_source,
            max_results: self.config.max_results,
            topic: request_topic,
            time_range: request.time_range.map(time_range_name),
            include_answer: self.config.include_answer,
            include_raw_content: self.config.include_raw_content,
            include_domains: (!self.config.include_domains.is_empty())
                .then_some(self.config.include_domains.as_slice()),
            exclude_domains: (!self.config.exclude_domains.is_empty())
                .then_some(self.config.exclude_domains.as_slice()),
            start_date: self.config.start_date.as_ref().map(TavilyDate::as_str),
            end_date: self.config.end_date.as_ref().map(TavilyDate::as_str),
            country: self.config.country.as_ref().map(TavilyCountry::as_str),
            auto_parameters: self.config.auto_parameters,
            exact_match: self.config.exact_match,
            include_usage: self.config.include_usage,
            include_images: self.config.include_images,
            include_image_descriptions: self.config.include_image_descriptions,
            include_favicon: self.config.include_favicon,
            safe_search: safe_search.then_some(true),
        };
        let response = self
            .client
            .post_json(&self.config.endpoint, headers, &payload)
            .await?;
        if !response.status.is_success() {
            return Err(tavily_error(&response, &secrets));
        }

        let payload: TavilyResponse = serde_json::from_slice(&response.body).map_err(|_| {
            ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::InvalidResponse,
                "Tavily success response did not match its contract",
            )
        })?;

        let results = adapt_results(payload.results, self.config.max_results)?;
        let request_id = non_empty(payload.request_id)
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .or_else(|| {
                response
                    .header("x-request-id")
                    .map(|value| sanitize_provider_text_with_secrets(value, 128, &secrets))
            });
        let answers = non_empty(payload.answer)
            .map(|answer| sanitize_provider_text_with_secrets(&answer, 16 * 1024, &secrets))
            .filter(|answer| !answer.is_empty())
            .into_iter()
            .collect();
        let reported_search_depth = self
            .config
            .auto_parameters
            .then(|| {
                payload
                    .auto_parameters
                    .as_ref()
                    .and_then(|parameters| parameters.get("search_depth"))
                    .and_then(Value::as_str)
                    .and_then(documented_search_depth)
            })
            .flatten()
            .or_else(|| request_search_depth.map(TavilySearchDepth::as_str));
        let reported_topic = self
            .config
            .auto_parameters
            .then(|| {
                payload
                    .auto_parameters
                    .as_ref()
                    .and_then(|parameters| parameters.get("topic"))
                    .and_then(Value::as_str)
                    .and_then(documented_topic)
            })
            .flatten()
            .or_else(|| request_topic.map(TavilyTopic::as_str));
        let mut metadata = BTreeMap::new();
        if let Some(auto_parameters) = payload.auto_parameters {
            let auto_parameters = sanitize_provider_metadata(auto_parameters, &secrets);
            metadata.insert("auto_parameters".to_string(), auto_parameters.value);
            if auto_parameters.truncated {
                metadata.insert("auto_parameters_truncated".to_string(), Value::Bool(true));
            }
        }
        metadata.insert(
            "access_mode".to_string(),
            Value::String(
                if authenticated {
                    "authenticated"
                } else {
                    "keyless"
                }
                .to_string(),
            ),
        );
        if let Some(reported_search_depth) = reported_search_depth {
            metadata.insert(
                "search_depth".to_string(),
                Value::String(reported_search_depth.to_string()),
            );
        }
        if let Some(reported_topic) = reported_topic {
            metadata.insert(
                "topic".to_string(),
                Value::String(reported_topic.to_string()),
            );
        }
        Ok(ProviderResponse {
            results,
            answers,
            images: adapt_images(payload.images),
            report: ProviderReport {
                request_id,
                response_time_ms: payload
                    .response_time
                    .and_then(FlexibleNumber::seconds_to_ms),
                usage: payload
                    .usage
                    .and_then(TavilyUsage::credits)
                    .map(|credits| SearchUsage::new().with_credits(credits)),
                metadata,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

fn documented_search_depth(value: &str) -> Option<&'static str> {
    match value {
        "advanced" => Some("advanced"),
        "basic" => Some("basic"),
        "fast" => Some("fast"),
        "ultra-fast" => Some("ultra-fast"),
        _ => None,
    }
}

fn documented_topic(value: &str) -> Option<&'static str> {
    match value {
        "general" => Some("general"),
        "news" => Some("news"),
        "finance" => Some("finance"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderMap;
    use serde_json::Value;

    use super::config::DEFAULT_ENDPOINT;
    use super::*;
    use crate::providers::{CredentialSource, ProviderAuthentication};

    #[test]
    fn wire_modes_match_tavily_contract() {
        assert_eq!(
            serde_json::to_value(TavilyAnswer::None).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            serde_json::to_value(TavilyAnswer::Advanced).unwrap(),
            Value::String("advanced".to_string())
        );
        assert_eq!(
            serde_json::to_value(TavilyRawContent::Markdown).unwrap(),
            Value::String("markdown".to_string())
        );
        assert_eq!(
            serde_json::to_value(TavilySearchDepth::UltraFast).unwrap(),
            Value::String("ultra-fast".to_string())
        );
    }

    #[test]
    fn config_enforces_documented_bounds_and_depth() {
        let config = TavilyConfig::new().unwrap();
        assert_eq!(config.endpoint().as_str(), DEFAULT_ENDPOINT);
        assert_eq!(config.max_results(), 5);
        assert!(!config.include_usage);
        assert_eq!(config.clone().with_max_results(0).unwrap().max_results(), 0);
        assert!(config.clone().with_max_results(21).is_err());
        assert!(config.clone().with_chunks_per_source(4).is_err());
        let invalid = config.with_chunks_per_source(2).unwrap();
        assert!(TavilyProvider::new(invalid).is_err());
    }

    #[test]
    fn auto_parameters_omit_only_unpinned_depth_and_topic_defaults() {
        let automatic = TavilyConfig::new().unwrap().with_auto_parameters(true);
        assert_eq!(automatic.request_search_depth(false), None);
        assert_eq!(automatic.request_topic(), None);
        assert_eq!(
            automatic.request_search_depth(true),
            Some(TavilySearchDepth::Basic)
        );

        let explicit = automatic
            .clone()
            .with_search_depth(TavilySearchDepth::Basic)
            .with_topic(TavilyTopic::General);
        assert_eq!(
            explicit.request_search_depth(false),
            Some(TavilySearchDepth::Basic)
        );
        assert_eq!(explicit.request_topic(), Some(TavilyTopic::General));

        let country = automatic.with_country(TavilyCountry::new("canada").unwrap());
        assert_eq!(country.request_topic(), Some(TavilyTopic::General));
    }

    #[test]
    fn domain_filters_are_normalized_and_validated() {
        let config = TavilyConfig::new()
            .unwrap()
            .with_include_domains(["Docs.Rust-Lang.org", "BÜCHER.example"])
            .unwrap();
        assert_eq!(
            config.include_domains(),
            ["docs.rust-lang.org", "xn--bcher-kva.example"]
        );
        for invalid in [
            "https://example.com/path",
            "user@example.com",
            "example.com?query",
            "-example.com",
            "example-.com",
            "exa_mple.com",
        ] {
            assert!(
                TavilyConfig::new()
                    .unwrap()
                    .with_include_domains([invalid])
                    .is_err(),
                "{invalid}"
            );
        }
        let too_many: Vec<_> = (0..301)
            .map(|index| format!("domain-{index}.example"))
            .collect();
        assert!(TavilyConfig::new()
            .unwrap()
            .with_include_domains(too_many)
            .is_err());
    }

    #[test]
    fn missing_key_uses_keyless_readiness() {
        let provider = TavilyProvider::new(
            TavilyConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::none()),
        )
        .unwrap();

        assert_eq!(
            provider.readiness(),
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Anonymous,
            }
        );
    }

    #[test]
    fn authenticated_readiness_validates_the_optional_project_source() {
        let provider = TavilyProvider::new(
            TavilyConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::value("valid-key"))
                .with_project(CredentialSource::environment("INVALID=PROJECT")),
        )
        .unwrap();

        assert_eq!(provider.readiness(), ProviderReadiness::InvalidCredential);
    }

    #[test]
    fn anonymous_readiness_ignores_an_unused_invalid_project_source() {
        let provider = TavilyProvider::new(
            TavilyConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::none())
                .with_project(CredentialSource::environment("INVALID=PROJECT")),
        )
        .unwrap();

        assert_eq!(
            provider.readiness(),
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Anonymous,
            }
        );
    }

    #[test]
    fn authenticated_readiness_rejects_header_unsafe_keys_and_projects() {
        let invalid_key = TavilyProvider::new(
            TavilyConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::value("invalid\nkey")),
        )
        .unwrap();
        let invalid_project = TavilyProvider::new(
            TavilyConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::value("valid-key"))
                .with_project(CredentialSource::value("invalid\nproject")),
        )
        .unwrap();

        assert_eq!(
            invalid_key.readiness(),
            ProviderReadiness::InvalidCredential
        );
        assert_eq!(
            invalid_project.readiness(),
            ProviderReadiness::InvalidCredential
        );
    }

    #[test]
    fn validates_dates_country_images_and_safe_search_cross_fields() {
        let reversed_dates = TavilyConfig::new()
            .unwrap()
            .with_start_date(TavilyDate::new("2026-07-20").unwrap())
            .with_end_date(TavilyDate::new("2026-01-01").unwrap());
        assert!(TavilyProvider::new(reversed_dates).is_err());

        let news_country = TavilyConfig::new()
            .unwrap()
            .with_topic(TavilyTopic::News)
            .with_country(TavilyCountry::new("united states").unwrap());
        assert!(TavilyProvider::new(news_country).is_err());

        let descriptions_without_images =
            TavilyConfig::new().unwrap().with_image_descriptions(true);
        assert!(TavilyProvider::new(descriptions_without_images).is_err());

        let unsupported_safe_search = TavilyConfig::new()
            .unwrap()
            .with_search_depth(TavilySearchDepth::UltraFast)
            .with_safe_search(true);
        assert!(TavilyProvider::new(unsupported_safe_search).is_err());
    }

    #[test]
    fn plan_limit_statuses_map_to_quota() {
        for status in [402, 432, 433] {
            let response = super::super::http::ProviderHttpResponse {
                status: reqwest::StatusCode::from_u16(status).unwrap(),
                headers: HeaderMap::new(),
                body: br#"{"detail":"usage limit","request_id":"req-limit"}"#.to_vec(),
            };
            let error = tavily_error(&response, &[]);

            assert_eq!(error.kind(), "provider_quota");
            assert!(error.to_string().contains("req-limit"));
        }
    }
}
