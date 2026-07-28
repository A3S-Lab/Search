use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::*;
use crate::{
    EngineConfig, SafeSearch, SearchCoalescer, SearchCoalescerConfig, SearchQuery, SearchResult,
};

struct CountingEngine {
    config: EngineConfig,
    calls: Arc<AtomicUsize>,
    delay: Duration,
}

impl CountingEngine {
    fn new(calls: Arc<AtomicUsize>, delay: Duration) -> Self {
        Self {
            config: EngineConfig {
                name: "Counting".to_string(),
                shortcut: "counting".to_string(),
                ..EngineConfig::default()
            },
            calls,
            delay,
        }
    }
}

#[async_trait]
impl Engine for CountingEngine {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        Ok(vec![SearchResult::new(
            format!("https://example.test/{}", query.page),
            query.query.clone(),
            query.language.clone().unwrap_or_default(),
        )])
    }
}

fn configured_search(coalescer: SearchCoalescer, engine: impl Engine + 'static) -> Search {
    let mut search = Search::new().with_request_coalescer(coalescer);
    search.add_engine(engine);
    search
}

#[tokio::test]
async fn shared_coalescer_executes_one_upstream_call_for_identical_concurrent_requests() {
    let coalescer = SearchCoalescer::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let first = configured_search(
        coalescer.clone(),
        CountingEngine::new(Arc::clone(&calls), Duration::from_millis(25)),
    );
    let second = configured_search(
        coalescer.clone(),
        CountingEngine::new(Arc::clone(&calls), Duration::from_millis(25)),
    );
    let query = SearchQuery::new("shared request");

    let (first_result, second_result) =
        tokio::join!(first.search(query.clone()), second.search(query));

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_result.unwrap().items().len(), 1);
    assert_eq!(second_result.unwrap().items().len(), 1);
    let snapshot = coalescer.snapshot();
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(snapshot.leader_requests, 1);
    assert_eq!(snapshot.shared_requests, 1);
    assert_eq!(snapshot.bypassed_requests, 0);
    assert_eq!(snapshot.abandoned_requests, 0);
}

#[tokio::test]
async fn query_controls_are_part_of_the_coalescing_identity() {
    let coalescer = SearchCoalescer::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let search = configured_search(
        coalescer.clone(),
        CountingEngine::new(Arc::clone(&calls), Duration::from_millis(25)),
    );
    let english = SearchQuery::new("same text")
        .with_language("en-US")
        .with_safesearch(SafeSearch::Moderate);
    let chinese = SearchQuery::new("same text")
        .with_language("zh-CN")
        .with_safesearch(SafeSearch::Strict);

    let (english_result, chinese_result) =
        tokio::join!(search.search(english), search.search(chinese));

    assert!(english_result.is_ok());
    assert!(chinese_result.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let snapshot = coalescer.snapshot();
    assert_eq!(snapshot.leader_requests, 2);
    assert_eq!(snapshot.shared_requests, 0);
}

#[tokio::test]
async fn engine_configuration_is_part_of_the_coalescing_identity() {
    let coalescer = SearchCoalescer::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let first_engine = CountingEngine::new(Arc::clone(&calls), Duration::from_millis(25));
    let mut second_engine = CountingEngine::new(Arc::clone(&calls), Duration::from_millis(25));
    second_engine.config.weight = 2.0;
    let first = configured_search(coalescer.clone(), first_engine);
    let second = configured_search(coalescer.clone(), second_engine);
    let query = SearchQuery::new("same query, different policy");

    let (first_result, second_result) =
        tokio::join!(first.search(query.clone()), second.search(query));

    assert!(first_result.is_ok());
    assert!(second_result.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let snapshot = coalescer.snapshot();
    assert_eq!(snapshot.leader_requests, 2);
    assert_eq!(snapshot.shared_requests, 0);
}

#[tokio::test]
async fn completed_requests_are_not_cached() {
    let coalescer = SearchCoalescer::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let search = configured_search(
        coalescer.clone(),
        CountingEngine::new(Arc::clone(&calls), Duration::ZERO),
    );
    let query = SearchQuery::new("fresh request");

    search.search(query.clone()).await.unwrap();
    search.search(query).await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(coalescer.snapshot().leader_requests, 2);
}

#[tokio::test]
async fn capacity_is_bounded_and_excess_distinct_requests_bypass_coalescing() {
    let coalescer = SearchCoalescer::new(SearchCoalescerConfig { max_in_flight: 1 });
    let calls = Arc::new(AtomicUsize::new(0));
    let search = configured_search(
        coalescer.clone(),
        CountingEngine::new(Arc::clone(&calls), Duration::from_millis(25)),
    );

    let (first, second) = tokio::join!(
        search.search(SearchQuery::new("first distinct request")),
        search.search(SearchQuery::new("second distinct request")),
    );

    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let snapshot = coalescer.snapshot();
    assert_eq!(snapshot.max_in_flight, 1);
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(snapshot.leader_requests, 1);
    assert_eq!(snapshot.bypassed_requests, 1);
}

struct CancelledLeaderEngine {
    config: EngineConfig,
    calls: Arc<AtomicUsize>,
    started: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl Engine for CancelledLeaderEngine {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            self.started.notify_waiters();
            return pending().await;
        }
        Ok(vec![SearchResult::new(
            "https://example.test/recovered",
            query.query.clone(),
            "recovered after leader cancellation",
        )])
    }
}

#[tokio::test]
async fn cancelling_the_leader_wakes_a_follower_to_retry_without_hanging() {
    let coalescer = SearchCoalescer::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let mut search = Search::new().with_request_coalescer(coalescer.clone());
    search.add_engine(CancelledLeaderEngine {
        config: EngineConfig {
            name: "Cancellation".to_string(),
            shortcut: "cancellation".to_string(),
            ..EngineConfig::default()
        },
        calls: Arc::clone(&calls),
        started: Arc::clone(&started),
    });
    let search = Arc::new(search);
    let query = SearchQuery::new("cancellation-safe request");
    let started_wait = started.notified();

    let leader_search = Arc::clone(&search);
    let leader_query = query.clone();
    let leader = tokio::spawn(async move { leader_search.search(leader_query).await });
    started_wait.await;

    let follower_search = Arc::clone(&search);
    let follower = tokio::spawn(async move { follower_search.search(query).await });
    for _ in 0..1_000 {
        if coalescer.snapshot().shared_requests == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(coalescer.snapshot().shared_requests, 1);

    leader.abort();
    let _ = leader.await;
    let recovered = tokio::time::timeout(Duration::from_secs(1), follower)
        .await
        .expect("follower should not hang after leader cancellation")
        .expect("follower task should complete")
        .expect("follower search should succeed");

    assert_eq!(recovered.items().len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let snapshot = coalescer.snapshot();
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(snapshot.abandoned_requests, 1);
    assert_eq!(snapshot.leader_requests, 2);
}
