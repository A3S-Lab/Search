use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::http::serve_on;
use super::{parse_search_body, reply_for_results, NativeSearchRequest, NativeWebSearch};
use crate::{SearchError, SearchResult, SearchResults};

struct FixedSearch {
    calls: AtomicUsize,
}

#[async_trait]
impl NativeWebSearch for FixedSearch {
    async fn search(&self, request: &NativeSearchRequest) -> crate::Result<SearchResults> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut results = SearchResults::new();
        let mut hit = SearchResult::new(
            "https://www.rust-lang.org/",
            "Rust",
            format!("language {}", request.query),
        );
        hit.full_text = Some("article body".to_string());
        results.add_result(hit);
        Ok(results)
    }
}

struct FailingSearch;

#[async_trait]
impl NativeWebSearch for FailingSearch {
    async fn search(&self, _request: &NativeSearchRequest) -> crate::Result<SearchResults> {
        Err(SearchError::Timeout)
    }
}

#[test]
fn search_body_rejects_a_non_web_source_and_an_empty_query() {
    let rejected = parse_search_body(br#"{"query":"rust","sources":["news"]}"#).unwrap_err();
    let super::SearchReply::Json { status, body } = rejected;
    assert_eq!(status, 400);
    assert_eq!(body["code"], "INVALID_REQUEST");

    let empty = parse_search_body(br#"{"query":"  "}"#).unwrap_err();
    let super::SearchReply::Json { status, .. } = empty;
    assert_eq!(status, 400);
}

#[test]
fn results_use_the_native_web_envelope() {
    let request = parse_search_body(
        br#"{"query":"rust","limit":4,"sources":["web"],"scrapeOptions":{"formats":[{"type":"markdown"}]}}"#,
    )
    .unwrap();
    assert!(request.include_markdown);
    let mut results = SearchResults::new();
    let mut hit = SearchResult::new("https://www.rust-lang.org/", "Rust", "language");
    hit.full_text = Some("body".to_string());
    results.add_result(hit);
    let super::SearchReply::Json { status, body } = reply_for_results(&request, &results);
    assert_eq!(status, 200);
    assert_eq!(body["success"], true);
    assert_eq!(
        body["data"]["results"]["web"][0]["url"],
        "https://www.rust-lang.org/"
    );
    assert_eq!(body["data"]["results"]["web"][0]["markdown"], "body");
}

#[test]
fn listen_address_is_host_port_and_a_public_bind_needs_a_key() {
    let loopback = super::http::parse_listen_addr("127.0.0.1:8090").unwrap();
    assert!(super::http::public_bind_requires_key(loopback, None).is_ok());
    let public = super::http::parse_listen_addr("0.0.0.0:8090").unwrap();
    assert!(super::http::public_bind_requires_key(public, None).is_err());
    assert!(super::http::public_bind_requires_key(public, Some("deployment-secret")).is_ok());
    assert!(super::http::parse_listen_addr("not-an-address").is_err());
}

#[tokio::test]
async fn serve_checks_the_deployment_key_and_returns_web_hits() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let search = Arc::new(FixedSearch {
        calls: AtomicUsize::new(0),
    });
    let calls = Arc::clone(&search);
    tokio::spawn(serve_on(
        listener,
        search,
        Some("deployment-secret".to_string()),
    ));

    let health = http("GET", &address, "/health", None, "").await;
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.contains("\"status\":\"ok\""), "{health}");
    assert_eq!(calls.calls.load(Ordering::SeqCst), 0);

    let denied = http(
        "POST",
        &address,
        "/v1/search",
        Some("Bearer other"),
        r#"{"query":"rust","limit":4,"sources":["web"]}"#,
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 401"), "{denied}");
    assert_eq!(calls.calls.load(Ordering::SeqCst), 0);

    let allowed = http(
        "POST",
        &address,
        "/v1/search",
        Some("Bearer deployment-secret"),
        r#"{"query":"rust","limit":4,"sources":["web"]}"#,
    )
    .await;
    assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
    assert!(allowed.contains("https://www.rust-lang.org/"), "{allowed}");
    assert_eq!(calls.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn serve_returns_unavailable_when_search_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(serve_on(listener, Arc::new(FailingSearch), None));
    let response = http(
        "POST",
        &address,
        "/v1/search",
        None,
        r#"{"query":"rust","sources":["web"]}"#,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert!(response.contains("UNAVAILABLE"), "{response}");
}

async fn http(
    method: &str,
    address: &std::net::SocketAddr,
    path: &str,
    authorization: Option<&str>,
    body: &str,
) -> String {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let auth = authorization
        .map(|value| format!("Authorization: {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).into_owned()
}
