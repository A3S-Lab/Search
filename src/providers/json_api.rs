//! Transport shell for required-credential JSON search APIs.
//!
//! This type owns every step that can leak a credential or accept an unsafe
//! endpoint. A vendor maps its request and response, then classifies vendor
//! error codes. It does not build authorization headers or retain the key.

use reqwest::header::{HeaderMap, AUTHORIZATION};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use super::credential::SecretString;
use super::http::{
    bearer_header, insert_header, secret_header, validate_provider_endpoint, ProviderHttpClient,
    ProviderHttpResponse,
};
use super::protocol::{
    redact_credential, sanitize_provider_text_with_secrets, strip_simple_markup,
};
use super::{
    CredentialSource, ProviderAuthentication, ProviderHttpConfig, ProviderReadiness,
    ProviderRequest, ProviderResponse,
};
use crate::{ProviderError, ProviderErrorKind, Result, SearchError, TimeRange};

/// How a JSON search API sends its required credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonAuth {
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// A custom header whose value is the raw key.
    Header(&'static str),
}

/// Transport, credential, and identity shared by authenticated JSON providers.
#[derive(Debug, Clone)]
pub(crate) struct JsonSearchApi {
    id: &'static str,
    name: &'static str,
    endpoint: Url,
    api_key: CredentialSource,
    http: ProviderHttpConfig,
    auth: JsonAuth,
}

impl JsonSearchApi {
    /// Creates a required-credential API from its documented defaults.
    pub(crate) fn new(
        id: &'static str,
        name: &'static str,
        endpoint: &str,
        environment: &'static str,
    ) -> Result<Self> {
        let endpoint = Url::parse(endpoint).map_err(|_| {
            ProviderError::new(
                id,
                ProviderErrorKind::InvalidRequest,
                "built-in provider endpoint is invalid",
            )
        })?;
        validate_provider_endpoint(id, &endpoint)?;
        Ok(Self {
            id,
            name,
            endpoint,
            api_key: CredentialSource::environment(environment),
            http: ProviderHttpConfig::default(),
            auth: JsonAuth::Bearer,
        })
    }

    /// Selects a non-bearer authorization header.
    pub(crate) fn with_auth(mut self, auth: JsonAuth) -> Self {
        self.auth = auth;
        self
    }

    /// Replaces the endpoint after the shared safety checks.
    pub(crate) fn with_endpoint(mut self, endpoint: Url) -> Result<Self> {
        validate_provider_endpoint(self.id, &endpoint)?;
        self.endpoint = endpoint;
        Ok(self)
    }

    /// Replaces the credential source.
    pub(crate) fn with_api_key(mut self, api_key: CredentialSource) -> Self {
        self.api_key = api_key;
        self
    }

    /// Replaces HTTP transport limits.
    pub(crate) fn with_http_config(mut self, http: ProviderHttpConfig) -> Self {
        self.http = http;
        self
    }

    /// Returns the configured endpoint.
    pub(crate) fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Builds the redirect-free client used by the provider.
    pub(crate) fn client(&self) -> Result<ProviderHttpClient> {
        validate_provider_endpoint(self.id, &self.endpoint)?;
        ProviderHttpClient::new(self.id, self.http)
    }

    /// Reports whether a header-safe credential is configured.
    pub(crate) fn readiness(&self) -> ProviderReadiness {
        match self.api_key.resolve(self.id) {
            Ok(Some(credential))
                if secret_header(self.id, credential.expose().to_string()).is_ok() =>
            {
                ProviderReadiness::Ready {
                    authentication: ProviderAuthentication::Authenticated,
                }
            }
            Ok(Some(_)) | Err(_) => ProviderReadiness::InvalidCredential,
            Ok(None) => ProviderReadiness::MissingCredential {
                environment_variable: self.api_key.environment_variable().map(str::to_string),
            },
        }
    }

    /// Strips simple markup and redacts the given secrets.
    pub(crate) fn clean(value: &str, max_chars: usize, secrets: &[&str]) -> String {
        clean_text(value, max_chars, secrets)
    }

    /// Returns a trimmed query inside the vendor character bound.
    pub(crate) fn bounded_query<'a>(
        &self,
        request: &'a ProviderRequest,
        max_chars: usize,
    ) -> Result<&'a str> {
        let query = request.query.trim();
        if query.is_empty() || query.chars().count() > max_chars {
            let message = format!("{} query must be 1 to {max_chars} characters", self.name);
            return Err(invalid_request(self.id, &message));
        }
        Ok(query)
    }

    /// Sends an authorized GET and returns a reply that still owns the key.
    pub(crate) async fn get(&self, client: &ProviderHttpClient, url: &Url) -> Result<JsonReply> {
        let authorized = self.authorize()?;
        let response = client.get(url, authorized.headers()).await?;
        Ok(JsonReply::new(self, response, authorized.key))
    }

    /// Sends an authorized JSON POST to the configured endpoint.
    pub(crate) async fn post_json<T: Serialize + ?Sized>(
        &self,
        client: &ProviderHttpClient,
        body: &T,
    ) -> Result<JsonReply> {
        let authorized = self.authorize()?;
        let response = client
            .post_json(&self.endpoint, authorized.headers(), body)
            .await?;
        Ok(JsonReply::new(self, response, authorized.key))
    }

    fn authorize(&self) -> Result<AuthorizedCall> {
        let key = self.api_key.resolve(self.id)?.ok_or_else(|| {
            ProviderError::new(
                self.id,
                ProviderErrorKind::Authentication,
                format!("{} API key is not configured", self.name),
            )
        })?;
        let mut headers = HeaderMap::new();
        match self.auth {
            JsonAuth::Bearer => {
                headers.insert(AUTHORIZATION, bearer_header(self.id, &key)?);
            }
            JsonAuth::Header(name) => {
                insert_header(
                    self.id,
                    &mut headers,
                    name,
                    secret_header(self.id, key.expose().to_string())?,
                )?;
            }
        }
        Ok(AuthorizedCall { headers, key })
    }
}

struct AuthorizedCall {
    headers: HeaderMap,
    key: SecretString,
}

impl AuthorizedCall {
    fn headers(&self) -> HeaderMap {
        self.headers.clone()
    }
}

/// One authorized HTTP exchange. The vendor maps the body; this value redacts.
pub(crate) struct JsonReply {
    id: &'static str,
    name: &'static str,
    response: ProviderHttpResponse,
    secret: SecretString,
}

impl JsonReply {
    fn new(api: &JsonSearchApi, response: ProviderHttpResponse, secret: SecretString) -> Self {
        Self {
            id: api.id,
            name: api.name,
            response,
            secret,
        }
    }

    pub(crate) fn is_success(&self) -> bool {
        self.response.status.is_success()
    }

    pub(crate) fn status(&self) -> u16 {
        self.response.status.as_u16()
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.response.body
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.response.header(name)
    }

    /// Returns the resolved secret so response mapping can redact it.
    pub(crate) fn secrets(&self) -> [&str; 1] {
        [self.secret.expose()]
    }

    /// Redacts the call credential from every caller-visible string.
    ///
    /// Vendor mapping still bounds and classifies fields. This is the boundary
    /// that keeps a missed field from leaving with the key.
    pub(crate) fn seal(&self, response: ProviderResponse) -> ProviderResponse {
        redact_response(response, self.secret.expose())
    }

    /// Decodes the body as JSON.
    pub(crate) fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(self.body()).map_err(|_| self.contract_error())
    }

    /// Reports that a decoded success body did not match the vendor contract.
    pub(crate) fn contract_error(&self) -> SearchError {
        ProviderError::new(
            self.id,
            ProviderErrorKind::InvalidResponse,
            format!("{} success response did not match its contract", self.name),
        )
        .into()
    }

    /// Classifies a failed exchange using the shared error envelope.
    pub(crate) fn reject(
        &self,
        fallback: &str,
        classify: impl FnOnce(Option<&str>, Option<&str>, u16) -> Option<ProviderErrorKind>,
    ) -> SearchError {
        let (code, message, request_id) = error_fields(self.body());
        let kind = classify(code.as_deref(), message.as_deref(), self.status());
        self.reject_with(fallback, message.as_deref(), request_id.as_deref(), kind)
    }

    /// Classifies a failure whose message already came from a typed body.
    pub(crate) fn reject_with(
        &self,
        fallback: &str,
        message: Option<&str>,
        request_id: Option<&str>,
        kind: Option<ProviderErrorKind>,
    ) -> SearchError {
        let status = self.status();
        let message = message
            .map(|message| sanitize_provider_text_with_secrets(message, 300, &self.secrets()))
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| fallback.to_string());
        let request_id = request_id
            .map(|request_id| sanitize_provider_text_with_secrets(request_id, 128, &self.secrets()))
            .filter(|request_id| !request_id.is_empty())
            .or_else(|| {
                self.header("x-request-id").map(|request_id| {
                    sanitize_provider_text_with_secrets(request_id, 128, &self.secrets())
                })
            });
        let mut error = ProviderError::new(
            self.id,
            kind.unwrap_or_else(|| status_kind(status)),
            message,
        )
        .with_status(status);
        if let Some(request_id) = request_id {
            error = error.with_request_id(request_id);
        }
        if let Some(retry_after) = self.response.retry_after_seconds() {
            error = error.with_retry_after(retry_after);
        }
        error.into()
    }
}

fn status_kind(status: u16) -> ProviderErrorKind {
    match status {
        400 => ProviderErrorKind::InvalidRequest,
        401 => ProviderErrorKind::Authentication,
        402 => ProviderErrorKind::Quota,
        403 => ProviderErrorKind::Permission,
        408 | 425 | 500 | 502 | 503 | 504 => ProviderErrorKind::Unavailable,
        429 => ProviderErrorKind::RateLimited,
        status if (400..=499).contains(&status) => ProviderErrorKind::InvalidRequest,
        _ => ProviderErrorKind::Unavailable,
    }
}

/// Extracts a vendor code, human message, and request id from a JSON error envelope.
fn error_fields(body: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return (None, None, None);
    };
    let payload = value
        .get("Response")
        .filter(|value| value.is_object())
        .unwrap_or(&value);
    let error = payload
        .get("Error")
        .or_else(|| payload.get("error"))
        .filter(|value| value.is_object());
    let code = string_field(error.unwrap_or(payload), &["Code", "code"])
        .or_else(|| string_field(payload, &["code", "Code"]));
    let message = string_field(error.unwrap_or(payload), &["Message", "message", "msg"])
        .or_else(|| string_field(payload, &["message", "Message", "msg", "detail"]))
        .or_else(|| match payload.get("error") {
            Some(Value::String(value)) if !value.trim().is_empty() => {
                Some(value.trim().to_string())
            }
            _ => None,
        });
    let request_id = string_field(
        payload,
        &["RequestId", "requestId", "request_id", "log_id", "logId"],
    );
    (code, message, request_id)
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| match value.get(*name) {
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    })
}

pub(crate) fn clean_text(value: &str, max_chars: usize, secrets: &[&str]) -> String {
    sanitize_provider_text_with_secrets(&strip_simple_markup(value), max_chars, secrets)
}

fn redact_response(mut response: ProviderResponse, secret: &str) -> ProviderResponse {
    if secret.is_empty() {
        return response;
    }
    for result in &mut response.results {
        redact_string(&mut result.url, secret);
        redact_string(&mut result.title, secret);
        redact_string(&mut result.snippet, secret);
        redact_option(&mut result.full_text, secret);
        redact_option(&mut result.thumbnail, secret);
        redact_option(&mut result.published_date, secret);
        redact_option(&mut result.favicon, secret);
        for image in &mut result.images {
            redact_string(&mut image.url, secret);
            redact_option(&mut image.description, secret);
        }
    }
    for suggestion in &mut response.suggestions {
        redact_string(suggestion, secret);
    }
    for answer in &mut response.answers {
        redact_string(answer, secret);
    }
    for image in &mut response.images {
        redact_string(&mut image.url, secret);
        redact_option(&mut image.description, secret);
    }
    redact_option(&mut response.report.request_id, secret);
    let metadata = std::mem::take(&mut response.report.metadata);
    response.report.metadata = metadata
        .into_iter()
        .map(|(mut key, mut value)| {
            redact_string(&mut key, secret);
            redact_json(&mut value, secret);
            (key, value)
        })
        .collect();
    response
}

fn redact_option(value: &mut Option<String>, secret: &str) {
    if let Some(value) = value {
        redact_string(value, secret);
    }
}

fn redact_string(value: &mut String, secret: &str) {
    redact_credential(value, secret);
}

fn redact_json(value: &mut Value, secret: &str) {
    match value {
        Value::String(text) => redact_string(text, secret),
        Value::Array(values) => {
            for value in values {
                redact_json(value, secret);
            }
        }
        Value::Object(fields) => {
            let pairs = std::mem::take(fields);
            *fields = pairs
                .into_iter()
                .map(|(mut key, mut child)| {
                    redact_string(&mut key, secret);
                    redact_json(&mut child, secret);
                    (key, child)
                })
                .collect();
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub(crate) fn invalid_request(provider: &'static str, message: &str) -> SearchError {
    ProviderError::new(provider, ProviderErrorKind::InvalidRequest, message).into()
}

pub(crate) fn invalid_config(provider: &'static str, message: &str) -> SearchError {
    ProviderError::new(provider, ProviderErrorKind::InvalidRequest, message).into()
}

/// Provider-neutral duration for a [`TimeRange`].
pub(crate) struct TimeWindow {
    /// Inclusive recency window in minutes.
    pub minutes: u32,
    /// Inclusive recency window in seconds.
    pub seconds: u64,
}

/// Maps a search time range to a shared duration.
///
/// Vendor query parameters stay in the vendor module.
pub(crate) fn time_window(range: TimeRange) -> TimeWindow {
    match range {
        TimeRange::Day => TimeWindow {
            minutes: 1_440,
            seconds: 24 * 60 * 60,
        },
        TimeRange::Week => TimeWindow {
            minutes: 10_080,
            seconds: 7 * 24 * 60 * 60,
        },
        TimeRange::Month => TimeWindow {
            minutes: 43_200,
            seconds: 30 * 24 * 60 * 60,
        },
        TimeRange::Year => TimeWindow {
            minutes: 525_600,
            seconds: 365 * 24 * 60 * 60,
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::redact_response;
    use crate::providers::{ProviderReport, ProviderResponse, ProviderResult};
    use crate::SearchImage;

    #[test]
    fn sealed_response_redacts_the_credential_in_every_visible_string() {
        let secret = "sk-live-secret";
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert(secret.to_string(), json!({"note": secret}));
        let response = ProviderResponse {
            results: vec![ProviderResult::new(
                format!("https://example.com/?q={secret}"),
                secret,
                format!("see {secret}"),
            )
            .with_full_text(secret)
            .with_image(SearchImage::new(secret).with_description(secret))],
            suggestions: vec![secret.to_string()],
            answers: vec![secret.to_string()],
            images: vec![SearchImage::new(secret)],
            report: ProviderReport {
                request_id: Some(secret.to_string()),
                metadata,
                ..Default::default()
            },
        };

        let sealed = redact_response(response, secret);
        let rendered = format!("{sealed:?}");
        assert!(!rendered.contains(secret));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn sealed_response_redacts_transport_encodings_of_the_credential() {
        let secret = "sk/live+secret";
        let encoded = urlencoding::encode(secret).into_owned();
        let flipped = flip_percent_hex_case(&encoded);
        assert_ne!(
            flipped, encoded,
            "the encoding must have hex digits to flip"
        );

        let response = ProviderResponse {
            results: vec![ProviderResult::new(
                format!("https://example.com/result?k={flipped}"),
                format!("see {flipped}"),
                "text",
            )],
            ..Default::default()
        };
        let sealed = redact_response(response, secret);
        let rendered = format!("{sealed:?}");
        assert!(!rendered.contains(secret), "{rendered}");
        assert!(!rendered.contains(&flipped), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
    }

    fn flip_percent_hex_case(value: &str) -> String {
        let bytes = value.as_bytes();
        let mut flipped = String::with_capacity(value.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%'
                && index + 2 < bytes.len()
                && bytes[index + 1].is_ascii_hexdigit()
                && bytes[index + 2].is_ascii_hexdigit()
            {
                flipped.push('%');
                flipped.push(match bytes[index + 1] {
                    b'a'..=b'f' => bytes[index + 1] - b'a' + b'A',
                    b'A'..=b'F' => bytes[index + 1] - b'A' + b'a',
                    digit => digit,
                } as char);
                flipped.push(match bytes[index + 2] {
                    b'a'..=b'f' => bytes[index + 2] - b'a' + b'A',
                    b'A'..=b'F' => bytes[index + 2] - b'A' + b'a',
                    digit => digit,
                } as char);
                index += 3;
                continue;
            }
            flipped.push(bytes[index] as char);
            index += 1;
        }
        flipped
    }
}
