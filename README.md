# A3S Search

Extensible web search for Rust and the command line.

`a3s-search` runs conventional search engines and native third-party search
providers in parallel, merges duplicate URLs, ranks independent evidence, and
preserves provider-native answers, relevance, full text, images, usage, and
request reports.

## Highlights

- Native [AnySearch](https://www.anysearch.com/) MCP/JSON-RPC integration
- Native [Tavily](https://www.tavily.com/) Search API integration
- Anonymous AnySearch and keyless Tavily access, with optional bearer authentication
- Provider-neutral `SearchProvider` protocol for downstream integrations
- Parallel execution with isolated timeouts and partial-failure results
- Deterministic URL deduplication, rich-field merging, duplicate evidence
  suppression, and consensus ranking
- Typed ACL configuration with redacted credential sources
- Conventional HTTP, RSS, and optional A3S Use Browser engines
- Query answers, suggestions, images, full text, favicons, relevance, usage, and reports
- Bounded provider responses and sanitized provider errors
- Bundled Codex Skill in every release archive

## Install

Install the CLI:

```bash
cargo install a3s-search
# or
brew install A3S-Lab/tap/a3s-search
```

Add the library:

```toml
[dependencies]
a3s-search = "2"
tokio = { version = "1", features = ["full"] }
```

Optional features:

| Feature | Purpose |
| --- | --- |
| `headless` | Enable A3S Use Browser rendering for Google, Baidu, and JavaScript pages |
| `lightpanda` | Add the Lightpanda backend; implies `headless` |

Provider APIs do not require either browser feature.

## CLI quick start

List engines and provider readiness:

```bash
a3s-search engines
```

Search both native providers without an API key:

```bash
a3s-search "Rust async runtime guidance" \
  --engines anysearch,tavily \
  --format json \
  --limit 10
```

Authenticate through the environment when higher quotas or authenticated
features are needed:

```bash
export ANYSEARCH_API_KEY="..."
export TAVILY_API_KEY="..."
export TAVILY_PROJECT="..." # optional; authenticated Tavily requests only

a3s-search "Rust async runtime guidance" \
  --engines anysearch,tavily \
  --format json
```

Other useful controls:

```bash
a3s-search "query" --engines ddg,wiki,anysearch,tavily
a3s-search "query" --language en-US --time-range month
a3s-search "query" --safesearch moderate --timeout 20
a3s-search "query" --format compact
```

`--limit` limits displayed results. Provider-side result limits belong in ACL.

## Native providers

| Provider | Native protocol | Credential-free mode | Default result limit | Rich evidence |
| --- | --- | --- | --- | --- |
| AnySearch | MCP over JSON-RPC 2.0 | Anonymous | 10, range `1..=10` | Full text, total count, timing, request ID |
| Tavily | Tavily Search REST API | `X-Tavily-Access-Mode: keyless` | 5, range `0..=20` | Answers, relevance, raw content, images, favicon, usage, metadata |

### AnySearch

The built-in provider sends `tools/call` requests for the AnySearch `search`
tool to `POST https://api.anysearch.com/mcp`. It prefers structured content and
supports the official Markdown result fallback. `ANYSEARCH_API_KEY` is optional;
when present it is sent as a bearer token.

This integration follows the
[AnySearch Skill v2.1.0](https://github.com/anysearch-ai/anysearch-skill/tree/v2.1.0)
MCP contract linked from AnySearch's Skill download. AnySearch also documents a
separate `/v1/search` REST surface; that REST request schema is not the Skill
protocol and is intentionally not mixed into this provider.

The one-query `SearchProvider` contract implements the Skill's `search`
operation. Workflow operations such as `get_sub_domains`, `batch_search`, and
`extract` remain in the official AnySearch Skill: sub-domain discovery is not
guessed by this library, batch orchestration belongs to the caller, and page
extraction is available through `enrich_full_text`. Obtain a documented
sub-domain and its required parameters before configuring vertical routing.

AnySearch vertical routing supports:

```text
general, resource, social_media, finance, academic, legal, health,
business, security, ip, code, energy, environment, agriculture,
travel, film, gaming
```

When using a sub-domain, use the `{domain}.{sub_domain}` form and keep its
prefix equal to `domain`.

### Tavily

The built-in provider sends typed requests to
`POST https://api.tavily.com/search`. Without `TAVILY_API_KEY`, it uses Tavily's
documented keyless header. With a key, it uses bearer authentication and sends
`TAVILY_PROJECT` as `X-Project-ID` only when that project value is configured.

Supported controls include search depth, topic, direct answers, raw content,
domain lists, calendar-valid date bounds, country boost, automatic parameters,
exact matching, images, image descriptions, favicons, usage, and safe search.
Tavily safe search requires authenticated enterprise access and `basic` or
`advanced` depth. Defaults follow Tavily's API contract, including
`include_usage = false`; enable usage explicitly in ACL or with
`TavilyConfig::with_include_usage(true)` when credit evidence is needed.

## ACL configuration

Use `.acl` files for provider-specific controls and credential references.
This example shows every provider attribute:

```acl
timeout {
  value = 20
}

health {
  max_failures = 3
  suspend_seconds = 60
}

provider "anysearch" {
  enabled = true
  weight = 1.0
  timeout = 20
  endpoint = "https://api.anysearch.com/mcp"
  api_key = env("ANYSEARCH_API_KEY")
  http_timeout = 30
  max_response_bytes = 2097152

  max_results = 10
  domain = "code"
  sub_domain = "code.doc"
  sub_domain_params = {
    library = "tokio"
    filters = {
      stable = true
    }
  }
}

provider "tavily" {
  enabled = true
  weight = 1.2
  timeout = 20
  endpoint = "https://api.tavily.com/search"
  api_key = env("TAVILY_API_KEY")
  project = env("TAVILY_PROJECT")
  http_timeout = 30
  max_response_bytes = 2097152

  search_depth = "advanced"
  chunks_per_source = 3
  max_results = 10
  topic = "general"
  include_answer = "advanced"
  include_raw_content = "markdown"
  include_domains = ["docs.rs", "rust-lang.org"]
  exclude_domains = ["example.com"]
  start_date = "2026-01-01"
  end_date = "2026-07-20"
  country = "united states"
  auto_parameters = true
  exact_match = false
  include_usage = true
  include_images = true
  include_image_descriptions = true
  include_favicon = true
  safe_search = false
}

engine "ddg" {
  enabled = true
  weight = 0.8
  timeout = 10
}
```

Run with the configuration:

```bash
a3s-search --config search.acl engines
a3s-search "query" --config search.acl --format json
```

Configuration rules:

- Prefer `env("VARIABLE")`; credential debug output is redacted.
- Provider readiness resolves credential sources and verifies that API-key and
  authenticated Tavily project values can be represented safely as headers.
- Use `api_key = null` to force AnySearch anonymous or Tavily keyless mode.
- Never put credentials in endpoint URLs.
- Provider endpoints must use HTTPS; loopback HTTP is accepted for tests.
- AnySearch `sub_domain_params` are resource-bounded before serialization:
  nesting, collection sizes, node count, and text volume must remain within
  the provider adapter's defensive limits.
- Integral numeric settings are range-checked without saturation. Fractional,
  negative, out-of-range, zero timeout, zero weight, and duplicate engine
  configurations are rejected instead of silently coerced.
- `chunks_per_source` requires Tavily `search_depth = "advanced"`.
- With Tavily `auto_parameters = true`, omit `search_depth` and `topic` to let
  Tavily select them. Explicit values override automatic selection as documented.
  `chunks_per_source`, `country`, or safe search pin the compatible field needed
  for those controls. Reports expose the actual documented depth and topic
  selected by Tavily when returned in `auto_parameters`; an unpinned value is
  omitted from the report if Tavily does not disclose its selection.
- Tavily follows the official `include_usage = false` default; set it to
  `true` when reports should include consumed credits.
- `include_image_descriptions` requires `include_images = true`.
- `country` applies only to Tavily's `general` topic.
- Tavily domain filters accept bare DNS names only and normalize international
  names to their ASCII representation.
- An include-domain and exclude-domain list cannot contain the same domain.
- If `--engines` is absent, enabled ACL engines and providers are selected.
- Explicit `--engines` selection still respects `enabled = false`.
- `--timeout` overrides configured orchestration timeouts.

## Structured evidence and partial failures

Use `--format json` for machine-readable evidence. The top-level object contains:

| Field | Meaning |
| --- | --- |
| `results` | Ranked, deduplicated results up to the CLI display limit |
| `answers` | Provider-supplied direct answers |
| `suggestions` | Provider or engine query suggestions |
| `images` | Query-level images |
| `reports` | Provider request IDs, total counts, timings, usage, and bounded metadata |
| `errors` | Per-engine failure entries |
| `count` | Number of displayed results |
| `total_count` | Number of aggregated results before the display limit |
| `duration_ms` | End-to-end orchestration duration |

Each result can preserve `engines`, `score`, `relevance_score`, `published_date`,
`full_text`, `favicon`, and result-level `images`.

Tavily's provider-controlled `auto_parameters` object is recursively bounded and
credential-redacted before it enters a report. If any keys, values, nesting, or
items were truncated, the report includes
`metadata.auto_parameters_truncated = true`. The generic provider adapter also
sets `metadata.metadata_truncated = true` when it must bound metadata from a
custom provider.

Every provider response also crosses a provider-neutral normalization boundary.
It accepts only bounded HTTP(S) URLs without embedded credentials; bounds
results, titles, snippets, full text, dates, answers, suggestions, and image
descriptions; limits result-level and query-level image collections; and
normalizes finite relevance into `0.0..=1.0`. Invalid entries are dropped.
When this changes provider output, the report contains a reserved
`metadata._a3s_normalization` object with bounded counters. Request identifiers
are bounded, non-finite or negative usage is discarded, and provider metadata
is recursively bounded before it reaches `SearchReport`.

One failed source does not discard successful sources. The CLI writes warnings
to stderr and keeps failures in `errors`; even an all-source failure returns an
empty result set with the individual errors. Treat those failures as part of
the evidence rather than silently ignoring them.

## Library usage

Run the built-in providers directly:

```rust,no_run
use a3s_search::providers::BuiltinProvider;
use a3s_search::{Search, SearchQuery};

#[tokio::main]
async fn main() -> a3s_search::Result<()> {
    let mut search = Search::new();
    search.add_engine(BuiltinProvider::AnySearch.create_engine()?);
    search.add_engine(BuiltinProvider::Tavily.create_engine()?);

    let results = search
        .search(SearchQuery::new("extensible Rust search"))
        .await?;

    for result in results.items() {
        println!("{} {} {:?}", result.score, result.url, result.engines);
    }
    for report in results.reports() {
        println!("{:?}", report);
    }
    Ok(())
}
```

`BuiltinProvider::create_engine` uses `ANYSEARCH_API_KEY`, `TAVILY_API_KEY`, and
`TAVILY_PROJECT` when present. Construct `AnySearchConfig` or `TavilyConfig`
directly for fully typed programmatic configuration.

### Add an external provider

Implement only the provider-neutral public protocol, then adapt it with
`ProviderEngine`:

```rust,no_run
use a3s_search::providers::{
    ProviderAuthentication, ProviderCapabilities, ProviderDescriptor,
    ProviderEngine, ProviderReadiness, ProviderReport, ProviderRequest,
    ProviderResponse, ProviderResult, SearchProvider,
};
use a3s_search::{Search, SearchQuery};
use async_trait::async_trait;

struct MyProvider;

#[async_trait]
impl SearchProvider for MyProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            "my-provider",
            "My Provider",
            "https://search.example.com/",
            ProviderCapabilities::new()
                .with_answers(true)
                .with_full_text(true),
        )
    }

    fn readiness(&self) -> ProviderReadiness {
        ProviderReadiness::Ready {
            authentication: ProviderAuthentication::Authenticated,
        }
    }

    async fn search(
        &self,
        request: &ProviderRequest,
    ) -> a3s_search::Result<ProviderResponse> {
        Ok(ProviderResponse::new()
            .with_result(
                ProviderResult::new(
                    "https://example.com/result",
                    format!("Result for {}", request.query),
                    "Provider-neutral snippet",
                )
                .with_full_text("Complete source content")
                .with_relevance_score(0.9),
            )
            .with_answer("Provider-neutral answer")
            .with_report(
                ProviderReport::new()
                    .with_request_id("request-1")
                    .with_total_results(1),
            ))
    }
}

#[tokio::main]
async fn main() -> a3s_search::Result<()> {
    let mut search = Search::new();
    search.add_engine(ProviderEngine::new(MyProvider));
    let results = search.search(SearchQuery::new("custom provider")).await?;
    assert_eq!(results.items().len(), 1);
    Ok(())
}
```

Provider protocol types are `#[non_exhaustive]`; use their public constructors
and builders. This keeps downstream providers source-compatible when common
evidence fields are added later.

### Migrating from 1.x

Version 2 introduces the provider architecture as an explicit API boundary.
Code that constructed `SearchResult` or `SearchConfig` with struct literals
should use `SearchResult::new` or `SearchConfig::new` and then set public
fields. Exhaustive `SearchError` matches must handle `SearchError::Provider`
and retain a wildcard arm. These types are now non-exhaustive so future
provider evidence and configuration can be added without another source
compatibility break.

## Architecture

```text
AnySearchProvider ─┐
TavilyProvider ────┼─ SearchProvider ─ ProviderEngine ─┐
CustomProvider ────┘                                  │
                                                      ├─ Search
HTTP/RSS engines ─────────────── Engine ──────────────┘
                                                         │
                                                         ▼
                                      Aggregator → SearchResults
```

The layers have separate responsibilities:

- `SearchProvider` models typed API requests, readiness, capabilities, rich
  responses, and sanitized provider failures.
- `ProviderEngine` adapts any provider into the stable `Engine` orchestration
  contract and applies the provider-neutral rich-output normalization boundary.
- `Search` owns parallel execution, timeouts, health, and partial failures.
- `Aggregator` owns URL normalization, merging, provenance, and ranking.
- Provider modules own wire protocols and are split into configuration,
  request, response, and error concerns.
- `ProviderHttpClient` enforces HTTPS, disables redirects, limits decompressed
  response size, applies timeouts, and never exposes response bodies in errors.

Client-side provider failures such as authentication, permission, quota, and
invalid request errors remain visible in `errors` but do not trip the engine
health circuit breaker. Transport, timeout, invalid-response, rate-limit, and
service failures still contribute to suspension.

This boundary lets a new provider reuse orchestration and aggregation without
adding provider-specific branches to the search core.

## Conventional engines

| Shortcut | Engine | Transport |
| --- | --- | --- |
| `ddg` | DuckDuckGo | HTTP |
| `brave` | Brave Search | HTTP |
| `bing` | Bing International | RSS |
| `wiki` | Wikipedia | JSON API |
| `sogou` | Sogou | HTTP |
| `360` | 360 Search | HTTP |
| `bing_cn` | Bing China | RSS |
| `g` | Google | A3S Use Browser, `headless` feature |
| `baidu` | Baidu | A3S Use Browser, `headless` feature |

Provider APIs and conventional engines can participate in the same search.

## Browser rendering and full text

With the `headless` feature, Search adapts the typed
`a3s_use_browser::PageRenderer` interface. A3S Use owns browser discovery,
managed installation, process lifecycle, and tab limits; Search owns only
URL-to-HTML adaptation, wait conditions, retries, and metrics. Search never
invokes the A3S Use CLI or MCP surface.

Providers may return `full_text` directly. For snippet-only results,
`enrich_full_text` can fetch each result page and extract the main article body:

```rust,no_run
use a3s_search::{enrich_full_text, HttpFetcher, PageFetcher};
use std::sync::Arc;
use std::time::Duration;

# async fn enrich(
#     results: &mut a3s_search::SearchResults,
# ) {
let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
enrich_full_text(results, fetcher, 8, Duration::from_secs(10)).await;
# }
```

Failed enrichment keeps the original snippet. JavaScript pages can use
`BrowserFetcher` through the same `PageFetcher` interface.

## Proxy and metrics

Conventional HTTP engines can use `HttpFetcher::with_proxy` or a
`PooledHttpFetcher` backed by `ProxyPool`. Provider APIs intentionally own
their bounded direct HTTP clients and do not inherit scraping proxies. The CLI
never echoes a potentially credential-bearing `--proxy` URL.

Attach `Metrics` to `Search` for success/failure counts and latency
percentiles. Provider-native request timing and usage remain available in
`SearchReport`.

### Static proxy pool

```rust,no_run
use a3s_search::proxy::{ProxyConfig, ProxyPool, ProxyProtocol, ProxyStrategy};

let proxies = vec![
    ProxyConfig::new("10.0.0.1", 8080).with_protocol(ProxyProtocol::Http),
    ProxyConfig::new("10.0.0.2", 1080).with_protocol(ProxyProtocol::Socks5),
];
let pool = ProxyPool::with_proxies(proxies).with_strategy(ProxyStrategy::RoundRobin);
```

### Dynamic proxy provider

```rust,no_run
use a3s_search::proxy::{spawn_auto_refresh, ProxyConfig, ProxyPool, ProxyProvider};
use a3s_search::{PageFetcher, PooledHttpFetcher};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

struct MyProxyProvider;

#[async_trait]
impl ProxyProvider for MyProxyProvider {
    async fn fetch_proxies(&self) -> a3s_search::Result<Vec<ProxyConfig>> {
        Ok(vec![ProxyConfig::new("10.0.0.1", 8080)])
    }

    fn refresh_interval(&self) -> Duration {
        Duration::from_secs(60)
    }
}

let pool = Arc::new(ProxyPool::with_provider(MyProxyProvider));
let _refresh = spawn_auto_refresh(Arc::clone(&pool));
let fetcher: Arc<dyn PageFetcher> =
    Arc::new(PooledHttpFetcher::new(Arc::clone(&pool)));
```

`PooledHttpFetcher` reuses one `reqwest` client per proxy URL while rotating
proxies between requests. `ProxyPool` supports:

| Method | Purpose |
| --- | --- |
| `new()` | Create an empty, disabled pool |
| `with_proxies(proxies)` | Create a static pool |
| `with_provider(provider)` | Create a dynamically refreshed pool |
| `with_strategy(strategy)` | Select round-robin or random routing |
| `get_proxy()` | Select the next proxy |
| `add_proxy()` / `remove_proxy()` | Mutate the pool |
| `set_enabled(bool)` / `is_enabled()` | Control routing |
| `refresh()` | Refresh from the dynamic provider |
| `len()` / `is_empty()` | Inspect pool size |
| `create_client(user_agent)` | Build a client for the selected proxy |

### Metrics

```rust,no_run
use a3s_search::metrics::{Metrics, TimingGuard};
use a3s_search::{HttpFetcher, Search};
use std::sync::Arc;

let metrics = Arc::new(Metrics::new());
let _search = Search::new().with_metrics(Arc::clone(&metrics));
let _fetcher = HttpFetcher::new().with_metrics(Arc::clone(&metrics));

metrics.record_success(std::time::Duration::from_millis(150));
metrics.record_failure("timeout", true);

# async fn inspect(metrics: Arc<Metrics>) {
let snapshot = metrics.snapshot().await;
println!("Success rate: {:.1}%", snapshot.success_rate());
println!("P50 latency: {}ms", snapshot.latency_p50_ms);
# }
```

`MetricsSnapshot` exposes total successes and failures, transient and permanent
failure counts, low-cardinality error counts, and p50/p95/p99 latency.
`TimingGuard` records success or failure latency through an RAII-style timer.

## API reference

This section keeps the main stable surfaces discoverable. Rustdoc remains the
authoritative source for exhaustive fields and feature gating.

### Search orchestration

```rust,ignore
impl Search {
    pub fn new() -> Self;
    pub fn with_health_config(config: HealthConfig) -> Self;
    pub fn with_metrics(self, metrics: Arc<Metrics>) -> Self;
    pub fn set_metrics(&mut self, metrics: Option<Arc<Metrics>>);
    pub fn metrics(&self) -> Option<Arc<Metrics>>;
    pub fn add_engine<E: Engine + 'static>(&mut self, engine: E);
    pub fn set_timeout(&mut self, timeout: Duration);
    pub fn engine_count(&self) -> usize;
    pub async fn search(&self, query: SearchQuery) -> Result<SearchResults>;
}
```

`SearchQuery` carries:

```rust,ignore
pub struct SearchQuery {
    pub query: String,
    pub categories: Vec<EngineCategory>,
    pub language: Option<String>,
    pub safesearch: SafeSearch,
    pub page: u32,
    pub time_range: Option<TimeRange>,
    pub engines: Vec<String>,
}
```

Use `SearchQuery::new`, `with_categories`, `with_language`,
`with_safesearch`, `with_page`, `with_time_range`, and `with_engines`.
`SafeSearch` provides `Off`, `Moderate`, and `Strict`; `TimeRange` provides
`Day`, `Week`, `Month`, and `Year`.

### Engine and provider extension points

Conventional engines implement:

```rust,ignore
#[async_trait]
pub trait Engine: Send + Sync {
    fn config(&self) -> &EngineConfig;
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>>;
    async fn search_output(&self, query: &SearchQuery) -> Result<EngineOutput>;
    fn name(&self) -> &str;
    fn shortcut(&self) -> &str;
    fn weight(&self) -> f64;
    fn is_enabled(&self) -> bool;
}
```

The default `search_output` wraps `search`, so existing engines do not need to
implement rich evidence. Native third-party integrations implement:

```rust,ignore
#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn readiness(&self) -> ProviderReadiness;
    async fn search(&self, request: &ProviderRequest)
        -> Result<ProviderResponse>;
}
```

`ProviderEngine::new(provider)` adapts this protocol to `Engine`.
`ProviderDescriptor` advertises stable identity and capabilities.
`ProviderRequest` carries normalized query controls. `ProviderResponse`
contains `ProviderResult`, answers, suggestions, images, and `ProviderReport`.
Use public constructors and builders because provider types are
`#[non_exhaustive]`.

### Results and reports

```rust,ignore
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub content: String,
    pub result_type: ResultType,
    pub engines: HashSet<String>,
    pub positions: Vec<u32>,
    pub score: f64,
    pub relevance_score: Option<f64>,
    pub thumbnail: Option<String>,
    pub published_date: Option<String>,
    pub favicon: Option<String>,
    pub images: Vec<SearchImage>,
    pub full_text: Option<String>,
}
```

`SearchResults` exposes `items`, `items_mut`, `suggestions`, `answers`,
`images`, `errors`, and `reports`; `count` and `duration_ms` are public summary
fields. `SearchReport` contains engine/provider identity, request ID, total
matches, provider timing, optional `SearchUsage`, and bounded provider
metadata.

### Fetching and browser rendering

```rust,ignore
#[async_trait]
pub trait PageFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<String>;
}

pub enum WaitStrategy {
    Load,
    NetworkIdle { idle_ms: u64 },
    Selector { css: String, timeout_ms: u64 },
    Delay { ms: u64 },
}
```

With `headless`, `BrowserFetcher` adapts any
`a3s_use_browser::PageRenderer`. It supports wait strategy, user agent,
timeout, retries, and metrics. `BrowserPool` owns shared tab concurrency and
provides `warm_up` and idempotent terminal `shutdown`. Its typed provider
choices are discovered, managed, or explicit Chrome/Lightpanda executables;
managed variants explicitly permit A3S Use to install the selected browser.

### Health, proxy, and metrics types

`HealthConfig` exposes `max_failures` and `suspend_duration`.
`ProxyConfig` contains host, port, protocol, and optional authentication;
`ProxyPool` supports static or dynamic providers and round-robin or random
selection. `Metrics` supports recording, snapshots, request counts, success
rate, and reset. See the preceding proxy and metrics examples for end-to-end
usage.

## Bundled Skill and release layout

Every platform archive contains:

```text
a3s-search
skills/a3s-search/SKILL.md
skills/a3s-search/agents/openai.yaml
```

The Skill guides coding agents through provider selection, JSON evidence,
anonymous/keyless and authenticated modes, ACL, secret handling, and partial
failures. Homebrew installs the binary under `bin` and the Skill under:

```text
share/a3s-search/skills/a3s-search
```

## Development

Run checks from the `a3s-search` repository root:

```bash
cargo fmt --all -- --check
cargo test --no-default-features
cargo test --all-features
cargo clippy --all-targets --no-default-features -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo package --locked
scripts/test-release-package.sh
```

The provider contract tests use loopback mock servers and verify protocol,
authentication, response adaptation, error sanitization, and CLI evidence.
Live provider smoke tests are separate because they depend on external service
availability.

## A3S ecosystem

```text
a3s-box      - MicroVM sandbox
a3s-code     - AI coding agent
a3s-lane     - Queue
a3s-memory   - Memory
a3s-search   - Search
```

## License

MIT
