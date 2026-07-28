use std::collections::VecDeque;
use std::time::Duration;

/// Optional count-based circuit window for intermittent failures and slow calls.
#[derive(Debug, Clone)]
pub struct CircuitWindowConfig {
    /// Maximum recent outcomes retained per engine.
    pub size: usize,
    /// Minimum outcomes required before rate thresholds are evaluated.
    pub minimum_calls: usize,
    /// Failure ratio in the inclusive `0.0..=1.0` range that opens a circuit.
    pub failure_rate_threshold: f64,
    /// Successful or failed calls at least this slow count toward the slow rate.
    pub slow_call_duration: Duration,
    /// Slow-call ratio in the inclusive `0.0..=1.0` range that opens a circuit.
    pub slow_call_rate_threshold: f64,
}

impl Default for CircuitWindowConfig {
    fn default() -> Self {
        Self {
            size: 20,
            minimum_calls: 10,
            failure_rate_threshold: 0.5,
            slow_call_duration: Duration::from_secs(5),
            slow_call_rate_threshold: 0.8,
        }
    }
}

#[derive(Debug)]
pub(super) struct SlidingWindow {
    config: Option<NormalizedWindowConfig>,
    outcomes: VecDeque<CallOutcome>,
    failures: usize,
    slow_calls: usize,
}

#[derive(Debug, Clone, Copy)]
struct NormalizedWindowConfig {
    size: usize,
    minimum_calls: usize,
    failure_rate_threshold: f64,
    slow_call_duration: Duration,
    slow_call_rate_threshold: f64,
}

#[derive(Debug, Clone, Copy)]
struct CallOutcome {
    failed: bool,
    slow: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct WindowSnapshot {
    pub recorded_calls: usize,
    pub failure_rate: Option<f64>,
    pub slow_call_rate: Option<f64>,
}

impl SlidingWindow {
    const MAX_SIZE: usize = 10_000;

    pub(super) fn new(config: Option<&CircuitWindowConfig>) -> Self {
        let config = config.map(|config| {
            let size = config.size.clamp(1, Self::MAX_SIZE);
            NormalizedWindowConfig {
                size,
                minimum_calls: config.minimum_calls.clamp(1, size),
                failure_rate_threshold: normalized_rate(config.failure_rate_threshold, 0.5),
                slow_call_duration: config.slow_call_duration,
                slow_call_rate_threshold: normalized_rate(config.slow_call_rate_threshold, 0.8),
            }
        });
        Self {
            config,
            outcomes: VecDeque::new(),
            failures: 0,
            slow_calls: 0,
        }
    }

    pub(super) fn record(&mut self, failed: bool, duration: Duration) -> bool {
        let Some(config) = self.config else {
            return false;
        };
        let outcome = CallOutcome {
            failed,
            slow: duration >= config.slow_call_duration,
        };
        if self.outcomes.len() == config.size {
            if let Some(evicted) = self.outcomes.pop_front() {
                self.failures = self.failures.saturating_sub(usize::from(evicted.failed));
                self.slow_calls = self.slow_calls.saturating_sub(usize::from(evicted.slow));
            }
        }
        self.outcomes.push_back(outcome);
        self.failures = self.failures.saturating_add(usize::from(outcome.failed));
        self.slow_calls = self.slow_calls.saturating_add(usize::from(outcome.slow));

        self.outcomes.len() >= config.minimum_calls
            && (self.failure_rate() >= config.failure_rate_threshold
                || self.slow_call_rate() >= config.slow_call_rate_threshold)
    }

    pub(super) fn clear(&mut self) {
        self.outcomes.clear();
        self.failures = 0;
        self.slow_calls = 0;
    }

    pub(super) fn snapshot(&self) -> WindowSnapshot {
        let enough_calls = self
            .config
            .is_some_and(|config| self.outcomes.len() >= config.minimum_calls);
        WindowSnapshot {
            recorded_calls: self.outcomes.len(),
            failure_rate: enough_calls.then(|| self.failure_rate()),
            slow_call_rate: enough_calls.then(|| self.slow_call_rate()),
        }
    }

    fn failure_rate(&self) -> f64 {
        ratio(self.failures, self.outcomes.len())
    }

    fn slow_call_rate(&self) -> f64 {
        ratio(self.slow_calls, self.outcomes.len())
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn normalized_rate(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        fallback
    }
}
