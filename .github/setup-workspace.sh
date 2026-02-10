#!/bin/bash
# Setup a minimal workspace context for building the Search crate standalone.
# The Search repo is normally a submodule of the a3s workspace, so we need to
# recreate the workspace structure for `edition.workspace = true` to resolve.

set -euo pipefail

REPO_DIR="$(pwd)"
WORKSPACE_DIR="$(mktemp -d)"

# Create workspace structure
mkdir -p "$WORKSPACE_DIR/crates/updater/src"

# Create workspace root Cargo.toml
cat > "$WORKSPACE_DIR/Cargo.toml" << 'EOF'
[workspace]
resolver = "2"
members = ["crates/search", "crates/updater"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/a3s-lab/a3s"
authors = ["A3S Lab"]

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
async-trait = "0.1"
futures = "0.3"
tokio-test = "0.4"
EOF

# Create stub a3s-updater crate
cat > "$WORKSPACE_DIR/crates/updater/Cargo.toml" << 'EOF'
[package]
name = "a3s-updater"
version = "0.1.0"
edition = "2021"
authors = ["A3S Lab"]
license = "MIT"
EOF
echo "pub fn stub() {}" > "$WORKSPACE_DIR/crates/updater/src/lib.rs"

# Symlink the Search repo into the workspace
ln -s "$REPO_DIR" "$WORKSPACE_DIR/crates/search"

echo "$WORKSPACE_DIR"
