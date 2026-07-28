//! Error types for the search library.

use std::fmt;

use thiserror::Error;

/// Result type alias for search operations.
pub type Result<T> = std::result::Result<T, SearchError>;

/// Stable classes for failures returned by native search providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderErrorKind {
    /// The provider rejected the request shape or an unsupported query option.
    InvalidRequest,
    /// A required credential is missing or was rejected.
    Authentication,
    /// The credential is valid but cannot access the requested capability.
    Permission,
    /// The account or anonymous allowance has no quota remaining.
    Quota,
    /// The provider throttled the request.
    RateLimited,
    /// The provider or one of its dependencies is temporarily unavailable.
    Unavailable,
    /// The response did not match the documented provider contract.
    InvalidResponse,
    /// The request could not reach the provider.
    Transport,
}

impl ProviderErrorKind {
    /// Returns a stable lowercase identifier suitable for metrics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Authentication => "authentication",
            Self::Permission => "permission",
            Self::Quota => "quota",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::Transport => "transport",
        }
    }
}

impl fmt::Display for ProviderErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A sanitized provider failure with optional support and retry context.
///
/// Provider response bodies are deliberately not retained. Integrations should
/// extract only a bounded human-readable message and request identifier so
/// quota responses cannot leak newly issued credentials or other account data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    provider: String,
    kind: ProviderErrorKind,
    status: Option<u16>,
    application_code: Option<i64>,
    message: String,
    request_id: Option<String>,
    retry_after_seconds: Option<u64>,
}

impl ProviderError {
    /// Creates a provider error from already-sanitized context.
    pub fn new(
        provider: impl Into<String>,
        kind: ProviderErrorKind,
        message: impl Into<String>,
    ) -> Self {
        let provider = sanitize_error_text(&provider.into(), 64);
        let message = sanitize_error_text(&message.into(), 512);
        Self {
            provider: if provider.is_empty() {
                "unknown".to_string()
            } else {
                provider
            },
            kind,
            status: None,
            application_code: None,
            message: if message.is_empty() {
                "provider request failed".to_string()
            } else {
                message
            },
            request_id: None,
            retry_after_seconds: None,
        }
    }

    /// Attaches the HTTP status returned by the provider.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Attaches a provider application or JSON-RPC error code.
    pub fn with_application_code(mut self, code: i64) -> Self {
        self.application_code = Some(code);
        self
    }

    /// Attaches a provider request identifier for support correlation.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        let request_id = sanitize_error_text(&request_id.into(), 128);
        self.request_id = (!request_id.is_empty()).then_some(request_id);
        self
    }

    /// Attaches the provider's bounded retry delay.
    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after_seconds = Some(seconds.min(86_400));
        self
    }

    /// Provider identifier.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Stable provider error class.
    pub const fn kind(&self) -> ProviderErrorKind {
        self.kind
    }

    /// HTTP status, when the request reached the provider.
    pub const fn status(&self) -> Option<u16> {
        self.status
    }

    /// Provider application or JSON-RPC error code, when supplied.
    pub const fn application_code(&self) -> Option<i64> {
        self.application_code
    }

    /// Sanitized message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Provider request identifier, when supplied.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Retry delay advertised by the provider, when supplied.
    pub const fn retry_after_seconds(&self) -> Option<u64> {
        self.retry_after_seconds
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} provider {}: {}",
            self.provider, self.kind, self.message
        )?;
        if let Some(status) = self.status {
            write!(formatter, " (HTTP {status})")?;
        }
        if let Some(code) = self.application_code {
            write!(formatter, " (code: {code})")?;
        }
        if let Some(request_id) = &self.request_id {
            write!(formatter, " (request_id: {request_id})")?;
        }
        if let Some(seconds) = self.retry_after_seconds {
            write!(formatter, " (retry after {seconds}s)")?;
        }
        Ok(())
    }
}

impl std::error::Error for ProviderError {}

fn sanitize_error_text(value: &str, max_chars: usize) -> String {
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

/// Errors that can occur during search operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SearchError {
    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// An HTTP endpoint returned a non-success status without exposing its
    /// response body.
    #[error("HTTP request failed with status {status}")]
    HttpStatus {
        /// Numeric HTTP status.
        status: u16,
        /// Bounded Retry-After delay, when supplied.
        retry_after_seconds: Option<u64>,
    },

    /// Failed to parse response.
    #[error("Failed to parse response: {0}")]
    Parse(String),

    /// A search endpoint returned an interactive challenge or interstitial
    /// instead of a result page.
    #[error("Search response blocked: {0}")]
    Challenge(String),

    /// A search endpoint returned a successful HTTP response whose structure
    /// does not match either a result page or a legitimate empty state.
    #[error("Invalid search response: {0}")]
    InvalidResponse(String),

    /// Engine is temporarily suspended.
    #[error("Engine '{0}' is suspended until {1}")]
    EngineSuspended(String, String),

    /// Search timeout exceeded.
    #[error("Search timeout exceeded")]
    Timeout,

    /// A transient operation failed but the shared retry budget denied
    /// another attempt.
    #[error("Retry budget exhausted after transient failure: {0}")]
    RetryBudgetExhausted(String),

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

    /// A typed third-party provider failure.
    #[error(transparent)]
    Provider(#[from] ProviderError),

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
            Self::HttpStatus { status: 429, .. } => "rate_limited",
            Self::HttpStatus {
                status: 401 | 403, ..
            } => "permission_denied",
            Self::HttpStatus { status: 404, .. } => "not_found",
            Self::HttpStatus { status, .. }
                if matches!(*status, 408 | 425 | 500 | 502 | 503 | 504) =>
            {
                "http_unavailable"
            }
            Self::HttpStatus { .. } => "http_status",
            Self::Parse(_) => "parse",
            Self::Challenge(_) => "challenge",
            Self::InvalidResponse(_) => "invalid_response",
            Self::EngineSuspended(_, _) => "engine_suspended",
            Self::Timeout => "timeout",
            Self::RetryBudgetExhausted(_) => "retry_budget_exhausted",
            Self::NoEngines => "no_engines",
            Self::InvalidQuery(_) => "invalid_query",
            Self::UrlParse(_) => "url_parse",
            Self::Browser(_) => "browser",
            Self::Proxy(_) => "proxy",
            Self::Network(_) => "network",
            Self::PermissionDenied(_) => "permission_denied",
            Self::NotFound(_) => "not_found",
            Self::RateLimited(_) => "rate_limited",
            Self::Provider(error) => match error.kind() {
                ProviderErrorKind::InvalidRequest => "provider_invalid_request",
                ProviderErrorKind::Authentication => "provider_authentication",
                ProviderErrorKind::Permission => "provider_permission",
                ProviderErrorKind::Quota => "provider_quota",
                ProviderErrorKind::RateLimited => "provider_rate_limited",
                ProviderErrorKind::Unavailable => "provider_unavailable",
                ProviderErrorKind::InvalidResponse => "provider_invalid_response",
                ProviderErrorKind::Transport => "provider_transport",
            },
            Self::Other(_) => "other",
        }
    }

    /// Returns true if this error is likely transient and retrying might help.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(e) => e.is_timeout() || e.is_connect() || e.is_decode(),
            Self::HttpStatus { status, .. } => {
                *status == 429 || matches!(*status, 408 | 425 | 500 | 502 | 503 | 504)
            }
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
            Self::RetryBudgetExhausted(_) => true,
            Self::Challenge(_) => true,
            Self::Provider(error) => matches!(
                error.kind(),
                ProviderErrorKind::RateLimited
                    | ProviderErrorKind::Unavailable
                    | ProviderErrorKind::Transport
            ),
            _ => false,
        }
    }

    /// Returns true if this error indicates a client-side issue that won't be fixed by retrying.
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::NotFound(_)
                | Self::PermissionDenied(_)
                | Self::InvalidQuery(_)
                | Self::HttpStatus {
                    status: 400 | 401 | 403 | 404,
                    ..
                }
        ) || matches!(
            self,
            Self::Provider(error)
                if matches!(
                    error.kind(),
                    ProviderErrorKind::InvalidRequest
                        | ProviderErrorKind::Authentication
                        | ProviderErrorKind::Permission
                        | ProviderErrorKind::Quota
                )
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
            Self::HttpStatus { status: 429, .. } => 80,
            Self::HttpStatus { status, .. }
                if matches!(*status, 408 | 425 | 500 | 502 | 503 | 504) =>
            {
                70
            }
            Self::HttpStatus { .. } => 10,
            Self::Browser(msg) if msg.contains("timeout") => 80,
            Self::Browser(msg) if msg.contains("connection reset") => 70,
            Self::Provider(error) => match error.kind() {
                ProviderErrorKind::RateLimited => 80,
                ProviderErrorKind::Unavailable => 75,
                ProviderErrorKind::Transport => 70,
                ProviderErrorKind::InvalidResponse => 25,
                ProviderErrorKind::Quota
                | ProviderErrorKind::Authentication
                | ProviderErrorKind::Permission
                | ProviderErrorKind::InvalidRequest => 0,
            },

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
            Self::Challenge(_) => 25,
            Self::InvalidResponse(_) => 0,
            Self::UrlParse(_) => 20,
            Self::Proxy(_) => 30,
            Self::RetryBudgetExhausted(_) => 0,
            Self::Other(_) => 25,
        }
    }

    /// Returns bounded provider or HTTP retry context, when available.
    pub const fn retry_after_seconds(&self) -> Option<u64> {
        match self {
            Self::Provider(error) => error.retry_after_seconds(),
            Self::HttpStatus {
                retry_after_seconds,
                ..
            } => *retry_after_seconds,
            _ => None,
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

    #[test]
    fn test_provider_error_exposes_only_sanitized_context() {
        let error = ProviderError::new(
            "tavily",
            ProviderErrorKind::RateLimited,
            "request throttled",
        )
        .with_status(429)
        .with_request_id("req-123")
        .with_retry_after(30);

        assert_eq!(error.provider(), "tavily");
        assert_eq!(error.kind(), ProviderErrorKind::RateLimited);
        assert_eq!(error.status(), Some(429));
        assert_eq!(error.message(), "request throttled");
        assert_eq!(error.request_id(), Some("req-123"));
        assert_eq!(error.retry_after_seconds(), Some(30));
        assert_eq!(
            error.to_string(),
            "tavily provider rate_limited: request throttled (HTTP 429) \
             (request_id: req-123) (retry after 30s)"
        );
    }

    #[test]
    fn test_provider_error_classification() {
        let rate_limited = SearchError::from(ProviderError::new(
            "anysearch",
            ProviderErrorKind::RateLimited,
            "slow down",
        ));
        assert_eq!(rate_limited.kind(), "provider_rate_limited");
        assert!(rate_limited.is_transient());
        assert!(!rate_limited.is_client_error());
        assert_eq!(rate_limited.retry_score(), 80);

        let quota = SearchError::from(ProviderError::new(
            "tavily",
            ProviderErrorKind::Quota,
            "plan limit reached",
        ));
        assert_eq!(quota.kind(), "provider_quota");
        assert!(!quota.is_transient());
        assert!(quota.is_client_error());
        assert_eq!(quota.retry_score(), 0);
    }

    #[test]
    fn provider_error_constructor_enforces_bounded_terminal_safe_context() {
        let error = ProviderError::new(
            "  custom\nprovider  ",
            ProviderErrorKind::InvalidResponse,
            format!("{}\u{0}\n", "message ".repeat(200)),
        )
        .with_request_id(format!("{}\n", "request ".repeat(100)))
        .with_retry_after(u64::MAX);

        assert_eq!(error.provider(), "custom provider");
        assert!(error.message().chars().count() <= 512);
        assert!(error
            .message()
            .chars()
            .all(|character| !character.is_control()));
        assert!(error.request_id().unwrap().chars().count() <= 128);
        assert!(error
            .request_id()
            .unwrap()
            .chars()
            .all(|character| !character.is_control()));
        assert_eq!(error.retry_after_seconds(), Some(86_400));
    }
}
