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
fn stable_release_uses_a_pinned_independent_verifier_in_a_protected_environment() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let verifier = job_body(workflow, "commercial-search-gates");
    assert!(verifier.contains("needs.classify.outputs.stable == 'true'"));
    assert!(verifier.contains("environment: stable-release"));
    assert!(verifier.contains("python-version: \"3.12.12\""));
    assert!(
        verifier.contains("uses: A3S-Lab/SearchVerifier@93f8e5db93b3b68a886f90d56395883e98fdf2e0")
    );
    assert!(verifier.contains("name: frozen-crate-${{ needs.classify.outputs.version }}"));
    assert!(verifier.contains("evaluated-commit: ${{ github.sha }}"));
    assert!(verifier.contains("git-ref: ${{ github.ref }}"));
    assert!(verifier.contains("corpus-key: ${{ secrets.A3S_SEARCH_CORPUS_KEY }}"));
    assert!(verifier.contains("if: always()"));
    assert!(verifier.contains("name: release-evidence-${{ needs.classify.outputs.version }}"));
    assert!(verifier.contains("if-no-files-found: error"));
    for forbidden in [
        "actions/checkout",
        "self-hosted",
        "contents: write",
        "CARGO_REGISTRY_TOKEN",
        "cargo publish",
    ] {
        assert!(
            !verifier.contains(forbidden),
            "independent verifier job must not gain a publication path: {forbidden}"
        );
    }
}

#[test]
fn release_freezes_one_exact_crate_before_external_evidence_is_considered() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let freeze = job_body(workflow, "freeze-crate");
    let commercial = job_body(workflow, "commercial-search-gates");
    let aggregate = job_body(workflow, "commercial-release-gate");

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
    assert!(commercial.contains("needs: [classify, ci, freeze-crate]"));
    assert!(aggregate.contains("needs: [classify, ci, freeze-crate, commercial-search-gates]"));
    assert!(aggregate.contains("FROZEN_CRATE_RESULT: ${{ needs.freeze-crate.result }}"));
    assert!(aggregate.contains("test \"$FROZEN_CRATE_RESULT\" = success"));
    assert!(aggregate.contains(
        "VERIFIED_CRATE_SHA256: ${{ needs.commercial-search-gates.outputs.frozen_crate_sha256 }}"
    ));
    assert!(aggregate.contains("test \"$VERIFIED_CRATE_SHA256\" = \"$FROZEN_CRATE_SHA256\""));
    assert!(aggregate
        .contains("EVIDENCE_SHA256: ${{ needs.commercial-search-gates.outputs.evidence_sha256 }}"));
    assert!(aggregate.contains(
        "EVIDENCE_ARTIFACT_ID: ${{ needs.commercial-search-gates.outputs.artifact_id }}"
    ));
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
    let aggregate = job_body(workflow, "commercial-release-gate");
    let build = job_body(workflow, "build-cli");
    let publish = job_body(workflow, "publish-crate");
    let github = job_body(workflow, "github-release");
    let homebrew = job_body(workflow, "update-homebrew");

    assert!(aggregate.contains("needs: [classify, ci, freeze-crate, commercial-search-gates]"));
    assert!(aggregate.contains("if: always() && !cancelled()"));
    assert!(build.contains("needs: commercial-release-gate"));
    assert!(publish.contains("if: needs.classify.outputs.stable == 'true'"));
    assert!(publish.contains(
        "needs: [classify, freeze-crate, commercial-search-gates, commercial-release-gate]"
    ));
    assert!(github.contains("needs: [classify, commercial-release-gate, publish-crate, build-cli]"));
    assert!(github.contains("if: |"));
    assert!(github.contains("always() &&"));
    assert!(github.contains("!cancelled() &&"));
    assert!(github.contains("needs.commercial-release-gate.result == 'success'"));
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
    assert!(build.contains("needs.commercial-release-gate.result == 'success'"));
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
    for job in [
        "classify",
        "ci",
        "freeze-crate",
        "build-cli",
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
fn crate_publication_uses_the_same_pinned_exact_byte_uploader_without_candidate_code() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let publish = job_body(workflow, "publish-crate");
    assert!(publish.contains("if: needs.classify.outputs.stable == 'true'"));
    assert!(publish.contains("python-version: \"3.12.12\""));
    assert!(publish.contains("name: frozen-crate-${{ needs.classify.outputs.version }}"));
    assert!(publish.contains("name: release-evidence-${{ needs.classify.outputs.version }}"));
    assert!(publish
        .contains("uses: A3S-Lab/SearchVerifier/publish@93f8e5db93b3b68a886f90d56395883e98fdf2e0"));
    assert!(
        publish.contains("expected-crate-sha256: ${{ needs.freeze-crate.outputs.crate_sha256 }}")
    );
    assert!(publish.contains(
        "expected-evidence-sha256: ${{ needs.commercial-search-gates.outputs.evidence_sha256 }}"
    ));
    assert!(publish.contains("expected-commit: ${{ github.sha }}"));
    assert!(publish.contains("expected-git-ref: ${{ github.ref }}"));
    assert!(publish.contains("registry-token: ${{ secrets.CARGO_TOKEN }}"));
    for forbidden in [
        "actions/checkout",
        "dtolnay/rust-toolchain",
        "cargo build",
        "cargo test",
        "cargo publish",
        "cargo package",
        "build.rs",
    ] {
        assert!(
            !publish.contains(forbidden),
            "credentialed uploader must not execute candidate code: {forbidden}"
        );
    }
}

#[test]
fn verifier_and_uploader_are_pinned_to_the_same_immutable_revision() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let revision = "93f8e5db93b3b68a886f90d56395883e98fdf2e0";
    let verifier = job_body(workflow, "commercial-search-gates");
    let publish = job_body(workflow, "publish-crate");

    assert!(verifier.contains(&format!("A3S-Lab/SearchVerifier@{revision}")));
    assert!(publish.contains(&format!("A3S-Lab/SearchVerifier/publish@{revision}")));
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
