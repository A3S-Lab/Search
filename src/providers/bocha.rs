//! Native Bocha Web Search API provider.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, JsonSearchApi};
use super::protocol::{
    non_empty, sanitize_provider_multiline_text, sanitize_provider_text_with_secrets,
    strip_simple_markup, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use crate::{ProviderErrorKind, Result, ResultType};
use url::Url;

const PROVIDER_ID: &str = "bocha";
const DEFAULT_ENDPOINT: &str = "https://api.bochaai.com/v1/web-search";
const MAX_COUNT: u8 = 50;

/// Typed Bocha Web Search request defaults and credentials.
#[derive(Debug, Clone)]
pub struct BochaConfig {
    api: JsonSearchApi,
    max_results: u8,
    summary: bool,
}

impl BochaConfig {
    /// Creates the default Bocha configuration.
    ///
    /// `BOCHA_API_KEY` is required. Summary excerpts are requested so callers
    /// receive the AI-oriented page evidence Bocha documents for this API.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(PROVIDER_ID, "Bocha", DEFAULT_ENDPOINT, "BOCHA_API_KEY")?,
            max_results: 10,
            summary: true,
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

    /// Sets the result cap in the documented `1..=50` range.
    ///
    /// The Bocha wire field remains `count`.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=MAX_COUNT).contains(&max_results) {
            return Err(invalid_config(
                PROVIDER_ID,
                "Bocha max_results must be between 1 and 50",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Controls whether Bocha returns page summaries.
    pub fn with_summary(mut self, summary: bool) -> Self {
        self.summary = summary;
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

/// Native Rust implementation of the Bocha Web Search API.
#[derive(Debug)]
pub struct BochaProvider {
    config: BochaConfig,
    client: ProviderHttpClient,
}

impl BochaProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: BochaConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `BOCHA_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(BochaConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for BochaProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "Bocha",
            "https://open.bochaai.com/",
            ProviderCapabilities::new()
                .with_time_range(true)
                .with_full_text(self.config.summary),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, 2_000)?;
        let payload = BochaRequest {
            query,
            freshness: request.time_range.map(bocha_freshness),
            summary: self.config.summary,
            count: self.config.max_results,
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(reply.reject("Bocha request failed", |code, _, status| {
                bocha_code_kind(code, status)
            }));
        }

        let mut payload: Value = reply.decode()?;
        if declared_success_error(&payload) {
            return Err(
                reply.reject("Bocha returned an application error", |code, _, _| {
                    bocha_code_kind(code, 400)
                }),
            );
        }
        let request_id = string_field(&payload, &["log_id", "request_id", "requestId"])
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .filter(|value| !value.is_empty());
        let nested_pages = payload
            .get("data")
            .is_some_and(|data| data.get("webPages").is_some() || data.get("webpages").is_some());
        let document = if nested_pages {
            payload
                .get_mut("data")
                .map(Value::take)
                .unwrap_or(Value::Null)
        } else {
            payload
        };
        let parsed: BochaDocument =
            serde_json::from_value(document).map_err(|_| reply.contract_error())?;
        let pages = parsed
            .web_pages
            .or(parsed.web_pages_lower)
            .unwrap_or(BochaPages {
                total_estimated_matches: None,
                value: Vec::new(),
            });
        let results = pages
            .value
            .into_iter()
            .take(usize::from(self.config.max_results))
            .filter_map(|hit| adapt_hit(hit, &secrets))
            .collect();
        Ok(reply.seal(ProviderResponse {
            results,
            report: ProviderReport {
                request_id,
                total_results: pages.total_estimated_matches,
                ..Default::default()
            },
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct BochaRequest<'a> {
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    freshness: Option<&'static str>,
    summary: bool,
    count: u8,
}

#[derive(Deserialize)]
struct BochaDocument {
    #[serde(rename = "webPages")]
    web_pages: Option<BochaPages>,
    #[serde(rename = "webpages")]
    web_pages_lower: Option<BochaPages>,
}

#[derive(Deserialize)]
struct BochaPages {
    #[serde(rename = "totalEstimatedMatches")]
    total_estimated_matches: Option<u64>,
    #[serde(default)]
    value: Vec<BochaHit>,
}

#[derive(Deserialize)]
struct BochaHit {
    name: Option<String>,
    url: Option<String>,
    #[serde(rename = "displayUrl")]
    display_url: Option<String>,
    snippet: Option<String>,
    summary: Option<String>,
    #[serde(rename = "siteIcon")]
    site_icon: Option<String>,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
}

fn adapt_hit(hit: BochaHit, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit
        .url
        .as_deref()
        .and_then(validated_web_url)
        .or_else(|| hit.display_url.as_deref().and_then(validated_web_url))?;
    let title = JsonSearchApi::clean(hit.name.as_deref().unwrap_or(&url), 300, secrets);
    let snippet = JsonSearchApi::clean(hit.snippet.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
    if let Some(summary) = non_empty(hit.summary) {
        let summary =
            sanitize_provider_text_with_secrets(&strip_simple_markup(&summary), 16 * 1024, secrets);
        let summary = sanitize_provider_multiline_text(&summary, 16 * 1024);
        if !summary.is_empty() {
            result = result.with_full_text(summary);
        }
    }
    if let Some(date) = non_empty(hit.date_published) {
        let date = JsonSearchApi::clean(&date, 64, secrets);
        if !date.is_empty() {
            result = result.with_published_date(date);
        }
    }
    if let Some(icon) = hit.site_icon.as_deref().and_then(validated_web_url) {
        result = result.with_favicon(icon);
    }
    Some(result)
}

fn bocha_freshness(range: crate::TimeRange) -> &'static str {
    match range {
        crate::TimeRange::Day => "oneDay",
        crate::TimeRange::Week => "oneWeek",
        crate::TimeRange::Month => "oneMonth",
        crate::TimeRange::Year => "oneYear",
    }
}

fn declared_success_error(payload: &Value) -> bool {
    let Some(code) = payload.get("code") else {
        return false;
    };
    match code {
        Value::Number(code) => code.as_u64().is_some_and(|code| code != 200 && code != 0),
        Value::String(code) => {
            let code = code.trim();
            !code.is_empty() && code != "200" && !code.eq_ignore_ascii_case("success")
        }
        _ => false,
    }
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn bocha_code_kind(code: Option<&str>, status: u16) -> Option<ProviderErrorKind> {
    match code.unwrap_or("") {
        "401" | "Unauthorized" => Some(ProviderErrorKind::Authentication),
        "402" | "403" => Some(ProviderErrorKind::Quota),
        "429" => Some(ProviderErrorKind::RateLimited),
        "400" => Some(ProviderErrorKind::InvalidRequest),
        _ if status == 200 => Some(ProviderErrorKind::InvalidRequest),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_must_stay_in_the_documented_range() {
        assert!(BochaConfig::new().unwrap().with_max_results(0).is_err());
        assert!(BochaConfig::new().unwrap().with_max_results(51).is_err());
        assert_eq!(
            BochaConfig::new()
                .unwrap()
                .with_max_results(8)
                .unwrap()
                .max_results,
            8
        );
    }
}
