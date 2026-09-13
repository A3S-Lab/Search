<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="A3S Search 将浏览器、HTTP 与 RSS 以及原生 API 源收敛为同一类型化的 Rust 元搜索结果">
</p>

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <strong>一次查询输入。一个类型化的结果边界输出。</strong>
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Search/actions/workflows/ci.yml"><img alt="A3S Search CI" src="https://img.shields.io/github/actions/workflow/status/A3S-Lab/Search/ci.yml?branch=main&amp;style=flat-square&amp;label=CI"></a>
  <a href="https://crates.io/crates/a3s-search"><img alt="crates.io 上的 a3s-search" src="https://img.shields.io/crates/v/a3s-search?style=flat-square&amp;color=4f8cff"></a>
  <a href="https://docs.rs/a3s-search"><img alt="a3s-search 文档" src="https://img.shields.io/docsrs/a3s-search?style=flat-square&amp;color=35c98d"></a>
  <a href="https://www.rust-lang.org/"><img alt="以 Rust 实现" src="https://img.shields.io/badge/Rust-native-9ba7b4?style=flat-square"></a>
  <a href="./LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/license-MIT-171c24?style=flat-square"></a>
</p>

<p align="center">
  <a href="#运行一次搜索">运行</a> ·
  <a href="#选择源">源</a> ·
  <a href="#只相信你请求过的字段">字段</a> ·
  <a href="#排序与收据">收据</a> ·
  <a href="#配置">配置</a> ·
  <a href="#扩展">扩展</a>
</p>

---

A3S Search 是一个 Rust 库和 CLI。它对你选择的源执行一次类型化查询，并返回一个 `SearchResults`。浏览器页面、HTTP/RSS 文档和原生搜索 API 都穿越同一 `Engine` 边界。运行时并发执行、保留部分失败、规范化并去重 URL，然后融合源排序。

> [!IMPORTANT]
> A3S Search 只负责检索。它不规划查询、不判断含义、不核实主张、不撰写报告。DeepResearch 等调用方拥有这些决定。级联收据记录的是结构执行，不是语义批准。

## 运行一次搜索

安装已发布命令，然后搜索。Moli 是默认无头回退；只有那一层需要运行时才安装它。

```bash
cargo install a3s-search
# 或：brew install A3S-Lab/tap/a3s-search

a3s-search "Rust async runtime guidance" --format json --limit 10
```

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/lexmount/moli/releases/latest/download/moli-installer.sh | sh
```

可执行文件不在 `PATH` 上时，设置 `A3S_MOLI_EXECUTABLE`。

显式 `--engines` 就是完整计划。它不会被扩展成默认级联，每个被选源收到同一 `SearchQuery`。

```bash
a3s-search "Rust async runtime guidance" \
  --engines anysearch,tavily \
  --format json \
  --limit 10
```

JSON 把有用输出和降级路径放在同一文档里：

```text
SearchResults
├── results[]       排序后的 URL、摘要、来源、日期、丰富字段
├── answers[]       直接答案，仅当请求了该提供方的答案
├── images[]        图片，仅当请求了该提供方的图片
├── reports[]       请求 ID、耗时、用量、有界元数据
├── failures[]      类型化错误、瞬时状态、重试延迟
└── outcomes[]      success、empty、failure、timeout、rejected、circuit-open
```

默认 CLI 还会发出 `cascade_receipt` 和 `cascade_receipt_binding`。

### 嵌入 Rust

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

`Search` 由调用方拥有。宿主选择引擎、权重、超时和共享可靠性控制。crate 不把它们藏进全局状态。只要 HTTP/API 构建时使用 `default-features = false`。

## 选择源

A3S Search 不维护私有网页索引。没有显式选择、也没有 ACL 源选择时，CLI 使用下面的计划，并在结构要求满足后停止：

```text
1  native API     anysearch + tavily
2  HTTP / RSS     wiki + ddg + bing
3  headless       brave_browser + bing_browser through Moli
```

无头功能关闭的构建不使用浏览器回退，而是把 `brave` 放进 HTTP 层。各层共享一个截止时间（默认 20 秒）。层内引擎并发运行。只有上层尚未满足结构要求时，才会构造下一层。

| 类别 | Shortcut | 源 | 何时运行 |
| --- | --- | --- | --- |
| 原生 API | `anysearch` | AnySearch | 默认主层 |
| 原生 API | `tavily` | Tavily | 默认主层 |
| HTTP/RSS | `wiki` | Wikipedia | 默认第二层 |
| HTTP/RSS | `ddg` | DuckDuckGo | 默认第二层 |
| HTTP/RSS | `bing` | Bing 国际 RSS | 默认第二层 |
| 浏览器 | `brave_browser` | Brave Search | 默认最终回退 |
| 浏览器 | `bing_browser` | Bing 国际 | 默认最终回退 |
| HTTP/RSS | `brave` | Brave Search | 仅无头功能关闭的构建 |
| 浏览器 | `g` | Google | 显式 |
| 浏览器 | `baidu` | Baidu | 显式 |
| HTTP/RSS | `sogou` | Sogou | 显式 |
| HTTP/RSS | `360` | 360 搜索 | 显式 |
| HTTP/RSS | `bing_cn` | Bing 中国 RSS | 显式 |
| 原生 API | `tinyfish` | TinyFish Search | 显式，计费 |
| 原生 API | `bocha` | Bocha Web Search | 显式，计费 |
| 原生 API | `aliyun` | 阿里云 IQS | 显式，计费 |
| 原生 API | `tencent` | 腾讯云搜索 | 显式，计费 |
| 原生 API | `firecrawl` | Firecrawl Search | 显式，计费 |

`bing_cn` 请求 `www.bing.com/search`，并带上 `format=rss`、`setlang=zh-CN` 和 `mkt=zh-CN`。不使用 `cn.bing.com`：该主机会把部分客户端从 `/search` 重定向到首页，随后返回 HTML。

HTML 引擎把 CAPTCHA、同意页和反爬页拒绝为类型化的 `challenge` 失败。无关的成功页是 `invalid_response`，不是空结果集。

### 计费 API 不进入默认计划

TinyFish、Bocha、阿里云 IQS、腾讯云搜索和 Firecrawl 需要 API key。它们被排除在默认级联之外，以免缺少 key 时每次搜索都失败，或环境里已有 key 时意外计费。必须显式选择：

```bash
export TINYFISH_API_KEY="..."
export BOCHA_API_KEY="..."
export ALIYUN_IQS_API_KEY="..."
export TENCENTCLOUD_WSA_APIKEY="..."
export FIRECRAWL_API_KEY="..."

a3s-search "query" --engines bocha --format json
```

AnySearch 和 Tavily 接受可选 bearer 认证，也可以无 key 运行。`--proxy` 用于抓取传输。原生提供方 API 请求保持直连。

## 只相信你请求过的字段

<p align="center">
  <img src="./assets/readme/architecture.svg" width="100%" alt="A3S Search 让一次精确查询穿过源执行、规范化、URL 融合和加权秩融合，语义策略留在调用方">
</p>

每个原生 API 都实现 `SearchProvider`。`ProviderEngine` 是进入级联、排序和 CLI 的唯一适配器。公开结果上限在 Rust 和 ACL 中都是 `max_results`。厂商线上名称（`count`、`limit`、`Cnt`、`numResults`）不是 ACL 属性。

需要请求标志才会产生的输出能力跟随该标志。不要把未请求的答案、图片、用量或页面正文当成可用。

| 提供方 | 默认请求 | 对响应的含义 |
| --- | --- | --- |
| AnySearch | MCP `search` 工具 | 会请求全文；没有关闭它的标志 |
| Tavily | 纯源文本 | 答案、图片和用量在显式请求前关闭。设置 `include_raw_content = "none"` 可退出源文本 |
| TinyFish | 无缩略图 | `images` 跟随 `include_thumbnail`（默认 false）。支持分页 |
| Bocha | 摘要开启 | `full_text` 跟随 `summary`（默认 true）。设置 `summary = false` 可省略页面摘要 |
| 阿里云 IQS | `LiteAdvanced`，无页面正文 | `full_text` 跟随 `include_main_text`（默认 false）。图片仍可能出现；请求无法关闭它们 |
| 腾讯云 | 省略 `Cnt` | 全文和图片仍可能出现。只有打算使用该付费条数时才设置 `max_results` |
| Firecrawl | 网页元数据 | `full_text` 跟随 `include_markdown`（默认 false）。该标志会抓取每条结果并额外计费。`images` 跟随图片源 |

需要凭证的 JSON API 共享一个传输外壳。厂商模块只拥有选项、请求映射、响应映射和错误码分类。可选认证和 MCP 编解码不进入这个外壳。凭证放在 `env("VARIABLE")` 中，不放进端点 URL，并在响应丢弃前从成功体中封口。

## 排序与收据

秩融合不是语义打分。URL 规范化并去重之后，每个源的贡献是：

```text
engine weight × reciprocal rank × provider-local relevance factor
```

提供方相关性只在该提供方自己的响应内校准。无关 API 的分数不放在同一尺度上，也不用查询文本匹配替代它们。

`RetrievalRequirements::for_limit` 对单结果请求要求一个逻辑源，对多结果请求要求两个独立逻辑源。同一上游的浏览器和 HTTP 变体算作一个源，因此重复传输不能停止回退。CLI 再用 `select_structural_window` 处理可见 Top-K，依据只包括 URL 有效性、主机、来源和共识。它不读取查询、标题、摘要、语言或出版方。

`receipt_binding()` 是已校验收据上的域分离 SHA-256。与可信摘要比较时可以检测替换。它不证明谁运行了搜索，也不证明页面为真。

## 配置

ACL 选择源、凭证、超时、排序和提供方选项。`--config` 是父级标志：

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
  api_key           = env("ALIYUN_IQS_API_KEY")
  engine_type       = "lite-advanced"
  include_main_text = true
}

provider "firecrawl" {
  api_key          = env("FIRECRAWL_API_KEY")
  max_results      = 8
  include_markdown = false
}
```

上面的 Tavily 块选择了答案、图片和用量。不设置这些属性时保持默认，只请求源文本。`enabled = false` 记录 `engine_disabled`，且不访问网络。`count`、`limit`、`cnt` 等未知线上名称会被拒绝。

类型化表面：
[`SearchConfig`](https://docs.rs/a3s-search/latest/a3s_search/struct.SearchConfig.html)、
[`AnySearchConfig`](https://docs.rs/a3s-search/latest/a3s_search/providers/struct.AnySearchConfig.html)、
[`TavilyConfig`](https://docs.rs/a3s-search/latest/a3s_search/providers/struct.TavilyConfig.html)、
`TinyFishConfig`、`BochaConfig`、`AliyunConfig`、`TencentConfig` 和
`FirecrawlConfig`。

## 扩展

源只返回普通网页或媒体结果时使用 `Engine`。源有能力、就绪状态、丰富输出或结构化报告时使用 `SearchProvider`，再用 `ProviderEngine` 适配：

```rust
let engine = ProviderEngine::new(my_provider);
search.add_engine(engine);
```

直接答案、图片、全文、用量和报告保持为一等字段。它们不会被改写成合成网页结果。

<details>
<summary><strong>普通 Engine 实现</strong></summary>

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

## 运行

| 控制 | 约束什么 |
| --- | --- |
| `HealthMonitor` | 单个 `Search` 上的连续失败挂起 |
| `CircuitBreaker` | 共享的 closed/open/half-open 源状态 |
| `Bulkhead` | 每个引擎的在途工作与队列等待 |
| `RetryBudget` | 重试放大 |
| `SearchCoalescer` | 相同的重叠请求；不是缓存 |
| `Metrics` | 内存中的计数与延迟百分位 |

只在兼容的租户、凭证、端点和代理之间共享这些控制。CLI 只持久化与凭证无关的挑战熔断，不保存查询、结果或密钥。需要隔离状态范围时，把 `A3S_SEARCH_STATE_DIR` 设为绝对目录。

| 构建 | 浏览器行为 |
| --- | --- |
| default / `headless` / `moli` | 从 `A3S_MOLI_EXECUTABLE`、PATH 或官方安装位置发现 Moli |
| `--browser chrome` | 显式 Chrome/Chromium 后端 |
| `lightpanda` | 仅显式后端；Windows 需要 WSL2 |
| `--no-default-features` | 无浏览器/CDP 栈 |

```bash
cargo run -- "query" --browser moli
cargo run -- "query" --browser chrome
cargo run --features lightpanda -- "query" --browser lightpanda
```

原生提供方可以直接返回 `full_text`。只有摘要的结果可以用 `enrich_full_text` 富化。富化失败时保留摘要，且不改变排序或级联决定。抓取代理不作用于原生提供方客户端。

## 从 v2 升级

版本 3 从这个 crate 中移除了语义策略，并把 Moli 设为默认浏览器回退。

- `SearchQuality`、`SearchQualityFloor` 和 `query_match_score` 已删除。在宿主中评估相关性。
- 用 `RetrievalRequirements` 构建级联。用 `push_tier_with_decision` 记录不透明的宿主决定；收据将其标为 `external_policy`。
- 消费 `SearchCascadeOutcomeV2` 和 `SearchCascadeReceiptV2`。这些字段都不批准页面。
- 默认 feature 包含 Moli。`--no-default-features` 是 HTTP/API 构建。
- 没有显式选择时，CLI 运行 `API → HTTP/RSS → headless`。

v3.0.9 之前未发布的候选身份已退役，不再复用。从 v3.0.9 起，发布保障留在本仓库：确切的源修订和包字节必须匹配，crates.io、GitHub Release 或 Homebrew 才能发布。预发布标签只能发布 GitHub CLI 归档。

## 开发

从本仓库运行检查，而不是从 A3S monorepo 根目录：

```bash
cargo fmt --all -- --check
cargo test --no-default-features --locked
cargo test --all-features --locked
cargo clippy --all-targets --no-default-features --locked -- -D warnings
cargo clippy --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
```

发布工作流还会运行两组 clippy feature、包与冻结检查，以及固定的 Moli 夹具。300 秒可靠性 soak 是可选的，不在 CI 中。缺失或不匹配的包证据会使 crates.io、GitHub Release 和 Homebrew 失败即关闭。

平台归档包含 CLI 和 `skills/a3s-search/SKILL.md`。该 Skill 选择源并读取结构证据。它不评估页面。

## 生态

- [A3S](https://github.com/A3S-Lab/a3s) — 平台入口
- [A3S Code](https://github.com/A3S-Lab/Code) — 编码代理运行时
- [Moli](https://github.com/lexmount/moli) — 默认无头浏览器运行时
- [A3S Browser](https://github.com/A3S-Lab/Browser) — 渲染契约与 Chrome 后端
- [A3S Science](https://github.com/A3S-Lab/Science) — 检索内核之上的研究工作流

## 贡献

把语义评估留在检索内核之外。为行为变更补充测试，并在提交前运行两组 feature。

## 许可证

[MIT](./LICENSE)
