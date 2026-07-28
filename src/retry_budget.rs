//! Shared token budget that bounds retry amplification.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Token-bucket policy for retries across many requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryBudgetConfig {
    /// Maximum tokens retained for short retry bursts.
    pub capacity: u64,
    /// Tokens consumed by one retry attempt.
    pub retry_cost: u64,
    /// Tokens restored by one successful operation.
    pub success_credit: u64,
}

impl Default for RetryBudgetConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            retry_cost: 10,
            success_credit: 1,
        }
    }
}

/// Point-in-time retry-budget diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryBudgetSnapshot {
    /// Maximum token capacity.
    pub capacity: u64,
    /// Tokens currently available.
    pub available: u64,
    /// Retry attempts admitted since creation.
    pub admitted_retries: u64,
    /// Retry attempts rejected since creation.
    pub rejected_retries: u64,
}

/// Shared retry token bucket.
///
/// A retry consumes `retry_cost` tokens and one successful operation restores
/// `success_credit` tokens up to `capacity`. With the default 10:1 cost/credit
/// ratio, sustained retries remain near ten percent while a bounded initial
/// reserve absorbs isolated failures.
#[derive(Debug, Clone)]
pub struct RetryBudget {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    capacity: u64,
    retry_cost: u64,
    success_credit: u64,
    available: AtomicU64,
    admitted_retries: AtomicU64,
    rejected_retries: AtomicU64,
}

impl RetryBudget {
    /// Creates a full token bucket with the supplied policy.
    pub fn new(config: RetryBudgetConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                capacity: config.capacity,
                retry_cost: config.retry_cost.max(1),
                success_credit: config.success_credit,
                available: AtomicU64::new(config.capacity),
                admitted_retries: AtomicU64::new(0),
                rejected_retries: AtomicU64::new(0),
            }),
        }
    }

    /// Attempts to consume tokens for exactly one retry.
    pub fn try_acquire_retry(&self) -> bool {
        let admitted = self
            .inner
            .available
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                (available >= self.inner.retry_cost)
                    .then_some(available.saturating_sub(self.inner.retry_cost))
            })
            .is_ok();
        if admitted {
            self.inner.admitted_retries.fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner.rejected_retries.fetch_add(1, Ordering::Relaxed);
        }
        admitted
    }

    /// Restores bounded credit after a successful operation.
    pub fn record_success(&self) {
        if self.inner.success_credit == 0 || self.inner.capacity == 0 {
            return;
        }
        let _ =
            self.inner
                .available
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                    Some(
                        available
                            .saturating_add(self.inner.success_credit)
                            .min(self.inner.capacity),
                    )
                });
    }

    /// Returns point-in-time token and admission counters.
    pub fn snapshot(&self) -> RetryBudgetSnapshot {
        RetryBudgetSnapshot {
            capacity: self.inner.capacity,
            available: self.inner.available.load(Ordering::Acquire),
            admitted_retries: self.inner.admitted_retries.load(Ordering::Relaxed),
            rejected_retries: self.inner.rejected_retries.load(Ordering::Relaxed),
        }
    }
}

impl Default for RetryBudget {
    fn default() -> Self {
        Self::new(RetryBudgetConfig::default())
    }
}

#[cfg(test)]
#[path = "retry_budget/tests.rs"]
mod tests;
