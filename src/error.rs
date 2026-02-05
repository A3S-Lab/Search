//! Error types for the search library.

use thiserror::Error;

/// Result type alias for search operations.
pub type Result<T> = std::result::Result<T, SearchError>;

/// Errors that can occur during search operations.
#[derive(Error, Debug)]
pub enum SearchError {
    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Failed to parse response.
    #[error("Failed to parse response: {0}")]
    Parse(String),

    /// Engine is temporarily suspended.
    #[error("Engine '{0}' is suspended until {1}")]
    EngineSuspended(String, String),

    /// Search timeout exceeded.
    #[error("Search timeout exceeded")]
    Timeout,

    /// No engines configured.
    #[error("No search engines configured")]
    NoEngines,

    /// Invalid query.
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// URL parsing error.
    #[error("URL parsing error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_parse() {
        let err = SearchError::Parse("invalid JSON".to_string());
        assert_eq!(err.to_string(), "Failed to parse response: invalid JSON");
    }

    #[test]
    fn test_error_display_engine_suspended() {
        let err = SearchError::EngineSuspended("Google".to_string(), "2024-01-01".to_string());
        assert_eq!(err.to_string(), "Engine 'Google' is suspended until 2024-01-01");
    }

    #[test]
    fn test_error_display_timeout() {
        let err = SearchError::Timeout;
        assert_eq!(err.to_string(), "Search timeout exceeded");
    }

    #[test]
    fn test_error_display_no_engines() {
        let err = SearchError::NoEngines;
        assert_eq!(err.to_string(), "No search engines configured");
    }

    #[test]
    fn test_error_display_invalid_query() {
        let err = SearchError::InvalidQuery("empty query".to_string());
        assert_eq!(err.to_string(), "Invalid query: empty query");
    }

    #[test]
    fn test_error_display_other() {
        let err = SearchError::Other("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn test_error_debug() {
        let err = SearchError::Timeout;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Timeout"));
    }
}
