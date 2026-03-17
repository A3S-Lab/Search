# 发布指南 / Release Guide

本文档说明如何发布 A3S Search 的新版本。

## 前置要求

### 1. GitHub Secrets 配置

在 GitHub 仓库设置中配置以下 secrets（Settings → Secrets and variables → Actions）：

| Secret 名称 | 用途 | 获取方式 |
|------------|------|---------|
| `CARGO_TOKEN` | 发布到 crates.io | `cat ~/.cargo/credentials.toml` |
| `NPM_TOKEN` | 发布到 npm | `cat ~/.npmrc \| grep authToken` |
| `PYPI_TOKEN` | 发布到 PyPI | `cat ~/.pypirc \| grep password` |
| `HOMEBREW_TAP_TOKEN` | 更新 Homebrew formula | `gh auth token` 或创建 GitHub PAT |

设置命令示例：
```bash
# 设置 crates.io token
echo "YOUR_TOKEN" | gh secret set CARGO_TOKEN --repo A3S-Lab/Search

# 设置 npm token
echo "YOUR_TOKEN" | gh secret set NPM_TOKEN --repo A3S-Lab/Search

# 设置 PyPI token
echo "YOUR_TOKEN" | gh secret set PYPI_TOKEN --repo A3S-Lab/Search

# 设置 Homebrew token
gh auth token | gh secret set HOMEBREW_TAP_TOKEN --repo A3S-Lab/Search
```

### 2. 版本号规范

遵循语义化版本（Semantic Versioning）：
- **MAJOR.MINOR.PATCH** (例如：0.8.0)
- **MAJOR**: 不兼容的 API 变更
- **MINOR**: 向后兼容的功能新增
- **PATCH**: 向后兼容的问题修复

## 发布流程

### 步骤 1: 更新版本号

需要在以下 **4 个文件** 中更新版本号：

```bash
# 设置新版本号
NEW_VERSION="0.9.0"

# 1. 核心 crate (Cargo.toml)
sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml

# 2. Python SDK (pyproject.toml)
sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" sdk/python/pyproject.toml

# 3. Node SDK 主包 (package.json)
sed -i '' "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" sdk/node/package.json

# 4. Node SDK 平台包 (npm/*/package.json)
find sdk/node/npm -name "package.json" -not -path "*/node_modules/*" \
  -exec sed -i '' "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/g" {} \;

# 5. 更新 Node SDK 的 optionalDependencies 版本
sed -i '' "s/@a3s-lab\/search-\([^\"]*\)\": \"[^\"]*\"/@a3s-lab\/search-\1\": \"$NEW_VERSION\"/g" sdk/node/package.json
```

### 步骤 2: 更新 Cargo.lock

```bash
cargo update -p a3s-search
```

### 步骤 3: 运行测试

```bash
# 核心库测试
cargo test --lib

# Node SDK 测试
cd sdk/node && npm test && cd ../..

# Python SDK 测试
cd sdk/python && pytest && cd ../..
```

### 步骤 4: 提交变更

```bash
git add -A
git commit -m "chore: bump version to v$NEW_VERSION"
```

### 步骤 5: 创建并推送 tag

```bash
# 创建 tag
git tag "v$NEW_VERSION"

# 推送到远程（同时推送 commit 和 tag）
git push origin main --tags
```

### 步骤 6: 监控 CI/CD

推送 tag 后，GitHub Actions 会自动触发 `release.yml` workflow，执行以下任务：

1. **CI 检查** (ci job)
   - 代码格式检查 (cargo fmt)
   - Clippy 静态分析
   - 单元测试

2. **构建 CLI 二进制** (build-cli job)
   - macOS arm64/x64
   - Linux arm64/x64

3. **发布到 crates.io** (publish-crate job)
   - 发布核心 Rust crate

4. **创建 GitHub Release** (github-release job)
   - 上传 CLI 二进制文件
   - 生成 checksums
   - 自动生成 release notes

5. **更新 Homebrew** (update-homebrew job)
   - 自动更新 homebrew-tap 仓库的 formula

6. **发布 Node SDK** (publish-node job)
   - 构建 7 个平台的原生模块
   - 发布到 npm (@a3s-lab/search)

7. **发布 Python SDK** (publish-python job)
   - 构建 7 个平台的 wheels
   - 发布到 PyPI (a3s-search)

监控命令：
```bash
# 查看最近的 workflow 运行
gh run list --repo A3S-Lab/Search --limit 5

# 查看特定 run 的详情
gh run view <run-id> --repo A3S-Lab/Search

# 实时查看日志
gh run watch <run-id> --repo A3S-Lab/Search
```

## 发布后验证

### 1. 验证 crates.io

```bash
cargo search a3s-search
# 或访问: https://crates.io/crates/a3s-search
```

### 2. 验证 npm

```bash
npm view @a3s-lab/search version
# 或访问: https://www.npmjs.com/package/@a3s-lab/search
```

### 3. 验证 PyPI

```bash
pip index versions a3s-search
# 或访问: https://pypi.org/project/a3s-search/
```

### 4. 验证 GitHub Release

```bash
gh release view "v$NEW_VERSION" --repo A3S-Lab/Search
# 或访问: https://github.com/A3S-Lab/Search/releases
```

### 5. 验证 Homebrew

```bash
brew update
brew info a3s-lab/tap/a3s-search
```

## 故障排查

### crates.io 发布失败

**错误**: "please provide a non-empty token"
```bash
# 检查 secret 是否设置
gh secret list --repo A3S-Lab/Search | grep CARGO_TOKEN

# 重新设置
echo "YOUR_TOKEN" | gh secret set CARGO_TOKEN --repo A3S-Lab/Search
```

**错误**: "crate version X.Y.Z is already uploaded"
- crates.io 不允许覆盖已发布的版本
- 需要增加版本号重新发布

### npm 发布失败

**错误**: "404 Not Found - PUT https://registry.npmjs.org/@a3s-lab%2fsearch"
- npm 组织 `@a3s-lab` 不存在或无权限
- 访问 https://www.npmjs.com/org/create 创建组织

**错误**: "platform packages not found"
- 检查 `sdk/node/npm/` 目录下是否有所有 7 个平台的 package.json
- 平台列表：darwin-arm64, darwin-x64, linux-x64-gnu, linux-x64-musl, linux-arm64-gnu, linux-arm64-musl, win32-x64-msvc

### PyPI 发布失败

**错误**: "version already exists"
- PyPI 不允许覆盖已发布的版本
- workflow 使用 `--skip-existing` 标志，会跳过已存在的版本

### Homebrew 更新失败

**错误**: "remote: Permission to A3S-Lab/homebrew-tap.git denied"
- `HOMEBREW_TAP_TOKEN` 权限不足
- 需要有 homebrew-tap 仓库的写权限

## 回滚

如果发布出现问题，可以：

1. **撤销 crates.io 发布**
   - crates.io 不支持删除版本，只能 yank
   ```bash
   cargo yank --vers X.Y.Z a3s-search
   ```

2. **撤销 npm 发布**
   ```bash
   npm unpublish @a3s-lab/search@X.Y.Z
   ```

3. **撤销 PyPI 发布**
   - 登录 https://pypi.org/project/a3s-search/
   - 在 Manage → Releases 中删除版本

4. **删除 GitHub Release**
   ```bash
   gh release delete vX.Y.Z --repo A3S-Lab/Search --yes
   git push --delete origin vX.Y.Z
   ```

## 自动化脚本

创建一个发布脚本 `scripts/release.sh`：

```bash
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
sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" sdk/python/pyproject.toml
sed -i '' "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" sdk/node/package.json
find sdk/node/npm -name "package.json" -not -path "*/node_modules/*" \
  -exec sed -i '' "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/g" {} \;
sed -i '' "s/@a3s-lab\/search-\([^\"]*\)\": \"[^\"]*\"/@a3s-lab\/search-\1\": \"$NEW_VERSION\"/g" sdk/node/package.json

# 2. 更新 Cargo.lock
echo "🔒 Updating Cargo.lock..."
cargo update -p a3s-search

# 3. 运行测试
echo "🧪 Running tests..."
cargo test --lib
(cd sdk/node && npm test)
(cd sdk/python && pytest)

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
```

使用方法：
```bash
chmod +x scripts/release.sh
./scripts/release.sh 0.9.0
```

## CI/CD 架构

```
Tag Push (v*)
    │
    ├─► CI Checks (fmt, clippy, test)
    │
    ├─► Build CLI (4 platforms)
    │   └─► Upload artifacts
    │
    ├─► Publish to crates.io
    │
    ├─► GitHub Release
    │   ├─► Download CLI artifacts
    │   ├─► Generate checksums
    │   └─► Create release with binaries
    │
    ├─► Update Homebrew
    │   ├─► Compute SHA256
    │   ├─► Update formula
    │   └─► Push to homebrew-tap
    │
    ├─► Publish Node SDK (7 platforms)
    │   ├─► Build native modules
    │   ├─► Publish platform packages
    │   └─► Publish main package
    │
    └─► Publish Python SDK (7 platforms)
        ├─► Build wheels
        └─► Upload to PyPI
```

## 相关链接

- **GitHub Repository**: https://github.com/A3S-Lab/Search
- **crates.io**: https://crates.io/crates/a3s-search
- **npm**: https://www.npmjs.com/package/@a3s-lab/search
- **PyPI**: https://pypi.org/project/a3s-search/
- **Homebrew Tap**: https://github.com/A3S-Lab/homebrew-tap
