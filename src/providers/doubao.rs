//! Native Volcengine Doubao Search (豆包搜索) web API provider.
//!
//! Doubao Search answers every application error with HTTP 200 and an
//! `ResponseMetadata.Error` envelope, so a 2xx status alone is not success.
//! Rich-card hits (weather, exchange rates, ...) arrive without a URL; their
//! summaries become answers instead of being dropped.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

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

const PROVIDER_ID: &str = "doubao";
const DEFAULT_ENDPOINT: &str = "https://open.feedcoopapi.com/search_api/web_search";
const MAX_COUNT: u8 = 50;
const MAX_QUERY_CHARS: usize = 1_000;
const MAX_ANSWERS: usize = 3;

/// Typed Doubao Search request defaults and credentials.
#[derive(Debug, Clone)]
pub struct DoubaoConfig {
    api: JsonSearchApi,
    max_results: u8,
    authoritative_only: bool,
    query_rewrite: bool,
}

impl DoubaoConfig {
    /// Creates the default Doubao Search configuration.
    ///
    /// `DOUBAO_SEARCH_API_KEY` is required. Every source authority level is
    /// accepted and the vendor query rewrite stays off.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(
                PROVIDER_ID,
                "Doubao Search",
                DEFAULT_ENDPOINT,
                "DOUBAO_SEARCH_API_KEY",
            )?,
            max_results: 10,
            authoritative_only: false,
            query_rewrite: false,
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
    /// The Doubao wire field remains `Count`.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=MAX_COUNT).contains(&max_results) {
            return Err(invalid_config(
                PROVIDER_ID,
                "Doubao Search max_results must be between 1 and 50",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Restricts results to sources Doubao rates "very authoritative".
    pub fn with_authoritative_only(mut self, authoritative_only: bool) -> Self {
        self.authoritative_only = authoritative_only;
        self
    }

    /// Lets Doubao rewrite conversational questions into search queries.
    pub fn with_query_rewrite(mut self, query_rewrite: bool) -> Self {
        self.query_rewrite = query_rewrite;
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

/// Native Rust implementation of the Doubao Search web API.
#[derive(Debug)]
pub struct DoubaoProvider {
    config: DoubaoConfig,
    client: ProviderHttpClient,
}

impl DoubaoProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: DoubaoConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `DOUBAO_SEARCH_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(DoubaoConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for DoubaoProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "Doubao Search",
            "https://www.volcengine.com/docs/87772/2272953",
            ProviderCapabilities::new()
                .with_time_range(true)
                .with_answers(true)
                .with_full_text(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, MAX_QUERY_CHARS)?;
        let payload = DoubaoRequest {
            query,
            search_type: "web",
            count: self.config.max_results,
            time_range: request.time_range.map(doubao_time_range),
            filter: self
                .config
                .authoritative_only
                .then_some(DoubaoFilter { auth_info_level: 1 }),
            query_control: self.config.query_rewrite.then_some(DoubaoQueryControl {
                query_rewrite: true,
            }),
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            let envelope = reply.decode::<DoubaoEnvelope>().ok();
            return Err(match envelope.as_ref().and_then(DoubaoEnvelope::error) {
                Some(error) => reply.reject_with(
                    "Doubao Search request failed",
                    error.message.as_deref(),
                    envelope.as_ref().and_then(DoubaoEnvelope::request_id),
                    error.kind(),
                ),
                None => reply.reject("Doubao Search request failed", |code, _, _| {
                    code.and_then(doubao_code_kind)
                }),
            });
        }

        let envelope: DoubaoEnvelope = reply.decode()?;
        if let Some(error) = envelope.error() {
            return Err(reply.reject_with(
                "Doubao Search rejected the request",
                error.message.as_deref(),
                envelope.request_id(),
                error.kind().or(Some(ProviderErrorKind::InvalidRequest)),
            ));
        }
        let request_id = envelope
            .request_id()
            .map(|value| sanitize_provider_text_with_secrets(value, 128, &secrets))
            .filter(|value| !value.is_empty());
        let result = envelope.result.ok_or_else(|| reply.contract_error())?;

        let mut results = Vec::new();
        let mut answers = Vec::new();
        for hit in result.web_results.unwrap_or_default() {
            match adapt_hit(&hit, &secrets) {
                Some(result) => results.push(result),
                None if answers.len() < MAX_ANSWERS => {
                    answers.extend(card_answer(&hit, &secrets));
                }
                None => {}
            }
        }
        results.truncate(usize::from(self.config.max_results));
        Ok(reply.seal(ProviderResponse {
            results,
            answers,
            report: ProviderReport {
                request_id,
                total_results: result.result_count,
                response_time_ms: result.time_cost,
                ..Default::default()
            },
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct DoubaoRequest<'a> {
    #[serde(rename = "Query")]
    query: &'a str,
    #[serde(rename = "SearchType")]
    search_type: &'static str,
    #[serde(rename = "Count")]
    count: u8,
    #[serde(rename = "TimeRange", skip_serializing_if = "Option::is_none")]
    time_range: Option<&'static str>,
    #[serde(rename = "Filter", skip_serializing_if = "Option::is_none")]
    filter: Option<DoubaoFilter>,
    #[serde(rename = "QueryControl", skip_serializing_if = "Option::is_none")]
    query_control: Option<DoubaoQueryControl>,
}

#[derive(Serialize)]
struct DoubaoFilter {
    #[serde(rename = "AuthInfoLevel")]
    auth_info_level: u8,
}

#[derive(Serialize)]
struct DoubaoQueryControl {
    #[serde(rename = "QueryRewrite")]
    query_rewrite: bool,
}

#[derive(Deserialize)]
struct DoubaoEnvelope {
    #[serde(rename = "ResponseMetadata")]
    metadata: Option<DoubaoMetadata>,
    #[serde(rename = "Result")]
    result: Option<DoubaoResult>,
}

impl DoubaoEnvelope {
    fn error(&self) -> Option<&DoubaoError> {
        self.metadata.as_ref()?.error.as_ref()
    }

    fn request_id(&self) -> Option<&str> {
        self.metadata.as_ref()?.request_id.as_deref()
    }
}

#[derive(Deserialize)]
struct DoubaoMetadata {
    #[serde(rename = "RequestId")]
    request_id: Option<String>,
    #[serde(rename = "Error")]
    error: Option<DoubaoError>,
}

#[derive(Deserialize)]
struct DoubaoError {
    #[serde(rename = "CodeN")]
    code_n: Option<i64>,
    #[serde(rename = "Code")]
    code: Option<String>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

impl DoubaoError {
    fn kind(&self) -> Option<ProviderErrorKind> {
        doubao_code_kind(self.code.as_deref().unwrap_or("")).or_else(|| {
            self.code_n
                .and_then(|code| doubao_code_kind(&code.to_string()))
        })
    }
}

#[derive(Deserialize)]
struct DoubaoResult {
    #[serde(rename = "ResultCount")]
    result_count: Option<u64>,
    #[serde(rename = "TimeCost")]
    time_cost: Option<u64>,
    #[serde(rename = "WebResults")]
    web_results: Option<Vec<DoubaoHit>>,
}

#[derive(Deserialize)]
struct DoubaoHit {
    #[serde(rename = "Title")]
    title: Option<String>,
    #[serde(rename = "Url")]
    url: Option<String>,
    #[serde(rename = "Snippet")]
    snippet: Option<String>,
    #[serde(rename = "Summary")]
    summary: Option<String>,
    #[serde(rename = "Content")]
    content: Option<String>,
    #[serde(rename = "PublishTime")]
    publish_time: Option<String>,
    #[serde(rename = "LogoUrl")]
    logo_url: Option<String>,
}

fn adapt_hit(hit: &DoubaoHit, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit.url.as_deref().and_then(validated_web_url)?;
    let title = JsonSearchApi::clean(hit.title.as_deref().unwrap_or(&url), 300, secrets);
    let title = if title.is_empty() { url.clone() } else { title };
    let snippet = JsonSearchApi::clean(hit.snippet.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
    let text = non_empty(hit.summary.clone()).or_else(|| non_empty(hit.content.clone()));
    if let Some(text) = text {
        let text =
            sanitize_provider_text_with_secrets(&strip_simple_markup(&text), 16 * 1024, secrets);
        let text = sanitize_provider_multiline_text(&text, 16 * 1024);
        if !text.is_empty() {
            result = result.with_full_text(text);
        }
    }
    if let Some(date) = non_empty(hit.publish_time.clone()) {
        let date = JsonSearchApi::clean(&date, 64, secrets);
        if !date.is_empty() {
            result = result.with_published_date(date);
        }
    }
    if let Some(logo) = hit.logo_url.as_deref().and_then(validated_web_url) {
        result = result.with_favicon(logo);
    }
    Some(result)
}

/// Card summaries are markdown reports; answers are single-line text.
fn card_answer(hit: &DoubaoHit, secrets: &[&str]) -> Option<String> {
    let text = non_empty(hit.summary.clone()).or_else(|| non_empty(hit.snippet.clone()))?;
    let plain = text
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('#')
                .trim_start_matches(['-', '*'])
                .trim()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let text = JsonSearchApi::clean(&plain, 2_000, secrets);
    (!text.is_empty()).then_some(text)
}

fn doubao_time_range(range: crate::TimeRange) -> &'static str {
    match range {
        crate::TimeRange::Day => "OneDay",
        crate::TimeRange::Week => "OneWeek",
        crate::TimeRange::Month => "OneMonth",
        crate::TimeRange::Year => "OneYear",
    }
}

fn doubao_code_kind(code: &str) -> Option<ProviderErrorKind> {
    match code {
        "invalid_api_key" | "10403" | "700901" | "InvalidAccessKey" => {
            Some(ProviderErrorKind::Authentication)
        }
        "10406" | "10407" | "10408" | "10412" | "FunctionUnavailable" => {
            Some(ProviderErrorKind::Quota)
        }
        "429" | "100018" | "700429" | "FlowLimitExceeded" => Some(ProviderErrorKind::RateLimited),
        "100013" => Some(ProviderErrorKind::Permission),
        "10400" | "10402" | "10409" => Some(ProviderErrorKind::InvalidRequest),
        "10500" => Some(ProviderErrorKind::Unavailable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_must_stay_in_the_documented_range() {
        assert!(DoubaoConfig::new().unwrap().with_max_results(0).is_err());
        assert!(DoubaoConfig::new().unwrap().with_max_results(51).is_err());
        assert_eq!(
            DoubaoConfig::new()
                .unwrap()
                .with_max_results(50)
                .unwrap()
                .max_results,
            50
        );
    }

    #[test]
    fn vendor_codes_map_to_the_closed_error_set() {
        let error = |code: &str, code_n| DoubaoError {
            code_n,
            code: Some(code.to_string()),
            message: None,
        };
        assert_eq!(
            error("invalid_api_key", Some(700901)).kind(),
            Some(ProviderErrorKind::Authentication)
        );
        assert_eq!(
            error("10406", Some(10406)).kind(),
            Some(ProviderErrorKind::Quota)
        );
        assert_eq!(
            error("FlowLimitExceeded", None).kind(),
            Some(ProviderErrorKind::RateLimited)
        );
        assert_eq!(
            error("unknown", Some(700429)).kind(),
            Some(ProviderErrorKind::RateLimited)
        );
        assert_eq!(error("unknown", None).kind(), None);
    }
}
