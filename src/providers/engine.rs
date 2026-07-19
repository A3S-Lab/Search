//! Adapter from the provider protocol to the public engine protocol.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{
    metadata::sanitize_provider_metadata, normalization::normalize_provider_response,
    protocol::sanitize_provider_text, ProviderDescriptor, ProviderReadiness, ProviderRequest,
    ProviderResponse, SearchProvider,
};
use crate::{
    Engine, EngineCategory, EngineConfig, EngineOutput, ProviderError, ProviderErrorKind, Result,
    SearchQuery, SearchReport, SearchResult, SearchUsage,
};

/// Reusable adapter that runs a [`SearchProvider`] as an [`Engine`].
pub struct ProviderEngine {
    config: EngineConfig,
    provider: Arc<dyn SearchProvider>,
}

impl ProviderEngine {
    /// Creates an engine from a provider implementation.
    pub fn new<P>(provider: P) -> Self
    where
        P: SearchProvider + 'static,
    {
        Self::from_arc(Arc::new(provider))
    }

    /// Creates an engine from a shared provider implementation.
    pub fn from_arc(provider: Arc<dyn SearchProvider>) -> Self {
        let descriptor = provider.descriptor();
        let config = EngineConfig {
            name: descriptor.name.to_string(),
            shortcut: descriptor.id.to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 15,
            enabled: true,
            paging: descriptor.capabilities.paging,
            safesearch: descriptor.capabilities.safe_search,
        };
        Self { config, provider }
    }

    /// Replaces the engine-facing configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    /// Returns the provider descriptor.
    pub fn descriptor(&self) -> ProviderDescriptor {
        self.provider.descriptor()
    }

    /// Returns provider readiness without exposing credential values.
    pub fn readiness(&self) -> ProviderReadiness {
        self.provider.readiness()
    }

    /// Returns the underlying provider protocol object.
    pub fn provider(&self) -> &dyn SearchProvider {
        self.provider.as_ref()
    }

    async fn execute(&self, query: &SearchQuery) -> Result<EngineOutput> {
        let descriptor = self.provider.descriptor();
        if query.page == 0 {
            return Err(ProviderError::new(
                descriptor.id,
                ProviderErrorKind::InvalidRequest,
                "provider result pages start at one",
            )
            .into());
        }
        if query.page > 1 && !descriptor.capabilities.paging {
            return Err(ProviderError::new(
                descriptor.id,
                ProviderErrorKind::InvalidRequest,
                format!("{} does not support result pagination", descriptor.name),
            )
            .into());
        }
        if query.safesearch != crate::SafeSearch::Off && !descriptor.capabilities.safe_search {
            return Err(ProviderError::new(
                descriptor.id,
                ProviderErrorKind::InvalidRequest,
                format!("{} does not support safe-search controls", descriptor.name),
            )
            .into());
        }
        if query.time_range.is_some() && !descriptor.capabilities.time_range {
            return Err(ProviderError::new(
                descriptor.id,
                ProviderErrorKind::InvalidRequest,
                format!("{} does not support time-range filters", descriptor.name),
            )
            .into());
        }

        let request = ProviderRequest::from(query);
        let response = self.provider.search(&request).await?;
        Ok(self.adapt(response))
    }

    fn adapt(&self, response: ProviderResponse) -> EngineOutput {
        let response = normalize_provider_response(response);
        let descriptor = self.provider.descriptor();
        let sanitized_metadata = sanitize_provider_metadata(
            Value::Object(response.report.metadata.into_iter().collect()),
            &[],
        );
        let mut metadata: BTreeMap<String, Value> = match sanitized_metadata.value {
            Value::Object(metadata) => metadata.into_iter().collect(),
            _ => Default::default(),
        };
        if sanitized_metadata.truncated {
            metadata.insert("metadata_truncated".to_string(), Value::Bool(true));
        }
        let results = response
            .results
            .into_iter()
            .map(|provider_result| {
                let mut result = SearchResult::new(
                    provider_result.url,
                    provider_result.title,
                    provider_result.snippet,
                )
                .with_type(provider_result.result_type);
                result.full_text = provider_result.full_text;
                result.relevance_score = provider_result.relevance_score;
                result.thumbnail = provider_result.thumbnail;
                result.published_date = provider_result.published_date;
                result.favicon = provider_result.favicon;
                result.images = provider_result.images;
                result
            })
            .collect();

        let report = SearchReport {
            engine: self.config.name.clone(),
            provider: Some(descriptor.id.to_string()),
            request_id: response.report.request_id.and_then(|request_id| {
                let request_id = sanitize_provider_text(&request_id, 128);
                (!request_id.is_empty()).then_some(request_id)
            }),
            total_results: response.report.total_results,
            response_time_ms: response.report.response_time_ms,
            usage: response.report.usage.and_then(normalize_usage),
            metadata,
        };

        EngineOutput {
            results,
            suggestions: response.suggestions,
            answers: response.answers,
            images: response.images,
            reports: vec![report],
        }
    }
}

fn normalize_usage(mut usage: SearchUsage) -> Option<SearchUsage> {
    usage.credits = usage
        .credits
        .filter(|credits| credits.is_finite() && *credits >= 0.0);
    usage.credits.is_some().then_some(usage)
}

impl fmt::Debug for ProviderEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderEngine")
            .field("config", &self.config)
            .field("descriptor", &self.provider.descriptor())
            .field("readiness", &self.provider.readiness())
            .finish()
    }
}

#[async_trait]
impl Engine for ProviderEngine {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        self.execute(query).await.map(|output| output.results)
    }

    async fn search_output(&self, query: &SearchQuery) -> Result<EngineOutput> {
        self.execute(query).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        providers::{ProviderAuthentication, ProviderCapabilities, ProviderReport, ProviderResult},
        SearchImage, SearchUsage,
    };

    struct MockProvider;

    #[async_trait]
    impl SearchProvider for MockProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "mock",
                name: "Mock Provider",
                homepage: "https://example.com",
                capabilities: ProviderCapabilities::new()
                    .with_answers(true)
                    .with_full_text(true)
                    .with_usage(true),
            }
        }

        fn readiness(&self) -> ProviderReadiness {
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Authenticated,
            }
        }

        async fn search(&self, _request: &ProviderRequest) -> Result<ProviderResponse> {
            let mut metadata = BTreeMap::new();
            metadata.insert(
                "depth".to_string(),
                serde_json::Value::String("advanced".to_string()),
            );
            metadata.insert(
                "oversized".to_string(),
                serde_json::Value::String("x".repeat(2_000)),
            );
            Ok(ProviderResponse {
                results: vec![
                    ProviderResult::new("https://example.com", "Example", "Snippet")
                        .with_full_text("Full text")
                        .with_relevance_score(0.75)
                        .with_favicon("https://example.com/favicon.ico")
                        .with_image(SearchImage::new("https://example.com/page-image.png")),
                ],
                suggestions: vec!["suggestion".to_string()],
                answers: vec!["answer".to_string()],
                images: vec![SearchImage::new("https://example.com/query-image.png")],
                report: ProviderReport {
                    request_id: Some("req-1".to_string()),
                    total_results: Some(10),
                    response_time_ms: Some(125),
                    usage: Some(SearchUsage::new().with_credits(1.0)),
                    metadata,
                },
            })
        }
    }

    struct InvalidReportProvider;

    #[async_trait]
    impl SearchProvider for InvalidReportProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor::new(
                "invalid-report",
                "Invalid Report Provider",
                "https://example.com",
                ProviderCapabilities::new().with_usage(true),
            )
        }

        fn readiness(&self) -> ProviderReadiness {
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Authenticated,
            }
        }

        async fn search(&self, _request: &ProviderRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse::new().with_report(ProviderReport {
                request_id: Some(format!("  {}\n", "r".repeat(256))),
                usage: Some(SearchUsage::new().with_credits(f64::NAN)),
                ..Default::default()
            }))
        }
    }

    struct InvalidOutputProvider;

    #[async_trait]
    impl SearchProvider for InvalidOutputProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor::new(
                "invalid-output",
                "Invalid Output Provider",
                "https://example.com",
                ProviderCapabilities::new()
                    .with_answers(true)
                    .with_images(true)
                    .with_full_text(true),
            )
        }

        fn readiness(&self) -> ProviderReadiness {
            ProviderReadiness::Ready {
                authentication: ProviderAuthentication::Authenticated,
            }
        }

        async fn search(&self, _request: &ProviderRequest) -> Result<ProviderResponse> {
            let mut valid = ProviderResult::new(
                "https://example.com/result",
                format!("  {}\n", "title ".repeat(200)),
                format!("{}\u{0}", "snippet ".repeat(1_000)),
            )
            .with_full_text(format!("{}\u{0}", "body\n".repeat(60_000)))
            .with_relevance_score(f64::INFINITY)
            .with_thumbnail("javascript:alert(1)")
            .with_published_date(format!("{}\n", "2026-07-20 ".repeat(30)))
            .with_favicon("https://user:secret@example.com/favicon.ico");
            valid.images = vec![
                SearchImage::new("javascript:alert(1)"),
                SearchImage::new("https://example.com/image.png")
                    .with_description("description ".repeat(500)),
            ];

            Ok(ProviderResponse {
                results: vec![
                    ProviderResult::new("javascript:alert(1)", "Invalid", "Invalid"),
                    valid,
                ],
                suggestions: vec![format!("{}\n", "suggestion ".repeat(200))],
                answers: vec![format!("{}\u{0}", "answer ".repeat(5_000))],
                images: vec![
                    SearchImage::new("data:image/png;base64,AAAA"),
                    SearchImage::new("https://example.com/query.png")
                        .with_description("description ".repeat(500)),
                ],
                report: ProviderReport::new(),
            })
        }
    }

    #[tokio::test]
    async fn adapts_provider_response_to_rich_engine_output() {
        let engine = ProviderEngine::new(MockProvider);
        let output = engine
            .search_output(&SearchQuery::new("rust"))
            .await
            .unwrap();

        assert_eq!(engine.shortcut(), "mock");
        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].full_text.as_deref(), Some("Full text"));
        assert_eq!(output.results[0].relevance_score, Some(0.75));
        assert_eq!(
            output.results[0].favicon.as_deref(),
            Some("https://example.com/favicon.ico")
        );
        assert_eq!(output.results[0].images.len(), 1);
        assert_eq!(output.images.len(), 1);
        assert_eq!(output.answers, vec!["answer"]);
        assert_eq!(output.suggestions, vec!["suggestion"]);
        assert_eq!(output.reports[0].provider.as_deref(), Some("mock"));
        assert_eq!(output.reports[0].request_id.as_deref(), Some("req-1"));
        assert!(
            output.reports[0].metadata["oversized"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= 512
        );
        assert_eq!(
            output.reports[0].metadata["metadata_truncated"],
            serde_json::Value::Bool(true)
        );
    }

    #[tokio::test]
    async fn rejects_pagination_before_calling_non_paging_provider() {
        let engine = ProviderEngine::new(MockProvider);
        let error = engine
            .search(&SearchQuery::new("rust").with_page(2))
            .await
            .unwrap_err();

        assert_eq!(error.kind(), "provider_invalid_request");
    }

    #[tokio::test]
    async fn rejects_invalid_page_and_unsupported_query_controls() {
        let engine = ProviderEngine::new(MockProvider);

        let page_zero = engine
            .search(&SearchQuery::new("rust").with_page(0))
            .await
            .unwrap_err();
        assert_eq!(page_zero.kind(), "provider_invalid_request");

        let safe_search = engine
            .search(&SearchQuery::new("rust").with_safesearch(crate::SafeSearch::Strict))
            .await
            .unwrap_err();
        assert_eq!(safe_search.kind(), "provider_invalid_request");

        let time_range = engine
            .search(&SearchQuery::new("rust").with_time_range(crate::TimeRange::Week))
            .await
            .unwrap_err();
        assert_eq!(time_range.kind(), "provider_invalid_request");
    }

    #[tokio::test]
    async fn bounds_request_ids_and_discards_invalid_usage_from_custom_providers() {
        let output = ProviderEngine::new(InvalidReportProvider)
            .search_output(&SearchQuery::new("rust"))
            .await
            .unwrap();
        let report = &output.reports[0];

        assert_eq!(report.request_id.as_deref().unwrap().chars().count(), 128);
        assert!(!report
            .request_id
            .as_deref()
            .unwrap()
            .chars()
            .any(char::is_control));
        assert!(report.usage.is_none());
    }

    #[tokio::test]
    async fn normalizes_all_untrusted_custom_provider_output_fields() {
        let output = ProviderEngine::new(InvalidOutputProvider)
            .search_output(&SearchQuery::new("rust"))
            .await
            .unwrap();

        assert_eq!(output.results.len(), 1);
        let result = &output.results[0];
        assert_eq!(result.url, "https://example.com/result");
        assert!(result.title.chars().count() <= 512);
        assert!(result.content.chars().count() <= 2_000);
        assert!(result.full_text.as_deref().unwrap().chars().count() <= 256 * 1024);
        assert_eq!(result.relevance_score, None);
        assert_eq!(result.thumbnail, None);
        assert_eq!(result.favicon, None);
        assert!(result.published_date.as_deref().unwrap().chars().count() <= 128);
        assert_eq!(result.images.len(), 1);
        assert!(
            result.images[0]
                .description
                .as_deref()
                .unwrap()
                .chars()
                .count()
                <= 1_000
        );

        assert_eq!(output.images.len(), 1);
        assert!(output.answers[0].chars().count() <= 16 * 1024);
        assert!(output.suggestions[0].chars().count() <= 512);
        assert!(output.answers[0]
            .chars()
            .all(|character| !character.is_control()));
        assert!(output.suggestions[0]
            .chars()
            .all(|character| !character.is_control()));
        assert_eq!(
            output.reports[0].metadata["_a3s_normalization"]["changed"],
            Value::Bool(true)
        );
    }
}
