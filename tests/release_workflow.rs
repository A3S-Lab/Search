#[test]
fn release_waits_for_the_exact_browser_crate_before_validation() {
    let workflow = include_str!("../.github/workflows/release.yml");
    let manifest = include_str!("../Cargo.toml");
    let gate = workflow
        .find("https://crates.io/api/v1/crates/a3s-use-browser/${version}")
        .expect("release workflow must wait for the Browser crate");
    let validation = workflow
        .find("cargo fmt -- --check")
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
