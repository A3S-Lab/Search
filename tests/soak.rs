//! Opt-in endurance tests for the public search reliability contracts.

#[path = "soak/gate.rs"]
mod gate;
#[path = "soak/harness.rs"]
mod harness;
#[path = "soak/live.rs"]
mod live;
#[path = "soak/policy.rs"]
mod policy;
#[path = "soak/resources.rs"]
mod resources;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use a3s_search::SearchQuery;
use futures::future::join_all;
use serde_json::json;
use tokio::sync::Barrier;

use harness::{SoakConfig, SoakCounters, SoakRuntime};
use resources::{sample_resources, summarize_resources};

#[tokio::test]
async fn deterministic_bulkhead_probe_produces_one_structured_rejection() {
    let runtime = SoakRuntime::new();

    assert_eq!(runtime.exercise_bulkhead_rejection().await, 1);
    let snapshot = runtime.bulkhead.snapshot("soak_cancellation");
    assert_eq!(snapshot.in_flight, 0);
    assert_eq!(snapshot.queued, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "long-running deterministic stability soak; run explicitly"]
async fn deterministic_reliability_soak() {
    let config = SoakConfig::from_env();
    let runtime = SoakRuntime::new();
    let counters = Arc::new(SoakCounters::default());
    let deadline = Instant::now() + config.duration;
    let barrier = Arc::new(Barrier::new(config.workers));
    let keep_running = Arc::new(AtomicBool::new(true));
    let resource_running = Arc::new(AtomicBool::new(true));
    let resource_samples = Arc::new(Mutex::new(Vec::new()));
    let sampler = tokio::spawn(sample_resources(
        Arc::clone(&resource_running),
        Arc::clone(&resource_samples),
        config.resource_warmup,
    ));

    let mut workers = Vec::with_capacity(config.workers);
    for worker_id in 0..config.workers {
        let runtime = runtime.clone();
        let counters = Arc::clone(&counters);
        let barrier = Arc::clone(&barrier);
        let keep_running = Arc::clone(&keep_running);
        workers.push(tokio::spawn(async move {
            let mut wave = 0_u64;
            loop {
                let elected = barrier.wait().await;
                if elected.is_leader() {
                    keep_running.store(Instant::now() < deadline, Ordering::Release);
                }
                barrier.wait().await;
                if !keep_running.load(Ordering::Acquire) {
                    break;
                }

                let group = worker_id / config.duplicate_group_size;
                let query = SearchQuery::new(format!("generic soak wave {wave} group {group}"));
                counters.requests.fetch_add(1, Ordering::Relaxed);
                let started = Instant::now();
                match tokio::time::timeout(config.request_timeout, runtime.run_query(query)).await {
                    Ok(observation) => counters.record(observation, started.elapsed()),
                    Err(_) => {
                        counters.deadline_timeouts.fetch_add(1, Ordering::Relaxed);
                    }
                }
                wave = wave.saturating_add(1);
            }
        }));
    }

    let cancellation = tokio::spawn(run_cancellation_soak(
        runtime.clone(),
        Arc::clone(&counters),
        deadline,
    ));
    let join_budget = config.duration.saturating_add(Duration::from_secs(10));
    let worker_results = tokio::time::timeout(join_budget, join_all(workers))
        .await
        .expect("workers did not drain after the soak deadline");
    for result in worker_results {
        result.expect("soak worker panicked");
    }
    cancellation.await.expect("cancellation soak task panicked");

    let forced_rejections = runtime.exercise_bulkhead_rejection().await;
    counters
        .rejected
        .fetch_add(forced_rejections, Ordering::Relaxed);
    runtime.force_recovery().await;
    resource_running.store(false, Ordering::Release);
    sampler.await.expect("resource sampler panicked");
    let resources = summarize_resources(&resource_samples.lock().unwrap());
    let coalescer = runtime.coalescer.snapshot();
    let bulkhead_snapshots = runtime
        .engine_shortcuts()
        .into_iter()
        .map(|shortcut| runtime.bulkhead.snapshot(shortcut))
        .collect::<Vec<_>>();

    let requests = counters.requests.load(Ordering::Relaxed);
    let completed = counters.completed.load(Ordering::Relaxed);
    let deadline_timeouts = counters.deadline_timeouts.load(Ordering::Relaxed);
    let retrieval_requirement_failures = counters
        .retrieval_requirement_failures
        .load(Ordering::Relaxed);
    let headless_only = counters.headless_only.load(Ordering::Relaxed);
    let http_fallback = counters.http_fallback.load(Ordering::Relaxed);
    let api_fallback = counters.api_fallback.load(Ordering::Relaxed);
    let cancellation_attempts = counters.cancellation_attempts.load(Ordering::Relaxed);
    let cancellation_recovered = counters.cancellation_recovered.load(Ordering::Relaxed);
    let cancellation_failures = counters.cancellation_failures.load(Ordering::Relaxed);

    println!(
        "SOAK_REPORT={}",
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "duration_seconds": config.duration.as_secs(),
            "workers": config.workers,
            "requests": requests,
            "completed": completed,
            "deadline_timeouts": deadline_timeouts,
            "retrieval_requirement_failures": retrieval_requirement_failures,
            "paths": {
                "headless_only": headless_only,
                "http_fallback": http_fallback,
                "api_fallback": api_fallback,
            },
            "outcomes": {
                "circuit_open": counters.circuit_open.load(Ordering::Relaxed),
                "bulkhead_rejected": counters.rejected.load(Ordering::Relaxed),
            },
            "latency_ms": {
                "p50": counters.latency.percentile_ms(0.50),
                "p95": counters.latency.percentile_ms(0.95),
                "p99": counters.latency.percentile_ms(0.99),
                "max": counters.latency.max_ms(),
            },
            "engine_calls": {
                "api": runtime.api_probe.calls(),
                "http": runtime.http_probe.calls(),
                "headless": runtime.headless_probe.calls(),
                "cancellation": runtime.cancellation_probe.calls(),
            },
            "max_engine_concurrency": {
                "api": runtime.api_probe.max_in_flight(),
                "http": runtime.http_probe.max_in_flight(),
                "headless": runtime.headless_probe.max_in_flight(),
                "cancellation": runtime.cancellation_probe.max_in_flight(),
            },
            "coalescing": {
                "in_flight": coalescer.in_flight,
                "leaders": coalescer.leader_requests,
                "shared": coalescer.shared_requests,
                "bypassed": coalescer.bypassed_requests,
                "abandoned": coalescer.abandoned_requests,
            },
            "cancellation": {
                "attempts": cancellation_attempts,
                "recovered": cancellation_recovered,
                "failures": cancellation_failures,
            },
            "resources": resources,
        })
    );

    assert!(
        requests >= config.workers as u64 * 10,
        "insufficient soak load"
    );
    assert_eq!(completed, requests, "not every request completed");
    assert_eq!(deadline_timeouts, 0, "request deadline was exceeded");
    assert_eq!(
        retrieval_requirement_failures, 0,
        "fallback exhausted below the structural retrieval requirements"
    );
    assert!(
        headless_only > 0,
        "healthy headless path was never observed"
    );
    assert!(http_fallback > 0, "HTTP fallback was never observed");
    assert!(api_fallback > 0, "API fallback was never observed");
    assert!(
        api_fallback < completed,
        "the final API tier ran for every request"
    );
    assert!(
        counters.circuit_open.load(Ordering::Relaxed) > 0,
        "circuit never opened"
    );
    assert!(
        counters.rejected.load(Ordering::Relaxed) > 0,
        "bulkhead rejection was never observed"
    );
    assert!(
        coalescer.shared_requests > 0,
        "no concurrent request was coalesced"
    );
    assert_eq!(
        coalescer.in_flight, 0,
        "coalesced flights leaked after drain"
    );
    assert_eq!(
        coalescer.bypassed_requests, 0,
        "coalescer capacity was unexpectedly exhausted"
    );
    assert!(
        cancellation_attempts > 0,
        "cancellation path was never exercised"
    );
    assert_eq!(
        cancellation_failures, 0,
        "a cancelled leader stranded a follower"
    );
    assert_eq!(cancellation_recovered, cancellation_attempts);
    let tier_concurrency_limit = runtime
        .max_concurrent
        .saturating_mul(runtime.retrieval_tier_width());
    for probe in [
        &runtime.api_probe,
        &runtime.http_probe,
        &runtime.headless_probe,
    ] {
        assert!(probe.max_in_flight() <= tier_concurrency_limit);
    }
    assert!(runtime.cancellation_probe.max_in_flight() <= runtime.max_concurrent);
    for snapshot in bulkhead_snapshots {
        assert_eq!(snapshot.in_flight, 0, "bulkhead permit leaked after drain");
        assert_eq!(snapshot.queued, 0, "bulkhead queue did not drain");
    }
    assert!(
        resources.rss_growth_kib <= config.max_rss_growth_kib as i64,
        "RSS growth exceeded the soak threshold: {resources:?}"
    );
    if resources.samples >= 120 {
        assert!(
            resources.tail_rss_slope_kib_per_minute <= config.max_tail_rss_slope_kib_per_minute,
            "tail RSS slope indicates a slow leak: {resources:?}"
        );
    }
    assert!(
        resources.fd_growth <= config.max_fd_growth as isize,
        "file descriptor growth exceeded the soak threshold: {resources:?}"
    );
}

async fn run_cancellation_soak(
    runtime: SoakRuntime,
    counters: Arc<SoakCounters>,
    deadline: Instant,
) {
    let mut sequence = 0_u64;
    while Instant::now() + Duration::from_millis(100) < deadline {
        counters
            .cancellation_attempts
            .fetch_add(1, Ordering::Relaxed);
        let query = SearchQuery::new(format!("cancellation flight {sequence}"));
        let shared_before = runtime.coalescer.snapshot().shared_requests;
        let leader_search = Arc::new(runtime.cancellation_search());
        let follower_search = Arc::new(runtime.cancellation_search());
        let leader_query = query.clone();
        let leader = tokio::spawn(async move { leader_search.search(leader_query).await });
        tokio::time::sleep(Duration::from_millis(2)).await;
        let follower = tokio::spawn(async move { follower_search.search(query).await });
        for _ in 0..1_000 {
            if runtime.coalescer.snapshot().shared_requests > shared_before {
                break;
            }
            tokio::task::yield_now().await;
        }
        let joined = runtime.coalescer.snapshot().shared_requests > shared_before;
        leader.abort();
        let _ = leader.await;
        let recovered = tokio::time::timeout(Duration::from_secs(1), follower).await;
        match recovered {
            Ok(Ok(Ok(results))) if joined && results.items().len() == 5 => {
                counters
                    .cancellation_recovered
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                counters
                    .cancellation_failures
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        sequence = sequence.saturating_add(1);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
