//! First-principles contract for the native provider architecture.
//!
//! These tests fail if a billed JSON API leaves the shared shell, if a
//! credential survives a success or error body, or if cascade selection treats
//! a billed API as a default or anonymous source.

mod support;

use a3s_search::providers::{
    AliyunConfig, AliyunProvider, AnySearchProvider, BochaConfig, BochaProvider, BuiltinProvider,
    CredentialSource, FirecrawlConfig, FirecrawlProvider, ProviderEngine, ProviderReadiness,
    ProviderRequest, SearchProvider, TavilyProvider, TencentConfig, TencentProvider,
    TinyFishConfig, TinyFishProvider,
};
use a3s_search::{Engine, SafeSearch, SearchQuery};
use serde_json::json;
use support::provider_server::{MockResponse, MockServer};
use url::Url;

const SECRET: &str = "sk-architecture-secret";
const BILLED: [&str; 5] = ["tinyfish", "bocha", "aliyun", "tencent", "firecrawl"];

type EchoBody = fn() -> String;
type KeyedProvider = fn(Url) -> a3s_search::Result<Box<dyn SearchProvider>>;

#[test]
fn billed_providers_are_explicit_and_optional_auth_codecs_stay_anonymous() {
    let defaults: Vec<_> = BuiltinProvider::DEFAULT
        .iter()
        .copied()
        .map(BuiltinProvider::id)
        .collect::<Vec<_>>();
    assert_eq!(defaults, ["anysearch", "tavily"]);

    for provider in BuiltinProvider::ALL {
        let engine = provider.create_engine().unwrap();
        let descriptor = engine.descriptor();
        assert_eq!(descriptor.id, provider.id());
        assert_eq!(engine.shortcut(), provider.id());
        assert_eq!(engine.config().shortcut, provider.id());
        if BILLED.contains(&provider.id()) {
            assert!(
                !descriptor.capabilities.anonymous,
                "{} must not enter an anonymous default plan",
                provider.id()
            );
            assert!(
                descriptor.capabilities.time_range,
                "{} advertises time range",
                provider.id()
            );
            assert!(
                !descriptor.capabilities.safe_search,
                "{} does not advertise safe search",
                provider.id()
            );
            assert_eq!(
                descriptor.capabilities.paging,
                provider.id() == "tinyfish",
                "{} paging capability",
                provider.id()
            );
            assert!(!defaults.contains(&provider.id()));
        }
    }

    assert!(
        AnySearchProvider::from_env()
            .unwrap()
            .descriptor()
            .capabilities
            .anonymous
    );
    assert!(
        TavilyProvider::from_env()
            .unwrap()
            .descriptor()
            .capabilities
            .anonymous
    );
}

#[test]
fn required_key_shell_rejects_unsafe_endpoints_and_missing_credentials() {
    let insecure = Url::parse("http://example.com/search").unwrap();
    let credentialed = Url::parse("https://user:secret@example.com/search").unwrap();

    assert!(TinyFishConfig::new()
        .unwrap()
        .with_endpoint(insecure.clone())
        .is_err());
    assert!(BochaConfig::new()
        .unwrap()
        .with_endpoint(credentialed.clone())
        .is_err());
    assert!(AliyunConfig::new()
        .unwrap()
        .with_endpoint(insecure.clone())
        .is_err());
    assert!(TencentConfig::new()
        .unwrap()
        .with_endpoint(credentialed.clone())
        .is_err());
    assert!(FirecrawlConfig::new()
        .unwrap()
        .with_endpoint(insecure)
        .is_err());

    assert!(matches!(
        BochaProvider::new(
            BochaConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::none())
        )
        .unwrap()
        .readiness(),
        ProviderReadiness::MissingCredential { .. }
    ));
    assert!(matches!(
        FirecrawlProvider::new(
            FirecrawlConfig::new()
                .unwrap()
                .with_api_key(CredentialSource::none())
        )
        .unwrap()
        .readiness(),
        ProviderReadiness::MissingCredential { .. }
    ));
}

#[tokio::test]
async fn missing_credential_fails_before_the_network() {
    for build in [
        missing_tinyfish as fn(Url) -> a3s_search::Result<Box<dyn SearchProvider>>,
        missing_bocha,
        missing_aliyun,
        missing_tencent,
        missing_firecrawl,
    ] {
        let server = MockServer::start(vec![MockResponse::json(200, b"{}")]);
        let provider = build(server.endpoint.clone()).unwrap();
        let error = provider
            .search(&ProviderRequest::new("rust"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), "provider_authentication");
        assert!(!error.to_string().contains(SECRET), "{}", error.to_string());
        assert!(
            server.requests().is_empty(),
            "a missing key must not leave the shell"
        );
    }
}

#[tokio::test]
async fn success_urls_are_sealed_against_the_call_credential() {
    let cases: [(&str, EchoBody, KeyedProvider); 5] = [
        ("tinyfish", tinyfish_echo, keyed_tinyfish),
        ("bocha", bocha_echo, keyed_bocha),
        ("aliyun", aliyun_echo, keyed_aliyun),
        ("tencent", tencent_echo, keyed_tencent),
        ("firecrawl", firecrawl_echo, keyed_firecrawl),
    ];

    for (id, body, build) in cases {
        let server = MockServer::start(vec![MockResponse::json(200, body().into_bytes())]);
        let provider = build(server.endpoint.clone()).unwrap();
        let response = provider
            .search(&ProviderRequest::new("rust"))
            .await
            .unwrap_or_else(|error| panic!("{id} search failed: {error}"));
        let rendered = format!("{response:?}");
        assert!(
            !rendered.contains(SECRET),
            "{id} leaked the credential into {rendered}"
        );
        assert!(
            response
                .results
                .iter()
                .any(|result| sealed_url(&result.url)),
            "{id} dropped the echoed URL instead of sealing it: {rendered}"
        );
        assert_eq!(server.requests().len(), 1, "{id}");
    }
}

#[tokio::test]
async fn http_errors_redact_the_credential() {
    let body =
        format!(r#"{{"code":"UNAUTHORIZED","message":"bad key {SECRET}","request_id":"req-1"}}"#);
    for build in [
        keyed_tinyfish as KeyedProvider,
        keyed_bocha,
        keyed_aliyun,
        keyed_tencent,
        keyed_firecrawl,
    ] {
        let server = MockServer::start(vec![MockResponse::json(401, body.clone().into_bytes())]);
        let provider = build(server.endpoint.clone()).unwrap();
        let error = provider
            .search(&ProviderRequest::new("rust"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), "provider_authentication");
        let rendered = error.to_string();
        assert!(!rendered.contains(SECRET), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
        assert!(rendered.contains("req-1"), "{rendered}");
        assert_eq!(server.requests().len(), 1);
    }
}

#[tokio::test]
async fn engine_rejects_unsupported_controls_before_the_network() {
    let server = MockServer::start(vec![MockResponse::json(200, bocha_echo().into_bytes())]);
    let engine = ProviderEngine::new(
        BochaProvider::new(
            BochaConfig::new()
                .unwrap()
                .with_endpoint(server.endpoint.clone())
                .unwrap()
                .with_api_key(CredentialSource::value(SECRET)),
        )
        .unwrap(),
    );

    let page = engine
        .search(&SearchQuery::new("rust").with_page(2))
        .await
        .unwrap_err();
    assert_eq!(page.kind(), "provider_invalid_request");
    assert!(page
        .to_string()
        .contains("does not support result pagination"));

    let safe = engine
        .search(&SearchQuery::new("rust").with_safesearch(SafeSearch::Strict))
        .await
        .unwrap_err();
    assert_eq!(safe.kind(), "provider_invalid_request");
    assert!(safe.to_string().contains("does not support safe-search"));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn engine_forwards_advertised_time_range() {
    let server = MockServer::start(vec![MockResponse::json(200, bocha_echo().into_bytes())]);
    let engine = ProviderEngine::new(
        BochaProvider::new(
            BochaConfig::new()
                .unwrap()
                .with_endpoint(server.endpoint.clone())
                .unwrap()
                .with_api_key(CredentialSource::value(SECRET)),
        )
        .unwrap(),
    );

    engine
        .search(&SearchQuery::new("rust").with_time_range(a3s_search::TimeRange::Day))
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["freshness"], "oneDay");
}

#[tokio::test]
async fn engine_forwards_advertised_paging() {
    let server = MockServer::start(vec![MockResponse::json(200, tinyfish_echo().into_bytes())]);
    let engine = ProviderEngine::new(tinyfish_provider(server.endpoint.clone()));

    engine
        .search(&SearchQuery::new("rust").with_page(2))
        .await
        .unwrap();
    assert_eq!(server.requests().len(), 1);
    assert!(server.requests()[0].request_line.contains("page=1"));

    let server = MockServer::start(vec![MockResponse::json(200, tinyfish_echo().into_bytes())]);
    let engine = ProviderEngine::new(tinyfish_provider(server.endpoint.clone()));
    let error = engine
        .search(&SearchQuery::new("rust").with_safesearch(SafeSearch::Moderate))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "provider_invalid_request");
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn public_result_cap_uses_vendor_wire_fields() {
    let server = MockServer::start(vec![MockResponse::json(200, tencent_echo().into_bytes())]);
    let provider = TencentProvider::new(
        TencentConfig::new()
            .unwrap()
            .with_endpoint(server.endpoint.clone())
            .unwrap()
            .with_api_key(CredentialSource::value(SECRET))
            .with_max_results(20)
            .unwrap(),
    )
    .unwrap();

    provider
        .search(&ProviderRequest::new("rust"))
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&server.requests()[0].body).unwrap();
    assert_eq!(body["Cnt"], 20);
    assert!(body.get("max_results").is_none());
}

fn echo_url() -> String {
    format!("https://example.com/result?k={SECRET}")
}

fn sealed_url(url: &str) -> bool {
    if url.contains(SECRET) {
        return false;
    }
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    parsed
        .query_pairs()
        .any(|(_, value)| value.contains("REDACTED"))
        || url.to_ascii_uppercase().contains("REDACTED")
}

fn tinyfish_echo() -> String {
    json!({
        "results": [{
            "title": "Result",
            "snippet": "text",
            "url": echo_url(),
        }]
    })
    .to_string()
}

fn bocha_echo() -> String {
    json!({
        "code": 200,
        "data": {
            "webPages": {
                "value": [{
                    "name": "Result",
                    "snippet": "text",
                    "url": echo_url(),
                }]
            }
        }
    })
    .to_string()
}

fn aliyun_echo() -> String {
    json!({
        "pageItems": [{
            "title": "Result",
            "snippet": "text",
            "link": echo_url(),
        }]
    })
    .to_string()
}

fn tencent_echo() -> String {
    json!({
        "Response": {
            "Pages": [json!({
                "title": "Result",
                "url": echo_url(),
                "passage": "text",
            }).to_string()]
        }
    })
    .to_string()
}

fn firecrawl_echo() -> String {
    json!({
        "success": true,
        "data": {
            "web": [{
                "title": "Result",
                "description": "text",
                "url": echo_url(),
            }]
        }
    })
    .to_string()
}

fn tinyfish_provider(endpoint: Url) -> TinyFishProvider {
    TinyFishProvider::new(
        TinyFishConfig::new()
            .unwrap()
            .with_endpoint(endpoint)
            .unwrap()
            .with_api_key(CredentialSource::value(SECRET)),
    )
    .unwrap()
}

fn missing_tinyfish(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(TinyFishProvider::new(
        TinyFishConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::none()),
    )?))
}

fn missing_bocha(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(BochaProvider::new(
        BochaConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::none()),
    )?))
}

fn missing_aliyun(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(AliyunProvider::new(
        AliyunConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::none()),
    )?))
}

fn missing_tencent(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(TencentProvider::new(
        TencentConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::none()),
    )?))
}

fn missing_firecrawl(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(FirecrawlProvider::new(
        FirecrawlConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::none()),
    )?))
}

fn keyed_tinyfish(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(TinyFishProvider::new(
        TinyFishConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::value(SECRET)),
    )?))
}

fn keyed_bocha(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(BochaProvider::new(
        BochaConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::value(SECRET)),
    )?))
}

fn keyed_aliyun(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(AliyunProvider::new(
        AliyunConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::value(SECRET)),
    )?))
}

fn keyed_tencent(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(TencentProvider::new(
        TencentConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::value(SECRET)),
    )?))
}

fn keyed_firecrawl(endpoint: Url) -> a3s_search::Result<Box<dyn SearchProvider>> {
    Ok(Box::new(FirecrawlProvider::new(
        FirecrawlConfig::new()?
            .with_endpoint(endpoint)?
            .with_api_key(CredentialSource::value(SECRET)),
    )?))
}
