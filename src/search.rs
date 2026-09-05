//! Search orchestration.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::future::join_all;
use tokio::time::Duration;
use tracing::debug;

use crate::coalescer::{SearchCoalescingAdmission, SearchRequestKey};
use crate::engine_registry::{EngineRegistry, EngineSelection};
use crate::engine_runner::{EngineExecution, EngineRunner};
use crate::{
    Aggregator, Bulkhead, CircuitBreaker, Engine, HealthConfig, HealthMonitor, Metrics,
    RankingConfig, Result, SearchCoalescer, SearchError, SearchQuery, SearchResults,
};

/// Meta search engine that orchestrates searches across multiple engines.
pub struct Search {
    engine_registry: EngineRegistry,
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
        Self::with_health_config(HealthConfig::default())
    }

    /// Creates a new search instance with a custom health configuration.
    pub fn with_health_config(config: HealthConfig) -> Self {
        Self {
            engine_registry: EngineRegistry::default(),
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
    ///
    /// The engine's configuration is captured at registration time. This gives
    /// every request a stable timeout/category/weight descriptor; build a new
    /// [`Search`] instance when those configuration values need to change.
    /// Trait-provided identity methods are sampled when a request is admitted.
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E) {
        let config = self.engine_registry.add(engine);
        self.aggregator
            .set_engine_weight(&config.name, config.weight);
    }

    /// Overrides the timeout applied to each engine during searches.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout_override = Some(timeout);
    }

    /// Returns the number of configured engines.
    pub fn engine_count(&self) -> usize {
        self.engine_registry.len()
    }

    /// Performs a search across all configured engines.
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults> {
        if self.engine_registry.is_empty() {
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
            self.engine_registry.configs(),
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

        let selection = self.select_engines(&query);
        debug!("Searching {} engines", selection.attempts.len());
        let runner = EngineRunner::new(
            self.timeout_override,
            self.metrics.as_ref().map(Arc::clone),
            self.bulkhead.clone(),
        );

        let futures: Vec<_> = selection
            .attempts
            .into_iter()
            .map(|attempt| {
                let runner = runner.clone();
                let query = Arc::clone(&query);
                async move { runner.run(attempt, query).await }
            })
            .collect();

        let all_results: Vec<_> = join_all(futures).await;

        let mut engine_errors = selection
            .skipped_failures
            .into_iter()
            .map(|failure| (failure, false))
            .collect::<Vec<_>>();
        let mut outcomes = selection.skipped_outcomes;
        let outputs: Vec<_> = all_results
            .into_iter()
            .filter_map(|execution| match execution {
                EngineExecution::Completed {
                    engine_name: name,
                    output,
                    outcome,
                } => {
                    outcomes.push(outcome);
                    Some((name, output))
                }
                EngineExecution::Failed {
                    failure,
                    affects_health,
                    outcome,
                } => {
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
    fn select_engines(&self, query: &SearchQuery) -> EngineSelection {
        let health = self.health.lock().ok();
        self.engine_registry
            .select(query, health.as_deref(), self.circuit_breaker.as_ref())
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "search/coalescing_tests.rs"]
mod coalescing_tests;
#[cfg(test)]
#[path = "search/tests.rs"]
mod tests;
