//! Client for the A3S gateway search service at `POST /v1/search`.
//!
//! The gateway forwards that route to `a3s-search serve`, which answers from
//! Wikipedia, DuckDuckGo, Brave, and Bing. This provider does not send
//! Firecrawl-only filters.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, JsonAuth, JsonSearchApi};
use super::protocol::{
    non_empty, sanitize_provider_multiline_text, sanitize_provider_text_with_secrets,
    strip_simple_markup, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use crate::{ProviderErrorKind, Result, ResultType, SearchUsage};
use url::Url;

const PROVIDER_ID: &str = "a3s";
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8080/v1/search";
const MAX_QUERY_CHARS: usize = 500;
const MAX_LIMIT: u8 = 100;

/// How A3S expects the caller credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum A3sAuth {
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// `X-API-Key: <key>`.
    ApiKey,
}

/// Typed A3S search defaults and credentials.
#[derive(Debug, Clone)]
pub struct A3sConfig {
    api: JsonSearchApi,
    max_results: u8,
    include_markdown: bool,
}

impl A3sConfig {
    /// Creates the default A3S configuration.
    ///
    /// `A3S_GATEWAY_KEY` is required. The default request asks for web metadata
    /// only. Markdown scraping is opt-in.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(PROVIDER_ID, "A3S", DEFAULT_ENDPOINT, "A3S_GATEWAY_KEY")?,
            max_results: 10,
            include_markdown: false,
        })
    }

    /// Replaces the API endpoint.
    pub fn with_endpoint(mut self, endpoint: Url) -> Result<Self> {
        self.api = self.api.with_endpoint(endpoint)?;
        Ok(self)
    }

    /// Replaces the API-key source.
    pub fn with_api_key(mut self, api_key: CredentialSource) -> Self {
        self.api = self.api.with_api_key(api_key);
        self
    }

    /// Selects Bearer or `X-API-Key` authentication.
    pub fn with_auth(mut self, auth: A3sAuth) -> Self {
        let wire = match auth {
            A3sAuth::Bearer => JsonAuth::Bearer,
            A3sAuth::ApiKey => JsonAuth::Header("x-api-key"),
        };
        self.api = self.api.with_auth(wire);
        self
    }

    /// Sets the result cap in the `1..=100` range.
    ///
    /// The A3S wire field remains `limit`.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=MAX_LIMIT).contains(&max_results) {
            return Err(invalid_config(
                PROVIDER_ID,
                "A3S max_results must be between 1 and 100",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Requests markdown page text for each result.
    pub fn with_include_markdown(mut self, include_markdown: bool) -> Self {
        self.include_markdown = include_markdown;
        self
    }

    /// Replaces HTTP transport limits.
    pub fn with_http_config(mut self, http: ProviderHttpConfig) -> Self {
        self.api = self.api.with_http_config(http);
        self
    }

    /// Returns the configured endpoint.
    pub fn endpoint(&self) -> &Url {
        self.api.endpoint()
    }
}

/// Native Rust implementation of the A3S search API.
#[derive(Debug)]
pub struct A3sProvider {
    config: A3sConfig,
    client: ProviderHttpClient,
}

impl A3sProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: A3sConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `A3S_GATEWAY_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(A3sConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for A3sProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        let mut capabilities = ProviderCapabilities::new().with_usage(true);
        if self.config.include_markdown {
            capabilities = capabilities.with_full_text(true);
        }
        ProviderDescriptor::new(
            PROVIDER_ID,
            "A3S",
            "https://github.com/A3S-Lab/Search",
            capabilities,
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, MAX_QUERY_CHARS)?;
        let payload = A3sRequest {
            query,
            limit: self.max_results_limit(),
            sources: ["web"],
            scrape_options: self.config.include_markdown.then_some(A3sScrapeOptions {
                formats: [A3sFormat { kind: "markdown" }],
            }),
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(reply.reject("A3S request failed", |code, message, _| {
                a3s_code_kind(code, message)
            }));
        }

        let payload: Value = reply.decode()?;
        match payload.get("success") {
            Some(Value::Bool(false)) => {
                return Err(
                    reply.reject("A3S rejected the request", |code, message, _| {
                        a3s_code_kind(code, message).or(Some(ProviderErrorKind::InvalidRequest))
                    }),
                );
            }
            Some(Value::Bool(true)) => {}
            _ => return Err(reply.contract_error()),
        }
        let document: A3sDocument =
            serde_json::from_value(payload).map_err(|_| reply.contract_error())?;
        let mut results = Vec::new();
        for hit in document.data.hits() {
            if let Some(result) = adapt_web(hit, &secrets) {
                results.push(result);
            }
        }
        results.truncate(usize::from(self.max_results_limit()));
        let request_id = document
            .id
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .filter(|value| !value.is_empty());
        Ok(reply.seal(ProviderResponse {
            results,
            report: ProviderReport {
                request_id,
                usage: document.credits_used.and_then(usage_from_credits),
                ..Default::default()
            },
            ..Default::default()
        }))
    }
}

impl A3sProvider {
    fn max_results_limit(&self) -> u8 {
        self.config.max_results
    }
}

#[derive(Serialize)]
struct A3sRequest<'a> {
    query: &'a str,
    limit: u8,
    sources: [&'static str; 1],
    #[serde(rename = "scrapeOptions", skip_serializing_if = "Option::is_none")]
    scrape_options: Option<A3sScrapeOptions>,
}

#[derive(Serialize)]
struct A3sScrapeOptions {
    formats: [A3sFormat; 1],
}

#[derive(Serialize)]
struct A3sFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct A3sDocument {
    #[serde(default)]
    data: A3sData,
    id: Option<String>,
    #[serde(rename = "creditsUsed")]
    credits_used: Option<f64>,
}

#[derive(Deserialize, Default)]
struct A3sData {
    #[serde(default)]
    web: Vec<A3sWebHit>,
    #[serde(default)]
    results: Option<A3sResults>,
}

impl A3sData {
    fn hits(self) -> Vec<A3sWebHit> {
        let mut hits = self.web;
        match self.results {
            Some(A3sResults::List(list)) => hits.extend(list),
            Some(A3sResults::Grouped { web }) => hits.extend(web),
            None => {}
        }
        hits
    }
}

/// `POST /v1/search` puts hits in `data.results`. With `sources` set, A3S
/// sends the grouped object; without sources it sends a flat list.
#[derive(Deserialize)]
#[serde(untagged)]
enum A3sResults {
    List(Vec<A3sWebHit>),
    Grouped {
        #[serde(default)]
        web: Vec<A3sWebHit>,
    },
}

#[derive(Deserialize)]
struct A3sWebHit {
    title: Option<String>,
    description: Option<String>,
    url: Option<String>,
    markdown: Option<String>,
}

fn adapt_web(hit: A3sWebHit, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit.url.as_deref().and_then(validated_web_url)?;
    let title = titled(hit.title.as_deref(), &url, secrets);
    let snippet = JsonSearchApi::clean(hit.description.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
    if let Some(markdown) = non_empty(hit.markdown) {
        result = attach_full_text(result, &markdown, secrets);
    }
    Some(result)
}

fn attach_full_text(result: ProviderResult, markdown: &str, secrets: &[&str]) -> ProviderResult {
    let text =
        sanitize_provider_text_with_secrets(&strip_simple_markup(markdown), 16 * 1024, secrets);
    let text = sanitize_provider_multiline_text(&text, 16 * 1024);
    if text.is_empty() {
        result
    } else {
        result.with_full_text(text)
    }
}

fn titled(title: Option<&str>, url: &str, secrets: &[&str]) -> String {
    let title = JsonSearchApi::clean(title.unwrap_or(url), 300, secrets);
    if title.is_empty() {
        url.to_string()
    } else {
        title
    }
}

fn usage_from_credits(credits: f64) -> Option<SearchUsage> {
    (credits.is_finite() && credits >= 0.0).then(|| SearchUsage::new().with_credits(credits))
}

fn a3s_code_kind(code: Option<&str>, message: Option<&str>) -> Option<ProviderErrorKind> {
    match code.unwrap_or("") {
        "UNAUTHORIZED" | "INVALID_API_KEY" | "MISSING_API_KEY" => {
            Some(ProviderErrorKind::Authentication)
        }
        "PAYMENT_REQUIRED" | "INSUFFICIENT_CREDITS" | "QUOTA_EXCEEDED" => {
            Some(ProviderErrorKind::Quota)
        }
        "RATE_LIMIT_EXCEEDED" | "RATE_LIMITED" => Some(ProviderErrorKind::RateLimited),
        "FORBIDDEN" => Some(ProviderErrorKind::Permission),
        _ if message.is_some_and(mentions_quota) => Some(ProviderErrorKind::Quota),
        _ => None,
    }
}

fn mentions_quota(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("credit") || message.contains("quota") || message.contains("payment required")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_results_rejects_values_outside_the_vendor_range() {
        assert!(A3sConfig::new().unwrap().with_max_results(0).is_err());
        assert!(A3sConfig::new().unwrap().with_max_results(101).is_err());
        assert!(A3sConfig::new().unwrap().with_max_results(10).is_ok());
    }
}
