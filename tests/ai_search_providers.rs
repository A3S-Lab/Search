mod support;

use a3s_search::providers::{
    A3sAuth, A3sConfig, A3sProvider, AliyunConfig, AliyunProvider, BochaConfig, BochaProvider,
    CredentialSource, DeepSeekConfig, DeepSeekProvider, DoubaoConfig, DoubaoProvider,
    FirecrawlCategory, FirecrawlConfig, FirecrawlProvider, FirecrawlSource, ProviderRequest,
    SearchProvider, TencentConfig, TencentProvider, TinyFishConfig, TinyFishDomainType,
    TinyFishProvider,
};
use a3s_search::TimeRange;
use serde_json::Value;
use support::provider_server::{MockResponse, MockServer};

fn request(query: &str) -> ProviderRequest {
    ProviderRequest::new(query)
}

#[tokio::test]
async fn tinyfish_sends_documented_query_and_maps_results() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "query": "rust",
            "results": [{
                "position": 1,
                "site_name": "rust-lang.org",
                "title": "Rust",
                "snippet": "A language",
                "url": "https://www.rust-lang.org/",
                "date": "2026-01-02",
                "thumbnail_url": "https://www.rust-lang.org/thumb.png"
            }],
            "total_results": 1,
            "page": 1,
            "request_id": "tf-1"
        }"#,
    )
    .with_header("x-request-id", "tf-header")]);
    let provider = TinyFishProvider::new(
        TinyFishConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("tf-secret"))
            .with_location("us")
            .unwrap()
            .with_domain_type(TinyFishDomainType::News)
            .with_include_thumbnail(true),
    )
    .unwrap();

    let response = provider
        .search(
            &request("rust async")
                .with_page(2)
                .with_language("en-US".to_string())
                .with_time_range(TimeRange::Day),
        )
        .await
        .unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].url, "https://www.rust-lang.org/");
    assert_eq!(
        response.results[0].thumbnail.as_deref(),
        Some("https://www.rust-lang.org/thumb.png")
    );
    assert_eq!(response.report.total_results, Some(1));
    assert_eq!(response.report.request_id.as_deref(), Some("tf-1"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].request_line.starts_with("GET /search?"));
    assert!(
        requests[0].request_line.contains("query=rust+async")
            || requests[0].request_line.contains("query=rust%20async")
    );
    assert!(requests[0].request_line.contains("location=US"));
    assert!(requests[0].request_line.contains("language=en"));
    assert!(requests[0].request_line.contains("domain_type=news"));
    assert!(requests[0].request_line.contains("page=1"));
    assert!(requests[0].request_line.contains("recency_minutes=1440"));
    assert!(requests[0].request_line.contains("include_thumbnail=true"));
    assert_eq!(requests[0].header("x-api-key"), Some("tf-secret"));
    assert!(requests[0].body.is_empty());
}

#[tokio::test]
async fn tinyfish_authentication_errors_do_not_leak_the_key() {
    let server = MockServer::start(vec![MockResponse::json(
        401,
        br#"{"error":{"code":"INVALID_API_KEY","message":"bad key tf-secret"},"request_id":"tf-err"}"#,
    )]);
    let provider = TinyFishProvider::new(
        TinyFishConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("tf-secret")),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(error.kind(), "provider_authentication");
    assert!(!error.to_string().contains("tf-secret"));
    assert!(error.to_string().contains("tf-err"));
}

#[tokio::test]
async fn bocha_posts_web_search_and_keeps_summary_text() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "code": 200,
            "log_id": "bocha-1",
            "data": {
                "webPages": {
                    "totalEstimatedMatches": 12,
                    "value": [{
                        "name": "Rust",
                        "url": "https://www.rust-lang.org/",
                        "snippet": "short",
                        "summary": "longer page summary",
                        "siteIcon": "https://www.rust-lang.org/favicon.ico",
                        "datePublished": "2026-02-01"
                    }]
                }
            }
        }"#,
    )]);
    let provider = BochaProvider::new(
        BochaConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("bocha-secret"))
            .with_max_results(8)
            .unwrap(),
    )
    .unwrap();

    let response = provider
        .search(&request("rust").with_time_range(TimeRange::Week))
        .await
        .unwrap();

    assert_eq!(
        response.results[0].full_text.as_deref(),
        Some("longer page summary")
    );
    assert_eq!(response.report.total_results, Some(12));
    assert_eq!(response.report.request_id.as_deref(), Some("bocha-1"));
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["query"], "rust");
    assert_eq!(body["freshness"], "oneWeek");
    assert_eq!(body["summary"], true);
    assert_eq!(body["count"], 8);
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some("Bearer bocha-secret")
    );
}

#[tokio::test]
async fn aliyun_maps_ranked_pages_scene_answers_and_usage() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "requestId": "iqs-1",
            "pageItems": [{
                "title": "Rust <em>async</em>",
                "link": "https://docs.rs/tokio",
                "snippet": "runtime &amp; tasks",
                "publishedTime": "2026-03-01T00:00:00+08:00",
                "rerankScore": 0.91,
                "hostLogo": "https://docs.rs/favicon.ico",
                "images": ["https://docs.rs/logo.png"]
            }],
            "sceneItems": [{"detail": "{\"title\":\"Tokio\"}"}],
            "searchInformation": {"searchTime": 321},
            "costCredits": {"search": {"liteAdvancedTextSearch": 1}, "valueAdded": {"summary": 0}}
        }"#,
    )]);
    let provider = AliyunProvider::new(
        AliyunConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("iqs-secret"))
            .with_max_results(5)
            .unwrap(),
    )
    .unwrap();

    let response = provider
        .search(&request("tokio").with_time_range(TimeRange::Month))
        .await
        .unwrap();

    assert_eq!(response.results[0].title, "Rust async");
    assert_eq!(response.results[0].snippet, "runtime & tasks");
    assert_eq!(response.results[0].relevance_score, Some(0.91));
    assert_eq!(response.answers, vec!["Tokio".to_string()]);
    assert_eq!(response.report.response_time_ms, Some(321));
    assert_eq!(
        response.report.usage.and_then(|usage| usage.credits),
        Some(1.0)
    );
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["engineType"], "LiteAdvanced");
    assert_eq!(body["timeRange"], "OneMonth");
    assert_eq!(body["advancedParams"]["numResults"], "5");
    assert_eq!(body["contents"]["rerankScore"], true);
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some("Bearer iqs-secret")
    );
}

#[tokio::test]
async fn tencent_parses_json_string_pages_and_classifies_errors() {
    let server = MockServer::start(vec![
        MockResponse::json(
            200,
            br#"{
                "Response": {
                    "Query": "rust",
                    "RequestId": "wsa-1",
                    "Version": "premium",
                    "Pages": ["{\"title\":\"Rust\",\"url\":\"https://www.rust-lang.org/\",\"passage\":\"language\",\"score\":0.8,\"date\":\"2024-06-07 19:00:51\",\"images\":[\"https://www.rust-lang.org/logo.png\"]}"]
                }
            }"#,
        ),
        MockResponse::json(
            200,
            br#"{"Response":{"Error":{"Code":"UnauthorizedOperation","Message":"bad key wsa-secret"},"RequestId":"wsa-err"}}"#,
        ),
    ]);
    let provider = TencentProvider::new(
        TencentConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("wsa-secret")),
    )
    .unwrap();

    let response = provider
        .search(&request("rust").with_time_range(TimeRange::Year))
        .await
        .unwrap();
    assert_eq!(response.results[0].url, "https://www.rust-lang.org/");
    assert_eq!(response.results[0].relevance_score, Some(0.8));
    assert_eq!(response.results[0].images.len(), 1);
    assert_eq!(response.report.request_id.as_deref(), Some("wsa-1"));
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["Query"], "rust");
    assert_eq!(body["Mode"], 0);
    assert!(body.get("Cnt").is_none());
    assert!(body["FromTime"].as_u64().is_some());

    let error = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(error.kind(), "provider_authentication");
    assert!(!error.to_string().contains("wsa-secret"));
    assert!(error.to_string().contains("wsa-err"));
}

#[tokio::test]
async fn firecrawl_posts_web_search_and_maps_usage() {
    let server = MockServer::start(vec![
        MockResponse::json(
            200,
            br#"{
                "success": true,
                "id": "fc-1",
                "creditsUsed": 2,
                "warning": "research category changes later",
                "data": {
                    "web": [{
                        "url": "https://www.rust-lang.org/",
                        "title": "Rust",
                        "description": "A language",
                        "category": "research"
                    }],
                    "news": [{
                        "url": "https://blog.rust-lang.org/news",
                        "title": "Rust news",
                        "snippet": "release",
                        "date": "2 days ago"
                    }]
                }
            }"#,
        ),
        MockResponse::json(
            401,
            br#"{"success":false,"error":"bad key fc-secret","code":"UNAUTHORIZED"}"#,
        ),
    ]);
    let provider = FirecrawlProvider::new(
        FirecrawlConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("fc-secret"))
            .with_max_results(5)
            .unwrap()
            .with_country("de")
            .unwrap()
            .with_categories(vec![FirecrawlCategory::Research])
            .unwrap()
            .with_sources(vec![FirecrawlSource::Web, FirecrawlSource::News])
            .unwrap(),
    )
    .unwrap();

    let response = provider
        .search(&request("rust").with_time_range(TimeRange::Month))
        .await
        .unwrap();
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.results[0].url, "https://www.rust-lang.org/");
    assert_eq!(
        response.results[1].result_type,
        a3s_search::ResultType::News
    );
    assert_eq!(response.report.request_id.as_deref(), Some("fc-1"));
    assert_eq!(
        response
            .report
            .usage
            .as_ref()
            .and_then(|usage| usage.credits),
        Some(2.0)
    );
    assert_eq!(
        response
            .report
            .metadata
            .get("warning")
            .and_then(|value| value.as_str()),
        Some("research category changes later")
    );
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["query"], "rust");
    assert_eq!(body["limit"], 5);
    assert_eq!(body["tbs"], "qdr:m");
    assert_eq!(body["country"], "DE");
    assert_eq!(body["categories"][0], "research");
    assert_eq!(body["sources"][0], "web");
    assert_eq!(body["sources"][1], "news");
    assert!(body.get("scrapeOptions").is_none());
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some("Bearer fc-secret")
    );

    let error = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(error.kind(), "provider_authentication");
    assert!(!error.to_string().contains("fc-secret"));
}

#[tokio::test]
async fn a3s_posts_web_search_and_redacts_the_key() {
    let server = MockServer::start(vec![
        MockResponse::json(
            200,
            br#"{
                "success": true,
                "data": {
                    "results": {
                        "web": [{
                            "url": "https://www.rust-lang.org/",
                            "title": "Rust",
                            "description": "A language mentioning a3s-secret",
                            "markdown": "body a3s-secret"
                        }]
                    }
                }
            }"#,
        ),
        MockResponse::json(
            200,
            br#"{"success":false,"error":"need credits a3s-secret","code":"INSUFFICIENT_CREDITS"}"#,
        ),
        MockResponse::json(
            401,
            br#"{"success":false,"error":"bad key a3s-secret","code":"UNAUTHORIZED"}"#,
        ),
    ]);
    let provider = A3sProvider::new(
        A3sConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("a3s-secret"))
            .with_auth(A3sAuth::ApiKey)
            .with_max_results(4)
            .unwrap()
            .with_include_markdown(true),
    )
    .unwrap();

    let response = provider.search(&request("rust")).await.unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].url, "https://www.rust-lang.org/");
    assert!(!response.results[0].snippet.contains("a3s-secret"));
    assert!(response.results[0]
        .full_text
        .as_deref()
        .is_some_and(|text| !text.contains("a3s-secret")));
    assert!(response.report.request_id.is_none());
    assert!(response.report.usage.is_none());
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["query"], "rust");
    assert_eq!(body["limit"], 4);
    assert_eq!(body["sources"][0], "web");
    assert_eq!(body["scrapeOptions"]["formats"][0]["type"], "markdown");
    assert!(body.get("categories").is_none());
    assert!(body.get("country").is_none());
    assert!(body.get("tbs").is_none());
    assert_eq!(server.requests()[0].header("x-api-key"), Some("a3s-secret"));
    assert!(server.requests()[0].header("authorization").is_none());

    let quota = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(quota.kind(), "provider_quota");
    assert!(!quota.to_string().contains("a3s-secret"));

    let error = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(error.kind(), "provider_authentication");
    assert!(!error.to_string().contains("a3s-secret"));
}

#[tokio::test]
async fn doubao_posts_web_search_maps_cards_and_classifies_200_errors() {
    let server = MockServer::start(vec![
        MockResponse::json(
            200,
            br##"{
                "ResponseMetadata": {"RequestId": "db-1"},
                "Result": {
                    "ResultCount": 2,
                    "TimeCost": 87,
                    "WebResults": [
                        {
                            "Title": "# Weather",
                            "Url": "",
                            "Summary": "# Hangzhou weather\n\n## Today\n- 25C sunny doubao-secret"
                        },
                        {
                            "Title": "Rust <em>1.90</em>",
                            "Url": "https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/",
                            "Snippet": "Announcing doubao-secret",
                            "Summary": "Rust 1.90 ships workspace publishing",
                            "Content": "full page",
                            "PublishTime": "2025-09-18T00:00:00+08:00",
                            "LogoUrl": "https://blog.rust-lang.org/favicon.ico"
                        }
                    ]
                }
            }"##,
        ),
        MockResponse::json(
            200,
            br#"{"ResponseMetadata":{"RequestId":"db-2","Error":{"CodeN":700901,"Code":"invalid_api_key","Message":"invalid api key doubao-secret"}},"Result":null}"#,
        ),
        MockResponse::json(
            200,
            br#"{"ResponseMetadata":{"Error":{"CodeN":10406,"Code":"10406","Message":"free quota exhausted"}},"Result":null}"#,
        ),
        MockResponse::json(429, br#"{"ResponseMetadata":{"Error":{"Code":"FlowLimitExceeded"}}}"#),
    ]);
    let provider = DoubaoProvider::new(
        DoubaoConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("doubao-secret"))
            .with_max_results(5)
            .unwrap()
            .with_authoritative_only(true)
            .with_query_rewrite(true),
    )
    .unwrap();

    let response = provider
        .search(&request("rust").with_time_range(TimeRange::Week))
        .await
        .unwrap();
    assert_eq!(response.results.len(), 1);
    let hit = &response.results[0];
    assert_eq!(
        hit.url,
        "https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/"
    );
    assert_eq!(hit.title, "Rust 1.90");
    assert!(!hit.snippet.contains("doubao-secret"));
    assert_eq!(
        hit.full_text.as_deref(),
        Some("Rust 1.90 ships workspace publishing")
    );
    assert_eq!(
        hit.published_date.as_deref(),
        Some("2025-09-18T00:00:00+08:00")
    );
    assert_eq!(
        response.answers,
        ["Hangzhou weather Today 25C sunny [REDACTED]"]
    );
    assert_eq!(response.report.request_id.as_deref(), Some("db-1"));
    assert_eq!(response.report.total_results, Some(2));
    assert_eq!(response.report.response_time_ms, Some(87));

    let captured = &server.requests()[0];
    assert_eq!(
        captured.header("authorization"),
        Some("Bearer doubao-secret")
    );
    let body: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(body["Query"], "rust");
    assert_eq!(body["SearchType"], "web");
    assert_eq!(body["Count"], 5);
    assert_eq!(body["TimeRange"], "OneWeek");
    assert_eq!(body["Filter"]["AuthInfoLevel"], 1);
    assert_eq!(body["QueryControl"]["QueryRewrite"], true);

    let auth = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(auth.kind(), "provider_authentication");
    assert!(!auth.to_string().contains("doubao-secret"));

    let quota = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(quota.kind(), "provider_quota");

    let limited = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(limited.kind(), "provider_rate_limited");
}

#[tokio::test]
async fn deepseek_maps_summaries_and_keeps_filters_inside_the_query_bound() {
    let server = MockServer::start(vec![
        MockResponse::json(
            200,
            br##"{
                "results": [
                    {
                        "title": "Rust 1.90",
                        "url": "https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/",
                        "snippet": "Announcing deepseek-secret",
                        "summary": "Rust 1.90 ships workspace publishing",
                        "content": "",
                        "site_icon": "https://blog.rust-lang.org/favicon.ico",
                        "date": "2025-09-18T00:00:00+00:00"
                    },
                    {"title": "Dropped", "url": "", "snippet": "no url"}
                ]
            }"##,
        ),
        MockResponse::json(
            402,
            br#"{"error":{"message":"insufficient balance deepseek-secret","type":"insufficient_quota","code":"insufficient_quota"}}"#,
        ),
        MockResponse::json(
            429,
            br#"{"error":{"message":"slow down","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#,
        ),
    ]);
    let provider = DeepSeekProvider::new(
        DeepSeekConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("deepseek-secret"))
            .with_max_results(5)
            .unwrap(),
    )
    .unwrap();
    assert!(provider.descriptor().capabilities.full_text);
    assert!(provider.descriptor().capabilities.time_range);

    let response = provider
        .search(
            &request("rust")
                .with_language("zh-CN")
                .with_time_range(TimeRange::Week),
        )
        .await
        .unwrap();
    assert_eq!(response.results.len(), 1);
    let hit = &response.results[0];
    assert_eq!(hit.title, "Rust 1.90");
    assert!(!hit.snippet.contains("deepseek-secret"));
    assert_eq!(
        hit.full_text.as_deref(),
        Some("Rust 1.90 ships workspace publishing")
    );
    assert_eq!(
        hit.published_date.as_deref(),
        Some("2025-09-18T00:00:00+00:00")
    );
    assert_eq!(
        hit.favicon.as_deref(),
        Some("https://blog.rust-lang.org/favicon.ico")
    );

    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["count"], 5);
    assert_eq!(body["need_content"], false);
    let query = body["query"].as_str().unwrap();
    assert!(query.starts_with("rust lang:zh after:"), "{query}");
    assert!(query.chars().count() <= 128, "{query}");
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some("Bearer deepseek-secret")
    );

    let quota = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(quota.kind(), "provider_quota");
    assert!(!quota.to_string().contains("deepseek-secret"));

    let limited = provider.search(&request("rust")).await.unwrap_err();
    assert_eq!(limited.kind(), "provider_rate_limited");

    let too_long = "查".repeat(129);
    let error = provider.search(&request(&too_long)).await.unwrap_err();
    assert_eq!(error.kind(), "provider_invalid_request");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn deepseek_page_body_is_opt_in_and_a_tight_query_drops_filters() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{"results":[{"title":"Page","url":"https://example.com/a","snippet":"short","summary":"model text","content":"full page"}]}"#,
    )]);
    let provider = DeepSeekProvider::new(
        DeepSeekConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("deepseek-secret"))
            .with_need_content(true),
    )
    .unwrap();
    // 122 + " lang:zh" exceeds the 128-character query bound, so the filter is omitted.
    let tight = "a".repeat(122);
    let response = provider
        .search(&request(&tight).with_language("zh-CN"))
        .await
        .unwrap();
    assert_eq!(response.results[0].full_text.as_deref(), Some("full page"));
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["query"], tight);
    assert_eq!(body["need_content"], true);
}

#[tokio::test]
async fn doubao_default_request_omits_optional_filters() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{"ResponseMetadata":{},"Result":{"WebResults":null}}"#,
    )]);
    let provider = DoubaoProvider::new(
        DoubaoConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("doubao-secret")),
    )
    .unwrap();

    let response = provider.search(&request("rust")).await.unwrap();
    assert!(response.results.is_empty());
    let body: Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["Count"], 10);
    for absent in ["TimeRange", "Filter", "QueryControl"] {
        assert!(body.get(absent).is_none(), "{absent} must be omitted");
    }
}
