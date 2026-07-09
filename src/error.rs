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

    /// Browser operation failed.
    #[error("Browser error: {0}")]
    Browser(String),

    /// Proxy operation failed.
    #[error("Proxy error: {0}")]
    Proxy(String),

    /// Network connectivity issue.
    #[error("Network error: {0}")]
    Network(String),

    /// Authentication or permission denied.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Resource not found (e.g., CAPTCHAs, blocked pages).
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Rate limited or throttled.
    #[error("Rate limited: {0}")]
    RateLimited(String),

    /// Generic error.
    #[error("{0}")]
    Other(String),
}

impl SearchError {
    /// Returns a stable, low-cardinality name for metrics and logging.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Http(e) if e.is_timeout() => "http_timeout",
            Self::Http(e) if e.is_connect() => "http_connect",
            Self::Http(e) if e.is_decode() => "http_decode",
            Self::Http(_) => "http",
            Self::Parse(_) => "parse",
            Self::EngineSuspended(_, _) => "engine_suspended",
            Self::Timeout => "timeout",
            Self::NoEngines => "no_engines",
            Self::InvalidQuery(_) => "invalid_query",
            Self::UrlParse(_) => "url_parse",
            Self::Browser(_) => "browser",
            Self::Proxy(_) => "proxy",
            Self::Network(_) => "network",
            Self::PermissionDenied(_) => "permission_denied",
            Self::NotFound(_) => "not_found",
            Self::RateLimited(_) => "rate_limited",
            Self::Other(_) => "other",
        }
    }

    /// Returns true if this error is likely transient and retrying might help.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(e) => e.is_timeout() || e.is_connect() || e.is_decode(),
            Self::Browser(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("timeout")
                    || msg.contains("connection")
                    || msg.contains("channel")
                    || msg.contains("tab closed")
                    || msg.contains("disconnected")
                    || msg.contains("net::err_")
            }
            Self::Network(_) => true,
            Self::RateLimited(_) => true,
            Self::Timeout => true,
            _ => false,
        }
    }

    /// Returns true if this error indicates a client-side issue that won't be fixed by retrying.
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::NotFound(_) | Self::PermissionDenied(_) | Self::InvalidQuery(_)
        )
    }

    /// Returns a score from 0-100 indicating how likely retrying will help.
    pub fn retry_score(&self) -> u8 {
        match self {
            // High retry value
            Self::Timeout => 90,
            Self::Network(_) => 85,
            Self::RateLimited(_) => 80,
            Self::Http(e) if e.is_timeout() => 85,
            Self::Http(e) if e.is_connect() => 75,
            Self::Browser(msg) if msg.contains("timeout") => 80,
            Self::Browser(msg) if msg.contains("connection reset") => 70,

            // Medium retry value
            Self::Http(_) => 50,
            Self::Browser(_) => 40,

            // Low/no retry value
            Self::NotFound(_) => 10,
            Self::PermissionDenied(_) => 5,
            Self::InvalidQuery(_) => 0,
            Self::EngineSuspended(_, _) => 0,
            Self::NoEngines => 0,
            Self::Parse(_) => 20,
            Self::UrlParse(_) => 20,
            Self::Proxy(_) => 30,
            Self::Other(_) => 25,
        }
    }
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
        assert_eq!(
            err.to_string(),
            "Engine 'Google' is suspended until 2024-01-01"
        );
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
    fn test_error_display_browser() {
        let err = SearchError::Browser("chrome crashed".to_string());
        assert_eq!(err.to_string(), "Browser error: chrome crashed");
    }

    #[test]
    fn test_error_display_other() {
        let err = SearchError::Other("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn test_error_display_network() {
        let err = SearchError::Network("connection refused".to_string());
        assert_eq!(err.to_string(), "Network error: connection refused");
    }

    #[test]
    fn test_error_display_rate_limited() {
        let err = SearchError::RateLimited("too many requests".to_string());
        assert_eq!(err.to_string(), "Rate limited: too many requests");
    }

    #[test]
    fn test_error_display_not_found() {
        let err = SearchError::NotFound("CAPTCHA page".to_string());
        assert_eq!(err.to_string(), "Resource not found: CAPTCHA page");
    }

    #[test]
    fn test_error_display_permission_denied() {
        let err = SearchError::PermissionDenied("access denied".to_string());
        assert_eq!(err.to_string(), "Permission denied: access denied");
    }

    #[test]
    fn test_error_debug() {
        let err = SearchError::Timeout;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Timeout"));
    }

    #[test]
    fn test_error_url_parse() {
        let url_err = url::Url::parse("not a url").unwrap_err();
        let err: SearchError = url_err.into();
        let msg = err.to_string();
        assert!(msg.contains("URL parsing error"));
    }

    #[test]
    fn test_is_transient_timeout() {
        assert!(SearchError::Timeout.is_transient());
    }

    #[test]
    fn test_is_transient_rate_limited() {
        assert!(SearchError::RateLimited("too many".to_string()).is_transient());
    }

    #[test]
    fn test_is_transient_not_found() {
        assert!(!SearchError::NotFound("not found".to_string()).is_transient());
    }

    #[test]
    fn test_is_client_error_not_found() {
        assert!(SearchError::NotFound("not found".to_string()).is_client_error());
    }

    #[test]
    fn test_is_client_error_invalid_query() {
        assert!(SearchError::InvalidQuery("bad".to_string()).is_client_error());
    }

    #[test]
    fn test_retry_score_timeout() {
        assert_eq!(SearchError::Timeout.retry_score(), 90);
    }

    #[test]
    fn test_retry_score_invalid_query() {
        assert_eq!(
            SearchError::InvalidQuery("bad".to_string()).retry_score(),
            0
        );
    }

    #[test]
    fn test_retry_score_rate_limited() {
        assert_eq!(
            SearchError::RateLimited("too many".to_string()).retry_score(),
            80
        );
    }
}
