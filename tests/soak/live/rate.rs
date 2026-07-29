use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::corpus::ProviderPolicy;
use super::driver::UpstreamCallReceipt;

#[derive(Default)]
pub(super) struct ProviderRateTracker {
    minimum_intervals: HashMap<String, Duration>,
    last_execution: HashMap<String, Instant>,
    blocked_until: HashMap<String, Instant>,
    pub compliance_violations: u64,
}

impl ProviderRateTracker {
    pub(super) fn new(policies: &[Vec<ProviderPolicy>]) -> Self {
        let minimum_intervals = policies
            .iter()
            .flatten()
            .map(|policy| (policy.scope.clone(), policy.minimum_interval()))
            .collect();
        Self {
            minimum_intervals,
            ..Self::default()
        }
    }

    pub(super) fn record(&mut self, attempt_started: Instant, calls: &[UpstreamCallReceipt]) {
        for call in calls {
            let Some(minimum_interval) = self.minimum_intervals.get(&call.provider_scope).copied()
            else {
                self.compliance_violations = self.compliance_violations.saturating_add(1);
                continue;
            };
            let started = attempt_started + Duration::from_millis(call.started_offset_ms);
            let finished = attempt_started + Duration::from_millis(call.ended_offset_ms);
            if self
                .last_execution
                .get(&call.provider_scope)
                .is_some_and(|previous| {
                    started.saturating_duration_since(*previous) < minimum_interval
                })
            {
                self.compliance_violations = self.compliance_violations.saturating_add(1);
            }
            if self
                .blocked_until
                .get(&call.provider_scope)
                .is_some_and(|deadline| started < *deadline)
            {
                self.compliance_violations = self.compliance_violations.saturating_add(1);
            }
            self.last_execution
                .insert(call.provider_scope.clone(), started);
            if call.retry_after_seconds.is_some() || is_rate_limited(call.failure_kind.as_deref()) {
                let delay = Duration::from_secs(call.retry_after_seconds.unwrap_or_default())
                    .max(minimum_interval);
                let deadline = finished + delay;
                self.blocked_until
                    .entry(call.provider_scope.clone())
                    .and_modify(|current| *current = (*current).max(deadline))
                    .or_insert(deadline);
            }
        }
    }
}

pub(super) fn is_rate_limited(kind: Option<&str>) -> bool {
    matches!(kind, Some("rate_limited" | "provider_rate_limited"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> String {
        format!("sha256:{}", "a".repeat(64))
    }

    fn policy() -> Vec<Vec<ProviderPolicy>> {
        vec![vec![ProviderPolicy {
            scope: scope(),
            minimum_interval_seconds: 10,
        }]]
    }

    fn call(offset_ms: u64) -> UpstreamCallReceipt {
        UpstreamCallReceipt {
            provider_scope: scope(),
            engine_shortcut: "opaque-engine".to_string(),
            started_offset_ms: offset_ms,
            ended_offset_ms: offset_ms + 10,
            is_retry: false,
            failure_kind: None,
            retryable: false,
            retry_after_seconds: None,
        }
    }

    #[test]
    fn predeclared_scope_cadence_is_enforced_across_attempts() {
        let origin = Instant::now();
        let mut tracker = ProviderRateTracker::new(&policy());
        tracker.record(origin, &[call(0)]);
        tracker.record(origin + Duration::from_secs(9), &[call(0)]);
        assert_eq!(tracker.compliance_violations, 1);
        tracker.record(origin + Duration::from_secs(20), &[call(0)]);
        assert_eq!(tracker.compliance_violations, 1);
    }

    #[test]
    fn retry_after_extends_the_predeclared_cadence() {
        let origin = Instant::now();
        let mut limited = call(0);
        limited.failure_kind = Some("rate_limited".to_string());
        limited.retry_after_seconds = Some(30);
        limited.retryable = true;
        let mut tracker = ProviderRateTracker::new(&policy());
        tracker.record(origin, &[limited]);
        tracker.record(origin + Duration::from_secs(20), &[call(0)]);
        assert_eq!(tracker.compliance_violations, 1);
    }

    #[test]
    fn retry_after_is_honored_for_every_valid_retryable_failure() {
        let origin = Instant::now();
        let mut unavailable = call(0);
        unavailable.failure_kind = Some("temporary".to_string());
        unavailable.retry_after_seconds = Some(30);
        unavailable.retryable = true;
        let mut tracker = ProviderRateTracker::new(&policy());
        tracker.record(origin, &[unavailable]);
        tracker.record(origin + Duration::from_secs(20), &[call(0)]);
        assert_eq!(tracker.compliance_violations, 1);
        tracker.record(origin + Duration::from_secs(31), &[call(0)]);
        assert_eq!(tracker.compliance_violations, 1);
    }
}
