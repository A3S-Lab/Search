//! Search result types.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Type of search result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultType {
    /// Standard web result.
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

impl Default for ResultType {
    fn default() -> Self {
        Self::Web
    }
}

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub engines: HashSet<String>,
    /// Positions in each engine's results.
    pub positions: Vec<u32>,
    /// Calculated score for ranking.
    pub score: f64,
    /// Thumbnail URL (for images/videos).
    pub thumbnail: Option<String>,
    /// Published date (for news).
    pub published_date: Option<String>,
}

impl SearchResult {
    /// Creates a new search result.
    pub fn new(url: impl Into<String>, title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
            content: content.into(),
            result_type: ResultType::Web,
            engines: HashSet::new(),
            positions: Vec::new(),
            score: 0.0,
            thumbnail: None,
            published_date: None,
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

    /// Returns a normalized URL for deduplication (without scheme and trailing slash).
    pub fn normalized_url(&self) -> String {
        let url = self.url.trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        url.to_lowercase()
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

    /// Adds a suggestion.
    pub fn add_suggestion(&mut self, suggestion: impl Into<String>) {
        self.suggestions.push(suggestion.into());
    }

    /// Adds an answer.
    pub fn add_answer(&mut self, answer: impl Into<String>) {
        self.answers.push(answer.into());
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

    /// Sets the search duration.
    pub fn set_duration(&mut self, duration_ms: u64) {
        self.duration_ms = duration_ms;
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
        assert!(result.thumbnail.is_none());
        assert!(result.published_date.is_none());
    }

    #[test]
    fn test_search_result_with_type() {
        let result = SearchResult::new("url", "title", "content")
            .with_type(ResultType::Image);
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
        assert_eq!(result.thumbnail, Some("https://example.com/thumb.jpg".to_string()));
    }

    #[test]
    fn test_search_result_with_published_date() {
        let result = SearchResult::new("url", "title", "content")
            .with_published_date("2024-01-15");
        assert_eq!(result.published_date, Some("2024-01-15".to_string()));
    }

    #[test]
    fn test_normalized_url_https() {
        let result = SearchResult::new("https://Example.COM/Path/", "t", "c");
        assert_eq!(result.normalized_url(), "example.com/path");
    }

    #[test]
    fn test_normalized_url_http() {
        let result = SearchResult::new("http://Example.COM/Path/", "t", "c");
        assert_eq!(result.normalized_url(), "example.com/path");
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
    fn test_search_results_new() {
        let results = SearchResults::new();
        assert_eq!(results.count, 0);
        assert_eq!(results.duration_ms, 0);
        assert!(results.items().is_empty());
        assert!(results.suggestions().is_empty());
        assert!(results.answers().is_empty());
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
        assert_eq!(results.suggestions().len(), 2);
        assert_eq!(results.suggestions()[0], "suggestion1");
    }

    #[test]
    fn test_search_results_add_answer() {
        let mut results = SearchResults::new();
        results.add_answer("42");
        assert_eq!(results.answers().len(), 1);
        assert_eq!(results.answers()[0], "42");
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
        let result = SearchResult::new("https://example.com", "Title", "Content");
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"url\":\"https://example.com\""));
        assert!(json.contains("\"title\":\"Title\""));
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
        let result = SearchResult::new("url", "title", "content")
            .with_type(ResultType::Image);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"result_type\":\"image\""));
    }
}
