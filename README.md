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
- **PageFetcher Abstraction**: Pluggable page fetching (plain HTTP or headless browser)
- **CLI Tool**: Command-line interface for quick searches

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

# Search with Google (requires headless feature and Chrome installed)
a3s-search "Rust programming" -e g,ddg --headless

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
| `g` | Google | Google Search (requires `headless` feature + `--headless` flag) |

### Supported Search Engines

#### International Engines

| Engine | Shortcut | Description |
|--------|----------|-------------|
| DuckDuckGo | `ddg` | Privacy-focused search |
| Brave | `brave` | Brave Search |
| Wikipedia | `wiki` | Wikipedia API |
| Google | `g` | Google Search (headless browser, `headless` feature) |

#### Chinese Engines (中国搜索引擎)

| Engine | Shortcut | Description |
|--------|----------|-------------|
| Sogou | `sogou` | 搜狗搜索 |
| So360 | `360` | 360搜索 |

## Quality Metrics

### Test Coverage

**207 comprehensive unit tests** (183 library + 24 CLI) with **94.07% line coverage**:

| Module | Lines | Coverage | Functions | Coverage |
|--------|-------|----------|-----------|----------|
| engine.rs | 116 | 100.00% | 17 | 100.00% |
| error.rs | 34 | 100.00% | 8 | 100.00% |
| query.rs | 114 | 100.00% | 20 | 100.00% |
| result.rs | 194 | 100.00% | 35 | 100.00% |
| search.rs | 337 | 99.41% | 58 | 100.00% |
| aggregator.rs | 241 | 99.17% | 24 | 100.00% |
| proxy.rs | 410 | 99.02% | 91 | 96.70% |
| engines/google.rs | 180 | 96.11% | 19 | 73.68% |
| engines/brave.rs | 140 | 95.71% | 20 | 75.00% |
| engines/so360.rs | 132 | 95.45% | 18 | 77.78% |
| engines/sogou.rs | 131 | 95.42% | 17 | 76.47% |
| fetcher_http.rs | 29 | 93.10% | 7 | 85.71% |
| fetcher.rs | 40 | 92.50% | 6 | 100.00% |
| engines/wikipedia.rs | 114 | 87.72% | 20 | 85.00% |
| engines/duckduckgo.rs | 132 | 86.36% | 20 | 70.00% |
| browser.rs | 167 | 52.69% | 31 | 41.94% |
| **TOTAL** | **2511** | **94.07%** | **411** | **88.08%** |

*Note: `browser.rs` has lower coverage because `BrowserPool::acquire_browser()` and `BrowserFetcher::fetch()` require a running Chrome process. Integration tests (in `tests/integration.rs`) verify real browser functionality but are `#[ignore]` by default.*

Run coverage report:
```bash
LLVM_COV="$(rustup run stable rustc --print sysroot)/lib/rustlib/$(rustc -vV | grep host | cut -d' ' -f2)/bin/llvm-cov" \
LLVM_PROFDATA="$(rustup run stable rustc --print sysroot)/lib/rustlib/$(rustc -vV | grep host | cut -d' ' -f2)/bin/llvm-profdata" \
cargo llvm-cov -p a3s-search --features headless --lib --summary-only
```

### Running Tests

```bash
# Default build (5 engines, 192 tests)
cargo test -p a3s-search

# With headless feature (6 engines, 207 tests)
cargo test -p a3s-search --features headless

# Integration tests (requires network + Chrome for Google)
cargo test -p a3s-search --features headless -- --ignored
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
│  │  │ Google (headless feature)       │          │ │
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
a3s-search = "0.3"
tokio = { version = "1", features = ["full"] }

# Optional: enable headless browser support for Google engine
# a3s-search = { version = "0.3", features = ["headless"] }
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
| Chrome/Chromium | System package manager | Required for `headless` feature |

### Build Commands

```bash
# Build (default, 5 engines)
cargo build -p a3s-search

# Build with headless browser support (6 engines, includes Google)
cargo build -p a3s-search --features headless

# Test (default)
cargo test -p a3s-search

# Test with headless feature
cargo test -p a3s-search --features headless

# Test with output
cargo test -p a3s-search -- --nocapture

# Run examples
cargo run -p a3s-search --example basic_search
cargo run -p a3s-search --example chinese_search

# Run CLI
cargo run -p a3s-search -- "query"

# Run CLI with Google (headless)
cargo run -p a3s-search --features headless -- "query" -e g --headless
```

### Project Structure

```
search/
├── Cargo.toml
├── README.md
├── examples/
│   ├── basic_search.rs      # Basic usage example
│   └── chinese_search.rs    # Chinese engines example
├── tests/
│   └── integration.rs       # Integration tests (network-dependent)
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
    ├── browser.rs           # BrowserPool, BrowserFetcher (headless feature)
    └── engines/
        ├── mod.rs           # Engine exports
        ├── duckduckgo.rs    # DuckDuckGo
        ├── brave.rs         # Brave Search
        ├── google.rs        # Google (headless feature)
        ├── wikipedia.rs     # Wikipedia
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
- [x] 6 built-in engines (4 international + 2 Chinese)
- [x] Headless browser support for JS-rendered engines (Google via `headless` feature)
- [x] PageFetcher abstraction (HttpFetcher + BrowserFetcher)
- [x] BrowserPool with tab concurrency control

### Phase 2: Enhanced Features 🚧 (Planned)

- [ ] Image search support
- [ ] News search support
- [ ] Result caching
- [ ] Engine health monitoring
- [ ] Automatic engine suspension on failures
- [ ] More headless engines (Baidu, Bing China, Yandex via `headless` feature)
- [ ] More plain-HTTP engines (Qwant, etc.)

### Phase 3: Advanced 📋 (Future)

- [ ] Instant answers (calculator, weather, etc.)
- [ ] Infobox extraction
- [ ] Search suggestions
- [ ] Spelling corrections
- [ ] Plugin system
- [ ] **ML-based Ranking**: Learning-to-rank model for result quality
  - [ ] Feature extraction (engine agreement, position, freshness, domain authority)
  - [ ] Lightweight ranking model (logistic regression / small neural net)
  - [ ] A/B testing framework for ranking experiments
  - [ ] Feedback loop: click-through rate → ranking improvement
- [ ] **OpenTelemetry Integration**:
  - [ ] Span: `a3s.search.query` with attributes: query, engines, result_count, latency_ms
  - [ ] Per-engine spans: `a3s.search.engine.{name}` with timeout/error tracking
  - [ ] Metrics: `a3s_search_engine_latency_seconds{engine}` histogram
  - [ ] Metrics: `a3s_search_engine_errors_total{engine}` counter

## License

MIT
