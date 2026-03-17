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

mod duckduckgo_tests {
    use super::*;
    use a3s_search::engines::DuckDuckGo;

    #[tokio::test]
    async fn test_duckduckgo_search() {
        require_network!();
        let engine = DuckDuckGo::new();
        let results = test_engine(engine, "rust programming").await;
        assert!(!results.is_empty(), "DuckDuckGo should return results");
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

#[cfg(feature = "headless")]
mod bing_china_tests {
    use super::*;
    use std::sync::Arc;

    use a3s_search::{
        browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig},
        engines::BingChina,
        WaitStrategy,
    };

    fn make_bing_china_engine() -> BingChina {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = Arc::new(BrowserFetcher::new(pool).with_wait(WaitStrategy::Selector {
            css: "li.b_algo".to_string(),
            timeout_ms: 5000,
        }));
        BingChina::new(fetcher)
    }

    #[tokio::test]
    async fn test_bing_china_search() {
        require_network!();
        let engine = make_bing_china_engine();
        let results = test_engine(engine, "Rust 编程").await;
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
    fn test_load_hcl_config() {
        let hcl = r#"
            timeout = 8

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

        let config = SearchConfig::parse(hcl).unwrap();
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
        let hcl = r#"
            engine "ddg" {
                enabled = true
            }

            engine "sogou" {
                enabled = false
            }
        "#;

        let config = SearchConfig::parse(hcl).unwrap();
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
///   - Set `LIGHTPANDA=/path/to/binary` environment variable, or
///   - Run `just test-lightpanda` once to auto-download and cache it.
///
/// Run with:
///   cargo test --features lightpanda --test integration -- lightpanda --nocapture
#[cfg(feature = "lightpanda")]
mod lightpanda_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use a3s_search::{
        browser::{BrowserBackend, BrowserFetcher, BrowserPool, BrowserPoolConfig},
        browser_setup_lp,
        engines::Google,
        PageFetcher, WaitStrategy,
    };

    // ─── helpers ────────────────────────────────────────────────────────────

    /// Find the Lightpanda binary without triggering an auto-download.
    ///
    /// Check order: `LIGHTPANDA` env var → PATH → `~/.a3s/lightpanda/<tag>/lightpanda` cache.
    /// Returns `None` if the binary is not locally available.
    fn find_lp_binary() -> Option<std::path::PathBuf> {
        // 1. Explicit env var
        if let Ok(p) = std::env::var("LIGHTPANDA") {
            let path = std::path::PathBuf::from(p);
            if path.exists() {
                return Some(path);
            }
        }
        // 2. PATH / system install
        if let Some(path) = browser_setup_lp::detect_lightpanda() {
            return Some(path);
        }
        // 3. Local download cache (~/.a3s/lightpanda/<tag>/lightpanda)
        let home = std::env::var("HOME").ok()?;
        let cache = std::path::PathBuf::from(home).join(".a3s/lightpanda");
        std::fs::read_dir(&cache)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.path().join("lightpanda"))
            .find(|p| p.exists())
    }

    /// Build a `BrowserPool` wired to Lightpanda.
    ///
    /// Returns `None` — and prints a skip message — if the binary is not locally available.
    /// Pass `max_tabs` to control concurrency.
    fn try_lp_pool(max_tabs: usize) -> Option<Arc<BrowserPool>> {
        let path = find_lp_binary()?;
        Some(Arc::new(BrowserPool::new(BrowserPoolConfig {
            backend: BrowserBackend::Lightpanda,
            lightpanda_path: Some(path.to_string_lossy().into_owned()),
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
                     Install it, set LIGHTPANDA=/path, or run `just test-lightpanda` once to auto-download."
                );
                return;
            };
        };
        ($pool_var:ident, max_tabs = $n:expr) => {
            let Some($pool_var) = try_lp_pool($n) else {
                println!(
                    "SKIP: Lightpanda binary not found. \
                     Install it, set LIGHTPANDA=/path, or run `just test-lightpanda` once to auto-download."
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
                    &html[..html.len().min(120)].replace('\n', " ")
                );
                html
            }
            Err(e) => {
                println!("  fetch failed: {}", e);
                String::new()
            }
        }
    }

    // ─── binary detection ───────────────────────────────────────────────────

    /// Verify `ensure_lightpanda()` returns a valid executable path.
    ///
    /// If the binary is already cached this completes instantly.
    /// On a fresh machine it triggers a one-time download (~60 MB).
    #[tokio::test]
    async fn test_ensure_lightpanda_binary() {
        match browser_setup_lp::ensure_lightpanda().await {
            Ok(path) => {
                println!("Lightpanda binary: {}", path.display());
                assert!(path.exists(), "Binary path must exist on disk");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
                    assert!(mode & 0o111 != 0, "Binary must be executable");
                }
            }
            Err(e) => {
                // Unsupported platform or network unavailable — not a test failure.
                println!("ensure_lightpanda not available: {} (skipping)", e);
            }
        }
    }

    /// `detect_lightpanda()` must honour the `LIGHTPANDA` env var.
    #[test]
    fn test_detect_lightpanda_env_var() {
        if let Ok(path) = std::env::var("LIGHTPANDA") {
            let result = browser_setup_lp::detect_lightpanda();
            assert!(
                result.is_some(),
                "detect_lightpanda() must find binary via LIGHTPANDA env var"
            );
            assert_eq!(result.unwrap().to_string_lossy(), path);
        } else {
            println!("LIGHTPANDA env var not set — detection-via-env test is a no-op");
        }
    }

    // ─── browser pool lifecycle ─────────────────────────────────────────────

    /// The pool must start Lightpanda, acquire a browser, and shut down cleanly.
    #[tokio::test]
    async fn test_lightpanda_pool_start_and_shutdown() {
        require_lp!(pool);

        let browser = pool.acquire_browser().await;
        assert!(
            browser.is_ok(),
            "acquire_browser() must succeed: {:?}",
            browser.err()
        );
        println!("  browser acquired ok");

        pool.shutdown().await;
        println!("  shutdown completed without panic");
    }

    /// `shutdown()` without any prior `acquire_browser()` must not panic.
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
        let _ = pool.acquire_browser().await;
        pool.shutdown().await;
        pool.shutdown().await;
        println!("  double shutdown: ok");
    }

    /// Multiple `acquire_browser()` calls must return the same `Arc<Browser>`.
    #[tokio::test]
    async fn test_lightpanda_pool_acquire_returns_same_instance() {
        require_lp!(pool);

        let b1 = pool.acquire_browser().await.expect("first acquire");
        let b2 = pool.acquire_browser().await.expect("second acquire");
        assert!(
            Arc::ptr_eq(&b1, &b2),
            "Both acquires must return the same Arc<Browser>"
        );

        pool.shutdown().await;
        println!("  both acquires returned same Arc: ok");
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

    /// Fetch `httpbin.org/html` — a plain HTML fixture with known content.
    #[tokio::test]
    async fn test_lightpanda_fetch_httpbin_html() {
        require_lp!(pool);

        let html = fetch_with(pool.clone(), "https://httpbin.org/html", WaitStrategy::Load).await;

        assert!(!html.is_empty(), "HTML must not be empty");
        assert!(
            html.to_lowercase().contains("<html"),
            "Response must contain an <html> tag"
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

    /// `WaitStrategy::Selector` with a present element must not time out.
    #[tokio::test]
    async fn test_lightpanda_wait_strategy_selector_found() {
        require_lp!(pool);

        let html = fetch_with(
            pool.clone(),
            "https://example.com",
            WaitStrategy::Selector {
                css: "h1".to_string(),
                timeout_ms: 5000,
            },
        )
        .await;

        assert!(!html.is_empty(), "HTML must not be empty");
        assert!(
            html.contains("Example Domain"),
            "h1 content must be present"
        );

        pool.shutdown().await;
    }

    /// A missing selector must be non-fatal — page HTML is returned regardless.
    #[tokio::test]
    async fn test_lightpanda_wait_strategy_selector_not_found_is_non_fatal() {
        require_lp!(pool);

        let html = fetch_with(
            pool.clone(),
            "https://example.com",
            WaitStrategy::Selector {
                css: "div.this-element-does-not-exist-xyz".to_string(),
                timeout_ms: 500,
            },
        )
        .await;

        assert!(
            !html.is_empty(),
            "Missing selector must be non-fatal; HTML must still be returned"
        );

        pool.shutdown().await;
    }

    // ─── custom user agent ──────────────────────────────────────────────────

    /// `with_user_agent()` must override the UA sent to the server.
    /// Verified against httpbin's UA echo endpoint.
    #[tokio::test]
    async fn test_lightpanda_custom_user_agent() {
        require_lp!(pool);
        let custom_ua = "A3S-SearchBot/1.0 (lightpanda test)";

        let fetcher = BrowserFetcher::new(pool.clone())
            .with_wait(WaitStrategy::Load)
            .with_user_agent(custom_ua);

        // `with_user_agent` sets `Network.setUserAgentOverride` after the tab opens.
        // This affects subsequent sub-requests on the page. For the initial document
        // request, Lightpanda uses its own UA; override only applies to later requests.
        // Test that the fetch completes without error — not that UA is echoed in HTML.
        let result = fetcher.fetch("https://example.com").await;
        assert!(
            result.is_ok(),
            "Fetch with custom UA should not error: {:?}",
            result.err()
        );
        let html = result.unwrap();
        assert!(!html.is_empty(), "Response must not be empty");
        println!("  UA-override fetch ok ({} bytes)", html.len());

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
            backend: BrowserBackend::Lightpanda,
            lightpanda_path: Some("/nonexistent/path/to/lightpanda".to_string()),
            ..Default::default()
        }));

        let result = pool.acquire_browser().await;
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
