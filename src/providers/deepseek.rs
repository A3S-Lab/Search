//! Native DeepSeek Search API provider.
//!
//! `POST /v1/tools/search` on `api.deepseek.com`. The query is at most 128
//! characters; a longer query fails this engine so the cascade can continue.
//! `summary` is the model-facing excerpt. `need_content` asks for the page
//! body and stays off unless configured, because the call is billed per request.

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, invalid_request, JsonSearchApi};
use super::protocol::{
    non_empty, sanitize_provider_multiline_text, sanitize_provider_text_with_secrets,
    strip_simple_markup, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderRequest, ProviderResponse, ProviderResult, SearchProvider,
};
use crate::{Result, ResultType, TimeRange};

const PROVIDER_ID: &str = "deepseek";
const DEFAULT_ENDPOINT: &str = "https://api.deepseek.com/v1/tools/search";
const MAX_QUERY_CHARS: usize = 128;
const MAX_COUNT: u8 = 50;

/// Typed DeepSeek Search request defaults and credentials.
#[derive(Debug, Clone)]
pub struct DeepSeekConfig {
    api: JsonSearchApi,
    max_results: u8,
    need_content: bool,
}

impl DeepSeekConfig {
    /// Creates the default DeepSeek Search configuration.
    ///
    /// `DEEPSEEK_API_KEY` is required. Page body is not requested: `summary`
    /// is the documented model-facing text, and each call is billed either way.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(
                PROVIDER_ID,
                "DeepSeek Search",
                DEFAULT_ENDPOINT,
                "DEEPSEEK_API_KEY",
            )?,
            max_results: 10,
            need_content: false,
        })
    }

    /// Replaces the API endpoint.
    pub fn with_endpoint(mut self, endpoint: url::Url) -> Result<Self> {
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
    /// The DeepSeek wire field remains `count`.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=MAX_COUNT).contains(&max_results) {
            return Err(invalid_config(
                PROVIDER_ID,
                "DeepSeek Search max_results must be between 1 and 50",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Controls whether DeepSeek returns page body in `content`.
    pub fn with_need_content(mut self, need_content: bool) -> Self {
        self.need_content = need_content;
        self
    }

    /// Replaces HTTP transport limits.
    pub fn with_http_config(mut self, http: ProviderHttpConfig) -> Self {
        self.api = self.api.with_http_config(http);
        self
    }

    /// Returns the configured endpoint.
    pub fn endpoint(&self) -> &url::Url {
        self.api.endpoint()
    }
}

/// Native Rust implementation of the DeepSeek Search API.
#[derive(Debug)]
pub struct DeepSeekProvider {
    config: DeepSeekConfig,
    client: ProviderHttpClient,
}

impl DeepSeekProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: DeepSeekConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `DEEPSEEK_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(DeepSeekConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for DeepSeekProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "DeepSeek Search",
            "https://api-docs.deepseek.com/guides/search",
            ProviderCapabilities::new()
                .with_time_range(true)
                .with_full_text(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = compose_query(request)?;
        let payload = DeepSeekRequest {
            query: &query,
            count: self.config.max_results,
            need_content: self.config.need_content,
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(reply.reject("DeepSeek Search request failed", |_, _, _| None));
        }
        let parsed: DeepSeekDocument = reply.decode()?;
        let results = parsed
            .results
            .into_iter()
            .take(usize::from(self.config.max_results))
            .filter_map(|hit| adapt_hit(hit, self.config.need_content, &secrets))
            .collect();
        Ok(reply.seal(ProviderResponse {
            results,
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct DeepSeekRequest<'a> {
    query: &'a str,
    count: u8,
    need_content: bool,
}

#[derive(Deserialize)]
struct DeepSeekDocument {
    #[serde(default)]
    results: Vec<DeepSeekHit>,
}

#[derive(Deserialize)]
struct DeepSeekHit {
    title: Option<String>,
    url: Option<String>,
    snippet: Option<String>,
    summary: Option<String>,
    content: Option<String>,
    site_icon: Option<String>,
    date: Option<String>,
}

/// DeepSeek filters are query syntax. Append them only while the whole query
/// stays inside the documented 128-character bound.
fn compose_query(request: &ProviderRequest) -> Result<String> {
    let query = request.query.trim();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(invalid_request(
            PROVIDER_ID,
            "DeepSeek Search query must be 1 to 128 characters",
        ));
    }
    let mut extras = Vec::new();
    if let Some(language) = request.language.as_deref().and_then(language_token) {
        extras.push(format!("lang:{language}"));
    }
    if let Some(range) = request.time_range {
        extras.push(format!("after:{}", utc_date_days_ago(range_days(range))));
    }
    let Some(suffix) = (!extras.is_empty()).then(|| extras.join(" ")) else {
        return Ok(query.to_string());
    };
    if query.chars().count() + 1 + suffix.chars().count() > MAX_QUERY_CHARS {
        return Ok(query.to_string());
    }
    Ok(format!("{query} {suffix}"))
}

fn language_token(language: &str) -> Option<String> {
    let token: String = language
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .flat_map(char::to_lowercase)
        .collect();
    (2..=8).contains(&token.len()).then_some(token)
}

fn range_days(range: TimeRange) -> u64 {
    match range {
        TimeRange::Day => 1,
        TimeRange::Week => 7,
        TimeRange::Month => 31,
        TimeRange::Year => 365,
    }
}

fn adapt_hit(hit: DeepSeekHit, need_content: bool, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit.url.as_deref().and_then(validated_web_url)?;
    let title = JsonSearchApi::clean(hit.title.as_deref().unwrap_or(&url), 300, secrets);
    let snippet = JsonSearchApi::clean(hit.snippet.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
    let body = need_content
        .then(|| non_empty(hit.content))
        .flatten()
        .or_else(|| non_empty(hit.summary));
    if let Some(body) = body {
        let body =
            sanitize_provider_text_with_secrets(&strip_simple_markup(&body), 16 * 1024, secrets);
        let body = sanitize_provider_multiline_text(&body, 16 * 1024);
        if !body.is_empty() {
            result = result.with_full_text(body);
        }
    }
    if let Some(date) = non_empty(hit.date) {
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

fn utc_date_days_ago(days_ago: u64) -> String {
    let today = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() / 86_400)
        .unwrap_or(0);
    civil_date(today.saturating_sub(days_ago))
}

/// Howard Hinnant's `civil_from_days`, for a UTC day count since 1970-01-01.
fn civil_date(days_since_epoch: u64) -> String {
    let z = days_since_epoch as i64 + 719_468;
    let era = z / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_must_stay_in_the_documented_range() {
        assert!(DeepSeekConfig::new().unwrap().with_max_results(0).is_err());
        assert!(DeepSeekConfig::new().unwrap().with_max_results(51).is_err());
        assert_eq!(
            DeepSeekConfig::new()
                .unwrap()
                .with_max_results(8)
                .unwrap()
                .max_results,
            8
        );
    }

    #[test]
    fn civil_dates_match_the_unix_epoch() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(20_454), "2026-01-01");
    }
}
