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
    for result in results.items().take(10) {
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
- **CLI Tool**: Command-line interface for quick searches

## CLI Usage

### Installation

```bash
cargo install a3s-search
```

### Commands

```bash
# Basic search (uses DuckDuckGo and Wikipedia by default)
a3s-search "Rust programming"

# Search with specific engines
a3s-search "Rust programming" -e ddg,wiki,baidu

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
| `google` | Google | Google Search |
| `wiki` | Wikipedia | Wikipedia API |
| `baidu` | Baidu | 百度搜索 |
| `sogou` | Sogou | 搜狗搜索 |
| `bing_cn` | Bing China | 必应中国 |
| `360` | 360 Search | 360搜索 |

### Supported Search Engines

#### International Engines

| Engine | Shortcut | Description |
|--------|----------|-------------|
| DuckDuckGo | `ddg` | Privacy-focused search |
| Brave | `brave` | Brave Search |
| Google | `g` | Google Search |
| Wikipedia | `wiki` | Wikipedia API |

#### Chinese Engines (中国搜索引擎)

| Engine | Shortcut | Description |
|--------|----------|-------------|
| Baidu | `baidu` | 百度搜索 |
| Sogou | `sogou` | 搜狗搜索 |
| BingChina | `bing_cn` | 必应中国 |
| So360 | `360` | 360搜索 |

## Quality Metrics

### Test Coverage

**188 unit tests** (167 library + 21 CLI) + **22 integration tests** with comprehensive coverage:

| Module | Lines | Coverage | Functions | Coverage |
|--------|-------|----------|-----------|----------|
| aggregator.rs | 239 | 99.16% | 25 | 100.00% |
| engine.rs | 119 | 100.00% | 18 | 100.00% |
| error.rs | 29 | 100.00% | 7 | 100.00% |
| query.rs | 113 | 100.00% | 20 | 100.00% |
| result.rs | 193 | 100.00% | 36 | 100.00% |
| search.rs | 313 | 99.36% | 58 | 100.00% |
| proxy.rs | 417 | 98.32% | 91 | 97.80% |
| duckduckgo.rs | 138 | 80.43% | 20 | 70.00% |
| wikipedia.rs | 114 | 87.72% | 20 | 85.00% |
| baidu.rs | 99 | 71.72% | 15 | 66.67% |
| sogou.rs | 95 | 70.53% | 15 | 66.67% |
| bing_china.rs | 95 | 70.53% | 15 | 66.67% |
| so360.rs | 95 | 70.53% | 15 | 66.67% |
| brave.rs | 108 | 62.96% | 18 | 55.56% |
| google.rs | 109 | 63.30% | 18 | 55.56% |
| **Total** | **2276** | **89.19%** | **391** | **87.72%** |

*Note: Engine implementations require HTTP requests for full coverage. Integration tests (in `tests/integration.rs`) verify real HTTP functionality but are `#[ignore]` by default.*

Run coverage report:
```bash
cargo llvm-cov -p a3s-search --lib --summary-only
```

### Running Tests

```bash
# Run unit tests
cargo test -p a3s-search

# Run with output
cargo test -p a3s-search -- --nocapture

# Run integration tests (requires network)
cargo test -p a3s-search -- --ignored
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
```

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
a3s-search = "0.1"
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

### Chinese Search (中文搜索)

```rust
use a3s_search::{Search, SearchQuery, engines::{Baidu, Sogou, BingChina, So360}};

let mut search = Search::new();
search.add_engine(Baidu::new());      // 百度
search.add_engine(Sogou::new());      // 搜狗
search.add_engine(BingChina::new());  // 必应中国
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
| `cargo-llvm-cov` | `cargo install cargo-llvm-cov` | Code coverage |

### Build Commands

```bash
# Build
cargo build -p a3s-search

# Test
cargo test -p a3s-search

# Test with output
cargo test -p a3s-search -- --nocapture

# Coverage
cargo llvm-cov -p a3s-search --lib --summary-only

# Run examples
cargo run -p a3s-search --example basic_search
cargo run -p a3s-search --example chinese_search

# Run CLI
cargo run -p a3s-search --bin a3s-search -- search "query"
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
    └── engines/
        ├── mod.rs           # Engine exports
        ├── duckduckgo.rs    # DuckDuckGo
        ├── brave.rs         # Brave Search
        ├── google.rs        # Google
        ├── wikipedia.rs     # Wikipedia
        ├── baidu.rs         # Baidu (百度)
        ├── sogou.rs         # Sogou (搜狗)
        ├── bing_china.rs    # Bing China (必应中国)
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

### Phase 2: Enhanced Features 🚧 (Planned)

- [ ] Image search support
- [ ] News search support
- [ ] Result caching
- [ ] Engine health monitoring
- [ ] Automatic engine suspension on failures
- [ ] More engines (Yandex, Qwant, etc.)

### Phase 3: Advanced 📋 (Future)

- [ ] Instant answers (calculator, weather, etc.)
- [ ] Infobox extraction
- [ ] Search suggestions
- [ ] Spelling corrections
- [ ] Plugin system

## License

MIT
