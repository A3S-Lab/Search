//! Shared, concurrency-safe circuit breaking for search engines.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::EngineFailure;

/// Shared circuit-breaker policy.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Consecutive transient failures required to open a closed circuit.
    pub failure_threshold: u32,
    /// Consecutive empty responses required to open a closed circuit.
    pub empty_threshold: u32,
    /// Default open duration for transient failures and abandoned probes.
    pub transient_open_duration: Duration,
    /// Default open duration for quota, authentication, permission, and other
    /// terminal failures that require external state to change.
    pub terminal_open_duration: Duration,
    /// Maximum provider-advertised or configured open duration.
    pub max_open_duration: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            empty_threshold: 3,
            transient_open_duration: Duration::from_secs(60),
            terminal_open_duration: Duration::from_secs(15 * 60),
            max_open_duration: Duration::from_secs(24 * 60 * 60),
        }
    }
}

/// Observable circuit state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Requests may execute normally.
    Closed,
    /// Requests are skipped until the retry deadline.
    Open,
    /// Exactly one probe is executing after an open period.
    HalfOpen,
}

/// Point-in-time circuit diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitSnapshot {
    /// Current circuit state.
    pub state: CircuitState,
    /// Consecutive transient failures in the current closed generation.
    pub consecutive_failures: u32,
    /// Consecutive empty responses in the current closed generation.
    pub consecutive_empty: u32,
    /// Remaining open time, if the circuit is open.
    pub retry_after: Option<Duration>,
}

/// Reason a request was not admitted by an open circuit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitOpen {
    /// Remaining duration before a half-open probe may execute.
    pub retry_after: Duration,
}

/// Shared registry whose state survives distinct `Search` instances.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    config: Arc<CircuitBreakerConfig>,
    inner: Arc<Mutex<Registry>>,
}

#[derive(Debug, Default)]
struct Registry {
    entries: HashMap<String, CircuitEntry>,
}

#[derive(Debug)]
struct CircuitEntry {
    state: EntryState,
    generation: u64,
    consecutive_failures: u32,
    consecutive_empty: u32,
}

impl Default for CircuitEntry {
    fn default() -> Self {
        Self {
            state: EntryState::Closed,
            generation: 0,
            consecutive_failures: 0,
            consecutive_empty: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum EntryState {
    Closed,
    Open { until: Instant },
    HalfOpen,
}

/// Admission token for one engine attempt.
#[derive(Debug)]
pub struct CircuitPermit {
    completion: Option<PermitCompletion>,
}

#[derive(Debug)]
struct PermitCompletion {
    breaker: CircuitBreaker,
    key: String,
    generation: u64,
    probe: bool,
}

impl CircuitBreaker {
    /// Creates an empty shared registry with the supplied policy.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config: Arc::new(config),
            inner: Arc::new(Mutex::new(Registry::default())),
        }
    }

    /// Attempts to admit one engine call.
    ///
    /// Closed circuits admit concurrent work. Once an open deadline expires,
    /// only one caller receives a half-open probe permit.
    pub fn acquire(&self, key: &str) -> Result<CircuitPermit, CircuitOpen> {
        let key = normalized_key(key);
        let now = Instant::now();
        let mut registry = lock_recover(&self.inner);
        let entry = registry.entries.entry(key.clone()).or_default();
        let probe = match entry.state {
            EntryState::Closed => false,
            EntryState::Open { until } if now < until => {
                return Err(CircuitOpen {
                    retry_after: until.saturating_duration_since(now),
                });
            }
            EntryState::Open { .. } => {
                entry.state = EntryState::HalfOpen;
                true
            }
            EntryState::HalfOpen => {
                return Err(CircuitOpen {
                    retry_after: Duration::ZERO,
                });
            }
        };
        Ok(CircuitPermit {
            completion: Some(PermitCompletion {
                breaker: self.clone(),
                key,
                generation: entry.generation,
                probe,
            }),
        })
    }

    /// Returns diagnostics without admitting a request.
    pub fn snapshot(&self, key: &str) -> CircuitSnapshot {
        let now = Instant::now();
        let registry = lock_recover(&self.inner);
        let Some(entry) = registry.entries.get(&normalized_key(key)) else {
            return CircuitSnapshot {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                consecutive_empty: 0,
                retry_after: None,
            };
        };
        let (state, retry_after) = match entry.state {
            EntryState::Closed => (CircuitState::Closed, None),
            EntryState::Open { until } => (
                CircuitState::Open,
                Some(until.saturating_duration_since(now)),
            ),
            EntryState::HalfOpen => (CircuitState::HalfOpen, Some(Duration::ZERO)),
        };
        CircuitSnapshot {
            state,
            consecutive_failures: entry.consecutive_failures,
            consecutive_empty: entry.consecutive_empty,
            retry_after,
        }
    }

    fn record_success(&self, completion: PermitCompletion) {
        let mut registry = lock_recover(&self.inner);
        let entry = registry.entries.entry(completion.key).or_default();
        if entry.generation != completion.generation {
            return;
        }
        entry.state = EntryState::Closed;
        entry.consecutive_failures = 0;
        entry.consecutive_empty = 0;
    }

    fn record_empty(&self, completion: PermitCompletion) {
        let mut registry = lock_recover(&self.inner);
        let entry = registry.entries.entry(completion.key).or_default();
        if entry.generation != completion.generation {
            return;
        }
        entry.consecutive_failures = 0;
        entry.consecutive_empty = entry.consecutive_empty.saturating_add(1);
        if completion.probe || entry.consecutive_empty >= self.config.empty_threshold.max(1) {
            open_entry(
                entry,
                bounded_duration(
                    self.config.transient_open_duration,
                    self.config.max_open_duration,
                ),
            );
        } else {
            entry.state = EntryState::Closed;
        }
    }

    fn record_failure(&self, completion: PermitCompletion, failure: &EngineFailure) {
        let mut registry = lock_recover(&self.inner);
        let entry = registry.entries.entry(completion.key).or_default();
        if entry.generation != completion.generation {
            return;
        }
        if is_request_scoped_failure(&failure.kind) {
            if completion.probe {
                open_entry(
                    entry,
                    bounded_duration(
                        self.config.transient_open_duration,
                        self.config.max_open_duration,
                    ),
                );
            }
            return;
        }
        entry.consecutive_empty = 0;
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);

        let terminal = is_terminal_failure(&failure.kind);
        let rate_limited = is_rate_limited(&failure.kind);
        let should_open = completion.probe
            || terminal
            || rate_limited
            || entry.consecutive_failures >= self.config.failure_threshold.max(1);
        if !should_open {
            entry.state = EntryState::Closed;
            return;
        }

        let default_duration = if terminal {
            self.config.terminal_open_duration
        } else {
            self.config.transient_open_duration
        };
        let duration = failure
            .retry_after_seconds
            .map(Duration::from_secs)
            .unwrap_or(default_duration);
        open_entry(
            entry,
            bounded_duration(duration, self.config.max_open_duration),
        );
    }

    fn abandon_probe(&self, completion: PermitCompletion) {
        if !completion.probe {
            return;
        }
        let mut registry = lock_recover(&self.inner);
        let entry = registry.entries.entry(completion.key).or_default();
        if entry.generation != completion.generation || !matches!(entry.state, EntryState::HalfOpen)
        {
            return;
        }
        open_entry(
            entry,
            bounded_duration(
                self.config.transient_open_duration,
                self.config.max_open_duration,
            ),
        );
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

impl CircuitPermit {
    /// Closes the circuit and resets failure counters.
    pub fn record_success(mut self) {
        if let Some(completion) = self.completion.take() {
            let breaker = completion.breaker.clone();
            breaker.record_success(completion);
        }
    }

    /// Records a structurally valid response with no useful output.
    pub fn record_empty(mut self) {
        if let Some(completion) = self.completion.take() {
            let breaker = completion.breaker.clone();
            breaker.record_empty(completion);
        }
    }

    /// Records one typed engine failure.
    pub fn record_failure(mut self, failure: &EngineFailure) {
        if let Some(completion) = self.completion.take() {
            let breaker = completion.breaker.clone();
            breaker.record_failure(completion, failure);
        }
    }
}

impl Drop for CircuitPermit {
    fn drop(&mut self) {
        if let Some(completion) = self.completion.take() {
            let breaker = completion.breaker.clone();
            breaker.abandon_probe(completion);
        }
    }
}

fn open_entry(entry: &mut CircuitEntry, duration: Duration) {
    entry.generation = entry.generation.wrapping_add(1);
    entry.state = EntryState::Open {
        until: representable_deadline(Instant::now(), duration),
    };
}

fn is_terminal_failure(kind: &str) -> bool {
    matches!(
        kind,
        "provider_quota" | "provider_authentication" | "provider_permission" | "permission_denied"
    )
}

fn is_rate_limited(kind: &str) -> bool {
    matches!(kind, "provider_rate_limited" | "rate_limited")
}

fn is_request_scoped_failure(kind: &str) -> bool {
    matches!(kind, "provider_invalid_request" | "invalid_query")
}

fn bounded_duration(duration: Duration, maximum: Duration) -> Duration {
    duration.min(maximum)
}

fn normalized_key(key: &str) -> String {
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        "unknown".to_string()
    } else {
        key
    }
}

fn representable_deadline(now: Instant, mut duration: Duration) -> Instant {
    loop {
        if let Some(deadline) = now.checked_add(duration) {
            return deadline;
        }
        duration /= 2;
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
#[path = "circuit/tests.rs"]
mod tests;
