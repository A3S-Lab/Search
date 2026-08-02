mod support;

use std::io::Write;
use std::process::{Command, Output};

use serde_json::{json, Value};
use support::provider_server::{MockResponse, MockServer};
use tempfile::NamedTempFile;

fn config_file(contents: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("create temporary ACL config");
    file.write_all(contents.as_bytes())
        .expect("write temporary ACL config");
    file.flush().expect("flush temporary ACL config");
    file
}

fn run_search(query: &str, provider: &str, config: &NamedTempFile) -> Output {
    run_search_with_args(query, provider, config, &[])
}

fn run_search_with_args(
    query: &str,
    provider: &str,
    config: &NamedTempFile,
    extra_args: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_a3s-search"));
    command.args([
        query,
        "--engines",
        provider,
        "--config",
        config.path().to_str().expect("UTF-8 temporary path"),
        "--format",
        "json",
    ]);
    command.args(extra_args);
    command.output().expect("run a3s-search")
}

#[test]
fn anysearch_cli_uses_typed_acl_and_emits_full_provider_report() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "_meta": {"request_id": "any-cli-1"},
                "structuredContent": {
                "results": [{
                    "title": "Rust",
                    "url": "https://www.rust-lang.org/",
                    "snippet": "A language empowering everyone",
                    "full_text": "Full AnySearch content"
                }],
                "total_results": 77,
                "response_time_ms": 12
                },
                "content": []
            }
        }"#,
    )]);
    let config = config_file(&format!(
        r#"
        provider "anysearch" {{
            endpoint = "{}"
            api_key = null
            max_results = 7
            domain = "code"
            sub_domain = "code.doc"
            sub_domain_params = {{
                library = "tokio"
                filters = {{ stable = true }}
            }}
        }}
        "#,
        server.endpoint
    ));

    let output = run_search("rust async", "anysearch", &config);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value =
        serde_json::from_slice(&output.stdout).expect("CLI stdout must contain JSON");
    assert_eq!(payload["query"], "rust async");
    assert_eq!(payload["count"], 1);
    assert_eq!(payload["results"][0]["full_text"], "Full AnySearch content");
    assert_eq!(payload["reports"][0]["provider"], "anysearch");
    assert_eq!(payload["reports"][0]["request_id"], "any-cli-1");
    assert_eq!(payload["reports"][0]["total_results"], 77);
    assert_eq!(payload["reports"][0]["response_time_ms"], 12);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].header("authorization").is_none());
    let request: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["method"], "tools/call");
    assert_eq!(request["params"]["name"], "search");
    assert_eq!(request["params"]["arguments"]["query"], "rust async");
    assert_eq!(request["params"]["arguments"]["max_results"], 7);
    assert_eq!(request["params"]["arguments"]["domain"], "code");
    assert_eq!(request["params"]["arguments"]["sub_domain"], "code.doc");
    assert_eq!(
        request["params"]["arguments"]["sub_domain_params"]["library"],
        "tokio"
    );
    assert_eq!(
        request["params"]["arguments"]["sub_domain_params"]["filters"]["stable"],
        true
    );
}

#[test]
fn verbose_json_keeps_stdout_machine_readable() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "structuredContent": {
                    "results": [{
                        "title": "Rust",
                        "url": "https://www.rust-lang.org/",
                        "snippet": "A language empowering everyone"
                    }]
                },
                "content": []
            }
        }"#,
    )]);
    let config = config_file(&format!(
        r#"
        provider "anysearch" {{
            endpoint = "{}"
            api_key = null
        }}
        "#,
        server.endpoint
    ));

    let output = run_search_with_args("rust async", "anysearch", &config, &["--verbose"]);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout)
        .expect("verbose CLI stdout must contain only the JSON document");
    assert_eq!(payload["query"], "rust async");
    assert!(
        !output.stderr.is_empty(),
        "verbose diagnostics must be written to stderr"
    );
}

#[test]
fn tavily_cli_emits_answers_relevance_content_usage_and_metadata() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br##"{
            "answer": "Tokio is a Rust asynchronous runtime.",
            "images": [{"url": "https://images.example/tokio.png", "description": "Tokio logo"}],
            "results": [{
                "title": "Tokio",
                "url": "https://tokio.rs/",
                "content": "An asynchronous runtime.",
                "score": 0.93,
                "raw_content": "# Tokio\nFull Markdown",
                "favicon": "https://tokio.rs/favicon.ico",
                "images": ["https://tokio.rs/runtime.png"]
            }],
            "response_time": 0.2,
            "auto_parameters": {"topic": "general"},
            "usage": {"credits": 1},
            "request_id": "tvly-cli-1"
        }"##,
    )]);
    let config = config_file(&format!(
        r#"
        provider "tavily" {{
            endpoint = "{}"
            api_key = "tvly-cli-secret"
            project = "project-cli"
            search_depth = "advanced"
            chunks_per_source = 2
            max_results = 6
            topic = "general"
            include_answer = "advanced"
            include_raw_content = "markdown"
            include_domains = ["tokio.rs"]
            auto_parameters = true
            include_usage = true
            include_images = true
            include_image_descriptions = true
            include_favicon = true
        }}
        "#,
        server.endpoint
    ));

    let output = run_search("rust async", "tavily", &config);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value =
        serde_json::from_slice(&output.stdout).expect("CLI stdout must contain JSON");
    assert_eq!(
        payload["answers"],
        json!(["Tokio is a Rust asynchronous runtime."])
    );
    assert_eq!(payload["results"][0]["relevance_score"], 0.93);
    assert_eq!(payload["results"][0]["full_text"], "# Tokio\nFull Markdown");
    assert_eq!(
        payload["results"][0]["favicon"],
        "https://tokio.rs/favicon.ico"
    );
    assert_eq!(
        payload["results"][0]["images"][0]["url"],
        "https://tokio.rs/runtime.png"
    );
    assert_eq!(payload["images"][0]["description"], "Tokio logo");
    assert_eq!(payload["reports"][0]["provider"], "tavily");
    assert_eq!(payload["reports"][0]["request_id"], "tvly-cli-1");
    assert_eq!(payload["reports"][0]["response_time_ms"], 200);
    assert_eq!(payload["reports"][0]["usage"]["credits"], 1.0);
    assert_eq!(
        payload["reports"][0]["metadata"]["auto_parameters"]["topic"],
        "general"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("authorization"),
        Some("Bearer tvly-cli-secret")
    );
    assert_eq!(requests[0].header("x-project-id"), Some("project-cli"));
    let request: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(request["search_depth"], "advanced");
    assert_eq!(request["chunks_per_source"], 2);
    assert_eq!(request["include_answer"], "advanced");
    assert_eq!(request["include_raw_content"], "markdown");
    assert_eq!(request["include_domains"], json!(["tokio.rs"]));
    assert_eq!(request["include_usage"], true);
    assert_eq!(request["include_images"], true);
    assert_eq!(request["include_image_descriptions"], true);
    assert_eq!(request["include_favicon"], true);
}

#[test]
fn tavily_cli_uses_keyless_mode_without_a_key() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br#"{
            "query": "rust",
            "results": [],
            "response_time": "0.1",
            "request_id": "tvly-keyless-cli-1"
        }"#,
    )]);
    let config = config_file(&format!(
        r#"
        provider "tavily" {{
            endpoint = "{}"
            api_key = null
        }}
        "#,
        server.endpoint
    ));

    let output = run_search("rust", "tavily", &config);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["reports"][0]["request_id"], "tvly-keyless-cli-1");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].header("authorization").is_none());
    assert_eq!(requests[0].header("x-tavily-access-mode"), Some("keyless"));
    let request: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(request["include_raw_content"], "text");
}

#[test]
fn provider_cli_explains_that_the_scraping_proxy_is_not_inherited() {
    let server = MockServer::start(vec![MockResponse::json(
        200,
        br###"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [{
                    "type": "text",
                    "text": "## Search Results (0 results, 1ms)"
                }]
            }
        }"###,
    )]);
    let config = config_file(&format!(
        r#"
        provider "anysearch" {{
            endpoint = "{}"
            api_key = null
        }}
        "#,
        server.endpoint
    ));

    let output = run_search_with_args(
        "rust",
        "anysearch",
        &config,
        &["--proxy", "not-a-valid-proxy://proxy-user:proxy-secret"],
    );

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("native provider API requests remain direct"),
        "stderr did not explain proxy scope: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("proxy-secret"));
    assert_eq!(server.requests().len(), 1);
}
