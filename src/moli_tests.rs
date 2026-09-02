use super::*;
use a3s_use_browser::WaitCondition;
use std::fs;
use std::sync::Arc;

#[test]
fn default_config_has_bounded_parallelism_and_no_implicit_download() {
    let config = MoliPoolConfig::default();
    assert_eq!(config.max_tabs, DEFAULT_MAX_TABS);
    assert!(config.executable.is_none());
    assert!(config.proxy_url.is_none());
    assert_eq!(
        MoliPool::default().available_tab_permits(),
        DEFAULT_MAX_TABS
    );
}

#[test]
fn builder_options_are_typed_and_composable() {
    let config = MoliPoolConfig::with_executable("/tmp/moli")
        .with_proxy("http://user:secret@example.test:8080")
        .with_profile_dir("/tmp/moli-profile")
        .with_max_tabs(0);
    assert_eq!(config.executable, Some(PathBuf::from("/tmp/moli")));
    assert_eq!(config.max_tabs, 0);
    assert_eq!(config.profile_dir, Some(PathBuf::from("/tmp/moli-profile")));
}

#[test]
fn arguments_map_every_typed_wait_condition() {
    let config = MoliPoolConfig::default();
    let cases = [
        (WaitCondition::Load, vec!["load"]),
        (WaitCondition::DomContentLoaded, vec!["domcontentloaded"]),
        (
            WaitCondition::NetworkIdle { idle_ms: 42 },
            vec!["networkidle"],
        ),
        (
            WaitCondition::Selector {
                css: "#ready".to_string(),
                timeout_ms: 500,
            },
            vec!["load", "--wait-selector", "#ready"],
        ),
        (
            WaitCondition::Delay { ms: 250 },
            vec!["load", "--delay-ms", "250"],
        ),
    ];

    for (wait, expected) in cases {
        let request = RenderRequest {
            url: url::Url::parse("https://example.test/").unwrap(),
            timeout_ms: 1_000,
            wait,
            user_agent: Some("a3s-test".to_string()),
            screenshot_path: None,
        };
        let args = fetch_arguments(&config, &request)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let wait_index = args.iter().position(|arg| arg == "--wait-until").unwrap();
        assert_eq!(
            &args[wait_index + 1..wait_index + 1 + expected.len()],
            expected
        );
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--user-agent", "a3s-test"]));
        assert!(args.windows(2).any(|pair| pair == ["--timeout", "1000"]));
    }
}

#[test]
fn request_validation_rejects_empty_selector_and_screenshots() {
    let empty_selector = RenderRequest {
        url: url::Url::parse("https://example.test/").unwrap(),
        timeout_ms: 1_000,
        wait: WaitCondition::Selector {
            css: "   ".to_string(),
            timeout_ms: 100,
        },
        user_agent: None,
        screenshot_path: None,
    };
    let error = validate_request(&empty_selector).unwrap_err();
    assert_eq!(error.code, "use.browser.invalid_request");

    let screenshot = RenderRequest {
        screenshot_path: Some(PathBuf::from("/tmp/page.png")),
        ..empty_selector
    };
    let error = validate_request(&screenshot).unwrap_err();
    assert_eq!(error.code, "use.browser.unsupported");
}

#[test]
fn diagnostics_redact_proxy_and_remain_bounded() {
    let proxy = "http://user:secret@example.test:8080";
    let mut stderr = vec![b'x'; STDERR_DIAGNOSTIC_LIMIT + 100];
    stderr.extend_from_slice(proxy.as_bytes());
    let diagnostic = bounded_diagnostic(&stderr, Some(proxy));
    assert!(diagnostic.contains("<redacted-proxy>"));
    assert!(!diagnostic.contains("secret"));
    assert!(diagnostic.len() <= STDERR_DIAGNOSTIC_LIMIT + "<redacted-proxy>".len());
}

#[test]
fn oversized_output_is_rejected_before_utf8_decoding() {
    let output = CapturedOutput {
        bytes: Vec::new(),
        truncated: true,
    };
    let error = decode_fetch_output(
        success_status(),
        output,
        CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        },
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "use.browser.output_too_large");
}

#[test]
fn nonzero_process_output_is_typed_and_redacts_proxy() {
    let proxy = "http://user:secret@example.test:8080";
    let error = decode_fetch_output(
        failure_status(),
        CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        },
        CapturedOutput {
            bytes: format!("request failed through {proxy}").into_bytes(),
            truncated: false,
        },
        Some(proxy),
    )
    .unwrap_err();
    assert_eq!(error.code, "use.browser.process");
    assert!(error.message.contains("<redacted-proxy>"));
    assert!(!error.message.contains("secret"));
}

#[test]
fn empty_and_non_utf8_process_output_is_rejected() {
    let empty = decode_fetch_output(
        success_status(),
        CapturedOutput {
            bytes: b" \n".to_vec(),
            truncated: false,
        },
        CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        },
        None,
    )
    .unwrap_err();
    assert_eq!(empty.code, "use.browser.empty");

    let non_utf8 = decode_fetch_output(
        success_status(),
        CapturedOutput {
            bytes: vec![0xff],
            truncated: false,
        },
        CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        },
        None,
    )
    .unwrap_err();
    assert_eq!(non_utf8.code, "use.browser.process");
}

#[test]
fn explicit_missing_executable_is_actionable() {
    let pool = MoliPool::from_executable("/definitely/missing/moli");
    let error = pool.warm_up().unwrap_err();
    assert_eq!(error.code, "use.browser.runtime_missing");
    assert!(error.suggestion.unwrap().contains(MOLI_REPOSITORY_URL));
}

#[tokio::test]
async fn shutdown_closes_admission_without_spawning() {
    let pool = MoliPool::new(MoliPoolConfig::with_executable("/definitely/missing/moli"));
    pool.shutdown();
    let request = RenderRequest::new(url::Url::parse("https://example.test/").unwrap());
    let error = pool.render(request).await.unwrap_err();
    assert_eq!(error.code, "use.browser.closed");
    assert!(pool.is_shutdown());
}

#[cfg(unix)]
fn fixture_script(output: &str, sleep_seconds: Option<u64>) -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("moli-fixture");
    let sleep = sleep_seconds
        .map(|seconds| format!("sleep {seconds}\n"))
        .unwrap_or_default();
    fs::write(&path, format!("#!/bin/sh\n{sleep}printf '%s' '{output}'\n")).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    (directory, path)
}

#[cfg(unix)]
#[tokio::test]
async fn renders_html_through_the_real_process_boundary() {
    let (_directory, executable) = fixture_script(
        "<!doctype html><html><body><main>MOLI_FIXTURE</main></body></html>",
        None,
    );
    let pool = Arc::new(MoliPool::from_executable(executable));
    let request = RenderRequest {
        url: url::Url::parse("https://example.test/page").unwrap(),
        timeout_ms: 10_000,
        wait: WaitCondition::Load,
        user_agent: Some("a3s-test-agent".to_string()),
        screenshot_path: None,
    };
    let page = pool.render(request).await.unwrap();
    assert!(page.html.contains("MOLI_FIXTURE"));
    assert_eq!(page.status, None);
    assert_eq!(page.content_type.as_deref(), Some("text/html"));
}

#[cfg(unix)]
#[tokio::test]
async fn process_timeout_kills_the_child_and_returns_typed_error() {
    let (_directory, executable) = fixture_script("too late", Some(5));
    let pool = MoliPool::from_executable(executable);
    let request = RenderRequest {
        url: url::Url::parse("https://example.test/slow").unwrap(),
        timeout_ms: 40,
        wait: WaitCondition::Load,
        user_agent: None,
        screenshot_path: None,
    };
    let started = Instant::now();
    let error = pool.render(request).await.unwrap_err();
    assert_eq!(error.code, "use.browser.timeout");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[tokio::test]
async fn semaphore_limits_concurrent_moli_processes() {
    let (_directory, executable) = fixture_script("ok", Some(1));
    let pool = Arc::new(MoliPool::new(
        MoliPoolConfig::with_executable(executable).with_max_tabs(1),
    ));
    let request = || RenderRequest {
        url: url::Url::parse("https://example.test/").unwrap(),
        timeout_ms: 10_000,
        wait: WaitCondition::Load,
        user_agent: None,
        screenshot_path: None,
    };
    let first = {
        let pool = Arc::clone(&pool);
        tokio::spawn(async move { pool.render(request()).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(pool.available_tab_permits(), 0);
    let second = {
        let pool = Arc::clone(&pool);
        tokio::spawn(async move { pool.render(request()).await })
    };
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(pool.available_tab_permits(), 1);
}

#[cfg(unix)]
fn success_status() -> ExitStatus {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 0")
        .status()
        .unwrap()
}

#[cfg(unix)]
fn failure_status() -> ExitStatus {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 7")
        .status()
        .unwrap()
}

#[cfg(not(unix))]
fn failure_status() -> ExitStatus {
    std::process::Command::new("cmd")
        .args(["/C", "exit", "7"])
        .status()
        .unwrap()
}

#[cfg(not(unix))]
fn success_status() -> ExitStatus {
    std::process::Command::new("cmd")
        .args(["/C", "exit", "0"])
        .status()
        .unwrap()
}
