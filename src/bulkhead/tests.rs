use super::*;

#[tokio::test]
async fn zero_queue_rejects_excess_work_immediately() {
    let bulkhead = Bulkhead::new(BulkheadConfig {
        max_concurrent: 1,
        max_queued: 0,
        max_queue_wait: Duration::from_secs(1),
    });
    let permit = bulkhead.acquire("api").await.unwrap();

    assert_eq!(
        bulkhead.acquire("api").await.unwrap_err(),
        BulkheadRejection::Saturated
    );
    assert_eq!(bulkhead.snapshot("api").in_flight, 1);
    drop(permit);
    assert!(bulkhead.acquire("api").await.is_ok());
}

#[tokio::test]
async fn bounded_waiter_runs_after_capacity_is_released() {
    let bulkhead = Bulkhead::new(BulkheadConfig {
        max_concurrent: 1,
        max_queued: 1,
        max_queue_wait: Duration::from_secs(1),
    });
    let permit = bulkhead.acquire("api").await.unwrap();
    let waiting = bulkhead.clone();
    let waiter = tokio::spawn(async move { waiting.acquire("api").await });

    while bulkhead.snapshot("api").queued == 0 {
        tokio::task::yield_now().await;
    }
    drop(permit);

    assert!(waiter.await.unwrap().is_ok());
}

#[tokio::test]
async fn queue_capacity_is_bounded() {
    let bulkhead = Bulkhead::new(BulkheadConfig {
        max_concurrent: 1,
        max_queued: 1,
        max_queue_wait: Duration::from_secs(1),
    });
    let _permit = bulkhead.acquire("api").await.unwrap();
    let waiting = bulkhead.clone();
    let waiter = tokio::spawn(async move { waiting.acquire("api").await });
    while bulkhead.snapshot("api").queued == 0 {
        tokio::task::yield_now().await;
    }

    assert_eq!(
        bulkhead.acquire("api").await.unwrap_err(),
        BulkheadRejection::Saturated
    );
    waiter.abort();
    let _ = waiter.await;
}

#[tokio::test]
async fn queue_timeout_releases_admission_capacity() {
    let bulkhead = Bulkhead::new(BulkheadConfig {
        max_concurrent: 1,
        max_queued: 1,
        max_queue_wait: Duration::from_millis(1),
    });
    let permit = bulkhead.acquire("api").await.unwrap();

    assert_eq!(
        bulkhead.acquire("api").await.unwrap_err(),
        BulkheadRejection::QueueTimeout
    );
    assert_eq!(bulkhead.snapshot("api").queued, 0);
    drop(permit);
    assert!(bulkhead.acquire("api").await.is_ok());
}

#[tokio::test]
async fn engine_keys_have_independent_capacity() {
    let bulkhead = Bulkhead::new(BulkheadConfig {
        max_concurrent: 1,
        max_queued: 0,
        max_queue_wait: Duration::ZERO,
    });
    let _api = bulkhead.acquire("api").await.unwrap();

    assert!(bulkhead.acquire("http").await.is_ok());
    assert_eq!(
        bulkhead.acquire("api").await.unwrap_err().kind(),
        BulkheadRejectionKind::Saturated
    );
}

#[test]
fn impossible_limits_are_normalized_without_panicking() {
    let bulkhead = Bulkhead::new(BulkheadConfig {
        max_concurrent: 0,
        max_queued: usize::MAX,
        max_queue_wait: Duration::ZERO,
    });
    let snapshot = bulkhead.snapshot("api");

    assert_eq!(snapshot.max_concurrent, 1);
    assert_eq!(
        snapshot.max_concurrent.saturating_add(snapshot.max_queued),
        Semaphore::MAX_PERMITS
    );
}
