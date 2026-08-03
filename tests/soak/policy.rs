use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct LiveCanaryPolicy {
    pub minimum_cases: u64,
    pub minimum_nonempty_rate_lcb: f64,
    pub minimum_structural_sufficiency_rate_lcb: f64,
    pub maximum_terminal_failure_rate_ucb: f64,
    pub maximum_p95_latency_ms: u64,
    pub maximum_p99_latency_ms: u64,
    pub maximum_retry_amplification: f64,
    pub maximum_second_tier_escalation_rate_ucb: f64,
    pub maximum_final_tier_escalation_rate_ucb: f64,
    pub minimum_resource_coverage_ratio: f64,
    pub maximum_resource_sample_gap_ms: u64,
    pub maximum_rss_growth_kib: i64,
    pub maximum_tail_rss_slope_kib_per_minute: f64,
    pub maximum_fd_growth: isize,
}

impl LiveCanaryPolicy {
    pub(super) const fn release_floor() -> Self {
        Self {
            minimum_cases: 40,
            minimum_nonempty_rate_lcb: 0.90,
            minimum_structural_sufficiency_rate_lcb: 0.90,
            maximum_terminal_failure_rate_ucb: 0.10,
            maximum_p95_latency_ms: 10_000,
            maximum_p99_latency_ms: 30_000,
            maximum_retry_amplification: 1.05,
            maximum_second_tier_escalation_rate_ucb: 0.50,
            maximum_final_tier_escalation_rate_ucb: 0.20,
            minimum_resource_coverage_ratio: 0.90,
            maximum_resource_sample_gap_ms: 180_000,
            maximum_rss_growth_kib: 131_072,
            maximum_tail_rss_slope_kib_per_minute: 1_024.0,
            maximum_fd_growth: 16,
        }
    }

    pub(super) fn from_env() -> Self {
        let floor = Self::release_floor();
        let policy = Self {
            minimum_cases: env_value("A3S_SEARCH_LIVE_CANARY_MIN_CASES", floor.minimum_cases),
            minimum_nonempty_rate_lcb: env_value(
                "A3S_SEARCH_LIVE_CANARY_MIN_NONEMPTY_RATE_LCB",
                floor.minimum_nonempty_rate_lcb,
            ),
            minimum_structural_sufficiency_rate_lcb: env_value(
                "A3S_SEARCH_LIVE_CANARY_MIN_STRUCTURAL_SUFFICIENCY_RATE_LCB",
                floor.minimum_structural_sufficiency_rate_lcb,
            ),
            maximum_terminal_failure_rate_ucb: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_TERMINAL_FAILURE_RATE_UCB",
                floor.maximum_terminal_failure_rate_ucb,
            ),
            maximum_p95_latency_ms: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_P95_LATENCY_MS",
                floor.maximum_p95_latency_ms,
            ),
            maximum_p99_latency_ms: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_P99_LATENCY_MS",
                floor.maximum_p99_latency_ms,
            ),
            maximum_retry_amplification: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_RETRY_AMPLIFICATION",
                floor.maximum_retry_amplification,
            ),
            maximum_second_tier_escalation_rate_ucb: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_SECOND_TIER_ESCALATION_RATE_UCB",
                floor.maximum_second_tier_escalation_rate_ucb,
            ),
            maximum_final_tier_escalation_rate_ucb: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_FINAL_TIER_ESCALATION_RATE_UCB",
                floor.maximum_final_tier_escalation_rate_ucb,
            ),
            minimum_resource_coverage_ratio: env_value(
                "A3S_SEARCH_LIVE_CANARY_MIN_RESOURCE_COVERAGE_RATIO",
                floor.minimum_resource_coverage_ratio,
            ),
            maximum_resource_sample_gap_ms: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_RESOURCE_SAMPLE_GAP_MS",
                floor.maximum_resource_sample_gap_ms,
            ),
            maximum_rss_growth_kib: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_RSS_GROWTH_KIB",
                floor.maximum_rss_growth_kib,
            ),
            maximum_tail_rss_slope_kib_per_minute: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_TAIL_RSS_SLOPE_KIB_PER_MINUTE",
                floor.maximum_tail_rss_slope_kib_per_minute,
            ),
            maximum_fd_growth: env_value(
                "A3S_SEARCH_LIVE_CANARY_MAX_FD_GROWTH",
                floor.maximum_fd_growth,
            ),
        };
        policy.assert_valid();
        policy.assert_not_weaker_than(&floor);
        policy
    }

    fn assert_valid(&self) {
        for (name, value) in [
            ("minimum non-empty rate", self.minimum_nonempty_rate_lcb),
            (
                "minimum structural sufficiency rate",
                self.minimum_structural_sufficiency_rate_lcb,
            ),
            (
                "maximum terminal failure rate",
                self.maximum_terminal_failure_rate_ucb,
            ),
            (
                "maximum second-tier escalation rate",
                self.maximum_second_tier_escalation_rate_ucb,
            ),
            (
                "maximum final-tier escalation rate",
                self.maximum_final_tier_escalation_rate_ucb,
            ),
            (
                "minimum resource coverage ratio",
                self.minimum_resource_coverage_ratio,
            ),
        ] {
            assert!(
                value.is_finite() && (0.0..=1.0).contains(&value),
                "{name} must be a finite probability"
            );
        }
        assert!(
            self.minimum_cases >= 40,
            "sealed canary needs at least 40 cases"
        );
        assert!(
            self.maximum_p95_latency_ms > 0
                && self.maximum_p99_latency_ms >= self.maximum_p95_latency_ms,
            "latency limits must be positive and ordered"
        );
        assert!(
            self.maximum_retry_amplification.is_finite() && self.maximum_retry_amplification >= 1.0,
            "retry amplification limit must be finite and at least one"
        );
        assert!(self.maximum_resource_sample_gap_ms > 0);
        assert!(self.maximum_rss_growth_kib >= 0);
        assert!(
            self.maximum_tail_rss_slope_kib_per_minute.is_finite()
                && self.maximum_tail_rss_slope_kib_per_minute >= 0.0
        );
        assert!(self.maximum_fd_growth >= 0);
    }

    fn assert_not_weaker_than(&self, floor: &Self) {
        assert!(
            self.minimum_cases >= floor.minimum_cases,
            "case count weakens the release floor"
        );
        assert!(
            self.minimum_nonempty_rate_lcb >= floor.minimum_nonempty_rate_lcb,
            "non-empty rate weakens the release floor"
        );
        assert!(
            self.minimum_structural_sufficiency_rate_lcb
                >= floor.minimum_structural_sufficiency_rate_lcb,
            "structural sufficiency rate weakens the release floor"
        );
        assert!(
            self.maximum_terminal_failure_rate_ucb <= floor.maximum_terminal_failure_rate_ucb,
            "terminal failure rate weakens the release floor"
        );
        assert!(self.maximum_p95_latency_ms <= floor.maximum_p95_latency_ms);
        assert!(self.maximum_p99_latency_ms <= floor.maximum_p99_latency_ms);
        assert!(self.maximum_retry_amplification <= floor.maximum_retry_amplification);
        assert!(
            self.maximum_second_tier_escalation_rate_ucb
                <= floor.maximum_second_tier_escalation_rate_ucb
        );
        assert!(
            self.maximum_final_tier_escalation_rate_ucb
                <= floor.maximum_final_tier_escalation_rate_ucb
        );
        assert!(self.minimum_resource_coverage_ratio >= floor.minimum_resource_coverage_ratio);
        assert!(self.maximum_resource_sample_gap_ms <= floor.maximum_resource_sample_gap_ms);
        assert!(self.maximum_rss_growth_kib <= floor.maximum_rss_growth_kib);
        assert!(
            self.maximum_tail_rss_slope_kib_per_minute
                <= floor.maximum_tail_rss_slope_kib_per_minute
        );
        assert!(self.maximum_fd_growth <= floor.maximum_fd_growth);
    }
}

fn env_value<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|error| panic!("{name} is invalid: {error}")),
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => panic!("cannot read {name}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_environment_override_can_only_tighten_the_release_floor() {
        let floor = LiveCanaryPolicy::release_floor();
        let weakeners: [fn(&mut LiveCanaryPolicy); 14] = [
            |policy| policy.minimum_cases -= 1,
            |policy| policy.minimum_nonempty_rate_lcb -= 0.01,
            |policy| policy.minimum_structural_sufficiency_rate_lcb -= 0.01,
            |policy| policy.maximum_terminal_failure_rate_ucb += 0.01,
            |policy| policy.maximum_p95_latency_ms += 1,
            |policy| policy.maximum_p99_latency_ms += 1,
            |policy| policy.maximum_retry_amplification += 0.01,
            |policy| policy.maximum_second_tier_escalation_rate_ucb += 0.01,
            |policy| policy.maximum_final_tier_escalation_rate_ucb += 0.01,
            |policy| policy.minimum_resource_coverage_ratio -= 0.01,
            |policy| policy.maximum_resource_sample_gap_ms += 1,
            |policy| policy.maximum_rss_growth_kib += 1,
            |policy| policy.maximum_tail_rss_slope_kib_per_minute += 1.0,
            |policy| policy.maximum_fd_growth += 1,
        ];
        for weaken in weakeners {
            let mut candidate = floor;
            weaken(&mut candidate);
            assert!(
                std::panic::catch_unwind(|| candidate.assert_not_weaker_than(&floor)).is_err(),
                "a weakened canary policy was accepted: {candidate:?}"
            );
        }
    }
}
