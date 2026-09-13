//! Native Firecrawl Search API provider.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, JsonSearchApi};
use super::protocol::{
    non_empty, sanitize_provider_multiline_text, sanitize_provider_text_with_secrets,
    strip_simple_markup, validated_hostnames, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use crate::{ProviderErrorKind, Result, ResultType, SearchImage, SearchUsage, TimeRange};
use url::Url;

const PROVIDER_ID: &str = "firecrawl";
const DEFAULT_ENDPOINT: &str = "https://api.firecrawl.dev/v2/search";
const MAX_QUERY_CHARS: usize = 500;
const MAX_LIMIT: u8 = 100;
const MAX_DOMAINS: usize = 50;
const MAX_LOCATION_CHARS: usize = 200;

/// A Firecrawl search source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FirecrawlSource {
    /// Ordinary web results. This is the API default.
    Web,
    /// News results.
    News,
    /// Image results.
    Images,
}

impl FirecrawlSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::News => "news",
            Self::Images => "images",
        }
    }
}

/// A Firecrawl result category filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FirecrawlCategory {
    /// GitHub repositories, issues, and documentation.
    Github,
    /// Academic and research websites.
    Research,
    /// PDF documents.
    Pdf,
    /// Developer Index records. Exclusive with other categories.
    Developer,
}

impl FirecrawlCategory {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Research => "research",
            Self::Pdf => "pdf",
            Self::Developer => "developer",
        }
    }
}

/// Typed Firecrawl Search request defaults and credentials.
#[derive(Debug, Clone)]
pub struct FirecrawlConfig {
    api: JsonSearchApi,
    max_results: u8,
    location: Option<String>,
    country: Option<String>,
    include_domains: Vec<String>,
    exclude_domains: Vec<String>,
    sources: Vec<FirecrawlSource>,
    categories: Vec<FirecrawlCategory>,
    include_markdown: bool,
}

impl FirecrawlConfig {
    /// Creates the default Firecrawl configuration.
    ///
    /// `FIRECRAWL_API_KEY` is required. The default request asks only for web
    /// search metadata. Markdown scraping is opt-in because it bills and waits
    /// for a page fetch of every result.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(
                PROVIDER_ID,
                "Firecrawl",
                DEFAULT_ENDPOINT,
                "FIRECRAWL_API_KEY",
            )?,
            max_results: 10,
            location: None,
            country: None,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            sources: vec![FirecrawlSource::Web],
            categories: Vec::new(),
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

    /// Sets the result cap in the documented `1..=100` range.
    ///
    /// The Firecrawl wire field remains `limit`.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=MAX_LIMIT).contains(&max_results) {
            return Err(invalid_config(
                PROVIDER_ID,
                "Firecrawl max_results must be between 1 and 100",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Sets the geo location string Firecrawl documents for search.
    pub fn with_location(mut self, location: impl Into<String>) -> Result<Self> {
        self.location = Some(validated_location(location.into())?);
        Ok(self)
    }

    /// Sets an ISO-style country code. Firecrawl also accepts `UK`.
    pub fn with_country(mut self, country: impl Into<String>) -> Result<Self> {
        self.country = Some(validated_country(country.into())?);
        Ok(self)
    }

    /// Restricts results to hostnames. Cannot be combined with exclusions.
    pub fn with_include_domains<I, S>(mut self, domains: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if !self.exclude_domains.is_empty() {
            return Err(invalid_config(
                PROVIDER_ID,
                "Firecrawl include_domains cannot be combined with exclude_domains",
            ));
        }
        self.include_domains = validated_domains(domains)?;
        Ok(self)
    }

    /// Excludes hostnames. Cannot be combined with inclusions.
    pub fn with_exclude_domains<I, S>(mut self, domains: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if !self.include_domains.is_empty() {
            return Err(invalid_config(
                PROVIDER_ID,
                "Firecrawl exclude_domains cannot be combined with include_domains",
            ));
        }
        self.exclude_domains = validated_domains(domains)?;
        Ok(self)
    }

    /// Replaces the requested sources. An empty list is rejected.
    pub fn with_sources(mut self, sources: Vec<FirecrawlSource>) -> Result<Self> {
        if sources.is_empty() {
            return Err(invalid_config(
                PROVIDER_ID,
                "Firecrawl sources must include at least one of web, news, or images",
            ));
        }
        self.sources = sources;
        Ok(self)
    }

    /// Replaces category filters. `developer` cannot be combined with others.
    pub fn with_categories(mut self, categories: Vec<FirecrawlCategory>) -> Result<Self> {
        if categories.contains(&FirecrawlCategory::Developer) && categories.len() > 1 {
            return Err(invalid_config(
                PROVIDER_ID,
                "Firecrawl developer category cannot be combined with other categories",
            ));
        }
        self.categories = categories;
        Ok(self)
    }

    /// Requests markdown page text for each result. This bills a scrape.
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

/// Native Rust implementation of the Firecrawl Search API.
#[derive(Debug)]
pub struct FirecrawlProvider {
    config: FirecrawlConfig,
    client: ProviderHttpClient,
}

impl FirecrawlProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: FirecrawlConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `FIRECRAWL_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(FirecrawlConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for FirecrawlProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        let mut capabilities = ProviderCapabilities::new()
            .with_time_range(true)
            .with_usage(true);
        if self.config.sources.contains(&FirecrawlSource::Images) {
            capabilities = capabilities.with_images(true);
        }
        if self.config.include_markdown {
            capabilities = capabilities.with_full_text(true);
        }
        ProviderDescriptor::new(
            PROVIDER_ID,
            "Firecrawl",
            "https://www.firecrawl.dev/",
            capabilities,
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, MAX_QUERY_CHARS)?;
        let payload = FirecrawlRequest {
            query,
            limit: self.config.max_results,
            tbs: request.time_range.map(tbs_label),
            location: self.config.location.as_deref(),
            country: self.config.country.as_deref(),
            include_domains: &self.config.include_domains,
            exclude_domains: &self.config.exclude_domains,
            sources: sources_wire(&self.config.sources),
            categories: self
                .config
                .categories
                .iter()
                .copied()
                .map(FirecrawlCategory::as_str)
                .collect(),
            scrape_options: self
                .config
                .include_markdown
                .then_some(FirecrawlScrapeOptions {
                    formats: [FirecrawlFormat { kind: "markdown" }],
                    only_main_content: true,
                }),
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(
                reply.reject("Firecrawl request failed", |code, message, _| {
                    firecrawl_code_kind(code, message)
                }),
            );
        }

        let payload: Value = reply.decode()?;
        match payload.get("success") {
            Some(Value::Bool(false)) => {
                return Err(
                    reply.reject("Firecrawl rejected the request", |code, message, _| {
                        firecrawl_code_kind(code, message)
                            .or(Some(ProviderErrorKind::InvalidRequest))
                    }),
                );
            }
            Some(Value::Bool(true)) => {}
            _ => return Err(reply.contract_error()),
        }
        let document: FirecrawlDocument =
            serde_json::from_value(payload).map_err(|_| reply.contract_error())?;
        let data = document.data.unwrap_or_default();
        let mut results = Vec::new();
        for hit in data.web.into_iter().chain(data.developer) {
            if let Some(result) = adapt_web(hit, &secrets) {
                results.push(result);
            }
        }
        for hit in data.news {
            if let Some(result) = adapt_news(hit, &secrets) {
                results.push(result);
            }
        }
        for hit in data.images {
            if let Some(result) = adapt_image(hit, &secrets) {
                results.push(result);
            }
        }
        results.truncate(usize::from(self.config.max_results) * self.config.sources.len().max(1));
        let request_id = document
            .id
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .filter(|value| !value.is_empty());
        let mut report = ProviderReport {
            request_id,
            usage: document.credits_used.and_then(usage_from_credits),
            ..Default::default()
        };
        if let Some(warning) = document.warning.as_deref() {
            let warning = JsonSearchApi::clean(warning, 300, &secrets);
            if !warning.is_empty() {
                report
                    .metadata
                    .insert("warning".to_string(), warning.into());
            }
        }
        Ok(reply.seal(ProviderResponse {
            results,
            report,
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct FirecrawlRequest<'a> {
    query: &'a str,
    limit: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    tbs: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<&'a str>,
    #[serde(rename = "includeDomains", skip_serializing_if = "slice_is_empty")]
    include_domains: &'a [String],
    #[serde(rename = "excludeDomains", skip_serializing_if = "slice_is_empty")]
    exclude_domains: &'a [String],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    categories: Vec<&'static str>,
    #[serde(rename = "scrapeOptions", skip_serializing_if = "Option::is_none")]
    scrape_options: Option<FirecrawlScrapeOptions>,
}

#[derive(Serialize)]
struct FirecrawlScrapeOptions {
    formats: [FirecrawlFormat; 1],
    #[serde(rename = "onlyMainContent")]
    only_main_content: bool,
}

#[derive(Serialize)]
struct FirecrawlFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize, Default)]
struct FirecrawlDocument {
    data: Option<FirecrawlData>,
    id: Option<String>,
    #[serde(rename = "creditsUsed")]
    credits_used: Option<f64>,
    warning: Option<String>,
}

#[derive(Deserialize, Default)]
struct FirecrawlData {
    #[serde(default)]
    web: Vec<FirecrawlWebHit>,
    #[serde(default)]
    news: Vec<FirecrawlNewsHit>,
    #[serde(default)]
    images: Vec<FirecrawlImageHit>,
    #[serde(default)]
    developer: Vec<FirecrawlWebHit>,
}

#[derive(Deserialize)]
struct FirecrawlWebHit {
    title: Option<String>,
    description: Option<String>,
    url: Option<String>,
    markdown: Option<String>,
    category: Option<String>,
}

#[derive(Deserialize)]
struct FirecrawlNewsHit {
    title: Option<String>,
    snippet: Option<String>,
    url: Option<String>,
    date: Option<String>,
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
}

#[derive(Deserialize)]
struct FirecrawlImageHit {
    title: Option<String>,
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
    url: Option<String>,
}

fn adapt_web(hit: FirecrawlWebHit, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit.url.as_deref().and_then(validated_web_url)?;
    let title = titled(hit.title.as_deref(), &url, secrets);
    let snippet = JsonSearchApi::clean(hit.description.as_deref().unwrap_or(""), 2_000, secrets);
    let result_type = match hit.category.as_deref() {
        Some("pdf") => ResultType::File,
        _ => ResultType::Web,
    };
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(result_type);
    if let Some(markdown) = non_empty(hit.markdown) {
        result = attach_full_text(result, &markdown, secrets);
    }
    Some(result)
}

fn adapt_news(hit: FirecrawlNewsHit, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit.url.as_deref().and_then(validated_web_url)?;
    let title = titled(hit.title.as_deref(), &url, secrets);
    let snippet = JsonSearchApi::clean(hit.snippet.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::News);
    if let Some(date) = non_empty(hit.date) {
        let date = JsonSearchApi::clean(&date, 64, secrets);
        if !date.is_empty() {
            result = result.with_published_date(date);
        }
    }
    if let Some(image) = hit.image_url.as_deref().and_then(validated_web_url) {
        result = result.with_thumbnail(image);
    }
    Some(result)
}

fn adapt_image(hit: FirecrawlImageHit, secrets: &[&str]) -> Option<ProviderResult> {
    let page = hit.url.as_deref().and_then(validated_web_url);
    let image = hit.image_url.as_deref().and_then(validated_web_url);
    let url = page.clone().or_else(|| image.clone())?;
    let title = titled(hit.title.as_deref(), &url, secrets);
    let mut result =
        ProviderResult::new(url, title, String::new()).with_result_type(ResultType::Image);
    if let Some(image) = image {
        result = result
            .with_thumbnail(image.clone())
            .with_image(SearchImage::new(image));
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

fn sources_wire(sources: &[FirecrawlSource]) -> Vec<&'static str> {
    if sources == [FirecrawlSource::Web] {
        Vec::new()
    } else {
        sources
            .iter()
            .copied()
            .map(FirecrawlSource::as_str)
            .collect()
    }
}

fn tbs_label(range: TimeRange) -> &'static str {
    match range {
        TimeRange::Day => "qdr:d",
        TimeRange::Week => "qdr:w",
        TimeRange::Month => "qdr:m",
        TimeRange::Year => "qdr:y",
    }
}

fn usage_from_credits(credits: f64) -> Option<SearchUsage> {
    (credits.is_finite() && credits >= 0.0).then(|| SearchUsage::new().with_credits(credits))
}

fn firecrawl_code_kind(code: Option<&str>, message: Option<&str>) -> Option<ProviderErrorKind> {
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

fn validated_location(value: String) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MAX_LOCATION_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(invalid_config(
            PROVIDER_ID,
            "Firecrawl location must be 1 to 200 characters without control characters",
        ));
    }
    Ok(value.to_string())
}

fn validated_country(value: String) -> Result<String> {
    let value = value.trim();
    if value.len() != 2
        || !value
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return Err(invalid_config(
            PROVIDER_ID,
            "Firecrawl country must be a two-letter code",
        ));
    }
    Ok(value.to_ascii_uppercase())
}

fn validated_domains<I, S>(domains: I) -> Result<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let domains = validated_hostnames(domains)
        .ok_or_else(|| invalid_config(PROVIDER_ID, "Firecrawl domain filters must be hostnames"))?;
    if domains.is_empty() || domains.len() > MAX_DOMAINS {
        return Err(invalid_config(
            PROVIDER_ID,
            "Firecrawl domain filters accept 1 to 50 hostnames",
        ));
    }
    Ok(domains)
}

fn slice_is_empty(value: &[String]) -> bool {
    value.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_country_and_domain_filters_are_validated() {
        assert!(FirecrawlConfig::new().unwrap().with_max_results(0).is_err());
        assert!(FirecrawlConfig::new()
            .unwrap()
            .with_max_results(101)
            .is_err());
        assert!(FirecrawlConfig::new().unwrap().with_country("USA").is_err());
        assert_eq!(
            FirecrawlConfig::new()
                .unwrap()
                .with_country("uk")
                .unwrap()
                .country
                .as_deref(),
            Some("UK")
        );
        assert!(FirecrawlConfig::new()
            .unwrap()
            .with_include_domains(["https://example.com"])
            .is_err());
        let config = FirecrawlConfig::new()
            .unwrap()
            .with_include_domains(["example.com"])
            .unwrap();
        assert!(config.with_exclude_domains(["other.com"]).is_err());
        assert!(FirecrawlConfig::new()
            .unwrap()
            .with_categories(vec![
                FirecrawlCategory::Developer,
                FirecrawlCategory::Research
            ])
            .is_err());
    }
}
