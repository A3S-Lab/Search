//! Product contracts the architecture shell tests do not cover.
//!
//! These tests fail if a vendor treats an application error as an empty
//! success, downgrades a quota code to a permission error, sends a billed
//! extra by default, or accepts a query the vendor API rejects.

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use a3s_search::providers::{
    AliyunConfig, AliyunProvider, BochaConfig, BochaProvider, CredentialSource, FirecrawlCategory,
    FirecrawlConfig, FirecrawlProvider, FirecrawlSource, ProviderEngine, ProviderRequest,
    SearchProvider, TencentConfig, TencentProvider, TinyFishConfig, TinyFishDomainType,
    TinyFishProvider,
};
use a3s_search::{Engine, SearchConfig, SearchQuery, TimeRange};
use serde_json::{json, Value};
use support::provider_server::{MockResponse, MockServer};
use url::Url;

const SECRET: &str = "sk-product-secret";

#[tokio::test]
async fn http_200_application_errors_are_not_empty_success() {
    let cases = [
        (
            "bocha",
            json!({"code": 403, "msg": format!("quota {SECRET}"), "log_id": "b-app"}).to_string(),
            "provider_quota",
            "b-app",
        ),
        (
            "aliyun",
            json!({
                "code": "Retrieval.Arrears",
                "message": format!("no credit {SECRET}"),
                "requestId": "a-app",
            })
            .to_string(),
            "provider_quota",
            "a-app",
        ),
        (
            "firecrawl",
            json!({
                "success": false,
                "code": "INSUFFICIENT_CREDITS",
                "error": format!("need credits {SECRET}"),
            })
            .to_string(),
            "provider_quota",
            "",
        ),
        (
            "tencent",
            json!({
                "Response": {
                    "Error": {
                        "Code": "ResourceUnavailable",
                        "Message": format!("plan exhausted {SECRET}"),
                    },
                    "RequestId": "w-app",
                }
            })
            .to_string(),
            "provider_quota",
            "w-app",
        ),
    ];

    for (id, body, kind, request_id) in cases {
        let server = MockServer::start(vec![MockResponse::json(200, body.into_bytes())]);
        let provider = keyed(id, server.endpoint.clone());
        let error = provider
            .search(&ProviderRequest::new("rust"))
            .await
            .expect_err(&format!("{id} treated an application error as success"));
        let rendered = error.to_string();
        assert_eq!(error.kind(), kind, "{id}: {rendered}");
        assert!(!rendered.contains(SECRET), "{id} leaked {rendered}");
        if !request_id.is_empty() {
            assert!(rendered.contains(request_id), "{id}: {rendered}");
        }
        assert_eq!(server.requests().len(), 1, "{id}");
    }
}

#[tokio::test]
async fn undeclared_success_is_not_an_empty_result() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{"data":{"web":[{"url":"https://example.com/hidden","title":"Hidden"}]}}"#,
    )]);
    let error = keyed("firecrawl", server.endpoint.clone())
        .search(&ProviderRequest::new("rust"))
        .await
        .expect_err("a body that does not declare success must not return results");
    assert_eq!(error.kind(), "provider_invalid_response", "{error}");
    assert!(!error.to_string().contains(SECRET));
}

#[tokio::test]
async fn quota_codes_are_not_downgraded_to_transport_status() {
    let bocha = MockServer::start(vec![MockResponse::json(
        403,
        format!(r#"{{"code":"403","message":"quota {SECRET}","log_id":"b-403"}}"#).into_bytes(),
    )]);
    let error = keyed("bocha", bocha.endpoint.clone())
        .search(&ProviderRequest::new("rust"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "provider_quota", "{error}");
    assert!(!error.to_string().contains(SECRET));

    let tinyfish = MockServer::start(vec![MockResponse::json(
        403,
        format!(
            r#"{{"error":{{"code":"INSUFFICIENT_CREDITS","message":"no credits {SECRET}"}},"request_id":"tf-q"}}"#
        )
        .into_bytes(),
    )]);
    let error = keyed("tinyfish", tinyfish.endpoint.clone())
        .search(&ProviderRequest::new("rust"))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "provider_quota", "{error}");
    assert!(error.to_string().contains("tf-q"), "{error}");
    assert!(!error.to_string().contains(SECRET));
}

#[tokio::test]
async fn vendor_codes_override_a_conflicting_http_status() {
    let cases = [
        (
            "tinyfish",
            400,
            json!({"error": {"code": "RATE_LIMIT_EXCEEDED", "message": "slow down"}}).to_string(),
            "provider_rate_limited",
        ),
        (
            "tinyfish",
            400,
            json!({"error": {"code": "FORBIDDEN", "message": "blocked"}}).to_string(),
            "provider_permission",
        ),
        (
            "bocha",
            500,
            json!({"code": "401", "message": "unauthorized"}).to_string(),
            "provider_authentication",
        ),
        (
            "bocha",
            500,
            json!({"code": "429", "message": "slow down"}).to_string(),
            "provider_rate_limited",
        ),
        (
            "aliyun",
            500,
            json!({"code": "Retrieval.NotActivate", "message": "closed"}).to_string(),
            "provider_permission",
        ),
        (
            "aliyun",
            400,
            json!({"code": "Retrieval.Throttling.User", "message": "slow down"}).to_string(),
            "provider_rate_limited",
        ),
        (
            "tencent",
            200,
            json!({"Response": {"Error": {"Code": "RequestLimitExceeded", "Message": "slow down"}}}).to_string(),
            "provider_rate_limited",
        ),
        (
            "firecrawl",
            400,
            json!({"success": false, "code": "RATE_LIMIT_EXCEEDED", "error": "slow down"}).to_string(),
            "provider_rate_limited",
        ),
        (
            "firecrawl",
            500,
            json!({"success": false, "code": "FORBIDDEN", "error": "blocked"}).to_string(),
            "provider_permission",
        ),
    ];

    for (id, status, body, kind) in cases {
        let server = MockServer::start(vec![MockResponse::json(status, body.into_bytes())]);
        let error = keyed(id, server.endpoint.clone())
            .search(&ProviderRequest::new("rust"))
            .await
            .expect_err(id);
        assert_eq!(error.kind(), kind, "{id} status {status}: {error}");
    }
}

#[tokio::test]
async fn defaults_omit_billed_extras() {
    let tinyfish = search_default("tinyfish").await;
    assert!(!tinyfish.request_line.contains("include_thumbnail"));

    let bocha = search_default("bocha").await;
    let body = json_body(&bocha.body);
    assert_eq!(body["summary"], true);
    assert!(body.get("count").is_some());

    let aliyun = search_default("aliyun").await;
    let body = json_body(&aliyun.body);
    assert_eq!(body["engineType"], "LiteAdvanced");
    assert_eq!(body["contents"]["markdownText"], false);
    assert_eq!(body["contents"]["summary"], false);
    assert_eq!(body["contents"]["mainText"], false);

    let tencent = search_default("tencent").await;
    let body = json_body(&tencent.body);
    assert!(body.get("Cnt").is_none());

    let firecrawl = MockServer::start(vec![ok("firecrawl")]);
    let provider = FirecrawlProvider::new(
        FirecrawlConfig::new()
            .unwrap()
            .with_endpoint(firecrawl.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value(SECRET)),
    )
    .unwrap();
    assert!(!provider.descriptor().capabilities.full_text);
    assert!(!provider.descriptor().capabilities.images);
    provider
        .search(&ProviderRequest::new("rust"))
        .await
        .unwrap();
    let body = json_body(&firecrawl.requests()[0].body);
    assert!(body.get("scrapeOptions").is_none());
    assert!(body.get("sources").is_none());

    let opted = MockServer::start(vec![ok("firecrawl")]);
    let opted_provider = FirecrawlProvider::new(
        FirecrawlConfig::new()
            .unwrap()
            .with_endpoint(opted.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value(SECRET))
            .with_include_markdown(true),
    )
    .unwrap();
    assert!(opted_provider.descriptor().capabilities.full_text);
    opted_provider
        .search(&ProviderRequest::new("rust"))
        .await
        .unwrap();
    let body = json_body(&opted.requests()[0].body);
    assert_eq!(body["scrapeOptions"]["formats"][0]["type"], "markdown");

    let quiet = MockServer::start(vec![ok("bocha")]);
    BochaProvider::new(
        BochaConfig::new()
            .unwrap()
            .with_endpoint(quiet.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value(SECRET))
            .with_summary(false),
    )
    .unwrap()
    .search(&ProviderRequest::new("rust"))
    .await
    .unwrap();
    assert_eq!(json_body(&quiet.requests()[0].body)["summary"], false);
}

#[test]
fn vendor_options_that_the_api_rejects_never_become_a_request() {
    let error = FirecrawlConfig::new()
        .unwrap()
        .with_categories(vec![
            FirecrawlCategory::Developer,
            FirecrawlCategory::Github,
        ])
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("developer category cannot be combined"));

    let error = FirecrawlConfig::new()
        .unwrap()
        .with_sources(Vec::<FirecrawlSource>::new())
        .unwrap_err();
    assert!(error.to_string().contains("at least one"));

    let error = FirecrawlConfig::new()
        .unwrap()
        .with_include_domains(["example.com"])
        .unwrap()
        .with_exclude_domains(["other.com"])
        .unwrap_err();
    assert!(error.to_string().contains("cannot be combined"));
}

#[tokio::test]
async fn empty_queries_fail_before_the_network() {
    for id in ["tinyfish", "bocha", "aliyun", "tencent", "firecrawl"] {
        let server = MockServer::start(vec![ok(id)]);
        let error = keyed(id, server.endpoint.clone())
            .search(&ProviderRequest::new("   "))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), "provider_invalid_request", "{id}: {error}");
        assert!(
            error.to_string().contains("query must be 1 to"),
            "{id}: {error}"
        );
        assert!(server.requests().is_empty(), "{id}");
    }
}

#[tokio::test]
async fn tinyfish_rejects_out_of_contract_queries_before_the_network() {
    let server = MockServer::start(vec![ok("tinyfish")]);
    let engine = ProviderEngine::new(tinyfish(server.endpoint.clone(), TinyFishDomainType::Web));
    let error = engine
        .search(&SearchQuery::new("rust").with_page(12))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "provider_invalid_request");
    assert!(error.to_string().contains("page must be between 1 and 11"));
    assert!(server.requests().is_empty());

    let papers = ProviderEngine::new(tinyfish(
        server.endpoint.clone(),
        TinyFishDomainType::ResearchPaper,
    ));
    let error = papers
        .search(&SearchQuery::new("rust").with_time_range(TimeRange::Day))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not accept a time range"));
    assert!(server.requests().is_empty());

    let long = MockServer::start(vec![ok("firecrawl")]);
    let engine = ProviderEngine::new(
        FirecrawlProvider::new(
            FirecrawlConfig::new()
                .unwrap()
                .with_endpoint(long.endpoint.clone())
                .unwrap()
                .with_api_key(CredentialSource::value(SECRET)),
        )
        .unwrap(),
    );
    let error = engine
        .search(&SearchQuery::new("q".repeat(501)))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("1 to 500"), "{error}");
    assert!(long.requests().is_empty());

    let page = MockServer::start(vec![ok("tinyfish")]);
    let engine = ProviderEngine::new(tinyfish(page.endpoint.clone(), TinyFishDomainType::Web));
    engine
        .search(&SearchQuery::new("rust").with_page(11))
        .await
        .unwrap();
    assert!(page.requests()[0].request_line.contains("page=10"));
}

#[tokio::test]
async fn advertised_time_ranges_use_documented_wire_labels() {
    let ranges = [
        (
            TimeRange::Day,
            "oneDay",
            "OneDay",
            "qdr:d",
            "1440",
            24 * 60 * 60,
        ),
        (
            TimeRange::Week,
            "oneWeek",
            "OneWeek",
            "qdr:w",
            "10080",
            7 * 24 * 60 * 60,
        ),
        (
            TimeRange::Month,
            "oneMonth",
            "OneMonth",
            "qdr:m",
            "43200",
            30 * 24 * 60 * 60,
        ),
        (
            TimeRange::Year,
            "oneYear",
            "OneYear",
            "qdr:y",
            "525600",
            365 * 24 * 60 * 60,
        ),
    ];

    for (range, bocha, aliyun, firecrawl, minutes, seconds) in ranges {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let bocha_server = MockServer::start(vec![ok("bocha")]);
        keyed("bocha", bocha_server.endpoint.clone())
            .search(&request_with_range(range))
            .await
            .unwrap();
        assert_eq!(
            json_body(&bocha_server.requests()[0].body)["freshness"],
            bocha
        );

        let aliyun_server = MockServer::start(vec![ok("aliyun")]);
        keyed("aliyun", aliyun_server.endpoint.clone())
            .search(&request_with_range(range))
            .await
            .unwrap();
        assert_eq!(
            json_body(&aliyun_server.requests()[0].body)["timeRange"],
            aliyun
        );

        let firecrawl_server = MockServer::start(vec![ok("firecrawl")]);
        keyed("firecrawl", firecrawl_server.endpoint.clone())
            .search(&request_with_range(range))
            .await
            .unwrap();
        assert_eq!(
            json_body(&firecrawl_server.requests()[0].body)["tbs"],
            firecrawl
        );

        let tinyfish_server = MockServer::start(vec![ok("tinyfish")]);
        keyed("tinyfish", tinyfish_server.endpoint.clone())
            .search(&request_with_range(range))
            .await
            .unwrap();
        assert!(tinyfish_server.requests()[0]
            .request_line
            .contains(&format!("recency_minutes={minutes}")));

        let tencent_server = MockServer::start(vec![ok("tencent")]);
        keyed("tencent", tencent_server.endpoint.clone())
            .search(&request_with_range(range))
            .await
            .unwrap();
        let from_time = json_body(&tencent_server.requests()[0].body)["FromTime"]
            .as_u64()
            .unwrap();
        let expected = started.saturating_sub(seconds);
        assert!(
            (expected.saturating_sub(30)..=expected.saturating_add(30)).contains(&from_time),
            "{range:?} FromTime {from_time} is not near {expected}"
        );
    }
}

#[tokio::test]
async fn acl_public_attributes_reach_the_vendor_wire() {
    let tinyfish = MockServer::start(vec![ok("tinyfish")]);
    search_acl(
        "tinyfish",
        &format!(
            r#"provider "tinyfish" {{ endpoint = "{}" api_key = "{SECRET}" domain_type = "news" include_thumbnail = true }}"#,
            tinyfish.endpoint
        ),
    )
    .await;
    let line = &tinyfish.requests()[0].request_line;
    assert!(line.contains("domain_type=news"), "{line}");
    assert!(line.contains("include_thumbnail=true"), "{line}");
    assert_eq!(tinyfish.requests()[0].header("x-api-key"), Some(SECRET));

    let bocha = MockServer::start(vec![ok("bocha")]);
    search_acl(
        "bocha",
        &format!(
            r#"provider "bocha" {{ endpoint = "{}" api_key = "{SECRET}" max_results = 8 summary = false }}"#,
            bocha.endpoint
        ),
    )
    .await;
    let body = json_body(&bocha.requests()[0].body);
    assert_eq!(body["count"], 8);
    assert_eq!(body["summary"], false);
    assert!(body.get("max_results").is_none());

    let aliyun = MockServer::start(vec![ok("aliyun")]);
    search_acl(
        "aliyun",
        &format!(
            r#"provider "aliyun" {{ endpoint = "{}" api_key = "{SECRET}" engine_type = "lite-advanced" max_results = 6 }}"#,
            aliyun.endpoint
        ),
    )
    .await;
    let body = json_body(&aliyun.requests()[0].body);
    assert_eq!(body["engineType"], "LiteAdvanced");
    assert_eq!(body["advancedParams"]["numResults"], "6");

    let tencent = MockServer::start(vec![ok("tencent")]);
    search_acl(
        "tencent",
        &format!(
            r#"provider "tencent" {{ endpoint = "{}" api_key = "{SECRET}" max_results = 20 site = "rust-lang.org" }}"#,
            tencent.endpoint
        ),
    )
    .await;
    let body = json_body(&tencent.requests()[0].body);
    assert_eq!(body["Cnt"], 20);
    assert_eq!(body["Site"], "rust-lang.org");
    assert!(body.get("max_results").is_none());

    let firecrawl = MockServer::start(vec![ok("firecrawl")]);
    let engine = engine_from_acl(
        "firecrawl",
        &format!(
            r#"provider "firecrawl" {{ endpoint = "{}" api_key = "{SECRET}" max_results = 4 country = "us" include_markdown = true }}"#,
            firecrawl.endpoint
        ),
    );
    assert!(engine.descriptor().capabilities.full_text);
    engine.search(&SearchQuery::new("rust")).await.unwrap();
    let body = json_body(&firecrawl.requests()[0].body);
    assert_eq!(body["limit"], 4);
    assert_eq!(body["country"], "US");
    assert_eq!(body["scrapeOptions"]["formats"][0]["type"], "markdown");
    assert!(body.get("max_results").is_none());
}

fn request_with_range(range: TimeRange) -> ProviderRequest {
    ProviderRequest::new("rust").with_time_range(range)
}

fn json_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn ok(id: &str) -> MockResponse {
    let body = match id {
        "tinyfish" => json!({"results": []}),
        "bocha" => json!({"code": 200, "data": {"webPages": {"value": []}}}),
        "aliyun" => json!({}),
        "tencent" => json!({"Response": {}}),
        "firecrawl" => json!({"success": true, "data": {}}),
        _ => unreachable!("{id}"),
    };
    MockResponse::json(200, body.to_string().into_bytes())
}

async fn search_default(id: &str) -> support::provider_server::CapturedRequest {
    let server = MockServer::start(vec![ok(id)]);
    keyed(id, server.endpoint.clone())
        .search(&ProviderRequest::new("rust"))
        .await
        .unwrap();
    server.requests().into_iter().next().unwrap()
}

fn keyed(id: &str, endpoint: Url) -> Box<dyn SearchProvider> {
    match id {
        "tinyfish" => Box::new(tinyfish(endpoint, TinyFishDomainType::Web)),
        "bocha" => Box::new(
            BochaProvider::new(
                BochaConfig::new()
                    .unwrap()
                    .with_endpoint(endpoint)
                    .unwrap()
                    .with_api_key(CredentialSource::value(SECRET)),
            )
            .unwrap(),
        ),
        "aliyun" => Box::new(
            AliyunProvider::new(
                AliyunConfig::new()
                    .unwrap()
                    .with_endpoint(endpoint)
                    .unwrap()
                    .with_api_key(CredentialSource::value(SECRET)),
            )
            .unwrap(),
        ),
        "tencent" => Box::new(
            TencentProvider::new(
                TencentConfig::new()
                    .unwrap()
                    .with_endpoint(endpoint)
                    .unwrap()
                    .with_api_key(CredentialSource::value(SECRET)),
            )
            .unwrap(),
        ),
        "firecrawl" => Box::new(
            FirecrawlProvider::new(
                FirecrawlConfig::new()
                    .unwrap()
                    .with_endpoint(endpoint)
                    .unwrap()
                    .with_api_key(CredentialSource::value(SECRET)),
            )
            .unwrap(),
        ),
        _ => unreachable!("{id}"),
    }
}

fn tinyfish(endpoint: Url, domain_type: TinyFishDomainType) -> TinyFishProvider {
    TinyFishProvider::new(
        TinyFishConfig::new()
            .unwrap()
            .with_endpoint(endpoint)
            .unwrap()
            .with_api_key(CredentialSource::value(SECRET))
            .with_domain_type(domain_type),
    )
    .unwrap()
}

fn engine_from_acl(id: &str, acl: &str) -> ProviderEngine {
    SearchConfig::parse(acl)
        .unwrap()
        .create_provider_engine(id)
        .unwrap()
        .unwrap()
}

async fn search_acl(id: &str, acl: &str) {
    engine_from_acl(id, acl)
        .search(&SearchQuery::new("rust"))
        .await
        .unwrap();
}
