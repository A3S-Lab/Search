//! Native AnySearch JSON-RPC provider.

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::Value;

use super::http::{bearer_header, validate_provider_endpoint, ProviderHttpClient};
use super::protocol::{non_empty, sanitize_provider_text_with_secrets};
use super::{
    ProviderCapabilities, ProviderDescriptor, ProviderReadiness, ProviderReport, ProviderRequest,
    ProviderResponse, SearchProvider,
};
use crate::{ProviderError, ProviderErrorKind, Result};

mod config;
mod error;
mod request;
mod response;

pub use config::{AnySearchConfig, AnySearchDomain, AnySearchSubDomain};

use error::{
    anysearch_http_error, anysearch_rpc_error, classify_failure, invalid_request, invalid_response,
};
use request::{AnySearchArguments, AnySearchCallParams, AnySearchRpcRequest};
use response::{
    first_text_content, parse_search_markdown, parse_structured_content, AnySearchRpcEnvelope,
};

const PROVIDER_ID: &str = "anysearch";
const CLIENT_HEADER: &str = "x-anysearch-client";

/// Native Rust implementation of the AnySearch MCP search tool.
#[derive(Debug)]
pub struct AnySearchProvider {
    config: AnySearchConfig,
    client: ProviderHttpClient,
    next_request_id: AtomicU64,
}

impl AnySearchProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: AnySearchConfig) -> Result<Self> {
        config.validate()?;
        validate_provider_endpoint(PROVIDER_ID, &config.endpoint)?;
        let client = ProviderHttpClient::new(PROVIDER_ID, config.http)?;
        Ok(Self {
            config,
            client,
            next_request_id: AtomicU64::new(1),
        })
    }

    /// Creates a provider using `ANYSEARCH_API_KEY` when available.
    pub fn from_env() -> Result<Self> {
        Self::new(AnySearchConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for AnySearchProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "AnySearch",
            "https://www.anysearch.com/",
            ProviderCapabilities::new()
                .with_anonymous(true)
                .with_full_text(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        match self.config.api_key.resolve(PROVIDER_ID) {
            Ok(None) => ProviderReadiness::Ready {
                authentication: super::ProviderAuthentication::Anonymous,
            },
            Ok(Some(api_key)) if bearer_header(PROVIDER_ID, &api_key).is_ok() => {
                ProviderReadiness::Ready {
                    authentication: super::ProviderAuthentication::Authenticated,
                }
            }
            Ok(Some(_)) | Err(_) => ProviderReadiness::InvalidCredential,
        }
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        if request.query.trim().is_empty() {
            return Err(invalid_request("AnySearch query must not be empty"));
        }

        let api_key = self.config.api_key.resolve(PROVIDER_ID)?;
        let secrets: Vec<_> = api_key
            .as_ref()
            .map(|credential| credential.expose())
            .into_iter()
            .collect();
        let mut headers = HeaderMap::new();
        if let Some(api_key) = api_key.as_ref() {
            headers.insert(AUTHORIZATION, bearer_header(PROVIDER_ID, api_key)?);
        }
        headers.insert(
            HeaderName::from_static(CLIENT_HEADER),
            HeaderValue::from_static(concat!("a3s-search/", env!("CARGO_PKG_VERSION"))),
        );

        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let payload = AnySearchRpcRequest {
            jsonrpc: "2.0",
            id: request_id,
            method: "tools/call",
            params: AnySearchCallParams {
                name: "search",
                arguments: AnySearchArguments {
                    query: request.query.trim(),
                    max_results: self.config.max_results,
                    domain: self.config.domain,
                    sub_domain: self
                        .config
                        .sub_domain
                        .as_ref()
                        .map(AnySearchSubDomain::as_str),
                    sub_domain_params: (!self.config.sub_domain_params.is_empty())
                        .then_some(&self.config.sub_domain_params),
                },
            },
        };
        let response = self
            .client
            .post_json(&self.config.endpoint, headers, &payload)
            .await?;

        if !response.status.is_success() {
            return Err(anysearch_http_error(&response, &secrets));
        }

        let envelope: AnySearchRpcEnvelope =
            serde_json::from_slice(&response.body).map_err(|_| {
                ProviderError::new(
                    PROVIDER_ID,
                    ProviderErrorKind::InvalidResponse,
                    "AnySearch returned malformed JSON-RPC",
                )
            })?;
        if envelope.jsonrpc.as_deref() != Some("2.0") {
            return Err(invalid_response(
                "AnySearch response did not declare JSON-RPC 2.0",
            ));
        }
        if envelope.id.as_ref().and_then(Value::as_u64) != Some(request_id) {
            return Err(invalid_response(
                "AnySearch response identifier did not match the request",
            ));
        }
        if let Some(error) = envelope.error {
            return Err(anysearch_rpc_error(
                error,
                response.header("x-request-id"),
                None,
                response.retry_after_seconds(),
                &secrets,
            ));
        }

        let result = envelope.result.ok_or_else(|| {
            ProviderError::new(
                PROVIDER_ID,
                ProviderErrorKind::InvalidResponse,
                "AnySearch response did not contain a result",
            )
        })?;
        let provider_request_id = result
            .meta
            .as_ref()
            .and_then(|metadata| non_empty(metadata.request_id.clone()))
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .or_else(|| {
                response
                    .header("x-request-id")
                    .map(|value| sanitize_provider_text_with_secrets(value, 128, &secrets))
            });

        if result.is_error {
            let message = first_text_content(&result.content)
                .map(|value| sanitize_provider_text_with_secrets(value, 300, &secrets))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "AnySearch search tool returned an error".to_string());
            let mut error =
                ProviderError::new(PROVIDER_ID, classify_failure(None, None, &message), message);
            if let Some(request_id) = provider_request_id {
                error = error.with_request_id(request_id);
            }
            return Err(error.into());
        }

        let parsed = match result
            .structured_content
            .as_ref()
            .and_then(|content| parse_structured_content(content, self.config.max_results))
        {
            Some(parsed) => parsed,
            None => {
                let markdown = first_text_content(&result.content).ok_or_else(|| {
                    ProviderError::new(
                        PROVIDER_ID,
                        ProviderErrorKind::InvalidResponse,
                        "AnySearch response did not contain searchable content",
                    )
                })?;
                parse_search_markdown(markdown, self.config.max_results)?
            }
        };

        Ok(ProviderResponse {
            results: parsed.results,
            report: ProviderReport {
                request_id: provider_request_id,
                total_results: parsed.total_results,
                response_time_ms: parsed.response_time_ms,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::config::DEFAULT_ENDPOINT;
    use super::response::{parse_search_markdown, AnySearchRpcError};
    use super::*;
    use crate::providers::{CredentialSource, ProviderAuthentication};
    use crate::SearchError;

    #[test]
    fn sub_domain_validation_normalizes_and_checks_shape() {
        assert_eq!(
            AnySearchSubDomain::new("Code.Doc").unwrap().as_str(),
            "code.doc"
        );
        assert!(AnySearchSubDomain::new("missing-dot").is_err());
        assert!(AnySearchSubDomain::new("too.many.parts").is_err());
        assert!(AnySearchSubDomain::new("bad space.doc").is_err());
    }

    #[test]
    fn sub_domain_deserialization_preserves_validation_and_normalization() {
        let sub_domain: AnySearchSubDomain = serde_json::from_str("\"Code.Doc\"").unwrap();

        assert_eq!(sub_domain.as_str(), "code.doc");
        assert!(serde_json::from_str::<AnySearchSubDomain>("\"missing-dot\"").is_err());
    }

    #[test]
    fn config_enforces_official_result_bounds_and_endpoint() {
        let config = AnySearchConfig::new().unwrap();
        assert_eq!(config.max_results(), 10);
        assert_eq!(config.endpoint().as_str(), DEFAULT_ENDPOINT);
        assert!(config.clone().with_max_results(0).is_err());
        assert!(config.with_max_results(11).is_err());
    }

    #[test]
    fn config_enforces_vertical_route_invariants() {
        let missing_domain = AnySearchConfig::new()
            .unwrap()
            .with_sub_domain(AnySearchSubDomain::new("code.doc").unwrap());
        assert!(AnySearchProvider::new(missing_domain).is_err());

        let wrong_domain = AnySearchConfig::new()
            .unwrap()
            .with_domain(AnySearchDomain::Finance)
            .with_sub_domain(AnySearchSubDomain::new("code.doc").unwrap());
        assert!(AnySearchProvider::new(wrong_domain).is_err());

        let mut params = BTreeMap::new();
        params.insert("symbol".to_string(), Value::String("AAPL".to_string()));
        let missing_sub_domain = AnySearchConfig::new()
            .unwrap()
            .with_domain(AnySearchDomain::Finance)
            .with_sub_domain_params(params);
        assert!(AnySearchProvider::new(missing_sub_domain).is_err());
    }

    #[test]
    fn config_bounds_nested_sub_domain_parameters() {
        let config_with_params = |params| {
            AnySearchConfig::new()
                .unwrap()
                .with_domain(AnySearchDomain::Code)
                .with_sub_domain(AnySearchSubDomain::new("code.doc").unwrap())
                .with_sub_domain_params(params)
        };

        let mut nested = Value::Null;
        for _ in 0..40 {
            nested = serde_json::json!({"nested": nested});
        }
        let mut params = BTreeMap::new();
        params.insert("filter".to_string(), nested);
        assert!(AnySearchProvider::new(config_with_params(params)).is_err());

        let mut params = BTreeMap::new();
        params.insert("filter".to_string(), Value::String("x".repeat(17_000)));
        assert!(AnySearchProvider::new(config_with_params(params)).is_err());

        let mut params = BTreeMap::new();
        for index in 0..5 {
            params.insert(
                format!("filter_{index}"),
                Value::Array(vec![Value::Null; 256]),
            );
        }
        assert!(AnySearchProvider::new(config_with_params(params)).is_err());
    }

    #[test]
    fn default_readiness_is_anonymous_without_environment_key() {
        let provider = AnySearchProvider::new(
            AnySearchConfig::new()
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
    fn readiness_rejects_credentials_that_cannot_be_sent_as_headers() {
        let provider = AnySearchProvider::new(
            AnySearchConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::value("invalid\nkey")),
        )
        .unwrap();

        assert_eq!(provider.readiness(), ProviderReadiness::InvalidCredential);
    }

    #[test]
    fn markdown_parser_matches_live_anysearch_shape() {
        let parsed = parse_search_markdown(
            "## Search Results (1 result, 42ms)\n\n### 1. Rust\n- **URL**: https://www.rust-lang.org/\n- Full AnySearch content\n",
            10,
        )
        .unwrap();

        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.total_results, Some(1));
        assert_eq!(parsed.response_time_ms, Some(42));
        assert_eq!(
            parsed.results[0].full_text.as_deref(),
            Some("Full AnySearch content")
        );
    }

    #[test]
    fn rpc_error_ignores_data_by_construction() {
        let error = anysearch_rpc_error(
            AnySearchRpcError {
                code: -32602,
                message: Some("invalid arguments".to_string()),
            },
            Some("req-1"),
            None,
            None,
            &[],
        );

        assert_eq!(error.kind(), "provider_invalid_request");
        let SearchError::Provider(error) = error else {
            panic!("expected provider error");
        };
        assert_eq!(error.application_code(), Some(-32602));
        assert_eq!(error.request_id(), Some("req-1"));
    }

    #[test]
    fn rpc_internal_errors_are_transient() {
        let error = anysearch_rpc_error(
            AnySearchRpcError {
                code: -32603,
                message: Some("internal error".to_string()),
            },
            None,
            None,
            None,
            &[],
        );

        assert_eq!(error.kind(), "provider_unavailable");
        assert!(error.is_transient());
    }

    #[test]
    fn rpc_rate_limit_message_is_not_misclassified_as_quota() {
        assert_eq!(
            classify_failure(None, Some(-32000), "rate limit reached"),
            ProviderErrorKind::RateLimited
        );
    }
}
