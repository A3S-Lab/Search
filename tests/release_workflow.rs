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
fn release_has_no_external_verifier_or_python_action_dependency() {
    let workflow = include_str!("../.github/workflows/release.yml");

    for forbidden in [
        "uses: A3S-Lab/",
        "commercial-search-gates",
        "A3S_SEARCH_CORPUS_KEY",
        "actions/setup-python",
        "release-evidence-",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "release workflow retained removed verifier dependency: {forbidden}"
        );
    }
}

#[test]
fn release_freezes_one_exact_crate_before_the_aggregate_gate() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let freeze = job_body(workflow, "freeze-crate");
    let aggregate = job_body(workflow, "release-gate");

    assert!(freeze.contains("needs: [classify, ci]"));
    assert!(freeze.contains("ref: ${{ github.sha }}"));
    assert!(freeze.contains("persist-credentials: false"));
    assert!(freeze.contains("cargo package --locked"));
    assert!(freeze.contains("scripts/freeze-crate.sh"));
    assert!(freeze.contains("name: frozen-crate-${{ needs.classify.outputs.version }}"));
    assert!(freeze.contains("if-no-files-found: error"));
    assert!(freeze.contains("crate_sha256: ${{ steps.identity.outputs.crate_sha256 }}"));
    assert!(freeze.contains("artifact_id: ${{ steps.upload.outputs.artifact-id }}"));
    assert!(freeze.contains("artifact_digest: ${{ steps.upload.outputs.artifact-digest }}"));
    assert!(aggregate.contains("needs: [classify, ci, freeze-crate]"));
    assert!(aggregate.contains("FROZEN_CRATE_RESULT: ${{ needs.freeze-crate.result }}"));
    assert!(aggregate.contains("test \"$FROZEN_CRATE_RESULT\" = success"));
    assert!(aggregate.contains("FROZEN_CRATE_SHA256"));
    assert!(aggregate.contains("FROZEN_ARTIFACT_ID"));
    assert!(aggregate.contains("FROZEN_ARTIFACT_DIGEST"));
}

#[test]
fn frozen_crate_job_has_no_release_credentials_or_publication_path() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let freeze = job_body(workflow, "freeze-crate");

    for forbidden in [
        "contents: write",
        "environment:",
        "secrets.",
        "CARGO_REGISTRY_TOKEN",
        "cargo publish",
        "self-hosted",
    ] {
        assert!(
            !freeze.contains(forbidden),
            "frozen crate job must remain unprivileged: {forbidden}"
        );
    }
}

#[test]
fn every_release_write_path_is_transitively_blocked_by_the_aggregate_gate() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let aggregate = job_body(workflow, "release-gate");
    let build = job_body(workflow, "build-cli");
    let publish = job_body(workflow, "publish-crate");
    let github = job_body(workflow, "github-release");
    let homebrew = job_body(workflow, "update-homebrew");

    assert!(aggregate.contains("needs: [classify, ci, freeze-crate]"));
    assert!(aggregate.contains("if: always() && !cancelled()"));
    assert!(build.contains("needs: release-gate"));
    assert!(publish.contains("if: needs.classify.outputs.stable == 'true'"));
    assert!(publish.contains("needs: [classify, freeze-crate, release-gate]"));
    assert!(github.contains("needs: [classify, release-gate, publish-crate, build-cli]"));
    assert!(github.contains("if: |"));
    assert!(github.contains("always() &&"));
    assert!(github.contains("!cancelled() &&"));
    assert!(github.contains("needs.release-gate.result == 'success'"));
    assert!(github.contains("needs.build-cli.result == 'success'"));
    assert!(github.contains("needs.publish-crate.result == 'success'"));
    assert!(github.contains("needs.publish-crate.result == 'skipped'"));
    assert!(homebrew.contains("needs: [classify, github-release]"));
    assert!(homebrew.contains("needs.classify.outputs.stable == 'true'"));
}

#[test]
fn prerelease_can_publish_github_binaries_without_registry_or_homebrew() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let build = job_body(workflow, "build-cli");
    let publish = job_body(workflow, "publish-crate");
    let github = job_body(workflow, "github-release");
    let homebrew = job_body(workflow, "update-homebrew");
    let outcome = job_body(workflow, "release-outcome");

    assert!(build.contains("always() &&"));
    assert!(build.contains("!cancelled() &&"));
    assert!(build.contains("needs.release-gate.result == 'success'"));
    assert!(publish.contains("if: needs.classify.outputs.stable == 'true'"));
    assert!(github.contains("needs.classify.outputs.stable == 'false'"));
    assert!(github.contains("needs.publish-crate.result == 'skipped'"));
    assert!(github.contains("PRERELEASE_FLAG=\"--prerelease\""));
    assert!(homebrew.contains("needs.classify.outputs.stable == 'true'"));
    assert!(outcome.contains("needs.github-release.result"));
    assert!(outcome.contains("test \"$GITHUB_RELEASE_RESULT\" = success"));
    assert!(outcome.contains("test \"$HOMEBREW_RESULT\" = skipped"));
}

#[test]
fn cancellation_cannot_continue_into_release_artifacts_or_publication() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let aggregate = job_body(workflow, "release-gate");
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
    for job in [
        "classify",
        "ci",
        "freeze-crate",
        "build-cli",
        "publish-crate",
        "github-release",
    ] {
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
fn stable_crate_publication_reproduces_the_frozen_package_in_a_protected_environment() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let publish = job_body(workflow, "publish-crate");

    assert!(publish.contains("environment: stable-release"));
    assert!(publish.contains("name: frozen-crate-${{ needs.classify.outputs.version }}"));
    assert!(publish.contains("EXPECTED_CRATE_SHA256"));
    assert!(publish.contains("cargo package --locked"));
    assert!(publish.contains("cargo publish --locked --no-verify"));
    assert!(publish.contains("CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_TOKEN }}"));
    assert!(publish.contains("sha256sum \"$frozen\""));
    assert!(publish.contains("sha256sum \"$packaged\""));
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
