use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use a3s_search::{
    Bulkhead, BulkheadConfig, CircuitBreaker, CircuitBreakerConfig, CircuitState,
    CircuitWindowConfig, Engine, EngineConfig, EngineOutcomeKind, RetrievalRequirements, Search,
    SearchCascade, SearchCoalescer, SearchQuery, SearchResult, SearchResults,
};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy)]
pub(super) struct SoakConfig {
    pub duration: Duration,
    pub workers: usize,
    pub duplicate_group_size: usize,
    pub request_timeout: Duration,
    pub resource_warmup: Duration,
    pub max_rss_growth_kib: u64,
    pub max_tail_rss_slope_kib_per_minute: f64,
    pub max_fd_growth: usize,
}

impl SoakConfig {
    pub(super) fn from_env() -> Self {
        let duration = Duration::from_secs(env_u64("A3S_SEARCH_SOAK_SECONDS", 300).max(1));
        Self {
            duration,
            workers: env_usize("A3S_SEARCH_SOAK_WORKERS", 24).clamp(2, 256),
            duplicate_group_size: env_usize("A3S_SEARCH_SOAK_DUPLICATE_GROUP", 4).clamp(2, 32),
            request_timeout: Duration::from_millis(
                env_u64("A3S_SEARCH_SOAK_REQUEST_TIMEOUT_MS", 2_000).max(100),
            ),
            resource_warmup: Duration::from_secs(
                env_u64("A3S_SEARCH_SOAK_RESOURCE_WARMUP_SECONDS", 10)
                    .min(duration.as_secs().saturating_div(4)),
            ),
            max_rss_growth_kib: env_u64("A3S_SEARCH_SOAK_MAX_RSS_GROWTH_KIB", 65_536),
            max_tail_rss_slope_kib_per_minute: env_f64(
                "A3S_SEARCH_SOAK_MAX_TAIL_RSS_SLOPE_KIB_PER_MINUTE",
                1_024.0,
            )
            .max(0.0),
            max_fd_growth: env_usize("A3S_SEARCH_SOAK_MAX_FD_GROWTH", 8),
        }
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &f64| value.is_finite())
        .unwrap_or(default)
}

#[derive(Debug, Clone, Copy)]
enum Tier {
    Api,
    Http,
    Headless,
    Cancellation,
}

#[derive(Debug)]
struct PhaseClock {
    started: Instant,
    force_healthy: AtomicBool,
}

impl PhaseClock {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            force_healthy: AtomicBool::new(false),
        }
    }

    fn phase(&self) -> u64 {
        if self.force_healthy.load(Ordering::Acquire) {
            3
        } else {
            (self.started.elapsed().as_millis() as u64 / 1_000) % 4
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct EngineProbe {
    calls: AtomicU64,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl EngineProbe {
    fn enter(self: &Arc<Self>) -> EngineGuard {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let current = self.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        self.max_in_flight.fetch_max(current, Ordering::AcqRel);
        EngineGuard(Arc::clone(self))
    }

    pub(super) fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    pub(super) fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::Acquire)
    }
}

struct EngineGuard(Arc<EngineProbe>);

impl Drop for EngineGuard {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone)]
struct FaultEngine {
    config: EngineConfig,
    tier: Tier,
    clock: Arc<PhaseClock>,
    probe: Arc<EngineProbe>,
}

impl FaultEngine {
    fn new(
        name: &str,
        shortcut: &str,
        tier: Tier,
        clock: Arc<PhaseClock>,
        probe: Arc<EngineProbe>,
    ) -> Self {
        Self {
            config: EngineConfig {
                name: name.to_string(),
                shortcut: shortcut.to_string(),
                timeout: 1,
                ..EngineConfig::default()
            },
            tier,
            clock,
            probe,
        }
    }
}

#[async_trait]
impl Engine for FaultEngine {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> a3s_search::Result<Vec<SearchResult>> {
        let _guard = self.probe.enter();
        match self.tier {
            Tier::Api => {
                tokio::time::sleep(Duration::from_millis(4)).await;
                Ok(good_results(query, &self.config.shortcut))
            }
            Tier::Http => {
                tokio::time::sleep(Duration::from_millis(8)).await;
                if self.clock.phase() == 2 {
                    Ok(Vec::new())
                } else {
                    Ok(good_results(query, &self.config.shortcut))
                }
            }
            Tier::Headless => {
                tokio::time::sleep(Duration::from_millis(12)).await;
                match self.clock.phase() {
                    1 => Err(a3s_search::SearchError::RateLimited(
                        "deterministic soak throttle".to_string(),
                    )),
                    2 => Ok(Vec::new()),
                    _ => Ok(good_results(query, &self.config.shortcut)),
                }
            }
            Tier::Cancellation => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(good_results(query, &self.config.shortcut))
            }
        }
    }
}

fn good_results(query: &SearchQuery, tier: &str) -> Vec<SearchResult> {
    (0..5)
        .map(|index| {
            SearchResult::new(
                format!("https://{tier}-{index}.soak.invalid/evidence"),
                format!("{} evidence {index}", query.query),
                format!("Independent {} result for {}", tier, query.query),
            )
        })
        .collect()
}

#[derive(Clone)]
pub(super) struct SoakRuntime {
    pub circuit: CircuitBreaker,
    pub bulkhead: Bulkhead,
    pub coalescer: SearchCoalescer,
    clock: Arc<PhaseClock>,
    api: [FaultEngine; 2],
    http: [FaultEngine; 2],
    headless: [FaultEngine; 2],
    cancellation: FaultEngine,
    pub api_probe: Arc<EngineProbe>,
    pub http_probe: Arc<EngineProbe>,
    pub headless_probe: Arc<EngineProbe>,
    pub cancellation_probe: Arc<EngineProbe>,
    pub max_concurrent: usize,
}

impl SoakRuntime {
    pub(super) fn new() -> Self {
        let clock = Arc::new(PhaseClock::new());
        let api_probe = Arc::new(EngineProbe::default());
        let http_probe = Arc::new(EngineProbe::default());
        let headless_probe = Arc::new(EngineProbe::default());
        let cancellation_probe = Arc::new(EngineProbe::default());
        let max_concurrent = 4;
        let circuit = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            empty_threshold: 2,
            transient_open_duration: Duration::from_millis(50),
            terminal_open_duration: Duration::from_millis(100),
            max_open_duration: Duration::from_millis(500),
            open_backoff_factor: 2,
            open_jitter_ratio: 0.0,
            window: Some(CircuitWindowConfig {
                size: 20,
                minimum_calls: 10,
                failure_rate_threshold: 0.5,
                slow_call_duration: Duration::from_millis(100),
                slow_call_rate_threshold: 0.8,
            }),
        });
        let bulkhead = Bulkhead::new(BulkheadConfig {
            max_concurrent,
            max_queued: 16,
            max_queue_wait: Duration::from_millis(250),
        });
        Self {
            circuit,
            bulkhead,
            coalescer: SearchCoalescer::default(),
            api: [
                FaultEngine::new(
                    "Soak API Alpha",
                    "soak_api_alpha",
                    Tier::Api,
                    Arc::clone(&clock),
                    Arc::clone(&api_probe),
                ),
                FaultEngine::new(
                    "Soak API Beta",
                    "soak_api_beta",
                    Tier::Api,
                    Arc::clone(&clock),
                    Arc::clone(&api_probe),
                ),
            ],
            http: [
                FaultEngine::new(
                    "Soak HTTP Alpha",
                    "soak_http_alpha",
                    Tier::Http,
                    Arc::clone(&clock),
                    Arc::clone(&http_probe),
                ),
                FaultEngine::new(
                    "Soak Shared Source",
                    "soak_http_shared",
                    Tier::Http,
                    Arc::clone(&clock),
                    Arc::clone(&http_probe),
                ),
            ],
            headless: [
                FaultEngine::new(
                    "Soak Headless Alpha",
                    "soak_headless_alpha",
                    Tier::Headless,
                    Arc::clone(&clock),
                    Arc::clone(&headless_probe),
                ),
                FaultEngine::new(
                    "Soak Shared Source",
                    "soak_headless_shared",
                    Tier::Headless,
                    Arc::clone(&clock),
                    Arc::clone(&headless_probe),
                ),
            ],
            cancellation: FaultEngine::new(
                "Soak Cancellation",
                "soak_cancellation",
                Tier::Cancellation,
                Arc::clone(&clock),
                Arc::clone(&cancellation_probe),
            ),
            clock,
            api_probe,
            http_probe,
            headless_probe,
            cancellation_probe,
            max_concurrent,
        }
    }

    fn search(&self, engines: impl IntoIterator<Item = FaultEngine>) -> Search {
        let mut search = Search::new()
            .with_circuit_breaker(self.circuit.clone())
            .with_bulkhead(self.bulkhead.clone())
            .with_request_coalescer(self.coalescer.clone());
        for engine in engines {
            search.add_engine(engine);
        }
        search
    }

    pub(super) async fn run_query(&self, query: SearchQuery) -> RequestObservation {
        let mut cascade = SearchCascade::new(query.clone(), RetrievalRequirements::for_limit(5));
        let headless = self.search(self.headless.clone());
        let http = self.search(self.http.clone());
        let api = self.search(self.api.clone());
        cascade
            .run_tier_if_needed("headless", || async {
                search_or_empty(&headless, query.clone()).await
            })
            .await;
        cascade
            .run_tier_if_needed("http", || async {
                search_or_empty(&http, query.clone()).await
            })
            .await;
        cascade
            .run_tier_if_needed("api", || async { search_or_empty(&api, query).await })
            .await;
        let circuit_open = cascade
            .results()
            .outcomes()
            .iter()
            .filter(|outcome| outcome.kind == EngineOutcomeKind::CircuitOpen)
            .count() as u64;
        let rejected = cascade
            .results()
            .outcomes()
            .iter()
            .filter(|outcome| outcome.kind == EngineOutcomeKind::Rejected)
            .count() as u64;
        RequestObservation {
            tiers: cascade.reports().len(),
            requirements_met: !cascade.needs_next_tier(),
            circuit_open,
            rejected,
        }
    }

    pub(super) fn cancellation_search(&self) -> Search {
        self.search([self.cancellation.clone()])
    }

    pub(super) fn engine_shortcuts(&self) -> Vec<&str> {
        self.api
            .iter()
            .chain(&self.http)
            .chain(&self.headless)
            .chain(std::iter::once(&self.cancellation))
            .map(|engine| engine.config.shortcut.as_str())
            .collect()
    }

    pub(super) const fn retrieval_tier_width(&self) -> usize {
        self.api.len()
    }

    pub(super) async fn exercise_bulkhead_rejection(&self) -> u64 {
        let mut permits = Vec::with_capacity(self.max_concurrent);
        for _ in 0..self.max_concurrent {
            permits.push(
                self.bulkhead
                    .acquire("soak_cancellation")
                    .await
                    .expect("bulkhead capacity must be available after the worker drain"),
            );
        }
        let results = self
            .cancellation_search()
            .search(SearchQuery::new("generic bulkhead saturation probe"))
            .await
            .expect("bulkhead rejection must remain a structured search outcome");
        let rejected = results
            .outcomes()
            .iter()
            .filter(|outcome| outcome.kind == EngineOutcomeKind::Rejected)
            .count() as u64;
        drop(permits);
        rejected
    }

    pub(super) async fn force_recovery(&self) {
        self.clock.force_healthy.store(true, Ordering::Release);
        for engine in self.headless.iter().chain(&self.http).cloned() {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let shortcut = engine.config.shortcut.clone();
                let search = self.search([engine.clone()]);
                let results = search
                    .search(SearchQuery::new("generic recovery probe"))
                    .await
                    .expect("recovery search should remain structurally valid");
                if results
                    .outcomes()
                    .iter()
                    .any(|outcome| outcome.kind == EngineOutcomeKind::Success)
                {
                    assert_eq!(self.circuit.snapshot(&shortcut).state, CircuitState::Closed);
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "circuit did not recover: {shortcut}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

async fn search_or_empty(search: &Search, query: SearchQuery) -> SearchResults {
    search
        .search(query)
        .await
        .unwrap_or_else(|_| SearchResults::new())
}

#[derive(Debug)]
pub(super) struct RequestObservation {
    pub tiers: usize,
    pub requirements_met: bool,
    pub circuit_open: u64,
    pub rejected: u64,
}

#[derive(Debug, Default)]
pub(super) struct SoakCounters {
    pub requests: AtomicU64,
    pub completed: AtomicU64,
    pub deadline_timeouts: AtomicU64,
    pub retrieval_requirement_failures: AtomicU64,
    pub headless_only: AtomicU64,
    pub http_fallback: AtomicU64,
    pub api_fallback: AtomicU64,
    pub circuit_open: AtomicU64,
    pub rejected: AtomicU64,
    pub cancellation_attempts: AtomicU64,
    pub cancellation_recovered: AtomicU64,
    pub cancellation_failures: AtomicU64,
    pub latency: LatencyHistogram,
}

impl SoakCounters {
    pub(super) fn record(&self, observation: RequestObservation, elapsed: Duration) {
        self.completed.fetch_add(1, Ordering::Relaxed);
        self.latency.record(elapsed);
        if !observation.requirements_met {
            self.retrieval_requirement_failures
                .fetch_add(1, Ordering::Relaxed);
        }
        match observation.tiers {
            1 => &self.headless_only,
            2 => &self.http_fallback,
            _ => &self.api_fallback,
        }
        .fetch_add(1, Ordering::Relaxed);
        self.circuit_open
            .fetch_add(observation.circuit_open, Ordering::Relaxed);
        self.rejected
            .fetch_add(observation.rejected, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(super) struct LatencyHistogram {
    buckets: [AtomicU64; 32],
    max_micros: AtomicU64,
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            max_micros: AtomicU64::new(0),
        }
    }
}

impl LatencyHistogram {
    fn record(&self, duration: Duration) {
        let micros = u64::try_from(duration.as_micros())
            .unwrap_or(u64::MAX)
            .max(1);
        let bucket = (u64::BITS - micros.leading_zeros()) as usize;
        self.buckets[bucket.min(self.buckets.len() - 1)].fetch_add(1, Ordering::Relaxed);
        self.max_micros.fetch_max(micros, Ordering::Relaxed);
    }

    pub(super) fn percentile_ms(&self, percentile: f64) -> u64 {
        let total: u64 = self
            .buckets
            .iter()
            .map(|bucket| bucket.load(Ordering::Relaxed))
            .sum();
        let target = ((total as f64 * percentile).ceil() as u64).max(1);
        let mut seen = 0;
        for (index, bucket) in self.buckets.iter().enumerate() {
            seen += bucket.load(Ordering::Relaxed);
            if seen >= target {
                return (1_u64 << index).div_ceil(1_000);
            }
        }
        0
    }

    pub(super) fn max_ms(&self) -> u64 {
        self.max_micros.load(Ordering::Relaxed).div_ceil(1_000)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn fault_topology_preserves_source_quorum_across_transport_fallback() {
        let runtime = SoakRuntime::new();

        for tier in [&runtime.headless, &runtime.http, &runtime.api] {
            let sources = tier
                .iter()
                .map(|engine| engine.config.name.as_str())
                .collect::<HashSet<_>>();
            assert_eq!(sources.len(), 2);
        }

        assert_eq!(runtime.headless[1].config.name, runtime.http[1].config.name);
        assert_ne!(
            runtime.headless[1].config.shortcut,
            runtime.http[1].config.shortcut
        );
    }
}
