use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::gate::LiveCanaryMeasurements;
use super::corpus::LoadedCampaign;
use super::driver::{verify_file_identity, DriverError};

pub(super) fn required_absolute_file(name: &str) -> PathBuf {
    let path = PathBuf::from(super::required_env(name));
    assert!(path.is_absolute(), "{name} must be an absolute path");
    let metadata =
        std::fs::symlink_metadata(&path).unwrap_or_else(|error| panic!("inspect {name}: {error}"));
    assert!(
        metadata.file_type().is_file(),
        "{name} must identify a regular non-symlink file"
    );
    path
}

pub(super) fn required_sha256_identity(name: &str) -> String {
    let value = super::required_env(name);
    let digest = value.strip_prefix("sha256:").unwrap_or(&value);
    assert!(
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{name} must contain one SHA-256 digest"
    );
    format!("sha256:{}", digest.to_ascii_lowercase())
}

pub(super) fn verify_live_artifacts(
    campaign: &LoadedCampaign,
    driver_path: &Path,
    driver_identity: &str,
    candidate_path: &Path,
    candidate_identity: &str,
) -> Result<(), DriverError> {
    verify_file_identity(driver_path, driver_identity)?;
    verify_file_identity(candidate_path, candidate_identity)?;
    campaign
        .verify_artifact_identities()
        .map_err(DriverError::SealedArtifact)
}

pub(super) fn record_artifact_violation(
    error: DriverError,
    measurements: &mut LiveCanaryMeasurements,
    failure_kinds: &mut BTreeMap<String, u64>,
    fatal_driver_error: &mut Option<String>,
) {
    measurements.receipt_integrity_violations =
        measurements.receipt_integrity_violations.saturating_add(1);
    *failure_kinds.entry(error.kind().to_string()).or_default() += 1;
    fatal_driver_error.get_or_insert_with(|| error.to_string());
}
