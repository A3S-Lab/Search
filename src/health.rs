//! Engine health monitoring with automatic suspend/resume.
//!
//! Tracks consecutive failures per engine. When an engine exceeds
//! `max_failures`, it is suspended for `suspend_duration`. After
//! the suspension expires, the engine is automatically re-enabled
//! for a probe request.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Configuration for the health monitor.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Number of consecutive failures before suspending an engine.
    pub max_failures: u32,
    /// How long to suspend a failing engine.
    pub suspend_duration: Duration,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            max_failures: 3,
            suspend_duration: Duration::from_secs(60),
        }
    }
}

/// Per-engine health state.
#[derive(Debug)]
struct EngineHealth {
    consecutive_failures: u32,
    suspended_until: Option<Instant>,
}

impl EngineHealth {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            suspended_until: None,
        }
    }
}

/// Monitors engine health and auto-suspends failing engines.
#[derive(Debug)]
pub struct HealthMonitor {
    config: HealthConfig,
    engines: HashMap<String, EngineHealth>,
}

impl HealthMonitor {
    /// Creates a new health monitor with the given configuration.
    pub fn new(config: HealthConfig) -> Self {
        Self {
            config,
            engines: HashMap::new(),
        }
    }

    /// Returns whether the engine is currently suspended.
    pub fn is_suspended(&self, name: &str) -> bool {
        if let Some(health) = self.engines.get(name) {
            if let Some(until) = health.suspended_until {
                return Instant::now() < until;
            }
        }
        false
    }

    /// Records a successful search for the engine, resetting its failure count.
    pub fn record_success(&mut self, name: &str) {
        let health = self
            .engines
            .entry(name.to_string())
            .or_insert_with(EngineHealth::new);
        health.consecutive_failures = 0;
        health.suspended_until = None;
    }

    /// Records a failed search. Suspends the engine if it exceeds `max_failures`.
    pub fn record_failure(&mut self, name: &str) {
        let health = self
            .engines
            .entry(name.to_string())
            .or_insert_with(EngineHealth::new);
        health.consecutive_failures = health.consecutive_failures.saturating_add(1);

        if health.consecutive_failures >= self.config.max_failures {
            health.suspended_until = Some(representable_deadline(
                Instant::now(),
                self.config.suspend_duration,
            ));
        }
    }

    /// Returns the number of consecutive failures for an engine.
    pub fn failure_count(&self, name: &str) -> u32 {
        self.engines
            .get(name)
            .map(|h| h.consecutive_failures)
            .unwrap_or(0)
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

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new(HealthConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_config_default() {
        let config = HealthConfig::default();
        assert_eq!(config.max_failures, 3);
        assert_eq!(config.suspend_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_health_monitor_new_engine_not_suspended() {
        let monitor = HealthMonitor::default();
        assert!(!monitor.is_suspended("google"));
    }

    #[test]
    fn test_record_success_resets_failures() {
        let mut monitor = HealthMonitor::default();
        monitor.record_failure("google");
        monitor.record_failure("google");
        assert_eq!(monitor.failure_count("google"), 2);

        monitor.record_success("google");
        assert_eq!(monitor.failure_count("google"), 0);
        assert!(!monitor.is_suspended("google"));
    }

    #[test]
    fn test_suspend_after_max_failures() {
        let config = HealthConfig {
            max_failures: 2,
            suspend_duration: Duration::from_secs(60),
        };
        let mut monitor = HealthMonitor::new(config);

        monitor.record_failure("google");
        assert!(!monitor.is_suspended("google"));

        monitor.record_failure("google");
        assert!(monitor.is_suspended("google"));
    }

    #[test]
    fn test_suspend_expires() {
        let config = HealthConfig {
            max_failures: 1,
            suspend_duration: Duration::from_millis(0),
        };
        let mut monitor = HealthMonitor::new(config);

        monitor.record_failure("google");
        // With 0ms duration, suspension expires immediately
        assert!(!monitor.is_suspended("google"));
    }

    #[test]
    fn test_success_clears_suspension() {
        let config = HealthConfig {
            max_failures: 1,
            suspend_duration: Duration::from_secs(3600),
        };
        let mut monitor = HealthMonitor::new(config);

        monitor.record_failure("google");
        assert!(monitor.is_suspended("google"));

        monitor.record_success("google");
        assert!(!monitor.is_suspended("google"));
    }

    #[test]
    fn test_independent_engines() {
        let mut monitor = HealthMonitor::new(HealthConfig {
            max_failures: 2,
            ..Default::default()
        });

        monitor.record_failure("google");
        monitor.record_failure("google");
        monitor.record_failure("bing");

        assert!(monitor.is_suspended("google"));
        assert!(!monitor.is_suspended("bing"));
    }

    #[test]
    fn test_failure_count_unknown_engine() {
        let monitor = HealthMonitor::default();
        assert_eq!(monitor.failure_count("nonexistent"), 0);
    }

    #[test]
    fn test_extreme_suspension_duration_does_not_overflow() {
        let mut monitor = HealthMonitor::new(HealthConfig {
            max_failures: 1,
            suspend_duration: Duration::MAX,
        });

        monitor.record_failure("extreme");

        assert_eq!(monitor.failure_count("extreme"), 1);
        assert!(monitor.is_suspended("extreme"));
    }

    #[test]
    fn test_failure_count_saturates() {
        let mut monitor = HealthMonitor::default();
        monitor.engines.insert(
            "saturated".to_string(),
            EngineHealth {
                consecutive_failures: u32::MAX,
                suspended_until: None,
            },
        );

        monitor.record_failure("saturated");

        assert_eq!(monitor.failure_count("saturated"), u32::MAX);
    }
}
