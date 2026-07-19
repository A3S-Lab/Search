#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 <a3s-search-binary> <output.tar.gz>" >&2
}

if [[ $# -ne 2 ]]; then
  usage
  exit 2
fi

binary=$1
archive=$2
repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
skill_root="${repository_root}/skills/a3s-search"

if [[ ! -f "${binary}" || ! -x "${binary}" ]]; then
  echo "Release binary must be an executable file: ${binary}" >&2
  exit 1
fi

for required_file in \
  "${skill_root}/SKILL.md" \
  "${skill_root}/agents/openai.yaml"; do
  if [[ ! -f "${required_file}" ]]; then
    echo "Required release file is missing: ${required_file}" >&2
    exit 1
  fi
done

archive_parent=$(dirname "${archive}")
if [[ ! -d "${archive_parent}" ]]; then
  echo "Archive parent directory does not exist: ${archive_parent}" >&2
  exit 1
fi

staging=$(mktemp -d "${TMPDIR:-/tmp}/a3s-search-package.XXXXXX")
archive_tmp="${archive}.tmp.$$"

cleanup() {
  if [[ -d "${staging}" ]]; then
    find "${staging}" -depth -delete
  fi
  if [[ -f "${archive_tmp}" ]]; then
    find "${archive_tmp}" -delete
  fi
}
trap cleanup EXIT

install -m 0755 "${binary}" "${staging}/a3s-search"
install -d "${staging}/skills/a3s-search/agents"
install -m 0644 "${skill_root}/SKILL.md" \
  "${staging}/skills/a3s-search/SKILL.md"
install -m 0644 "${skill_root}/agents/openai.yaml" \
  "${staging}/skills/a3s-search/agents/openai.yaml"

tar -czf "${archive_tmp}" \
  -C "${staging}" \
  a3s-search \
  skills/a3s-search/SKILL.md \
  skills/a3s-search/agents/openai.yaml

expected_listing=$'a3s-search\nskills/a3s-search/SKILL.md\nskills/a3s-search/agents/openai.yaml'
actual_listing=$(tar -tzf "${archive_tmp}")
if [[ "${actual_listing}" != "${expected_listing}" ]]; then
  echo "Release archive contains unexpected paths:" >&2
  printf '%s\n' "${actual_listing}" >&2
  exit 1
fi

mv "${archive_tmp}" "${archive}"
