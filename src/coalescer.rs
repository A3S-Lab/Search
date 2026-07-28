//! Bounded single-flight coordination for identical in-flight searches.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::Notify;

use crate::{EngineConfig, SearchQuery, SearchResults};

/// Capacity policy for an in-flight search coalescer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SearchCoalescerConfig {
    /// Maximum number of distinct request identities tracked concurrently.
    ///
    /// Once the bound is reached, new distinct identities bypass coalescing
    /// and continue through the normal search path. Existing identities can
    /// still join their current flight. A value of zero disables tracking.
    pub max_in_flight: usize,
}

impl Default for SearchCoalescerConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 1_024,
        }
    }
}

/// Point-in-time and cumulative single-flight diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SearchCoalescerSnapshot {
    /// Configured maximum distinct request identities tracked concurrently.
    pub max_in_flight: usize,
    /// Distinct request identities currently executing.
    pub in_flight: usize,
    /// Requests that created and executed a tracked flight.
    pub leader_requests: u64,
    /// Requests that reused an already running identical flight.
    pub shared_requests: u64,
    /// Requests that bypassed tracking because distinct-flight capacity was full.
    pub bypassed_requests: u64,
    /// Flights abandoned before producing a result, usually by cancellation.
    pub abandoned_requests: u64,
}

/// Shared, bounded registry that collapses identical concurrent searches.
///
/// The registry stores only in-flight work and removes a flight immediately
/// after completion; it is not a result cache. Clone one registry only across
/// callers that share compatible tenant, credential, endpoint, proxy, safe
/// search, freshness, and policy boundaries.
#[derive(Debug, Clone)]
pub struct SearchCoalescer {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: SearchCoalescerConfig,
    flights: Mutex<HashMap<SearchRequestKey, Arc<SearchFlight>>>,
    leader_requests: AtomicU64,
    shared_requests: AtomicU64,
    bypassed_requests: AtomicU64,
    abandoned_requests: AtomicU64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SearchRequestKey {
    query: SearchQuery,
    engines: Vec<EngineFingerprint>,
    timeout_override: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EngineFingerprint {
    name: String,
    shortcut: String,
    categories: Vec<crate::EngineCategory>,
    weight_bits: u64,
    timeout: u64,
    enabled: bool,
    paging: bool,
    safesearch: bool,
}

impl From<&EngineConfig> for EngineFingerprint {
    fn from(config: &EngineConfig) -> Self {
        Self {
            name: config.name.clone(),
            shortcut: config.shortcut.clone(),
            categories: config.categories.clone(),
            weight_bits: config.weight.to_bits(),
            timeout: config.timeout,
            enabled: config.enabled,
            paging: config.paging,
            safesearch: config.safesearch,
        }
    }
}

impl SearchRequestKey {
    pub(crate) fn new<'a>(
        query: SearchQuery,
        engines: impl IntoIterator<Item = &'a EngineConfig>,
        timeout_override: Option<Duration>,
    ) -> Self {
        Self {
            query,
            engines: engines.into_iter().map(EngineFingerprint::from).collect(),
            timeout_override,
        }
    }
}

pub(crate) enum SearchCoalescingAdmission {
    Leader(SearchFlightLeader),
    Follower(Arc<SearchFlight>),
    Bypass,
}

#[derive(Debug)]
pub(crate) struct SearchFlight {
    state: Mutex<SearchFlightState>,
    notify: Notify,
}

#[derive(Debug)]
enum SearchFlightState {
    Running,
    Completed(Box<SearchResults>),
    Abandoned,
}

pub(crate) struct SearchFlightLeader {
    coalescer: SearchCoalescer,
    key: SearchRequestKey,
    flight: Arc<SearchFlight>,
    finished: bool,
}

impl SearchCoalescer {
    /// Creates an empty coalescer with a bounded distinct-flight capacity.
    pub fn new(config: SearchCoalescerConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                flights: Mutex::new(HashMap::new()),
                leader_requests: AtomicU64::new(0),
                shared_requests: AtomicU64::new(0),
                bypassed_requests: AtomicU64::new(0),
                abandoned_requests: AtomicU64::new(0),
            }),
        }
    }

    /// Returns current capacity and cumulative sharing diagnostics.
    pub fn snapshot(&self) -> SearchCoalescerSnapshot {
        SearchCoalescerSnapshot {
            max_in_flight: self.inner.config.max_in_flight,
            in_flight: lock_recover(&self.inner.flights).len(),
            leader_requests: self.inner.leader_requests.load(Ordering::Relaxed),
            shared_requests: self.inner.shared_requests.load(Ordering::Relaxed),
            bypassed_requests: self.inner.bypassed_requests.load(Ordering::Relaxed),
            abandoned_requests: self.inner.abandoned_requests.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn acquire(&self, key: SearchRequestKey) -> SearchCoalescingAdmission {
        let mut flights = lock_recover(&self.inner.flights);
        if let Some(flight) = flights.get(&key) {
            self.inner.shared_requests.fetch_add(1, Ordering::Relaxed);
            return SearchCoalescingAdmission::Follower(Arc::clone(flight));
        }
        if flights.len() >= self.inner.config.max_in_flight {
            self.inner.bypassed_requests.fetch_add(1, Ordering::Relaxed);
            return SearchCoalescingAdmission::Bypass;
        }

        let flight = Arc::new(SearchFlight {
            state: Mutex::new(SearchFlightState::Running),
            notify: Notify::new(),
        });
        flights.insert(key.clone(), Arc::clone(&flight));
        self.inner.leader_requests.fetch_add(1, Ordering::Relaxed);
        SearchCoalescingAdmission::Leader(SearchFlightLeader {
            coalescer: self.clone(),
            key,
            flight,
            finished: false,
        })
    }

    fn remove(&self, key: &SearchRequestKey, flight: &Arc<SearchFlight>) {
        let mut flights = lock_recover(&self.inner.flights);
        if flights
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, flight))
        {
            flights.remove(key);
        }
    }
}

impl Default for SearchCoalescer {
    fn default() -> Self {
        Self::new(SearchCoalescerConfig::default())
    }
}

impl SearchFlight {
    pub(crate) async fn wait(&self) -> Option<SearchResults> {
        loop {
            let notified = self.notify.notified();
            match &*lock_recover(&self.state) {
                SearchFlightState::Running => {}
                SearchFlightState::Completed(results) => return Some(results.as_ref().clone()),
                SearchFlightState::Abandoned => return None,
            }
            notified.await;
        }
    }
}

impl SearchFlightLeader {
    pub(crate) fn complete(mut self, results: SearchResults) {
        *lock_recover(&self.flight.state) = SearchFlightState::Completed(Box::new(results));
        self.coalescer.remove(&self.key, &self.flight);
        self.finished = true;
        self.flight.notify.notify_waiters();
    }
}

impl Drop for SearchFlightLeader {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        *lock_recover(&self.flight.state) = SearchFlightState::Abandoned;
        self.coalescer.remove(&self.key, &self.flight);
        self.coalescer
            .inner
            .abandoned_requests
            .fetch_add(1, Ordering::Relaxed);
        self.flight.notify.notify_waiters();
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
