//! Shared, bounded concurrency isolation for search engines.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Per-engine concurrency and queue policy.
#[derive(Debug, Clone)]
pub struct BulkheadConfig {
    /// Maximum simultaneous calls admitted for one engine key.
    pub max_concurrent: usize,
    /// Maximum additional calls allowed to wait for one engine key.
    pub max_queued: usize,
    /// Maximum time a queued call may wait for execution capacity.
    pub max_queue_wait: Duration,
}

impl Default for BulkheadConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            max_queued: 16,
            max_queue_wait: Duration::from_millis(250),
        }
    }
}

/// Stable reason a bulkhead rejected an engine attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkheadRejectionKind {
    /// The in-flight and bounded queue capacity was already occupied.
    Saturated,
    /// The attempt entered the queue but no execution permit arrived in time.
    QueueTimeout,
}

/// Typed local-overload rejection from a per-engine bulkhead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BulkheadRejection {
    /// No bounded admission slot was available.
    #[error("engine bulkhead is saturated")]
    Saturated,
    /// Bounded queue wait expired.
    #[error("engine bulkhead queue wait timed out")]
    QueueTimeout,
}

impl BulkheadRejection {
    /// Returns a stable, low-cardinality rejection kind.
    pub const fn kind(self) -> BulkheadRejectionKind {
        match self {
            Self::Saturated => BulkheadRejectionKind::Saturated,
            Self::QueueTimeout => BulkheadRejectionKind::QueueTimeout,
        }
    }

    pub(crate) const fn failure_kind(self) -> &'static str {
        match self {
            Self::Saturated => "bulkhead_saturated",
            Self::QueueTimeout => "bulkhead_queue_timeout",
        }
    }
}

/// Point-in-time bulkhead diagnostics for one engine key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkheadSnapshot {
    /// Configured simultaneous execution limit.
    pub max_concurrent: usize,
    /// Configured bounded queue size.
    pub max_queued: usize,
    /// Calls currently holding execution permits.
    pub in_flight: usize,
    /// Calls currently waiting for execution permits.
    pub queued: usize,
}

/// Shared registry that isolates each engine from concurrent overload.
#[derive(Debug, Clone)]
pub struct Bulkhead {
    config: Arc<NormalizedConfig>,
    inner: Arc<Mutex<HashMap<String, Arc<Entry>>>>,
}

#[derive(Debug)]
struct NormalizedConfig {
    max_concurrent: usize,
    max_queued: usize,
    max_queue_wait: Duration,
}

#[derive(Debug)]
struct Entry {
    execution: Arc<Semaphore>,
    admission: Arc<Semaphore>,
    max_concurrent: usize,
    max_queued: usize,
}

/// RAII capacity token held for one admitted engine attempt.
#[derive(Debug)]
pub struct BulkheadPermit {
    _execution: OwnedSemaphorePermit,
    _admission: OwnedSemaphorePermit,
}

impl Bulkhead {
    /// Creates an empty shared registry with a normalized bounded policy.
    pub fn new(config: BulkheadConfig) -> Self {
        let max_concurrent = config.max_concurrent.clamp(1, Semaphore::MAX_PERMITS);
        let max_queued = config
            .max_queued
            .min(Semaphore::MAX_PERMITS.saturating_sub(max_concurrent));
        Self {
            config: Arc::new(NormalizedConfig {
                max_concurrent,
                max_queued,
                max_queue_wait: config.max_queue_wait,
            }),
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Acquires bounded capacity for one engine call.
    ///
    /// Calls beyond the in-flight plus queue bound fail immediately. Calls
    /// admitted to the queue fail after `max_queue_wait` instead of waiting
    /// indefinitely and consuming the caller deadline.
    pub async fn acquire(&self, key: &str) -> Result<BulkheadPermit, BulkheadRejection> {
        let entry = self.entry(key);
        let admission = Arc::clone(&entry.admission)
            .try_acquire_owned()
            .map_err(|_| BulkheadRejection::Saturated)?;

        let execution = if self.config.max_queue_wait.is_zero() {
            Arc::clone(&entry.execution)
                .try_acquire_owned()
                .map_err(|_| BulkheadRejection::QueueTimeout)?
        } else {
            tokio::time::timeout(
                self.config.max_queue_wait,
                Arc::clone(&entry.execution).acquire_owned(),
            )
            .await
            .map_err(|_| BulkheadRejection::QueueTimeout)?
            .map_err(|_| BulkheadRejection::Saturated)?
        };

        Ok(BulkheadPermit {
            _execution: execution,
            _admission: admission,
        })
    }

    /// Returns point-in-time capacity diagnostics without acquiring a permit.
    pub fn snapshot(&self, key: &str) -> BulkheadSnapshot {
        let entry = self.entry(key);
        let in_flight = entry
            .max_concurrent
            .saturating_sub(entry.execution.available_permits());
        let admitted = entry
            .max_concurrent
            .saturating_add(entry.max_queued)
            .saturating_sub(entry.admission.available_permits());
        BulkheadSnapshot {
            max_concurrent: entry.max_concurrent,
            max_queued: entry.max_queued,
            in_flight,
            queued: admitted.saturating_sub(in_flight),
        }
    }

    fn entry(&self, key: &str) -> Arc<Entry> {
        let key = normalized_key(key);
        let mut entries = lock_recover(&self.inner);
        Arc::clone(entries.entry(key).or_insert_with(|| {
            Arc::new(Entry {
                execution: Arc::new(Semaphore::new(self.config.max_concurrent)),
                admission: Arc::new(Semaphore::new(
                    self.config
                        .max_concurrent
                        .saturating_add(self.config.max_queued),
                )),
                max_concurrent: self.config.max_concurrent,
                max_queued: self.config.max_queued,
            })
        }))
    }
}

impl Default for Bulkhead {
    fn default() -> Self {
        Self::new(BulkheadConfig::default())
    }
}

fn normalized_key(key: &str) -> String {
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        "unknown".to_string()
    } else {
        key
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
#[path = "bulkhead/tests.rs"]
mod tests;
