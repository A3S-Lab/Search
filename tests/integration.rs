//! Integration tests for search engines using real HTTP requests.
//!
//! Tests that make live network requests use `require_network!` to skip
//! gracefully when the internet is unavailable, rather than failing.
//! Run all tests normally with: `cargo test -p a3s-search --test integration`

use a3s_search::{Engine, SearchQuery, SearchResult};

/// Skip a network-dependent test gracefully when the internet is unreachable.
///
/// Attempts a TCP connection to `1.1.1.1:80` (Cloudflare DNS / HTTP). If the
/// connection fails, the test prints a message and returns immediately (pass).
macro_rules! require_network {
    () => {
        if std::net::TcpStream::connect_timeout(
            &"1.1.1.1:80".parse().unwrap(),
            std::time::Duration::from_secs(3),
        )
        .is_err()
        {
            println!("  no network connectivity — skipping test");
            return;
        }
    };
}

/// Helper to run an engine test
async fn test_engine<E: Engine>(engine: E, query: &str) -> Vec<SearchResult> {
    let query = SearchQuery::new(query);
    match engine.search(&query).await {
        Ok(results) => {
            println!(
                "Engine '{}' returned {} results for '{}'",
                engine.name(),
                results.len(),
                query.query
            );
            for (i, result) in results.iter().take(3).enumerate() {
                println!("  {}. {} - {}", i + 1, result.title, result.url);
            }
            results
        }
        Err(e) => {
            println!("Engine '{}' failed: {}", engine.name(), e);
            vec![]
        }
    }
}

/// Shared localhost HTTP fixture server for integration tests.
///
/// Returns a fixed HTML body to every request. Accepts non-blocking so `Drop` can stop
/// and join the thread without leaking the socket; the accepted socket is set back to
/// blocking so the request is fully drained before reply (avoids RST-on-close resets).
mod fixture_server {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    pub(crate) struct FixtureServer {
        pub(crate) url: String,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl FixtureServer {
        pub(crate) fn start(body: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            listener.set_nonblocking(true).expect("set nonblocking");
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = Arc::clone(&stop);
            let handle = std::thread::spawn(move || {
                while !stop_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // The accepted socket inherits the listener's non-blocking flag on
                            // macOS; make it blocking (with a timeout) so we fully drain the
                            // request before replying. Closing with unread inbound bytes makes
                            // the kernel send RST, which intermittently resets the client.
                            let _ = stream.set_nonblocking(false);
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                            let mut buf = [0u8; 2048];
                            let _ = stream.read(&mut buf); // best-effort drain of the request
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes());
                            let _ = stream.flush();
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            FixtureServer {
                url,
                stop,
                handle: Some(handle),
            }
        }
    }

    impl Drop for FixtureServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }
}

mod duckduckgo_tests {
    use super::*;
    use a3s_search::engines::DuckDuckGo;

    #[tokio::test]
    async fn test_duckduckgo_search() {
        require_network!();
        let engine = DuckDuckGo::new();
        let results = test_engine(engine, "rust programming").await;
        // DuckDuckGo may return an empty anti-bot response even when general
        // network connectivity is healthy. Parser correctness is covered by
        // deterministic engine unit tests; this live smoke only verifies that
        // the request completes without making a third-party result count a
        // release gate.
        println!("DuckDuckGo live smoke returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_duckduckgo_chinese_query() {
        require_network!();
        let engine = DuckDuckGo::new();
        let results = test_engine(engine, "Rust 编程语言").await;
        // May or may not return results for Chinese queries
        println!("Chinese query returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_duckduckgo_config() {
        let engine = DuckDuckGo::new();
        assert_eq!(engine.name(), "DuckDuckGo");
        assert_eq!(engine.shortcut(), "ddg");
        assert!(engine.is_enabled());
    }
}

mod wikipedia_tests {
    use super::*;
    use a3s_search::engines::Wikipedia;

    #[tokio::test]
    async fn test_wikipedia_search() {
        require_network!();
        let engine = Wikipedia::new();
        let results = test_engine(engine, "rust programming language").await;
        assert!(!results.is_empty(), "Wikipedia should return results");
    }

    #[tokio::test]
    async fn test_wikipedia_chinese() {
        require_network!();
        let engine = Wikipedia::new().with_language("zh");
        let results = test_engine(engine, "Rust").await;
        println!("Chinese Wikipedia returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_wikipedia_config() {
        let engine = Wikipedia::new();
        assert_eq!(engine.name(), "Wikipedia");
        assert_eq!(engine.shortcut(), "wiki");
        assert!(engine.is_enabled());
    }
}

mod brave_tests {
    use super::*;
    use a3s_search::engines::Brave;

    #[tokio::test]
    async fn test_brave_search() {
        require_network!();
        let engine = Brave::new();
        let results = test_engine(engine, "rust programming").await;
        // Brave may block automated requests
        println!("Brave returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_brave_config() {
        let engine = Brave::new();
        assert_eq!(engine.name(), "Brave");
        assert_eq!(engine.shortcut(), "brave");
        assert!(engine.is_enabled());
    }
}

mod sogou_tests {
    use super::*;
    use a3s_search::engines::Sogou;

    #[tokio::test]
    async fn test_sogou_search() {
        require_network!();
        let engine = Sogou::new();
        let results = test_engine(engine, "Rust 编程").await;
        println!("Sogou returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_sogou_config() {
        let engine = Sogou::new();
        assert_eq!(engine.name(), "Sogou");
        assert_eq!(engine.shortcut(), "sogou");
        assert!(engine.is_enabled());
    }
}

mod so360_tests {
    use super::*;
    use a3s_search::engines::So360;

    #[tokio::test]
    async fn test_so360_search() {
        require_network!();
        let engine = So360::new();
        let results = test_engine(engine, "Rust 编程").await;
        println!("360 Search returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_so360_config() {
        let engine = So360::new();
        assert_eq!(engine.name(), "360 Search");
        assert_eq!(engine.shortcut(), "360");
        assert!(engine.is_enabled());
    }
}

#[cfg(feature = "headless")]
mod google_tests {
    use super::*;
    use std::sync::Arc;

    use a3s_search::{
        browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
        engines::Google,
        WaitStrategy,
    };

    fn make_google_engine() -> Google {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = Arc::new(BrowserFetcher::new(pool).with_wait(WaitStrategy::Selector {
            css: "div.g".to_string(),
            timeout_ms: 5000,
        }));
        Google::new(fetcher)
    }

    #[tokio::test]
    async fn test_google_search() {
        require_network!();
        let engine = make_google_engine();
        let results = test_engine(engine, "rust programming").await;
        // Google anti-bot detection may block headless browsers; don't assert on count.
        println!("Google returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_google_config() {
        let engine = make_google_engine();
        assert_eq!(engine.name(), "Google");
        assert_eq!(engine.shortcut(), "g");
        assert!(engine.is_enabled());
        assert_eq!(engine.weight(), 1.5);
    }
}

#[cfg(feature = "headless")]
mod baidu_tests {
    use super::*;
    use std::sync::Arc;

    use a3s_search::{
        browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
        engines::Baidu,
        WaitStrategy,
    };

    fn make_baidu_engine() -> Baidu {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = Arc::new(BrowserFetcher::new(pool).with_wait(WaitStrategy::Selector {
            css: "div.c-container".to_string(),
            timeout_ms: 5000,
        }));
        Baidu::new(fetcher)
    }

    #[tokio::test]
    async fn test_baidu_search() {
        require_network!();
        let engine = make_baidu_engine();
        let results = test_engine(engine, "Rust 编程").await;
        println!("Baidu returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_baidu_config() {
        let engine = make_baidu_engine();
        assert_eq!(engine.name(), "Baidu");
        assert_eq!(engine.shortcut(), "baidu");
        assert!(engine.is_enabled());
    }
}

mod bing_china_tests {
    use super::*;
    use std::sync::Arc;

    use a3s_search::{engines::BingChina, HttpFetcher};

    fn make_bing_china_engine() -> BingChina {
        BingChina::new(Arc::new(HttpFetcher::new()))
    }

    #[tokio::test]
    async fn test_bing_china_search() {
        require_network!();
        let engine = make_bing_china_engine();
        let started = std::time::Instant::now();
        let results = test_engine(engine, "巴威 2020 台风 登陆 风速 灾情 预警").await;
        assert!(
            started.elapsed() <= std::time::Duration::from_secs(15),
            "Bing China search exceeded the 15-second convergence budget"
        );
        assert!(!results.is_empty(), "Bing China returned no results");
        println!("Bing China returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_bing_china_config() {
        let engine = make_bing_china_engine();
        assert_eq!(engine.name(), "Bing China");
        assert_eq!(engine.shortcut(), "bing_cn");
        assert!(engine.is_enabled());
    }
}

mod cli_bing_rss_tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn test_bing_china_cli_returns_json_within_15_seconds() {
        require_network!();

        let started = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_a3s-search"))
            .args([
                "巴威 2020 台风 登陆 风速 灾情 预警",
                "-e",
                "bing_cn",
                "-f",
                "json",
                "-t",
                "15",
            ])
            .output()
            .expect("run a3s-search");
        let elapsed = started.elapsed();

        assert!(output.status.success(), "CLI failed: {output:?}");
        assert!(
            elapsed <= Duration::from_secs(15),
            "CLI took {elapsed:?}, exceeding the 15-second convergence budget"
        );

        let payload: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("CLI stdout must be JSON");
        assert!(payload["count"].as_u64().unwrap_or_default() > 0);
        assert_eq!(payload["errors"].as_array().map(Vec::len), Some(0));
    }
}

mod bing_tests {
    use super::*;
    use a3s_search::engines::Bing;

    #[tokio::test]
    async fn test_bing_search() {
        require_network!();
        let engine = Bing::new();
        let results = test_engine(engine, "rust programming").await;
        println!("Bing returned {} results", results.len());
    }

    #[tokio::test]
    async fn test_bing_config() {
        let engine = Bing::new();
        assert_eq!(engine.name(), "Bing");
        assert_eq!(engine.shortcut(), "bing");
        assert!(engine.is_enabled());
    }
}

mod health_monitor_tests {
    use std::time::Duration;

    use a3s_search::{engines::DuckDuckGo, HealthConfig, Search, SearchQuery};

    #[tokio::test]
    async fn test_health_monitor_with_real_engines() {
        require_network!();
        let config = HealthConfig {
            max_failures: 2,
            suspend_duration: Duration::from_secs(60),
        };
        let mut search = Search::with_health_config(config);
        search.add_engine(DuckDuckGo::new());

        let query = SearchQuery::new("rust programming");
        let results = search.search(query).await.unwrap();

        println!(
            "Health monitor test: {} results in {}ms, {} errors",
            results.count,
            results.duration_ms,
            results.errors().len()
        );
    }
}

mod config_tests {
    use a3s_search::SearchConfig;

    #[test]
    fn test_load_acl_config() {
        let acl = r#"
            timeout {
                value = 8
            }

            health {
                max_failures    = 5
                suspend_seconds = 120
            }

            engine "ddg" {
                enabled = true
                weight  = 1.0
            }

            engine "bing" {
                enabled = true
                weight  = 1.2
            }

            engine "wiki" {
                enabled = true
                weight  = 0.8
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        assert_eq!(config.timeout, 8);
        assert_eq!(config.engines.len(), 3);

        let health = config.health_config();
        assert_eq!(health.max_failures, 5);

        let enabled = config.enabled_engines();
        assert_eq!(enabled.len(), 3);
        assert!(enabled.contains(&"ddg"));
        assert!(enabled.contains(&"bing"));
        assert!(enabled.contains(&"wiki"));
    }

    #[test]
    fn test_config_disabled_engines() {
        let acl = r#"
            engine "ddg" {
                enabled = true
            }

            engine "sogou" {
                enabled = false
            }
        "#;

        let config = SearchConfig::parse(acl).unwrap();
        let enabled = config.enabled_engines();
        assert_eq!(enabled.len(), 1);
        assert!(enabled.contains(&"ddg"));
    }
}

/// Lightpanda browser backend integration tests.
///
/// Tests run automatically — no `--ignored` flag needed.
/// Each test checks whether Lightpanda is available before running;
/// if the binary is not found it prints a skip message and returns (passes vacuously).
///
/// To make all tests exercise real assertions:
///   - Install lightpanda and put it in PATH, or
///   - Set `A3S_LIGHTPANDA_EXECUTABLE=/path/to/binary`, or
///   - Install it explicitly with `a3s install use/browser`.
///
/// Run with:
///   cargo test --features lightpanda --test integration -- lightpanda --nocapture
#[cfg(feature = "lightpanda")]
mod lightpanda_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::fixture_server::FixtureServer;
    use a3s_search::{
        browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig, BrowserProvider},
        engines::Google,
        PageFetcher, WaitStrategy,
    };
    use a3s_use_browser::detect_lightpanda;

    // ─── helpers ────────────────────────────────────────────────────────────

    /// Find the Lightpanda binary without triggering an auto-download.
    ///
    /// Check order is owned by A3S Browser: environment, PATH, then its managed cache.
    /// Returns `None` if the binary is not locally available.
    fn find_lp_binary() -> Option<std::path::PathBuf> {
        detect_lightpanda()
    }

    /// Build a `BrowserPool` wired to Lightpanda.
    ///
    /// Returns `None` — and prints a skip message — if the binary is not locally available.
    /// Pass `max_tabs` to control concurrency.
    fn try_lp_pool(max_tabs: usize) -> Option<Arc<BrowserPool>> {
        let path = find_lp_binary()?;
        Some(Arc::new(BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(path),
            max_tabs,
            ..Default::default()
        })))
    }

    /// Log a skip message and return early from the calling test function.
    macro_rules! require_lp {
        ($pool_var:ident) => {
            let Some($pool_var) = try_lp_pool(3) else {
                println!(
                     "SKIP: Lightpanda binary not found. \
                     Install it, set A3S_LIGHTPANDA_EXECUTABLE=/path, or run `a3s install use/browser`."
                );
                return;
            };
        };
        ($pool_var:ident, max_tabs = $n:expr) => {
            let Some($pool_var) = try_lp_pool($n) else {
                println!(
                     "SKIP: Lightpanda binary not found. \
                     Install it, set A3S_LIGHTPANDA_EXECUTABLE=/path, or run `a3s install use/browser`."
                );
                return;
            };
        };
    }

    /// Fetch `url` with a given wait strategy, print a size/snippet, and return the HTML.
    async fn fetch_with(pool: Arc<BrowserPool>, url: &str, wait: WaitStrategy) -> String {
        let fetcher = BrowserFetcher::new(pool).with_wait(wait);
        match fetcher.fetch(url).await {
            Ok(html) => {
                println!(
                    "  fetched {} bytes from {}\n  snippet: {}...",
                    html.len(),
                    url,
                    html[..html.len().min(120)].replace('\n', " ")
                );
                html
            }
            Err(e) => {
                println!("  fetch failed: {}", e);
                String::new()
            }
        }
    }

    /// `detect_lightpanda()` must honour the A3S executable override.
    #[test]
    fn test_detect_lightpanda_env_var() {
        if let Ok(path) = std::env::var("A3S_LIGHTPANDA_EXECUTABLE") {
            let result = detect_lightpanda();
            assert!(
                result.is_some(),
                "detect_lightpanda() must find the A3S override"
            );
            assert_eq!(result.unwrap().to_string_lossy(), path);
        } else {
            println!("A3S_LIGHTPANDA_EXECUTABLE not set — detection test is a no-op");
        }
    }

    // ─── browser pool lifecycle ─────────────────────────────────────────────

    /// The pool must start Lightpanda, acquire a browser, and shut down cleanly.
    #[tokio::test]
    async fn test_lightpanda_pool_start_and_shutdown() {
        require_lp!(pool);

        let browser = pool.warm_up().await;
        assert!(
            browser.is_ok(),
            "warm_up() must succeed: {:?}",
            browser.err()
        );
        println!("  browser acquired ok");

        drop(browser);
        pool.shutdown().await;
        println!("  shutdown completed without panic");
    }

    /// `shutdown()` without any prior warm-up must not panic.
    #[tokio::test]
    async fn test_lightpanda_pool_shutdown_without_acquire() {
        require_lp!(pool);
        pool.shutdown().await;
        println!("  clean shutdown with no prior acquire: ok");
    }

    /// Double `shutdown()` must be idempotent.
    #[tokio::test]
    async fn test_lightpanda_pool_double_shutdown() {
        require_lp!(pool);
        let _ = pool.warm_up().await;
        pool.shutdown().await;
        pool.shutdown().await;
        println!("  double shutdown: ok");
    }

    /// Multiple warm-ups reuse the provider without exposing its implementation handle.
    #[tokio::test]
    async fn test_lightpanda_pool_warm_up_is_idempotent() {
        require_lp!(pool);

        pool.warm_up().await.expect("first warm-up");
        pool.warm_up().await.expect("second warm-up");
        pool.shutdown().await;
        println!("  repeated warm-up reused the provider: ok");
    }

    // ─── basic page fetch ───────────────────────────────────────────────────

    /// Fetch `example.com` — the canonical minimal test page.
    /// Verifies the full path: spawn → CDP connect → navigate → extract HTML.
    #[tokio::test]
    async fn test_lightpanda_fetch_example_com() {
        require_lp!(pool);

        let html = fetch_with(pool.clone(), "https://example.com", WaitStrategy::Load).await;

        assert!(!html.is_empty(), "HTML must not be empty");
        assert!(
            html.contains("Example Domain"),
            "example.com HTML must contain 'Example Domain', got: {}",
            &html[..html.len().min(500)]
        );

        pool.shutdown().await;
    }

    /// Fetch a deterministic localhost page through the installed Lightpanda binary.
    #[tokio::test]
    async fn test_lightpanda_fetch_local_html_fixture() {
        require_lp!(pool);
        let server = FixtureServer::start(
            "<!DOCTYPE html><html><body><main>LIGHTPANDA_LOCAL_FIXTURE</main></body></html>",
        );

        let html = fetch_with(pool.clone(), &server.url, WaitStrategy::Load).await;

        assert!(!html.is_empty(), "HTML must not be empty");
        assert!(
            html.contains("LIGHTPANDA_LOCAL_FIXTURE"),
            "Response must contain the fixture marker, got: {}",
            &html[..html.len().min(500)]
        );

        pool.shutdown().await;
    }

    // ─── wait strategies ────────────────────────────────────────────────────

    /// `WaitStrategy::Delay { ms }` must wait at least `ms` milliseconds.
    #[tokio::test]
    async fn test_lightpanda_wait_strategy_delay() {
        require_lp!(pool);
        let start = std::time::Instant::now();

        let html = fetch_with(
            pool.clone(),
            "https://example.com",
            WaitStrategy::Delay { ms: 500 },
        )
        .await;

        let elapsed = start.elapsed();
        println!("  elapsed: {}ms", elapsed.as_millis());

        assert!(
            !html.is_empty(),
            "HTML must not be empty with Delay strategy"
        );
        assert!(
            elapsed >= Duration::from_millis(500),
            "Delay of 500ms must be respected, got {}ms",
            elapsed.as_millis()
        );

        pool.shutdown().await;
    }

    /// `WaitStrategy::NetworkIdle { idle_ms }` must wait at least `idle_ms` after load.
    #[tokio::test]
    async fn test_lightpanda_wait_strategy_network_idle() {
        require_lp!(pool);
        let start = std::time::Instant::now();

        let html = fetch_with(
            pool.clone(),
            "https://example.com",
            WaitStrategy::NetworkIdle { idle_ms: 300 },
        )
        .await;

        let elapsed = start.elapsed();
        println!("  elapsed: {}ms", elapsed.as_millis());

        assert!(
            !html.is_empty(),
            "HTML must not be empty with NetworkIdle strategy"
        );
        assert!(
            elapsed >= Duration::from_millis(300),
            "NetworkIdle idle_ms must be respected"
        );

        pool.shutdown().await;
    }

    /// Lightpanda command rendering cannot promise selector wait semantics.
    #[tokio::test]
    async fn test_lightpanda_selector_wait_is_explicitly_unsupported() {
        require_lp!(pool);

        let result = BrowserFetcher::new(Arc::clone(&pool))
            .with_wait(WaitStrategy::Selector {
                css: "h1".to_string(),
                timeout_ms: 5000,
            })
            .with_retries(0, 0)
            .fetch("https://example.com")
            .await;

        let error = result.expect_err("Lightpanda selector waits must fail explicitly");
        assert!(error.to_string().contains("use.browser.unsupported"));

        pool.shutdown().await;
    }

    // ─── custom user agent ──────────────────────────────────────────────────

    /// Lightpanda must reject an exact UA override instead of silently ignoring it.
    #[tokio::test]
    async fn test_lightpanda_exact_user_agent_is_explicitly_unsupported() {
        require_lp!(pool);
        let custom_ua = "A3S-SearchBot/1.0 (lightpanda test)";

        let fetcher = BrowserFetcher::new(pool.clone())
            .with_wait(WaitStrategy::Load)
            .with_user_agent(custom_ua);

        let error = fetcher
            .fetch("https://example.com")
            .await
            .expect_err("Lightpanda exact user-agent overrides must fail explicitly");
        assert!(error.to_string().contains("use.browser.unsupported"));

        pool.shutdown().await;
    }

    // ─── concurrency ────────────────────────────────────────────────────────

    /// Three concurrent fetches (max_tabs=3) must all complete without deadlock.
    #[tokio::test]
    async fn test_lightpanda_concurrent_fetches() {
        require_lp!(pool);
        let start = std::time::Instant::now();

        let urls = [
            "https://example.com",
            "https://httpbin.org/html",
            "https://www.iana.org/domains/reserved",
        ];

        let handles: Vec<_> = urls
            .iter()
            .map(|url| {
                let fetcher = BrowserFetcher::new(Arc::clone(&pool)).with_wait(WaitStrategy::Load);
                let url = url.to_string();
                tokio::spawn(async move { fetcher.fetch(&url).await })
            })
            .collect();

        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.expect("task must not panic"))
            .collect();

        let elapsed = start.elapsed();
        let successes = results.iter().filter(|r| r.is_ok()).count();
        println!(
            "  3 concurrent fetches: {}/3 succeeded in {}ms",
            successes,
            elapsed.as_millis()
        );

        // Lightpanda has limited concurrent-tab support; require at least 1 success
        // to confirm the pool doesn't deadlock. A full 3/3 is expected with Chrome.
        assert!(
            successes >= 1,
            "At least 1/3 concurrent fetches must succeed (deadlock check), got {}/3",
            successes
        );

        pool.shutdown().await;
    }

    /// With `max_tabs=1`, two concurrent fetches must serialize (total ≥ 2× delay).
    #[tokio::test]
    async fn test_lightpanda_semaphore_limits_concurrency() {
        require_lp!(pool, max_tabs = 1);
        let start = std::time::Instant::now();

        let f1 = BrowserFetcher::new(Arc::clone(&pool)).with_wait(WaitStrategy::Delay { ms: 300 });
        let f2 = BrowserFetcher::new(Arc::clone(&pool)).with_wait(WaitStrategy::Delay { ms: 300 });

        let (r1, r2) = tokio::join!(
            f1.fetch("https://example.com"),
            f2.fetch("https://example.com"),
        );

        let elapsed = start.elapsed();
        println!(
            "  sequential total: {}ms (expected ≥600ms)",
            elapsed.as_millis()
        );

        assert!(
            elapsed >= Duration::from_millis(600),
            "max_tabs=1 must force 2×300ms fetches to serialize, got {}ms",
            elapsed.as_millis()
        );
        assert!(r1.is_ok(), "First fetch: {:?}", r1.err());
        assert!(r2.is_ok(), "Second fetch: {:?}", r2.err());

        pool.shutdown().await;
    }

    // ─── search engine integration ──────────────────────────────────────────

    /// End-to-end Google search via Lightpanda.
    ///
    /// Note: Google may serve a CAPTCHA to headless browsers — 0 results is a
    /// known Lightpanda/Google anti-bot issue, not a code bug.
    #[tokio::test]
    async fn test_lightpanda_google_search() {
        use super::test_engine;

        require_lp!(pool);

        let fetcher: Arc<dyn PageFetcher> = Arc::new(
            BrowserFetcher::new(Arc::clone(&pool)).with_wait(WaitStrategy::Selector {
                css: "div.g".to_string(),
                timeout_ms: 8000,
            }),
        );
        let engine = Google::new(fetcher);

        let results = test_engine(engine, "rust programming language").await;

        if results.is_empty() {
            println!(
                "  Google returned 0 results — likely CAPTCHA/anti-bot. \
                 Known Lightpanda limitation; not a test failure."
            );
        } else {
            println!("  Google via Lightpanda: {} results", results.len());
            for (i, r) in results.iter().take(3).enumerate() {
                println!("    {}. {} — {}", i + 1, r.title, r.url);
            }
        }

        pool.shutdown().await;
    }

    // ─── error handling ─────────────────────────────────────────────────────

    /// An invalid URL must not panic — either an Err or empty HTML is acceptable.
    #[tokio::test]
    async fn test_lightpanda_fetch_invalid_url() {
        require_lp!(pool);

        let fetcher = BrowserFetcher::new(pool.clone()).with_wait(WaitStrategy::Load);
        let result = fetcher
            .fetch("https://this-domain-does-not-exist.invalid")
            .await;

        match &result {
            Err(e) => println!("  returned error for invalid URL (expected): {}", e),
            Ok(html) => println!("  returned {} bytes for invalid URL", html.len()),
        }
        // Either outcome is fine — the important thing is no panic.

        pool.shutdown().await;
    }

    /// A non-existent binary path must produce a clear `Browser` error.
    #[tokio::test]
    async fn test_lightpanda_missing_binary_error() {
        // This test doesn't need a real binary — it intentionally passes a bad path.
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig {
            provider: BrowserProvider::LightpandaExecutable(
                "/nonexistent/path/to/lightpanda".into(),
            ),
            ..Default::default()
        }));

        let result = pool.warm_up().await;
        assert!(
            result.is_err(),
            "Acquiring browser with missing binary must return Err"
        );
        let err = result.unwrap_err().to_string();
        println!("  error: {}", err);
        assert!(
            err.to_lowercase().contains("lightpanda")
                || err.contains("spawn")
                || err.contains("Failed"),
            "Error must mention the failure cause: {}",
            err
        );
    }

    // ─── HTML content verification ──────────────────────────────────────────

    /// Rendered HTML must begin with `<!DOCTYPE` or `<html`.
    #[tokio::test]
    async fn test_lightpanda_returns_valid_html_structure() {
        require_lp!(pool);

        let html = fetch_with(pool.clone(), "https://example.com", WaitStrategy::Load).await;
        let trimmed = html.trim_start().to_lowercase();

        assert!(
            trimmed.starts_with("<!doctype") || trimmed.starts_with("<html"),
            "Rendered HTML must start with <!DOCTYPE or <html, got: {}",
            &trimmed[..trimmed.len().min(80)]
        );
        assert!(
            html.to_lowercase().contains("</html>"),
            "Rendered HTML must contain closing </html> tag"
        );

        pool.shutdown().await;
    }

    /// Rendered HTML must include `<head>` and `<body>` sections.
    #[tokio::test]
    async fn test_lightpanda_html_has_head_and_body() {
        require_lp!(pool);

        let html = fetch_with(pool.clone(), "https://example.com", WaitStrategy::Load).await;
        let lower = html.to_lowercase();

        assert!(lower.contains("<head"), "HTML must contain <head>");
        assert!(lower.contains("<body"), "HTML must contain <body>");

        pool.shutdown().await;
    }
}

mod meta_search_tests {
    use a3s_search::{
        engines::{DuckDuckGo, Wikipedia},
        Search, SearchQuery,
    };

    #[tokio::test]
    async fn test_meta_search_multiple_engines() {
        require_network!();
        let mut search = Search::new();
        search.add_engine(DuckDuckGo::new());
        search.add_engine(Wikipedia::new());

        let query = SearchQuery::new("rust programming language");
        let results = search.search(query).await.unwrap();

        println!(
            "Meta search returned {} results in {}ms",
            results.count, results.duration_ms
        );

        for (i, result) in results.items().iter().take(5).enumerate() {
            println!(
                "  {}. {} (engines: {:?}, score: {:.2})",
                i + 1,
                result.title,
                result.engines,
                result.score
            );
        }

        assert!(
            !results.items().is_empty(),
            "Meta search should return results"
        );
    }

    #[tokio::test]
    async fn test_meta_search_chinese() {
        require_network!();
        use a3s_search::engines::{So360, Sogou};

        let mut search = Search::new();
        search.add_engine(Sogou::new());
        search.add_engine(So360::new());

        let query = SearchQuery::new("Rust 编程语言");
        let results = search.search(query).await.unwrap();

        println!(
            "Chinese meta search returned {} results in {}ms",
            results.count, results.duration_ms
        );

        for (i, result) in results.items().iter().take(5).enumerate() {
            println!(
                "  {}. {} (engines: {:?})",
                i + 1,
                result.title,
                result.engines
            );
        }
    }
}

mod proxy_pool_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use a3s_search::proxy::{
        spawn_auto_refresh, ProxyConfig, ProxyPool, ProxyProvider, ProxyStrategy,
    };
    use a3s_search::PooledHttpFetcher;
    use async_trait::async_trait;

    /// A mock provider that returns a fixed list and tracks call count.
    struct CountingProvider {
        proxies: Vec<ProxyConfig>,
        call_count: Arc<tokio::sync::Mutex<u32>>,
    }

    #[async_trait]
    impl ProxyProvider for CountingProvider {
        async fn fetch_proxies(&self) -> a3s_search::Result<Vec<ProxyConfig>> {
            let mut count = self.call_count.lock().await;
            *count += 1;
            Ok(self.proxies.clone())
        }

        fn refresh_interval(&self) -> Duration {
            Duration::from_millis(100) // fast refresh for testing
        }
    }

    #[test]
    fn test_proxy_pool_enabled_toggle() {
        let pool = Arc::new(ProxyPool::with_proxies(vec![ProxyConfig::new(
            "10.0.0.1", 8080,
        )]));

        assert!(pool.is_enabled());
        pool.set_enabled(false);
        assert!(!pool.is_enabled());
        pool.set_enabled(true);
        assert!(pool.is_enabled());
    }

    #[tokio::test]
    async fn test_proxy_pool_disabled_returns_none() {
        let pool = Arc::new(ProxyPool::with_proxies(vec![ProxyConfig::new(
            "10.0.0.1", 8080,
        )]));

        // Enabled — should return proxy
        let proxy = pool.get_proxy().await;
        assert!(proxy.is_some());

        // Disabled — should return None
        pool.set_enabled(false);
        let proxy = pool.get_proxy().await;
        assert!(proxy.is_none());

        // Re-enabled — should return proxy again
        pool.set_enabled(true);
        let proxy = pool.get_proxy().await;
        assert!(proxy.is_some());
    }

    #[tokio::test]
    async fn test_proxy_pool_round_robin_rotation() {
        let pool = ProxyPool::with_proxies(vec![
            ProxyConfig::new("10.0.0.1", 8080),
            ProxyConfig::new("10.0.0.2", 8080),
            ProxyConfig::new("10.0.0.3", 8080),
        ]);

        let p1 = pool.get_proxy().await.unwrap();
        let p2 = pool.get_proxy().await.unwrap();
        let p3 = pool.get_proxy().await.unwrap();
        let p4 = pool.get_proxy().await.unwrap();

        assert_eq!(p1.host, "10.0.0.1");
        assert_eq!(p2.host, "10.0.0.2");
        assert_eq!(p3.host, "10.0.0.3");
        assert_eq!(p4.host, "10.0.0.1"); // wraps around
    }

    #[tokio::test]
    async fn test_proxy_pool_random_strategy() {
        let pool = ProxyPool::with_proxies(vec![
            ProxyConfig::new("10.0.0.1", 8080),
            ProxyConfig::new("10.0.0.2", 8080),
        ])
        .with_strategy(ProxyStrategy::Random);

        let proxy = pool.get_proxy().await.unwrap();
        assert!(proxy.host == "10.0.0.1" || proxy.host == "10.0.0.2");
    }

    #[tokio::test]
    async fn test_auto_refresh_calls_provider() {
        let call_count = Arc::new(tokio::sync::Mutex::new(0u32));
        let provider = CountingProvider {
            proxies: vec![ProxyConfig::new("10.0.0.1", 8080)],
            call_count: Arc::clone(&call_count),
        };

        let pool = Arc::new(ProxyPool::with_provider(provider));
        let handle = spawn_auto_refresh(Arc::clone(&pool));

        // Wait for initial refresh + at least one periodic refresh
        tokio::time::sleep(Duration::from_millis(250)).await;

        let count = *call_count.lock().await;
        assert!(
            count >= 2,
            "Provider should be called at least twice, got {}",
            count
        );
        assert_eq!(pool.len().await, 1);

        handle.abort();
    }

    #[test]
    fn test_pooled_http_fetcher_creation() {
        let pool = Arc::new(ProxyPool::with_proxies(vec![ProxyConfig::new(
            "10.0.0.1", 8080,
        )]));
        let _fetcher = PooledHttpFetcher::new(pool);
    }

    #[test]
    fn test_pooled_http_fetcher_with_timeout() {
        let pool = Arc::new(ProxyPool::new());
        let fetcher = PooledHttpFetcher::new(pool).with_timeout(Duration::from_secs(15));
        // Should not panic
        drop(fetcher);
    }
}

/// Full-text enrichment integration tests.
///
/// The happy-path test serves a fixture page from a localhost ephemeral port and runs
/// the real `HttpFetcher` -> Readability extraction path end to end. The server thread
/// is stopped and joined on drop, so no socket or thread is leaked. These tests need no
/// internet — they only talk to `127.0.0.1`.
mod full_text_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use a3s_search::{enrich_full_text, HttpFetcher, PageFetcher, SearchResult, SearchResults};

    use super::fixture_server::FixtureServer;

    const ARTICLE_HTML: &str = r#"<html><head><title>Fixture</title></head><body>
        <nav>home about UNIQUE_NAV login signup</nav>
        <article>
          <h1>Fixture Article</h1>
          <p>UNIQUE_BODY This is the principal article body that Readability should
          keep. It contains several full sentences so the algorithm scores it as the
          main content rather than discarding it as page noise.</p>
          <p>A second substantial paragraph adds more textual weight to the article
          node, ensuring the extractor selects this block over the surrounding nav,
          sidebar, and footer chrome present on the page.</p>
          <p>A third paragraph keeps the text density high and the link ratio low,
          which is what the scoring heuristic rewards when choosing the dominant
          content container of an HTML document.</p>
        </article>
        <footer>UNIQUE_FOOTER copyright 2026</footer>
    </body></html>"#;

    #[tokio::test]
    async fn enrich_extracts_main_text_over_real_http() {
        let server = FixtureServer::start(ARTICLE_HTML);

        let mut results = SearchResults::new();
        results.add_result(SearchResult::new(
            server.url.clone(),
            "Fixture",
            "the snippet",
        ));

        let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        enrich_full_text(&mut results, fetcher, 4, Duration::from_secs(5)).await;

        let item = &results.items()[0];
        let body = item
            .full_text
            .as_deref()
            .expect("full_text should be populated from the fixture page");
        assert!(body.contains("UNIQUE_BODY"), "kept the article body");
        assert!(!body.contains("UNIQUE_NAV"), "dropped the nav");
        assert!(!body.contains("UNIQUE_FOOTER"), "dropped the footer");
        assert_eq!(item.content, "the snippet", "snippet must be preserved");
    }

    #[tokio::test]
    async fn enrich_skips_unreachable_host_and_keeps_snippet() {
        // Port 1 on loopback refuses immediately -> the fetch errors, no internet needed.
        let mut results = SearchResults::new();
        results.add_result(SearchResult::new(
            "http://127.0.0.1:1/",
            "Dead",
            "snippet stays",
        ));

        let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        enrich_full_text(&mut results, fetcher, 4, Duration::from_millis(800)).await;

        let item = &results.items()[0];
        assert!(
            item.full_text.is_none(),
            "failed fetch leaves full_text None"
        );
        assert_eq!(item.content, "snippet stays");
    }
}

/// Real-network full-text enrichment tests.
///
/// These hit the live internet, so they are guarded by `require_network!` and skip
/// gracefully when offline. They exercise the real `HttpFetcher` -> Readability path
/// against stable, content-rich pages.
mod full_text_real_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use a3s_search::{
        engines::Wikipedia, enrich_full_text, HttpFetcher, PageFetcher, Search, SearchQuery,
        SearchResult, SearchResults,
    };

    /// Real fetch + extraction against a stable, content-rich page (no search engine involved).
    /// Wikipedia serves server-rendered HTML to a normal User-Agent, so this is reliable.
    #[tokio::test]
    async fn real_enrich_extracts_wikipedia_article() {
        require_network!();

        let mut results = SearchResults::new();
        results.add_result(SearchResult::new(
            "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "Rust",
            "the snippet",
        ));

        let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        enrich_full_text(&mut results, fetcher, 4, Duration::from_secs(20)).await;

        let item = &results.items()[0];
        match item.full_text.as_deref() {
            Some(text) => {
                println!("  extracted {} chars of article body", text.len());
                assert!(
                    text.len() > 1000,
                    "a Wikipedia article body should be substantial, got {} chars",
                    text.len()
                );
                assert!(
                    text.contains("Rust"),
                    "extracted text should mention the subject"
                );
                assert!(
                    !text.contains("Jump to navigation"),
                    "Readability should have dropped the nav chrome"
                );
                assert_eq!(item.content, "the snippet", "snippet must be preserved");
            }
            None => {
                // A transient network / anti-bot hiccup must not hard-fail the suite.
                println!("  full_text not populated (transient fetch failure) — skipping asserts");
            }
        }
    }

    /// Full pipeline: real meta-search -> enrich the real result URLs -> extract main text.
    #[tokio::test]
    async fn real_search_then_enrich_pipeline() {
        require_network!();

        let mut search = Search::new();
        search.add_engine(Wikipedia::new());
        let mut results = search
            .search(SearchQuery::new("rust programming language"))
            .await
            .expect("search should not error when online");

        println!("  search returned {} results", results.items().len());
        if results.items().is_empty() {
            println!("  no search results (transient) — skipping enrich");
            return;
        }

        let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        enrich_full_text(&mut results, fetcher, 3, Duration::from_secs(20)).await;

        let enriched = results
            .items()
            .iter()
            .filter(|r| r.full_text.is_some())
            .count();
        println!(
            "  enriched {}/{} results with extracted full text",
            enriched,
            results.items().len()
        );
        for r in results.items().iter().take(3) {
            let chars = r.full_text.as_deref().map(str::len).unwrap_or(0);
            println!("    {} -> {} chars  [{}]", r.title, chars, r.url);
        }

        // Every result keeps a snippet or gains full text — enrichment never destroys data.
        for r in results.items() {
            assert!(!r.content.is_empty() || r.full_text.is_some());
        }
        // Wikipedia article pages are reliably fetchable and extractable.
        assert!(
            enriched >= 1,
            "at least one real result should yield extracted full text"
        );
    }
}

/// Headless-browser full-text enrichment tests (`--features headless`).
///
/// The A/B test is the key proof: a page whose article body is injected by JavaScript is
/// invisible to a plain HTTP fetch but rendered and extracted by the headless browser.
/// Both are guarded by `require_network!` and skip gracefully if Chrome cannot launch.
#[cfg(feature = "headless")]
mod full_text_headless_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use a3s_search::{
        browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
        enrich_full_text, HttpFetcher, PageFetcher, SearchResult, SearchResults, WaitStrategy,
    };

    use super::fixture_server::FixtureServer;

    /// The `<article>` body is written by JavaScript at runtime, so only a JS-executing
    /// browser can see it; a plain HTTP fetch receives an empty `<div id="app">`.
    const JS_PAGE_HTML: &str = r#"<!DOCTYPE html><html><head><title>JS Fixture</title></head><body>
        <nav>UNIQUE_NAV home about contact</nav>
        <div id="app"></div>
        <script>
          document.getElementById('app').innerHTML =
            '<article><h1>JS Rendered</h1>' +
            '<p>JS_BODY_MARKER This article body is injected by JavaScript at runtime, so a ' +
            'plain HTTP fetch never sees it; only a headless browser that executes scripts can ' +
            'render and extract it. Several sentences ensure Readability keeps it as the main ' +
            'content rather than discarding it as noise.</p>' +
            '<p>A second paragraph adds textual weight so the article node wins over the nav.</p>' +
            '<p>A third paragraph keeps the density high and the link ratio low for the scorer.</p>' +
            '</article>';
        </script>
    </body></html>"#;

    /// Returns a warmed browser pool, or `None` (with a printed skip) if Chrome cannot launch.
    async fn browser_pool_or_skip() -> Option<Arc<BrowserPool>> {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        match pool.warm_up().await {
            Ok(_) => Some(pool),
            Err(e) => {
                println!("  SKIP: headless browser unavailable: {e}");
                None
            }
        }
    }

    /// Core proof that headless rendering matters: JS-injected content that plain HTTP
    /// cannot see is rendered and extracted by the headless browser.
    #[tokio::test]
    async fn headless_renders_js_body_that_http_misses() {
        require_network!();
        let Some(pool) = browser_pool_or_skip().await else {
            return;
        };

        let server = FixtureServer::start(JS_PAGE_HTML);

        // (a) Plain HTTP runs no JavaScript -> never sees the injected article.
        let mut http_results = SearchResults::new();
        http_results.add_result(SearchResult::new(server.url.clone(), "JS", "snippet"));
        let http: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
        enrich_full_text(&mut http_results, http, 2, Duration::from_secs(10)).await;
        let http_text = http_results.items()[0].full_text.as_deref().unwrap_or("");
        assert!(
            !http_text.contains("JS_BODY_MARKER"),
            "plain HTTP must not see JS-injected content, got: {http_text:?}"
        );

        // (b) The headless browser executes the script, so the article IS extracted.
        let mut hl_results = SearchResults::new();
        hl_results.add_result(SearchResult::new(server.url.clone(), "JS", "snippet"));
        let headless: Arc<dyn PageFetcher> = Arc::new(
            BrowserFetcher::new(Arc::clone(&pool)).with_wait(WaitStrategy::Delay { ms: 300 }),
        );
        enrich_full_text(&mut hl_results, headless, 2, Duration::from_secs(30)).await;

        let hl_text = hl_results.items()[0]
            .full_text
            .as_deref()
            .expect("headless should extract the JS-rendered article");
        println!(
            "  headless extracted {} chars from the JS-rendered page",
            hl_text.len()
        );
        assert!(
            hl_text.contains("JS_BODY_MARKER"),
            "headless must render the JS-injected body"
        );
        assert!(!hl_text.contains("UNIQUE_NAV"), "nav chrome still dropped");
        assert_eq!(
            hl_results.items()[0].content,
            "snippet",
            "snippet preserved"
        );

        pool.shutdown().await;
    }

    /// Headless path against the real web: render a live article and extract its main text.
    #[tokio::test]
    async fn headless_enrich_extracts_real_article() {
        require_network!();
        let Some(pool) = browser_pool_or_skip().await else {
            return;
        };

        let mut results = SearchResults::new();
        results.add_result(SearchResult::new(
            "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "Rust",
            "the snippet",
        ));
        let fetcher: Arc<dyn PageFetcher> =
            Arc::new(BrowserFetcher::new(Arc::clone(&pool)).with_wait(WaitStrategy::Load));
        enrich_full_text(&mut results, fetcher, 2, Duration::from_secs(40)).await;

        match results.items()[0].full_text.as_deref() {
            Some(text) => {
                println!("  headless extracted {} chars from Wikipedia", text.len());
                assert!(text.len() > 1000, "real article body should be substantial");
                assert!(text.contains("Rust"));
                assert_eq!(results.items()[0].content, "the snippet");
            }
            None => {
                println!(
                    "  full_text not populated (transient headless failure) — skipping asserts"
                )
            }
        }

        pool.shutdown().await;
    }
}
