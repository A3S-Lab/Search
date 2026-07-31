#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 <crate-file> <new-output-directory> <git-commit> <git-ref>" >&2
}

if [[ $# -ne 4 ]]; then
  usage
  exit 2
fi

crate_file=$1
output_directory=$2
git_commit=$3
git_ref=$4
repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "${repository_root}"

if [[ ! -f "${crate_file}" || -L "${crate_file}" ]]; then
  echo "Frozen crate input must be a regular non-symlink file: ${crate_file}" >&2
  exit 1
fi
if [[ -e "${output_directory}" ]]; then
  echo "Frozen crate output must not already exist: ${output_directory}" >&2
  exit 1
fi
if [[ ! "${git_commit}" =~ ^[0-9a-f]{40}$ ]]; then
  echo "Git commit must be one lowercase 40-character object ID." >&2
  exit 1
fi

read -r package_name package_version repository_url < <(
  cargo metadata --locked --no-deps --format-version 1 |
    python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
root = metadata["packages"][0]
print(root["name"], root["version"], root["repository"])
'
)

expected_ref="refs/tags/v${package_version}"
if [[ "${git_ref}" != "${expected_ref}" ]]; then
  echo "Git ref must be ${expected_ref}, got ${git_ref}" >&2
  exit 1
fi

expected_crate="${package_name}-${package_version}.crate"
if [[ "$(basename "${crate_file}")" != "${expected_crate}" ]]; then
  echo "Crate file must be named ${expected_crate}" >&2
  exit 1
fi

crate_size=$(wc -c < "${crate_file}" | tr -d '[:space:]')
if [[ "${crate_size}" -eq 0 || "${crate_size}" -gt 104857600 ]]; then
  echo "Frozen crate must be non-empty and no larger than 100 MiB." >&2
  exit 1
fi

expected_manifest="${package_name}-${package_version}/Cargo.toml"
archive_listing=$(tar -tzf "${crate_file}")
if ! grep -Fxq "${expected_manifest}" <<<"${archive_listing}"; then
  echo "Crate archive does not contain ${expected_manifest}" >&2
  exit 1
fi

output_parent=$(dirname "${output_directory}")
if [[ ! -d "${output_parent}" ]]; then
  echo "Frozen crate output parent does not exist: ${output_parent}" >&2
  exit 1
fi

staging=$(mktemp -d "${output_parent}/.a3s-search-frozen.XXXXXX")
published=false
cleanup() {
  if [[ "${published}" != true && -d "${staging}" ]]; then
    find "${staging}" -depth -delete
  fi
}
trap cleanup EXIT

install -m 0644 "${crate_file}" "${staging}/${expected_crate}"

python3 - \
  "${staging}/${expected_crate}" \
  "${staging}/frozen-crate.json" \
  "${git_commit}" \
  "${git_ref}" \
  "${package_name}" \
  "${package_version}" \
  "${repository_url}" \
  "${repository_root}/Cargo.toml" \
  "${repository_root}/Cargo.lock" <<'PY'
import hashlib
import json
import os
import pathlib
import sys

crate_path = pathlib.Path(sys.argv[1])
manifest_path = pathlib.Path(sys.argv[2])
git_commit, git_ref, package_name, package_version, repository_url = sys.argv[3:8]
cargo_toml_path = pathlib.Path(sys.argv[8])
cargo_lock_path = pathlib.Path(sys.argv[9])


def identity(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


payload = {
    "schema": "a3s/search-frozen-crate/v1",
    "repository": repository_url,
    "git_commit": git_commit,
    "git_ref": git_ref,
    "package": {
        "name": package_name,
        "version": package_version,
        "file": crate_path.name,
        "sha256": identity(crate_path),
        "size_bytes": crate_path.stat().st_size,
    },
    "source": {
        "cargo_toml_sha256": identity(cargo_toml_path),
        "cargo_lock_sha256": identity(cargo_lock_path),
    },
}

encoded = json.dumps(
    payload,
    ensure_ascii=True,
    sort_keys=True,
    separators=(",", ":"),
).encode("utf-8") + b"\n"
descriptor = os.open(
    manifest_path,
    os.O_WRONLY | os.O_CREAT | os.O_EXCL,
    0o644,
)
with os.fdopen(descriptor, "wb") as stream:
    stream.write(encoded)
    stream.flush()
    os.fsync(stream.fileno())
PY

mv "${staging}" "${output_directory}"
published=true
trap - EXIT
