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
  <a href="#configuration">Configuration</a> •
  <a href="#engines">Engines</a> •
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
- **ACL Configuration**: Load settings from `.acl` config files
- **Headless Browser**: Chrome and Lightpanda backends for JS-rendered engines
- **Auto-Download**: Automatically detects or downloads browsers
- **Metrics Collection**: Built-in metrics for observability
- **Full-Text Extraction**: Fetch result pages and extract the main article content (Readability algorithm), dropping nav/ads/boilerplate

## Quick Start

### Installation

```toml
[dependencies]
a3s-search = "1.4"
tokio = { version = "1", features = ["full"] }
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `headless` | Chrome/Chromium headless backend (via chromiumoxide) |
| `lightpanda` | Lightpanda headless backend (Linux/macOS, implies `headless`) |

```toml
# Default (no headless engines)
a3s-search = "1.4"

# With headless browsers
a3s-search = { version = "1.4", features = ["headless"] }

# With Lightpanda (Linux/macOS only)
a3s-search = { version = "1.4", features = ["lightpanda"] }
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

### CLI Search

```bash
a3s-search "rust async" --engines ddg,wiki --limit 5
a3s-search "rust async" --page 2 --language en-US --safesearch moderate
a3s-search "rust async" --time-range month --format json
```

JSON output includes `results`, `errors`, `count`, `total_count`, and `duration_ms`,
so callers can inspect partial engine failures without parsing stderr.

## Full-Text Extraction

Search engines return only snippets. `enrich_full_text` fetches each result's page —
through the same `PageFetcher` you already use, so it reuses the proxy pool, headless
browser, and health machinery — and fills `SearchResult::full_text` with the extracted
main article content. Fetch errors and timeouts are skipped, so a result always keeps
its snippet; the pass only ever *adds* information.

```rust
use std::sync::Arc;
use std::time::Duration;
use a3s_search::{Search, SearchQuery, HttpFetcher, PageFetcher, enrich_full_text};
use a3s_search::engines::DuckDuckGo;

let mut search = Search::new();
search.add_engine(DuckDuckGo::new());
let mut results = search.search(SearchQuery::new("rust async")).await?;

// 8 concurrent fetches, 10s timeout each.
let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
enrich_full_text(&mut results, fetcher, 8, Duration::from_secs(10)).await;

for r in results.items() {
    match &r.full_text {
        Some(text) => println!("{} — {} chars of body", r.title, text.len()),
        None => println!("{} — snippet only: {}", r.title, r.content),
    }
}
```

### JavaScript-rendered pages (headless)

Some pages build their article body with client-side JavaScript, which a plain HTTP
fetch never sees. Enable the `headless` feature and swap in a `BrowserFetcher` — it
implements the same `PageFetcher`, so `enrich_full_text` is unchanged. Chrome is
auto-detected or downloaded on first use.

```rust
use std::sync::Arc;
use std::time::Duration;
use a3s_search::browser::{BrowserFetcher, BrowserPool, BrowserPoolConfig};
use a3s_search::{enrich_full_text, PageFetcher};

// Reuse one browser pool across fetches; each fetch opens a tab.
let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
let fetcher: Arc<dyn PageFetcher> = Arc::new(BrowserFetcher::new(pool));

// Lower concurrency than plain HTTP — each tab is heavier.
enrich_full_text(&mut results, fetcher, 3, Duration::from_secs(30)).await;
```

Extraction is CPU-bound and runs on a blocking thread pool regardless of the fetcher used.

## Configuration

### ACL Configuration File

Create a `.acl` configuration file:

```acl
timeout {
  value = 10
}

health {
  max_failures = 3
  suspend_seconds = 60
}

engine "ddg" {
  enabled = true
  weight = 1.0
}

engine "brave" {
  enabled = true
  weight = 1.2
}

engine "google" {
  enabled = true
  weight = 1.5
}
```

Load the configuration:

```rust
use a3s_search::{Engine, Search, SearchConfig};
use a3s_search::engines::DuckDuckGo;

let config = SearchConfig::load("search.acl")?;
let health = config.health_config();
let mut search = Search::with_health_config(health);

let engine = DuckDuckGo::new();
let engine_config = config.apply_engine_config(engine.config().clone());
search.add_engine(engine.with_config(engine_config));
```

The CLI can load the same file:

```bash
a3s-search "rust async" --config search.acl
```

When `--engines` is not provided, enabled `engine "<shortcut>"` blocks decide
which engines are added. `--timeout` overrides configuration at runtime; otherwise
the top-level `timeout` block is applied to each engine, with per-engine
`timeout` taking precedence.

### Configuration Options

#### Timeout Block

```acl
timeout {
  value = 10  # Default timeout in seconds for all engines
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `value` | number | 10 | Default timeout in seconds |

#### Health Block

```acl
health {
  max_failures = 3        # Failures before suspending engine
  suspend_seconds = 60    # How long to suspend after max failures
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_failures` | number | 3 | Consecutive failures before suspension |
| `suspend_seconds` | number | 60 | Suspension duration in seconds |

#### Engine Block

```acl
engine "ddg" {
  enabled = true   # Enable/disable this engine
  weight = 1.0     # Ranking weight (higher = more influence)
  timeout = 15     # Per-engine timeout override (optional)
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | true | Whether the engine is active |
| `weight` | number | 1.0 | Ranking influence multiplier |
| `timeout` | number | inherit | Per-engine timeout override |

## Engines

### Available Engines

| Shortcut | Engine | Type | Categories |
|----------|--------|------|------------|
| `ddg` | DuckDuckGo | HTTP | General |
| `brave` | Brave Search | HTTP | General, News |
| `bing` | Bing International | HTTP | General, Images, Videos, News |
| `wiki` | Wikipedia | HTTP | General |
| `sogou` | 搜狗搜索 | HTTP | General |
| `360` | 360搜索 | HTTP | General |
| `g` | Google Search | Headless | General |
| `baidu` | 百度搜索 | Headless | General |
| `bing_cn` | 必应中国 | Headless | General |

### Using Headless Engines

```rust
use a3s_search::{Search, SearchQuery, BrowserPool, BrowserPoolConfig, BrowserBackend};
use a3s_search::engines::{Google, DuckDuckGo};
use std::sync::Arc;

let mut search = Search::new();

// Create browser pool with Chrome backend
let config = BrowserPoolConfig {
    backend: BrowserBackend::Chrome,
    max_tabs: 4,
    ..Default::default()
};
let pool = Arc::new(BrowserPool::new(config));

// Add engines
search.add_engine(DuckDuckGo::new());
search.add_engine(Google::new(pool));

let results = search.search(SearchQuery::new("rust programming")).await?;
```

## Proxy Pool

### Static Proxy List

```rust
use a3s_search::proxy::{ProxyPool, ProxyConfig, ProxyProtocol};

let proxies = vec![
    ProxyConfig::new("10.0.0.1", 8080).with_protocol(ProxyProtocol::Http),
    ProxyConfig::new("10.0.0.2", 8080).with_protocol(ProxyProtocol::Socks5),
];
let pool = ProxyPool::with_proxies(proxies);
```

### Dynamic Proxy Provider

```rust
use a3s_search::proxy::{ProxyPool, ProxyProvider, ProxyConfig};
use async_trait::async_trait;
use std::sync::Arc;

struct MyProxyProvider { /* ... */ }

#[async_trait]
impl ProxyProvider for MyProxyProvider {
    async fn fetch_proxies(&self) -> a3s_search::Result<Vec<ProxyConfig>> {
        // Fetch from API, database, etc.
        Ok(vec![ProxyConfig::new("10.0.0.1", 8080)])
    }

    fn refresh_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }
}

let pool = Arc::new(ProxyPool::with_provider(MyProxyProvider { /* ... */ }));
let _handle = a3s_search::proxy::spawn_auto_refresh(Arc::clone(&pool));
```

`PooledHttpFetcher` reuses a reqwest client per proxy URL, preserving connection
pools while still rotating proxies for each request:

```rust
use a3s_search::{PageFetcher, PooledHttpFetcher};

let fetcher: Arc<dyn PageFetcher> = Arc::new(PooledHttpFetcher::new(Arc::clone(&pool)));
```

### ProxyPool API

| Method | Description |
|--------|-------------|
| `new()` | Create empty, disabled pool |
| `with_proxies(proxies)` | Create with static proxy list |
| `with_provider(provider)` | Create with dynamic provider |
| `get_proxy()` | Get next proxy (round-robin or random) |
| `add_proxy()` | Add a proxy |
| `remove_proxy()` | Remove a proxy |
| `set_enabled(bool)` | Enable/disable pool |
| `is_enabled()` | Check if enabled |
| `len()` | Number of proxies |
| `refresh()` | Force refresh from provider |
| `create_client()` | Create reqwest Client with proxy |

### ProxyStrategy

| Strategy | Description |
|----------|-------------|
| `RoundRobin` | Cycle through proxies sequentially (default) |
| `Random` | Select random proxy |

## Metrics

Track fetcher and per-engine search performance with built-in metrics:

```rust
use a3s_search::metrics::{Metrics, TimingGuard};
use a3s_search::{HttpFetcher, Search};
use std::sync::Arc;

let metrics = Arc::new(Metrics::new());
let search = Search::new().with_metrics(Arc::clone(&metrics));
let fetcher = HttpFetcher::new().with_metrics(Arc::clone(&metrics));

// Record success
metrics.record_success(std::time::Duration::from_millis(150));

// Record failure
metrics.record_failure("timeout", true);

// Get snapshot
let snapshot = metrics.snapshot().await;
println!("Success rate: {:.1}%", snapshot.success_rate());
println!("P50 latency: {}ms", snapshot.latency_p50_ms);
```

### MetricsSnapshot Fields

| Field | Type | Description |
|-------|------|-------------|
| `successes` | u64 | Total successful requests |
| `failures` | u64 | Total failed requests |
| `transient_failures` | u64 | Transient (retriable) failures |
| `permanent_failures` | u64 | Non-transient failures |
| `error_counts` | HashMap | Error type distribution |
| `latency_p50_ms` | u64 | 50th percentile latency |
| `latency_p95_ms` | u64 | 95th percentile latency |
| `latency_p99_ms` | u64 | 99th percentile latency |

### TimingGuard

RAII guard for measuring request duration:

```rust
let guard = TimingGuard::new(Some(metrics.clone()));

// ... perform operation ...

let elapsed = guard.success(); // Records success with latency
// OR
let elapsed = guard.failure("error_type", is_transient: false); // Records failure
```

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
│  │  Headless Engines: google, baidu, bing_cn      │   │
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
| `Metrics` | In-memory metrics collection |

## API Reference

### Search

```rust
pub struct Search { /* ... */ }

impl Search {
    /// Create a new search instance
    pub fn new() -> Self;

    /// Create with health monitoring
    pub fn with_health_config(config: HealthConfig) -> Self;

    /// Attach metrics for per-engine search attempts
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self;

    /// Set or clear metrics for per-engine search attempts
    pub fn set_metrics(&mut self, metrics: Option<Arc<Metrics>>);

    /// Add a search engine
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E);

    /// Set default search timeout
    pub fn set_timeout(&mut self, timeout: Duration);

    /// Get number of configured engines
    pub fn engine_count(&self) -> usize;

    /// Perform a search
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults>;
}
```

### SearchQuery

```rust
pub struct SearchQuery {
    pub query: String,
    pub categories: Vec<EngineCategory>,
    pub language: Option<String>,
    pub safesearch: SafeSearch,
    pub page: u32,
    pub time_range: Option<TimeRange>,
    pub engines: Vec<String>,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Self;
    pub fn with_categories(mut self, categories: Vec<EngineCategory>) -> Self;
    pub fn with_language(mut self, language: impl Into<String>) -> Self;
    pub fn with_safesearch(mut self, level: SafeSearch) -> Self;
    pub fn with_page(mut self, page: u32) -> Self;
    pub fn with_time_range(mut self, range: TimeRange) -> Self;
    pub fn with_engines(mut self, engines: Vec<String>) -> Self;
}
```

### SafeSearch

```rust
pub enum SafeSearch {
    Off = 0,     // No filtering
    Moderate = 1, // Moderate filtering
    Strict = 2,   // Strict filtering
}
```

### TimeRange

```rust
pub enum TimeRange {
    Day,
    Week,
    Month,
    Year,
}
```

### EngineCategory

```rust
pub enum EngineCategory {
    General,
    Images,
    Videos,
    News,
    Maps,
    Music,
    Files,
    Science,
    Social,
}
```

### PageFetcher

```rust
#[async_trait]
pub trait PageFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<String>;
}
```

### WaitStrategy

```rust
pub enum WaitStrategy {
    Load,  // Wait for page load event (default)
    NetworkIdle { idle_ms: u64 },  // Wait for network idle
    Selector { css: String, timeout_ms: u64 },  // Wait for element
    Delay { ms: u64 },  // Fixed delay after load
}
```

### BrowserPool

```rust
pub struct BrowserPool { /* ... */ }

impl BrowserPool {
    pub fn new(config: BrowserPoolConfig) -> Self;
    pub async fn acquire_browser(&self) -> Result<Arc<Browser>>;
    pub async fn shutdown(&self);
}

pub struct BrowserPoolConfig {
    pub max_tabs: usize,           // Default: 4
    pub headless: bool,            // Default: true
    pub chrome_path: Option<String>,
    pub lightpanda_path: Option<String>,  // (lightpanda feature)
    pub proxy_url: Option<String>,
    pub launch_args: Vec<String>,
    pub backend: BrowserBackend,   // Chrome or Lightpanda
}

pub enum BrowserBackend {
    Chrome,     // Default without lightpanda
    Lightpanda, // Default with lightpanda
}
```

### BrowserFetcher

```rust
pub struct BrowserFetcher { /* ... */ }

impl BrowserFetcher {
    pub fn new(pool: Arc<BrowserPool>) -> Self;
    pub fn with_wait(mut self, wait: WaitStrategy) -> Self;
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self;
    pub fn with_retries(mut self, max_retries: u32, retry_delay_ms: u64) -> Self;
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self;
}

impl PageFetcher for BrowserFetcher {
    async fn fetch(&self, url: &str) -> Result<String>;
}
```

### HealthConfig

```rust
pub struct HealthConfig {
    pub max_failures: u32,           // Default: 3
    pub suspend_duration: Duration,  // Default: 60s
}
```

### ProxyPool

```rust
pub struct ProxyPool { /* ... */ }

impl ProxyPool {
    pub fn new() -> Self;
    pub fn with_proxies(proxies: Vec<ProxyConfig>) -> Self;
    pub fn with_provider<P: ProxyProvider + 'static>(provider: P) -> Self;
    pub fn with_strategy(mut self, strategy: ProxyStrategy) -> Self;
    pub fn set_enabled(&self, enabled: bool);
    pub fn is_enabled(&self) -> bool;
    pub async fn get_proxy(&self) -> Option<ProxyConfig>;
    pub async fn add_proxy(&self, proxy: ProxyConfig);
    pub async fn remove_proxy(&self, host: &str, port: u16);
    pub async fn refresh(&self) -> Result<()>;
    pub async fn len(&self) -> usize;
}

pub fn spawn_auto_refresh(pool: Arc<ProxyPool>) -> tokio::task::JoinHandle<()>;
```

### ProxyConfig

```rust
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub protocol: ProxyProtocol,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self;
    pub fn with_protocol(mut self, protocol: ProxyProtocol) -> Self;
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self;
    pub fn url(&self) -> String;
}
```

### Metrics

```rust
pub struct Metrics { /* ... */ }

impl Metrics {
    pub fn new() -> Self;
    pub fn record_success(&self, latency: Duration);
    pub fn record_failure(&self, error_type: &str, is_transient: bool);
    pub async fn snapshot(&self) -> MetricsSnapshot;
    pub fn total_requests(&self) -> u64;
    pub fn success_rate(&self) -> f64;
    pub async fn reset(&self);
}

pub struct MetricsSnapshot {
    pub successes: u64,
    pub failures: u64,
    pub transient_failures: u64,
    pub permanent_failures: u64,
    pub error_counts: HashMap<String, u64>,
    pub latency_p50_ms: u64,
    pub latency_p95_ms: u64,
    pub latency_p99_ms: u64,
}

pub struct TimingGuard { /* ... */ }

impl TimingGuard {
    pub fn new(metrics: Option<Arc<Metrics>>) -> Self;
    pub fn success(self) -> Duration;
    pub fn failure(self, error_type: &str, is_transient: bool) -> Duration;
}
```

### Engine Trait

```rust
#[async_trait]
pub trait Engine: Send + Sync {
    fn config(&self) -> &EngineConfig;
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;
    fn name(&self) -> &str;
    fn shortcut(&self) -> &str;
    fn weight(&self) -> f64;
    fn is_enabled(&self) -> bool;
}
```

### SearchResults

```rust
pub struct SearchResults { /* ... */ }

impl SearchResults {
    pub fn items(&self) -> &[SearchResult];
    pub fn errors(&self) -> &[(String, String)];
    pub fn suggestions(&self) -> &[String];
    pub fn answers(&self) -> &[String];
    pub fn count(&self) -> usize;
    pub fn duration_ms(&self) -> u64;
}
```

### SearchResult

```rust
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub content: String,
    pub result_type: ResultType,
    pub engines: Vec<String>,
    pub score: f64,
    pub thumbnail: Option<String>,
    pub published_date: Option<String>,
}
```

## Development

### Build Commands

```bash
# Build default
cargo build -p a3s-search

# Build with headless support
cargo build -p a3s-search --features headless

# Run tests
cargo test -p a3s-search --lib

# Format
cargo fmt -p a3s-search

# Clippy
cargo clippy -p a3s-search --no-default-features -- -D warnings
```

### Release

Releases are published to GitHub Releases with CLI binaries for multiple platforms.

```bash
# Create and push tag to trigger release
git tag v1.2.0
git push origin v1.2.0
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
