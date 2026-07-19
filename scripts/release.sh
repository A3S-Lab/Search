#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 <major.minor.patch>" >&2
}

if [[ $# -ne 1 ]]; then
  usage
  exit 2
fi

version=$1
if [[ ! "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "Version must be a semantic version without a leading v: ${version}" >&2
  exit 2
fi

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repository_root}"

if [[ $(git branch --show-current) != "main" ]]; then
  echo "Release preparation must run from the main branch." >&2
  exit 1
fi
if [[ -n $(git status --porcelain) ]]; then
  echo "Release preparation requires a clean worktree." >&2
  exit 1
fi
if git rev-parse --verify --quiet "refs/tags/v${version}" >/dev/null; then
  echo "Tag v${version} already exists." >&2
  exit 1
fi

NEW_VERSION="${version}" perl -0pi -e \
  's/(\[package\].*?\nversion = ")[^"]+(")/$1$ENV{NEW_VERSION}$2/s' \
  Cargo.toml

# Refresh only generated dependency metadata before locking every verification
# command to the reviewed dependency graph.
cargo metadata --format-version 1 >/dev/null

cargo fmt --all -- --check
cargo test --no-default-features --locked
cargo test --all-features --locked
cargo clippy --all-targets --no-default-features --locked -- -D warnings
cargo clippy --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
cargo package --locked --allow-dirty
scripts/test-release-package.sh
git diff --check

git add Cargo.toml Cargo.lock
git commit -m "release: prepare a3s-search ${version}"
git tag "v${version}"

cat <<EOF
Prepared a3s-search ${version} and created tag v${version}.
Review the commit and tag, then publish explicitly:

  git push origin main
  git push origin v${version}
EOF
