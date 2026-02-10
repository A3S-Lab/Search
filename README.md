# A3S Search

<p align="center">
  <strong>Embeddable Meta Search Engine</strong>
</p>

<p align="center">
  <em>Utility layer — aggregate search results from multiple engines with ranking and deduplication</em>
</p>

<p align="center">
  <a href="#features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#sdks">SDKs</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#api-reference">API Reference</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Search** is an embeddable meta search engine library inspired by [SearXNG](https://github.com/searxng/searxng). It aggregates search results from multiple search engines, deduplicates them, and ranks them using a consensus-based scoring algorithm.

### Basic Usage

```rust
use a3s_search::{Search, SearchQuery, engines::{DuckDuckGo, Wikipedia}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a new search instance
    let mut search = Search::new();

    // Add search engines
    search.add_engine(DuckDuckGo::new());
    search.add_engine(Wikipedia::new());

    // Perform a search
    let query = SearchQuery::new("rust programming");
    let results = search.search(query).await?;

    // Display results
    for result in results.items().iter().take(10) {
        println!("{}: {}", result.title, result.url);
        println!("  Engines: {:?}, Score: {:.2}", result.engines, result.score);
    }

    Ok(())
}
```

## Features

- **Multi-Engine Search**: Aggregate results from multiple search engines in parallel
- **Result Deduplication**: Merge duplicate results based on normalized URLs
- **Consensus Ranking**: Results found by multiple engines rank higher
- **Configurable Weights**: Adjust engine influence on final rankings
- **Async-First**: Built on Tokio for high-performance concurrent searches
- **Timeout Handling**: Per-engine timeout with graceful degradation
- **Extensible**: Easy to add custom search engines via the `Engine` trait
- **Proxy Pool**: Dynamic proxy IP rotation to avoid anti-crawler blocking
- **Headless Browser**: Optional Chrome/Chromium integration for JS-rendered engines (feature-gated)
- **Auto-Install Chrome**: Automatically detects or downloads Chrome for Testing when no browser is found
- **PageFetcher Abstraction**: Pluggable page fetching (plain HTTP or headless browser)
- **CLI Tool**: Command-line interface for quick searches
- **Native SDKs**: TypeScript (NAPI) and Python (PyO3) bindings with async support

## CLI Usage

### Installation

**Homebrew (macOS):**
```bash
brew tap A3S-Lab/tap
brew install a3s-search
```

**Cargo:**
```bash
cargo install a3s-search
```

### Commands

```bash
# Basic search (uses DuckDuckGo and Wikipedia by default)
a3s-search "Rust programming"

# Search with specific engines
a3s-search "Rust programming" -e ddg,wiki,sogou

# Search with Google (Chrome auto-installed if needed)
a3s-search "Rust programming" -e g,ddg

# Search with Chinese headless engines
a3s-search "Rust 编程" -e baidu,bing_cn

# Limit results
a3s-search "Rust programming" -l 5

# JSON output
a3s-search "Rust programming" -f json

# Compact output (tab-separated)
a3s-search "Rust programming" -f compact

# Use proxy
a3s-search "Rust programming" -p http://127.0.0.1:8080

# SOCKS5 proxy
a3s-search "Rust programming" -p socks5://127.0.0.1:1080

# Verbose mode
a3s-search "Rust programming" -v

# List available engines
a3s-search engines
```

### Available Engines

| Shortcut | Engine | Description |
|----------|--------|-------------|
| `ddg` | DuckDuckGo | Privacy-focused search |
| `brave` | Brave | Brave Search |
| `wiki` | Wikipedia | Wikipedia API |
| `sogou` | Sogou | 搜狗搜索 |
| `360` | 360 Search | 360搜索 |
| `g` | Google | Google Search (Chrome auto-installed) |
| `baidu` | Baidu | 百度搜索 (Chrome auto-installed) |
| `bing_cn` | Bing China | 必应中国 (Chrome auto-installed) |

### Supported Search Engines

#### International Engines

| Engine | Shortcut | Description |
|--------|----------|-------------|
| DuckDuckGo | `ddg` | Privacy-focused search |
| Brave | `brave` | Brave Search |
| Wikipedia | `wiki` | Wikipedia API |
| Google | `g` | Google Search (headless browser) |

#### Chinese Engines (中国搜索引擎)

| Engine | Shortcut | Description |
|--------|----------|-------------|
| Sogou | `sogou` | 搜狗搜索 |
| So360 | `360` | 360搜索 |
| Baidu | `baidu` | 百度搜索 (headless browser) |
| Bing China | `bing_cn` | 必应中国 (headless browser) |

### Automatic Chrome Setup

When using headless engines (`g`, `baidu`, `bing_cn`), Chrome/Chromium is required. A3S Search handles this automatically:

1. **Detect** — Checks `CHROME` env var, PATH commands, and well-known install paths
2. **Cache** — Looks for a previously downloaded Chrome in `~/.a3s/chromium/`
3. **Download** — If not found, downloads [Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/) from Google's official CDN

Supported platforms: **macOS** (arm64, x64) and **Linux** (x64).

```bash
# First run: Chrome is auto-downloaded if not installed
a3s-search "Rust programming" -e g
# Fetching Chrome for Testing version info...
# Downloading Chrome for Testing v145.0.7632.46 (mac-arm64)...
# Downloaded 150.2 MB, extracting...
# Chrome for Testing v145.0.7632.46 installed successfully!

# Subsequent runs: uses cached Chrome instantly
a3s-search "Rust programming" -e g

# Or set CHROME env var to use a specific binary
CHROME=/usr/bin/chromium a3s-search "query" -e g
```

## SDKs

Native bindings for TypeScript and Python, powered by NAPI-RS and PyO3. No subprocess spawning — direct FFI calls to the Rust library.

### TypeScript (Node.js)

```bash
cd sdk/node
npm install && npm run build
```

```typescript
import { A3SSearch } from '@a3s-lab/search';

const search = new A3SSearch();

// Simple search (uses DuckDuckGo + Wikipedia by default)
const response = await search.search('rust programming');

// With options
const response = await search.search('rust programming', {
  engines: ['ddg', 'wiki', 'brave'],
  limit: 5,
  timeout: 15,
  proxy: 'http://127.0.0.1:8080',
});

for (const r of response.results) {
  console.log(`${r.title}: ${r.url} (score: ${r.score})`);
}
console.log(`${response.count} results in ${response.durationMs}ms`);
```

### Python

```bash
cd sdk/python
maturin develop
```

```python
from a3s_search import A3SSearch

search = A3SSearch()

# Simple search (uses DuckDuckGo + Wikipedia by default)
response = await search.search("rust programming")

# With options
response = await search.search("rust programming",
    engines=["ddg", "wiki", "brave"],
    limit=5,
    timeout=15,
    proxy="http://127.0.0.1:8080",
)

for r in response.results:
    print(f"{r.title}: {r.url} (score: {r.score})")
print(f"{response.count} results in {response.duration_ms}ms")
```

### SDK Available Engines

Both SDKs support HTTP-based engines (no headless browser required):

| Shortcut | Aliases | Engine |
|----------|---------|--------|
| `ddg` | `duckduckgo` | DuckDuckGo |
| `brave` | — | Brave Search |
| `wiki` | `wikipedia` | Wikipedia API |
| `sogou` | — | Sogou (搜狗) |
| `360` | `so360` | 360 Search (360搜索) |

### SDK Tests

```bash
# Node.js (49 tests)
cd sdk/node && npm test

# Python (54 tests)
cd sdk/python && pytest
```

## Quality Metrics

### Test Coverage

**298 library + 31 CLI + 103 SDK = 401 total tests** with **91.15% Rust line coverage**:

| Module | Lines | Coverage | Functions | Coverage |
|--------|-------|----------|-----------|----------|
| engine.rs | 116 | 100.00% | 17 | 100.00% |
| error.rs | 52 | 100.00% | 10 | 100.00% |
| query.rs | 114 | 100.00% | 20 | 100.00% |
| result.rs | 194 | 100.00% | 35 | 100.00% |
| aggregator.rs | 292 | 100.00% | 30 | 100.00% |
| search.rs | 337 | 99.41% | 58 | 100.00% |
| proxy.rs | 410 | 99.02% | 91 | 96.70% |
| engines/duckduckgo.rs | 236 | 97.46% | 27 | 81.48% |
| engines/bing_china.rs | 164 | 96.95% | 18 | 77.78% |
| engines/baidu.rs | 146 | 96.58% | 17 | 76.47% |
| engines/google.rs | 180 | 96.11% | 19 | 73.68% |
| engines/brave.rs | 140 | 95.71% | 20 | 75.00% |
| engines/so360.rs | 132 | 95.45% | 18 | 77.78% |
| engines/sogou.rs | 131 | 95.42% | 17 | 76.47% |
| fetcher_http.rs | 29 | 93.10% | 7 | 85.71% |
| fetcher.rs | 73 | 93.15% | 10 | 100.00% |
| engines/wikipedia.rs | 153 | 90.85% | 26 | 88.46% |
| browser.rs | 244 | 68.85% | 42 | 61.90% |
| browser_setup.rs | 406 | 58.13% | 65 | 49.23% |
| **TOTAL** | **3549** | **91.15%** | **547** | **84.10%** |

*Note: `browser.rs` and `browser_setup.rs` have lower coverage because `BrowserPool::acquire_browser()`, `BrowserFetcher::fetch()`, and `download_chrome()` require a running Chrome process or network access. Integration tests verify real browser functionality but are `#[ignore]` by default.*

*SDK tests (49 Node.js + 54 Python = 103 tests) cover error classes, type contracts, input validation, engine validation, and integration with all 5 HTTP engines.*

Run coverage report:
```bash
# Default (19 modules, 267 tests, 91.15% coverage)
just test-cov

# Without headless (14 modules)
just test-cov --no-default-features

# Detailed file-by-file table
just cov-table

# HTML report (opens in browser)
just cov-html
```

### Running Tests

```bash
# Default build (8 engines, 298 tests)
cargo test -p a3s-search --lib

# Without headless (5 engines)
cargo test -p a3s-search --no-default-features --lib

# Integration tests (requires network + Chrome for Google)
cargo test -p a3s-search -- --ignored

# With progress display (via justfile)
just test

# SDK tests (requires native build first)
cd sdk/node && npm test       # 49 tests (vitest)
cd sdk/python && pytest       # 54 tests (pytest)
```

## Architecture

### Ranking Algorithm

The scoring algorithm is based on SearXNG's approach:

```
score = Σ (weight / position) for each engine
weight = engine_weight × num_engines_found
```

**Key factors:**
1. **Engine Weight**: Configurable per-engine multiplier (default: 1.0)
2. **Consensus**: Results found by multiple engines score higher
3. **Position**: Earlier positions in individual engines score higher

### Components

```
┌─────────────────────────────────────────────────────┐
│                     Search                          │
│  ┌───────────────────────────────────────────────┐ │
│  │              Engine Registry                   │ │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐         │ │
│  │  │DuckDuck │ │ Brave   │ │Wikipedia│  ...    │ │
│  │  │  Go     │ │         │ │         │         │ │
│  │  └─────────┘ └─────────┘ └─────────┘         │ │
│  │  ┌─────────────────────────────────┐          │ │
│  │  │ Google (headless browser)       │          │ │
│  │  │   └─ PageFetcher → BrowserPool  │          │ │
│  │  └─────────────────────────────────┘          │ │
│  └───────────────────────────────────────────────┘ │
│                      ↓ parallel search              │
│  ┌───────────────────────────────────────────────┐ │
│  │              Aggregator                        │ │
│  │  • Deduplicate by normalized URL              │ │
│  │  • Merge results from multiple engines        │ │
│  │  • Calculate consensus-based scores           │ │
│  │  • Sort by score (descending)                 │ │
│  └───────────────────────────────────────────────┘ │
│                      ↓                              │
│              SearchResults                          │
└─────────────────────────────────────────────────────┘

PageFetcher (trait)
  ├── HttpFetcher     (reqwest, plain HTTP)
  └── BrowserFetcher  (chromiumoxide, headless Chrome)
        └── BrowserPool (shared process, tab semaphore)
```

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
a3s-search = "0.5"
tokio = { version = "1", features = ["full"] }

# To disable headless browser support:
# a3s-search = { version = "0.5", default-features = false }
```

### Basic Search

```rust
use a3s_search::{Search, SearchQuery, engines::DuckDuckGo};

let mut search = Search::new();
search.add_engine(DuckDuckGo::new());

let query = SearchQuery::new("rust async");
let results = search.search(query).await?;

println!("Found {} results", results.count);
```

### Chinese Search (中文搜索)

```rust
use a3s_search::{Search, SearchQuery, engines::{Sogou, So360}};

let mut search = Search::new();
search.add_engine(Sogou::new());      // 搜狗
search.add_engine(So360::new());      // 360搜索

let query = SearchQuery::new("Rust 编程语言");
let results = search.search(query).await?;
```

### Query Options

```rust
use a3s_search::{SearchQuery, EngineCategory, SafeSearch, TimeRange};

let query = SearchQuery::new("rust tutorial")
    .with_categories(vec![EngineCategory::General])
    .with_language("en-US")
    .with_safesearch(SafeSearch::Moderate)
    .with_page(1)
    .with_time_range(TimeRange::Month);
```

### Custom Engine Weights

```rust
use a3s_search::{Search, EngineConfig, engines::Wikipedia};

// Wikipedia results will have 1.5x weight
let wiki = Wikipedia::new().with_config(EngineConfig {
    name: "Wikipedia".to_string(),
    shortcut: "wiki".to_string(),
    weight: 1.5,
    ..Default::default()
});

let mut search = Search::new();
search.add_engine(wiki);
```

### Using Proxy Pool (Anti-Crawler Protection)

```rust
use a3s_search::{Search, SearchQuery, engines::DuckDuckGo};
use a3s_search::proxy::{ProxyPool, ProxyConfig, ProxyProtocol, ProxyStrategy};

// Create a proxy pool with multiple proxies
let proxy_pool = ProxyPool::with_proxies(vec![
    ProxyConfig::new("proxy1.example.com", 8080),
    ProxyConfig::new("proxy2.example.com", 8080)
        .with_protocol(ProxyProtocol::Socks5),
    ProxyConfig::new("proxy3.example.com", 8080)
        .with_auth("username", "password"),
]).with_strategy(ProxyStrategy::RoundRobin);

let mut search = Search::new();
search.set_proxy_pool(proxy_pool);
search.add_engine(DuckDuckGo::new());

let query = SearchQuery::new("rust programming");
let results = search.search(query).await?;
```

### Dynamic Proxy Provider

```rust
use a3s_search::proxy::{ProxyPool, ProxyConfig, ProxyProvider};
use async_trait::async_trait;
use std::time::Duration;

// Implement custom proxy provider (e.g., from API)
struct MyProxyProvider {
    api_url: String,
}

#[async_trait]
impl ProxyProvider for MyProxyProvider {
    async fn fetch_proxies(&self) -> a3s_search::Result<Vec<ProxyConfig>> {
        // Fetch proxies from your API
        Ok(vec![
            ProxyConfig::new("dynamic-proxy.example.com", 8080),
        ])
    }

    fn refresh_interval(&self) -> Duration {
        Duration::from_secs(60) // Refresh every minute
    }
}

// Use with proxy pool
let provider = MyProxyProvider { api_url: "https://api.example.com/proxies".into() };
let proxy_pool = ProxyPool::with_provider(provider);
proxy_pool.refresh().await?; // Initial fetch
```

### Implementing Custom Engines

```rust
use a3s_search::{Engine, EngineConfig, EngineCategory, SearchQuery, SearchResult, Result};
use async_trait::async_trait;

struct MySearchEngine {
    config: EngineConfig,
}

impl MySearchEngine {
    fn new() -> Self {
        Self {
            config: EngineConfig {
                name: "MyEngine".to_string(),
                shortcut: "my".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.0,
                timeout: 5,
                enabled: true,
                paging: false,
                safesearch: false,
            },
        }
    }
}

#[async_trait]
impl Engine for MySearchEngine {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // Implement your search logic here
        Ok(vec![
            SearchResult::new(
                "https://example.com",
                "Example Result",
                "This is an example search result"
            )
        ])
    }
}
```

## API Reference

### Search

| Method | Description |
|--------|-------------|
| `new()` | Create a new search instance |
| `add_engine(engine)` | Add a search engine |
| `set_timeout(duration)` | Set default search timeout |
| `engine_count()` | Get number of configured engines |
| `search(query)` | Perform a search |
| `set_proxy_pool(pool)` | Set proxy pool for anti-crawler |
| `proxy_pool()` | Get reference to proxy pool |

### SearchQuery

| Method | Description |
|--------|-------------|
| `new(query)` | Create a new query |
| `with_categories(cats)` | Set target categories |
| `with_language(lang)` | Set language/locale |
| `with_safesearch(level)` | Set safe search level |
| `with_page(page)` | Set page number |
| `with_time_range(range)` | Set time range filter |
| `with_engines(engines)` | Limit to specific engines |

### SearchResult

| Field | Type | Description |
|-------|------|-------------|
| `url` | `String` | Result URL |
| `title` | `String` | Result title |
| `content` | `String` | Result snippet |
| `result_type` | `ResultType` | Type of result |
| `engines` | `HashSet<String>` | Engines that found this |
| `positions` | `Vec<u32>` | Positions in each engine |
| `score` | `f64` | Calculated ranking score |
| `thumbnail` | `Option<String>` | Thumbnail URL |
| `published_date` | `Option<String>` | Publication date |

### SearchResults

| Method | Description |
|--------|-------------|
| `items()` | Get result slice |
| `suggestions()` | Get query suggestions |
| `answers()` | Get direct answers |
| `count` | Number of results |
| `duration_ms` | Search duration in ms |

### Engine Trait

```rust
#[async_trait]
pub trait Engine: Send + Sync {
    /// Returns the engine configuration
    fn config(&self) -> &EngineConfig;

    /// Performs a search and returns results
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;

    /// Returns the engine name
    fn name(&self) -> &str { &self.config().name }

    /// Returns the engine shortcut
    fn shortcut(&self) -> &str { &self.config().shortcut }

    /// Returns the engine weight
    fn weight(&self) -> f64 { self.config().weight }

    /// Returns whether the engine is enabled
    fn is_enabled(&self) -> bool { self.config().enabled }
}
```

### EngineConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `String` | - | Display name |
| `shortcut` | `String` | - | Short identifier |
| `categories` | `Vec<EngineCategory>` | `[General]` | Categories |
| `weight` | `f64` | `1.0` | Ranking weight |
| `timeout` | `u64` | `5` | Timeout in seconds |
| `enabled` | `bool` | `true` | Is enabled |
| `paging` | `bool` | `false` | Supports pagination |
| `safesearch` | `bool` | `false` | Supports safe search |

### ProxyPool

| Method | Description |
|--------|-------------|
| `new()` | Create empty proxy pool (disabled) |
| `with_proxies(proxies)` | Create with static proxy list |
| `with_provider(provider)` | Create with dynamic provider |
| `with_strategy(strategy)` | Set selection strategy |
| `set_enabled(bool)` | Enable/disable proxy pool |
| `is_enabled()` | Check if enabled |
| `refresh()` | Refresh proxies from provider |
| `get_proxy()` | Get next proxy (based on strategy) |
| `add_proxy(proxy)` | Add a proxy to pool |
| `remove_proxy(host, port)` | Remove a proxy |
| `create_client(user_agent)` | Create HTTP client with proxy |

### ProxyConfig

| Method | Description |
|--------|-------------|
| `new(host, port)` | Create HTTP proxy config |
| `with_protocol(protocol)` | Set protocol (Http/Https/Socks5) |
| `with_auth(user, pass)` | Set authentication |
| `url()` | Get proxy URL string |

### ProxyStrategy

| Variant | Description |
|---------|-------------|
| `RoundRobin` | Rotate through proxies sequentially |
| `Random` | Select random proxy each time |

## Development

### Dependencies

| Dependency | Install | Purpose |
|------------|---------|---------|
| `cargo-llvm-cov` | `cargo install cargo-llvm-cov` | Code coverage (optional) |
| `lcov` | `brew install lcov` / `apt install lcov` | Coverage report formatting (optional) |
| Chrome/Chromium | Auto-installed | For headless browser engines (auto-downloaded if not found) |

### Build Commands

```bash
# Build (default, 8 engines including headless)
just build

# Build without headless browser support (5 engines)
just build --no-default-features

# Build release
just release

# Test (with colored progress display)
just test                    # All tests with pretty output
just test-raw                # Raw cargo output
just test-v                  # Verbose output (--nocapture)
just test-one TEST           # Run specific test

# Test subsets
just test-engine             # Engine module tests
just test-query              # Query module tests
just test-result             # Result module tests
just test-search             # Search module tests
just test-aggregator         # Aggregator module tests
just test-proxy              # Proxy module tests
just test-error              # Error module tests

# Coverage (requires cargo-llvm-cov)
just test-cov                # Pretty coverage with progress
just cov                     # Terminal coverage report
just cov-html                # HTML report (opens in browser)
just cov-table               # File-by-file table
just cov-ci                  # Generate lcov.info for CI
just cov-module proxy        # Coverage for specific module

# Format & Lint
just fmt                     # Format code
just fmt-check               # Check formatting
just lint                    # Clippy lint
just ci                      # Full CI checks (fmt + lint + test)

# Utilities
just check                   # Fast compile check
just watch                   # Watch and rebuild
just doc                     # Generate and open docs
just clean                   # Clean build artifacts
just update                  # Update dependencies
```

### Project Structure

```
search/
├── Cargo.toml
├── justfile
├── README.md
├── examples/
│   ├── basic_search.rs      # Basic usage example
│   └── chinese_search.rs    # Chinese engines example
├── tests/
│   └── integration.rs       # Integration tests (network-dependent)
├── sdk/
│   ├── node/                # TypeScript SDK (NAPI-RS)
│   │   ├── Cargo.toml       # Rust cdylib crate
│   │   ├── src/             # Rust NAPI bindings
│   │   ├── lib/             # TypeScript wrappers
│   │   ├── tests/           # vitest tests (49 tests)
│   │   └── package.json
│   └── python/              # Python SDK (PyO3)
│       ├── Cargo.toml       # Rust cdylib crate
│       ├── src/             # Rust PyO3 bindings
│       ├── a3s_search/      # Python wrappers
│       ├── tests/           # pytest tests (54 tests)
│       └── pyproject.toml
└── src/
    ├── main.rs              # CLI entry point
    ├── lib.rs               # Library entry point
    ├── engine.rs            # Engine trait and config
    ├── error.rs             # Error types
    ├── query.rs             # SearchQuery
    ├── result.rs            # SearchResult, SearchResults
    ├── aggregator.rs        # Result aggregation and ranking
    ├── search.rs            # Search orchestrator
    ├── proxy.rs             # Proxy pool and configuration
    ├── fetcher.rs           # PageFetcher trait, WaitStrategy
    ├── fetcher_http.rs      # HttpFetcher (reqwest wrapper)
    ├── browser.rs           # BrowserPool, BrowserFetcher (headless browser)
    ├── browser_setup.rs     # Chrome auto-detection and download
    └── engines/
        ├── mod.rs           # Engine exports
        ├── duckduckgo.rs    # DuckDuckGo
        ├── brave.rs         # Brave Search
        ├── google.rs        # Google (headless browser)
        ├── wikipedia.rs     # Wikipedia
        ├── baidu.rs         # Baidu (百度, headless browser)
        ├── bing_china.rs    # Bing China (必应中国, headless browser)
        ├── sogou.rs         # Sogou (搜狗)
        └── so360.rs         # 360 Search (360搜索)
```

## A3S Ecosystem

A3S Search is a **utility component** of the A3S ecosystem.

```
┌──────────────────────────────────────────────────────┐
│                    A3S Ecosystem                     │
│                                                      │
│  Infrastructure:  a3s-box     (MicroVM sandbox)     │
│                      │                               │
│  Application:     a3s-code    (AI coding agent)     │
│                    /   \                             │
│  Utilities:   a3s-lane  a3s-context  a3s-search    │
│               (queue)   (memory)     (search)       │
│                                          ▲          │
│                                          │          │
│                                    You are here     │
└──────────────────────────────────────────────────────┘
```

**Standalone Usage**: `a3s-search` works independently for any meta search needs:
- AI agents needing web search capabilities
- Privacy-focused search aggregation
- Research tools requiring multi-source results
- Any application needing unified search across engines

## Roadmap

### Phase 1: Core ✅ (Complete)

- [x] Engine trait abstraction
- [x] Result deduplication by URL
- [x] Consensus-based ranking algorithm
- [x] Parallel async search execution
- [x] Per-engine timeout handling
- [x] 8 built-in engines (4 international + 4 Chinese)
- [x] Headless browser support for JS-rendered engines (Google, Baidu, Bing China — enabled by default)
- [x] PageFetcher abstraction (HttpFetcher + BrowserFetcher)
- [x] BrowserPool with tab concurrency control
- [x] Proxy pool with dynamic provider support
- [x] CLI tool with Homebrew distribution
- [x] Automatic Chrome detection and download (Chrome for Testing)
- [x] 298 comprehensive unit tests with 91.15% line coverage
- [x] Proxy support for all engines via `-p` flag (HTTP/HTTPS/SOCKS5)
- [x] UTF-8 safe content truncation for CJK/emoji
- [x] Native SDKs: TypeScript (NAPI-RS) and Python (PyO3) with 103 tests

## License

MIT
