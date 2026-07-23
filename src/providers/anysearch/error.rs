//! Sanitized AnySearch HTTP and JSON-RPC error mapping.

use serde_json::Value;

use super::super::http::ProviderHttpResponse;
use super::super::protocol::sanitize_provider_text_with_secrets;
use super::response::{AnySearchRpcEnvelope, AnySearchRpcError};
use super::PROVIDER_ID;
use crate::{ProviderError, ProviderErrorKind, SearchError};

pub(super) fn anysearch_http_error(
    response: &ProviderHttpResponse,
    secrets: &[&str],
) -> SearchError {
    let status = response.status.as_u16();
    let envelope: Option<AnySearchRpcEnvelope> = serde_json::from_slice(&response.body).ok();
    if let Some(error) = envelope.and_then(|envelope| envelope.error) {
        let mut error = build_rpc_error(
            error,
            response.header("x-request-id"),
            Some(status),
            secrets,
        );
        if let Some(retry_after) = response.retry_after_seconds() {
            error = error.with_retry_after(retry_after);
        }
        return error.into();
    }

    let kind = classify_failure(Some(status), None, "");
    let fallback = failure_message(kind);
    let mut error = ProviderError::new(PROVIDER_ID, kind, fallback).with_status(status);
    if let Some(request_id) = sanitized_request_id(response.header("x-request-id"), secrets) {
        error = error.with_request_id(request_id);
    }
    if let Some(retry_after) = response.retry_after_seconds() {
        error = error.with_retry_after(retry_after);
    }
    error.into()
}

pub(super) fn anysearch_rpc_error(
    error: AnySearchRpcError,
    request_id: Option<&str>,
    status: Option<u16>,
    retry_after_seconds: Option<u64>,
    secrets: &[&str],
) -> SearchError {
    let mut error = build_rpc_error(error, request_id, status, secrets);
    if let Some(retry_after_seconds) = retry_after_seconds {
        error = error.with_retry_after(retry_after_seconds);
    }
    error.into()
}

pub(super) fn anysearch_declared_tool_error(
    text: Option<&str>,
    request_id: Option<&str>,
) -> SearchError {
    let diagnostic = text.and_then(first_non_empty_line).unwrap_or_default();
    let kind = classify_failure(None, None, diagnostic);
    build_tool_error(kind, request_id)
}

pub(super) fn anysearch_embedded_quota_error(
    text: Option<&str>,
    structured_content: Option<&Value>,
    request_id: Option<&str>,
) -> Option<SearchError> {
    let text_reports_quota = text
        .and_then(first_non_empty_line)
        .is_some_and(is_quota_exhaustion_marker);
    let structured_reports_quota = structured_content.is_some_and(structured_quota_failure);
    (text_reports_quota || structured_reports_quota)
        .then(|| build_tool_error(ProviderErrorKind::Quota, request_id))
}

fn build_tool_error(kind: ProviderErrorKind, request_id: Option<&str>) -> SearchError {
    let mut error = ProviderError::new(PROVIDER_ID, kind, failure_message(kind));
    if let Some(request_id) = request_id {
        error = error.with_request_id(request_id);
    }
    error.into()
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn is_quota_exhaustion_marker(marker: &str) -> bool {
    let marker: String = marker
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    marker.contains("quota")
        && ["exhaust", "exceed", "deplet", "reached"]
            .iter()
            .any(|term| marker.contains(term))
}

fn structured_quota_failure(value: &Value) -> bool {
    let candidate = value.get("data").unwrap_or(value);
    let Some(object) = candidate.as_object() else {
        return false;
    };
    object.contains_key("auto_registered")
        || ["status", "code", "error", "message"]
            .iter()
            .filter_map(|key| object.get(*key).and_then(Value::as_str))
            .any(is_quota_exhaustion_marker)
}

fn build_rpc_error(
    error: AnySearchRpcError,
    request_id: Option<&str>,
    status: Option<u16>,
    secrets: &[&str],
) -> ProviderError {
    let message = error
        .message
        .as_deref()
        .map(|message| sanitize_provider_text_with_secrets(message, 300, secrets))
        .filter(|message| !message.is_empty());
    let kind = classify_failure(status, Some(error.code), message.as_deref().unwrap_or(""));
    let mut provider_error = ProviderError::new(
        PROVIDER_ID,
        kind,
        message.unwrap_or_else(|| failure_message(kind).to_string()),
    )
    .with_application_code(error.code);
    if let Some(status) = status {
        provider_error = provider_error.with_status(status);
    }
    if let Some(request_id) = sanitized_request_id(request_id, secrets) {
        provider_error = provider_error.with_request_id(request_id);
    }
    provider_error
}

pub(super) fn classify_failure(
    status: Option<u16>,
    code: Option<i64>,
    message: &str,
) -> ProviderErrorKind {
    match status {
        Some(400) => return ProviderErrorKind::InvalidRequest,
        Some(401) => return ProviderErrorKind::Authentication,
        Some(402) => return ProviderErrorKind::Quota,
        Some(403) => return ProviderErrorKind::Permission,
        Some(408 | 425 | 500 | 502 | 503 | 504) => return ProviderErrorKind::Unavailable,
        Some(429) => return ProviderErrorKind::RateLimited,
        Some(status) if (400..=499).contains(&status) => {
            return ProviderErrorKind::InvalidRequest;
        }
        Some(_) => return ProviderErrorKind::Unavailable,
        None => {}
    }

    let message = message.to_ascii_lowercase();
    if message.contains("rate limit") || message.contains("too many requests") {
        ProviderErrorKind::RateLimited
    } else if message.contains("quota")
        || message.contains("credit")
        || message.contains("limit reached")
    {
        ProviderErrorKind::Quota
    } else if message.contains("api key")
        || message.contains("credential")
        || message.contains("unauthorized")
        || message.contains("authentication")
    {
        ProviderErrorKind::Authentication
    } else if message.contains("permission") || message.contains("forbidden") {
        ProviderErrorKind::Permission
    } else if matches!(code, Some(-32603 | -32099..=-32000)) {
        ProviderErrorKind::Unavailable
    } else {
        ProviderErrorKind::InvalidRequest
    }
}

fn failure_message(kind: ProviderErrorKind) -> &'static str {
    match kind {
        ProviderErrorKind::InvalidRequest => "AnySearch rejected the request",
        ProviderErrorKind::Authentication => "AnySearch rejected the configured credential",
        ProviderErrorKind::Permission => "AnySearch denied access",
        ProviderErrorKind::Quota => "AnySearch quota is exhausted",
        ProviderErrorKind::RateLimited => "AnySearch rate limit exceeded",
        ProviderErrorKind::Unavailable => "AnySearch is temporarily unavailable",
        ProviderErrorKind::InvalidResponse | ProviderErrorKind::Transport => {
            "AnySearch request failed"
        }
    }
}

fn sanitized_request_id(value: Option<&str>, secrets: &[&str]) -> Option<String> {
    value
        .map(|value| sanitize_provider_text_with_secrets(value, 128, secrets))
        .filter(|value| !value.is_empty())
}

pub(super) fn invalid_request(message: &str) -> SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidRequest, message).into()
}

pub(super) fn invalid_response(message: &str) -> SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidResponse, message).into()
}
