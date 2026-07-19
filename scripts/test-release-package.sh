#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/a3s-search-package-test.XXXXXX")

cleanup() {
  if [[ -d "${temporary_root}" ]]; then
    find "${temporary_root}" -depth -delete
  fi
}
trap cleanup EXIT

fixture_binary="${temporary_root}/fixture-a3s-search"
archive="${temporary_root}/a3s-search-test.tar.gz"
extracted="${temporary_root}/extracted"

printf '#!/usr/bin/env sh\nprintf "a3s-search test fixture\\n"\n' > "${fixture_binary}"
chmod 0755 "${fixture_binary}"

"${repository_root}/scripts/package-release.sh" "${fixture_binary}" "${archive}"

mkdir "${extracted}"
tar -xzf "${archive}" -C "${extracted}"

test -x "${extracted}/a3s-search"
cmp "${fixture_binary}" "${extracted}/a3s-search"
cmp \
  "${repository_root}/skills/a3s-search/SKILL.md" \
  "${extracted}/skills/a3s-search/SKILL.md"
cmp \
  "${repository_root}/skills/a3s-search/agents/openai.yaml" \
  "${extracted}/skills/a3s-search/agents/openai.yaml"

echo "Release archive layout and contents are valid."
