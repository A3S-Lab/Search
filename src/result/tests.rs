use super::*;

#[test]
fn test_result_type_default() {
    let default: ResultType = Default::default();
    assert_eq!(default, ResultType::Web);
}

#[test]
fn test_result_type_variants() {
    let types = vec![
        ResultType::Web,
        ResultType::Image,
        ResultType::Video,
        ResultType::News,
        ResultType::Map,
        ResultType::File,
        ResultType::Answer,
        ResultType::Infobox,
        ResultType::Suggestion,
    ];
    assert_eq!(types.len(), 9);
}

#[test]
fn test_search_result_new() {
    let result = SearchResult::new("https://example.com", "Title", "Content");
    assert_eq!(result.url, "https://example.com");
    assert_eq!(result.title, "Title");
    assert_eq!(result.content, "Content");
    assert_eq!(result.result_type, ResultType::Web);
    assert!(result.engines.is_empty());
    assert!(result.positions.is_empty());
    assert_eq!(result.score, 0.0);
    assert!(result.relevance_score.is_none());
    assert!(result.thumbnail.is_none());
    assert!(result.published_date.is_none());
    assert!(result.favicon.is_none());
    assert!(result.images.is_empty());
}

#[test]
fn test_search_result_with_type() {
    let result = SearchResult::new("url", "title", "content").with_type(ResultType::Image);
    assert_eq!(result.result_type, ResultType::Image);
}

#[test]
fn test_search_result_with_engine() {
    let result = SearchResult::new("url", "title", "content")
        .with_engine("google", 1)
        .with_engine("bing", 3);
    assert!(result.engines.contains("google"));
    assert!(result.engines.contains("bing"));
    assert_eq!(result.positions, vec![1, 3]);
}

#[test]
fn test_search_result_with_thumbnail() {
    let result = SearchResult::new("url", "title", "content")
        .with_thumbnail("https://example.com/thumb.jpg");
    assert_eq!(
        result.thumbnail,
        Some("https://example.com/thumb.jpg".to_string())
    );
}

#[test]
fn test_search_result_with_published_date() {
    let result = SearchResult::new("url", "title", "content").with_published_date("2024-01-15");
    assert_eq!(result.published_date, Some("2024-01-15".to_string()));
}

#[test]
fn test_search_result_with_relevance_score() {
    let result = SearchResult::new("url", "title", "content").with_relevance_score(0.82);
    assert_eq!(result.relevance_score, Some(0.82));
}

#[test]
fn test_normalized_url_https() {
    let result = SearchResult::new("https://Example.COM/Path/", "t", "c");
    assert_eq!(result.normalized_url(), "example.com/Path");
}

#[test]
fn test_normalized_url_http() {
    let result = SearchResult::new("http://Example.COM/Path/", "t", "c");
    assert_eq!(result.normalized_url(), "example.com/Path");
}

#[test]
fn test_normalized_url_no_scheme() {
    let result = SearchResult::new("example.com/path", "t", "c");
    assert_eq!(result.normalized_url(), "example.com/path");
}

#[test]
fn test_normalized_url_trailing_slash() {
    let result = SearchResult::new("https://example.com/", "t", "c");
    assert_eq!(result.normalized_url(), "example.com");
}

#[test]
fn test_normalized_url_removes_tracking_and_fragment() {
    let result = SearchResult::new(
        "https://www.Example.com/Path/?utm_source=newsletter&b=2&a=1#section",
        "t",
        "c",
    );
    assert_eq!(result.normalized_url(), "example.com/Path?a=1&b=2");
}

#[test]
fn test_normalized_url_sorts_query_pairs() {
    let first = SearchResult::new("https://example.com/path?b=2&a=1", "t", "c");
    let second = SearchResult::new("https://example.com/path?a=1&b=2", "t", "c");

    assert_eq!(first.normalized_url(), second.normalized_url());
}

#[test]
fn test_normalized_url_keeps_non_default_port() {
    let result = SearchResult::new("https://example.com:8443/path/", "t", "c");
    assert_eq!(result.normalized_url(), "example.com:8443/path");
}

#[test]
fn test_normalized_url_preserves_case_sensitive_path_and_query_values() {
    let upper = SearchResult::new("https://example.com/Docs?q=Rust", "t", "c");
    let lower = SearchResult::new("https://example.com/docs?q=rust", "t", "c");

    assert_ne!(upper.normalized_url(), lower.normalized_url());
}

#[test]
fn test_normalized_url_removes_default_port() {
    let explicit = SearchResult::new("https://example.com:443/path", "t", "c");
    let implicit = SearchResult::new("https://example.com/path", "t", "c");

    assert_eq!(explicit.normalized_url(), implicit.normalized_url());
}

#[test]
fn test_search_results_new() {
    let results = SearchResults::new();
    assert_eq!(results.count, 0);
    assert_eq!(results.duration_ms, 0);
    assert!(results.items().is_empty());
    assert!(results.suggestions().is_empty());
    assert!(results.answers().is_empty());
    assert!(results.images().is_empty());
    assert!(results.reports().is_empty());
}

#[test]
fn test_search_results_add_result() {
    let mut results = SearchResults::new();
    results.add_result(SearchResult::new("url1", "title1", "content1"));
    results.add_result(SearchResult::new("url2", "title2", "content2"));
    assert_eq!(results.count, 2);
    assert_eq!(results.items().len(), 2);
}

#[test]
fn test_search_results_add_suggestion() {
    let mut results = SearchResults::new();
    results.add_suggestion("suggestion1");
    results.add_suggestion("suggestion2");
    results.add_suggestion("suggestion1");
    assert_eq!(results.suggestions().len(), 2);
    assert_eq!(results.suggestions()[0], "suggestion1");
}

#[test]
fn test_search_results_add_answer() {
    let mut results = SearchResults::new();
    results.add_answer("42");
    results.add_answer("42");
    assert_eq!(results.answers().len(), 1);
    assert_eq!(results.answers()[0], "42");
}

#[test]
fn test_search_results_merge_duplicate_images_deterministically() {
    let mut results = SearchResults::new();
    results.add_image(SearchImage::new("https://example.com/image.png").with_description("short"));
    results.add_image(
        SearchImage::new("https://example.com/image.png")
            .with_description("a richer image description"),
    );
    results.add_image(SearchImage::new("https://a.example/image.png"));

    assert_eq!(results.images().len(), 2);
    assert_eq!(results.images()[0].url, "https://a.example/image.png");
    assert_eq!(
        results.images()[1].description.as_deref(),
        Some("a richer image description")
    );
}

#[test]
fn test_search_results_items_mut() {
    let mut results = SearchResults::new();
    results.add_result(SearchResult::new("url", "title", "content"));
    results.items_mut()[0].score = 5.0;
    assert_eq!(results.items()[0].score, 5.0);
}

#[test]
fn test_search_results_set_duration() {
    let mut results = SearchResults::new();
    results.set_duration(150);
    assert_eq!(results.duration_ms, 150);
}

#[test]
fn test_search_result_serialization() {
    let result = SearchResult::new("https://example.com", "Title", "Content")
        .with_engine("zeta", 1)
        .with_engine("alpha", 2);
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"url\":\"https://example.com\""));
    assert!(json.contains("\"title\":\"Title\""));
    assert!(json.contains("\"engines\":[\"alpha\",\"zeta\"]"));
}

#[test]
fn test_search_results_serialization() {
    let mut results = SearchResults::new();
    results.add_result(SearchResult::new("url", "title", "content"));
    results.set_duration(100);
    let json = serde_json::to_string(&results).unwrap();
    assert!(json.contains("\"duration_ms\":100"));
}

#[test]
fn test_result_type_serialization() {
    let result = SearchResult::new("url", "title", "content").with_type(ResultType::Image);
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"result_type\":\"image\""));
}

#[test]
fn test_search_results_errors_empty() {
    let results = SearchResults::new();
    assert!(results.errors().is_empty());
}

#[test]
fn test_search_results_add_error() {
    let mut results = SearchResults::new();
    results.add_error("Google", "CAPTCHA detected");
    assert_eq!(results.errors().len(), 1);
    assert_eq!(results.errors()[0].0, "Google");
    assert_eq!(results.errors()[0].1, "CAPTCHA detected");
    assert_eq!(results.failures().len(), 1);
    assert_eq!(results.failures()[0].kind, "unknown");
}

#[test]
fn engine_failure_from_search_error_preserves_provider_and_retry_context() {
    let error = SearchError::from(
        crate::ProviderError::new("tavily", crate::ProviderErrorKind::RateLimited, "slow down")
            .with_retry_after(30),
    );
    let failure = EngineFailure::from_search_error("Tavily", &error);

    assert_eq!(failure.engine, "Tavily");
    assert_eq!(failure.kind, "provider_rate_limited");
    assert_eq!(failure.provider.as_deref(), Some("tavily"));
    assert!(failure.transient);
    assert_eq!(failure.retry_after_seconds, Some(30));
}

#[test]
fn test_search_results_add_structured_failure_preserves_legacy_error_view() {
    let failure = EngineFailure::new(
        "AnySearch",
        "provider_quota",
        "AnySearch quota is exhausted",
    )
    .with_provider("anysearch")
    .with_transient(false);
    let mut results = SearchResults::new();
    results.add_failure(failure.clone());

    assert_eq!(results.failures(), &[failure]);
    assert_eq!(
        results.errors(),
        &[(
            "AnySearch".to_string(),
            "AnySearch quota is exhausted".to_string()
        )]
    );
}

#[test]
fn test_search_results_add_structured_report() {
    let report = SearchReport::new("Tavily")
        .with_provider("tavily")
        .with_request_id("req-123")
        .with_total_results(42)
        .with_response_time_ms(125)
        .with_usage(SearchUsage::new().with_credits(2.0))
        .with_metadata("search_depth", "advanced");
    let mut results = SearchResults::new();
    results.add_report(report.clone());

    assert_eq!(results.reports(), &[report]);
    let json = serde_json::to_value(&results).unwrap();
    assert_eq!(json["reports"][0]["provider"], "tavily");
    assert_eq!(json["reports"][0]["usage"]["credits"], 2.0);
    assert_eq!(json["reports"][0]["metadata"]["search_depth"], "advanced");
}

#[test]
fn test_search_results_multiple_errors() {
    let mut results = SearchResults::new();
    results.add_error("Google", "CAPTCHA detected");
    results.add_error("Baidu", "timed out");
    assert_eq!(results.errors().len(), 2);
    assert_eq!(results.errors()[1].0, "Baidu");
}

#[test]
fn test_search_results_errors_with_results() {
    let mut results = SearchResults::new();
    results.add_result(SearchResult::new("url", "title", "content"));
    results.add_error("Google", "failed");
    assert_eq!(results.count, 1);
    assert_eq!(results.errors().len(), 1);
}

#[test]
fn test_search_result_deserialize_without_full_text() {
    // Older persisted JSON lacking `full_text` must still load thanks to #[serde(default)].
    let json = r#"{
            "url": "https://example.com",
            "title": "T",
            "content": "snippet",
            "result_type": "web",
            "engines": [],
            "positions": [],
            "score": 1.0,
            "thumbnail": null,
            "published_date": null
        }"#;
    let r: SearchResult = serde_json::from_str(json).unwrap();
    assert!(r.full_text.is_none());
    assert_eq!(r.url, "https://example.com");
}

#[test]
fn test_search_result_full_text_roundtrip() {
    let mut r = SearchResult::new("https://example.com", "T", "snippet");
    r.full_text = Some("article body".to_string());
    let json = serde_json::to_string(&r).unwrap();
    let back: SearchResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.full_text.as_deref(), Some("article body"));
}
