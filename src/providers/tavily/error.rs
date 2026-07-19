//! Sanitized Tavily HTTP error mapping.

use serde::Deserialize;
use serde_json::Value;

use super::super::http::ProviderHttpResponse;
use super::super::protocol::sanitize_provider_text_with_secrets;
use super::PROVIDER_ID;
use crate::{ProviderError, ProviderErrorKind, SearchError};

#[derive(Deserialize)]
struct TavilyErrorEnvelope {
    detail: Option<Value>,
    message: Option<String>,
    request_id: Option<String>,
}

pub(super) fn tavily_error(response: &ProviderHttpResponse, secrets: &[&str]) -> SearchError {
    let status = response.status.as_u16();
    let envelope: Option<TavilyErrorEnvelope> = serde_json::from_slice(&response.body).ok();
    let kind = match status {
        400 => ProviderErrorKind::InvalidRequest,
        401 => ProviderErrorKind::Authentication,
        402 | 432 | 433 => ProviderErrorKind::Quota,
        403 => ProviderErrorKind::Permission,
        408 | 425 | 500 | 502 | 503 | 504 => ProviderErrorKind::Unavailable,
        429 => ProviderErrorKind::RateLimited,
        status if (400..=499).contains(&status) => ProviderErrorKind::InvalidRequest,
        _ => ProviderErrorKind::Unavailable,
    };
    let fallback = match kind {
        ProviderErrorKind::InvalidRequest => "Tavily rejected the request",
        ProviderErrorKind::Authentication => "Tavily rejected the configured credential",
        ProviderErrorKind::Permission => "Tavily denied access",
        ProviderErrorKind::Quota => "Tavily plan or usage limit reached",
        ProviderErrorKind::RateLimited => "Tavily rate limit exceeded",
        ProviderErrorKind::Unavailable => "Tavily is temporarily unavailable",
        ProviderErrorKind::InvalidResponse | ProviderErrorKind::Transport => {
            "Tavily request failed"
        }
    };
    let message = envelope
        .as_ref()
        .and_then(error_message)
        .map(|message| sanitize_provider_text_with_secrets(message, 300, secrets))
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| fallback.to_string());
    let request_id = envelope
        .and_then(|envelope| envelope.request_id)
        .map(|request_id| sanitize_provider_text_with_secrets(&request_id, 128, secrets))
        .filter(|request_id| !request_id.is_empty())
        .or_else(|| {
            response
                .header("x-request-id")
                .map(|request_id| sanitize_provider_text_with_secrets(request_id, 128, secrets))
        });

    let mut error = ProviderError::new(PROVIDER_ID, kind, message).with_status(status);
    if let Some(request_id) = request_id {
        error = error.with_request_id(request_id);
    }
    if let Some(retry_after) = response.retry_after_seconds() {
        error = error.with_retry_after(retry_after);
    }
    error.into()
}

fn error_message(envelope: &TavilyErrorEnvelope) -> Option<&str> {
    envelope
        .message
        .as_deref()
        .or_else(|| match &envelope.detail {
            Some(Value::String(message)) => Some(message),
            Some(Value::Object(detail)) => detail
                .get("message")
                .or_else(|| detail.get("error"))
                .and_then(Value::as_str),
            _ => None,
        })
}

pub(super) fn invalid_request(message: &str) -> SearchError {
    ProviderError::new(PROVIDER_ID, ProviderErrorKind::InvalidRequest, message).into()
}
