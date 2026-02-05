//! Search query representation.

use serde::{Deserialize, Serialize};

use crate::EngineCategory;

/// Safe search level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SafeSearch {
    /// No filtering.
    #[default]
    Off = 0,
    /// Moderate filtering.
    Moderate = 1,
    /// Strict filtering.
    Strict = 2,
}

/// Time range filter for search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeRange {
    Day,
    Week,
    Month,
    Year,
}

/// A search query with all parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// The search terms.
    pub query: String,
    /// Target categories.
    pub categories: Vec<EngineCategory>,
    /// Language/locale (e.g., "en-US").
    pub language: Option<String>,
    /// Safe search level.
    pub safesearch: SafeSearch,
    /// Page number (1-indexed).
    pub page: u32,
    /// Time range filter.
    pub time_range: Option<TimeRange>,
    /// Specific engines to use (by shortcut).
    pub engines: Vec<String>,
}

impl SearchQuery {
    /// Creates a new search query with the given terms.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            categories: vec![EngineCategory::General],
            language: None,
            safesearch: SafeSearch::Off,
            page: 1,
            time_range: None,
            engines: Vec::new(),
        }
    }

    /// Sets the categories to search.
    pub fn with_categories(mut self, categories: Vec<EngineCategory>) -> Self {
        self.categories = categories;
        self
    }

    /// Sets the language/locale.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Sets the safe search level.
    pub fn with_safesearch(mut self, level: SafeSearch) -> Self {
        self.safesearch = level;
        self
    }

    /// Sets the page number.
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    /// Sets the time range filter.
    pub fn with_time_range(mut self, range: TimeRange) -> Self {
        self.time_range = Some(range);
        self
    }

    /// Sets specific engines to use.
    pub fn with_engines(mut self, engines: Vec<String>) -> Self {
        self.engines = engines;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_query_new() {
        let query = SearchQuery::new("test query");
        assert_eq!(query.query, "test query");
        assert_eq!(query.categories, vec![EngineCategory::General]);
        assert_eq!(query.safesearch, SafeSearch::Off);
        assert_eq!(query.page, 1);
        assert!(query.language.is_none());
        assert!(query.time_range.is_none());
        assert!(query.engines.is_empty());
    }

    #[test]
    fn test_search_query_with_categories() {
        let query = SearchQuery::new("test")
            .with_categories(vec![EngineCategory::Images, EngineCategory::Videos]);
        assert_eq!(query.categories, vec![EngineCategory::Images, EngineCategory::Videos]);
    }

    #[test]
    fn test_search_query_with_language() {
        let query = SearchQuery::new("test").with_language("en-US");
        assert_eq!(query.language, Some("en-US".to_string()));
    }

    #[test]
    fn test_search_query_with_safesearch() {
        let query = SearchQuery::new("test").with_safesearch(SafeSearch::Strict);
        assert_eq!(query.safesearch, SafeSearch::Strict);
    }

    #[test]
    fn test_search_query_with_page() {
        let query = SearchQuery::new("test").with_page(5);
        assert_eq!(query.page, 5);
    }

    #[test]
    fn test_search_query_with_time_range() {
        let query = SearchQuery::new("test").with_time_range(TimeRange::Week);
        assert_eq!(query.time_range, Some(TimeRange::Week));
    }

    #[test]
    fn test_search_query_with_engines() {
        let query = SearchQuery::new("test")
            .with_engines(vec!["ddg".to_string(), "wiki".to_string()]);
        assert_eq!(query.engines, vec!["ddg", "wiki"]);
    }

    #[test]
    fn test_search_query_builder_chain() {
        let query = SearchQuery::new("rust programming")
            .with_categories(vec![EngineCategory::General])
            .with_language("en")
            .with_safesearch(SafeSearch::Moderate)
            .with_page(2)
            .with_time_range(TimeRange::Month)
            .with_engines(vec!["ddg".to_string()]);

        assert_eq!(query.query, "rust programming");
        assert_eq!(query.language, Some("en".to_string()));
        assert_eq!(query.safesearch, SafeSearch::Moderate);
        assert_eq!(query.page, 2);
        assert_eq!(query.time_range, Some(TimeRange::Month));
        assert_eq!(query.engines, vec!["ddg"]);
    }

    #[test]
    fn test_safe_search_default() {
        let default: SafeSearch = Default::default();
        assert_eq!(default, SafeSearch::Off);
    }

    #[test]
    fn test_safe_search_values() {
        assert_eq!(SafeSearch::Off as u8, 0);
        assert_eq!(SafeSearch::Moderate as u8, 1);
        assert_eq!(SafeSearch::Strict as u8, 2);
    }

    #[test]
    fn test_time_range_variants() {
        let day = TimeRange::Day;
        let week = TimeRange::Week;
        let month = TimeRange::Month;
        let year = TimeRange::Year;

        assert_ne!(day, week);
        assert_ne!(month, year);
    }

    #[test]
    fn test_search_query_serialization() {
        let query = SearchQuery::new("test");
        let json = serde_json::to_string(&query).unwrap();
        assert!(json.contains("\"query\":\"test\""));
    }

    #[test]
    fn test_search_query_deserialization() {
        let json = r#"{"query":"test","categories":["general"],"language":null,"safesearch":"Off","page":1,"time_range":null,"engines":[]}"#;
        let query: SearchQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.query, "test");
    }
}
