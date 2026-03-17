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
- **Dynamic Proxy Pool**: IP rotation with pluggable `ProxyProvider` trait and auto-refresh
- **Health Monitor**: Automatic engine suspension after repeated failures with configurable recovery
- **HCL Configuration**: Load engine and health settings from HCL config files
- **Headless Browser**: Optional Chrome/Chromium integration for JS-rendered engines (feature-gated)
- **Auto-Install Chrome**: Automatically detects or downloads Chrome for Testing when no browser is found
- **PageFetcher Abstraction**: Pluggable page fetching — `HttpFetcher`, `PooledHttpFetcher`, or `BrowserFetcher`
- **CLI Tool**: Command-line interface for quick searches
- **Native SDKs**: TypeScript (NAPI) and Python (PyO3) bindings with async support and dynamic proxy pool management

## CLI Usage

### Installation

**Homebrew (macOS):**
```bash
brew tap a3s-lab/tap https://github.com/A3S-Lab/homebrew-tap
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
| `bing` | Bing | Bing International |
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
| Bing | `bing` | Bing International |
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

Supported platforms: **macOS** (arm64, x64), **Linux** (x64), and **Windows** (x64, x86).

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
  engines: ['ddg', 'wiki', 'brave', 'bing'],
  limit: 5,
  timeout: 15,
  proxy: 'http://127.0.0.1:8080',
});

// Dynamic proxy pool (IP rotation)
await search.setProxyPool([
  'http://10.0.0.1:8080',
  'http://10.0.0.2:8080',
  'socks5://10.0.0.3:1080',
]);
const response = await search.search('rust programming');

// Toggle proxy pool at runtime
search.setProxyPoolEnabled(false);  // direct connection
search.setProxyPoolEnabled(true);   // re-enable rotation

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
    engines=["ddg", "wiki", "brave", "bing"],
    limit=5,
    timeout=15,
    proxy="http://127.0.0.1:8080",
)

# Dynamic proxy pool (IP rotation)
await search.set_proxy_pool([
    "http://10.0.0.1:8080",
    "http://10.0.0.2:8080",
    "socks5://10.0.0.3:1080",
])
response = await search.search("rust programming")

# Toggle proxy pool at runtime
search.set_proxy_pool_enabled(False)  # direct connection
search.set_proxy_pool_enabled(True)   # re-enable rotation

for r in response.results:
    print(f"{r.title}: {r.url} (score: {r.score})")
print(f"{response.count} results in {response.duration_ms}ms")
```

### SDK Available Engines

Both SDKs support all engines (HTTP and headless):

| Shortcut | Aliases | Engine | Type |
|----------|---------|--------|------|
| `ddg` | `duckduckgo` | DuckDuckGo | HTTP |
| `brave` | — | Brave Search | HTTP |
| `bing` | — | Bing International | HTTP |
| `wiki` | `wikipedia` | Wikipedia API | HTTP |
| `sogou` | — | Sogou (搜狗) | HTTP |
| `360` | `so360` | 360 Search (360搜索) | HTTP |
| `g` | `google` | Google Search | Headless |
| `baidu` | — | Baidu (百度) | Headless |
| `bing_cn` | — | Bing China (必应中国) | Headless |

### Chrome Setup for SDK

Headless engines require Chrome. Pre-download it after install:

**Python:**
```bash
# Option 1: CLI command (added to PATH on install)
a3s-search-setup

# Option 2: Python module
python -m a3s_search.ensure_chrome

# Option 3: In code (async)
from a3s_search import ensure_chrome
path = await ensure_chrome()
```

**Node.js:**
```bash
# Runs automatically on npm install via postinstall script
# Or manually:
node -e "require('@a3s-lab/search').ensureChrome().then(console.log)"
```

```typescript
// In code
import { ensureChrome } from '@a3s-lab/search';
const path = await ensureChrome();
console.log(`Chrome at: ${path}`);
```

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
# Default build (9 engines, 244+ lib tests)
cargo test -p a3s-search --lib

# Without headless (6 engines)
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

### System Overview

A3S Search is a **meta search engine** that aggregates results from multiple search engines, deduplicates them, and ranks them using a consensus-based algorithm. It supports both HTTP-based engines and JavaScript-rendered engines via headless browsers.

```
┌─────────────────────────────────────────────────────────────────┐
│                        A3S Search System                        │
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐    │
│  │   Rust API   │    │  Python SDK  │    │  Node.js SDK │    │
│  │   (Core)     │◄───┤   (PyO3)     │    │   (NAPI-RS)  │    │
│  └──────┬───────┘    └──────────────┘    └──────────────┘    │
│         │                                                       │
│         ▼                                                       │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                    Search Orchestrator                   │  │
│  │  • Query parsing & validation                           │  │
│  │  • Engine selection & filtering                         │  │
│  │  • Parallel execution (tokio::join_all)                 │  │
│  │  • Timeout handling (per-engine)                        │  │
│  │  • Health monitoring (auto-suspend failed engines)      │  │
│  └─────────────────────────────────────────────────────────┘  │
│         │                                                       │
│         ▼                                                       │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                    Engine Layer                          │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │  │
│  │  │ HTTP Engines │  │   Headless   │  │   Custom     │  │  │
│  │  │              │  │   Engines    │  │   Engines    │  │  │
│  │  │ • DuckDuckGo │  │ • Google     │  │ • User-      │  │  │
│  │  │ • Brave      │  │ • Baidu      │  │   defined    │  │  │
│  │  │ • Bing       │  │ • BingChina  │  │   (trait)    │  │  │
│  │  │ • Wikipedia  │  │              │  │              │  │  │
│  │  │ • Sogou      │  │              │  │              │  │  │
│  │  │ • 360        │  │              │  │              │  │  │
│  │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │  │
│  └─────────┼──────────────────┼──────────────────┼─────────┘  │
│            │                  │                  │             │
│            ▼                  ▼                  ▼             │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                  PageFetcher Layer                       │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │  │
│  │  │ HttpFetcher  │  │PooledHttp    │  │Browser       │  │  │
│  │  │              │  │Fetcher       │  │Fetcher       │  │  │
│  │  │ • reqwest    │  │ • ProxyPool  │  │ • Lightpanda │  │  │
│  │  │ • single     │  │ • Round-robin│  │ • Chrome     │  │  │
│  │  │   proxy      │  │ • IP rotation│  │ • CDP        │  │  │
│  │  └──────────────┘  └──────────────┘  └──────┬───────┘  │  │
│  └─────────────────────────────────────────────┼─────────┘  │
│                                                 │             │
│                                                 ▼             │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                  Browser Pool                            │  │
│  │  • Shared browser process (Lightpanda/Chrome)           │  │
│  │  • Tab concurrency control (semaphore)                  │  │
│  │  • Auto-download & cache (~/.a3s/)                      │  │
│  │  • CDP connection management                            │  │
│  └─────────────────────────────────────────────────────────┘  │
│         │                                                       │
│         ▼                                                       │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                    Aggregator                            │  │
│  │  • URL normalization & deduplication                    │  │
│  │  • Consensus-based scoring                              │  │
│  │  • Result merging & ranking                             │  │
│  │  • Suggestions & answers extraction                     │  │
│  └─────────────────────────────────────────────────────────┘  │
│         │                                                       │
│         ▼                                                       │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │                  SearchResults                           │  │
│  │  • Ranked results (by score)                            │  │
│  │  • Engine attribution                                   │  │
│  │  • Metadata (duration, count, errors)                   │  │
│  └─────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Core Components

#### 1. Search Orchestrator
- **Query Processing**: Parses and validates search queries
- **Engine Selection**: Filters engines based on query requirements
- **Parallel Execution**: Executes all engines concurrently using `tokio::join_all`
- **Timeout Management**: Per-engine timeout with graceful degradation
- **Health Monitoring**: Tracks engine failures and auto-suspends unhealthy engines

#### 2. Engine Layer
- **HTTP Engines**: Direct HTTP requests (DuckDuckGo, Brave, Bing, Wikipedia, Sogou, 360)
- **Headless Engines**: JavaScript rendering via browser (Google, Baidu, BingChina)
- **Custom Engines**: User-defined engines via `Engine` trait

#### 3. PageFetcher Layer
- **HttpFetcher**: Simple HTTP client with optional proxy
- **PooledHttpFetcher**: Proxy pool with round-robin IP rotation
- **BrowserFetcher**: Headless browser rendering (Lightpanda/Chrome)

#### 4. Browser Pool
- **Shared Process**: Single browser instance shared across all headless engines
- **Tab Concurrency**: Semaphore-based tab limit (default: 4)
- **Auto-Setup**: Automatic browser detection and download
- **CDP Protocol**: Chrome DevTools Protocol for page control

#### 5. Aggregator
- **Deduplication**: Normalizes URLs and merges duplicate results
- **Scoring**: Consensus-based ranking algorithm
- **Merging**: Combines results from multiple engines
- **Extraction**: Pulls out suggestions and instant answers

### Search Workflow

```
┌─────────────────────────────────────────────────────────────────┐
│                      Search Execution Flow                      │
└─────────────────────────────────────────────────────────────────┘

1. User Query
   │
   ├─► "rust programming"
   │   engines: ["ddg", "brave", "google"]
   │   limit: 10
   │   timeout: 15s
   │
   ▼
2. Query Validation & Parsing
   │
   ├─► SearchQuery {
   │     query: "rust programming",
   │     categories: [General],
   │     language: "en",
   │     safesearch: Moderate,
   │     page: 1
   │   }
   │
   ▼
3. Engine Selection & Filtering
   │
   ├─► Selected Engines:
   │   • DuckDuckGo (HTTP, weight: 1.0)
   │   • Brave (HTTP, weight: 1.0)
   │   • Google (Headless, weight: 1.0)
   │
   ▼
4. Parallel Engine Execution (tokio::join_all)
   │
   ├─────────────────┬─────────────────┬─────────────────┐
   │                 │                 │                 │
   ▼                 ▼                 ▼                 ▼
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│DuckDuckGo│    │  Brave   │    │  Google  │    │  Health  │
│          │    │          │    │          │    │ Monitor  │
│ HTTP GET │    │ HTTP GET │    │ Browser  │    │          │
│  ↓       │    │  ↓       │    │  Render  │    │ Track    │
│ Parse    │    │ Parse    │    │  ↓       │    │ Failures │
│ HTML     │    │ HTML     │    │ Parse    │    │          │
│  ↓       │    │  ↓       │    │ HTML     │    │          │
│ Results  │    │ Results  │    │  ↓       │    │          │
│ [10]     │    │ [10]     │    │ Results  │    │          │
│          │    │          │    │ [10]     │    │          │
└────┬─────┘    └────┬─────┘    └────┬─────┘    └──────────┘
     │               │               │
     │               │               │ (timeout: 15s each)
     │               │               │
     └───────────────┴───────────────┘
                     │
                     ▼
5. Result Aggregation
   │
   ├─► Collect all results (30 total)
   │   • DuckDuckGo: 10 results
   │   • Brave: 10 results
   │   • Google: 10 results
   │
   ▼
6. URL Normalization & Deduplication
   │
   ├─► Normalize URLs:
   │   • Remove tracking params
   │   • Lowercase domain
   │   • Remove www prefix
   │   • Normalize path
   │
   ├─► Merge duplicates:
   │   • Same URL from multiple engines
   │   • Combine engine lists
   │   • Merge positions
   │
   ├─► Result: 18 unique results
   │
   ▼
7. Consensus-Based Scoring
   │
   ├─► For each result:
   │   score = Σ (weight / position) for each engine
   │   weight = engine_weight × num_engines_found
   │
   ├─► Example:
   │   Result A found by DuckDuckGo (#1) and Brave (#2):
   │   score = (1.0 × 2 / 1) + (1.0 × 2 / 2) = 2.0 + 1.0 = 3.0
   │
   │   Result B found only by Google (#1):
   │   score = (1.0 × 1 / 1) = 1.0
   │
   │   → Result A ranks higher (consensus bonus)
   │
   ▼
8. Sorting & Limiting
   │
   ├─► Sort by score (descending)
   ├─► Apply limit (10 results)
   │
   ▼
9. SearchResults
   │
   └─► {
         results: [10 ranked results],
         count: 10,
         duration_ms: 1234,
         errors: [],
         suggestions: ["rust tutorial", "rust book"],
         answers: []
       }
```

### Headless Browser Workflow

```
┌─────────────────────────────────────────────────────────────────┐
│              Headless Engine Execution (Google)                 │
└─────────────────────────────────────────────────────────────────┘

1. Engine Initialization
   │
   ├─► Check if browser needed
   │   • Engine: Google (requires JS rendering)
   │   • Browser: Lightpanda (default) or Chrome
   │
   ▼
2. Browser Detection & Setup
   │
   ├─► Lightpanda (default):
   │   ├─ Check LIGHTPANDA env var
   │   ├─ Check PATH for lightpanda command
   │   ├─ Check cache: ~/.a3s/lightpanda/<tag>/
   │   └─ If not found: Download from GitHub releases
   │       ├─ Platform: macOS arm64
   │       ├─ URL: github.com/lightpanda-io/browser/releases
   │       ├─ Download: lightpanda-darwin-aarch64.tar.gz
   │       ├─ Extract to: ~/.a3s/lightpanda/nightly/
   │       └─ Set executable: chmod +x lightpanda
   │
   ├─► Chrome (fallback, browser="chrome"):
   │   ├─ Check CHROME env var
   │   ├─ Check PATH for chrome/chromium commands
   │   ├─ Check known paths (/Applications/Google Chrome.app, etc.)
   │   ├─ Check cache: ~/.a3s/chromium/<version>/
   │   └─ If not found: Download Chrome for Testing
   │       ├─ Platform: mac-arm64
   │       ├─ URL: googlechromelabs.github.io/chrome-for-testing
   │       ├─ Download: chrome-mac-arm64.zip (150MB)
   │       ├─ Extract to: ~/.a3s/chromium/131.0.6778.85/
   │       └─ Return: chrome executable path
   │
   ▼
3. Browser Pool Initialization
   │
   ├─► BrowserPool::new(config)
   │   • backend: Lightpanda (default)
   │   • max_tabs: 4
   │   • headless: true
   │   • proxy_url: None
   │
   ▼
4. Browser Launch (Lazy, on first request)
   │
   ├─► Lightpanda:
   │   ├─ Find free port (OS-assigned)
   │   ├─ Spawn: lightpanda serve --host 127.0.0.1 --port <port>
   │   ├─ Wait for CDP server ready (TCP connect)
   │   └─ Connect via WebSocket: ws://127.0.0.1:<port>
   │
   ├─► Chrome:
   │   ├─ Launch with args:
   │   │   --headless=new
   │   │   --disable-gpu
   │   │   --no-sandbox
   │   │   --disable-blink-features=AutomationControlled
   │   │   --user-agent=<realistic UA>
   │   └─ Connect via CDP (chromiumoxide)
   │
   ▼
5. Page Rendering
   │
   ├─► Acquire tab permit (semaphore, max 4 concurrent)
   │
   ├─► Create new tab
   │   • browser.new_page(url)
   │
   ├─► Navigate to search URL
   │   • https://www.google.com/search?q=rust+programming
   │
   ├─► Wait for page load
   │   • Strategy: Load (default)
   │   • Alternatives: NetworkIdle, Selector, Delay
   │
   ├─► Extract rendered HTML
   │   • page.content() → full HTML string
   │
   ├─► Close tab
   │   • page.close()
   │
   └─► Release tab permit
   │
   ▼
6. HTML Parsing
   │
   ├─► Parse with scraper (HTML5 parser)
   │
   ├─► Extract results:
   │   • Selector: div.g (Google result container)
   │   • Title: h3
   │   • URL: a[href]
   │   • Snippet: div.VwiC3b
   │
   ├─► Handle errors:
   │   • CAPTCHA detection
   │   • Rate limiting
   │   • Empty results
   │
   ▼
7. Return Results
   │
   └─► Vec<SearchResult> [10 results]
```

### Ranking Algorithm

The scoring algorithm is based on SearXNG's approach with consensus weighting:

```
score = Σ (weight / position) for each engine
weight = engine_weight × num_engines_found
```

**Key factors:**
1. **Engine Weight**: Configurable per-engine multiplier (default: 1.0)
2. **Consensus**: Results found by multiple engines score higher
3. **Position**: Earlier positions in individual engines score higher

**Example Calculation:**

```
Query: "rust programming"
Engines: DuckDuckGo (weight: 1.0), Brave (weight: 1.0), Google (weight: 1.0)

Result A: "The Rust Programming Language"
  • Found by DuckDuckGo at position 1
  • Found by Brave at position 1
  • Found by Google at position 2
  • num_engines_found = 3

  score = (1.0 × 3 / 1) + (1.0 × 3 / 1) + (1.0 × 3 / 2)
        = 3.0 + 3.0 + 1.5
        = 7.5

Result B: "Rust Tutorial"
  • Found by DuckDuckGo at position 3
  • Found by Google at position 5
  • num_engines_found = 2

  score = (1.0 × 2 / 3) + (1.0 × 2 / 5)
        = 0.67 + 0.4
        = 1.07

→ Result A ranks higher (consensus + better positions)
```

### Proxy Pool Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Proxy Pool System                          │
└─────────────────────────────────────────────────────────────────┘

1. ProxyPool (Arc<ProxyPool>)
   │
   ├─► Configuration:
   │   • Strategy: RoundRobin | Random
   │   • Enabled: AtomicBool (thread-safe toggle)
   │   • Proxies: RwLock<Vec<ProxyConfig>>
   │
   ├─► Static Mode:
   │   ProxyPool::with_proxies([
   │     "http://10.0.0.1:8080",
   │     "socks5://10.0.0.2:1080",
   │   ])
   │
   ├─► Dynamic Mode:
   │   ProxyPool::with_provider(MyProxyProvider)
   │   • Implements ProxyProvider trait
   │   • fetch_proxies() → Vec<ProxyConfig>
   │   • refresh_interval() → Duration
   │
   └─► Auto-Refresh:
       spawn_auto_refresh(pool)
       • Background task
       • Periodic refresh
       • Updates pool atomically
   │
   ▼
2. PooledHttpFetcher
   │
   ├─► Per-Request Rotation:
   │   • get_proxy() → ProxyConfig
   │   • create_client(proxy) → reqwest::Client
   │   • fetch(url) → HTML
   │
   ├─► Strategy: RoundRobin
   │   Request 1 → Proxy A
   │   Request 2 → Proxy B
   │   Request 3 → Proxy C
   │   Request 4 → Proxy A (cycle)
   │
   └─► Strategy: Random
       Request 1 → Proxy B
       Request 2 → Proxy A
       Request 3 → Proxy B
       Request 4 → Proxy C
   │
   ▼
3. Runtime Control
   │
   ├─► Enable/Disable (thread-safe):
   │   pool.set_enabled(false)  // Direct connection
   │   pool.set_enabled(true)   // Resume rotation
   │
   └─► No restart required
       • AtomicBool check on each request
       • Instant toggle
```

### Health Monitoring

```
┌─────────────────────────────────────────────────────────────────┐
│                    Health Monitor System                        │
└─────────────────────────────────────────────────────────────────┘

1. Configuration
   │
   ├─► HealthConfig {
   │     max_failures: 5,        // Suspend after 5 consecutive failures
   │     suspend_duration: 120s  // Suspend for 2 minutes
   │   }
   │
   ▼
2. Failure Tracking (per engine)
   │
   ├─► Success: Reset counter to 0
   │
   ├─► Failure: Increment counter
   │   • Network error
   │   • Timeout
   │   • Parse error
   │   • CAPTCHA
   │
   ├─► Threshold Reached:
   │   failures >= max_failures
   │   → Suspend engine
   │   → Record suspend_until timestamp
   │
   ▼
3. Engine Suspension
   │
   ├─► Suspended Engine:
   │   • Skipped in search execution
   │   • Not counted in results
   │   • Logged as suspended
   │
   ├─► Auto-Recovery:
   │   • Check suspend_until on each search
   │   • If current_time > suspend_until:
   │     → Re-enable engine
   │     → Reset failure counter
   │
   ▼
4. Example Timeline
   │
   ├─► T=0s:  Google search succeeds (failures: 0)
   ├─► T=10s: Google search fails (failures: 1)
   ├─► T=20s: Google search fails (failures: 2)
   ├─► T=30s: Google search fails (failures: 3)
   ├─► T=40s: Google search fails (failures: 4)
   ├─► T=50s: Google search fails (failures: 5)
   │           → SUSPENDED until T=170s
   ├─► T=60s: Google skipped (suspended)
   ├─► T=170s: Google re-enabled (auto-recovery)
   └─► T=180s: Google search succeeds (failures: 0)
```

### Component Diagram

```
PageFetcher (trait)
  ├── HttpFetcher        (reqwest, plain HTTP, single proxy)
  ├── PooledHttpFetcher  (reqwest, proxy pool rotation)
  └── BrowserFetcher     (chromiumoxide, headless browser)
        └── BrowserPool (shared process, tab semaphore)
              ├── Lightpanda (default, 59MB, <100ms startup)
              └── Chrome (fallback, 200MB, 1-2s startup)
```

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
a3s-search = "0.8"
tokio = { version = "1", features = ["full"] }

# To disable headless browser support:
# a3s-search = { version = "0.8", default-features = false }
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
use std::sync::Arc;
use a3s_search::{Search, SearchQuery, PooledHttpFetcher, PageFetcher};
use a3s_search::engines::{DuckDuckGo, DuckDuckGoParser};
use a3s_search::proxy::{ProxyPool, ProxyConfig, ProxyProtocol, ProxyStrategy};

// Create a proxy pool with multiple proxies
let pool = Arc::new(ProxyPool::with_proxies(vec![
    ProxyConfig::new("proxy1.example.com", 8080),
    ProxyConfig::new("proxy2.example.com", 8080)
        .with_protocol(ProxyProtocol::Socks5),
    ProxyConfig::new("proxy3.example.com", 8080)
        .with_auth("username", "password"),
]).with_strategy(ProxyStrategy::RoundRobin));

// PooledHttpFetcher rotates proxies per request
let fetcher: Arc<dyn PageFetcher> = Arc::new(PooledHttpFetcher::new(Arc::clone(&pool)));

let mut search = Search::new();
search.add_engine(DuckDuckGo::with_fetcher(DuckDuckGoParser, fetcher));

let query = SearchQuery::new("rust programming");
let results = search.search(query).await?;

// Toggle proxy pool at runtime (thread-safe via AtomicBool)
pool.set_enabled(false);  // direct connection
pool.set_enabled(true);   // re-enable rotation
```

### Dynamic Proxy Provider

```rust
use std::sync::Arc;
use a3s_search::proxy::{ProxyPool, ProxyConfig, ProxyProvider, spawn_auto_refresh};
use async_trait::async_trait;
use std::time::Duration;

// Implement custom proxy provider (e.g., from API, Redis, database)
struct MyProxyProvider {
    api_url: String,
}

#[async_trait]
impl ProxyProvider for MyProxyProvider {
    async fn fetch_proxies(&self) -> a3s_search::Result<Vec<ProxyConfig>> {
        // Fetch proxies from your API — format is up to you
        Ok(vec![
            ProxyConfig::new("dynamic-proxy.example.com", 8080),
        ])
    }

    fn refresh_interval(&self) -> Duration {
        Duration::from_secs(60) // Refresh every minute
    }
}

// Use with auto-refresh background task
let pool = Arc::new(ProxyPool::with_provider(
    MyProxyProvider { api_url: "https://api.example.com/proxies".into() }
));
let _refresh_handle = spawn_auto_refresh(Arc::clone(&pool));
// Pool now auto-refreshes every 60 seconds
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
| `with_health_config(config)` | Create with health monitoring |
| `add_engine(engine)` | Add a search engine |
| `set_timeout(duration)` | Set default search timeout |
| `engine_count()` | Get number of configured engines |
| `search(query)` | Perform a search |

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
| `set_enabled(bool)` | Enable/disable proxy pool (thread-safe, `&self`) |
| `is_enabled()` | Check if enabled |
| `refresh()` | Refresh proxies from provider |
| `get_proxy()` | Get next proxy (based on strategy) |
| `add_proxy(proxy)` | Add a proxy to pool |
| `remove_proxy(host, port)` | Remove a proxy |
| `len()` | Number of proxies in pool |
| `create_client(user_agent)` | Create HTTP client with proxy |

### PooledHttpFetcher

| Method | Description |
|--------|-------------|
| `new(pool)` | Create with `Arc<ProxyPool>` — rotates proxy per request |
| `with_timeout(duration)` | Set request timeout (default: 30s) |

### spawn_auto_refresh

```rust
pub fn spawn_auto_refresh(pool: Arc<ProxyPool>) -> tokio::task::JoinHandle<()>
```

Spawns a background task that periodically calls `pool.refresh()` based on the provider's `refresh_interval()`. Returns a handle that can be aborted to stop refreshing.

### HealthMonitor / HealthConfig

| Field/Method | Description |
|--------|-------------|
| `HealthConfig { max_failures, suspend_duration }` | Configure failure threshold and suspension time |
| `Search::with_health_config(config)` | Create search with health monitoring |

Engines are automatically suspended after `max_failures` consecutive failures and re-enabled after `suspend_duration`.

### SearchConfig (HCL)

| Method | Description |
|--------|-------------|
| `SearchConfig::load(path)` | Load config from `.hcl` file |
| `SearchConfig::parse(content)` | Parse HCL string |
| `health_config()` | Get `HealthConfig` from config |
| `enabled_engines()` | Get list of enabled engine shortcuts |

Example HCL config:
```hcl
timeout = 10

health {
  max_failures    = 5
  suspend_seconds = 120
}

engine "ddg" {
  enabled = true
  weight  = 1.0
}

engine "bing" {
  enabled = true
  weight  = 1.2
}
```

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

### Releasing

See [RELEASE.md](RELEASE.md) for detailed release instructions.

**Quick release:**
```bash
# Check GitHub secrets are configured
./scripts/check-secrets.sh

# Release new version (runs tests, commits, tags, pushes)
./scripts/release.sh 0.9.0

# Monitor CI/CD progress
gh run watch --repo A3S-Lab/Search
```

The release workflow automatically:
- ✅ Runs CI checks (fmt, clippy, tests)
- 📦 Publishes to crates.io
- 🐍 Publishes Python SDK to PyPI (7 platforms)
- 📦 Publishes Node.js SDK to npm (7 platforms)
- 🍺 Updates Homebrew formula
- 🎉 Creates GitHub Release with CLI binaries

### Project Structure

```
search/
├── Cargo.toml
├── justfile
├── README.md
├── RELEASE.md               # Release guide
├── .github/
│   ├── setup-workspace.sh   # CI workspace restructuring
│   └── workflows/
│       ├── ci.yml           # Push/PR checks
│       ├── release.yml      # Tag-triggered release
│       ├── publish-node.yml # Node SDK publishing
│       └── publish-python.yml # Python SDK publishing
├── scripts/
│   ├── release.sh           # Automated release script
│   └── check-secrets.sh     # Check GitHub secrets
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
    ├── search.rs            # Search orchestrator with HealthMonitor
    ├── config.rs            # HCL configuration loading
    ├── health.rs            # HealthMonitor, HealthConfig
    ├── proxy.rs             # ProxyPool, ProxyProvider, spawn_auto_refresh
    ├── fetcher.rs           # PageFetcher trait, WaitStrategy
    ├── fetcher_http.rs      # HttpFetcher + PooledHttpFetcher
    ├── html_engine.rs       # HtmlEngine<P> generic engine framework
    ├── browser.rs           # BrowserPool, BrowserFetcher (headless browser)
    ├── browser_setup.rs     # Chrome auto-detection and download
    └── engines/
        ├── mod.rs           # Engine exports
        ├── duckduckgo.rs    # DuckDuckGo
        ├── brave.rs         # Brave Search
        ├── bing.rs          # Bing International
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
- [x] 9 built-in engines (5 international + 4 Chinese)
- [x] Bing International engine (HTTP, no headless required)
- [x] Headless browser support for JS-rendered engines (Google, Baidu, Bing China — enabled by default)
- [x] PageFetcher abstraction (HttpFetcher + PooledHttpFetcher + BrowserFetcher)
- [x] BrowserPool with tab concurrency control
- [x] Dynamic proxy pool with pluggable `ProxyProvider` trait and `spawn_auto_refresh`
- [x] `PooledHttpFetcher` for per-request proxy IP rotation
- [x] Runtime proxy pool toggle via `AtomicBool` (`set_enabled(&self)`)
- [x] Health monitoring with automatic engine suspension and recovery
- [x] HCL configuration file loading for engines and health settings
- [x] CLI tool with Homebrew distribution
- [x] Automatic Chrome detection and download (Chrome for Testing)
- [x] Proxy support for all engines via `-p` flag (HTTP/HTTPS/SOCKS5)
- [x] UTF-8 safe content truncation for CJK/emoji
- [x] Native SDKs: TypeScript (NAPI-RS) and Python (PyO3) with dynamic proxy pool management
- [x] SDK proxy pool: `setProxyPool()`, `setProxyPoolEnabled()`, per-request `proxyPool` option

### Phase 2: SDK Headless Support ✅ (v0.8.0)

- [x] Enable headless feature in Python and Node SDKs (all 9 engines available)
- [x] `ensure_chrome()` / `ensure_chrome_sync()` bindings for Python and Node SDKs
- [x] Python post-install: `a3s-search-setup` CLI + `python -m a3s_search.ensure_chrome`
- [x] Node post-install: automatic Chrome download on `npm install`

## Community

Join us on [Discord](https://discord.gg/XVg6Hu6H) for questions, discussions, and updates.

## License

MIT
