//! Search result types.

use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, HashSet};
use url::form_urlencoded::Serializer as FormSerializer;

use crate::SearchError;

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

#[derive(Debug, Clone, Copy)]
pub(crate) struct RankSignal {
    pub(crate) position: u32,
    pub(crate) relevance: Option<f64>,
    pub(crate) contribution: f64,
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
    #[serde(skip)]
    pub(crate) rank_signals: BTreeMap<String, RankSignal>,
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
            rank_signals: BTreeMap::new(),
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

/// A structured failure from one search engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EngineFailure {
    /// Human-readable engine name.
    pub engine: String,
    /// Native provider identifier, when the engine wraps a provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Stable, low-cardinality error kind.
    pub kind: String,
    /// Bounded diagnostic safe for callers to display.
    pub message: String,
    /// Whether retrying the same engine may succeed without configuration changes.
    #[serde(default)]
    pub transient: bool,
    /// Provider- or circuit-advertised retry delay, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

impl EngineFailure {
    /// Creates a structured engine failure.
    pub fn new(
        engine: impl Into<String>,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            engine: engine.into(),
            provider: None,
            kind: kind.into(),
            message: message.into(),
            transient: false,
            retry_after_seconds: None,
        }
    }

    /// Attaches the native provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Marks whether retrying the same engine may succeed.
    pub fn with_transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Attaches a bounded retry delay.
    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after_seconds = Some(seconds.min(86_400));
        self
    }

    /// Converts an internal search error into the canonical structured engine
    /// failure representation.
    ///
    /// Search orchestration and the CLI both cross this boundary.  Keeping the
    /// provider and retry context enrichment here prevents those callers from
    /// drifting apart as new [`SearchError`] variants are added.
    pub fn from_search_error(engine: impl Into<String>, error: &SearchError) -> Self {
        let mut failure =
            Self::new(engine, error.kind(), error.to_string()).with_transient(error.is_transient());
        if let SearchError::Provider(provider) = error {
            failure = failure.with_provider(provider.provider());
        }
        if let Some(seconds) = error.retry_after_seconds() {
            failure = failure.with_retry_after(seconds);
        }
        failure
    }
}

/// Typed terminal state for one engine execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineOutcomeKind {
    /// The engine returned usable structured output.
    Success,
    /// The engine completed normally but returned no web or rich output.
    Empty,
    /// The engine executed and failed.
    Failure,
    /// The engine exceeded its orchestration timeout.
    Timeout,
    /// The engine was rejected by a bounded local concurrency policy.
    Rejected,
    /// The engine was skipped because a circuit or local health gate was open.
    CircuitOpen,
}

/// Observable result of one selected engine for one search request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EngineOutcome {
    /// Configured display name.
    pub engine: String,
    /// Stable engine shortcut used by shared circuit state.
    pub shortcut: String,
    /// Native provider identifier, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Terminal execution state.
    pub kind: EngineOutcomeKind,
    /// Number of ordinary web/media results returned by this engine.
    pub result_count: usize,
    /// End-to-end orchestration time spent on this engine attempt.
    pub duration_ms: u64,
    /// Structured failure or circuit-open reason, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<EngineFailure>,
}

impl EngineOutcome {
    pub(crate) fn completed(
        engine: impl Into<String>,
        shortcut: impl Into<String>,
        kind: EngineOutcomeKind,
        result_count: usize,
    ) -> Self {
        Self {
            engine: engine.into(),
            shortcut: shortcut.into(),
            provider: None,
            kind,
            result_count,
            duration_ms: 0,
            failure: None,
        }
    }

    pub(crate) fn failed(
        shortcut: impl Into<String>,
        failure: EngineFailure,
        kind: EngineOutcomeKind,
    ) -> Self {
        Self {
            engine: failure.engine.clone(),
            shortcut: shortcut.into(),
            provider: failure.provider.clone(),
            kind,
            result_count: 0,
            duration_ms: 0,
            failure: Some(failure),
        }
    }

    pub(crate) fn with_duration(mut self, duration: std::time::Duration) -> Self {
        self.duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
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
    /// Structured engine failures for policy-driven callers.
    #[serde(default)]
    failures: Vec<EngineFailure>,
    /// Structured per-engine execution reports.
    #[serde(default)]
    reports: Vec<SearchReport>,
    /// Typed outcome for every selected or circuit-skipped engine.
    #[serde(default)]
    outcomes: Vec<EngineOutcome>,
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
        self.add_failure(EngineFailure::new(engine, "unknown", error));
    }

    /// Returns engine errors (engine name, error message).
    pub fn errors(&self) -> &[(String, String)] {
        &self.errors
    }

    /// Records a structured engine failure while preserving the legacy error view.
    pub fn add_failure(&mut self, failure: EngineFailure) {
        self.errors
            .push((failure.engine.clone(), failure.message.clone()));
        self.failures.push(failure);
    }

    /// Returns structured engine failures.
    pub fn failures(&self) -> &[EngineFailure] {
        &self.failures
    }

    /// Records a structured engine execution report.
    pub fn add_report(&mut self, report: SearchReport) {
        self.reports.push(report);
    }

    /// Returns structured engine execution reports.
    pub fn reports(&self) -> &[SearchReport] {
        &self.reports
    }

    /// Records one engine outcome.
    pub fn add_outcome(&mut self, outcome: EngineOutcome) {
        self.outcomes.push(outcome);
    }

    /// Returns typed engine outcomes in deterministic selection order.
    pub fn outcomes(&self) -> &[EngineOutcome] {
        &self.outcomes
    }

    /// Merges another search tier through the canonical ranked-result merger.
    pub fn merge(&mut self, mut other: SearchResults) {
        let mut results = std::mem::take(&mut self.results);
        results.append(&mut other.results);
        self.results = crate::aggregator::merge_ranked_results(results);
        self.count = self.results.len();

        for suggestion in other.suggestions {
            self.add_suggestion(suggestion);
        }
        for answer in other.answers {
            self.add_answer(answer);
        }
        for image in other.images {
            self.add_image(image);
        }
        self.errors.append(&mut other.errors);
        self.failures.append(&mut other.failures);
        self.reports.append(&mut other.reports);
        self.outcomes.append(&mut other.outcomes);
        self.duration_ms = self.duration_ms.saturating_add(other.duration_ms);
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
#[path = "result/tests.rs"]
mod tests;
