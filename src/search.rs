//! Search orchestration.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::future::join_all;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::coalescer::{SearchCoalescingAdmission, SearchRequestKey};
use crate::{
    Aggregator, Bulkhead, CircuitBreaker, CircuitPermit, Engine, EngineFailure, EngineOutcome,
    EngineOutcomeKind, HealthConfig, HealthMonitor, Metrics, RankingConfig, Result,
    SearchCoalescer, SearchError, SearchQuery, SearchResults,
};

/// Meta search engine that orchestrates searches across multiple engines.
pub struct Search {
    engines: Vec<Arc<dyn Engine>>,
    aggregator: Aggregator,
    timeout_override: Option<Duration>,
    health: Mutex<HealthMonitor>,
    metrics: Option<Arc<Metrics>>,
    circuit_breaker: Option<CircuitBreaker>,
    bulkhead: Option<Bulkhead>,
    request_coalescer: Option<SearchCoalescer>,
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
            circuit_breaker: None,
            bulkhead: None,
            request_coalescer: None,
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
            circuit_breaker: None,
            bulkhead: None,
            request_coalescer: None,
        }
    }

    /// Attaches a metrics registry used to record per-engine search attempts.
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Uses a typed, domain-neutral rank-fusion policy.
    pub fn with_ranking_config(mut self, ranking: RankingConfig) -> Self {
        self.aggregator.set_ranking_config(ranking);
        self
    }

    /// Replaces the rank-fusion policy for later searches.
    pub fn set_ranking_config(&mut self, ranking: RankingConfig) {
        self.aggregator.set_ranking_config(ranking);
    }

    /// Returns the effective rank-fusion policy.
    pub fn ranking_config(&self) -> RankingConfig {
        self.aggregator.ranking_config()
    }

    /// Attaches shared circuit state that may be reused by other `Search`
    /// instances and later requests.
    pub fn with_circuit_breaker(mut self, circuit_breaker: CircuitBreaker) -> Self {
        self.circuit_breaker = Some(circuit_breaker);
        self
    }

    /// Sets or clears shared circuit state.
    pub fn set_circuit_breaker(&mut self, circuit_breaker: Option<CircuitBreaker>) {
        self.circuit_breaker = circuit_breaker;
    }

    /// Attaches shared, bounded per-engine concurrency isolation.
    pub fn with_bulkhead(mut self, bulkhead: Bulkhead) -> Self {
        self.bulkhead = Some(bulkhead);
        self
    }

    /// Sets or clears shared per-engine concurrency isolation.
    pub fn set_bulkhead(&mut self, bulkhead: Option<Bulkhead>) {
        self.bulkhead = bulkhead;
    }

    /// Attaches a shared registry that collapses identical concurrent searches.
    ///
    /// Completed flights are removed immediately, so this does not cache
    /// results. Share one registry only inside a compatible tenant,
    /// credential, endpoint, proxy, safe-search, freshness, and policy scope.
    pub fn with_request_coalescer(mut self, coalescer: SearchCoalescer) -> Self {
        self.request_coalescer = Some(coalescer);
        self
    }

    /// Sets or clears shared in-flight request coalescing.
    pub fn set_request_coalescer(&mut self, coalescer: Option<SearchCoalescer>) {
        self.request_coalescer = coalescer;
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
        let Some(coalescer) = self.request_coalescer.as_ref() else {
            return self.execute_search(query).await;
        };
        let key = SearchRequestKey::new(
            query.clone(),
            self.engines.iter().map(|engine| engine.config()),
            self.aggregator.ranking_config(),
            self.timeout_override,
        );

        loop {
            match coalescer.acquire(key.clone()) {
                SearchCoalescingAdmission::Leader(leader) => {
                    let result = self.execute_search(query.clone()).await;
                    if let Ok(results) = &result {
                        leader.complete(results.clone());
                    }
                    return result;
                }
                SearchCoalescingAdmission::Follower(flight) => {
                    if let Some(results) = flight.wait().await {
                        return Ok(results);
                    }
                }
                SearchCoalescingAdmission::Bypass => return self.execute_search(query).await,
            }
        }
    }

    async fn execute_search(&self, query: SearchQuery) -> Result<SearchResults> {
        let start = Instant::now();
        let query = Arc::new(query);

        let (engines_to_use, skipped_outcomes, skipped_failures) = self.select_engines(&query);
        debug!("Searching {} engines", engines_to_use.len());

        let futures: Vec<_> = engines_to_use
            .into_iter()
            .map(|attempt| {
                let engine = attempt.engine;
                let permit = attempt.permit;
                let query = Arc::clone(&query);
                let metrics = self.metrics.as_ref().map(Arc::clone);
                let bulkhead = self.bulkhead.clone();
                let timeout_duration = self
                    .timeout_override
                    .unwrap_or_else(|| Duration::from_secs(engine.config().timeout));

                async move {
                    let name = engine.name().to_string();
                    let shortcut = engine.shortcut().to_string();
                    let engine_start = Instant::now();
                    let _bulkhead_permit = match bulkhead {
                        None => None,
                        Some(bulkhead) => match bulkhead.acquire(&shortcut).await {
                            Ok(permit) => Some(permit),
                            Err(rejection) => {
                                if let Some(permit) = permit {
                                    permit.record_local_rejection();
                                }
                                if let Some(metrics) = metrics.as_ref() {
                                    metrics.record_failure(rejection.failure_kind(), true);
                                }
                                let failure = EngineFailure::new(
                                    name,
                                    rejection.failure_kind(),
                                    rejection.to_string(),
                                )
                                .with_transient(true);
                                let outcome = EngineOutcome::failed(
                                    shortcut,
                                    failure.clone(),
                                    EngineOutcomeKind::Rejected,
                                )
                                .with_duration(engine_start.elapsed());
                                return Err((failure, false, outcome));
                            }
                        },
                    };
                    match timeout(timeout_duration, engine.search_output(&query)).await {
                        Ok(Ok(output)) => {
                            let engine_duration = engine_start.elapsed();
                            if let Some(metrics) = metrics.as_ref() {
                                metrics.record_success(engine_duration);
                            }
                            debug!("Engine {} returned {} results", name, output.results.len());
                            let empty = output.results.is_empty()
                                && output.suggestions.is_empty()
                                && output.answers.is_empty()
                                && output.images.is_empty();
                            if let Some(permit) = permit {
                                if empty {
                                    permit.record_empty_with_duration(engine_duration);
                                } else {
                                    permit.record_success_with_duration(engine_duration);
                                }
                            }
                            let mut outcome = EngineOutcome::completed(
                                name.clone(),
                                shortcut,
                                if empty {
                                    EngineOutcomeKind::Empty
                                } else {
                                    EngineOutcomeKind::Success
                                },
                                output.results.len(),
                            )
                            .with_duration(engine_duration);
                            outcome.provider = output
                                .reports
                                .iter()
                                .find_map(|report| report.provider.clone());
                            Ok((name, output, outcome))
                        }
                        Ok(Err(e)) => {
                            let engine_duration = engine_start.elapsed();
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
                            if let Some(seconds) = e.retry_after_seconds() {
                                failure = failure.with_retry_after(seconds);
                            }
                            if let Some(permit) = permit {
                                permit.record_failure_with_duration(&failure, engine_duration);
                            }
                            let outcome = EngineOutcome::failed(
                                shortcut,
                                failure.clone(),
                                EngineOutcomeKind::Failure,
                            )
                            .with_duration(engine_duration);
                            Err((failure, affects_health, outcome))
                        }
                        Err(_) => {
                            let engine_duration = engine_start.elapsed();
                            if let Some(metrics) = metrics.as_ref() {
                                metrics.record_failure(SearchError::Timeout.kind(), true);
                            }
                            warn!("Engine {} timed out", name);
                            let failure = EngineFailure::new(name, "timeout", "timed out")
                                .with_transient(true);
                            if let Some(permit) = permit {
                                permit.record_failure_with_duration(&failure, engine_duration);
                            }
                            let outcome = EngineOutcome::failed(
                                shortcut,
                                failure.clone(),
                                EngineOutcomeKind::Timeout,
                            )
                            .with_duration(engine_duration);
                            Err((failure, true, outcome))
                        }
                    }
                }
            })
            .collect();

        let all_results: Vec<_> = join_all(futures).await;

        let mut engine_errors = skipped_failures
            .into_iter()
            .map(|failure| (failure, false))
            .collect::<Vec<_>>();
        let mut outcomes = skipped_outcomes;
        let outputs: Vec<_> = all_results
            .into_iter()
            .filter_map(|r| match r {
                Ok((name, output, outcome)) => {
                    outcomes.push(outcome);
                    Some((name, output))
                }
                Err((failure, affects_health, outcome)) => {
                    outcomes.push(outcome);
                    engine_errors.push((failure, affects_health));
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
        for outcome in outcomes {
            search_results.add_outcome(outcome);
        }
        for (failure, _) in engine_errors {
            search_results.add_failure(failure);
        }
        search_results.set_duration(start.elapsed().as_millis() as u64);

        Ok(search_results)
    }

    /// Selects engines based on query parameters, filtering out suspended engines.
    fn select_engines(
        &self,
        query: &SearchQuery,
    ) -> (Vec<EngineAttempt>, Vec<EngineOutcome>, Vec<EngineFailure>) {
        let health = self.health.lock().ok();
        let mut attempts = Vec::new();
        let mut outcomes = Vec::new();
        let mut failures = Vec::new();

        for engine in &self.engines {
            if !engine.is_enabled() {
                continue;
            }
            if !query.engines.is_empty() && !query.engines.contains(&engine.shortcut().to_string())
            {
                continue;
            }
            if query.engines.is_empty()
                && !query
                    .categories
                    .iter()
                    .any(|category| engine.config().categories.contains(category))
            {
                continue;
            }

            let shortcut = engine.shortcut().to_string();
            if health
                .as_ref()
                .is_some_and(|health| health.is_suspended(engine.name()))
            {
                debug!("Engine {} is suspended, skipping", engine.name());
                let failure = EngineFailure::new(
                    engine.name(),
                    "engine_suspended",
                    "local engine health monitor is open",
                )
                .with_transient(true);
                outcomes.push(EngineOutcome::failed(
                    shortcut,
                    failure.clone(),
                    EngineOutcomeKind::CircuitOpen,
                ));
                failures.push(failure);
                continue;
            }

            let permit = match self.circuit_breaker.as_ref() {
                None => None,
                Some(circuit_breaker) => match circuit_breaker.acquire(&shortcut) {
                    Ok(permit) => Some(permit),
                    Err(open) => {
                        let retry_after_seconds = duration_ceiling_seconds(open.retry_after);
                        let mut failure = EngineFailure::new(
                            engine.name(),
                            "circuit_open",
                            "shared engine circuit is open",
                        )
                        .with_transient(true);
                        if retry_after_seconds > 0 {
                            failure = failure.with_retry_after(retry_after_seconds);
                        }
                        outcomes.push(EngineOutcome::failed(
                            shortcut,
                            failure.clone(),
                            EngineOutcomeKind::CircuitOpen,
                        ));
                        failures.push(failure);
                        continue;
                    }
                },
            };
            attempts.push(EngineAttempt {
                engine: Arc::clone(engine),
                permit,
            });
        }

        (attempts, outcomes, failures)
    }
}

struct EngineAttempt {
    engine: Arc<dyn Engine>,
    permit: Option<CircuitPermit>,
}

fn duration_ceiling_seconds(duration: Duration) -> u64 {
    let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    millis.saturating_add(999) / 1_000
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

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

    struct CountingQuotaEngine {
        config: EngineConfig,
        calls: Arc<AtomicUsize>,
    }

    impl CountingQuotaEngine {
        fn new(name: &str, shortcut: &str, calls: Arc<AtomicUsize>) -> Self {
            Self {
                config: EngineConfig {
                    name: name.to_string(),
                    shortcut: shortcut.to_string(),
                    categories: vec![EngineCategory::General],
                    ..Default::default()
                },
                calls,
            }
        }
    }

    #[async_trait]
    impl Engine for CountingQuotaEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::new(
                "quota-provider",
                ProviderErrorKind::Quota,
                "quota exhausted",
            )
            .into())
        }
    }

    struct SlowEngine {
        config: EngineConfig,
        delay: Duration,
    }

    struct BlockingEngine {
        config: EngineConfig,
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl BlockingEngine {
        fn new(name: &str, started: Arc<Notify>, release: Arc<Notify>) -> Self {
            Self {
                config: EngineConfig {
                    name: name.to_string(),
                    shortcut: name.to_string(),
                    categories: vec![EngineCategory::General],
                    ..Default::default()
                },
                started,
                release,
            }
        }
    }

    #[async_trait]
    impl Engine for BlockingEngine {
        fn config(&self) -> &EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(vec![SearchResult::new(
                "https://example.com/result",
                "Result",
                "Content",
            )])
        }
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
    async fn test_search_records_conventional_engine_empty_outcome() {
        let mut search = Search::new().with_circuit_breaker(CircuitBreaker::default());
        search.add_engine(MockEngine::new("empty-http-engine", vec![]));

        let results = search
            .search(SearchQuery::new("generic empty result query"))
            .await
            .unwrap();

        assert!(results.items().is_empty());
        assert!(results.failures().is_empty());
        assert_eq!(results.outcomes().len(), 1);
        assert_eq!(results.outcomes()[0].kind, EngineOutcomeKind::Empty);
        assert_eq!(results.outcomes()[0].result_count, 0);
    }

    #[tokio::test]
    async fn test_distinct_search_instances_share_open_circuit_without_second_call() {
        let breaker = CircuitBreaker::new(crate::CircuitBreakerConfig {
            terminal_open_duration: Duration::from_secs(3_600),
            ..Default::default()
        });
        let calls = Arc::new(AtomicUsize::new(0));

        let mut first = Search::new().with_circuit_breaker(breaker.clone());
        first.add_engine(CountingQuotaEngine::new(
            "Quota API",
            "quota-api",
            Arc::clone(&calls),
        ));
        let first_results = first
            .search(SearchQuery::new("first generic query"))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(first_results.outcomes()[0].kind, EngineOutcomeKind::Failure);
        assert_eq!(first_results.failures()[0].kind, "provider_quota");

        let mut second = Search::new().with_circuit_breaker(breaker);
        second.add_engine(CountingQuotaEngine::new(
            "Quota API",
            "quota-api",
            Arc::clone(&calls),
        ));
        let second_results = second
            .search(SearchQuery::new("unrelated second query"))
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_results.outcomes().len(), 1);
        assert_eq!(
            second_results.outcomes()[0].kind,
            EngineOutcomeKind::CircuitOpen
        );
        assert_eq!(second_results.failures()[0].kind, "circuit_open");
        assert!(second_results.failures()[0]
            .retry_after_seconds
            .is_some_and(|seconds| seconds > 0));

        let json = serde_json::to_value(&second_results).unwrap();
        assert_eq!(json["outcomes"][0]["kind"], "circuit_open");
        assert_eq!(json["failures"][0]["kind"], "circuit_open");
        assert!(json["failures"][0]["retry_after_seconds"]
            .as_u64()
            .is_some_and(|seconds| seconds > 0));
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
        assert_eq!(results.outcomes().len(), 1);
        assert_eq!(results.outcomes()[0].kind, EngineOutcomeKind::Timeout);
        assert!(results.outcomes()[0].duration_ms >= 1);
        let json = serde_json::to_value(&results).unwrap();
        assert_eq!(json["outcomes"][0]["kind"], "timeout");
        assert!(json["outcomes"][0]["duration_ms"].as_u64().unwrap() >= 1);
    }

    #[tokio::test]
    async fn shared_bulkhead_rejects_excess_search_instances_without_opening_circuit() {
        let bulkhead = crate::Bulkhead::new(crate::BulkheadConfig {
            max_concurrent: 1,
            max_queued: 0,
            max_queue_wait: Duration::ZERO,
        });
        let circuit = CircuitBreaker::default();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());

        let mut first = Search::new()
            .with_bulkhead(bulkhead.clone())
            .with_circuit_breaker(circuit.clone());
        first.add_engine(BlockingEngine::new(
            "shared-engine",
            Arc::clone(&started),
            Arc::clone(&release),
        ));
        let first_task = tokio::spawn(async move {
            first
                .search(SearchQuery::new("first generic query"))
                .await
                .unwrap()
        });
        started.notified().await;

        let mut second = Search::new()
            .with_bulkhead(bulkhead.clone())
            .with_circuit_breaker(circuit.clone());
        second.add_engine(BlockingEngine::new(
            "shared-engine",
            Arc::new(Notify::new()),
            Arc::new(Notify::new()),
        ));
        let rejected = second
            .search(SearchQuery::new("second generic query"))
            .await
            .unwrap();

        assert_eq!(rejected.failures()[0].kind, "bulkhead_saturated");
        assert_eq!(rejected.outcomes()[0].kind, EngineOutcomeKind::Rejected);
        assert_eq!(bulkhead.snapshot("shared-engine").in_flight, 1);
        assert_eq!(
            circuit.snapshot("shared-engine").state,
            crate::CircuitState::Closed
        );

        release.notify_one();
        assert_eq!(first_task.await.unwrap().items().len(), 1);
    }

    #[tokio::test]
    async fn bulkhead_rejection_returns_half_open_probe_without_ejecting_upstream() {
        let bulkhead = crate::Bulkhead::new(crate::BulkheadConfig {
            max_concurrent: 1,
            max_queued: 0,
            max_queue_wait: Duration::ZERO,
        });
        let circuit = CircuitBreaker::new(crate::CircuitBreakerConfig {
            failure_threshold: 1,
            transient_open_duration: Duration::ZERO,
            open_jitter_ratio: 0.0,
            window: None,
            ..Default::default()
        });
        circuit.acquire("shared-engine").unwrap().record_failure(
            &EngineFailure::new("shared-engine", "provider_transport", "offline")
                .with_transient(true),
        );

        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut occupying = Search::new().with_bulkhead(bulkhead.clone());
        occupying.add_engine(BlockingEngine::new(
            "shared-engine",
            Arc::clone(&started),
            Arc::clone(&release),
        ));
        let occupying_task = tokio::spawn(async move {
            occupying
                .search(SearchQuery::new("occupying query"))
                .await
                .unwrap()
        });
        started.notified().await;

        let mut probing = Search::new()
            .with_bulkhead(bulkhead)
            .with_circuit_breaker(circuit.clone());
        probing.add_engine(MockEngine::new(
            "shared-engine",
            vec![SearchResult::new(
                "https://example.com/probe",
                "Probe",
                "Content",
            )],
        ));
        let rejected = probing
            .search(SearchQuery::new("probe query"))
            .await
            .unwrap();

        assert_eq!(rejected.failures()[0].kind, "bulkhead_saturated");
        assert_eq!(circuit.snapshot("shared-engine").ejection_count, 1);
        circuit
            .acquire("shared-engine")
            .expect("local saturation must return the half-open probe")
            .record_success();

        release.notify_one();
        assert_eq!(occupying_task.await.unwrap().items().len(), 1);
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
        assert_eq!(results.failures().len(), 1);
        assert_eq!(results.failures()[0].kind, "engine_suspended");
        assert_eq!(results.outcomes().len(), 2);
        assert!(results.outcomes().iter().any(|outcome| {
            outcome.kind == EngineOutcomeKind::CircuitOpen && outcome.shortcut == "flaky"
        }));

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

#[cfg(test)]
#[path = "search/coalescing_tests.rs"]
mod coalescing_tests;
