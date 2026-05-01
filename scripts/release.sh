#!/bin/bash
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <version>"
  echo "Example: $0 0.9.0"
  exit 1
fi

NEW_VERSION="$1"

echo "🚀 Releasing version $NEW_VERSION"

# 1. 更新版本号
echo "📝 Updating version numbers..."
sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml

# 2. 更新 Cargo.lock
echo "🔒 Updating Cargo.lock..."
cargo update -p a3s-search

# 3. 运行测试
echo "🧪 Running tests..."
cargo test --lib

# 4. 提交
echo "💾 Committing changes..."
git add -A
git commit -m "chore: bump version to v$NEW_VERSION"

# 5. 创建 tag
echo "🏷️  Creating tag..."
git tag "v$NEW_VERSION"

# 6. 推送
echo "📤 Pushing to remote..."
git push origin main --tags

echo "✅ Release v$NEW_VERSION initiated!"
echo "📊 Monitor progress: gh run watch --repo A3S-Lab/Search"
