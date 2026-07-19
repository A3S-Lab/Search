//! Search result types.

use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, HashSet};
use url::form_urlencoded::Serializer as FormSerializer;

/// An image returned for a search query or extracted from a result page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchImage {
    /// Absolute image URL.
    pub url: String,
    /// Optional provider-supplied image description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl SearchImage {
    /// Creates an image without a description.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            description: None,
        }
    }

    /// Attaches a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Type of search result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultType {
    /// Standard web result.
    #[default]
    Web,
    /// Image result.
    Image,
    /// Video result.
    Video,
    /// News article.
    News,
    /// Map/location result.
    Map,
    /// File download.
    File,
    /// Direct answer.
    Answer,
    /// Infobox (rich information panel).
    Infobox,
    /// Suggestion.
    Suggestion,
}

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchResult {
    /// Result URL.
    pub url: String,
    /// Result title.
    pub title: String,
    /// Result description/snippet.
    pub content: String,
    /// Type of result.
    pub result_type: ResultType,
    /// Engines that returned this result.
    #[serde(serialize_with = "serialize_sorted_engines")]
    pub engines: HashSet<String>,
    /// Positions in each engine's results.
    pub positions: Vec<u32>,
    /// Calculated score for ranking.
    pub score: f64,
    /// Native relevance reported by the source before meta-search aggregation.
    ///
    /// Providers should use a finite value in the inclusive `0.0..=1.0`
    /// range. The aggregator clamps the value defensively and keeps the
    /// strongest value when duplicate URLs are merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance_score: Option<f64>,
    /// Thumbnail URL (for images/videos).
    pub thumbnail: Option<String>,
    /// Published date (for news).
    pub published_date: Option<String>,
    /// Favicon URL returned by the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    /// Images extracted from this result page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<SearchImage>,
    /// Provider-supplied or extracted main article text.
    ///
    /// Native providers may populate this field directly. For snippet-only
    /// engines, [`enrich_full_text`](crate::enrich_full_text) can fetch and
    /// extract the page body.
    #[serde(default)]
    pub full_text: Option<String>,
}

impl SearchResult {
    /// Creates a new search result.
    pub fn new(
        url: impl Into<String>,
        title: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
            content: content.into(),
            result_type: ResultType::Web,
            engines: HashSet::new(),
            positions: Vec::new(),
            score: 0.0,
            relevance_score: None,
            thumbnail: None,
            published_date: None,
            favicon: None,
            images: Vec::new(),
            full_text: None,
        }
    }

    /// Sets the result type.
    pub fn with_type(mut self, result_type: ResultType) -> Self {
        self.result_type = result_type;
        self
    }

    /// Adds an engine that returned this result.
    pub fn with_engine(mut self, engine: impl Into<String>, position: u32) -> Self {
        self.engines.insert(engine.into());
        self.positions.push(position);
        self
    }

    /// Sets the thumbnail URL.
    pub fn with_thumbnail(mut self, thumbnail: impl Into<String>) -> Self {
        self.thumbnail = Some(thumbnail.into());
        self
    }

    /// Sets the published date.
    pub fn with_published_date(mut self, date: impl Into<String>) -> Self {
        self.published_date = Some(date.into());
        self
    }

    /// Sets the favicon URL.
    pub fn with_favicon(mut self, favicon: impl Into<String>) -> Self {
        self.favicon = Some(favicon.into());
        self
    }

    /// Adds an image extracted from this result page.
    pub fn with_image(mut self, image: SearchImage) -> Self {
        merge_image(&mut self.images, image);
        self
    }

    /// Sets the native relevance reported by the source.
    pub fn with_relevance_score(mut self, score: f64) -> Self {
        self.relevance_score = Some(score);
        self
    }

    /// Returns a normalized URL for deduplication (without scheme and trailing slash).
    pub fn normalized_url(&self) -> String {
        let value = self.url.trim();
        match url::Url::parse(value).or_else(|_| url::Url::parse(&format!("https://{value}"))) {
            Ok(url) => normalize_parsed_url(&url),
            Err(_) => value
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .to_string(),
        }
    }
}

fn serialize_sorted_engines<S>(
    engines: &HashSet<String>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut engines: Vec<_> = engines.iter().collect();
    engines.sort_unstable();
    engines.serialize(serializer)
}

fn normalize_parsed_url(url: &url::Url) -> String {
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.");
    let port = match (url.scheme(), url.port()) {
        ("http", Some(80)) | ("https", Some(443)) | (_, None) => String::new(),
        (_, Some(port)) => format!(":{port}"),
    };
    let path = match url.path().trim_end_matches('/') {
        "" => "",
        "/" => "",
        path => path,
    };

    let mut query_pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| !is_tracking_param(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    query_pairs.sort();

    let query = if query_pairs.is_empty() {
        String::new()
    } else {
        let mut serializer = FormSerializer::new(String::new());
        for (key, value) in query_pairs {
            serializer.append_pair(&key, &value);
        }
        format!("?{}", serializer.finish())
    };

    format!("{host}{port}{path}{query}")
}

fn is_tracking_param(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_")
        || matches!(
            key.as_str(),
            "fbclid" | "gclid" | "dclid" | "msclkid" | "mc_cid" | "mc_eid" | "igshid"
        )
}

/// Provider billing or quota usage associated with one search request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchUsage {
    /// Provider-defined credits consumed by the request.
    ///
    /// Native provider adapters preserve only finite, non-negative values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<f64>,
}

impl SearchUsage {
    /// Creates an empty usage record.
    pub const fn new() -> Self {
        Self { credits: None }
    }

    /// Attaches provider-defined credits consumed by the request.
    pub const fn with_credits(mut self, credits: f64) -> Self {
        self.credits = Some(credits);
        self
    }
}

/// Structured execution metadata returned by an engine.
///
/// Common fields stay typed while `metadata` provides a namespaced extension
/// point for provider-specific, non-secret response information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchReport {
    /// Configured engine display name.
    pub engine: String,
    /// Stable provider identifier, when the engine adapts a provider API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Provider request identifier for support correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Total number of matches reported by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_results: Option<u64>,
    /// Provider-side response time in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
    /// Provider billing or quota usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SearchUsage>,
    /// Additional provider metadata.
    ///
    /// Provider adapters must exclude secrets and keep values bounded.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl SearchReport {
    /// Creates an empty report for an engine.
    pub fn new(engine: impl Into<String>) -> Self {
        Self {
            engine: engine.into(),
            provider: None,
            request_id: None,
            total_results: None,
            response_time_ms: None,
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Identifies the third-party provider behind this engine.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
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

    /// Attaches the provider-side response time.
    pub fn with_response_time_ms(mut self, response_time_ms: u64) -> Self {
        self.response_time_ms = Some(response_time_ms);
        self
    }

    /// Attaches provider billing or quota usage.
    pub fn with_usage(mut self, usage: SearchUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Adds provider-specific metadata.
    ///
    /// Callers must not include credentials or other secrets.
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Container for aggregated search results.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResults {
    /// Main search results.
    results: Vec<SearchResult>,
    /// Query suggestions.
    suggestions: Vec<String>,
    /// Direct answers.
    answers: Vec<String>,
    /// Query-related images returned independently of individual results.
    #[serde(default)]
    images: Vec<SearchImage>,
    /// Engine errors (engine name → error message).
    errors: Vec<(String, String)>,
    /// Structured per-engine execution reports.
    #[serde(default)]
    reports: Vec<SearchReport>,
    /// Number of results.
    pub count: usize,
    /// Search duration in milliseconds.
    pub duration_ms: u64,
}

impl SearchResults {
    /// Creates a new empty result container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a result.
    pub fn add_result(&mut self, result: SearchResult) {
        self.results.push(result);
        self.count = self.results.len();
    }

    /// Adds a suggestion unless an identical suggestion is already present.
    pub fn add_suggestion(&mut self, suggestion: impl Into<String>) {
        let suggestion = suggestion.into();
        if !self.suggestions.contains(&suggestion) {
            self.suggestions.push(suggestion);
        }
    }

    /// Adds an answer unless an identical answer is already present.
    pub fn add_answer(&mut self, answer: impl Into<String>) {
        let answer = answer.into();
        if !self.answers.contains(&answer) {
            self.answers.push(answer);
        }
    }

    /// Adds a query-related image, merging duplicate URLs deterministically.
    pub fn add_image(&mut self, image: SearchImage) {
        merge_image(&mut self.images, image);
    }

    /// Returns the results.
    pub fn items(&self) -> &[SearchResult] {
        &self.results
    }

    /// Returns mutable results.
    pub fn items_mut(&mut self) -> &mut Vec<SearchResult> {
        &mut self.results
    }

    /// Returns the suggestions.
    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }

    /// Returns the answers.
    pub fn answers(&self) -> &[String] {
        &self.answers
    }

    /// Returns query-related images.
    pub fn images(&self) -> &[SearchImage] {
        &self.images
    }

    /// Records an engine error.
    pub fn add_error(&mut self, engine: impl Into<String>, error: impl Into<String>) {
        self.errors.push((engine.into(), error.into()));
    }

    /// Returns engine errors (engine name, error message).
    pub fn errors(&self) -> &[(String, String)] {
        &self.errors
    }

    /// Records a structured engine execution report.
    pub fn add_report(&mut self, report: SearchReport) {
        self.reports.push(report);
    }

    /// Returns structured engine execution reports.
    pub fn reports(&self) -> &[SearchReport] {
        &self.reports
    }

    /// Sets the search duration.
    pub fn set_duration(&mut self, duration_ms: u64) {
        self.duration_ms = duration_ms;
    }
}

pub(crate) fn merge_image(images: &mut Vec<SearchImage>, image: SearchImage) {
    if let Some(existing) = images.iter_mut().find(|existing| existing.url == image.url) {
        merge_image_description(&mut existing.description, image.description);
        return;
    }
    images.push(image);
    images.sort_by(|left, right| left.url.cmp(&right.url));
}

fn merge_image_description(existing: &mut Option<String>, new: Option<String>) {
    match (existing.as_ref(), new) {
        (None, Some(new)) => *existing = Some(new),
        (Some(current), Some(new))
            if new.len() > current.len() || (new.len() == current.len() && new < *current) =>
        {
            *existing = Some(new);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_type_default() {
        let default: ResultType = Default::default();
        assert_eq!(default, ResultType::Web);
    }

    #[test]
    fn test_result_type_variants() {
        let types = vec![
            ResultType::Web,
            ResultType::Image,
            ResultType::Video,
            ResultType::News,
            ResultType::Map,
            ResultType::File,
            ResultType::Answer,
            ResultType::Infobox,
            ResultType::Suggestion,
        ];
        assert_eq!(types.len(), 9);
    }

    #[test]
    fn test_search_result_new() {
        let result = SearchResult::new("https://example.com", "Title", "Content");
        assert_eq!(result.url, "https://example.com");
        assert_eq!(result.title, "Title");
        assert_eq!(result.content, "Content");
        assert_eq!(result.result_type, ResultType::Web);
        assert!(result.engines.is_empty());
        assert!(result.positions.is_empty());
        assert_eq!(result.score, 0.0);
        assert!(result.relevance_score.is_none());
        assert!(result.thumbnail.is_none());
        assert!(result.published_date.is_none());
        assert!(result.favicon.is_none());
        assert!(result.images.is_empty());
    }

    #[test]
    fn test_search_result_with_type() {
        let result = SearchResult::new("url", "title", "content").with_type(ResultType::Image);
        assert_eq!(result.result_type, ResultType::Image);
    }

    #[test]
    fn test_search_result_with_engine() {
        let result = SearchResult::new("url", "title", "content")
            .with_engine("google", 1)
            .with_engine("bing", 3);
        assert!(result.engines.contains("google"));
        assert!(result.engines.contains("bing"));
        assert_eq!(result.positions, vec![1, 3]);
    }

    #[test]
    fn test_search_result_with_thumbnail() {
        let result = SearchResult::new("url", "title", "content")
            .with_thumbnail("https://example.com/thumb.jpg");
        assert_eq!(
            result.thumbnail,
            Some("https://example.com/thumb.jpg".to_string())
        );
    }

    #[test]
    fn test_search_result_with_published_date() {
        let result = SearchResult::new("url", "title", "content").with_published_date("2024-01-15");
        assert_eq!(result.published_date, Some("2024-01-15".to_string()));
    }

    #[test]
    fn test_search_result_with_relevance_score() {
        let result = SearchResult::new("url", "title", "content").with_relevance_score(0.82);
        assert_eq!(result.relevance_score, Some(0.82));
    }

    #[test]
    fn test_normalized_url_https() {
        let result = SearchResult::new("https://Example.COM/Path/", "t", "c");
        assert_eq!(result.normalized_url(), "example.com/Path");
    }

    #[test]
    fn test_normalized_url_http() {
        let result = SearchResult::new("http://Example.COM/Path/", "t", "c");
        assert_eq!(result.normalized_url(), "example.com/Path");
    }

    #[test]
    fn test_normalized_url_no_scheme() {
        let result = SearchResult::new("example.com/path", "t", "c");
        assert_eq!(result.normalized_url(), "example.com/path");
    }

    #[test]
    fn test_normalized_url_trailing_slash() {
        let result = SearchResult::new("https://example.com/", "t", "c");
        assert_eq!(result.normalized_url(), "example.com");
    }

    #[test]
    fn test_normalized_url_removes_tracking_and_fragment() {
        let result = SearchResult::new(
            "https://www.Example.com/Path/?utm_source=newsletter&b=2&a=1#section",
            "t",
            "c",
        );
        assert_eq!(result.normalized_url(), "example.com/Path?a=1&b=2");
    }

    #[test]
    fn test_normalized_url_sorts_query_pairs() {
        let first = SearchResult::new("https://example.com/path?b=2&a=1", "t", "c");
        let second = SearchResult::new("https://example.com/path?a=1&b=2", "t", "c");

        assert_eq!(first.normalized_url(), second.normalized_url());
    }

    #[test]
    fn test_normalized_url_keeps_non_default_port() {
        let result = SearchResult::new("https://example.com:8443/path/", "t", "c");
        assert_eq!(result.normalized_url(), "example.com:8443/path");
    }

    #[test]
    fn test_normalized_url_preserves_case_sensitive_path_and_query_values() {
        let upper = SearchResult::new("https://example.com/Docs?q=Rust", "t", "c");
        let lower = SearchResult::new("https://example.com/docs?q=rust", "t", "c");

        assert_ne!(upper.normalized_url(), lower.normalized_url());
    }

    #[test]
    fn test_normalized_url_removes_default_port() {
        let explicit = SearchResult::new("https://example.com:443/path", "t", "c");
        let implicit = SearchResult::new("https://example.com/path", "t", "c");

        assert_eq!(explicit.normalized_url(), implicit.normalized_url());
    }

    #[test]
    fn test_search_results_new() {
        let results = SearchResults::new();
        assert_eq!(results.count, 0);
        assert_eq!(results.duration_ms, 0);
        assert!(results.items().is_empty());
        assert!(results.suggestions().is_empty());
        assert!(results.answers().is_empty());
        assert!(results.images().is_empty());
        assert!(results.reports().is_empty());
    }

    #[test]
    fn test_search_results_add_result() {
        let mut results = SearchResults::new();
        results.add_result(SearchResult::new("url1", "title1", "content1"));
        results.add_result(SearchResult::new("url2", "title2", "content2"));
        assert_eq!(results.count, 2);
        assert_eq!(results.items().len(), 2);
    }

    #[test]
    fn test_search_results_add_suggestion() {
        let mut results = SearchResults::new();
        results.add_suggestion("suggestion1");
        results.add_suggestion("suggestion2");
        results.add_suggestion("suggestion1");
        assert_eq!(results.suggestions().len(), 2);
        assert_eq!(results.suggestions()[0], "suggestion1");
    }

    #[test]
    fn test_search_results_add_answer() {
        let mut results = SearchResults::new();
        results.add_answer("42");
        results.add_answer("42");
        assert_eq!(results.answers().len(), 1);
        assert_eq!(results.answers()[0], "42");
    }

    #[test]
    fn test_search_results_merge_duplicate_images_deterministically() {
        let mut results = SearchResults::new();
        results
            .add_image(SearchImage::new("https://example.com/image.png").with_description("short"));
        results.add_image(
            SearchImage::new("https://example.com/image.png")
                .with_description("a richer image description"),
        );
        results.add_image(SearchImage::new("https://a.example/image.png"));

        assert_eq!(results.images().len(), 2);
        assert_eq!(results.images()[0].url, "https://a.example/image.png");
        assert_eq!(
            results.images()[1].description.as_deref(),
            Some("a richer image description")
        );
    }

    #[test]
    fn test_search_results_items_mut() {
        let mut results = SearchResults::new();
        results.add_result(SearchResult::new("url", "title", "content"));
        results.items_mut()[0].score = 5.0;
        assert_eq!(results.items()[0].score, 5.0);
    }

    #[test]
    fn test_search_results_set_duration() {
        let mut results = SearchResults::new();
        results.set_duration(150);
        assert_eq!(results.duration_ms, 150);
    }

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult::new("https://example.com", "Title", "Content")
            .with_engine("zeta", 1)
            .with_engine("alpha", 2);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"url\":\"https://example.com\""));
        assert!(json.contains("\"title\":\"Title\""));
        assert!(json.contains("\"engines\":[\"alpha\",\"zeta\"]"));
    }

    #[test]
    fn test_search_results_serialization() {
        let mut results = SearchResults::new();
        results.add_result(SearchResult::new("url", "title", "content"));
        results.set_duration(100);
        let json = serde_json::to_string(&results).unwrap();
        assert!(json.contains("\"duration_ms\":100"));
    }

    #[test]
    fn test_result_type_serialization() {
        let result = SearchResult::new("url", "title", "content").with_type(ResultType::Image);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"result_type\":\"image\""));
    }

    #[test]
    fn test_search_results_errors_empty() {
        let results = SearchResults::new();
        assert!(results.errors().is_empty());
    }

    #[test]
    fn test_search_results_add_error() {
        let mut results = SearchResults::new();
        results.add_error("Google", "CAPTCHA detected");
        assert_eq!(results.errors().len(), 1);
        assert_eq!(results.errors()[0].0, "Google");
        assert_eq!(results.errors()[0].1, "CAPTCHA detected");
    }

    #[test]
    fn test_search_results_add_structured_report() {
        let report = SearchReport::new("Tavily")
            .with_provider("tavily")
            .with_request_id("req-123")
            .with_total_results(42)
            .with_response_time_ms(125)
            .with_usage(SearchUsage::new().with_credits(2.0))
            .with_metadata("search_depth", "advanced");
        let mut results = SearchResults::new();
        results.add_report(report.clone());

        assert_eq!(results.reports(), &[report]);
        let json = serde_json::to_value(&results).unwrap();
        assert_eq!(json["reports"][0]["provider"], "tavily");
        assert_eq!(json["reports"][0]["usage"]["credits"], 2.0);
        assert_eq!(json["reports"][0]["metadata"]["search_depth"], "advanced");
    }

    #[test]
    fn test_search_results_multiple_errors() {
        let mut results = SearchResults::new();
        results.add_error("Google", "CAPTCHA detected");
        results.add_error("Baidu", "timed out");
        assert_eq!(results.errors().len(), 2);
        assert_eq!(results.errors()[1].0, "Baidu");
    }

    #[test]
    fn test_search_results_errors_with_results() {
        let mut results = SearchResults::new();
        results.add_result(SearchResult::new("url", "title", "content"));
        results.add_error("Google", "failed");
        assert_eq!(results.count, 1);
        assert_eq!(results.errors().len(), 1);
    }

    #[test]
    fn test_search_result_deserialize_without_full_text() {
        // Older persisted JSON lacking `full_text` must still load thanks to #[serde(default)].
        let json = r#"{
            "url": "https://example.com",
            "title": "T",
            "content": "snippet",
            "result_type": "web",
            "engines": [],
            "positions": [],
            "score": 1.0,
            "thumbnail": null,
            "published_date": null
        }"#;
        let r: SearchResult = serde_json::from_str(json).unwrap();
        assert!(r.full_text.is_none());
        assert_eq!(r.url, "https://example.com");
    }

    #[test]
    fn test_search_result_full_text_roundtrip() {
        let mut r = SearchResult::new("https://example.com", "T", "snippet");
        r.full_text = Some("正文 body".to_string());
        let json = serde_json::to_string(&r).unwrap();
        let back: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.full_text.as_deref(), Some("正文 body"));
    }
}
