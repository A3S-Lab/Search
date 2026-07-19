use async_trait::async_trait;

use a3s_search::providers::{
    ProviderAuthentication, ProviderCapabilities, ProviderDescriptor, ProviderEngine,
    ProviderReadiness, ProviderReport, ProviderRequest, ProviderResponse, ProviderResult,
    SearchProvider,
};
use a3s_search::{Engine, Result, ResultType, SearchImage, SearchQuery, SearchUsage};

struct CustomProvider;

#[async_trait]
impl SearchProvider for CustomProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            "custom",
            "Custom Provider",
            "https://search.example.com/",
            ProviderCapabilities::new()
                .with_answers(true)
                .with_images(true)
                .with_full_text(true)
                .with_usage(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        ProviderReadiness::Ready {
            authentication: ProviderAuthentication::Authenticated,
        }
    }

    async fn search(&self, request: &ProviderRequest) -> Result<ProviderResponse> {
        assert_eq!(request.query, "extensible providers");
        assert_eq!(request.page, 1);

        Ok(ProviderResponse::new()
            .with_result(
                ProviderResult::new(
                    "https://example.com/result",
                    "External provider",
                    "Provider-neutral result",
                )
                .with_result_type(ResultType::News)
                .with_full_text("Complete source content")
                .with_relevance_score(0.9)
                .with_thumbnail("https://example.com/thumbnail.png")
                .with_published_date("2026-07-20")
                .with_favicon("https://example.com/favicon.ico")
                .with_image(SearchImage::new("https://example.com/result-image.png")),
            )
            .with_answer("Provider-neutral answer")
            .with_suggestion("provider extension API")
            .with_image(SearchImage::new("https://example.com/query-image.png"))
            .with_report(
                ProviderReport::new()
                    .with_request_id("custom-request-1")
                    .with_total_results(1)
                    .with_response_time_ms(12)
                    .with_usage(SearchUsage::new().with_credits(0.5))
                    .with_metadata("tier", "custom"),
            ))
    }
}

#[tokio::test]
async fn downstream_provider_implementation_uses_only_public_builders() {
    let engine = ProviderEngine::new(CustomProvider);
    let output = engine
        .search_output(&SearchQuery::new("extensible providers"))
        .await
        .unwrap();

    assert_eq!(engine.shortcut(), "custom");
    assert_eq!(output.results.len(), 1);
    assert_eq!(output.results[0].result_type, ResultType::News);
    assert_eq!(
        output.results[0].full_text.as_deref(),
        Some("Complete source content")
    );
    assert_eq!(output.answers, ["Provider-neutral answer"]);
    assert_eq!(output.suggestions, ["provider extension API"]);
    assert_eq!(output.images.len(), 1);
    assert_eq!(output.reports[0].provider.as_deref(), Some("custom"));
    assert_eq!(
        output.reports[0].request_id.as_deref(),
        Some("custom-request-1")
    );
    assert_eq!(output.reports[0].usage.as_ref().unwrap().credits, Some(0.5));
}
