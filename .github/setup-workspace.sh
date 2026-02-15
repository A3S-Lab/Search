#!/bin/bash
# Setup a minimal workspace context for building the Search crate standalone.
# The Search repo is normally a submodule of the a3s workspace, so we need to
# recreate the workspace structure for `edition.workspace = true` to resolve.
#
# This script restructures the CURRENT directory in-place:
#   Before: ./ = Search repo root (Cargo.toml, src/, sdk/, ...)
#   After:  ./ = workspace root with crates/search/ and crates/updater/

set -euo pipefail

# Save current directory contents to a temp location
TMPDIR="$(mktemp -d)"
cp -a . "$TMPDIR/search"

# Clean current directory (except .git)
find . -maxdepth 1 ! -name '.' ! -name '.git' -exec rm -rf {} +

# Create workspace structure
mkdir -p crates/updater/src

# Move Search repo into crates/search
cp -a "$TMPDIR/search/." crates/search/

# Create workspace root Cargo.toml
cat > Cargo.toml << 'EOF'
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
cat > crates/updater/Cargo.toml << 'EOF'
[package]
name = "a3s-updater"
version = "0.2.0"
edition = "2021"
authors = ["A3S Lab"]
license = "MIT"

[dependencies]
anyhow = "1"
EOF
cat > crates/updater/src/lib.rs << 'EOF'
pub struct UpdateConfig {
    pub binary_name: &'static str,
    pub crate_name: &'static str,
    pub current_version: &'static str,
    pub github_owner: &'static str,
    pub github_repo: &'static str,
}
pub async fn run_update(_: &UpdateConfig) -> anyhow::Result<()> { Ok(()) }
EOF

# Clean up
rm -rf "$TMPDIR"

# Remove nested [workspace] sections from search crate Cargo.toml
# (the workspace root already defines these)
python3 - crates/search/Cargo.toml << 'PYEOF'
import sys, re
path = sys.argv[1]
with open(path) as f:
    content = f.read()
# Remove [workspace], [workspace.package], [workspace.dependencies] sections
content = re.sub(r'\[workspace\]\nmembers = \[\]\n\n', '', content)
content = re.sub(r'\[workspace\.package\]\n(?:.*\n)*?\n(?=\[)', '', content)
content = re.sub(r'\[workspace\.dependencies\]\n(?:.*\n)*?\n(?=\[)', '', content)
with open(path, 'w') as f:
    f.write(content)
PYEOF

echo "Workspace restructured. Search crate at: crates/search/"
