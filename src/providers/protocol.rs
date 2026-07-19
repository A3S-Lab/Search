//! Public search provider protocol.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::ProviderReadiness;
use crate::{Result, ResultType, SafeSearch, SearchImage, SearchQuery, SearchUsage, TimeRange};

/// Stable capabilities advertised by a provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProviderCapabilities {
    /// Requests may be made without an API credential.
    pub anonymous: bool,
    /// Provider supports result pagination.
    pub paging: bool,
    /// Provider accepts a safe-search option.
    pub safe_search: bool,
    /// Provider accepts a time-range option.
    pub time_range: bool,
    /// Provider may return direct answers.
    pub answers: bool,
    /// Provider may return query-related and per-result images.
    pub images: bool,
    /// Provider may return full source content.
    pub full_text: bool,
    /// Provider may return billing or quota usage.
    pub usage: bool,
}

impl ProviderCapabilities {
    /// Creates a capability set with every optional feature disabled.
    pub const fn new() -> Self {
        Self {
            anonymous: false,
            paging: false,
            safe_search: false,
            time_range: false,
            answers: false,
            images: false,
            full_text: false,
            usage: false,
        }
    }

    /// Declares support for credential-free requests.
    pub const fn with_anonymous(mut self, enabled: bool) -> Self {
        self.anonymous = enabled;
        self
    }

    /// Declares support for result pagination.
    pub const fn with_paging(mut self, enabled: bool) -> Self {
        self.paging = enabled;
        self
    }

    /// Declares support for safe-search controls.
    pub const fn with_safe_search(mut self, enabled: bool) -> Self {
        self.safe_search = enabled;
        self
    }

    /// Declares support for time-range filters.
    pub const fn with_time_range(mut self, enabled: bool) -> Self {
        self.time_range = enabled;
        self
    }

    /// Declares support for direct answers.
    pub const fn with_answers(mut self, enabled: bool) -> Self {
        self.answers = enabled;
        self
    }

    /// Declares support for query or result images.
    pub const fn with_images(mut self, enabled: bool) -> Self {
        self.images = enabled;
        self
    }

    /// Declares support for full source content.
    pub const fn with_full_text(mut self, enabled: bool) -> Self {
        self.full_text = enabled;
        self
    }

    /// Declares support for billing or quota usage metadata.
    pub const fn with_usage(mut self, enabled: bool) -> Self {
        self.usage = enabled;
        self
    }
}

/// Static identity and capabilities for a provider implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderDescriptor {
    /// Stable lowercase provider identifier.
    pub id: &'static str,
    /// Human-readable provider name.
    pub name: &'static str,
    /// Official provider homepage.
    pub homepage: &'static str,
    /// Supported provider features.
    pub capabilities: ProviderCapabilities,
}

impl ProviderDescriptor {
    /// Creates a provider descriptor.
    pub const fn new(
        id: &'static str,
        name: &'static str,
        homepage: &'static str,
        capabilities: ProviderCapabilities,
    ) -> Self {
        Self {
            id,
            name,
            homepage,
            capabilities,
        }
    }
}

/// Provider-neutral request derived from [`SearchQuery`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderRequest {
    /// Search terms.
    pub query: String,
    /// Requested language or locale.
    pub language: Option<String>,
    /// Requested safe-search level.
    pub safe_search: SafeSearch,
    /// Requested page, starting at one.
    pub page: u32,
    /// Requested time range.
    pub time_range: Option<TimeRange>,
}

impl ProviderRequest {
    /// Creates a request with provider-neutral defaults.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            language: None,
            safe_search: SafeSearch::Off,
            page: 1,
            time_range: None,
        }
    }

    /// Sets a language or locale hint.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Sets the requested safe-search level.
    pub fn with_safe_search(mut self, safe_search: SafeSearch) -> Self {
        self.safe_search = safe_search;
        self
    }

    /// Sets the requested result page.
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    /// Sets the requested time range.
    pub fn with_time_range(mut self, time_range: TimeRange) -> Self {
        self.time_range = Some(time_range);
        self
    }
}

impl From<&SearchQuery> for ProviderRequest {
    fn from(query: &SearchQuery) -> Self {
        let mut request = Self::new(query.query.clone())
            .with_safe_search(query.safesearch)
            .with_page(query.page);
        request.language = query.language.clone();
        request.time_range = query.time_range;
        request
    }
}

/// One provider-native result before adaptation to [`crate::SearchResult`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProviderResult {
    /// Result URL.
    pub url: String,
    /// Result title.
    pub title: String,
    /// Result snippet.
    pub snippet: String,
    /// Result kind.
    pub result_type: ResultType,
    /// Provider-returned full source content.
    pub full_text: Option<String>,
    /// Native relevance in the inclusive `0.0..=1.0` range.
    pub relevance_score: Option<f64>,
    /// Thumbnail URL.
    pub thumbnail: Option<String>,
    /// Published date.
    pub published_date: Option<String>,
    /// Favicon URL.
    pub favicon: Option<String>,
    /// Images extracted from the result page.
    pub images: Vec<SearchImage>,
}

impl ProviderResult {
    /// Creates a web result.
    pub fn new(
        url: impl Into<String>,
        title: impl Into<String>,
        snippet: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
            snippet: snippet.into(),
            result_type: ResultType::Web,
            full_text: None,
            relevance_score: None,
            thumbnail: None,
            published_date: None,
            favicon: None,
            images: Vec::new(),
        }
    }

    /// Attaches full source content.
    pub fn with_full_text(mut self, full_text: impl Into<String>) -> Self {
        self.full_text = Some(full_text.into());
        self
    }

    /// Sets the provider result kind.
    pub fn with_result_type(mut self, result_type: ResultType) -> Self {
        self.result_type = result_type;
        self
    }

    /// Attaches provider-native relevance.
    pub fn with_relevance_score(mut self, relevance_score: f64) -> Self {
        self.relevance_score = Some(relevance_score);
        self
    }

    /// Attaches a thumbnail URL.
    pub fn with_thumbnail(mut self, thumbnail: impl Into<String>) -> Self {
        self.thumbnail = Some(thumbnail.into());
        self
    }

    /// Attaches a published date.
    pub fn with_published_date(mut self, published_date: impl Into<String>) -> Self {
        self.published_date = Some(published_date.into());
        self
    }

    /// Attaches a favicon URL.
    pub fn with_favicon(mut self, favicon: impl Into<String>) -> Self {
        self.favicon = Some(favicon.into());
        self
    }

    /// Adds an image extracted from the result page.
    pub fn with_image(mut self, image: SearchImage) -> Self {
        crate::result::merge_image(&mut self.images, image);
        self
    }
}

/// Structured metadata from one provider response.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct ProviderReport {
    /// Provider request identifier.
    ///
    /// [`super::ProviderEngine`] removes control characters and bounds this
    /// field before exposing it as a search report.
    pub request_id: Option<String>,
    /// Provider-reported total matches.
    pub total_results: Option<u64>,
    /// Provider-side response time in milliseconds.
    pub response_time_ms: Option<u64>,
    /// Billing or quota usage.
    ///
    /// Usage values must be finite and non-negative. The provider adapter
    /// discards invalid values defensively.
    pub usage: Option<SearchUsage>,
    /// Provider-specific metadata.
    ///
    /// Implementations must exclude secrets and keep values reasonably
    /// bounded. [`super::ProviderEngine`] applies defensive output bounds.
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl ProviderReport {
    /// Creates an empty provider report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches a provider request identifier.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Attaches the provider's total result count.
    pub fn with_total_results(mut self, total_results: u64) -> Self {
        self.total_results = Some(total_results);
        self
    }

    /// Attaches provider-side response time in milliseconds.
    pub fn with_response_time_ms(mut self, response_time_ms: u64) -> Self {
        self.response_time_ms = Some(response_time_ms);
        self
    }

    /// Attaches billing or quota usage.
    pub fn with_usage(mut self, usage: SearchUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Adds provider-specific metadata.
    ///
    /// Implementations must not include credentials or other secrets.
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Provider response before adaptation to engine output.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct ProviderResponse {
    /// Search results.
    pub results: Vec<ProviderResult>,
    /// Query suggestions.
    pub suggestions: Vec<String>,
    /// Direct answers.
    pub answers: Vec<String>,
    /// Query-related images.
    pub images: Vec<SearchImage>,
    /// Structured request report.
    pub report: ProviderReport,
}

impl ProviderResponse {
    /// Creates an empty provider response.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one provider result.
    pub fn with_result(mut self, result: ProviderResult) -> Self {
        self.results.push(result);
        self
    }

    /// Replaces provider results.
    pub fn with_results(mut self, results: Vec<ProviderResult>) -> Self {
        self.results = results;
        self
    }

    /// Adds a query suggestion.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Adds a direct answer.
    pub fn with_answer(mut self, answer: impl Into<String>) -> Self {
        self.answers.push(answer.into());
        self
    }

    /// Adds a query-related image, merging duplicate URLs.
    pub fn with_image(mut self, image: SearchImage) -> Self {
        crate::result::merge_image(&mut self.images, image);
        self
    }

    /// Attaches structured provider execution metadata.
    pub fn with_report(mut self, report: ProviderReport) -> Self {
        self.report = report;
        self
    }
}

/// Protocol implemented by native third-party search providers.
#[async_trait]
pub trait SearchProvider: Send + Sync {
    /// Returns static provider identity and capabilities.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Reports whether the provider can currently accept requests.
    fn readiness(&self) -> ProviderReadiness;

    /// Executes one provider-neutral request.
    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse>;
}

pub(crate) fn sanitize_provider_text(value: &str, max_chars: usize) -> String {
    let mut sanitized = String::with_capacity(value.len().min(max_chars.saturating_mul(4)));
    let mut written = 0usize;
    let mut pending_space = false;

    for character in value.chars() {
        if character.is_whitespace() {
            pending_space = !sanitized.is_empty();
            continue;
        }
        if character.is_control() {
            continue;
        }
        if pending_space {
            if written.saturating_add(1) >= max_chars {
                break;
            }
            sanitized.push(' ');
            written += 1;
            pending_space = false;
        }
        if written >= max_chars {
            break;
        }
        sanitized.push(character);
        written += 1;
    }

    sanitized
}

pub(crate) fn sanitize_provider_multiline_text(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(max_chars)
        .collect()
}

pub(crate) fn sanitize_provider_text_with_secrets(
    value: &str,
    max_chars: usize,
    secrets: &[&str],
) -> String {
    let mut redacted = value.to_string();
    for secret in secrets.iter().copied().filter(|secret| !secret.is_empty()) {
        redacted = redacted.replace(secret, "[REDACTED]");
    }
    sanitize_provider_text(&redacted, max_chars)
}

pub(crate) fn validated_web_url(value: &str) -> Option<String> {
    const MAX_WEB_URL_BYTES: usize = 16 * 1024;

    let value = value.trim();
    if value.is_empty() || value.len() > MAX_WEB_URL_BYTES {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return None;
    }
    Some(url.to_string())
}

pub(crate) fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_sanitization_is_bounded_without_trailing_whitespace() {
        assert_eq!(
            sanitize_provider_text(" \n alpha\u{0} \t beta  ", 64),
            "alpha beta"
        );
        assert_eq!(sanitize_provider_text("alpha beta", 6), "alpha");
        assert_eq!(sanitize_provider_text("中文 测试", 4), "中文 测");
    }

    #[test]
    fn multiline_sanitization_preserves_only_safe_layout_controls() {
        assert_eq!(
            sanitize_provider_multiline_text(" \r\nalpha\u{0}\n\tbeta\r\n ", 64),
            "alpha\n\tbeta"
        );
    }

    #[test]
    fn web_urls_are_bounded_and_reject_credentials_or_unsafe_schemes() {
        assert_eq!(
            validated_web_url("https://example.com/path").as_deref(),
            Some("https://example.com/path")
        );
        assert!(validated_web_url("javascript:alert(1)").is_none());
        assert!(validated_web_url("https://user:secret@example.com").is_none());
        assert!(
            validated_web_url(&format!("https://example.com/{}", "x".repeat(17 * 1024))).is_none()
        );
    }
}
