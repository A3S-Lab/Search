//! Search orchestration.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::future::join_all;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::{
    Aggregator, Engine, EngineFailure, HealthConfig, HealthMonitor, Metrics, Result, SearchError,
    SearchQuery, SearchResults,
};

/// Meta search engine that orchestrates searches across multiple engines.
pub struct Search {
    engines: Vec<Arc<dyn Engine>>,
    aggregator: Aggregator,
    timeout_override: Option<Duration>,
    health: Mutex<HealthMonitor>,
    metrics: Option<Arc<Metrics>>,
}

impl Search {
    /// Creates a new search instance.
    pub fn new() -> Self {
        Self {
            engines: Vec::new(),
            aggregator: Aggregator::new(),
            timeout_override: None,
            health: Mutex::new(HealthMonitor::default()),
            metrics: None,
        }
    }

    /// Creates a new search instance with a custom health configuration.
    pub fn with_health_config(config: HealthConfig) -> Self {
        Self {
            engines: Vec::new(),
            aggregator: Aggregator::new(),
            timeout_override: None,
            health: Mutex::new(HealthMonitor::new(config)),
            metrics: None,
        }
    }

    /// Attaches a metrics registry used to record per-engine search attempts.
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Sets or clears the metrics registry used by this search instance.
    pub fn set_metrics(&mut self, metrics: Option<Arc<Metrics>>) {
        self.metrics = metrics;
    }

    /// Returns the configured metrics registry, if any.
    pub fn metrics(&self) -> Option<Arc<Metrics>> {
        self.metrics.as_ref().map(Arc::clone)
    }

    /// Adds a search engine.
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E) {
        let config = engine.config();
        self.aggregator
            .set_engine_weight(&config.name, config.weight);
        self.engines.push(Arc::new(engine));
    }

    /// Overrides the timeout applied to each engine during searches.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout_override = Some(timeout);
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
                let metrics = self.metrics.as_ref().map(Arc::clone);
                let timeout_duration = self
                    .timeout_override
                    .unwrap_or_else(|| Duration::from_secs(engine.config().timeout));

                async move {
                    let name = engine.name().to_string();
                    let engine_start = Instant::now();
                    match timeout(timeout_duration, engine.search_output(&query)).await {
                        Ok(Ok(output)) => {
                            if let Some(metrics) = metrics.as_ref() {
                                metrics.record_success(engine_start.elapsed());
                            }
                            debug!("Engine {} returned {} results", name, output.results.len());
                            Ok((name, output))
                        }
                        Ok(Err(e)) => {
                            if let Some(metrics) = metrics.as_ref() {
                                metrics.record_failure(e.kind(), e.is_transient());
                            }
                            warn!("Engine {} failed: {}", name, e);
                            let affects_health = !e.is_client_error();
                            let mut failure = EngineFailure::new(name, e.kind(), e.to_string())
                                .with_transient(e.is_transient());
                            if let SearchError::Provider(provider_error) = &e {
                                failure = failure.with_provider(provider_error.provider());
                            }
                            Err((failure, affects_health))
                        }
                        Err(_) => {
                            if let Some(metrics) = metrics.as_ref() {
                                metrics.record_failure(SearchError::Timeout.kind(), true);
                            }
                            warn!("Engine {} timed out", name);
                            Err((
                                EngineFailure::new(name, "timeout", "timed out")
                                    .with_transient(true),
                                true,
                            ))
                        }
                    }
                }
            })
            .collect();

        let all_results: Vec<_> = join_all(futures).await;

        let mut engine_errors = Vec::new();
        let outputs: Vec<_> = all_results
            .into_iter()
            .filter_map(|r| match r {
                Ok(pair) => Some(pair),
                Err(err) => {
                    engine_errors.push(err);
                    None
                }
            })
            .collect();

        // Update health state for each engine
        if let Ok(mut health) = self.health.lock() {
            for (name, _) in &outputs {
                health.record_success(name);
            }
            for (failure, affects_health) in &engine_errors {
                if *affects_health {
                    health.record_failure(&failure.engine);
                }
            }
        }

        let mut result_sets = Vec::with_capacity(outputs.len());
        let mut suggestions = Vec::new();
        let mut answers = Vec::new();
        let mut images = Vec::new();
        let mut reports = Vec::new();
        for (name, output) in outputs {
            result_sets.push((name, output.results));
            suggestions.extend(output.suggestions);
            answers.extend(output.answers);
            images.extend(output.images);
            reports.extend(output.reports);
        }

        let mut search_results = self.aggregator.aggregate(result_sets);
        for suggestion in suggestions {
            search_results.add_suggestion(suggestion);
        }
        for answer in answers {
            search_results.add_answer(answer);
        }
        for image in images {
            search_results.add_image(image);
        }
        for report in reports {
            search_results.add_report(report);
        }
        for (failure, _) in engine_errors {
            search_results.add_failure(failure);
        }
        search_results.set_duration(start.elapsed().as_millis() as u64);

        Ok(search_results)
    }

    /// Selects engines based on query parameters, filtering out suspended engines.
    fn select_engines(&self, query: &SearchQuery) -> Vec<Arc<dyn Engine>> {
        let health = self.health.lock().ok();

        self.engines
            .iter()
            .filter(|engine| {
                if !engine.is_enabled() {
                    return false;
                }

                // Skip suspended engines
                if let Some(ref h) = health {
                    if h.is_suspended(engine.name()) {
                        debug!("Engine {} is suspended, skipping", engine.name());
                        return false;
                    }
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
    use crate::{
        EngineCategory, EngineConfig, EngineOutput, Metrics, ProviderError, ProviderErrorKind,
        SearchImage, SearchReport, SearchResult,
    };
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

    struct RichEngine {
        config: EngineConfig,
    }

    #[async_trait]
    impl Engine for RichEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult::new(
                "https://example.com",
                "Example",
                "Snippet",
            )])
        }

        async fn search_output(&self, query: &SearchQuery) -> Result<EngineOutput> {
            let results = self.search(query).await?;
            Ok(EngineOutput::new(results)
                .with_answer("direct answer")
                .with_suggestion("suggested query")
                .with_image(
                    SearchImage::new("https://example.com/image.png")
                        .with_description("query image"),
                )
                .with_report(
                    SearchReport::new("Rich")
                        .with_provider("rich")
                        .with_request_id("req-1"),
                ))
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

    struct ClientErrorEngine {
        config: EngineConfig,
    }

    #[async_trait]
    impl Engine for ClientErrorEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            Err(ProviderError::new(
                "test-provider",
                ProviderErrorKind::Authentication,
                "credential is missing",
            )
            .into())
        }
    }

    struct SlowEngine {
        config: EngineConfig,
        delay: Duration,
    }

    impl SlowEngine {
        fn new(name: &str, delay: Duration) -> Self {
            Self {
                config: EngineConfig {
                    name: name.to_string(),
                    shortcut: name.to_string(),
                    categories: vec![EngineCategory::General],
                    timeout: 30,
                    ..Default::default()
                },
                delay,
            }
        }
    }

    #[async_trait]
    impl Engine for SlowEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            tokio::time::sleep(self.delay).await;
            Ok(vec![SearchResult::new(
                "https://slow.example",
                "Slow",
                "Delayed result",
            )])
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
    async fn test_search_with_health_config() {
        let config = HealthConfig {
            max_failures: 5,
            suspend_duration: Duration::from_secs(120),
        };
        let search = Search::with_health_config(config);
        assert_eq!(search.engine_count(), 0);
    }

    #[tokio::test]
    async fn test_search_add_engine() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new("test", vec![]));
        assert_eq!(search.engine_count(), 1);
    }

    #[tokio::test]
    async fn test_search_preserves_rich_engine_output() {
        let mut search = Search::new();
        search.add_engine(RichEngine {
            config: EngineConfig {
                name: "Rich".to_string(),
                shortcut: "rich".to_string(),
                ..Default::default()
            },
        });

        let results = search.search(SearchQuery::new("test")).await.unwrap();

        assert_eq!(results.items().len(), 1);
        assert_eq!(results.answers(), &["direct answer"]);
        assert_eq!(results.suggestions(), &["suggested query"]);
        assert_eq!(results.images().len(), 1);
        assert_eq!(
            results.images()[0].description.as_deref(),
            Some("query image")
        );
        assert_eq!(results.reports().len(), 1);
        assert_eq!(results.reports()[0].request_id.as_deref(), Some("req-1"));
    }

    #[tokio::test]
    async fn test_search_set_timeout() {
        let mut search = Search::new();
        search.set_timeout(Duration::from_millis(10));
        search.add_engine(SlowEngine::new("slow", Duration::from_millis(100)));

        let results = search.search(SearchQuery::new("test")).await.unwrap();

        assert!(results.items().is_empty());
        assert_eq!(results.errors().len(), 1);
        assert_eq!(results.errors()[0].0, "slow");
        assert!(results.errors()[0].1.contains("timed out"));
        assert_eq!(results.failures()[0].kind, "timeout");
        assert!(results.failures()[0].transient);
    }

    #[tokio::test]
    async fn test_extreme_timeout_does_not_overflow() {
        let mut search = Search::new();
        search.set_timeout(Duration::MAX);
        search.add_engine(MockEngine::new(
            "fast",
            vec![SearchResult::new(
                "https://example.com",
                "Example",
                "Content",
            )],
        ));

        let results = search.search(SearchQuery::new("test")).await.unwrap();

        assert_eq!(results.items().len(), 1);
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
        search.add_engine(SlowEngine::new("slow", Duration::from_millis(5)));

        let query = SearchQuery::new("test");
        let results = search.search(query).await.unwrap();

        assert!(results.duration_ms > 0);
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

        // Should record the engine error
        assert_eq!(results.errors().len(), 1);
        assert_eq!(results.errors()[0].0, "failing");
        assert!(results.errors()[0].1.contains("Engine failed"));
        assert_eq!(results.failures()[0].kind, "other");
        assert!(!results.failures()[0].transient);
    }

    #[tokio::test]
    async fn test_search_records_metrics() {
        let metrics = Arc::new(Metrics::new());
        let mut search = Search::new().with_metrics(Arc::clone(&metrics));
        search.add_engine(MockEngine::new(
            "working",
            vec![SearchResult::new(
                "https://working.com",
                "Working",
                "Content",
            )],
        ));
        search.add_engine(FailingEngine::new("failing"));

        let results = search.search(SearchQuery::new("test")).await.unwrap();
        assert_eq!(results.items().len(), 1);
        assert_eq!(results.errors().len(), 1);

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.successes, 1);
        assert_eq!(snapshot.failures, 1);
        assert_eq!(snapshot.error_counts.get("other"), Some(&1));
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

        // Should record both engine errors
        assert_eq!(results.errors().len(), 2);
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
    async fn test_health_records_success_on_engine_result() {
        let mut search = Search::new();
        search.add_engine(MockEngine::new(
            "engine1",
            vec![SearchResult::new("https://example.com", "Title", "Content")],
        ));

        let query = SearchQuery::new("test");
        search.search(query).await.unwrap();

        let health = search.health.lock().unwrap();
        assert_eq!(health.failure_count("engine1"), 0);
        assert!(!health.is_suspended("engine1"));
    }

    #[tokio::test]
    async fn test_health_records_failure_on_engine_error() {
        let mut search = Search::new();
        search.add_engine(FailingEngine::new("bad_engine"));

        let query = SearchQuery::new("test");
        search.search(query).await.unwrap();

        let health = search.health.lock().unwrap();
        assert_eq!(health.failure_count("bad_engine"), 1);
    }

    #[tokio::test]
    async fn test_health_does_not_suspend_provider_for_client_configuration_errors() {
        let mut search = Search::with_health_config(HealthConfig {
            max_failures: 1,
            suspend_duration: Duration::from_secs(3600),
        });
        search.add_engine(ClientErrorEngine {
            config: EngineConfig {
                name: "client-error".to_string(),
                shortcut: "client-error".to_string(),
                ..Default::default()
            },
        });

        let first = search.search(SearchQuery::new("first")).await.unwrap();
        let second = search.search(SearchQuery::new("second")).await.unwrap();

        assert_eq!(first.errors().len(), 1);
        assert_eq!(second.errors().len(), 1);
        assert_eq!(first.failures()[0].kind, "provider_authentication");
        assert_eq!(
            first.failures()[0].provider.as_deref(),
            Some("test-provider")
        );
        assert!(!first.failures()[0].transient);
        let health = search.health.lock().unwrap();
        assert_eq!(health.failure_count("client-error"), 0);
        assert!(!health.is_suspended("client-error"));
    }

    #[tokio::test]
    async fn test_health_suspends_after_repeated_failures() {
        let config = HealthConfig {
            max_failures: 2,
            suspend_duration: Duration::from_secs(3600),
        };
        let mut search = Search::with_health_config(config);
        search.add_engine(FailingEngine::new("flaky"));
        search.add_engine(MockEngine::new(
            "stable",
            vec![SearchResult::new("https://stable.com", "Stable", "Content")],
        ));

        // First failure
        let query = SearchQuery::new("test1");
        search.search(query).await.unwrap();

        // Second failure — should trigger suspension
        let query = SearchQuery::new("test2");
        search.search(query).await.unwrap();

        // Third search — flaky engine should be suspended
        let query = SearchQuery::new("test3");
        let results = search.search(query).await.unwrap();

        // Only stable engine should have been used
        assert_eq!(results.items().len(), 1);
        assert_eq!(results.items()[0].url, "https://stable.com");
        assert!(results.errors().is_empty());

        let health = search.health.lock().unwrap();
        assert!(health.is_suspended("flaky"));
    }

    #[tokio::test]
    async fn test_health_success_resets_failure_count() {
        let config = HealthConfig {
            max_failures: 3,
            suspend_duration: Duration::from_secs(60),
        };
        let mut search = Search::with_health_config(config);
        search.add_engine(MockEngine::new(
            "engine1",
            vec![SearchResult::new("https://example.com", "Title", "Content")],
        ));

        // Manually inject failures
        {
            let mut health = search.health.lock().unwrap();
            health.record_failure("engine1");
            health.record_failure("engine1");
            assert_eq!(health.failure_count("engine1"), 2);
        }

        // Successful search should reset
        let query = SearchQuery::new("test");
        search.search(query).await.unwrap();

        let health = search.health.lock().unwrap();
        assert_eq!(health.failure_count("engine1"), 0);
    }
}
