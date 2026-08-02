#[path = "live/artifact.rs"]
mod artifact;
#[path = "live/corpus.rs"]
mod corpus;
#[path = "live/driver.rs"]
mod driver;
#[path = "live/log.rs"]
mod log;
#[path = "live/observation.rs"]
mod observation;
#[path = "live/rate.rs"]
mod rate;
#[path = "live/resource.rs"]
mod resource;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;

use self::artifact::{
    record_artifact_violation, required_absolute_file, required_sha256_identity,
    verify_live_artifacts,
};
use self::corpus::load_campaign;
use self::driver::{DriverClient, DriverError, FailureStage};
use self::log::AttemptLog;
use self::observation::evaluate_attempt;
use self::rate::ProviderRateTracker;
use self::resource::DriverResourceTracker;
use super::gate::LiveCanaryMeasurements;
use super::policy::LiveCanaryPolicy;
use super::resources::{resource_snapshot, sample_resources, summarize_resources};

const EVALUATED_COMMIT_ENV: &str = "A3S_SEARCH_EVALUATED_COMMIT";
const DRIVER_ENV: &str = "A3S_SEARCH_LIVE_CANARY_DRIVER";
const DRIVER_SHA256_ENV: &str = "A3S_SEARCH_LIVE_CANARY_DRIVER_SHA256";
const CANDIDATE_BIN_ENV: &str = "A3S_SEARCH_LIVE_CANARY_CANDIDATE_BIN";
const CANDIDATE_SHA256_ENV: &str = "A3S_SEARCH_LIVE_CANARY_CANDIDATE_SHA256";
const FROZEN_CRATE_ENV: &str = "A3S_SEARCH_LIVE_CANARY_FROZEN_CRATE";
const FROZEN_CRATE_SHA256_ENV: &str = "A3S_SEARCH_LIVE_CANARY_FROZEN_CRATE_SHA256";
const RECEIPT_LOG_ENV: &str = "A3S_SEARCH_LIVE_CANARY_RECEIPT_LOG";
const REQUEST_TIMEOUT_CEILING: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Serialize)]
struct LiveCanaryConfig {
    interval_seconds: u64,
    request_timeout_seconds: u64,
    minimum_provider_interval_seconds: u64,
}

impl LiveCanaryConfig {
    fn from_env(minimum_provider_interval: Duration) -> Self {
        let config = Self {
            interval_seconds: env_u64(
                "A3S_SEARCH_LIVE_CANARY_INTERVAL_SECONDS",
                minimum_provider_interval.as_secs(),
            ),
            request_timeout_seconds: env_u64(
                "A3S_SEARCH_LIVE_CANARY_REQUEST_TIMEOUT_SECONDS",
                REQUEST_TIMEOUT_CEILING.as_secs(),
            ),
            minimum_provider_interval_seconds: minimum_provider_interval.as_secs(),
        };
        config.assert_valid();
        config
    }

    fn assert_valid(&self) {
        assert!(
            self.interval() >= self.minimum_provider_interval(),
            "request cadence cannot be faster than the sealed provider policy"
        );
        assert!(
            !self.request_timeout().is_zero() && self.request_timeout() <= REQUEST_TIMEOUT_CEILING,
            "request timeout must be positive and no greater than 60 seconds"
        );
    }

    fn interval(self) -> Duration {
        Duration::from_secs(self.interval_seconds)
    }

    fn request_timeout(self) -> Duration {
        Duration::from_secs(self.request_timeout_seconds)
    }

    fn minimum_provider_interval(self) -> Duration {
        Duration::from_secs(self.minimum_provider_interval_seconds)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "sealed one-pass live canary; requires the independent deployment driver"]
async fn sealed_live_tiered_canary_meets_release_floor() {
    require_headless_feature();
    let campaign = load_campaign();
    let config = LiveCanaryConfig::from_env(campaign.minimum_request_interval());
    let policy = LiveCanaryPolicy::from_env();
    assert!(
        campaign.queries.len() as u64 >= policy.minimum_cases,
        "sealed corpus is smaller than the configured release floor"
    );
    let evaluated_commit = evaluated_commit();
    let candidate = required_absolute_file(CANDIDATE_BIN_ENV);
    let candidate_identity = required_sha256_identity(CANDIDATE_SHA256_ENV);
    let frozen_crate = required_absolute_file(FROZEN_CRATE_ENV);
    let frozen_crate_identity = required_sha256_identity(FROZEN_CRATE_SHA256_ENV);
    let driver_path = required_absolute_file(DRIVER_ENV);
    let driver_identity = required_sha256_identity(DRIVER_SHA256_ENV);
    let receipt_log_path = std::path::PathBuf::from(required_env(RECEIPT_LOG_ENV));
    let mut attempt_log = AttemptLog::create(&receipt_log_path)
        .expect("create a new append-only live-canary receipt log");
    let campaign_started = Instant::now();
    let mut driver = DriverClient::start(
        &driver_path,
        &driver_identity,
        &candidate,
        &candidate_identity,
        &campaign.manifest_path,
        &evaluated_commit,
        &campaign.manifest_identity,
        &campaign.capabilities,
        &campaign.profiles,
    )
    .await
    .expect("start independent live-canary driver");
    let mut rate_tracker = ProviderRateTracker::new(&campaign.provider_policies);
    let mut driver_resources = DriverResourceTracker::default();
    let prewarm_resources = resource_snapshot();

    let resource_running = Arc::new(AtomicBool::new(true));
    let resource_samples = Arc::new(Mutex::new(Vec::new()));
    let sampler = tokio::spawn(sample_resources(
        Arc::clone(&resource_running),
        Arc::clone(&resource_samples),
        Duration::ZERO,
    ));
    let mut measurements = LiveCanaryMeasurements {
        expected_cases: campaign.queries.len() as u64,
        ..LiveCanaryMeasurements::default()
    };
    let mut latency_ms = Vec::with_capacity(campaign.queries.len());
    let mut failure_kinds = BTreeMap::<String, u64>::new();
    let mut fatal_driver_error = None;
    let mut next_attempt_at = Instant::now();

    for (index, query) in campaign.queries.iter().enumerate() {
        tokio::time::sleep(next_attempt_at.saturating_duration_since(Instant::now())).await;
        if let Err(error) = verify_live_artifacts(
            &campaign,
            &driver_path,
            &driver_identity,
            &candidate,
            &candidate_identity,
            &frozen_crate,
            &frozen_crate_identity,
        ) {
            record_artifact_violation(
                error,
                &mut measurements,
                &mut failure_kinds,
                &mut fatal_driver_error,
            );
            break;
        }
        let attempt_id = index as u64 + 1;
        if let Err(error) = attempt_log.append_start(attempt_id, &query.id) {
            measurements.receipt_integrity_violations =
                measurements.receipt_integrity_violations.saturating_add(1);
            fatal_driver_error = Some(format!("persist attempt start: {error}"));
            break;
        }
        measurements.attempts = measurements.attempts.saturating_add(1);
        let started = Instant::now();
        next_attempt_at = started + config.interval();
        let response =
            tokio::time::timeout(config.request_timeout(), driver.search(attempt_id, query)).await;
        let finished = Instant::now();

        let observation = match response {
            Ok(Ok(received)) => {
                if let Err(error) =
                    attempt_log.append_driver_terminal(attempt_id, &query.id, &received.raw_json)
                {
                    Err(DriverError::InvalidReceipt(format!(
                        "persist attempt receipt: {error}"
                    )))
                } else {
                    match serde_json::from_str(&received.raw_json) {
                        Ok(receipt) => evaluate_attempt(
                            attempt_id,
                            duration_millis(finished.saturating_duration_since(started)),
                            query,
                            &campaign.capabilities,
                            &campaign.profiles,
                            &campaign.provider_policies,
                            &evaluated_commit,
                            &driver.candidate_identity,
                            receipt,
                        )
                        .map_err(DriverError::InvalidReceipt),
                        Err(_) => Err(DriverError::InvalidJson),
                    }
                }
            }
            Ok(Err(error)) => {
                persist_harness_terminal(&mut attempt_log, attempt_id, &query.id, error)
            }
            Err(_) => persist_harness_terminal(
                &mut attempt_log,
                attempt_id,
                &query.id,
                DriverError::Timeout,
            ),
        };
        match observation {
            Ok(observation) => {
                measurements.record_observation(&observation);
                rate_tracker.record(started, &observation.calls);
                driver_resources.record(&observation.resource_samples);
                if let Some(kind) = observation.terminal_error_kind {
                    measurements.terminal_failures =
                        measurements.terminal_failures.saturating_add(1);
                    *failure_kinds.entry(kind.clone()).or_default() += 1;
                    if observation.terminal_failure_stage == Some(FailureStage::PreExecution) {
                        measurements.pre_execution_failures =
                            measurements.pre_execution_failures.saturating_add(1);
                        fatal_driver_error = Some(format!(
                            "pre-execution terminal failure for {}: {kind}",
                            query.id
                        ));
                    }
                } else {
                    measurements.completed = measurements.completed.saturating_add(1);
                    measurements.nonempty = measurements
                        .nonempty
                        .saturating_add(u64::from(observation.nonempty));
                    measurements.structurally_sufficient = measurements
                        .structurally_sufficient
                        .saturating_add(u64::from(observation.structurally_sufficient));
                }
            }
            Err(error) => {
                measurements.terminal_failures = measurements.terminal_failures.saturating_add(1);
                measurements.receipt_integrity_violations =
                    measurements.receipt_integrity_violations.saturating_add(1);
                *failure_kinds.entry(error.kind().to_string()).or_default() += 1;
                fatal_driver_error = Some(error.to_string());
            }
        }
        if let Err(error) = verify_live_artifacts(
            &campaign,
            &driver_path,
            &driver_identity,
            &candidate,
            &candidate_identity,
            &frozen_crate,
            &frozen_crate_identity,
        ) {
            record_artifact_violation(
                error,
                &mut measurements,
                &mut failure_kinds,
                &mut fatal_driver_error,
            );
        }
        latency_ms.push(duration_millis(finished.saturating_duration_since(started)));
        if fatal_driver_error.is_some() {
            break;
        }
    }

    let campaign_elapsed_ms = duration_millis(campaign_started.elapsed());
    resource_running.store(false, Ordering::Release);
    sampler.await.expect("resource sampler panicked");
    let harness_resources = {
        let harness_samples = resource_samples.lock().unwrap();
        (harness_samples.len() >= 2).then(|| summarize_resources(&harness_samples))
    };
    if let Err(error) = driver.shutdown().await {
        measurements.receipt_integrity_violations =
            measurements.receipt_integrity_violations.saturating_add(1);
        *failure_kinds.entry(error.kind().to_string()).or_default() += 1;
        fatal_driver_error.get_or_insert_with(|| error.to_string());
    }
    if let Err(error) = verify_live_artifacts(
        &campaign,
        &driver_path,
        &driver_identity,
        &candidate,
        &candidate_identity,
        &frozen_crate,
        &frozen_crate_identity,
    ) {
        record_artifact_violation(
            error,
            &mut measurements,
            &mut failure_kinds,
            &mut fatal_driver_error,
        );
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let released_resources = resource_snapshot();

    latency_ms.sort_unstable();
    measurements.p95_latency_ms = percentile(&latency_ms, 95);
    measurements.p99_latency_ms = percentile(&latency_ms, 99);
    measurements.attempt_log_start_records = attempt_log.start_records() as u64;
    measurements.attempt_log_terminal_records = attempt_log.terminal_records() as u64;
    measurements.rate_limit_compliance_violations = measurements
        .rate_limit_compliance_violations
        .saturating_add(rate_tracker.compliance_violations);
    let candidate_resources = driver_resources.summarize(campaign_elapsed_ms);
    measurements.receipt_integrity_violations = measurements
        .receipt_integrity_violations
        .saturating_add(driver_resources.integrity_violations);
    if let Some(resources) = candidate_resources {
        measurements.resource_samples = resources.samples;
        measurements.resource_coverage_ratio = resources.coverage_ratio;
        measurements.maximum_resource_sample_gap_ms = resources.maximum_gap_ms;
        measurements.rss_growth_kib = resources.rss_growth_kib;
        measurements.tail_rss_slope_kib_per_minute = resources.tail_rss_slope_kib_per_minute;
        measurements.fd_growth = resources.fd_growth;
    }
    let receipt_log_identity = attempt_log.identity().ok();
    if receipt_log_identity.is_none() {
        measurements.receipt_integrity_violations =
            measurements.receipt_integrity_violations.saturating_add(1);
    }
    let gate = policy.evaluate(&measurements);

    println!(
        "LIVE_CANARY_REPORT={}",
        json!({
            "schema_version": 7,
            "package_version": env!("CARGO_PKG_VERSION"),
            "evaluated_commit": evaluated_commit,
            "frozen_crate_sha256": frozen_crate_identity,
            "driver_sha256": driver.driver_identity,
            "candidate_sha256": driver.candidate_identity,
            "query_corpus": campaign.query_identity,
            "tier_manifest": campaign.manifest_identity,
            "artifact_identity_rechecks": "before_and_after_each_attempt_and_after_shutdown",
            "deployment_profiles": campaign.profiles,
            "receipt_log": receipt_log_identity,
            "campaign_id": campaign.campaign_id,
            "query_count": campaign.queries.len(),
            "capabilities": campaign.capabilities,
            "configuration": config,
            "measurements": measurements,
            "policy": policy,
            "gate": gate,
            "failure_kinds": failure_kinds,
            "fatal_driver_error": fatal_driver_error,
            "candidate_process_tree_resources": candidate_resources,
            "harness_resources": harness_resources,
            "harness_prewarm_resources": resource_json(prewarm_resources),
            "harness_released_resources": resource_json(released_resources),
        })
    );

    if let (Some((_, before)), Some((_, released))) = (prewarm_resources, released_resources) {
        assert!(
            released <= before.saturating_add(policy.maximum_fd_growth as usize),
            "live canary retained file descriptors after shutdown: {before} -> {released}"
        );
    }
    assert!(fatal_driver_error.is_none(), "independent driver failed");
    gate.assert_passed();
}

fn persist_harness_terminal(
    log: &mut AttemptLog,
    attempt_id: u64,
    query_id: &str,
    error: DriverError,
) -> Result<observation::AttemptObservation, DriverError> {
    match log.append_harness_terminal(attempt_id, query_id, error.kind()) {
        Ok(()) => Err(error),
        Err(log_error) => Err(DriverError::InvalidReceipt(format!(
            "persist harness terminal after {error}: {log_error}"
        ))),
    }
}

#[cfg(feature = "headless")]
fn require_headless_feature() {}

#[cfg(not(feature = "headless"))]
fn require_headless_feature() {
    panic!("live canary must validate the all-features candidate");
}

impl LiveCanaryMeasurements {
    fn record_observation(&mut self, observation: &observation::AttemptObservation) {
        self.engine_slots = self.engine_slots.saturating_add(observation.engine_slots);
        self.upstream_calls = self
            .upstream_calls
            .saturating_add(observation.upstream_calls);
        self.retry_attempts = self
            .retry_attempts
            .saturating_add(observation.retry_attempts);
        self.rate_limited_outcomes = self
            .rate_limited_outcomes
            .saturating_add(observation.rate_limited_outcomes);
        self.circuit_open = self.circuit_open.saturating_add(observation.circuit_open);
        self.second_tier_escalations = self
            .second_tier_escalations
            .saturating_add(u64::from(observation.second_tier_escalated));
        self.final_tier_escalations = self
            .final_tier_escalations
            .saturating_add(u64::from(observation.final_tier_escalated));
    }
}

fn evaluated_commit() -> String {
    let commit = required_env(EVALUATED_COMMIT_ENV);
    assert!(
        commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "evaluated commit must be a full 40-character Git object ID"
    );
    commit.to_ascii_lowercase()
}

pub(super) fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} for the sealed live canary"))
}

fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|error| panic!("{name} is invalid: {error}")),
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => panic!("cannot read {name}: {error}"),
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = sorted.len().saturating_mul(percentile).saturating_sub(1) / 100;
    sorted[index.min(sorted.len() - 1)]
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn resource_json(snapshot: Option<(u64, usize)>) -> serde_json::Value {
    snapshot.map_or(serde_json::Value::Null, |(rss_kib, file_descriptors)| {
        json!({
            "rss_kib": rss_kib,
            "file_descriptors": file_descriptors,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canary_cadence_follows_the_sealed_provider_policy() {
        let baseline = LiveCanaryConfig {
            interval_seconds: 60,
            request_timeout_seconds: 60,
            minimum_provider_interval_seconds: 60,
        };
        baseline.assert_valid();
        let faster = LiveCanaryConfig {
            interval_seconds: 59,
            ..baseline
        };
        assert!(std::panic::catch_unwind(|| faster.assert_valid()).is_err());
        let excessive_timeout = LiveCanaryConfig {
            request_timeout_seconds: 61,
            ..baseline
        };
        assert!(std::panic::catch_unwind(|| excessive_timeout.assert_valid()).is_err());
    }

    #[test]
    fn percentile_uses_nearest_rank_without_truncating_the_tail() {
        let values = (1..=100).collect::<Vec<_>>();
        assert_eq!(percentile(&values, 95), 95);
        assert_eq!(percentile(&values, 99), 99);
    }
}
