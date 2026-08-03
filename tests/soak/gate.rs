use serde::Serialize;

use super::policy::LiveCanaryPolicy;

const CONFIDENCE_Z_95: f64 = 1.959_963_984_540_054;

impl LiveCanaryPolicy {
    pub(super) fn evaluate(&self, measurements: &LiveCanaryMeasurements) -> LiveCanaryGateReport {
        let nonempty = RateEstimate::new(measurements.nonempty, measurements.attempts);
        let structural_sufficiency =
            RateEstimate::new(measurements.structurally_sufficient, measurements.attempts);
        let terminal_failures =
            RateEstimate::new(measurements.terminal_failures, measurements.attempts);
        let circuit_open = RateEstimate::new(measurements.circuit_open, measurements.engine_slots);
        let rate_limited = RateEstimate::new(
            measurements.rate_limited_outcomes,
            measurements
                .upstream_calls
                .saturating_add(measurements.retry_attempts),
        );
        let second_tier_escalation =
            RateEstimate::new(measurements.second_tier_escalations, measurements.attempts);
        let final_tier_escalation =
            RateEstimate::new(measurements.final_tier_escalations, measurements.attempts);
        let retry_amplification = (measurements.upstream_calls > 0).then(|| {
            measurements
                .upstream_calls
                .saturating_add(measurements.retry_attempts) as f64
                / measurements.upstream_calls as f64
        });
        let mut violations = Vec::new();

        if measurements.expected_cases < self.minimum_cases {
            violations.push(format!(
                "sealed cases {} below minimum {}",
                measurements.expected_cases, self.minimum_cases
            ));
        }
        if measurements.attempts != measurements.expected_cases {
            violations.push(format!(
                "case execution {} does not equal sealed case count {}",
                measurements.attempts, measurements.expected_cases
            ));
        }
        if measurements.attempt_log_start_records != measurements.attempts {
            violations.push(format!(
                "attempt-start log records {} do not equal attempts {}",
                measurements.attempt_log_start_records, measurements.attempts
            ));
        }
        if measurements.attempt_log_terminal_records != measurements.attempts {
            violations.push(format!(
                "attempt-terminal log records {} do not equal attempts {}",
                measurements.attempt_log_terminal_records, measurements.attempts
            ));
        }
        if measurements
            .completed
            .saturating_add(measurements.terminal_failures)
            != measurements.attempts
        {
            violations.push("request accounting does not reconcile".to_string());
        }
        if measurements.pre_execution_failures > 0 {
            violations.push(format!(
                "{} pre-execution terminal failures",
                measurements.pre_execution_failures
            ));
        }
        if measurements.nonempty > measurements.completed
            || measurements.structurally_sufficient > measurements.nonempty
        {
            violations.push("retrieval counters are internally inconsistent".to_string());
        }
        if measurements.engine_slots == 0
            || measurements.circuit_open > measurements.engine_slots
            || measurements.upstream_calls > measurements.engine_slots
        {
            violations.push("engine-attempt accounting is internally inconsistent".to_string());
        }
        if measurements.final_tier_escalations > measurements.second_tier_escalations
            || measurements.second_tier_escalations > measurements.attempts
        {
            violations.push("tier-escalation accounting is internally inconsistent".to_string());
        }
        if nonempty.lower_95 < self.minimum_nonempty_rate_lcb {
            violations.push(format!(
                "non-empty rate lower bound {:.6} below {:.6}",
                nonempty.lower_95, self.minimum_nonempty_rate_lcb
            ));
        }
        if structural_sufficiency.lower_95 < self.minimum_structural_sufficiency_rate_lcb {
            violations.push(format!(
                "structural sufficiency rate lower bound {:.6} below {:.6}",
                structural_sufficiency.lower_95, self.minimum_structural_sufficiency_rate_lcb
            ));
        }
        if terminal_failures.upper_95 > self.maximum_terminal_failure_rate_ucb {
            violations.push(format!(
                "terminal failure rate upper bound {:.6} above {:.6}",
                terminal_failures.upper_95, self.maximum_terminal_failure_rate_ucb
            ));
        }
        if measurements.p95_latency_ms > self.maximum_p95_latency_ms {
            violations.push(format!(
                "p95 latency {} ms above {} ms",
                measurements.p95_latency_ms, self.maximum_p95_latency_ms
            ));
        }
        if measurements.p99_latency_ms > self.maximum_p99_latency_ms {
            violations.push(format!(
                "p99 latency {} ms above {} ms",
                measurements.p99_latency_ms, self.maximum_p99_latency_ms
            ));
        }
        match retry_amplification {
            Some(value) if value <= self.maximum_retry_amplification => {}
            Some(value) => violations.push(format!(
                "retry amplification {value:.6} above {:.6}",
                self.maximum_retry_amplification
            )),
            None => violations.push("retry amplification has no upstream denominator".to_string()),
        }
        check_rate_upper_bound(
            &mut violations,
            "second-tier escalation rate",
            second_tier_escalation,
            self.maximum_second_tier_escalation_rate_ucb,
        );
        check_rate_upper_bound(
            &mut violations,
            "final-tier escalation rate",
            final_tier_escalation,
            self.maximum_final_tier_escalation_rate_ucb,
        );
        if measurements.rate_limit_compliance_violations > 0 {
            violations.push(format!(
                "{} provider rate-limit compliance violations",
                measurements.rate_limit_compliance_violations
            ));
        }
        if measurements.receipt_integrity_violations > 0 {
            violations.push(format!(
                "{} receipt-integrity violations",
                measurements.receipt_integrity_violations
            ));
        }
        if measurements.resource_samples < 2 {
            violations.push("resource timeline contains fewer than two samples".to_string());
        }
        if !measurements.resource_coverage_ratio.is_finite()
            || measurements.resource_coverage_ratio < self.minimum_resource_coverage_ratio
        {
            violations.push(format!(
                "resource coverage ratio {:.6} below {:.6}",
                measurements.resource_coverage_ratio, self.minimum_resource_coverage_ratio
            ));
        }
        if measurements.maximum_resource_sample_gap_ms > self.maximum_resource_sample_gap_ms {
            violations.push(format!(
                "resource sample gap {} ms above {} ms",
                measurements.maximum_resource_sample_gap_ms, self.maximum_resource_sample_gap_ms
            ));
        }
        if measurements.rss_growth_kib > self.maximum_rss_growth_kib {
            violations.push(format!(
                "RSS growth {} KiB above {} KiB",
                measurements.rss_growth_kib, self.maximum_rss_growth_kib
            ));
        }
        if measurements.tail_rss_slope_kib_per_minute > self.maximum_tail_rss_slope_kib_per_minute {
            violations.push(format!(
                "tail RSS slope {:.3} KiB/min above {:.3} KiB/min",
                measurements.tail_rss_slope_kib_per_minute,
                self.maximum_tail_rss_slope_kib_per_minute
            ));
        }
        if measurements.fd_growth > self.maximum_fd_growth {
            violations.push(format!(
                "file-descriptor growth {} above {}",
                measurements.fd_growth, self.maximum_fd_growth
            ));
        }

        LiveCanaryGateReport {
            passed: violations.is_empty(),
            nonempty,
            structural_sufficiency,
            terminal_failures,
            circuit_open,
            rate_limited,
            second_tier_escalation,
            final_tier_escalation,
            retry_amplification,
            violations,
        }
    }
}

fn check_rate_upper_bound(
    violations: &mut Vec<String>,
    name: &str,
    estimate: RateEstimate,
    maximum: f64,
) {
    if estimate.upper_95 > maximum {
        violations.push(format!(
            "{name} upper bound {:.6} above {:.6}",
            estimate.upper_95, maximum
        ));
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub(super) struct LiveCanaryMeasurements {
    pub expected_cases: u64,
    pub attempts: u64,
    pub attempt_log_start_records: u64,
    pub attempt_log_terminal_records: u64,
    pub completed: u64,
    pub terminal_failures: u64,
    pub pre_execution_failures: u64,
    pub nonempty: u64,
    pub structurally_sufficient: u64,
    pub engine_slots: u64,
    pub upstream_calls: u64,
    pub retry_attempts: u64,
    pub rate_limited_outcomes: u64,
    pub circuit_open: u64,
    pub second_tier_escalations: u64,
    pub final_tier_escalations: u64,
    pub rate_limit_compliance_violations: u64,
    pub receipt_integrity_violations: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub resource_samples: usize,
    pub resource_coverage_ratio: f64,
    pub maximum_resource_sample_gap_ms: u64,
    pub rss_growth_kib: i64,
    pub tail_rss_slope_kib_per_minute: f64,
    pub fd_growth: isize,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct LiveCanaryGateReport {
    pub passed: bool,
    pub nonempty: RateEstimate,
    pub structural_sufficiency: RateEstimate,
    pub terminal_failures: RateEstimate,
    pub circuit_open: RateEstimate,
    pub rate_limited: RateEstimate,
    pub second_tier_escalation: RateEstimate,
    pub final_tier_escalation: RateEstimate,
    pub retry_amplification: Option<f64>,
    pub violations: Vec<String>,
}

impl LiveCanaryGateReport {
    pub(super) fn assert_passed(&self) {
        assert!(
            self.passed,
            "live canary gate failed: {}",
            self.violations.join("; ")
        );
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct RateEstimate {
    pub observed: f64,
    pub lower_95: f64,
    pub upper_95: f64,
}

impl RateEstimate {
    fn new(successes: u64, trials: u64) -> Self {
        if trials == 0 || successes > trials {
            return Self {
                observed: 0.0,
                lower_95: 0.0,
                upper_95: 1.0,
            };
        }
        let observed = successes as f64 / trials as f64;
        let trials = trials as f64;
        let z_squared = CONFIDENCE_Z_95 * CONFIDENCE_Z_95;
        let denominator = 1.0 + z_squared / trials;
        let center = (observed + z_squared / (2.0 * trials)) / denominator;
        let margin = CONFIDENCE_Z_95
            * ((observed * (1.0 - observed) / trials + z_squared / (4.0 * trials * trials)).sqrt())
            / denominator;
        Self {
            observed,
            lower_95: (center - margin).clamp(0.0, 1.0),
            upper_95: (center + margin).clamp(0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> LiveCanaryPolicy {
        LiveCanaryPolicy::release_floor()
    }

    fn passing_measurements() -> LiveCanaryMeasurements {
        LiveCanaryMeasurements {
            expected_cases: 40,
            attempts: 40,
            attempt_log_start_records: 40,
            attempt_log_terminal_records: 40,
            completed: 40,
            nonempty: 40,
            structurally_sufficient: 40,
            engine_slots: 100,
            upstream_calls: 100,
            retry_attempts: 2,
            circuit_open: 2,
            second_tier_escalations: 5,
            final_tier_escalations: 1,
            p95_latency_ms: 2_000,
            p99_latency_ms: 8_000,
            resource_samples: 40,
            resource_coverage_ratio: 0.95,
            maximum_resource_sample_gap_ms: 60_000,
            rss_growth_kib: 4_096,
            tail_rss_slope_kib_per_minute: 32.0,
            fd_growth: 2,
            ..LiveCanaryMeasurements::default()
        }
    }

    #[test]
    fn healthy_sealed_canary_passes_every_predeclared_gate() {
        let report = policy().evaluate(&passing_measurements());
        assert!(report.passed, "{:?}", report.violations);
    }

    #[test]
    fn upstream_limits_and_open_circuits_remain_audited_without_blocking_release() {
        let mut measurements = passing_measurements();
        measurements.circuit_open = 50;
        measurements.rate_limited_outcomes = 30;

        let report = policy().evaluate(&measurements);

        assert!(report.passed, "{:?}", report.violations);
        assert_eq!(report.circuit_open.observed, 0.5);
        assert_eq!(report.rate_limited.observed, 30.0 / 102.0);
    }

    #[test]
    fn zero_observed_failures_still_need_enough_independent_cases() {
        let mut measurements = passing_measurements();
        measurements.expected_cases = 20;
        measurements.attempts = 20;
        measurements.attempt_log_start_records = 20;
        measurements.attempt_log_terminal_records = 20;
        measurements.completed = 20;
        measurements.nonempty = 20;
        measurements.structurally_sufficient = 20;
        let report = policy().evaluate(&measurements);
        assert!(!report.passed);
        assert!(report
            .violations
            .iter()
            .any(|violation| violation.contains("terminal failure rate")));
    }

    #[test]
    fn every_canary_dimension_fails_closed() {
        let mut measurements = passing_measurements();
        measurements.expected_cases = 41;
        measurements.attempt_log_start_records = 39;
        measurements.attempt_log_terminal_records = 38;
        measurements.completed = 35;
        measurements.terminal_failures = 5;
        measurements.pre_execution_failures = 1;
        measurements.nonempty = 35;
        measurements.structurally_sufficient = 20;
        measurements.p95_latency_ms = 10_001;
        measurements.p99_latency_ms = 30_001;
        measurements.retry_attempts = 50;
        measurements.circuit_open = 50;
        measurements.rate_limited_outcomes = 30;
        measurements.second_tier_escalations = 30;
        measurements.final_tier_escalations = 20;
        measurements.rate_limit_compliance_violations = 1;
        measurements.receipt_integrity_violations = 1;
        measurements.resource_samples = 1;
        measurements.resource_coverage_ratio = 0.5;
        measurements.maximum_resource_sample_gap_ms = 180_001;
        measurements.rss_growth_kib = 131_073;
        measurements.tail_rss_slope_kib_per_minute = 1_025.0;
        measurements.fd_growth = 17;

        let report = policy().evaluate(&measurements);
        assert!(!report.passed);
        for dimension in [
            "case execution",
            "attempt-start log records",
            "attempt-terminal log records",
            "pre-execution terminal",
            "structural sufficiency rate",
            "terminal failure rate",
            "p95 latency",
            "p99 latency",
            "retry amplification",
            "second-tier escalation rate",
            "final-tier escalation rate",
            "provider rate-limit compliance",
            "receipt-integrity",
            "resource timeline",
            "resource coverage ratio",
            "resource sample gap",
            "RSS growth",
            "tail RSS slope",
            "file-descriptor growth",
        ] {
            assert!(
                report
                    .violations
                    .iter()
                    .any(|violation| violation.contains(dimension)),
                "missing violation for {dimension}: {:?}",
                report.violations
            );
        }
    }
}
