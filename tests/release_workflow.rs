#[test]
fn release_waits_for_the_exact_browser_crate_before_validation() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let manifest = include_str!("../Cargo.toml");
    let gate = workflow
        .find("https://crates.io/api/v1/crates/a3s-use-browser/${version}")
        .expect("release workflow must wait for the Browser crate");
    let validation = workflow
        .find("cargo fmt --all -- --check")
        .expect("release workflow must retain its format gate");

    assert!(
        gate < validation,
        "Browser must be visible on crates.io before Search validation and packaging"
    );
    assert!(
        manifest.contains("a3s-use-browser = { version = \"="),
        "Search must use an exact Browser release version"
    );
}

#[test]
fn stable_release_fails_closed_without_executing_candidate_code_on_a_privileged_runner() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let blocker = job_body(workflow, "commercial-search-gates");
    assert!(blocker.contains("needs.classify.outputs.stable == 'true'"));
    assert!(blocker.contains("https://github.com/A3S-Lab/Search/issues/8"));
    assert!(blocker.contains("exit 1"));
    for forbidden in [
        "actions/checkout",
        "cargo test",
        "self-hosted",
        "environment:",
        "secrets.",
        "vars.",
    ] {
        assert!(
            !blocker.contains(forbidden),
            "stable blocker must not expose candidate code to privileged state: {forbidden}"
        );
    }
}

#[test]
fn every_release_write_path_is_transitively_blocked_by_the_aggregate_gate() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let aggregate = job_body(workflow, "commercial-release-gate");
    let build = job_body(workflow, "build-cli");
    let publish = job_body(workflow, "publish-crate");
    let github = job_body(workflow, "github-release");
    let homebrew = job_body(workflow, "update-homebrew");

    assert!(aggregate.contains("needs: [classify, ci, commercial-search-gates]"));
    assert!(aggregate.contains("if: always() && !cancelled()"));
    assert!(build.contains("needs: commercial-release-gate"));
    assert!(publish.contains("needs: commercial-release-gate"));
    assert!(github.contains("needs: [classify, publish-crate, build-cli]"));
    assert!(homebrew.contains("needs: [classify, github-release]"));
    assert!(homebrew.contains("needs.classify.outputs.stable == 'true'"));
}

#[test]
fn cancellation_cannot_continue_into_release_artifacts_or_publication() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let aggregate = job_body(workflow, "commercial-release-gate");
    assert!(aggregate.contains("if: always() && !cancelled()"));
    for downstream in [
        "build-cli",
        "publish-crate",
        "github-release",
        "update-homebrew",
    ] {
        assert!(
            job_body(workflow, downstream).contains("needs:"),
            "{downstream} must remain transitively downstream from the aggregate gate"
        );
    }
}

#[test]
fn candidate_checkouts_bind_to_the_trigger_commit_without_persisted_credentials() {
    let workflow = include_str!("../.github/workflows/release.yml");
    for job in ["classify", "ci", "build-cli", "github-release"] {
        let body = job_body(workflow, job);
        assert!(
            body.contains("ref: ${{ github.sha }}"),
            "{job} must checkout the immutable trigger commit"
        );
        assert!(
            body.contains("persist-credentials: false"),
            "{job} must not persist a credential in the candidate checkout"
        );
    }
}

#[test]
fn crate_publication_is_fail_closed_without_candidate_code_or_registry_secrets() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let publish = job_body(workflow, "publish-crate");
    assert!(publish.contains("https://github.com/A3S-Lab/Search/issues/8"));
    assert!(publish.contains("exit 1"));
    for forbidden in [
        "actions/checkout",
        "cargo publish",
        "cargo package",
        "CARGO_REGISTRY_TOKEN",
        "secrets.",
        "build.rs",
    ] {
        assert!(
            !publish.contains(forbidden),
            "blocked publication must not expose credentials or execute candidate code: {forbidden}"
        );
    }
}

#[test]
fn every_third_party_action_is_pinned_to_a_full_commit() {
    let workflow = include_str!("../.github/workflows/release.yml");
    for line in workflow
        .lines()
        .filter(|line| line.trim_start().starts_with("uses:"))
    {
        let reference = line
            .split_once('@')
            .unwrap_or_else(|| panic!("action is missing a revision: {line}"))
            .1
            .split_whitespace()
            .next()
            .unwrap_or_default();
        assert!(
            reference.len() == 40 && reference.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "action must be pinned to a full commit SHA: {line}"
        );
    }
}

#[test]
fn pinned_rust_action_also_selects_an_exact_compiler() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let setup_count = workflow.matches("uses: dtolnay/rust-toolchain@").count();
    let compiler_count = workflow.matches("toolchain: 1.96.0").count();

    assert!(setup_count > 0, "release workflow must install Rust");
    assert_eq!(
        compiler_count, setup_count,
        "each pinned rust-toolchain action must select the exact compiler"
    );
}

fn job_body<'a>(workflow: &'a str, job: &str) -> &'a str {
    let marker = format!("\n  {job}:\n");
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("workflow job is missing: {job}"))
        + 1;
    let tail = &workflow[start..];
    let mut offset = 0usize;
    for line in tail.split_inclusive('\n') {
        if offset > 0
            && line.starts_with("  ")
            && !line.starts_with("    ")
            && line.trim_end().ends_with(':')
        {
            return &tail[..offset];
        }
        offset += line.len();
    }
    tail
}
