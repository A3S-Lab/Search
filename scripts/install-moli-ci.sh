#!/usr/bin/env bash
set -euo pipefail

# Keep the CI runtime pinned so a changing "latest" release cannot silently
# change the browser contract tested by Search. The checksums are the published
# GitHub release digests for Moli v1.1.1.
version="1.1.1"
case "$(uname -s):$(uname -m)" in
  Linux:x86_64 | Linux:amd64)
    target="x86_64-unknown-linux-gnu"
    sha256="7b3eb9cbbf2cc8bd5ea9ef4a5bdb24cee2df35d26da621216a8b69c2aff3ebaa"
    ;;
  Linux:aarch64 | Linux:arm64)
    target="aarch64-unknown-linux-gnu"
    sha256="549484765476b8dd3fd93ebf59a089e4424425a961c14874974a88bba6d8b5b4"
    ;;
  Darwin:x86_64 | Darwin:amd64)
    target="x86_64-apple-darwin"
    sha256="bb4f80d6a2786909457a66675ec5cd2118038afaaedeac0d90f9911427d38f56"
    ;;
  Darwin:arm64 | Darwin:aarch64)
    target="aarch64-apple-darwin"
    sha256="56deed4634b9c77641ce31f3802b9bb3f32c6d7f28073f73901540429a29864b"
    ;;
  *)
    echo "Moli CI installer: unsupported platform $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/a3s-moli-${version}-${target}"
archive="${root}.tar.gz"
mkdir -p "$root"
url="https://github.com/lexmount/moli/releases/download/v${version}/moli-${target}.tar.gz"

curl --proto '=https' --tlsv1.2 --retry 3 --retry-all-errors --retry-delay 2 \
  -fsSL "$url" -o "$archive"
if command -v sha256sum >/dev/null 2>&1; then
  printf '%s  %s\n' "$sha256" "$archive" | sha256sum -c - >&2
else
  actual=$(shasum -a 256 "$archive" | awk '{print $1}')
  test "$actual" = "$sha256"
fi

tar -xzf "$archive" -C "$root" --strip-components=1
test -x "$root/moli"
# Keep stdout machine-readable for the workflow's GITHUB_ENV assignment while
# still exposing the verified runtime version in the job log.
"$root/moli" --version >&2
printf '%s\n' "$root/moli"
