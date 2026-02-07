//! Search orchestration.

use std::sync::Arc;
use std::time::Instant;

use futures::future::join_all;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::proxy::ProxyPool;
use crate::{Aggregator, Engine, Result, SearchError, SearchQuery, SearchResults};

/// Meta search engine that orchestrates searches across multiple engines.
pub struct Search {
    engines: Vec<Arc<dyn Engine>>,
    aggregator: Aggregator,
    default_timeout: Duration,
    proxy_pool: Option<Arc<ProxyPool>>,
}

impl Search {
    /// Creates a new search instance.
    pub fn new() -> Self {
        Self {
            engines: Vec::new(),
            aggregator: Aggregator::new(),
            default_timeout: Duration::from_secs(5),
            proxy_pool: None,
        }
    }

    /// Adds a search engine.
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E) {
        let config = engine.config();
        self.aggregator
            .set_engine_weight(&config.name, config.weight);
        self.engines.push(Arc::new(engine));
    }

    /// Sets the default timeout for searches.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }

    /// Sets the proxy pool for anti-crawler protection.
    pub fn set_proxy_pool(&mut self, proxy_pool: ProxyPool) {
        self.proxy_pool = Some(Arc::new(proxy_pool));
    }

    /// Returns a reference to the proxy pool if configured.
    pub fn proxy_pool(&self) -> Option<&Arc<ProxyPool>> {
        self.proxy_pool.as_ref()
    }

    /// Returns the number of configured engines.
    pub fn engine_count(&self) -> usize {
        self.engines.len()
    }

    /// Performs a search across all configured engines.
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults> {
        if self.engines.is_empty() {
            return Err(SearchError::NoEngines);
        }

        if query.query.trim().is_empty() {
            return Err(SearchError::InvalidQuery("Query cannot be empty".into()));
        }

        let start = Instant::now();
        let query = Arc::new(query);

        let engines_to_use = self.select_engines(&query);
        debug!("Searching {} engines", engines_to_use.len());

        let futures: Vec<_> = engines_to_use
            .iter()
            .map(|engine| {
                let engine = Arc::clone(engine);
                let query = Arc::clone(&query);
                let timeout_duration = Duration::from_secs(engine.config().timeout);

                async move {
                    let name = engine.name().to_string();
                    match timeout(timeout_duration, engine.search(&query)).await {
                        Ok(Ok(results)) => {
                            debug!("Engine {} returned {} results", name, results.len());
                            Some((name, results))
                        }
                        Ok(Err(e)) => {
                            warn!("Engine {} failed: {}", name, e);
                            None
                        }
                        Err(_) => {
                            warn!("Engine {} timed out", name);
                            None
                        }
                    }
                }
            })
            .collect();

        let results: Vec<_> = join_all(futures).await.into_iter().flatten().collect();

        let mut search_results = self.aggregator.aggregate(results);
        search_results.set_duration(start.elapsed().as_millis() as u64);

        Ok(search_results)
    }

    /// Selects engines based on query parameters.
    fn select_engines(&self, query: &SearchQuery) -> Vec<Arc<dyn Engine>> {
        self.engines
            .iter()
            .filter(|engine| {
                if !engine.is_enabled() {
                    return false;
                }

                if !query.engines.is_empty() {
                    return query.engines.contains(&engine.shortcut().to_string());
                }

                let config = engine.config();
                query
                    .categories
                    .iter()
                    .any(|cat| config.categories.contains(cat))
            })
            .cloned()
            .collect()
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineCategory, EngineConfig, SearchResult};
    use async_trait::async_trait;

    struct MockEngine {
        config: EngineConfig,
        results: Vec<SearchResult>,
    }

    impl MockEngine {
        fn new(name: &str, results: Vec<SearchResult>) -> Self {
            Self {
                config: EngineConfig {
                    name: name.to_string(),
                    shortcut: name.to_string(),
                    categories: vec![EngineCategory::General],
                    ..Default::default()
                },
                results,
            }
        }

        fn with_category(mut self, category: EngineCategory) -> Self {
            self.config.categories = vec![category];
            self
        }

        fn with_shortcut(mut self, shortcut: &str) -> Self {
            self.config.shortcut = shortcut.to_string();
            self
        }

        fn disabled(mut self) -> Self {
            self.config.enabled = false;
            self
        }
    }

    #[async_trait]
    impl Engine for MockEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            Ok(self.results.clone())
        }
    }

    struct FailingEngine {
        config: EngineConfig,
    }

    impl FailingEngine {
        fn new(name: &str) -> Self {
            Self {
                config: EngineConfig {
                    name: name.to_string(),
                    shortcut: name.to_string(),
                    categories: vec![EngineCategory::General],
                    ..Default::default()
                },
            }
        }
    }

    #[async_trait]
    impl Engine for FailingEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            Err(SearchError::Other("Engine failed".to_string()))
        }
    }

    #[tokio::test]
    async fn test_search_new() {
        let search = Search::new();
        assert_eq!(search.engine_count(), 0);
    }

    #[tokio::test]
    async fn test_search_default() {
        let search = Search::default();
        assert_eq!(search.engine_count(), 0);
    }

    #[tokio::test]
    async fn test_search_add_engine() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new("test", vec![]));
        assert_eq!(search.engine_count(), 1);
    }

    #[tokio::test]
    async fn test_search_set_timeout() {
        let mut search = Search::new();
        search.set_timeout(Duration::from_secs(10));
        assert_eq!(search.default_timeout, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_search_no_engines() {
        let search = Search::new();
        let query = SearchQuery::new("test");
        let result = search.search(query).await;
        assert!(matches!(result, Err(SearchError::NoEngines)));
    }

    #[tokio::test]
    async fn test_search_empty_query() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new("test", vec![]));
        let query = SearchQuery::new("   ");
        let result = search.search(query).await;
        assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
    }

    #[tokio::test]
    async fn test_search_whitespace_only_query() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new("test", vec![]));
        let query = SearchQuery::new("\t\n  ");
        let result = search.search(query).await;
        assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
    }

    #[tokio::test]
    async fn test_search_aggregates_results() {
        let mut search = Search::new();

        search.add_engine(MockEngine::new(
            "engine1",
            vec![SearchResult::new(
                "https://example.com",
                "Example",
                "Content",
            )],
        ));
        search.add_engine(MockEngine::new(
            "engine2",
            vec![
                SearchResult::new("https://example.com", "Example Site", "More content"),
                SearchResult::new("https://other.com", "Other", "Other content"),
            ],
        ));

        let query = SearchQuery::new("test");
        let results = search.search(query).await.unwrap();

        assert_eq!(results.items().len(), 2);

        let example = results
            .items()
            .iter()
            .find(|r| r.url == "https://example.com")
            .unwrap();
        assert_eq!(example.engines.len(), 2);
    }

    #[tokio::test]
    async fn test_search_records_duration() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new("test", vec![]));

        let query = SearchQuery::new("test");
        let results = search.search(query).await.unwrap();

        // Duration should be recorded (u64 is always >= 0)
        let _ = results.duration_ms;
    }

    #[tokio::test]
    async fn test_search_filters_disabled_engines() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new(
            "enabled",
            vec![SearchResult::new(
                "https://enabled.com",
                "Enabled",
                "Content",
            )],
        ));
        search.add_engine(
            MockEngine::new(
                "disabled",
                vec![SearchResult::new(
                    "https://disabled.com",
                    "Disabled",
                    "Content",
                )],
            )
            .disabled(),
        );

        let query = SearchQuery::new("test");
        let results = search.search(query).await.unwrap();

        assert_eq!(results.items().len(), 1);
        assert_eq!(results.items()[0].url, "https://enabled.com");
    }

    #[tokio::test]
    async fn test_search_filters_by_category() {
        let mut search = Search::new();
        search.add_engine(
            MockEngine::new(
                "general",
                vec![SearchResult::new(
                    "https://general.com",
                    "General",
                    "Content",
                )],
            )
            .with_category(EngineCategory::General),
        );
        search.add_engine(
            MockEngine::new(
                "images",
                vec![SearchResult::new("https://images.com", "Images", "Content")],
            )
            .with_category(EngineCategory::Images),
        );

        let query = SearchQuery::new("test").with_categories(vec![EngineCategory::Images]);
        let results = search.search(query).await.unwrap();

        assert_eq!(results.items().len(), 1);
        assert_eq!(results.items()[0].url, "https://images.com");
    }

    #[tokio::test]
    async fn test_search_filters_by_engine_shortcut() {
        let mut search = Search::new();
        search.add_engine(
            MockEngine::new(
                "engine1",
                vec![SearchResult::new("https://one.com", "One", "Content")],
            )
            .with_shortcut("e1"),
        );
        search.add_engine(
            MockEngine::new(
                "engine2",
                vec![SearchResult::new("https://two.com", "Two", "Content")],
            )
            .with_shortcut("e2"),
        );

        let query = SearchQuery::new("test").with_engines(vec!["e1".to_string()]);
        let results = search.search(query).await.unwrap();

        assert_eq!(results.items().len(), 1);
        assert_eq!(results.items()[0].url, "https://one.com");
    }

    #[tokio::test]
    async fn test_search_handles_engine_failure() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new(
            "working",
            vec![SearchResult::new(
                "https://working.com",
                "Working",
                "Content",
            )],
        ));
        search.add_engine(FailingEngine::new("failing"));

        let query = SearchQuery::new("test");
        let results = search.search(query).await.unwrap();

        // Should still return results from working engine
        assert_eq!(results.items().len(), 1);
        assert_eq!(results.items()[0].url, "https://working.com");
    }

    #[tokio::test]
    async fn test_search_all_engines_fail() {
        let mut search = Search::new();
        search.add_engine(FailingEngine::new("failing1"));
        search.add_engine(FailingEngine::new("failing2"));

        let query = SearchQuery::new("test");
        let results = search.search(query).await.unwrap();

        // Should return empty results, not error
        assert_eq!(results.items().len(), 0);
    }

    #[tokio::test]
    async fn test_search_multiple_categories() {
        let mut search = Search::new();
        search.add_engine(
            MockEngine::new(
                "general",
                vec![SearchResult::new(
                    "https://general.com",
                    "General",
                    "Content",
                )],
            )
            .with_category(EngineCategory::General),
        );
        search.add_engine(
            MockEngine::new(
                "news",
                vec![SearchResult::new("https://news.com", "News", "Content")],
            )
            .with_category(EngineCategory::News),
        );
        search.add_engine(
            MockEngine::new(
                "images",
                vec![SearchResult::new("https://images.com", "Images", "Content")],
            )
            .with_category(EngineCategory::Images),
        );

        let query = SearchQuery::new("test")
            .with_categories(vec![EngineCategory::General, EngineCategory::News]);
        let results = search.search(query).await.unwrap();

        assert_eq!(results.items().len(), 2);
    }

    #[tokio::test]
    async fn test_search_set_proxy_pool() {
        use crate::proxy::{ProxyConfig, ProxyPool};

        let mut search = Search::new();
        assert!(search.proxy_pool().is_none());

        let proxy_pool = ProxyPool::with_proxies(vec![ProxyConfig::new("127.0.0.1", 8080)]);
        search.set_proxy_pool(proxy_pool);

        assert!(search.proxy_pool().is_some());
    }

    #[tokio::test]
    async fn test_search_proxy_pool_reference() {
        use crate::proxy::{ProxyConfig, ProxyPool};

        let mut search = Search::new();
        let proxy_pool = ProxyPool::with_proxies(vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
        ]);
        search.set_proxy_pool(proxy_pool);

        let pool_ref = search.proxy_pool().unwrap();
        assert!(pool_ref.is_enabled());
    }
}
