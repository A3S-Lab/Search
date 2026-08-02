use std::fs;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn direct_chrome_scope() -> String {
    let mut digest = Sha256::new();
    digest.update(b"a3s/search-cli-transport-scope/v1\0");
    digest.update(b"chrome");
    digest.update([0]);
    digest.update(b"direct");
    format!("{:x}", digest.finalize())
}

#[test]
fn a_later_cli_process_skips_an_open_source_without_network_access() {
    let state_directory = tempdir().unwrap();
    let scope = direct_chrome_scope();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;
    let state = serde_json::json!({
        "schema": "a3s/search-cli-circuit-state/v1",
        "scope_sha256": format!("sha256:{scope}"),
        "entries": {
            "wiki": {
                "open_until_unix_ms": now + 60_000,
                "updated_at_unix_ms": now,
                "ejection_count": 1,
                "failure_kind": "challenge"
            }
        }
    });
    fs::write(
        state_directory
            .path()
            .join(format!("circuit-state-v1-{scope}.json")),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_a3s-search"))
        .arg("cross process circuit probe")
        .args(["--engines", "wiki", "--format", "json", "--timeout", "2"])
        .env("A3S_SEARCH_STATE_DIR", state_directory.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["outcomes"][0]["shortcut"], "wiki");
    assert_eq!(payload["outcomes"][0]["kind"], "circuit_open");
    assert!(payload["outcomes"][0]["failure"]["retry_after_seconds"]
        .as_u64()
        .is_some_and(|seconds| seconds > 0));
}
