//! Integration tests for search engines using real HTTP requests.
//!
//! These tests are marked with `#[ignore]` by default because they require
//! network access and may be slow or flaky.
//!
//! Run with: `cargo test -p a3s-search --test integration -- --ignored`

use a3s_search::{Engine, SearchQuery, SearchResult};

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
    #[ignore]
    async fn test_duckduckgo_search() {
        let engine = DuckDuckGo::new();
        let results = test_engine(engine, "rust programming").await;
        assert!(!results.is_empty(), "DuckDuckGo should return results");
    }

    #[tokio::test]
    #[ignore]
    async fn test_duckduckgo_chinese_query() {
        let engine = DuckDuckGo::new();
        let results = test_engine(engine, "Rust 编程语言").await;
        // May or may not return results for Chinese queries
        println!("Chinese query returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_wikipedia_search() {
        let engine = Wikipedia::new();
        let results = test_engine(engine, "rust programming language").await;
        assert!(!results.is_empty(), "Wikipedia should return results");
    }

    #[tokio::test]
    #[ignore]
    async fn test_wikipedia_chinese() {
        let engine = Wikipedia::new().with_language("zh");
        let results = test_engine(engine, "Rust").await;
        println!("Chinese Wikipedia returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_brave_search() {
        let engine = Brave::new();
        let results = test_engine(engine, "rust programming").await;
        // Brave may block automated requests
        println!("Brave returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_sogou_search() {
        let engine = Sogou::new();
        let results = test_engine(engine, "Rust 编程").await;
        println!("Sogou returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_so360_search() {
        let engine = So360::new();
        let results = test_engine(engine, "Rust 编程").await;
        println!("360 Search returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_google_search() {
        let engine = make_google_engine();
        let results = test_engine(engine, "rust programming").await;
        assert!(!results.is_empty(), "Google should return results");
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_baidu_search() {
        let engine = make_baidu_engine();
        let results = test_engine(engine, "Rust 编程").await;
        println!("Baidu returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_bing_china_search() {
        let engine = make_bing_china_engine();
        let results = test_engine(engine, "Rust 编程").await;
        println!("Bing China returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_bing_search() {
        let engine = Bing::new();
        let results = test_engine(engine, "rust programming").await;
        println!("Bing returned {} results", results.len());
    }

    #[tokio::test]
    #[ignore]
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
    #[ignore]
    async fn test_health_monitor_with_real_engines() {
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

mod meta_search_tests {
    use a3s_search::{
        engines::{DuckDuckGo, Wikipedia},
        Search, SearchQuery,
    };

    #[tokio::test]
    #[ignore]
    async fn test_meta_search_multiple_engines() {
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
    #[ignore]
    async fn test_meta_search_chinese() {
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
