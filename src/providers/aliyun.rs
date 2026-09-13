//! Native Alibaba Cloud IQS Unified Search provider.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::http::ProviderHttpClient;
use super::json_api::{invalid_config, JsonSearchApi};
use super::protocol::{
    bounded_relevance, sanitize_provider_multiline_text, sanitize_provider_text_with_secrets,
    strip_simple_markup, validated_web_url,
};
use super::{
    CredentialSource, ProviderCapabilities, ProviderDescriptor, ProviderHttpConfig,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use crate::{ProviderErrorKind, Result, ResultType, SearchImage, SearchUsage};

const PROVIDER_ID: &str = "aliyun";
const DEFAULT_ENDPOINT: &str = "https://cloud-iqs.aliyuncs.com/search/unified";

/// IQS engine selected for one Alibaba Cloud search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliyunEngineType {
    /// Standard open-web search.
    Generic,
    /// Standard search with more authoritative recall.
    GenericAdvanced,
    /// Lightweight semantic search. Supports result count and site filters.
    LiteAdvanced,
}

impl AliyunEngineType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "Generic",
            Self::GenericAdvanced => "GenericAdvanced",
            Self::LiteAdvanced => "LiteAdvanced",
        }
    }
}

/// Typed Alibaba Cloud IQS request defaults and credentials.
#[derive(Debug, Clone)]
pub struct AliyunConfig {
    api: JsonSearchApi,
    engine_type: AliyunEngineType,
    max_results: u8,
    include_main_text: bool,
}

impl AliyunConfig {
    /// Creates the default IQS configuration.
    ///
    /// `ALIYUN_IQS_API_KEY` is required. The default engine is `LiteAdvanced`
    /// because it accepts a result count and returns AI-oriented snippets.
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: JsonSearchApi::new(
                PROVIDER_ID,
                "Alibaba Cloud IQS",
                DEFAULT_ENDPOINT,
                "ALIYUN_IQS_API_KEY",
            )?,
            engine_type: AliyunEngineType::LiteAdvanced,
            max_results: 10,
            include_main_text: false,
        })
    }

    /// Replaces the API endpoint.
    pub fn with_endpoint(mut self, endpoint: Url) -> Result<Self> {
        self.api = self.api.with_endpoint(endpoint)?;
        Ok(self)
    }

    /// Replaces the API-key source.
    pub fn with_api_key(mut self, api_key: CredentialSource) -> Self {
        self.api = self.api.with_api_key(api_key);
        self
    }

    /// Sets the IQS engine.
    ///
    /// Result count is only sent for [`AliyunEngineType::LiteAdvanced`].
    pub fn with_engine_type(mut self, engine_type: AliyunEngineType) -> Result<Self> {
        if engine_type != AliyunEngineType::LiteAdvanced && self.max_results != 10 {
            return Err(invalid_config(
                PROVIDER_ID,
                "Alibaba Cloud IQS max_results is only supported for lite-advanced",
            ));
        }
        self.engine_type = engine_type;
        Ok(self)
    }

    /// Sets the result count sent to `LiteAdvanced` (`1..=50`).
    pub fn with_max_results(mut self, max_results: u8) -> Result<Self> {
        if !(1..=50).contains(&max_results) {
            return Err(invalid_config(
                PROVIDER_ID,
                "Alibaba Cloud IQS max_results must be between 1 and 50",
            ));
        }
        if self.engine_type != AliyunEngineType::LiteAdvanced && max_results != 10 {
            return Err(invalid_config(
                PROVIDER_ID,
                "Alibaba Cloud IQS max_results is only supported for lite-advanced",
            ));
        }
        self.max_results = max_results;
        Ok(self)
    }

    /// Requests parsed page text. This is a billed content option.
    pub fn with_include_main_text(mut self, include_main_text: bool) -> Self {
        self.include_main_text = include_main_text;
        self
    }

    /// Replaces HTTP transport limits.
    pub fn with_http_config(mut self, http: ProviderHttpConfig) -> Self {
        self.api = self.api.with_http_config(http);
        self
    }

    /// Returns the configured endpoint.
    pub fn endpoint(&self) -> &Url {
        self.api.endpoint()
    }
}

/// Native Rust implementation of Alibaba Cloud IQS Unified Search.
#[derive(Debug)]
pub struct AliyunProvider {
    config: AliyunConfig,
    client: ProviderHttpClient,
}

impl AliyunProvider {
    /// Creates a provider from typed configuration.
    pub fn new(config: AliyunConfig) -> Result<Self> {
        let client = config.api.client()?;
        Ok(Self { config, client })
    }

    /// Creates a provider using `ALIYUN_IQS_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::new(AliyunConfig::new()?)
    }
}

#[async_trait]
impl SearchProvider for AliyunProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            PROVIDER_ID,
            "Alibaba Cloud IQS",
            "https://www.aliyun.com/product/iqs",
            ProviderCapabilities::new()
                .with_time_range(true)
                .with_answers(true)
                .with_images(true)
                .with_full_text(true)
                .with_usage(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        self.config.api.readiness()
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        let query = self.config.api.bounded_query(request, 1_024)?;
        let payload = AliyunRequest {
            query,
            time_range: request
                .time_range
                .map(aliyun_time_range)
                .unwrap_or("NoLimit"),
            engine_type: self.config.engine_type.as_str(),
            contents: AliyunContents {
                main_text: self.config.include_main_text,
                markdown_text: false,
                summary: false,
                rerank_score: true,
            },
            advanced_params: (self.config.engine_type == AliyunEngineType::LiteAdvanced).then(
                || AliyunAdvancedParams {
                    num_results: self.config.max_results.to_string(),
                },
            ),
        };
        let reply = self.config.api.post_json(&self.client, &payload).await?;
        let secrets = reply.secrets();
        if !reply.is_success() {
            return Err(
                reply.reject("Alibaba Cloud IQS request failed", |code, _, _| {
                    code.and_then(aliyun_code_kind)
                }),
            );
        }

        let payload: AliyunResponse = reply.decode()?;
        if let Some(code) = payload.code.as_deref().filter(|code| !code.is_empty()) {
            return Err(reply.reject_with(
                "Alibaba Cloud IQS rejected the request",
                payload.message.as_deref(),
                payload.request_id.as_deref(),
                aliyun_code_kind(code).or(Some(ProviderErrorKind::InvalidRequest)),
            ));
        }

        let results = payload
            .page_items
            .unwrap_or_default()
            .into_iter()
            .take(usize::from(self.config.max_results))
            .filter_map(|hit| adapt_hit(hit, &secrets))
            .collect();
        let answers = payload
            .scene_items
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| scene_answer(&item, &secrets))
            .take(3)
            .collect();
        let request_id = payload
            .request_id
            .map(|value| sanitize_provider_text_with_secrets(&value, 128, &secrets))
            .filter(|value| !value.is_empty());
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "engine_type".to_string(),
            Value::String(self.config.engine_type.as_str().to_string()),
        );
        Ok(reply.seal(ProviderResponse {
            results,
            answers,
            report: ProviderReport {
                request_id,
                response_time_ms: payload
                    .search_information
                    .and_then(|information| information.search_time),
                usage: payload.cost_credits.and_then(usage_credits),
                metadata,
                ..Default::default()
            },
            ..Default::default()
        }))
    }
}

#[derive(Serialize)]
struct AliyunRequest<'a> {
    query: &'a str,
    #[serde(rename = "timeRange")]
    time_range: &'a str,
    #[serde(rename = "engineType")]
    engine_type: &'a str,
    contents: AliyunContents,
    #[serde(rename = "advancedParams", skip_serializing_if = "Option::is_none")]
    advanced_params: Option<AliyunAdvancedParams>,
}

#[derive(Serialize)]
struct AliyunContents {
    #[serde(rename = "mainText")]
    main_text: bool,
    #[serde(rename = "markdownText")]
    markdown_text: bool,
    summary: bool,
    #[serde(rename = "rerankScore")]
    rerank_score: bool,
}

#[derive(Serialize)]
struct AliyunAdvancedParams {
    #[serde(rename = "numResults")]
    num_results: String,
}

#[derive(Deserialize)]
struct AliyunResponse {
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    code: Option<String>,
    message: Option<String>,
    #[serde(rename = "pageItems")]
    page_items: Option<Vec<AliyunHit>>,
    #[serde(rename = "sceneItems")]
    scene_items: Option<Vec<AliyunScene>>,
    #[serde(rename = "searchInformation")]
    search_information: Option<AliyunSearchInformation>,
    #[serde(rename = "costCredits")]
    cost_credits: Option<Value>,
}

#[derive(Deserialize)]
struct AliyunHit {
    title: Option<String>,
    link: Option<String>,
    snippet: Option<String>,
    #[serde(rename = "publishedTime")]
    published_time: Option<String>,
    #[serde(rename = "mainText")]
    main_text: Option<String>,
    images: Option<Vec<String>>,
    #[serde(rename = "hostLogo")]
    host_logo: Option<String>,
    #[serde(rename = "rerankScore")]
    rerank_score: Option<f64>,
}

#[derive(Deserialize)]
struct AliyunScene {
    detail: Option<String>,
}

#[derive(Deserialize)]
struct AliyunSearchInformation {
    #[serde(rename = "searchTime")]
    search_time: Option<u64>,
}

fn adapt_hit(hit: AliyunHit, secrets: &[&str]) -> Option<ProviderResult> {
    let url = hit.link.as_deref().and_then(validated_web_url)?;
    let title = JsonSearchApi::clean(hit.title.as_deref().unwrap_or(&url), 300, secrets);
    let snippet = JsonSearchApi::clean(hit.snippet.as_deref().unwrap_or(""), 2_000, secrets);
    let mut result = ProviderResult::new(url, title, snippet).with_result_type(ResultType::Web);
    if let Some(score) = hit.rerank_score.and_then(bounded_relevance) {
        result = result.with_relevance_score(score);
    }
    if let Some(date) = hit.published_time {
        let date = JsonSearchApi::clean(&date, 64, secrets);
        if !date.is_empty() {
            result = result.with_published_date(date);
        }
    }
    if let Some(text) = hit.main_text {
        let text =
            sanitize_provider_text_with_secrets(&strip_simple_markup(&text), 32 * 1024, secrets);
        let text = sanitize_provider_multiline_text(&text, 32 * 1024);
        if !text.is_empty() {
            result = result.with_full_text(text);
        }
    }
    if let Some(logo) = hit.host_logo.as_deref().and_then(validated_web_url) {
        result = result.with_favicon(logo);
    }
    for image in hit.images.unwrap_or_default() {
        if let Some(image) = validated_web_url(&image) {
            result = result.with_image(SearchImage::new(image));
        }
    }
    Some(result)
}

fn scene_answer(item: &AliyunScene, secrets: &[&str]) -> Option<String> {
    let detail = item.detail.as_deref()?.trim();
    if detail.is_empty() {
        return None;
    }
    let text = if let Ok(Value::Object(object)) = serde_json::from_str::<Value>(detail) {
        object
            .get("title")
            .or_else(|| object.get("content"))
            .or_else(|| object.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    } else {
        detail.to_string()
    };
    let text = JsonSearchApi::clean(&text, 500, secrets);
    (!text.is_empty()).then_some(text)
}

fn usage_credits(value: Value) -> Option<SearchUsage> {
    let mut credits = 0.0;
    accumulate_numbers(&value, &mut credits);
    (credits.is_finite() && credits >= 0.0).then(|| SearchUsage::new().with_credits(credits))
}

fn accumulate_numbers(value: &Value, total: &mut f64) {
    match value {
        Value::Number(number) => {
            if let Some(number) = number.as_f64() {
                *total += number;
            }
        }
        Value::Object(object) => {
            for child in object.values() {
                accumulate_numbers(child, total);
            }
        }
        Value::Array(values) => {
            for child in values {
                accumulate_numbers(child, total);
            }
        }
        _ => {}
    }
}

fn aliyun_time_range(range: crate::TimeRange) -> &'static str {
    match range {
        crate::TimeRange::Day => "OneDay",
        crate::TimeRange::Week => "OneWeek",
        crate::TimeRange::Month => "OneMonth",
        crate::TimeRange::Year => "OneYear",
    }
}

fn aliyun_code_kind(code: &str) -> Option<ProviderErrorKind> {
    match code {
        "InvalidAccessKeyId.NotFound" => Some(ProviderErrorKind::Authentication),
        "Retrieval.NotActivate" | "Retrieval.NotAuthorised" => Some(ProviderErrorKind::Permission),
        "Retrieval.Arrears"
        | "Retrieval.TestUserPeriodExpired"
        | "Retrieval.TestUserQueryPerDayExceeded" => Some(ProviderErrorKind::Quota),
        "Retrieval.Throttling.User" => Some(ProviderErrorKind::RateLimited),
        "Retrieval.InvalidPublishedDate" | "InvalidParameter" => {
            Some(ProviderErrorKind::InvalidRequest)
        }
        "InternalServerError" => Some(ProviderErrorKind::Unavailable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_count_is_rejected_outside_lite_advanced() {
        let config = AliyunConfig::new()
            .unwrap()
            .with_engine_type(AliyunEngineType::Generic)
            .unwrap();
        assert!(config.with_max_results(5).is_err());
        assert!(AliyunConfig::new().unwrap().with_max_results(0).is_err());
    }
}
