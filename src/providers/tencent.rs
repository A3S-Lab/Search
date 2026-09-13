//! Native Tencent Cloud Web Search API provider.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, time_window, JsonSearchApi};
use super::protocol::{
    bounded_relevance, sanitize_provider_multiline_text, sanitize_provider_text_with_secrets,
    strip_simple_markup, validated_domain, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use crate::{ProviderErrorKind, Result, ResultType, SearchImage};

const PROVIDER_ID: &str = "tencent";
const DEFAULT_ENDPOINT: &str = "https://api.wsa.cloud.tencent.com/SearchPro";

/// Vertical source filter accepted by Tencent Cloud SearchPro premium plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TencentIndustry {
    /// Government and party sources.
    Government,
    /// Authoritative news.
    News,
    /// English academic sources.
    Academic,
    /// Finance sources.
    Finance,
}

impl TencentIndustry {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Government => "gov",
            Self::News => "news",
            Self::Academic => "acad",
            Self::Finance => "finance",
        }
    }
}

/// Typed Tencent Cloud SearchPro request defaults and credentials.
#[derive(Debug, Clone)]
pub struct TencentConfig {
    api: JsonSearchApi,
    max_results: Option<u8>,
    site: Option<String>,
    industry: Option<TencentIndustry>,
}

impl TencentConfig {
    /// Creates the default SearchPro configuration.
    ///
    /// `TENCENTCLOUD_WSA_APIKEY` is required. Result count is omitted unless
    /// set, because the documented `Cnt` values are premium-plan only.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(
                PROVIDER_ID,
                "Tencent Cloud Search",
                DEFAULT_ENDPOINT,
                "TENCENTCLOUD_WSA_APIKEY",
            )?,
            max_results: None,
            site: None,
            industry: None,
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

    /// Sets the premium result cap. Allowed values are 10, 20, 30, 40, and 50.
    ///
    /// The Tencent wire field remains `Cnt`. It is omitted unless set, because
    /// those counts are premium-plan only.
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !matches!(max_results, 10 | 20 | 30 | 40 | 50) {
            return Err(invalid_config(
                PROVIDER_ID,
                "Tencent Cloud Search max_results must be 10, 20, 30, 40, or 50",
            ));
        }
        self.max_results = Some(max_results);
        Ok(self)
    }

    /// Restricts natural results to one site.
    pub fn with_site(mut self, site: impl Into<String>) -> Result<Self> {
        let site = validated_domain(&site.into()).ok_or_else(|| {
            invalid_config(PROVIDER_ID, "Tencent Cloud Search site must be a hostname")
        })?;
        self.site = Some(site);
        Ok(self)
    }

    /// Sets the premium industry filter.
    pub fn with_industry(mut self, industry: TencentIndustry) -> Self {
        self.industry = Some(industry);
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

/// Native Rust implementation of Tencent Cloud SearchPro API-key access.
#[derive(Debug)]
pub struct TencentProvider {
    config: TencentConfig,
    client: ProviderHttpClient,
}

impl TencentProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: TencentConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `TENCENTCLOUD_WSA_APIKEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(TencentConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for TencentProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "Tencent Cloud Search",
            "https://cloud.tencent.com/product/wsa",
            ProviderCapabilities::new()
                .with_time_range(true)
                .with_images(true)
                .with_full_text(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, 2_000)?;
        let payload = TencentRequest {
            query,
            mode: 0,
            cnt: self.config.max_results,
            site: self.config.site.as_deref(),
            industry: self.config.industry.map(TencentIndustry::as_str),
            from_time: request.time_range.and_then(from_time),
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(
                reply.reject("Tencent Cloud Search request failed", |code, _, _| {
                    code.and_then(tencent_code_kind)
                }),
            );
        }

        let envelope: Value = reply.decode()?;
        let payload = envelope.get("Response").unwrap_or(&envelope);
        if payload.get("Error").is_some() {
            return Err(
                reply.reject("Tencent Cloud Search request failed", |code, _, _| {
                    code.and_then(tencent_code_kind)
                        .or(Some(ProviderErrorKind::InvalidRequest))
                }),
            );
        }
        let parsed: TencentResponse =
            serde_json::from_value(payload.clone()).map_err(|_| reply.contract_error())?;
        let limit = self
            .config
            .max_results
            .map(usize::from)
            .unwrap_or(usize::MAX);
        let results = parsed
            .pages
            .unwrap_or_default()
            .into_iter()
            .take(limit)
            .filter_map(|page| adapt_page(page, &secrets))
            .collect();
        let request_id = parsed
            .request_id
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .filter(|value| !value.is_empty());
        let mut metadata = BTreeMap::new();
        if let Some(version) = parsed.version.filter(|value| !value.trim().is_empty()) {
            metadata.insert(
                "version".to_string(),
                Value::String(sanitize_provider_text_with_secrets(&version, 64, &secrets)),
            );
        }
        if let Some(message) = parsed.msg.filter(|value| !value.trim().is_empty()) {
            metadata.insert(
                "message".to_string(),
                Value::String(sanitize_provider_text_with_secrets(&message, 200, &secrets)),
            );
        }
        Ok(reply.seal(ProviderResponse {
            results,
            report: ProviderReport {
                request_id,
                metadata,
                ..Default::default()
            },
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct TencentRequest<'a> {
    #[serde(rename = "Query")]
    query: &'a str,
    #[serde(rename = "Mode")]
    mode: u8,
    #[serde(rename = "Cnt", skip_serializing_if = "Option::is_none")]
    cnt: Option<u8>,
    #[serde(rename = "Site", skip_serializing_if = "Option::is_none")]
    site: Option<&'a str>,
    #[serde(rename = "Industry", skip_serializing_if = "Option::is_none")]
    industry: Option<&'static str>,
    #[serde(rename = "FromTime", skip_serializing_if = "Option::is_none")]
    from_time: Option<u64>,
}

#[derive(Deserialize)]
struct TencentResponse {
    #[serde(rename = "Pages")]
    pages: Option<Vec<Value>>,
    #[serde(rename = "RequestId")]
    request_id: Option<String>,
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "Msg")]
    msg: Option<String>,
}

#[derive(Deserialize)]
struct TencentPage {
    title: Option<String>,
    url: Option<String>,
    passage: Option<String>,
    content: Option<String>,
    date: Option<String>,
    score: Option<f64>,
    favicon: Option<String>,
    images: Option<Vec<String>>,
    pics: Option<Vec<Value>>,
}

fn adapt_page(page: Value, secrets: &[&str]) -> Option<ProviderResult> {
    let page = match page {
        Value::String(page) => serde_json::from_str::<TencentPage>(&page).ok()?,
        other => serde_json::from_value::<TencentPage>(other).ok()?,
    };
    let url = page.url.as_deref().and_then(validated_web_url)?;
    let title = JsonSearchApi::clean(page.title.as_deref().unwrap_or(&url), 300, secrets);
    let snippet = JsonSearchApi::clean(page.passage.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
    if let Some(score) = page.score.and_then(bounded_relevance) {
        result = result.with_relevance_score(score);
    }
    if let Some(date) = page.date {
        let date = JsonSearchApi::clean(&date, 64, secrets);
        if !date.is_empty() {
            result = result.with_published_date(date);
        }
    }
    if let Some(content) = page.content {
        let content =
            sanitize_provider_text_with_secrets(&strip_simple_markup(&content), 32 * 1024, secrets);
        let content = sanitize_provider_multiline_text(&content, 32 * 1024);
        if !content.is_empty() {
            result = result.with_full_text(content);
        }
    }
    if let Some(favicon) = page.favicon.as_deref().and_then(validated_web_url) {
        result = result.with_favicon(favicon);
    }
    for image in page.images.unwrap_or_default() {
        if let Some(image) = validated_web_url(&image) {
            result = result.with_image(SearchImage::new(image));
        }
    }
    for image in page.pics.unwrap_or_default() {
        let url = image
            .as_str()
            .or_else(|| image.get("url").and_then(Value::as_str))
            .or_else(|| image.get("origin_url").and_then(Value::as_str));
        if let Some(image) = url.and_then(validated_web_url) {
            result = result.with_image(SearchImage::new(image));
        }
    }
    Some(result)
}

fn from_time(range: crate::TimeRange) -> Option<u64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    now.checked_sub(time_window(range).seconds)
}

fn tencent_code_kind(code: &str) -> Option<ProviderErrorKind> {
    match code {
        "UnauthorizedOperation" | "AuthFailure" => Some(ProviderErrorKind::Authentication),
        "ResourceNotFound" => Some(ProviderErrorKind::Permission),
        "ResourceUnavailable" => Some(ProviderErrorKind::Quota),
        "RequestLimitExceeded" => Some(ProviderErrorKind::RateLimited),
        "InvalidParameter" => Some(ProviderErrorKind::InvalidRequest),
        "InternalError" => Some(ProviderErrorKind::Unavailable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_count_and_site_are_validated() {
        assert!(TencentConfig::new().unwrap().with_max_results(15).is_err());
        assert!(TencentConfig::new()
            .unwrap()
            .with_site("https://qq.com")
            .is_err());
        let config = TencentConfig::new()
            .unwrap()
            .with_max_results(20)
            .unwrap()
            .with_site("news.qq.com")
            .unwrap();
        assert_eq!(config.max_results, Some(20));
        assert_eq!(config.site.as_deref(), Some("news.qq.com"));
    }
}
