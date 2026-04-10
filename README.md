# A3S Search

<p align="center">
  <strong>Embeddable Meta Search Engine</strong>
</p>

<p align="center">
  <em>Aggregate results from multiple engines with ranking and deduplication</em>
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

**A3S Search** is an embeddable meta search engine library. It aggregates results from multiple search engines, deduplicates them, and ranks them using a consensus-based scoring algorithm.

### Basic Usage

```rust
use a3s_search::{Search, SearchQuery, engines::{DuckDuckGo, Wikipedia}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut search = Search::new();
    search.add_engine(DuckDuckGo::new());
    search.add_engine(Wikipedia::new());

    let query = SearchQuery::new("rust programming");
    let results = search.search(query).await?;

    for result in results.items().iter().take(10) {
        println!("{}: {}", result.title, result.url);
    }

    Ok(())
}
```

## Features

- **Multi-Engine Search**: Aggregate results from multiple engines in parallel
- **9 Built-in Engines**: DuckDuckGo, Brave, Bing, Wikipedia, Sogou, 360, Google, Baidu, Bing China
- **Result Deduplication**: Merge duplicate results based on normalized URLs
- **Consensus Ranking**: Results found by multiple engines rank higher
- **Async-First**: Built on Tokio for high-performance concurrent searches
- **Timeout Handling**: Per-engine timeout with graceful degradation
- **Extensible**: Add custom engines via the `Engine` trait
- **Dynamic Proxy Pool**: IP rotation with pluggable `ProxyProvider` trait
- **Health Monitor**: Automatic engine suspension after repeated failures
- **HCL Configuration**: Load settings from `.hcl` config files
- **Headless Browser**: Chrome and Lightpanda backends for JS-rendered engines
- **Auto-Download**: Automatically detects or downloads browsers
- **Native SDKs**: TypeScript (NAPI) and Python (PyO3) bindings

## Quick Start

### Installation

```toml
[dependencies]
a3s-search = "0.9"
tokio = { version = "1", features = ["full"] }
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `chromiumoxide` | Shared CDP client (required by chromium/lightpanda) |
| `chromium` | Chrome/Chromium headless backend |
| `lightpanda` | Lightpanda headless backend (Linux/macOS) |
| `all-headless` | Enable both chromium and lightpanda |

```toml
# Default (no headless engines)
a3s-search = "0.9"

# With Chrome/Chromium backend
a3s-search = { version = "0.9", features = ["chromium"] }

# With Lightpanda backend
a3s-search = { version = "0.9", features = ["lightpanda"] }

# With both backends
a3s-search = { version = "0.9", features = ["all-headless"] }
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

### Headless Browser Mode

Enable headless browser for JS-rendered engines (Google, Baidu, Bing China):

```rust
use a3s_search::{Search, SearchQuery};
use a3s_search::browser::{BrowserBackend, BrowserPool, BrowserPoolConfig};
use a3s_search::engines::{Google, DuckDuckGo};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Enable headless mode with Chrome backend
    let mut search = Search::new();
    let pool = search.enable_headless(BrowserBackend::Chrome).await?;

    // Add engines
    search.add_engine(DuckDuckGo::new());
    search.add_engine(Google::new(pool.acquire_browser().await?));

    let results = search.search(SearchQuery::new("rust programming")).await?;
    println!("Found {} results", results.count);

    Ok(())
}
```

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
# Basic search
a3s-search "rust programming"

# With specific engines
a3s-search "rust programming" -e ddg,wiki

# Limit results
a3s-search "rust programming" -l 5

# JSON output
a3s-search "rust programming" -f json

# Use proxy
a3s-search "rust programming" -p http://127.0.0.1:8080

# List available engines
a3s-search engines
```

### Available Engines

| Shortcut | Engine | Type |
|----------|--------|------|
| `ddg` | DuckDuckGo | HTTP |
| `brave` | Brave Search | HTTP |
| `bing` | Bing International | HTTP |
| `wiki` | Wikipedia | HTTP |
| `sogou` | 搜狗搜索 | HTTP |
| `360` | 360搜索 | HTTP |
| `g` | Google Search | Headless |
| `baidu` | 百度搜索 | Headless |
| `bing_cn` | 必应中国 | Headless |

## SDKs

Native bindings for TypeScript and Python, powered by NAPI-RS and PyO3.

### TypeScript (Node.js)

```bash
npm install @a3s-lab/search
```

```typescript
import { A3SSearch } from '@a3s-lab/search';

const search = new A3SSearch();

// Simple search
const response = await search.search('rust programming');

// With options
const response = await search.search('rust programming', {
  engines: ['ddg', 'wiki', 'brave', 'bing'],
  limit: 5,
  timeout: 15,
  proxy: 'http://127.0.0.1:8080',
});

for (const r of response.results) {
  console.log(`${r.title}: ${r.url}`);
}
```

### Python

```bash
pip install a3s-search
```

```python
from a3s_search import A3SSearch

search = A3SSearch()

# Simple search
response = search.search("rust programming")

# With options
response = search.search("rust programming",
    engines=["ddg", "wiki", "brave", "bing"],
    limit=5,
    timeout=15,
    proxy="http://127.0.0.1:8080",
)

for r in response.results:
    print(f"{r.title}: {r.url}")
```

### SDK Types

Both SDKs expose headless browser configuration types:

**TypeScript:**
```typescript
type BrowserBackend = "Chrome" | "Lightpanda"

interface HeadlessConfig {
  backend: BrowserBackend
  browserPath?: string
  maxTabs?: number
  launchArgs?: string[]
}

interface SearchConfig {
  timeout: number
  engines: Record<string, EngineConfig>
  headless?: HeadlessConfig
}
```

**Python:**
```python
class BrowserBackend:
    Chrome
    Lightpanda

class HeadlessConfig:
    def __init__(self, backend: BrowserBackend, browser_path: str = None,
                 max_tabs: int = None, launch_args: list = None)

class SearchConfig:
    def __init__(self, timeout: int, engines: dict = None,
                 headless: HeadlessConfig = None)
```

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────┐
│                      A3S Search                      │
├─────────────────────────────────────────────────────┤
│  ┌─────────┐    ┌─────────┐    ┌─────────┐        │
│  │ Rust    │    │ Python  │    │ Node.js │        │
│  │ Core    │◄───┤ SDK     │    │ SDK     │        │
│  └────┬────┘    └─────────┘    └─────────┘        │
│       │                                             │
│       ▼                                             │
│  ┌─────────────────────────────────────────────┐   │
│  │              Search Orchestrator              │   │
│  │  • Parallel execution (tokio::join_all)      │   │
│  │  • Timeout handling                           │   │
│  │  • Health monitoring                          │   │
│  └─────────────────────────────────────────────┘   │
│       │                                             │
│       ▼                                             │
│  ┌─────────────────────────────────────────────┐   │
│  │                 Engine Layer                  │   │
│  │  HTTP Engines: ddg, brave, bing, wiki, ...   │   │
│  │  Headless Engines: google, baidu, bing_cn     │   │
│  └─────────────────────────────────────────────┘   │
│       │                                             │
│       ▼                                             │
│  ┌─────────────────────────────────────────────┐   │
│  │               PageFetcher Layer               │   │
│  │  HttpFetcher │ PooledHttpFetcher │ Browser    │   │
│  └─────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

### Core Components

| Component | Description |
|-----------|-------------|
| `Search` | Main orchestrator for parallel engine execution |
| `Engine` trait | Abstract interface for search engines |
| `PageFetcher` trait | Abstract interface for page fetching |
| `Aggregator` | URL deduplication and consensus ranking |
| `BrowserPool` | Shared headless browser process management |
| `ProxyPool` | Proxy rotation with auto-refresh |

## API Reference

### Search

```rust
pub struct Search { /* ... */ }

impl Search {
    /// Create a new search instance
    pub fn new() -> Self;

    /// Create with health monitoring
    pub fn with_health_config(config: HealthConfig) -> Self;

    /// Add a search engine
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E);

    /// Set default search timeout
    pub fn set_timeout(&mut self, timeout: Duration);

    /// Enable headless browser mode
    pub async fn enable_headless(&mut self, backend: BrowserBackend) -> Result<Arc<BrowserPool>>;

    /// Perform a search
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults>;
}
```

### BrowserBackend

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserBackend {
    /// Chrome/Chromium headless
    Chrome,
    /// Lightpanda headless browser (Linux/macOS only)
    Lightpanda,
}

impl Default for BrowserBackend {
    fn default() -> Self {
        #[cfg(feature = "lightpanda")]
        { Self::Lightpanda }
        #[cfg(not(feature = "lightpanda"))]
        { Self::Chrome }
    }
}
```

### HeadlessConfig

```rust
pub struct HeadlessConfig {
    /// Which headless backend to use
    pub backend: BrowserBackend,
    /// Path to browser executable (auto-detected if None)
    pub browser_path: Option<String>,
    /// Maximum concurrent tabs (default: 4)
    pub max_tabs: usize,
    /// Additional launch arguments
    pub launch_args: Vec<String>,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            backend: BrowserBackend::default(),
            browser_path: None,
            max_tabs: 4,
            launch_args: Vec::new(),
        }
    }
}
```

### SearchConfig (HCL)

```hcl
timeout = 10

health {
  max_failures    = 5
  suspend_seconds = 120
}

headless {
  backend     = "Chrome"  # or "Lightpanda"
  browser_path = null     # auto-detect
  max_tabs    = 4
  launch_args = []
}

engine "ddg" {
  enabled = true
  weight  = 1.0
}

engine "google" {
  enabled = true
  weight  = 1.0
}
```

### Engine Trait

```rust
#[async_trait]
pub trait Engine: Send + Sync {
    fn config(&self) -> &EngineConfig;
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;
}
```

## Development

### Build Commands

```bash
# Build default (all features)
cargo build -p a3s-search

# Build without headless
cargo build -p a3s-search --no-default-features

# Run tests
cargo test -p a3s-search --lib

# Format
cargo fmt -p a3s-search

# Clippy
cargo clippy -p a3s-search --no-default-features -- -D warnings
```

### Release

Releases are published to GitHub Releases with:
- CLI binaries (darwin-arm64, darwin-x86_64, linux-arm64, linux-x86_64)
- Python wheels (Python 3.9-3.13, many platforms)
- Node.js bindings (.node for multiple platforms)

```bash
# Create and push tag to trigger release
git tag v0.9.0
git push origin v0.9.0
```

## A3S Ecosystem

A3S Search is part of the A3S ecosystem:

```
a3s-box      - MicroVM sandbox
a3s-code     - AI coding agent
a3s-lane     - Queue
a3s-memory   - Memory
a3s-search   - Search
```

## License

MIT
