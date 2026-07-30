use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::required_env;

const QUERY_CORPUS_ENV: &str = "A3S_SEARCH_LIVE_CANARY_QUERY_CORPUS";
const QUERY_CORPUS_SHA256_ENV: &str = "A3S_SEARCH_LIVE_CANARY_QUERY_CORPUS_SHA256";
const TIER_MANIFEST_ENV: &str = "A3S_SEARCH_LIVE_CANARY_TIER_MANIFEST";
const TIER_MANIFEST_SHA256_ENV: &str = "A3S_SEARCH_LIVE_CANARY_TIER_MANIFEST_SHA256";
const MAX_CAMPAIGN_FILE_BYTES: usize = 1024 * 1024;
const MINIMUM_CANARY_QUERY_COUNT: usize = 40;
const MAXIMUM_PROVIDER_INTERVAL_SECONDS: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TierCapability {
    Api,
    HttpRss,
    Headless,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LiveCanaryQuery {
    pub id: String,
    pub query: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderPolicy {
    pub scope: String,
    pub minimum_interval_seconds: u64,
}

impl ProviderPolicy {
    pub(super) fn minimum_interval(&self) -> Duration {
        Duration::from_secs(self.minimum_interval_seconds)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveCanaryQueryCorpus {
    version: u32,
    campaign_id: String,
    queries: Vec<LiveCanaryQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TierManifest {
    version: u32,
    driver_protocol: u32,
    campaign_id: String,
    tiers: Vec<TierProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TierProfile {
    capability: TierCapability,
    profile: String,
    provider_policies: Vec<ProviderPolicy>,
}

pub(super) struct LoadedCampaign {
    pub campaign_id: String,
    pub query_identity: String,
    pub query_path: PathBuf,
    pub manifest_identity: String,
    pub manifest_path: PathBuf,
    pub capabilities: Vec<TierCapability>,
    pub profiles: Vec<String>,
    pub provider_policies: Vec<Vec<ProviderPolicy>>,
    pub queries: Vec<LiveCanaryQuery>,
}

impl LoadedCampaign {
    pub(super) fn minimum_request_interval(&self) -> Duration {
        self.provider_policies
            .iter()
            .flatten()
            .map(ProviderPolicy::minimum_interval)
            .max()
            .unwrap_or(Duration::from_secs(1))
    }

    pub(super) fn verify_artifact_identities(&self) -> Result<(), String> {
        verify_precommitted_path(&self.query_path, &self.query_identity, "query corpus")?;
        verify_precommitted_path(
            &self.manifest_path,
            &self.manifest_identity,
            "tier manifest",
        )
    }
}

pub(super) fn load_campaign() -> LoadedCampaign {
    let (query_path, query_bytes, query_identity) =
        load_precommitted(QUERY_CORPUS_ENV, QUERY_CORPUS_SHA256_ENV, "query corpus");
    let (manifest_path, manifest_bytes, manifest_identity) =
        load_precommitted(TIER_MANIFEST_ENV, TIER_MANIFEST_SHA256_ENV, "tier manifest");
    let query_corpus = serde_json::from_slice::<LiveCanaryQueryCorpus>(&query_bytes)
        .expect("live-canary query corpus must be valid JSON");
    let manifest = serde_json::from_slice::<TierManifest>(&manifest_bytes)
        .expect("live-canary tier manifest must be valid JSON");
    validate_query_corpus(&query_corpus);
    validate_manifest(&manifest);
    assert_eq!(
        query_corpus.campaign_id, manifest.campaign_id,
        "query corpus and tier manifest must bind the same campaign ID"
    );
    LoadedCampaign {
        campaign_id: query_corpus.campaign_id,
        query_identity,
        query_path,
        manifest_identity,
        manifest_path,
        capabilities: manifest.tiers.iter().map(|tier| tier.capability).collect(),
        profiles: manifest
            .tiers
            .iter()
            .map(|tier| tier.profile.clone())
            .collect(),
        provider_policies: manifest
            .tiers
            .iter()
            .map(|tier| tier.provider_policies.clone())
            .collect(),
        queries: query_corpus.queries,
    }
}

fn load_precommitted(
    path_env: &str,
    sha256_env: &str,
    description: &str,
) -> (PathBuf, Vec<u8>, String) {
    let path = PathBuf::from(required_env(path_env));
    assert!(path.is_absolute(), "{path_env} must be an absolute path");
    let expected = required_sha256(sha256_env);
    let expected_identity = format!("sha256:{expected}");
    let bytes =
        read_bounded_regular_file(&path, description).unwrap_or_else(|error| panic!("{error}"));
    let actual_identity = identity(&bytes);
    assert_eq!(
        actual_identity, expected_identity,
        "{description} does not match its precommitted SHA-256"
    );
    (path, bytes, actual_identity)
}

fn verify_precommitted_path(
    path: &std::path::Path,
    expected_identity: &str,
    description: &str,
) -> Result<(), String> {
    let bytes = read_bounded_regular_file(path, description)?;
    if identity(&bytes) != expected_identity {
        return Err(format!(
            "{description} changed after its precommitted identity was verified"
        ));
    }
    Ok(())
}

fn read_bounded_regular_file(path: &std::path::Path, description: &str) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("inspect {description}: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{description} must be a regular non-symlink file"));
    }
    if metadata.len() > MAX_CAMPAIGN_FILE_BYTES as u64 {
        return Err(format!("{description} exceeds the one-MiB limit"));
    }
    std::fs::read(path).map_err(|error| format!("read {description}: {error}"))
}

fn identity(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn required_sha256(name: &str) -> String {
    let value = required_env(name);
    let value = value.strip_prefix("sha256:").unwrap_or(&value);
    assert!(
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{name} must contain one SHA-256 digest"
    );
    value.to_ascii_lowercase()
}

fn validate_query_corpus(corpus: &LiveCanaryQueryCorpus) {
    assert_eq!(corpus.version, 1, "unsupported query-corpus version");
    assert!(
        corpus.queries.len() >= MINIMUM_CANARY_QUERY_COUNT,
        "sealed canary corpus must contain at least {MINIMUM_CANARY_QUERY_COUNT} queries"
    );
    validate_campaign_id(&corpus.campaign_id);
    let mut ids = HashSet::new();
    let mut queries = HashSet::new();
    for query in &corpus.queries {
        assert!(
            !query.id.trim().is_empty() && query.id.len() <= 128,
            "query ID must be bounded and non-empty"
        );
        assert!(
            ids.insert(query.id.trim()),
            "duplicate live-canary query ID"
        );
        let normalized = query.query.trim().to_lowercase();
        assert!(!normalized.is_empty(), "live-canary query cannot be blank");
        assert!(
            normalized.chars().count() <= 500,
            "live-canary query exceeds 500 characters"
        );
        assert!(queries.insert(normalized), "duplicate live-canary query");
        if let Some(language) = query.language.as_deref() {
            assert!(
                !language.trim().is_empty() && language.len() <= 35,
                "optional language tag must be bounded and non-empty"
            );
        }
    }
}

fn validate_manifest(manifest: &TierManifest) {
    assert_eq!(manifest.version, 1, "unsupported tier-manifest version");
    assert_eq!(manifest.driver_protocol, 3, "unsupported driver protocol");
    validate_campaign_id(&manifest.campaign_id);
    assert_eq!(
        manifest.tiers.len(),
        3,
        "tier manifest must declare exactly three capabilities"
    );
    let capabilities = manifest
        .tiers
        .iter()
        .map(|tier| tier.capability)
        .collect::<HashSet<_>>();
    assert_eq!(
        capabilities,
        HashSet::from([
            TierCapability::Api,
            TierCapability::HttpRss,
            TierCapability::Headless,
        ]),
        "tier manifest must declare API, HTTP/RSS, and headless exactly once"
    );
    let mut profiles = HashSet::new();
    let mut global_policies = HashMap::<&str, u64>::new();
    for tier in &manifest.tiers {
        validate_sha256_identity(&tier.profile, "deployment profile");
        assert!(
            profiles.insert(tier.profile.as_str()),
            "duplicate tier profile"
        );
        assert!(
            !tier.provider_policies.is_empty(),
            "each tier must precommit at least one provider policy"
        );
        let mut scopes = HashSet::new();
        for policy in &tier.provider_policies {
            validate_provider_policy(policy);
            assert!(
                scopes.insert(policy.scope.as_str()),
                "duplicate provider scope within a tier"
            );
            if let Some(previous) =
                global_policies.insert(policy.scope.as_str(), policy.minimum_interval_seconds)
            {
                assert_eq!(
                    previous, policy.minimum_interval_seconds,
                    "one provider scope cannot declare conflicting cadence policies"
                );
            }
        }
    }
}

fn validate_provider_policy(policy: &ProviderPolicy) {
    validate_sha256_identity(&policy.scope, "provider scope");
    assert!(
        (1..=MAXIMUM_PROVIDER_INTERVAL_SECONDS).contains(&policy.minimum_interval_seconds),
        "provider cadence must be positive and bounded"
    );
}

fn validate_sha256_identity(identity: &str, description: &str) {
    let digest = identity.strip_prefix("sha256:").unwrap_or_default();
    assert!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{description} must be an opaque lowercase SHA-256 identity"
    );
}

fn validate_campaign_id(campaign_id: &str) {
    assert!(
        !campaign_id.trim().is_empty() && campaign_id.len() <= 128,
        "campaign ID must be bounded and non-empty"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_corpus() -> LiveCanaryQueryCorpus {
        LiveCanaryQueryCorpus {
            version: 1,
            campaign_id: "sealed-campaign".to_string(),
            queries: (0..MINIMUM_CANARY_QUERY_COUNT)
                .map(|index| LiveCanaryQuery {
                    id: format!("case-{index}"),
                    query: format!("independent query {index}"),
                    language: None,
                })
                .collect(),
        }
    }

    fn policy(digest: char, seconds: u64) -> ProviderPolicy {
        ProviderPolicy {
            scope: format!("sha256:{}", digest.to_string().repeat(64)),
            minimum_interval_seconds: seconds,
        }
    }

    fn manifest() -> TierManifest {
        TierManifest {
            version: 1,
            driver_protocol: 3,
            campaign_id: "sealed-campaign".to_string(),
            tiers: [
                (TierCapability::Headless, 'c'),
                (TierCapability::HttpRss, 'b'),
                (TierCapability::Api, 'a'),
            ]
            .into_iter()
            .map(|(capability, digest)| TierProfile {
                capability,
                profile: format!("sha256:{}", digest.to_string().repeat(64)),
                provider_policies: vec![policy(digest, 60)],
            })
            .collect(),
        }
    }

    #[test]
    fn corpus_contract_is_domain_and_language_neutral() {
        validate_query_corpus(&query_corpus());
    }

    #[test]
    #[should_panic(expected = "duplicate live-canary query")]
    fn corpus_contract_rejects_duplicate_queries() {
        let mut corpus = query_corpus();
        corpus.queries[1].query = corpus.queries[0].query.clone();
        validate_query_corpus(&corpus);
    }

    #[test]
    fn manifest_uses_capabilities_opaque_profiles_and_predeclared_cadence() {
        validate_manifest(&manifest());
    }

    #[test]
    fn manifest_order_is_sealed_but_not_transport_hardcoded() {
        let mut manifest = manifest();
        manifest.tiers.rotate_left(1);
        validate_manifest(&manifest);
    }

    #[test]
    fn manifest_rejects_duplicate_or_missing_capabilities() {
        let mut manifest = manifest();
        manifest.tiers[2].capability = TierCapability::HttpRss;
        assert!(std::panic::catch_unwind(|| validate_manifest(&manifest)).is_err());
    }

    #[test]
    fn conflicting_policy_for_one_opaque_scope_is_rejected() {
        let mut manifest = manifest();
        manifest.tiers[1].provider_policies[0] = manifest.tiers[0].provider_policies[0].clone();
        manifest.tiers[1].provider_policies[0].minimum_interval_seconds = 30;
        assert!(std::panic::catch_unwind(|| validate_manifest(&manifest)).is_err());
    }

    #[test]
    fn campaign_artifact_recheck_rejects_changed_corpus_or_manifest() {
        let directory = tempfile::tempdir().unwrap();
        let query_path = directory.path().join("queries.json");
        let manifest_path = directory.path().join("manifest.json");
        std::fs::write(&query_path, b"sealed queries").unwrap();
        std::fs::write(&manifest_path, b"sealed manifest").unwrap();
        let campaign = LoadedCampaign {
            campaign_id: "sealed-campaign".to_string(),
            query_identity: identity(b"sealed queries"),
            query_path: query_path.clone(),
            manifest_identity: identity(b"sealed manifest"),
            manifest_path: manifest_path.clone(),
            capabilities: Vec::new(),
            profiles: Vec::new(),
            provider_policies: Vec::new(),
            queries: Vec::new(),
        };

        campaign.verify_artifact_identities().unwrap();
        std::fs::write(&query_path, b"changed queries").unwrap();
        assert!(campaign.verify_artifact_identities().is_err());

        std::fs::write(&query_path, b"sealed queries").unwrap();
        std::fs::write(&manifest_path, b"changed manifest").unwrap();
        assert!(campaign.verify_artifact_identities().is_err());
    }
}
