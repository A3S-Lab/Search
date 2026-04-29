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
  <a href="#headless-browser">Headless Browser</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#api-reference">API Reference</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Search** is an embeddable meta search engine library. It aggregates results from multiple search engines, deduplicates them, and ranks them using a consensus-based scoring algorithm.

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
- **Headless Browser**: Obscura backend for JS-rendered engines (Google, Baidu, Bing China)
- **Auto-Download**: Automatically detects or downloads obscura binary

## Quick Start

### Installation

```toml
[dependencies]
a3s-search = "1.1"
tokio = { version = "1", features = ["full"] }
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

### Using Proxy

```rust
use a3s_search::{Search, SearchQuery, PooledHttpFetcher, ProxyPool};
use std::sync::Arc;

let pool = Arc::new(ProxyPool::new());
let fetcher = Arc::new(PooledHttpFetcher::new(Arc::clone(&pool)));
let mut search = Search::new();
search.add_engine(DuckDuckGo::with_fetcher(DuckDuckGoParser, fetcher));
```

## Headless Browser

JavaScript-rendered engines (Google, Baidu, Bing China) require a headless browser. This library uses **obscura**, a lightweight Rust-native headless browser.

### Feature Flags

| Feature | Description |
|---------|-------------|
| `obscura` (default) | Obscura headless backend (Linux/macOS, x86_64/aarch64) |

### Setup

Obscura binary is auto-detected in order:
1. `OBSCURA` environment variable
2. `obscura` in PATH
3. Cached download in `~/.a3s/obscura/`
4. Downloaded from GitHub releases

### Usage with Obscura

```rust
use a3s_search::{Search, SearchQuery, ObscuraPool, ObscuraPoolConfig, ObscuraFetcher};
use a3s_search::engines::{Google, Baidu, BingChina};
use std::sync::Arc;

let pool_config = ObscuraPoolConfig::default();
let pool = Arc::new(ObscuraPool::new(pool_config));

// Google with selector wait strategy
let fetcher = ObscuraFetcher::new(Arc::clone(&pool))
    .with_wait(WaitStrategy::Selector {
        css: "div.g".to_string(),
        timeout_ms: 5000,
    });
let google = Google::new(Arc::new(fetcher));

let mut search = Search::new();
search.add_engine(google);
```

### CLI Usage

```bash
# Basic search (HTTP engines)
a3s-search "rust programming" -e ddg,wiki

# With headless engines (auto-installs obscura)
a3s-search "rust programming" -e g,baidu

# Use proxy
a3s-search "rust programming" -p http://127.0.0.1:8080

# List available engines
a3s-search engines
```

### Available Engines

| Shortcut | Engine | Type | Notes |
|----------|--------|------|-------|
| `ddg` | DuckDuckGo | HTTP | |
| `brave` | Brave Search | HTTP | |
| `bing` | Bing International | HTTP | |
| `wiki` | Wikipedia | HTTP | |
| `sogou` | 搜狗搜索 | HTTP | |
| `360` | 360搜索 | HTTP | |
| `g` | Google Search | Headless | Requires obscura |
| `baidu` | 百度搜索 | Headless | Requires obscura |
| `bing_cn` | 必应中国 | Headless | Requires obscura |

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────┐
│                      A3S Search                      │
├─────────────────────────────────────────────────────┤
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
│  │  HttpFetcher │ PooledHttpFetcher │ Obscura    │   │
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
| `ObscuraPool` | Shared obscura subprocess with tab concurrency |
| `ProxyPool` | Proxy rotation with auto-refresh |
| `HealthMonitor` | Tracks engine failures and suspensions |

## API Reference

### Search

```rust
pub struct Search { /* ... */ }

impl Search {
    pub fn new() -> Self;
    pub fn with_health_config(config: HealthConfig) -> Self;
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E);
    pub fn set_timeout(&mut self, timeout: Duration);
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults>;
}
```

### ObscuraPoolConfig

```rust
pub struct ObscuraPoolConfig {
    /// Maximum number of concurrent browser tabs (default: 4)
    pub max_tabs: usize,
    /// Path to obscura executable (auto-detected if None)
    pub obscura_path: Option<String>,
    /// Proxy URL for the browser (optional)
    pub proxy_url: Option<String>,
}

impl Default for ObscuraPoolConfig {
    fn default() -> Self {
        Self {
            max_tabs: 4,
            obscura_path: None,
            proxy_url: None,
        }
    }
}
```

### WaitStrategy

```rust
pub enum WaitStrategy {
    /// Wait for page load event (default)
    Load,
    /// Wait for load + idle milliseconds
    NetworkIdle { idle_ms: u64 },
    /// Wait for CSS selector to appear
    Selector { css: String, timeout_ms: u64 },
    /// Wait for load + fixed delay
    Delay { ms: u64 },
}
```

### SearchConfig (HCL)

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

engine "g" {
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
# Build
cargo build -p a3s-search

# Run tests
cargo test -p a3s-search --lib

# Format
cargo fmt -p a3s-search

# Clippy
cargo clippy -p a3s-search --no-default-features -- -D warnings
```

### Release

Releases are published to GitHub Releases with CLI binaries:
- darwin-arm64
- darwin-x86_64
- linux-arm64
- linux-x86_64

Create and push a tag to trigger release:

```bash
git tag v1.1.0
git push origin v1.1.0
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
