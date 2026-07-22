mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use a3s_search::providers::{
    AnySearchConfig, AnySearchDomain, AnySearchProvider, AnySearchSubDomain, CredentialSource,
    ProviderAuthentication, ProviderHttpConfig, ProviderReadiness, ProviderRequest, SearchProvider,
    TavilyAnswer, TavilyConfig, TavilyCountry, TavilyDate, TavilyProvider, TavilyRawContent,
    TavilySearchDepth, TavilyTopic,
};
use a3s_search::{SafeSearch, SearchError, TimeRange};
use serde_json::{json, Value};
use support::provider_server::{MockResponse, MockServer};

fn request(query: &str) -> ProviderRequest {
    ProviderRequest::new(query)
}

#[tokio::test]
async fn anysearch_anonymous_mcp_request_and_response_match_contract() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "_meta": {"request_id": "any-req-1"},
                "content": [{
                    "type": "text",
                    "text": "## Search Results (1 results, 42ms)\n\n### 1. Rust\n- **URL**: https://www.rust-lang.org/\n- Full AnySearch content\n"
                }]
            }
        }"###,
    )]);
    let mut sub_domain_params = BTreeMap::new();
    sub_domain_params.insert("library".to_string(), json!("tokio"));
    let config = AnySearchConfig::new()
        .unwrap()
        .with_endpoint(server.endpoint.clone())
        .unwrap()
        .with_api_key(CredentialSource::none())
        .with_max_results(7)
        .unwrap()
        .with_domain(AnySearchDomain::Code)
        .with_sub_domain(AnySearchSubDomain::new("code.doc").unwrap())
        .with_sub_domain_params(sub_domain_params);
    let provider = AnySearchProvider::new(config).unwrap();

    let response = provider.search(&request("rust async")).await.unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(
        response.results[0].full_text.as_deref(),
        Some("Full AnySearch content")
    );
    assert_eq!(response.report.request_id.as_deref(), Some("any-req-1"));
    assert_eq!(response.report.total_results, Some(1));
    assert_eq!(response.report.response_time_ms, Some(42));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].request_line, "POST /search HTTP/1.1");
    assert!(requests[0].header("authorization").is_none());
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    assert!(requests[0]
        .header("x-anysearch-client")
        .is_some_and(|value| value.starts_with("a3s-search/")));
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["method"], "tools/call");
    assert_eq!(body["params"]["name"], "search");
    assert_eq!(body["params"]["arguments"]["query"], "rust async");
    assert_eq!(body["params"]["arguments"]["max_results"], 7);
    assert_eq!(body["params"]["arguments"]["domain"], "code");
    assert_eq!(body["params"]["arguments"]["sub_domain"], "code.doc");
    assert_eq!(
        body["params"]["arguments"]["sub_domain_params"]["library"],
        "tokio"
    );
}

#[tokio::test]
async fn anysearch_embedded_anonymous_quota_failure_is_classified_without_leaking_credentials() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{
                    "type": "text",
                    "text": "daily_free_quota_exhausted\nThe free quota is exhausted. Configure the newly issued API key.\n\nAPI Key: as_sk_must-never-be-exposed\nUsername: must-never-be-exposed\nPassword: must-never-be-exposed"
                }]
            }
        }"###,
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();
    let rendered = error.to_string();

    assert_eq!(error.kind(), "provider_quota");
    assert!(rendered.contains("AnySearch quota is exhausted"));
    assert!(!rendered.contains("as_sk_"));
    assert!(!rendered.contains("must-never-be-exposed"));
    assert!(!rendered.contains("search-results header"));
}

#[tokio::test]
async fn anysearch_structured_auto_registration_is_not_mistaken_for_empty_search_content() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "structuredContent": {
                    "error": "quota_exhausted",
                    "auto_registered": {
                        "api_key": "as_sk_must-never-be-exposed",
                        "username": "must-never-be-exposed",
                        "password": "must-never-be-exposed"
                    }
                },
                "content": []
            }
        }"###,
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();
    let rendered = error.to_string();

    assert_eq!(error.kind(), "provider_quota");
    assert!(rendered.contains("AnySearch quota is exhausted"));
    assert!(!rendered.contains("as_sk_"));
    assert!(!rendered.contains("must-never-be-exposed"));
    assert!(!rendered.contains("searchable content"));
}

#[tokio::test]
async fn anysearch_default_request_omits_vertical_routing() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "_meta": {"request_id": "any-general-1"},
                "content": [{"type": "text", "text": "## Search Results (0 results, 8ms)\n"}]
            }
        }"###,
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    provider.search(&request("rust")).await.unwrap();

    let requests = server.requests();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let arguments = body["params"]["arguments"].as_object().unwrap();
    assert!(!arguments.contains_key("domain"));
    assert!(!arguments.contains_key("sub_domain"));
    assert!(!arguments.contains_key("sub_domain_params"));
    assert!(!arguments.contains_key("zone"));
}

#[tokio::test]
async fn anysearch_prefers_structured_content_when_available() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "_meta": {"request_id": "any-structured-1"},
                "structuredContent": {
                    "results": [{
                        "title": "Structured Rust",
                        "url": "https://www.rust-lang.org/",
                        "snippet": "Structured snippet",
                        "full_text": "Structured full content",
                        "score": 0.88
                    }],
                    "total_results": 9,
                    "response_time_ms": 17
                },
                "content": [{
                    "type": "text",
                    "text": "## Search Results (0 results, 999ms)\n"
                }]
            }
        }"###,
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    let response = provider.search(&request("rust")).await.unwrap();

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].title, "Structured Rust");
    assert_eq!(response.results[0].relevance_score, Some(0.88));
    assert_eq!(response.report.total_results, Some(9));
    assert_eq!(response.report.response_time_ms, Some(17));
}

#[tokio::test]
async fn providers_enforce_configured_result_limits_on_success_responses() {
    let anysearch_server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "structuredContent": {
                    "results": [
                        {"title": "First", "url": "https://first.example/"},
                        {"title": "Second", "url": "https://second.example/"}
                    ]
                },
                "content": []
            }
        }"###,
    )]);
    let anysearch = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(anysearch_server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none())
            .with_max_results(1)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        anysearch
            .search(&request("rust"))
            .await
            .unwrap()
            .results
            .len(),
        1
    );

    let tavily_server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "results": [
                {"title": "First", "url": "https://first.example/"},
                {"title": "Second", "url": "https://second.example/"}
            ]
        }"#,
    )]);
    let tavily = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(tavily_server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none())
            .with_max_results(1)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        tavily.search(&request("rust")).await.unwrap().results.len(),
        1
    );
}

#[tokio::test]
async fn anysearch_increments_json_rpc_request_ids() {
    let responses = [1, 2]
        .into_iter()
        .map(|id| {
            MockResponse::json(
                200,
                format!(
                    r###"{{"jsonrpc":"2.0","id":{id},"result":{{"_meta":{{"request_id":"any-{id}"}},"content":[{{"type":"text","text":"## Search Results (0 results, 1ms)\n"}}]}}}}"###
                ),
            )
        })
        .collect();
    let server = MockServer::start(responses);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    provider.search(&request("first")).await.unwrap();
    provider.search(&request("second")).await.unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(first["id"], 1);
    assert_eq!(second["id"], 2);
}

#[tokio::test]
async fn anysearch_tool_errors_are_sanitized_without_parsing_secret_data() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "_meta": {"request_id": "any-tool-error-1"},
                "isError": true,
                "content": [{
                    "type": "text",
                    "text": "invalid search arguments\nAPI Key: as_sk_response-secret"
                }],
                "structuredContent": {
                    "api_key": "must-never-be-exposed"
                }
            }
        }"#,
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();

    assert_eq!(error.kind(), "provider_invalid_request");
    assert!(error.to_string().contains("any-tool-error-1"));
    assert!(error.to_string().contains("AnySearch rejected the request"));
    assert!(!error.to_string().contains("as_sk_response-secret"));
    assert!(!error.to_string().contains("must-never-be-exposed"));
}

#[tokio::test]
async fn anysearch_configured_invalid_key_never_falls_back_to_anonymous() {
    let server = MockServer::start(vec![MockResponse::json(
        401,
        br#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32001,
                "message": "invalid API key",
                "data": {"api_key": "must-never-be-exposed"}
            }
        }"#,
    )
    .with_header("X-Request-ID", "any-auth-1")]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("invalid-secret")),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();

    assert_eq!(error.kind(), "provider_authentication");
    assert!(error.to_string().contains("any-auth-1"));
    assert!(!error.to_string().contains("invalid-secret"));
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("authorization"),
        Some("Bearer invalid-secret")
    );
}

#[tokio::test]
async fn anysearch_negative_rpc_error_codes_are_preserved_and_sanitized() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32602,
                "message": "invalid search arguments",
                "data": {"api_key": "must-never-be-exposed"}
            }
        }"#,
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();

    assert_eq!(error.kind(), "provider_invalid_request");
    let SearchError::Provider(provider_error) = &error else {
        panic!("expected provider error");
    };
    assert_eq!(provider_error.application_code(), Some(-32602));
    assert!(!error.to_string().contains("must-never-be-exposed"));
}

#[tokio::test]
async fn provider_errors_redact_configured_secrets_and_preserve_rpc_retry_context() {
    let anysearch_server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "rate limit reached for any-secret"
            }
        }"#,
    )
    .with_header("Retry-After", "17")]);
    let anysearch = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(anysearch_server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("any-secret")),
    )
    .unwrap();

    let anysearch_error = anysearch.search(&request("rust")).await.unwrap_err();
    assert_eq!(anysearch_error.kind(), "provider_rate_limited");
    assert!(anysearch_error.to_string().contains("retry after 17s"));
    assert!(!anysearch_error.to_string().contains("any-secret"));

    let tavily_server = MockServer::start(vec![MockResponse::json(
        401,
        br#"{
            "detail": "invalid credential tvly-secret",
            "request_id": "tvly-auth-1"
        }"#,
    )]);
    let tavily = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(tavily_server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("tvly-secret")),
    )
    .unwrap();

    let tavily_error = tavily.search(&request("rust")).await.unwrap_err();
    assert_eq!(tavily_error.kind(), "provider_authentication");
    assert!(!tavily_error.to_string().contains("tvly-secret"));
}

#[tokio::test]
async fn tavily_authenticated_request_rich_response_and_score_match_contract() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br##"{
            "query": "rust async",
            "answer": "Tokio is the dominant async runtime.",
            "images": [{
                "url": "https://images.example/tokio.png",
                "description": "Tokio logo"
            }],
            "results": [{
                "title": "Tokio",
                "url": "https://tokio.rs/",
                "content": "An asynchronous runtime for Rust.",
                "score": 0.91,
                "raw_content": "# Tokio\nFull Markdown",
                "published_date": "2026-07-01",
                "favicon": "https://tokio.rs/favicon.ico",
                "images": [{
                    "url": "https://tokio.rs/runtime.png",
                    "description": "Runtime diagram"
                }]
            }],
            "response_time": "1.25",
            "auto_parameters": {"topic": "general"},
            "usage": {"credits": 2},
            "request_id": "tvly-req-1"
        }"##,
    )]);
    let config = TavilyConfig::new()
        .unwrap()
        .with_endpoint(server.endpoint.clone())
        .unwrap()
        .with_api_key(CredentialSource::value("tvly-secret"))
        .with_project(CredentialSource::value("project-42"))
        .with_search_depth(TavilySearchDepth::Advanced)
        .with_chunks_per_source(2)
        .unwrap()
        .with_max_results(6)
        .unwrap()
        .with_topic(TavilyTopic::General)
        .with_answer(TavilyAnswer::Basic)
        .with_raw_content(TavilyRawContent::Markdown)
        .with_include_domains(["tokio.rs"])
        .unwrap()
        .with_exclude_domains(["example.com"])
        .unwrap()
        .with_auto_parameters(true)
        .with_exact_match(true)
        .with_include_usage(true)
        .with_start_date(TavilyDate::new("2026-01-01").unwrap())
        .with_end_date(TavilyDate::new("2026-07-19").unwrap())
        .with_country(TavilyCountry::new("united states").unwrap())
        .with_include_images(true)
        .with_image_descriptions(true)
        .with_favicon(true);
    let provider = TavilyProvider::new(config).unwrap();
    let mut provider_request = request("rust async");
    provider_request.language = Some("en".to_string());
    provider_request.time_range = Some(TimeRange::Week);

    let response = provider.search(&provider_request).await.unwrap();

    assert_eq!(
        response.answers,
        vec!["Tokio is the dominant async runtime."]
    );
    assert_eq!(response.results[0].relevance_score, Some(0.91));
    assert_eq!(
        response.results[0].full_text.as_deref(),
        Some("# Tokio\nFull Markdown")
    );
    assert_eq!(
        response.results[0].favicon.as_deref(),
        Some("https://tokio.rs/favicon.ico")
    );
    assert_eq!(response.results[0].images.len(), 1);
    assert_eq!(response.images.len(), 1);
    assert_eq!(
        response.images[0].description.as_deref(),
        Some("Tokio logo")
    );
    assert_eq!(response.report.response_time_ms, Some(1250));
    assert_eq!(
        response
            .report
            .usage
            .as_ref()
            .and_then(|usage| usage.credits),
        Some(2.0)
    );
    assert_eq!(
        response.report.metadata["auto_parameters"]["topic"],
        "general"
    );
    assert_eq!(response.report.metadata["access_mode"], "authenticated");
    assert_eq!(response.report.metadata["search_depth"], "advanced");
    assert_eq!(response.report.metadata["topic"], "general");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("authorization"),
        Some("Bearer tvly-secret")
    );
    assert!(requests[0].header("x-tavily-access-mode").is_none());
    assert_eq!(requests[0].header("x-project-id"), Some("project-42"));
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["query"], "rust async");
    assert_eq!(body["search_depth"], "advanced");
    assert_eq!(body["chunks_per_source"], 2);
    assert_eq!(body["max_results"], 6);
    assert_eq!(body["topic"], "general");
    assert_eq!(body["time_range"], "week");
    assert_eq!(body["include_answer"], "basic");
    assert_eq!(body["include_raw_content"], "markdown");
    assert_eq!(body["include_domains"], json!(["tokio.rs"]));
    assert_eq!(body["exclude_domains"], json!(["example.com"]));
    assert_eq!(body["auto_parameters"], true);
    assert_eq!(body["exact_match"], true);
    assert_eq!(body["include_usage"], true);
    assert_eq!(body["start_date"], "2026-01-01");
    assert_eq!(body["end_date"], "2026-07-19");
    assert_eq!(body["country"], "united states");
    assert_eq!(body["include_images"], true);
    assert_eq!(body["include_image_descriptions"], true);
    assert_eq!(body["include_favicon"], true);
}

#[tokio::test]
async fn tavily_auto_parameters_can_select_unconfigured_depth_and_topic() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "query": "latest rust news",
            "answer": null,
            "images": [],
            "results": [],
            "response_time": 0.1,
            "auto_parameters": {
                "search_depth": "advanced",
                "topic": "news"
            },
            "request_id": "tvly-auto-1"
        }"#,
    )]);
    let provider = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none())
            .with_auto_parameters(true),
    )
    .unwrap();

    let response = provider.search(&request("latest rust news")).await.unwrap();

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["auto_parameters"], true);
    assert!(
        body.get("search_depth").is_none(),
        "an implicit default would override Tavily auto-parameters"
    );
    assert!(
        body.get("topic").is_none(),
        "an implicit default would override Tavily auto-parameters"
    );
    assert_eq!(response.report.metadata["search_depth"], "advanced");
    assert_eq!(response.report.metadata["topic"], "news");
}

#[tokio::test]
async fn tavily_auto_parameters_do_not_invent_unreported_depth_or_topic() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "query": "latest rust news",
            "results": [],
            "response_time": 0.1,
            "request_id": "tvly-auto-missing-1"
        }"#,
    )]);
    let provider = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none())
            .with_auto_parameters(true),
    )
    .unwrap();

    let response = provider.search(&request("latest rust news")).await.unwrap();

    let requests = server.requests();
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("search_depth").is_none());
    assert!(body.get("topic").is_none());
    assert!(!response.report.metadata.contains_key("search_depth"));
    assert!(!response.report.metadata.contains_key("topic"));
}

#[tokio::test]
async fn tavily_auto_parameter_metadata_is_bounded_and_redacts_credentials() {
    const API_KEY: &str = "tvly-metadata-secret";
    const PROJECT: &str = "project-metadata-secret";

    let mut auto_parameters = serde_json::Map::new();
    auto_parameters.insert(
        format!("a_key_{API_KEY}"),
        Value::String(format!("credentials: {API_KEY} and {PROJECT}")),
    );
    auto_parameters.insert("b_long".to_string(), Value::String("x".repeat(2_000)));
    auto_parameters.insert(
        "c_items".to_string(),
        Value::Array((0..100).map(Value::from).collect()),
    );
    auto_parameters.insert(
        "d_nested".to_string(),
        json!({"one": {"two": {"three": {"four": {"too_deep": true}}}}}),
    );
    for index in 0..80 {
        auto_parameters.insert(format!("z_field_{index:03}"), Value::from(index));
    }
    let body = json!({
        "results": [],
        "auto_parameters": Value::Object(auto_parameters),
        "request_id": "tvly-metadata-1"
    });
    let server = MockServer::start(vec![MockResponse::json(
        200,
        serde_json::to_vec(&body).unwrap(),
    )]);
    let provider = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value(API_KEY))
            .with_project(CredentialSource::value(PROJECT))
            .with_auto_parameters(true),
    )
    .unwrap();

    let response = provider.search(&request("rust")).await.unwrap();
    let serialized = serde_json::to_string(&response.report.metadata).unwrap();
    let metadata = response.report.metadata["auto_parameters"]
        .as_object()
        .unwrap();

    assert!(!serialized.contains(API_KEY));
    assert!(!serialized.contains(PROJECT));
    assert!(metadata.len() <= 32);
    assert!(metadata["b_long"].as_str().unwrap().chars().count() <= 512);
    assert!(metadata["c_items"].as_array().unwrap().len() <= 32);
    assert!(!serialized.contains("too_deep"));
    assert_eq!(response.report.metadata["auto_parameters_truncated"], true);
}

#[tokio::test]
async fn tavily_keyless_request_is_anonymous_and_accepts_zero_results_limit() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "query": "rust",
            "answer": null,
            "images": [],
            "results": [],
            "response_time": "1.67",
            "request_id": "tvly-keyless-1"
        }"#,
    )]);
    let provider = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none())
            .with_project(CredentialSource::value("must-not-be-sent"))
            .with_max_results(0)
            .unwrap(),
    )
    .unwrap();

    assert_eq!(
        provider.readiness(),
        ProviderReadiness::Ready {
            authentication: ProviderAuthentication::Anonymous
        }
    );
    let response = provider.search(&request("rust")).await.unwrap();

    assert_eq!(response.report.response_time_ms, Some(1670));
    assert_eq!(response.report.metadata["access_mode"], "keyless");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].header("authorization").is_none());
    assert_eq!(requests[0].header("x-tavily-access-mode"), Some("keyless"));
    assert!(requests[0].header("x-project-id").is_none());
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["max_results"], 0);
    assert_eq!(body["include_usage"], false);
}

#[tokio::test]
async fn tavily_safe_search_rejects_keyless_and_unsupported_depths_before_network_access() {
    let keyless_server = MockServer::start(Vec::new());
    let keyless = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(keyless_server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();
    let error = keyless
        .search(&request("rust").with_safe_search(SafeSearch::Strict))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "provider_permission");
    assert!(keyless_server.requests().is_empty());

    let fast_server = MockServer::start(Vec::new());
    let fast = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(fast_server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("enterprise-key"))
            .with_search_depth(TavilySearchDepth::Fast),
    )
    .unwrap();
    let error = fast
        .search(&request("rust").with_safe_search(SafeSearch::Moderate))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "provider_invalid_request");
    assert!(fast_server.requests().is_empty());
}

#[tokio::test]
async fn provider_rate_limit_preserves_bounded_retry_context() {
    let server = MockServer::start(vec![MockResponse::json(
        429,
        br#"{"detail":"slow down","request_id":"tvly-rate-1"}"#,
    )
    .with_header("Retry-After", "120")]);
    let provider = TavilyProvider::new(
        TavilyConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value("secret")),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();

    assert_eq!(error.kind(), "provider_rate_limited");
    assert!(error.is_transient());
    assert!(error.to_string().contains("retry after 120s"));
    assert!(error.to_string().contains("tvly-rate-1"));
}

#[tokio::test]
async fn provider_transport_does_not_follow_redirects() {
    let redirect_target = MockServer::start(vec![MockResponse::json(
        200,
        br#"{"jsonrpc":"2.0","id":1,"result":{"content":[]}}"#,
    )]);
    let source = MockServer::start(vec![MockResponse::redirect(
        redirect_target.endpoint.as_str(),
    )]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(source.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none()),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();

    assert_eq!(source.requests().len(), 1);
    assert!(redirect_target.requests().is_empty());
    assert_eq!(error.kind(), "provider_unavailable");
}

#[tokio::test]
async fn provider_transport_bounds_chunked_decompressed_body() {
    let oversized = vec![b'x'; 2048];
    let server = MockServer::start(vec![MockResponse::chunked_json(200, oversized)]);
    let provider = AnySearchProvider::new(
        AnySearchConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::none())
            .with_http_config(
                ProviderHttpConfig::default()
                    .with_timeout(Duration::from_secs(5))
                    .with_max_response_bytes(512),
            ),
    )
    .unwrap();

    let error = provider.search(&request("rust")).await.unwrap_err();

    assert_eq!(error.kind(), "provider_invalid_response");
    assert!(error.to_string().contains("512-byte safety limit"));
}
