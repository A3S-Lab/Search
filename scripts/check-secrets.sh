#!/usr/bin/env bash
set -euo pipefail

repository="A3S-Lab/Search"
required_secrets=(
  "CARGO_TOKEN"
  "HOMEBREW_TAP_TOKEN"
)

if ! command -v gh >/dev/null 2>&1; then
  echo "GitHub CLI (gh) is required." >&2
  exit 1
fi

configured=$(gh secret list --repo "${repository}" --json name --jq '.[].name')
missing=0

echo "Required GitHub Actions secrets for ${repository}:"
for secret in "${required_secrets[@]}"; do
  if grep -Fxq "${secret}" <<<"${configured}"; then
    echo "  ${secret}: configured"
  else
    echo "  ${secret}: missing"
    missing=1
  fi
done

if ((missing)); then
  echo >&2
  echo "Configure each missing value with:" >&2
  echo "  gh secret set SECRET_NAME --repo ${repository}" >&2
  exit 1
fi
