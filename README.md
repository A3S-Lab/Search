<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="A3S Search converges browser, HTTP and RSS, and native API sources into one typed Rust metasearch result">
</p>

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <strong>One query in. One typed result boundary out.</strong>
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Search/actions/workflows/ci.yml"><img alt="A3S Search CI" src="https://img.shields.io/github/actions/workflow/status/A3S-Lab/Search/ci.yml?branch=main&amp;style=flat-square&amp;label=CI"></a>
  <a href="https://crates.io/crates/a3s-search"><img alt="a3s-search on crates.io" src="https://img.shields.io/crates/v/a3s-search?style=flat-square&amp;color=4f8cff"></a>
  <a href="https://docs.rs/a3s-search"><img alt="a3s-search documentation" src="https://img.shields.io/docsrs/a3s-search?style=flat-square&amp;color=35c98d"></a>
  <a href="https://www.rust-lang.org/"><img alt="Implemented in Rust" src="https://img.shields.io/badge/Rust-native-9ba7b4?style=flat-square"></a>
  <a href="./LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-171c24?style=flat-square"></a>
</p>

<p align="center">
  <a href="#run-one-search">Run</a> ·
  <a href="#choose-sources">Sources</a> ·
  <a href="#trust-the-fields-you-asked-for">Fields</a> ·
  <a href="#rank-and-receipt">Receipt</a> ·
  <a href="#configure">Configure</a> ·
  <a href="#extend">Extend</a>
</p>

---

A3S Search is a Rust library and CLI that runs one typed query across the
sources you select, then returns one `SearchResults` value. Browser pages,
HTTP/RSS documents, and native search APIs all cross the same `Engine`
boundary. The runtime executes concurrently, keeps partial failures, normalizes
and deduplicates URLs, and fuses source ranks.

> [!IMPORTANT]
> A3S Search retrieves. It does not plan queries, judge meaning, verify claims,
> or write reports. Callers such as DeepResearch own those decisions. A cascade
> receipt records structural execution, not semantic approval.

## Run one search

Install the published command, then search. Moli is the default headless
fallback; install it only if that tier must run.

```bash
cargo install a3s-search
# or: brew install A3S-Lab/tap/a3s-search

a3s-search "Rust async runtime guidance" --format json --limit 10
```

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/lexmount/moli/releases/latest/download/moli-installer.sh | sh
```

Set `A3S_MOLI_EXECUTABLE` when the executable is not on `PATH`.

An explicit `--engines` list is the whole plan. It is never expanded into the
default cascade, and every selected source receives the same `SearchQuery`.

```bash
a3s-search "Rust async runtime guidance" \
  --engines anysearch,tavily \
  --format json \
  --limit 10
```

JSON keeps the useful output and the degraded path in one document:

```text
SearchResults
├── results[]       ranked URLs, snippets, provenance, dates, rich fields
├── answers[]       direct answers, only when the provider was asked for them
├── images[]        images, only when the provider was asked for them
├── reports[]       request IDs, timing, usage, bounded metadata
├── failures[]      typed error, transient state, retry delay
└── outcomes[]      success, empty, failure, timeout, rejected, circuit-open
```

The default CLI also emits `cascade_receipt` and `cascade_receipt_binding`.

### Embedded in Rust

```toml
[dependencies]
a3s-search = "3"
tokio = { version = "1", features = ["full"] }
```

```rust
use a3s_search::{
    engines::{DuckDuckGo, Wikipedia},
    Search, SearchQuery,
};

#[tokio::main]
async fn main() -> a3s_search::Result<()> {
    let mut search = Search::new();
    search.add_engine(DuckDuckGo::new());
    search.add_engine(Wikipedia::new());

    let results = search
        .search(SearchQuery::new("extensible Rust search"))
        .await?;

    for result in results.items() {
        println!("{:.3} {} {:?}", result.score, result.url, result.engines);
    }
    Ok(())
}
```

`Search` is caller-owned. The host chooses engines, weights, timeouts, and
shared reliability controls. The crate does not hide them in global state.
Use `default-features = false` for an HTTP/API-only build.

## Choose sources

A3S Search does not keep a private web index. With no explicit selection and no
ACL source selection, the CLI uses this plan and stops when structural
requirements are met:

```text
1  native API     anysearch + tavily
2  HTTP / RSS     wiki + ddg + bing
3  headless       brave_browser + bing_browser through Moli
```

A no-headless build adds `brave` to the HTTP tier instead of the browser
fallback. Tiers share one deadline (20 seconds by default). Engines in a tier
run concurrently. A lower tier is constructed only when the tier above it has
not met the structural requirement.

| Class | Shortcut | Source | When it runs |
| --- | --- | --- | --- |
| Native API | `anysearch` | AnySearch | Default primary tier |
| Native API | `tavily` | Tavily | Default primary tier |
| HTTP/RSS | `wiki` | Wikipedia | Default second tier |
| HTTP/RSS | `ddg` | DuckDuckGo | Default second tier |
| HTTP/RSS | `bing` | Bing International RSS | Default second tier |
| Browser | `brave_browser` | Brave Search | Default final fallback |
| Browser | `bing_browser` | Bing International | Default final fallback |
| HTTP/RSS | `brave` | Brave Search | No-headless build only |
| Browser | `g` | Google | Explicit |
| Browser | `baidu` | Baidu | Explicit |
| HTTP/RSS | `sogou` | Sogou | Explicit |
| HTTP/RSS | `360` | 360 Search | Explicit |
| HTTP/RSS | `bing_cn` | Bing China RSS | Explicit |
| Native API | `tinyfish` | TinyFish Search | Explicit, billed |
| Native API | `bocha` | Bocha Web Search | Explicit, billed |
| Native API | `aliyun` | Alibaba Cloud IQS | Explicit, billed |
| Native API | `tencent` | Tencent Cloud Search | Explicit, billed |
| Native API | `firecrawl` | Firecrawl Search | Explicit, billed |

`bing_cn` requests `www.bing.com/search` with `format=rss`, `setlang=zh-CN`,
and `mkt=zh-CN`. It does not use `cn.bing.com`: that host redirects some
clients off `/search` onto the homepage, which then returns HTML.

HTML engines reject CAPTCHA, consent, and anti-bot pages as typed `challenge`
failures. An unrelated successful page is `invalid_response`, not an empty
result set.

### Billed APIs stay out of the default plan

TinyFish, Bocha, Alibaba Cloud IQS, Tencent Cloud Search, and Firecrawl require
an API key. They are omitted from the default cascade so a missing key does not
fail every search and an ambient key does not start billing. Select them
explicitly:

```bash
export TINYFISH_API_KEY="..."
export BOCHA_API_KEY="..."
export ALIYUN_IQS_API_KEY="..."
export TENCENTCLOUD_WSA_APIKEY="..."
export FIRECRAWL_API_KEY="..."

a3s-search "query" --engines bocha --format json
```

AnySearch and Tavily accept optional bearer authentication and can run without
a key. `--proxy` applies to scraping transports. Native provider API requests
stay direct.

## Trust the fields you asked for

<p align="center">
  <img src="./assets/readme/architecture.svg" width="100%" alt="A3S Search passes one exact query through source execution, normalization, URL fusion, and weighted rank fusion while semantic policy stays with the caller">
</p>

Every native API implements `SearchProvider`. `ProviderEngine` is the only
adapter into the cascade, ranking, and the CLI. The public result cap is
`max_results` in Rust and ACL. Vendor wire names (`count`, `limit`, `Cnt`,
`numResults`) are not ACL attributes.

An output capability that requires a request flag follows that flag. Do not
treat an omitted answer, image, usage record, or page text as available.

| Provider | Default request | What that means for the response |
| --- | --- | --- |
| AnySearch | MCP `search` tool | Full text is requested; there is no flag that turns it off |
| Tavily | Plain source text | Answers, images, and usage stay off until requested. Set `include_raw_content = "none"` to opt out of source text |
| TinyFish | No thumbnails | `images` follows `include_thumbnail` (default false). Paging is supported |
| Bocha | Summaries on | `full_text` follows `summary` (default true). Set `summary = false` to omit page summaries |
| Aliyun IQS | `LiteAdvanced`, no page text | `full_text` follows `include_main_text` (default false). Images may still appear; the request cannot disable them |
| Tencent | `Cnt` omitted | Full text and images may appear. Set premium `max_results` only when that billed count is intended |
| Firecrawl | Web metadata | `full_text` follows `include_markdown` (default false). That flag scrapes each result and bills extra. `images` follows image sources |

Required-credential JSON APIs share one transport shell. The vendor module owns
options, request mapping, response mapping, and error-code classification.
Optional-auth and MCP codecs stay off that shell. Credentials belong in
`env("VARIABLE")`, never in endpoint URLs, and are sealed out of success bodies
before the reply is dropped.

## Rank and receipt

Rank fusion is not semantic scoring. After URL normalization and
deduplication, each source contributes:

```text
engine weight × reciprocal rank × provider-local relevance factor
```

A provider relevance value is calibrated only inside that provider's response.
Scores from unrelated APIs are not placed on one scale, and query-text matching
is not substituted for them.

`RetrievalRequirements::for_limit` asks for one logical source on a
single-result request and two independent logical sources when more than one
result is requested. Browser and HTTP variants of the same upstream count as
one source, so repeating a transport cannot stop fallback. The CLI then applies
`select_structural_window` to the visible Top-K using URL validity, hosts,
provenance, and consensus. It does not read the query, title, snippet,
language, or publisher.

`receipt_binding()` is a domain-separated SHA-256 over the validated receipt.
It detects substitution against a trusted digest. It does not prove who ran the
search or whether the pages are true.

## Configure

ACL selects sources, credentials, timeouts, ranking, and provider options.
`--config` is a parent flag:

```bash
a3s-search --config search.acl engines
a3s-search "query" --config search.acl --format json
```

```acl
timeout {
  value = 20
}

provider "tavily" {
  api_key             = env("TAVILY_API_KEY")
  max_results         = 10
  include_raw_content = "markdown"
  include_answer      = "advanced"
  include_images      = true
  include_usage       = true
}

provider "bocha" {
  api_key     = env("BOCHA_API_KEY")
  max_results = 8
  summary     = true
}

provider "aliyun" {
  api_key          = env("ALIYUN_IQS_API_KEY")
  engine_type      = "lite-advanced"
  include_main_text = true
}

provider "firecrawl" {
  api_key          = env("FIRECRAWL_API_KEY")
  max_results      = 8
  include_markdown = false
}
```

The Tavily block above opts into answers, images, and usage. Leave those
attributes unset to keep the default, which requests source text only.
`enabled = false` records `engine_disabled` and does not contact the network.
Unknown wire names such as `count`, `limit`, and `cnt` are rejected.

Typed surfaces:
[`SearchConfig`](https://docs.rs/a3s-search/latest/a3s_search/struct.SearchConfig.html),
[`AnySearchConfig`](https://docs.rs/a3s-search/latest/a3s_search/providers/struct.AnySearchConfig.html),
[`TavilyConfig`](https://docs.rs/a3s-search/latest/a3s_search/providers/struct.TavilyConfig.html),
`TinyFishConfig`, `BochaConfig`, `AliyunConfig`, `TencentConfig`, and
`FirecrawlConfig`.

## Extend

Use `Engine` for a source that returns ordinary web or media results. Use
`SearchProvider` when the source has capabilities, readiness, rich output, or
a structured report, then adapt it with `ProviderEngine`:

```rust
let engine = ProviderEngine::new(my_provider);
search.add_engine(engine);
```

Direct answers, images, full text, usage, and reports stay first-class fields.
They are not rewritten into synthetic web results.

<details>
<summary><strong>Ordinary Engine implementation</strong></summary>

```rust
#[async_trait::async_trait]
impl a3s_search::Engine for MyEngine {
    fn config(&self) -> &a3s_search::EngineConfig {
        &self.config
    }

    async fn search(
        &self,
        query: &a3s_search::SearchQuery,
    ) -> a3s_search::Result<Vec<a3s_search::SearchResult>> {
        // Call one source and map its response into SearchResult values.
        todo!()
    }
}
```

</details>

## Operate

| Control | What it bounds |
| --- | --- |
| `HealthMonitor` | Consecutive-failure suspension on one `Search` |
| `CircuitBreaker` | Shared closed/open/half-open source state |
| `Bulkhead` | In-flight work and queue wait per engine |
| `RetryBudget` | Retry amplification |
| `SearchCoalescer` | Identical overlapping requests; not a cache |
| `Metrics` | In-memory counts and latency percentiles |

Share controls only across compatible tenants, credentials, endpoints, and
proxies. The CLI persists credential-independent challenge circuits, not
queries, results, or secrets. Set `A3S_SEARCH_STATE_DIR` to an absolute
directory for an isolated state scope.

| Build | Browser behavior |
| --- | --- |
| default / `headless` / `moli` | Discover Moli from `A3S_MOLI_EXECUTABLE`, PATH, or the official install location |
| `--browser chrome` | Explicit Chrome/Chromium backend |
| `lightpanda` | Explicit backend only; Windows requires WSL2 |
| `--no-default-features` | No browser/CDP stack |

```bash
cargo run -- "query" --browser moli
cargo run -- "query" --browser chrome
cargo run --features lightpanda -- "query" --browser lightpanda
```

Native providers may return `full_text` directly. Snippet-only results can be
enriched with `enrich_full_text`. A failed enrichment keeps the snippet and
does not change rank or cascade decisions. Scraping proxies do not apply to
native provider clients.

## From v2

Version 3 removed semantic policy from this crate and made Moli the default
browser fallback.

- `SearchQuality`, `SearchQualityFloor`, and `query_match_score` are gone.
  Evaluate relevance in the host.
- Build cascades with `RetrievalRequirements`. Record an opaque host decision
  with `push_tier_with_decision`; the receipt marks it `external_policy`.
- Consume `SearchCascadeOutcomeV2` and `SearchCascadeReceiptV2`. None of those
  fields approves the pages.
- The default feature includes Moli. `--no-default-features` is the HTTP/API
  build.
- With no explicit selection, the CLI runs `API → HTTP/RSS → headless`.

Unpublished candidate identities before v3.0.9 were retired and are not reused.
From v3.0.9, release assurance lives in this repository: the exact source
revision and package bytes must match before crates.io, GitHub Release, or
Homebrew can publish. Prerelease tags may publish GitHub CLI archives only.

## Develop

Run checks from this repository, not from the A3S monorepo root:

```bash
cargo fmt --all -- --check
cargo test --no-default-features --locked
cargo test --all-features --locked
cargo clippy --all-targets --no-default-features --locked -- -D warnings
cargo clippy --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
```

The release workflow also runs both clippy feature sets, package and freeze
checks, and the pinned Moli fixture. A 300-second reliability soak is opt-in
and is not part of CI. Missing or mismatched package evidence keeps crates.io,
GitHub Release, and Homebrew fail-closed.

Platform archives include the CLI and `skills/a3s-search/SKILL.md`. The Skill
selects sources and reads structural evidence. It does not evaluate pages.

## Ecosystem

- [A3S](https://github.com/A3S-Lab/a3s) — platform entry point
- [A3S Code](https://github.com/A3S-Lab/Code) — coding-agent runtime
- [Moli](https://github.com/lexmount/moli) — default headless browser runtime
- [A3S Browser](https://github.com/A3S-Lab/Browser) — rendering contract and Chrome backend
- [A3S Science](https://github.com/A3S-Lab/Science) — research workflows above this kernel

## Contributing

Keep semantic evaluation outside the retrieval kernel. Cover behavior changes
with tests, and run both feature sets before submitting.

## License

[MIT](./LICENSE)
