//! Metrics collection for monitoring fetcher performance.
//!
//! This module provides a simple in-memory metrics registry that tracks:
//! - Request counts (success, failure, transient failure)
//! - Latency percentiles
//! - Error type distributions
//!
//! ## Example
//!
//! ```rust
//! use a3s_search::metrics::Metrics;
//! use std::sync::Arc;
//!
//! let metrics = Arc::new(Metrics::new());
//! metrics.record_success(std::time::Duration::from_millis(150));
//! metrics.record_failure("timeout", true);
//! # async fn example(metrics: Arc<Metrics>) {
//! println!("{:#?}", metrics.snapshot().await);
//! # }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// In-memory metrics registry.
#[derive(Debug, Clone)]
pub struct Metrics {
    inner: Arc<InnerMetrics>,
}

#[derive(Debug)]
struct InnerMetrics {
    /// Total successful requests.
    successes: AtomicU64,
    /// Total failed requests.
    failures: AtomicU64,
    /// Total transient (retried) failures.
    transient_failures: AtomicU64,
    /// Total non-transient failures (permanent errors).
    permanent_failures: AtomicU64,
    /// Error type counters.
    error_counts: Mutex<HashMap<String, u64>>,
    /// Latency tracker.
    latency_tracker: Mutex<LatencyTracker>,
}

#[derive(Debug, Default)]
struct LatencyTracker {
    /// Sliding window of latencies (in ms) for percentile calculation.
    window: Vec<u64>,
    /// Maximum window size.
    max_size: usize,
}

impl Metrics {
    /// Creates a new metrics instance.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InnerMetrics {
                successes: AtomicU64::new(0),
                failures: AtomicU64::new(0),
                transient_failures: AtomicU64::new(0),
                permanent_failures: AtomicU64::new(0),
                error_counts: Mutex::new(HashMap::new()),
                latency_tracker: Mutex::new(LatencyTracker {
                    window: Vec::with_capacity(1000),
                    max_size: 1000,
                }),
            }),
        }
    }

    /// Records a successful request with its latency.
    pub fn record_success(&self, latency: Duration) {
        self.inner.successes.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency);
    }

    /// Records a failed request.
    ///
    /// If `is_transient` is true, the failure is counted as potentially retriable.
    pub fn record_failure(&self, error_type: &str, is_transient: bool) {
        self.inner.failures.fetch_add(1, Ordering::Relaxed);

        if is_transient {
            self.inner
                .transient_failures
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner
                .permanent_failures
                .fetch_add(1, Ordering::Relaxed);
        }

        let mut error_counts = lock_recover(&self.inner.error_counts);
        *error_counts.entry(error_type.to_string()).or_insert(0) += 1;
    }

    /// Records latency for percentile calculation.
    fn record_latency(&self, latency: Duration) {
        let latency_ms = latency.as_millis() as u64;
        let mut tracker = lock_recover(&self.inner.latency_tracker);

        if tracker.window.len() >= tracker.max_size {
            // Remove oldest 10%
            let remove_count = tracker.max_size / 10;
            tracker.window.drain(0..remove_count);
        }

        tracker.window.push(latency_ms);
    }

    /// Returns a snapshot of current metrics.
    pub async fn snapshot(&self) -> MetricsSnapshot {
        let error_counts = lock_recover(&self.inner.error_counts);
        let error_map = error_counts.clone();

        let latency_tracker = lock_recover(&self.inner.latency_tracker);
        let (p50, p95, p99) = calculate_percentiles(&latency_tracker.window);

        MetricsSnapshot {
            successes: self.inner.successes.load(Ordering::Relaxed),
            failures: self.inner.failures.load(Ordering::Relaxed),
            transient_failures: self.inner.transient_failures.load(Ordering::Relaxed),
            permanent_failures: self.inner.permanent_failures.load(Ordering::Relaxed),
            error_counts: error_map,
            latency_p50_ms: p50,
            latency_p95_ms: p95,
            latency_p99_ms: p99,
        }
    }

    /// Returns the total request count.
    pub fn total_requests(&self) -> u64 {
        self.inner.successes.load(Ordering::Relaxed) + self.inner.failures.load(Ordering::Relaxed)
    }

    /// Returns the success rate as a percentage (0-100).
    pub fn success_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            return 100.0;
        }
        let successes = self.inner.successes.load(Ordering::Relaxed) as f64;
        (successes / total as f64) * 100.0
    }

    /// Resets all metrics to zero.
    pub async fn reset(&self) {
        self.inner.successes.store(0, Ordering::Relaxed);
        self.inner.failures.store(0, Ordering::Relaxed);
        self.inner.transient_failures.store(0, Ordering::Relaxed);
        self.inner.permanent_failures.store(0, Ordering::Relaxed);
        lock_recover(&self.inner.error_counts).clear();
        lock_recover(&self.inner.latency_tracker).window.clear();
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Snapshot of metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// Total successful requests.
    pub successes: u64,
    /// Total failed requests.
    pub failures: u64,
    /// Total transient failures.
    pub transient_failures: u64,
    /// Total permanent failures.
    pub permanent_failures: u64,
    /// Error type counts.
    pub error_counts: HashMap<String, u64>,
    /// 50th percentile latency in milliseconds.
    pub latency_p50_ms: u64,
    /// 95th percentile latency in milliseconds.
    pub latency_p95_ms: u64,
    /// 99th percentile latency in milliseconds.
    pub latency_p99_ms: u64,
}

impl MetricsSnapshot {
    /// Returns the total request count.
    pub fn total_requests(&self) -> u64 {
        self.successes + self.failures
    }

    /// Returns the success rate as a percentage (0-100).
    pub fn success_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            return 100.0;
        }
        (self.successes as f64 / total as f64) * 100.0
    }

    /// Returns the transient failure rate as a percentage of total failures.
    pub fn transient_failure_rate(&self) -> f64 {
        if self.failures == 0 {
            return 0.0;
        }
        (self.transient_failures as f64 / self.failures as f64) * 100.0
    }
}

/// Calculate percentiles from a sorted (or unsorted) slice.
fn calculate_percentiles(values: &[u64]) -> (u64, u64, u64) {
    if values.is_empty() {
        return (0, 0, 0);
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable();

    let len = sorted.len();

    fn percentile(sorted: &[u64], len: usize, p: f64) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let idx = ((len as f64 * p).ceil() as usize).saturating_sub(1);
        let idx = idx.min(len - 1);
        sorted[idx]
    }

    let p50 = percentile(&sorted, len, 0.50);
    let p95 = percentile(&sorted, len, 0.95);
    let p99 = percentile(&sorted, len, 0.99);

    (p50, p95, p99)
}

/// Timing guard for measuring elapsed time.
#[must_use]
pub struct TimingGuard {
    start: Instant,
    metrics: Option<Arc<Metrics>>,
}

impl TimingGuard {
    /// Start timing and associate with metrics.
    pub fn new(metrics: Option<Arc<Metrics>>) -> Self {
        Self {
            start: Instant::now(),
            metrics,
        }
    }

    /// Returns elapsed time and records success if metrics are configured.
    pub fn success(self) -> Duration {
        let elapsed = self.start.elapsed();
        if let Some(ref metrics) = self.metrics {
            metrics.record_success(elapsed);
        }
        elapsed
    }

    /// Returns elapsed time and records failure if metrics are configured.
    pub fn failure(self, error_type: &str, is_transient: bool) -> Duration {
        let elapsed = self.start.elapsed();
        if let Some(ref metrics) = self.metrics {
            metrics.record_failure(error_type, is_transient);
        }
        elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_percentiles_empty() {
        let (p50, p95, p99) = calculate_percentiles(&[]);
        assert_eq!(p50, 0);
        assert_eq!(p95, 0);
        assert_eq!(p99, 0);
    }

    #[test]
    fn test_calculate_percentiles_single() {
        let values = vec![100];
        let (p50, p95, p99) = calculate_percentiles(&values);
        assert_eq!(p50, 100);
        assert_eq!(p95, 100);
        assert_eq!(p99, 100);
    }

    #[test]
    fn test_calculate_percentiles_sorted() {
        let values: Vec<u64> = (1..=100).collect();
        let (p50, p95, p99) = calculate_percentiles(&values);
        assert_eq!(p50, 50);
        assert_eq!(p95, 95);
        assert_eq!(p99, 99);
    }

    #[test]
    fn test_calculate_percentiles_unsorted() {
        let values = vec![100, 1, 50, 25, 75];
        let (p50, p95, p99) = calculate_percentiles(&values);
        assert_eq!(p50, 50);
        // 95th percentile of 5 elements: index = 4 (0-based), so value = 100
        assert_eq!(p95, 100);
        assert_eq!(p99, 100);
    }

    #[tokio::test]
    async fn test_metrics_record_success() {
        let metrics = Metrics::new();
        metrics.record_success(Duration::from_millis(100));
        metrics.record_success(Duration::from_millis(200));

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.successes, 2);
        assert_eq!(snapshot.failures, 0);
        assert_eq!(snapshot.total_requests(), 2);
        assert_eq!(snapshot.success_rate(), 100.0);
    }

    #[tokio::test]
    async fn test_metrics_record_failure() {
        let metrics = Metrics::new();
        metrics.record_failure("timeout", true);
        metrics.record_failure("not_found", false);

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.successes, 0);
        assert_eq!(snapshot.failures, 2);
        assert_eq!(snapshot.transient_failures, 1);
        assert_eq!(snapshot.permanent_failures, 1);
    }

    #[tokio::test]
    async fn test_metrics_reset() {
        let metrics = Metrics::new();
        metrics.record_success(Duration::from_millis(100));
        metrics.record_failure("error", false);

        metrics.reset().await;

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.successes, 0);
        assert_eq!(snapshot.failures, 0);
    }

    #[tokio::test]
    async fn test_metrics_timing_guard_success() {
        let metrics = Arc::new(Metrics::new());
        let guard = TimingGuard::new(Some(Arc::clone(&metrics)));

        std::thread::sleep(Duration::from_millis(10));
        let elapsed = guard.success();

        assert!(elapsed >= Duration::from_millis(10));
        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.successes, 1);
    }

    #[tokio::test]
    async fn test_metrics_snapshot_success_rate() {
        let snapshot = MetricsSnapshot {
            successes: 80,
            failures: 20,
            transient_failures: 10,
            permanent_failures: 10,
            error_counts: HashMap::new(),
            latency_p50_ms: 50,
            latency_p95_ms: 95,
            latency_p99_ms: 99,
        };

        assert_eq!(snapshot.total_requests(), 100);
        assert_eq!(snapshot.success_rate(), 80.0);
        assert_eq!(snapshot.transient_failure_rate(), 50.0);
    }

    #[tokio::test]
    async fn test_error_counts() {
        let metrics = Metrics::new();
        metrics.record_failure("timeout", true);
        metrics.record_failure("timeout", true);
        metrics.record_failure("connection_reset", true);

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.error_counts.get("timeout"), Some(&2));
        assert_eq!(snapshot.error_counts.get("connection_reset"), Some(&1));
    }

    #[tokio::test]
    async fn test_latency_percentiles() {
        let metrics = Metrics::new();

        // Record latencies: 1ms to 100ms
        for i in 1..=100 {
            metrics.record_success(Duration::from_millis(i));
        }

        let snapshot = metrics.snapshot().await;
        // p50 should be around 50
        assert!(snapshot.latency_p50_ms >= 45 && snapshot.latency_p50_ms <= 55);
        // p95 should be around 95
        assert!(snapshot.latency_p95_ms >= 90 && snapshot.latency_p95_ms <= 100);
    }
}
