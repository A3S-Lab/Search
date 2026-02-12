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
