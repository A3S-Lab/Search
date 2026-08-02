use super::*;
use a3s_search::EngineFailure;
use tempfile::tempdir;

const TEST_STATE_FILE: &str = "circuit-state-v1.json";

fn scope() -> String {
    transport_scope(None, "chrome")
}

fn outcome(kind: &str, failure_kind: Option<&str>) -> SearchResults {
    serde_json::from_value(serde_json::json!({
        "results": [],
        "suggestions": [],
        "answers": [],
        "images": [],
        "errors": [],
        "failures": [],
        "reports": [],
        "outcomes": [{
            "engine": "Browser source",
            "shortcut": "browser_source",
            "kind": kind,
            "result_count": 0,
            "duration_ms": 1,
            "failure": failure_kind.map(|failure_kind| serde_json::json!({
                "engine": "Browser source",
                "kind": failure_kind,
                "message": "typed failure",
                "transient": true
            }))
        }],
        "count": 0,
        "duration_ms": 1
    }))
    .unwrap()
}

#[tokio::test]
async fn open_state_survives_a_new_cli_instance_without_query_content() {
    let directory = tempdir().unwrap();
    let path = directory.path().join(TEST_STATE_FILE);
    let shortcuts = vec!["browser_source".to_string()];
    let state = PersistentCircuitState::load(path.clone(), &shortcuts, scope())
        .await
        .unwrap();
    state
        .breaker()
        .acquire("browser_source")
        .unwrap()
        .record_failure(
            &EngineFailure::new("Browser source", "challenge", "verification").with_transient(true),
        );
    state
        .persist(&outcome("failure", Some("challenge")))
        .await
        .unwrap();

    let persisted = fs::read_to_string(&path).unwrap();
    assert!(persisted.contains(STATE_SCHEMA));
    assert!(persisted.contains("challenge"));
    assert!(!persisted.contains("verification"));

    let restored = PersistentCircuitState::load(path, &shortcuts, scope())
        .await
        .unwrap();
    assert_eq!(
        restored.breaker().snapshot("browser_source").state,
        CircuitState::Open
    );
    assert!(restored.breaker().acquire("browser_source").is_err());
}

#[tokio::test]
async fn credential_scoped_terminal_failures_are_not_persisted_globally() {
    let directory = tempdir().unwrap();
    let path = directory.path().join(TEST_STATE_FILE);
    let shortcuts = vec!["browser_source".to_string()];
    let state = PersistentCircuitState::load(path.clone(), &shortcuts, scope())
        .await
        .unwrap();
    state
        .breaker()
        .acquire("browser_source")
        .unwrap()
        .record_failure(&EngineFailure::new(
            "Provider source",
            "provider_quota",
            "quota exhausted",
        ));
    state
        .persist(&outcome("failure", Some("provider_quota")))
        .await
        .unwrap();

    let restored = PersistentCircuitState::load(path, &shortcuts, scope())
        .await
        .unwrap();
    assert_eq!(
        restored.breaker().snapshot("browser_source").state,
        CircuitState::Closed
    );
}

#[tokio::test]
async fn successful_half_open_probe_removes_only_the_loaded_generation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join(TEST_STATE_FILE);
    let now = unix_millis();
    create_private_directory(directory.path()).unwrap();
    write_state_atomically(
        &path,
        &StateFile {
            schema: STATE_SCHEMA.to_string(),
            scope_sha256: scope(),
            entries: BTreeMap::from([(
                "browser_source".to_string(),
                StateEntry {
                    open_until_unix_ms: now,
                    updated_at_unix_ms: now.saturating_sub(1),
                    ejection_count: 3,
                    failure_kind: "challenge".to_string(),
                },
            )]),
        },
    )
    .unwrap();
    let shortcuts = vec!["browser_source".to_string()];
    let state = PersistentCircuitState::load(path.clone(), &shortcuts, scope())
        .await
        .unwrap();
    state
        .breaker()
        .acquire("browser_source")
        .unwrap()
        .record_success();
    state.persist(&outcome("success", None)).await.unwrap();

    assert!(read_state(&path, &scope()).unwrap().entries.is_empty());
}

#[tokio::test]
async fn successful_probe_does_not_erase_a_concurrent_newer_open() {
    let directory = tempdir().unwrap();
    let path = directory.path().join(TEST_STATE_FILE);
    let now = unix_millis();
    create_private_directory(directory.path()).unwrap();
    let loaded_entry = StateEntry {
        open_until_unix_ms: now,
        updated_at_unix_ms: now.saturating_sub(1),
        ejection_count: 1,
        failure_kind: "challenge".to_string(),
    };
    write_state_atomically(
        &path,
        &StateFile {
            schema: STATE_SCHEMA.to_string(),
            scope_sha256: scope(),
            entries: BTreeMap::from([("browser_source".to_string(), loaded_entry)]),
        },
    )
    .unwrap();
    let shortcuts = vec!["browser_source".to_string()];
    let state = PersistentCircuitState::load(path.clone(), &shortcuts, scope())
        .await
        .unwrap();
    state
        .breaker()
        .acquire("browser_source")
        .unwrap()
        .record_success();

    let concurrent_entry = StateEntry {
        open_until_unix_ms: now.saturating_add(60_000),
        updated_at_unix_ms: now.saturating_add(1),
        ejection_count: 2,
        failure_kind: "challenge".to_string(),
    };
    write_state_atomically(
        &path,
        &StateFile {
            schema: STATE_SCHEMA.to_string(),
            scope_sha256: scope(),
            entries: BTreeMap::from([("browser_source".to_string(), concurrent_entry.clone())]),
        },
    )
    .unwrap();
    state.persist(&outcome("success", None)).await.unwrap();

    assert_eq!(
        read_state(&path, &scope()).unwrap().entries["browser_source"],
        concurrent_entry
    );
}

#[test]
fn corrupt_or_overscoped_state_fails_open_without_reusing_content() {
    let directory = tempdir().unwrap();
    let path = directory.path().join(TEST_STATE_FILE);
    fs::write(&path, b"{not-json}\n").unwrap();

    assert!(read_state(&path, &scope()).unwrap().entries.is_empty());
    assert_eq!(
        normalized_shortcuts(&["Bad key!".to_string()]),
        Vec::<String>::new()
    );
}

#[test]
fn transport_scopes_separate_direct_proxy_and_browser_routes() {
    assert_ne!(
        transport_scope(None, "chrome"),
        transport_scope(Some("http://127.0.0.1:8080"), "chrome")
    );
    assert_ne!(
        transport_scope(None, "chrome"),
        transport_scope(None, "lightpanda")
    );
}
