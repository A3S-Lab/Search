//! Native TinyFish Search API provider.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, invalid_request, time_window, JsonAuth, JsonSearchApi};
use super::protocol::{
    non_empty, sanitize_provider_text_with_secrets, validated_hostnames, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use crate::{ProviderErrorKind, Result, ResultType};

const PROVIDER_ID: &str = "tinyfish";
const DEFAULT_ENDPOINT: &str = "https://api.search.tinyfish.ai";
const MAX_QUERY_CHARS: usize = 2_000;
const MAX_PAGE: u32 = 10;

/// Content category accepted by the TinyFish Search API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TinyFishDomainType {
    /// Ranked web results.
    Web,
    /// Recent news articles.
    News,
    /// Academic papers.
    ResearchPaper,
}

impl TinyFishDomainType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::News => "news",
            Self::ResearchPaper => "research_paper",
        }
    }
}

/// Typed TinyFish request defaults and credentials.
#[derive(Debug, Clone)]
pub struct TinyFishConfig {
    api: JsonSearchApi,
    purpose: Option<String>,
    location: Option<String>,
    domain_type: TinyFishDomainType,
    include_domains: Vec<String>,
    exclude_domains: Vec<String>,
    include_thumbnail: bool,
}

impl TinyFishConfig {
    /// Creates the default TinyFish configuration.
    ///
    /// `TINYFISH_API_KEY` is required. Requests are not made without it.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(
                PROVIDER_ID,
                "TinyFish",
                DEFAULT_ENDPOINT,
                "TINYFISH_API_KEY",
            )?
            .with_auth(JsonAuth::Header("x-api-key")),
            purpose: None,
            location: None,
            domain_type: TinyFishDomainType::Web,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            include_thumbnail: false,
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

    /// Sets the optional search-intent statement.
    pub fn with_purpose(mut self, purpose: impl Into<String>) -> Result<Self> {
        let purpose = purpose.into().trim().to_string();
        if purpose.is_empty() || purpose.chars().count() > MAX_QUERY_CHARS {
            return Err(invalid_config(
                PROVIDER_ID,
                "TinyFish purpose must be 1 to 2000 characters",
            ));
        }
        self.purpose = Some(purpose);
        Ok(self)
    }

    /// Sets a two-letter country code for geo-targeted results.
    pub fn with_location(mut self, location: impl Into<String>) -> Result<Self> {
        let location = location.into().trim().to_ascii_uppercase();
        if location.len() != 2
            || !location
                .chars()
                .all(|character| character.is_ascii_alphabetic())
        {
            return Err(invalid_config(
                PROVIDER_ID,
                "TinyFish location must be a two-letter country code",
            ));
        }
        self.location = Some(location);
        Ok(self)
    }

    /// Sets the search category.
    pub fn with_domain_type(mut self, domain_type: TinyFishDomainType) -> Self {
        self.domain_type = domain_type;
        self
    }

    /// Restricts results to these domains.
    pub fn with_include_domains<I, S>(mut self, domains: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.include_domains = validated_domains(domains)?;
        Ok(self)
    }

    /// Excludes these domains from results.
    pub fn with_exclude_domains<I, S>(mut self, domains: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.exclude_domains = validated_domains(domains)?;
        Ok(self)
    }

    /// Requests result thumbnails when TinyFish has one.
    pub fn with_include_thumbnail(mut self, include_thumbnail: bool) -> Self {
        self.include_thumbnail = include_thumbnail;
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

/// Native Rust implementation of the TinyFish Search API.
#[derive(Debug)]
pub struct TinyFishProvider {
    config: TinyFishConfig,
    client: ProviderHttpClient,
}

impl TinyFishProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: TinyFishConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `TINYFISH_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(TinyFishConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for TinyFishProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "TinyFish",
            "https://www.tinyfish.ai/",
            ProviderCapabilities::new()
                .with_paging(true)
                .with_time_range(true)
                .with_images(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, MAX_QUERY_CHARS)?;
        let page = request.page.saturating_sub(1);
        if page > MAX_PAGE {
            return Err(invalid_request(
                PROVIDER_ID,
                "TinyFish page must be between 1 and 11",
            ));
        }
        if self.config.domain_type == TinyFishDomainType::ResearchPaper
            && request.time_range.is_some()
        {
            return Err(invalid_request(
                PROVIDER_ID,
                "TinyFish research_paper search does not accept a time range",
            ));
        }

        let mut endpoint = self.config.api.endpoint().clone();
        {
            let mut pairs = endpoint.query_pairs_mut();
            pairs.append_pair("query", query);
            if let Some(purpose) = self.config.purpose.as_deref() {
                pairs.append_pair("purpose", purpose);
            }
            if let Some(location) = self.config.location.as_deref() {
                pairs.append_pair("location", location);
            }
            if let Some(language) = language_code(request.language.as_deref()) {
                pairs.append_pair("language", language);
            }
            if !self.config.include_domains.is_empty() {
                pairs.append_pair("include_domains", &self.config.include_domains.join(","));
            }
            if !self.config.exclude_domains.is_empty() {
                pairs.append_pair("exclude_domains", &self.config.exclude_domains.join(","));
            }
            if self.config.domain_type != TinyFishDomainType::Web {
                pairs.append_pair("domain_type", self.config.domain_type.as_str());
            }
            if let Some(range) = request.time_range {
                pairs.append_pair("recency_minutes", &time_window(range).minutes.to_string());
            }
            if page > 0 {
                pairs.append_pair("page", &page.to_string());
            }
            if self.config.include_thumbnail {
                pairs.append_pair("include_thumbnail", "true");
            }
        }

        let reply = self.config.api.get(&self.client, &endpoint).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(reply.reject("TinyFish request failed", |code, _, _| {
                code.and_then(tinyfish_code_kind)
            }));
        }

        let payload: TinyFishResponse = reply.decode()?;
        let results = adapt_results(payload.results, &secrets);
        let request_id = non_empty(payload.request_id)
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .filter(|value| !value.is_empty())
            .or_else(|| {
                reply
                    .header("x-request-id")
                    .map(|value| sanitize_provider_text_with_secrets(value, 128, &secrets))
            });
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "domain_type".to_string(),
            Value::String(self.config.domain_type.as_str().to_string()),
        );
        if let Some(page) = payload.page {
            metadata.insert("page".to_string(), Value::from(page));
        }
        Ok(reply.seal(ProviderResponse {
            results,
            report: ProviderReport {
                request_id,
                total_results: payload.total_results,
                metadata,
                ..Default::default()
            },
            ..Default::default()
        }))
    }
}

#[derive(Deserialize)]
struct TinyFishResponse {
    results: Option<Vec<TinyFishHit>>,
    total_results: Option<u64>,
    page: Option<u64>,
    request_id: Option<String>,
}

#[derive(Deserialize)]
struct TinyFishHit {
    title: Option<String>,
    url: Option<String>,
    snippet: Option<String>,
    thumbnail_url: Option<String>,
    date: Option<String>,
}

fn adapt_results(hits: Option<Vec<TinyFishHit>>, secrets: &[&str]) -> Vec<ProviderResult> {
    hits.unwrap_or_default()
        .into_iter()
        .filter_map(|hit| {
            let url = hit.url.as_deref().and_then(validated_web_url)?;
            let title = JsonSearchApi::clean(hit.title.as_deref().unwrap_or(&url), 300, secrets);
            let snippet =
                JsonSearchApi::clean(hit.snippet.as_deref().unwrap_or(""), 2_000, secrets);
            let mut result =
                ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
            if let Some(date) = non_empty(hit.date) {
                let date = JsonSearchApi::clean(&date, 64, secrets);
                if !date.is_empty() {
                    result = result.with_published_date(date);
                }
            }
            if let Some(thumbnail) = hit.thumbnail_url.as_deref().and_then(validated_web_url) {
                result = result.with_thumbnail(thumbnail);
            }
            Some(result)
        })
        .collect()
}

fn language_code(language: Option<&str>) -> Option<&str> {
    let language = language?.split(['-', '_']).next()?.trim();
    (language.len() >= 2
        && language.len() <= 8
        && language
            .chars()
            .all(|character| character.is_ascii_alphabetic()))
    .then_some(language)
}

fn validated_domains<I, S>(domains: I) -> Result<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let domains = validated_hostnames(domains)
        .ok_or_else(|| invalid_config(PROVIDER_ID, "TinyFish domain filters must be hostnames"))?;
    if domains.len() > 50 {
        return Err(invalid_config(
            PROVIDER_ID,
            "TinyFish domain filters accept at most 50 domains",
        ));
    }
    Ok(domains)
}

fn tinyfish_code_kind(code: &str) -> Option<ProviderErrorKind> {
    match code {
        "MISSING_API_KEY" | "INVALID_API_KEY" | "UNAUTHORIZED" => {
            Some(ProviderErrorKind::Authentication)
        }
        "FORBIDDEN" | "SITE_BLOCKED" | "CONTENT_POLICY_VIOLATION" => {
            Some(ProviderErrorKind::Permission)
        }
        "INSUFFICIENT_CREDITS" | "DAILY_LIMIT_EXCEEDED" | "FEATURE_NOT_AVAILABLE" => {
            Some(ProviderErrorKind::Quota)
        }
        "RATE_LIMIT_EXCEEDED" => Some(ProviderErrorKind::RateLimited),
        "RETRY_REQUIRED" | "SERVICE_BUSY" | "TIMEOUT" | "INTERNAL_ERROR" => {
            Some(ProviderErrorKind::Unavailable)
        }
        "INVALID_INPUT" | "NOT_FOUND" => Some(ProviderErrorKind::InvalidRequest),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_is_not_ready() {
        let provider = TinyFishProvider::new(TinyFishConfig::new().unwrap().with_api_key(
            CredentialSource::environment("A3S_SEARCH_TINYFISH_KEY_THAT_MUST_NOT_EXIST"),
        ))
        .unwrap();
        assert!(!provider.readiness().is_ready());
    }

    #[test]
    fn location_and_domain_filters_are_validated() {
        assert!(TinyFishConfig::new().unwrap().with_location("USA").is_err());
        assert!(TinyFishConfig::new()
            .unwrap()
            .with_include_domains(["https://example.com"])
            .is_err());
        let config = TinyFishConfig::new().unwrap().with_location("us").unwrap();
        assert_eq!(config.location.as_deref(), Some("US"));
    }
}
