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
    frozen_crate_path: &Path,
    frozen_crate_identity: &str,
) -> Result<(), DriverError> {
    verify_file_identity(driver_path, driver_identity)?;
    verify_file_identity(candidate_path, candidate_identity)?;
    verify_file_identity(frozen_crate_path, frozen_crate_identity)?;
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

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::live::corpus::LoadedCampaign;

    fn identity(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn live_artifact_rechecks_include_the_exact_frozen_crate() {
        let directory = tempfile::tempdir().unwrap();
        let driver = directory.path().join("driver");
        let candidate = directory.path().join("candidate");
        let frozen_crate = directory.path().join("candidate.crate");
        let query_path = directory.path().join("queries.json");
        let manifest_path = directory.path().join("tiers.json");
        std::fs::write(&driver, b"driver").unwrap();
        std::fs::write(&candidate, b"candidate").unwrap();
        std::fs::write(&frozen_crate, b"frozen crate").unwrap();
        std::fs::write(&query_path, b"queries").unwrap();
        std::fs::write(&manifest_path, b"tiers").unwrap();
        let campaign = LoadedCampaign {
            campaign_id: "campaign".to_string(),
            query_identity: identity(b"queries"),
            query_path,
            manifest_identity: identity(b"tiers"),
            manifest_path,
            capabilities: Vec::new(),
            profiles: Vec::new(),
            provider_policies: Vec::new(),
            queries: Vec::new(),
        };

        verify_live_artifacts(
            &campaign,
            &driver,
            &identity(b"driver"),
            &candidate,
            &identity(b"candidate"),
            &frozen_crate,
            &identity(b"frozen crate"),
        )
        .unwrap();

        std::fs::write(&frozen_crate, b"substituted crate").unwrap();
        assert!(matches!(
            verify_live_artifacts(
                &campaign,
                &driver,
                &identity(b"driver"),
                &candidate,
                &identity(b"candidate"),
                &frozen_crate,
                &identity(b"frozen crate"),
            ),
            Err(DriverError::ArtifactIdentityMismatch)
        ));
    }
}
