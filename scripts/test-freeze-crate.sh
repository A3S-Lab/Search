#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/a3s-search-freeze-test.XXXXXX")

cleanup() {
  if [[ -d "${temporary_root}" ]]; then
    find "${temporary_root}" -depth -delete
  fi
}
trap cleanup EXIT

read -r package_name package_version < <(
  cd "${repository_root}"
  cargo metadata --locked --no-deps --format-version 1 |
    python3 -c '
import json
import sys

root = json.load(sys.stdin)["packages"][0]
print(root["name"], root["version"])
'
)

archive_root="${temporary_root}/${package_name}-${package_version}"
crate_file="${temporary_root}/${package_name}-${package_version}.crate"
output_directory="${temporary_root}/frozen"
git_commit=$(printf 'a%.0s' {1..40})
git_ref="refs/tags/v${package_version}"

mkdir "${archive_root}"
printf '[package]\nname = "%s"\nversion = "%s"\n' \
  "${package_name}" \
  "${package_version}" \
  > "${archive_root}/Cargo.toml"
tar -czf "${crate_file}" -C "${temporary_root}" "${package_name}-${package_version}"

"${repository_root}/scripts/freeze-crate.sh" \
  "${crate_file}" \
  "${output_directory}" \
  "${git_commit}" \
  "${git_ref}"

cmp "${crate_file}" "${output_directory}/${package_name}-${package_version}.crate"
python3 - \
  "${output_directory}/frozen-crate.json" \
  "${output_directory}/${package_name}-${package_version}.crate" \
  "${package_name}" \
  "${package_version}" \
  "${git_commit}" \
  "${git_ref}" <<'PY'
import hashlib
import json
import pathlib
import sys

manifest_path, crate_path = map(pathlib.Path, sys.argv[1:3])
package_name, package_version, git_commit, git_ref = sys.argv[3:]
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
crate_bytes = crate_path.read_bytes()

assert manifest["schema"] == "a3s/search-frozen-crate/v1"
assert manifest["repository"] == "https://github.com/A3S-Lab/Search"
assert manifest["git_commit"] == git_commit
assert manifest["git_ref"] == git_ref
assert manifest["package"]["name"] == package_name
assert manifest["package"]["version"] == package_version
assert manifest["package"]["file"] == crate_path.name
assert manifest["package"]["size_bytes"] == len(crate_bytes)
assert manifest["package"]["sha256"] == (
    "sha256:" + hashlib.sha256(crate_bytes).hexdigest()
)
assert manifest["source"]["cargo_toml_sha256"].startswith("sha256:")
assert manifest["source"]["cargo_lock_sha256"].startswith("sha256:")
assert manifest_path.read_bytes().endswith(b"\n")
PY

if "${repository_root}/scripts/freeze-crate.sh" \
  "${crate_file}" \
  "${output_directory}" \
  "${git_commit}" \
  "${git_ref}" >/dev/null 2>&1; then
  echo "Freeze script overwrote an existing evidence directory." >&2
  exit 1
fi

if "${repository_root}/scripts/freeze-crate.sh" \
  "${crate_file}" \
  "${temporary_root}/wrong-ref" \
  "${git_commit}" \
  "refs/tags/v0.0.0" >/dev/null 2>&1; then
  echo "Freeze script accepted a tag that does not match the package." >&2
  exit 1
fi

echo "Frozen crate bytes and identity manifest are valid."
