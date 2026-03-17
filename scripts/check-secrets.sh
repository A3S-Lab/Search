#!/bin/bash
# 检查 GitHub Actions secrets 配置状态

echo "🔍 Checking GitHub Actions secrets configuration..."
echo ""

REPO="A3S-Lab/Search"

# 检查必需的 secrets
REQUIRED_SECRETS=(
  "CARGO_TOKEN"
  "NPM_TOKEN"
  "PYPI_TOKEN"
  "HOMEBREW_TAP_TOKEN"
)

echo "Required secrets for $REPO:"
echo ""

for secret in "${REQUIRED_SECRETS[@]}"; do
  if gh secret list --repo "$REPO" | grep -q "^$secret"; then
    echo "✅ $secret - configured"
  else
    echo "❌ $secret - NOT configured"
  fi
done

echo ""
echo "To set a missing secret:"
echo "  echo 'YOUR_TOKEN' | gh secret set SECRET_NAME --repo $REPO"
echo ""
echo "To get tokens:"
echo "  CARGO_TOKEN:          cat ~/.cargo/credentials.toml"
echo "  NPM_TOKEN:            cat ~/.npmrc | grep authToken"
echo "  PYPI_TOKEN:           cat ~/.pypirc | grep password"
echo "  HOMEBREW_TAP_TOKEN:   gh auth token"
