//! Bounded, redirect-free HTTP transport for provider APIs.

use std::fmt;
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use url::{Host, Url};

use super::credential::SecretString;
use crate::{ProviderError, ProviderErrorKind, Result, SearchError};

const DEFAULT_MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// HTTP safety limits shared by provider integrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderHttpConfig {
    timeout: Duration,
    max_response_bytes: usize,
}

impl ProviderHttpConfig {
    /// Sets the transport timeout used when a provider is called directly.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the maximum decompressed response body size.
    pub fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    /// Returns the configured transport timeout.
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Returns the maximum decompressed response body size.
    pub const fn max_response_bytes(&self) -> usize {
        self.max_response_bytes
    }
}

impl Default for ProviderHttpConfig {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

pub(crate) struct ProviderHttpClient {
    provider: &'static str,
    client: Client,
    max_response_bytes: usize,
}

impl fmt::Debug for ProviderHttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderHttpClient")
            .field("provider", &self.provider)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl ProviderHttpClient {
    pub(crate) fn new(provider: &'static str, config: ProviderHttpConfig) -> Result<Self> {
        if config.timeout.is_zero() {
            return Err(SearchError::Other(
                "provider HTTP timeout must be greater than zero".to_string(),
            ));
        }
        if config.max_response_bytes == 0 {
            return Err(SearchError::Other(
                "provider response size limit must be greater than zero".to_string(),
            ));
        }

        let client = Client::builder()
            .user_agent(concat!("a3s-search/", env!("CARGO_PKG_VERSION")))
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| SearchError::Other("failed to create provider HTTP client".to_string()))?;

        Ok(Self {
            provider,
            client,
            max_response_bytes: config.max_response_bytes,
        })
    }

    pub(crate) async fn post_json<T: Serialize + ?Sized>(
        &self,
        endpoint: &Url,
        mut headers: HeaderMap,
        body: &T,
    ) -> Result<ProviderHttpResponse> {
        validate_provider_endpoint(self.provider, endpoint)?;

        let body = serde_json::to_vec(body).map_err(|_| {
            ProviderError::new(
                self.provider,
                ProviderErrorKind::InvalidRequest,
                "provider request could not be serialized",
            )
        })?;

        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self
            .client
            .post(endpoint.clone())
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|error| transport_error(self.provider, &error))?;

        let status = response.status();
        let headers = response.headers().clone();
        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_bytes as u64)
        {
            return Err(response_too_large(self.provider, self.max_response_bytes));
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| transport_error(self.provider, &error))?;
            if body.len().saturating_add(chunk.len()) > self.max_response_bytes {
                return Err(response_too_large(self.provider, self.max_response_bytes));
            }
            body.extend_from_slice(&chunk);
        }

        Ok(ProviderHttpResponse {
            status,
            headers,
            body,
        })
    }
}

pub(crate) struct ProviderHttpResponse {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Vec<u8>,
}

impl ProviderHttpResponse {
    pub(crate) fn retry_after_seconds(&self) -> Option<u64> {
        self.headers
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.min(86_400))
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }
}

pub(crate) fn bearer_header(provider: &str, credential: &SecretString) -> Result<HeaderValue> {
    secret_header(provider, format!("Bearer {}", credential.expose()))
}

pub(crate) fn secret_header(provider: &str, value: String) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(&value).map_err(|_| {
        ProviderError::new(
            provider,
            ProviderErrorKind::Authentication,
            "configured credential contains invalid header bytes",
        )
    })?;
    value.set_sensitive(true);
    Ok(value)
}

pub(crate) fn insert_header(
    provider: &str,
    headers: &mut HeaderMap,
    name: &'static str,
    value: HeaderValue,
) -> Result<()> {
    let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
        ProviderError::new(
            provider,
            ProviderErrorKind::InvalidRequest,
            "provider header name is invalid",
        )
    })?;
    headers.insert(name, value);
    Ok(())
}

pub(crate) fn validate_provider_endpoint(provider: &str, endpoint: &Url) -> Result<()> {
    if !endpoint.username().is_empty() || endpoint.password().is_some() {
        return Err(ProviderError::new(
            provider,
            ProviderErrorKind::InvalidRequest,
            "provider endpoint must not contain credentials",
        )
        .into());
    }
    if endpoint.fragment().is_some() {
        return Err(ProviderError::new(
            provider,
            ProviderErrorKind::InvalidRequest,
            "provider endpoint must not contain a fragment",
        )
        .into());
    }

    let secure = endpoint.scheme() == "https";
    let loopback_http = endpoint.scheme() == "http"
        && endpoint.host().is_some_and(|host| match host {
            Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
            Host::Ipv4(address) => address.is_loopback(),
            Host::Ipv6(address) => address.is_loopback(),
        });

    if secure || loopback_http {
        return Ok(());
    }

    Err(ProviderError::new(
        provider,
        ProviderErrorKind::InvalidRequest,
        "provider endpoint must use HTTPS (loopback HTTP is allowed for tests)",
    )
    .into())
}

fn transport_error(provider: &str, error: &reqwest::Error) -> SearchError {
    let message = if error.is_timeout() {
        "provider request timed out"
    } else if error.is_connect() {
        "could not connect to provider"
    } else if error.is_decode() || error.is_body() {
        "failed to read provider response"
    } else if error.is_builder() {
        "provider request could not be constructed"
    } else {
        "provider request failed"
    };
    ProviderError::new(provider, ProviderErrorKind::Transport, message).into()
}

fn response_too_large(provider: &str, limit: usize) -> SearchError {
    ProviderError::new(
        provider,
        ProviderErrorKind::InvalidResponse,
        format!("provider response exceeded the {limit}-byte safety limit"),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_requires_https_outside_loopback() {
        let error =
            validate_provider_endpoint("test", &Url::parse("http://example.com/search").unwrap())
                .unwrap_err();

        assert_eq!(error.kind(), "provider_invalid_request");
    }

    #[test]
    fn endpoint_allows_https_and_loopback_http() {
        validate_provider_endpoint("test", &Url::parse("https://example.com/search").unwrap())
            .unwrap();
        validate_provider_endpoint("test", &Url::parse("http://127.0.0.1:1234/search").unwrap())
            .unwrap();
        validate_provider_endpoint("test", &Url::parse("http://[::1]:1234/search").unwrap())
            .unwrap();
    }

    #[test]
    fn endpoint_rejects_embedded_credentials() {
        let error = validate_provider_endpoint(
            "test",
            &Url::parse("https://user:secret@example.com/search").unwrap(),
        )
        .unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn bearer_headers_are_marked_sensitive() {
        let secret = SecretString::new("secret".to_string()).unwrap();
        let value = bearer_header("test", &secret).unwrap();

        assert!(value.is_sensitive());
        assert!(!format!("{value:?}").contains("secret"));
    }

    #[test]
    fn http_config_rejects_zero_limits() {
        let error = ProviderHttpClient::new(
            "test",
            ProviderHttpConfig::default().with_max_response_bytes(0),
        )
        .unwrap_err();
        assert!(error.to_string().contains("greater than zero"));
    }
}
