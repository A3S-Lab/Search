//! Architecture contract: product cascade plan ownership and admission outcomes.

use a3s_search::{
    providers::BuiltinProvider, EngineOutcomeKind, EngineTier, EngineTierPlan, HealthConfig,
    Search, SearchQuery, SearchResult,
};
use async_trait::async_trait;

struct FailingEngine {
    config: a3s_search::EngineConfig,
}

impl FailingEngine {
    fn new(name: &str) -> Self {
        Self {
            config: a3s_search::EngineConfig {
                name: name.to_string(),
                shortcut: name.to_string(),
                ..Default::default()
            },
        }
    }
}

#[async_trait]
impl a3s_search::Engine for FailingEngine {
    fn config(&self) -> &a3s_search::EngineConfig {
        &self.config
    }

    async fn search(&self, _query: &SearchQuery) -> a3s_search::Result<Vec<SearchResult>> {
        Err(a3s_search::SearchError::Other("forced failure".to_string()))
    }
}

struct StableEngine;

#[async_trait]
impl a3s_search::Engine for StableEngine {
    fn config(&self) -> &a3s_search::EngineConfig {
        static CONFIG: std::sync::OnceLock<a3s_search::EngineConfig> = std::sync::OnceLock::new();
        CONFIG.get_or_init(|| a3s_search::EngineConfig {
            name: "stable".to_string(),
            shortcut: "stable".to_string(),
            ..Default::default()
        })
    }

    async fn search(&self, _query: &SearchQuery) -> a3s_search::Result<Vec<SearchResult>> {
        Ok(vec![SearchResult::new(
            "https://stable.example",
            "Stable",
            "ok",
        )])
    }
}

#[test]
fn library_owns_default_cascade_plan_and_excludes_billed_apis() {
    let plan = EngineTierPlan::builtin_default();
    let tiers = plan.tiers();
    assert_eq!(tiers[0].0, EngineTier::Api);
    assert_eq!(
        tiers[0].1,
        BuiltinProvider::DEFAULT
            .iter()
            .copied()
            .map(BuiltinProvider::id)
            .collect::<Vec<_>>()
            .as_slice()
    );
    for billed in ["tinyfish", "bocha", "aliyun", "tencent", "firecrawl"] {
        assert!(!plan.shortcuts().iter().any(|shortcut| shortcut == billed));
    }
}

#[tokio::test]
async fn local_health_suspension_is_not_a_shared_circuit_open() {
    let config = HealthConfig {
        max_failures: 1,
        suspend_duration: std::time::Duration::from_secs(3600),
    };
    let mut search = Search::with_health_config(config);
    search.add_engine(FailingEngine::new("flaky"));
    search.add_engine(StableEngine);

    search.search(SearchQuery::new("first")).await.unwrap();
    let results = search.search(SearchQuery::new("second")).await.unwrap();

    assert!(results.outcomes().iter().any(|outcome| {
        outcome.shortcut == "flaky" && outcome.kind == EngineOutcomeKind::HealthSuspended
    }));
    assert!(!results
        .outcomes()
        .iter()
        .any(|outcome| outcome.kind == EngineOutcomeKind::CircuitOpen));
}

#[tokio::test]
async fn attached_circuit_owns_isolation_over_local_health() {
    use a3s_search::{CircuitBreaker, CircuitBreakerConfig, EngineFailure};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    struct CountingFail {
        calls: Arc<AtomicUsize>,
        config: a3s_search::EngineConfig,
    }

    #[async_trait]
    impl a3s_search::Engine for CountingFail {
        fn config(&self) -> &a3s_search::EngineConfig {
            &self.config
        }

        async fn search(&self, _query: &SearchQuery) -> a3s_search::Result<Vec<SearchResult>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(a3s_search::SearchError::Other("fail".into()))
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        transient_open_duration: Duration::from_secs(3600),
        terminal_open_duration: Duration::from_secs(3600),
        max_open_duration: Duration::from_secs(3600),
        ..Default::default()
    });
    // Poison the shared circuit first.
    breaker
        .acquire("flaky")
        .unwrap()
        .record_failure(&EngineFailure::new("flaky", "other", "seed").with_transient(true));

    let health = HealthConfig {
        max_failures: 1,
        suspend_duration: Duration::from_secs(3600),
    };
    let mut search = Search::with_health_config(health).with_circuit_breaker(breaker);
    search.add_engine(CountingFail {
        calls: Arc::clone(&calls),
        config: a3s_search::EngineConfig {
            name: "flaky".into(),
            shortcut: "flaky".into(),
            ..Default::default()
        },
    });
    search.add_engine(StableEngine);

    let results = search.search(SearchQuery::new("query")).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(results.outcomes().iter().any(|outcome| {
        outcome.shortcut == "flaky" && outcome.kind == EngineOutcomeKind::CircuitOpen
    }));
    assert!(!results
        .outcomes()
        .iter()
        .any(|outcome| outcome.kind == EngineOutcomeKind::HealthSuspended));
}
