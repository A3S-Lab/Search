<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="A3S Search 将浏览器、HTTP 与 RSS 以及原生 API 源收敛为同一类型化的 Rust 元搜索结果">
</p>

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <strong>多种搜索源输入。一个类型化、可嵌入的结果边界输出。</strong>
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Search/actions/workflows/ci.yml"><img alt="A3S Search CI" src="https://img.shields.io/github/actions/workflow/status/A3S-Lab/Search/ci.yml?branch=main&amp;style=flat-square&amp;label=CI"></a>
  <a href="https://crates.io/crates/a3s-search"><img alt="crates.io 上的 a3s-search" src="https://img.shields.io/crates/v/a3s-search?style=flat-square&amp;color=4f8cff"></a>
  <a href="https://docs.rs/a3s-search"><img alt="a3s-search 文档" src="https://img.shields.io/docsrs/a3s-search?style=flat-square&amp;color=35c98d"></a>
  <a href="https://www.rust-lang.org/"><img alt="以 Rust 实现" src="https://img.shields.io/badge/Rust-native-9ba7b4?style=flat-square"></a>
  <a href="./LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/license-MIT-171c24?style=flat-square"></a>
</p>

<p align="center">
  <a href="#运行一次搜索">快速开始</a> ·
  <a href="#元搜索边界">边界</a> ·
  <a href="#检索源">源</a> ·
  <a href="#排序与回退">排序</a> ·
  <a href="#扩展引擎">扩展</a> ·
  <a href="#无全局策略的可靠性">可靠性</a>
</p>

---

A3S Search 是一个 Rust 库及配套 CLI，用于组合独立的网络搜索源。浏览器渲染引擎、常规 HTTP/RSS 端点与原生搜索 API 都穿越同一 `Engine` 边界。运行时并发扇出工作、保留部分失败、规范化并去重 URL、融合源排序，并返回一个结构化的 `SearchResults` 容器。

> [!IMPORTANT]
> A3S Search 是检索内核，不是研究代理。它不改写查询、不评判文档含义、不验证主张、不撰写报告。DeepResearch 等调用方拥有查询规划、语义评估、佐证与结论。

## 运行一次搜索

### 从 CLI

安装最新已发布命令：

```bash
cargo install a3s-search

# macOS or Linux through the A3S Homebrew tap
brew install A3S-Lab/tap/a3s-search
```

使用默认的 API 优先级联进行搜索。Moli 是默认无头回退。用官方安装程序安装，或在可执行文件位于别处时设置
`A3S_MOLI_EXECUTABLE=/path/to/moli`：

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/lexmount/moli/releases/latest/download/moli-installer.sh | sh
```

```bash
a3s-search "Rust async runtime guidance" --format json --limit 10
```

或自行选择确切的源与传输优先级：

```bash
a3s-search "Rust async runtime guidance" \
  --engines ddg,wiki,anysearch,tavily \
  --tier-order api,http-rss,headless \
  --language en-US \
  --time-range month \
  --browser-retries 0 \
  --format json
```

显式 `--engines` 列表绝不会被扩展。每个被选引擎收到同一类型化的 `SearchQuery`；CLI 不会创建隐藏的细化查询。

JSON 响应将有用输出与降级路径诊断放在一起：

```text
SearchResults
├── results[]       ranked URLs, snippets, provenance, dates, rich fields
├── answers[]       provider-native direct answers
├── images[]        provider-native query and result images
├── reports[]       request IDs, timing, usage, and bounded metadata
├── failures[]      typed error, transient state, provider, retry delay
└── outcomes[]      success, empty, failure, timeout, rejected, circuit-open
```

默认 CLI 还会发出 `cascade_receipt` 与
`cascade_receipt_binding`。它们记录结构执行状态；不是对返回页面的语义认可。

### 嵌入 Rust

添加库与 Tokio：

```toml
[dependencies]
a3s-search = "3"
tokio = { version = "1", features = ["full"] }
```

仅组合应用需要的源：

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

`Search` 由调用方拥有。宿主选择引擎、权重、超时、指标与共享可靠性控制；crate 不会把它们藏在全局状态中。

> [!IMPORTANT]
> 第 3 版刻意将 A3S Search 收窄为可嵌入的元搜索边界。它检索、规范化、合并、排序并记录结构性回退证据；调用方拥有语义质量与研究策略。升级前请参见
> [从 v2 迁移](#从-v2-迁移)。

## 元搜索边界

<p align="center">
  <img src="./assets/readme/architecture.svg" width="100%" alt="A3S Search 将一个确切查询经源执行、规范化、URL 融合与加权秩融合传递，语义策略仍由调用方持有">
</p>

| A3S Search 拥有 | 调用方拥有 |
| --- | --- |
| 源选择与并发执行 | 查询分解与调查策略 |
| 超时、类型化失败、重试、熔断与隔舱 | 相关性、权威性、时效与证据充分性 |
| 提供方中立的规范化与 URL 去重 | 佐证、矛盾处理与事实核验 |
| 保留来源的加权秩融合 | 报告结构、引用、语言与结论 |
| 结构性检索健康与级联收据 | 任何停止或继续研究的语义决策 |

公开架构有两条扩展路径：

```text
ordinary source ─────────────── Engine ───────────────┐
                                                     ├─ Search ─ Aggregator ─ SearchResults
native search API ─ SearchProvider ─ ProviderEngine ─┘
                                                                        │
                                      optional SearchCascade ─ receipt V2
```

- `Engine` 是普通网页或媒体结果的最小契约。其 `name` 是逻辑源身份；传输变体共享该名称并暴露不同的可选 `shortcut` 值。
- `SearchProvider` 建模原生 API 能力、就绪、丰富输出与提供方报告。
- `ProviderEngine` 将该提供方协议适配到与常规引擎相同的运行时。
- `Aggregator` 拥有 URL 规范化、字段合并、来源与秩融合——从不做查询文本打分。
- `SearchCascade` 是可选的。它记录有序检索层，并可将决策归因于结构要求或外部策略。

在内部，运行时在四个阶段保持同一边界显式：注册为每个引擎快照不可变配置描述符，选择解析源身份并仅准入匹配请求与当前可靠性状态的源，隔离的 runner 执行一次有界尝试，协调器将输出折叠进 `SearchResults`。这使超时、隔舱、熔断、指标与健康核算统一适用，而无需让源适配器或调用方依赖编排细节。

## 从 v2 迁移

第 3 版从元搜索层移除语义策略，并将 Moli 作为默认浏览器回退。有意的破坏性变更为：

- 移除 `SearchQuality`、`SearchQualityFloor` 与 `query_match_score`。在宿主中评估相关性、权威性、时效与证据充分性。
- 用 `RetrievalRequirements` 构造 `SearchCascade`。用 `push_tier` 做结构性回退，或用 `push_tier_with_decision` 记录外部策略做出的不透明决策。
- 消费 `SearchCascadeOutcomeV2` 与 `SearchCascadeReceiptV2`。Receipt V2 记录检索要求、最终健康、决策权威、耗尽、结果绑定与计数；这些字段都不是语义认可。
- 默认 Cargo feature 现包含基于 Moli 的无头检索。对仅 HTTP/API 的库构建使用 `default-features = false`。
- 在没有显式源选择时，CLI 运行 `API → HTTP/RSS → headless`。`--engines` 仍是确切源列表，需要不同操作顺序时 `--tier-order` 接受完整排列。

回退实现中不嵌入任何查询、主题、语言、出版方或相关性规则。

v3.0.0 至 v3.0.5 以及 v3.0.8 候选标签在发布前已退役。v3.0.6 与 v3.0.7 验证协议在创建 Search 标签前已退役。这些身份均不会被移动或复用。

从 v3.0.9 起，发布保障位于本 Rust 项目而非外部验证器。发布门检查确切源修订、包身份、结果与收据契约、确定性回退与故障行为、重试、延迟与资源释放。实时上游限制与打开的熔断仍是可观察结果；它们不会使原本成功的检索失败。相关性、事实支持、权威性、可答性与报告质量仍是 DeepResearch 等调用方的职责。

## 检索源

A3S Search 不维护私有网页索引。它嵌入宿主选择的源：

| 类别 | Shortcut | 源 | 传输 | 默认 CLI 计划 |
| --- | --- | --- | --- | --- |
| Browser | `brave_browser` | Brave Search | Moli（默认） | 最终回退 |
| Browser | `bing_browser` | Bing International | Moli（默认） | 最终回退 |
| Browser | `g` | Google | Moli（默认） | 显式 |
| Browser | `baidu` | Baidu | Moli（默认） | 显式 |
| HTTP/RSS | `ddg` | DuckDuckGo | HTTP | 第二层 |
| HTTP/RSS | `bing` | Bing International | RSS | 第二层 |
| HTTP/RSS | `wiki` | Wikipedia | MediaWiki JSON | 第二层 |
| HTTP/RSS | `brave` | Brave Search | HTTP | 无头构建 |
| HTTP/RSS | `sogou` | Sogou | HTTP | 显式 |
| HTTP/RSS | `360` | 360 Search | HTTP | 显式 |
| HTTP/RSS | `bing_cn` | Bing China | RSS | 显式 |
| Native API | `anysearch` | AnySearch | MCP / JSON-RPC 2.0 | 主层 |
| Native API | `tavily` | Tavily | REST | 主层 |
| Native API | `tinyfish` | TinyFish Search | REST | 显式 |
| Native API | `bocha` | Bocha Web Search | REST | 显式 |
| Native API | `aliyun` | Alibaba Cloud IQS | REST | 显式 |
| Native API | `tencent` | Tencent Cloud Search | REST | 显式 |
| Native API | `firecrawl` | Firecrawl Search | REST | 显式 |

HTML 引擎在解析前校验响应结构。CAPTCHA、验证、同意与反机器人页面成为类型化的瞬时 `challenge` 失败。无关的成功页面成为 `invalid_response`，而非虚假空结果。

`bing_cn` 在 `/search` 上请求中国 RSS 文档，并固定 `setlang=zh-CN` 与
`mkt=zh-CN`。不再使用 `cn.bing.com`：该主机对部分客户端会把 `/search`
重定向到首页，随后返回 HTML 而不是结果。

<details>
<summary><strong>原生提供方能力与凭证</strong></summary>

| 提供方 | 无凭证模式 | 丰富字段 |
| --- | --- | --- |
| [AnySearch](https://www.anysearch.com/) | 匿名 | 全文、总数、计时、请求 ID |
| [Tavily](https://www.tavily.com/) | 无密钥头 | 答案、相关性、原始内容、图片、favicon、用量、元数据 |
| [TinyFish](https://www.tinyfish.ai/) | 无 | 分页、时间范围、可选缩略图、新闻与论文分类 |
| [Bocha](https://open.bochaai.com/) | 无 | 时间范围、默认开启的网页摘要、站点图标 |
| [Alibaba Cloud IQS](https://www.aliyun.com/product/iqs) | 无 | 重排分数、可选正文、场景答案、用量 |
| [Tencent Cloud Search](https://cloud.tencent.com/product/wsa) | 无 | 相关性、时间范围、动态摘要、图片 |
| [Firecrawl](https://www.firecrawl.dev/) | 无 | 时间范围、分类、新闻与图片、用量 |

`tinyfish`、`bocha`、`aliyun`、`tencent`、`firecrawl` 需要 API key，不进入默认检索计划。显式选择：

```bash
export TINYFISH_API_KEY="..."
export BOCHA_API_KEY="..."
export ALIYUN_IQS_API_KEY="..."
export TENCENTCLOUD_WSA_APIKEY="..."
export FIRECRAWL_API_KEY="..."
a3s-search "query" --engines tinyfish,bocha,aliyun,tencent,firecrawl
```

AnySearch 和 Tavily 接受可选的 bearer 认证：

```bash
export ANYSEARCH_API_KEY="..."
export TAVILY_API_KEY="..."
export TAVILY_PROJECT="..." # authenticated Tavily requests only
```

在 ACL 中优先使用 `env("VARIABLE")`。凭证从不放入端点 URL，也不从提供方响应体中保留。

AnySearch 适配器通过 MCP `tools/call` 向 `POST https://api.anysearch.com/mcp` 发送单查询 `search` 工具，遵循
[AnySearch Skill v2.1.0](https://github.com/anysearch-ai/anysearch-skill/tree/v2.1.0)。
`batch_search` 与 `extract` 等工作流操作仍留在官方 AnySearch Skill 中。

Tavily 适配器支持深度、主题、直接答案、原始内容、域名过滤、日期边界、国家提升、自动参数、精确匹配、图片、图片描述、favicon、用量与安全搜索。跨字段要求在传输前校验。默认请求纯源文本，以便检索消费者无需二次抓取页面即可检查提供方原生证据。答案、图片与用量在显式请求前保持关闭。在 ACL 中设置 `include_raw_content = "none"`，或使用 `TavilyConfig::with_raw_content(TavilyRawContent::None)` 以退出源文本。

每个原生 API 都实现同一 `SearchProvider` 协议。`ProviderEngine` 是进入级联、排序与 CLI 的唯一适配器。需要凭证的 JSON API 共享同一传输外壳。外壳执行调用、分类传输失败，并在响应离开前用该次凭证封口成功体。厂商模块只提供选项、请求映射、响应映射与错误码分类。结果上限在 Rust 与 ACL 中统一为 `max_results`。厂商线上字段（`count`、`limit`、`Cnt`、`numResults`）留在对应模块内。可选认证与 MCP 编解码不进入这个外壳。

TinyFish 调用 `GET https://api.search.tinyfish.ai`，使用 `X-API-Key`。仅在 `include_thumbnail = true` 时请求缩略图。Bocha 调用 `POST https://api.bochaai.com/v1/web-search`，除非 `summary = false`，否则请求网页摘要。阿里云 IQS 调用 `POST https://cloud-iqs.aliyuncs.com/search/unified`，默认引擎为 `LiteAdvanced`。仅在 `include_main_text = true` 时请求正文。腾讯云搜索调用 `POST https://api.wsa.cloud.tencent.com/SearchPro`，未配置付费 `max_results` 时省略 `Cnt`。Firecrawl 调用 `POST https://api.firecrawl.dev/v2/search`，默认只返回搜索元数据。仅在需要页面正文时设置 `include_markdown = true`；该选项会抓取每条结果并额外计费。

</details>

## 排序与回退

### 秩融合，而非语义打分

聚合器首先对每个引擎响应去重，再跨引擎合并规范化 URL。常见跟踪参数被移除，同时保留源位置、来源、更丰富字段与秩信号。

每个源通过加权倒数秩融合贡献：

```text
engine weight × reciprocal rank × provider-local relevance factor
```

提供方相关性仅在该提供方响应内校准。A3S Search 不假装无关 API 的分数共享同一尺度，也不用手工查询或内容匹配替换它们。

### 惰性操作回退

当既无显式源也无 ACL 源选择时，配套 CLI 使用此默认计划：

```text
01  native API     anysearch + tavily
        ↓ continue only when structural requirements are not met
02  HTTP / RSS     wiki + ddg + bing
        ↓ continue only when structural requirements are not met
03  headless       brave_browser + bing_browser through Moli
        ↓
    results + cascade receipt V2
```

所有层共享一个端到端截止时间——默认 20 秒。层内引擎并发运行，成功输出在无关失败后仍保留。昂贵的下层仅在需要时构造。

`RetrievalHealth` 仅观察非语义事实：

- 可用与无效 HTTP(S) 结果计数；
- 不同的规范化主机与贡献引擎；
- 跨引擎 URL 共识；
- 类型化的成功、空、失败、超时、拒绝与熔断打开计数。

`RetrievalRequirements::for_limit` 对单结果请求要求一个逻辑源，对多结果请求要求两个独立逻辑源。同一上游的浏览器与 HTTP 变体保留一个源身份，因此传输重复不能停止回退。仅有一个成功源支撑的层因此会在不检查查询或结果文本的情况下继续到下一配置的传输。

CLI 在渲染其调用方可见的 Top-K 之前应用 `select_structural_window`。已健康的排序前缀不变。若完整候选集满足声明结构但前缀不满足，则有界的最小替换搜索选择高排名的可行窗口并保留相对秩。此选择仅使用 URL 有效性、规范化主机、逻辑源来源与共识；从不读取查询、标题、摘要、语言、出版方或主题。JSON 输出暴露 `visible_retrieval_health` 与 `visible_retrieval_requirements_met`，以便外部验证器可独立拒绝全集与可见健康之间的不匹配。

嵌入式调用方可提供不同的 `RetrievalRequirements`，或通过 `push_tier_with_decision` 记录不透明外部决策。收据将决策来源标记为 `external_policy`；Search 不复现或验证其语义推理。

### 可验证的结构收据

`finish_with_tier_plan` 返回 `SearchCascadeOutcomeV2`，它绑定：

- 完整的类型化查询；
- 配置的层计划与已执行前缀；
- 每个结构健康观察、决策与决策权威；
- 有序结果与丰富提供方字段；
- 失败、报告、结果、计数与计时元数据。

`receipt_binding()` 对已校验收据计算域分离的规范 SHA-256。与可信摘要比较时可检测替换。它不证明谁运行了搜索，也不证明结果是否为真；真实性仍需要可信签名或摘要日志。

## 扩展引擎

当源返回普通网页或媒体结果时使用 `Engine`：

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

当原生 API 暴露能力、就绪、丰富输出、用量或结构化报告时使用 `SearchProvider`：

```rust
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn readiness(&self) -> ProviderReadiness;
    async fn search(&self, request: &ProviderRequest)
        -> Result<ProviderResponse>;
}

let engine = ProviderEngine::new(my_provider);
search.add_engine(engine);
```

提供方输出在到达调用方之前穿越有界规范化边界。直接答案、建议、全文、图片、相关性、用量与报告保持为一等字段，而不是变成合成网页结果。

## 用 ACL 配置

使用 A3S Agent Configuration Language 进行源选择、凭证、超时、排序与提供方特定控制：

```acl
timeout {
  value = 20
}

ranking {
  rrf_rank_constant       = 60
  native_relevance_weight = 0.2
}

engine "brave_browser" {
  enabled = true
  weight  = 1.2
  timeout = 12
}

provider "anysearch" {
  enabled     = true
  api_key     = env("ANYSEARCH_API_KEY")
  max_results = 10
}

provider "tavily" {
  enabled             = true
  api_key             = env("TAVILY_API_KEY")
  project             = env("TAVILY_PROJECT")
  search_depth        = "advanced"
  chunks_per_source   = 3
  max_results         = 10
  include_answer      = "advanced"
  include_raw_content = "markdown"
  include_images      = true
  include_favicon     = true
}

provider "tinyfish" {
  api_key     = env("TINYFISH_API_KEY")
  domain_type = "web"
}

provider "bocha" {
  api_key     = env("BOCHA_API_KEY")
  max_results = 8
  summary     = true
}

provider "aliyun" {
  api_key     = env("ALIYUN_IQS_API_KEY")
  engine_type = "lite-advanced"
  max_results = 8
}

provider "tencent" {
  api_key = env("TENCENTCLOUD_WSA_APIKEY")
}

provider "firecrawl" {
  api_key     = env("FIRECRAWL_API_KEY")
  max_results = 8
  country     = "US"
}
```

```bash
a3s-search --config search.acl engines
a3s-search "query" --config search.acl --format json
```

ACL 解析拒绝未知排序字段、不安全端点、无效范围、重复源块、无效提供方组合，以及同时声明为引擎与提供方的源。含凭证的调试输出会被脱敏。

参见完整类型化表面：
[`SearchConfig`](https://docs.rs/a3s-search/latest/a3s_search/struct.SearchConfig.html)、
[`AnySearchConfig`](https://docs.rs/a3s-search/latest/a3s_search/providers/struct.AnySearchConfig.html)
与 [`TavilyConfig`](https://docs.rs/a3s-search/latest/a3s_search/providers/struct.TavilyConfig.html)。

## 无全局策略的可靠性

| 控制 | 边界 |
| --- | --- |
| `HealthMonitor` | 兼容的每 `Search` 连续失败挂起 |
| `CircuitBreaker` | 共享的 closed/open/half-open 源状态，含失败、空、慢调用与 `Retry-After` 策略 |
| `Bulkhead` | 有界的每引擎在途工作与队列等待 |
| `RetryBudget` | 限制重试放大的令牌桶 |
| `SearchCoalescer` | 对相同重叠请求的取消安全共享；从不是缓存 |
| `Metrics` | 内存中的成功/失败计数器与 p50/p95/p99 延迟 |

跨长期存活的 `Search` 实例共享兼容控制：

```rust,no_run
use a3s_search::{Bulkhead, CircuitBreaker, Search, SearchCoalescer};

let search = Search::new()
    .with_bulkhead(Bulkhead::default())
    .with_circuit_breaker(CircuitBreaker::default())
    .with_request_coalescer(SearchCoalescer::default());
```

将共享状态限定到兼容的租户、凭证、端点、代理、安全搜索设置、新鲜度要求与排序策略。嵌入式应用显式拥有其跨请求历史。

短生命周期 CLI 进程仅跨调用保留类型化交互挑战熔断。版本化、加锁的状态文件包含源 shortcut、重试截止时间与有界驱逐计数——从不包含查询、结果内容、凭证或语义判断。Linux 使用 XDG 状态目录；macOS 与 Windows 使用其平台本地应用数据目录。主机需要隔离状态范围时，将 `A3S_SEARCH_STATE_DIR` 设为绝对目录。单向传输范围摘要在不保留代理凭证的情况下分离 direct、proxy、Moli、Chrome 与 Lightpanda 路由。挑战、限流与终端提供方失败都会立即打开进程内熔断；仅与凭证无关的挑战状态跨越进程边界。过期条目允许一次半开探测并保留指数退避。

### 浏览器功能边界

默认 `headless` Cargo feature 包含类型化的 Moli CLI 渲染器。它通过 `a3s-use-browser::PageRenderer` 契约调用上游 `moli fetch --dump html` 命令，并保持子进程有界、可取消且隔离：

| 构建 | 运行时行为 |
| --- | --- |
| default / `headless` / `moli` | 从 `A3S_MOLI_EXECUTABLE`、PATH 或官方安装位置发现 Moli |
| `--browser chrome` | 使用显式 Chrome/Chromium A3S Browser 后端 |
| `lightpanda` | 将 Lightpanda 添加为显式后端；从不隐式选择 |
| `--no-default-features` | 移除浏览器/CDP 依赖栈 |

从 [`lexmount/moli`](https://github.com/lexmount/moli) 安装 Moli，或将 `A3S_MOLI_EXECUTABLE` 设为现有可执行文件。Moli 支持 macOS、Linux 与 Windows。Chrome/Chromium 仍可作为显式兼容后端；Lightpanda 在 Windows 主机上需要 WSL2。

Moli 适配器拥有可执行文件发现、进程生命周期、有界并行与清理。Search 拥有搜索 URL、等待条件、HTML 校验、有界重试与搜索特定指标。

```bash
cargo run -- "query" --browser moli
cargo run -- "query" --browser chrome
cargo run --features lightpanda -- "query" --browser lightpanda
```

<details>
<summary><strong>全文富化、代理与指标</strong></summary>

原生提供方可直接返回 `full_text`；AnySearch 会请求它，Tavily 默认请求纯源文本。仅摘要的结果可通过同一 `PageFetcher` 抽象富化：

```rust
use a3s_search::{enrich_full_text, HttpFetcher, PageFetcher, SearchResults};
use std::{sync::Arc, time::Duration};

async fn enrich(results: &mut SearchResults) {
    let fetcher: Arc<dyn PageFetcher> = Arc::new(HttpFetcher::new());
    enrich_full_text(results, fetcher, 8, Duration::from_secs(10)).await;
}
```

失败的富化保留摘要。全文仍是调用方数据，从不改变 A3S Search 内部的秩融合或级联决策。

常规引擎支持静态代理或轮换 `ProxyPool`。提供方 API 使用独立的有界 HTTP 客户端，不继承抓取代理。将一个 `Metrics` 注册表附加到 `Search`、`HttpFetcher` 或 `BrowserFetcher`，以获取请求计数、失败类别与延迟百分位。

</details>

## 开发与发布保障

从 Search 仓库运行检查，而不是从 A3S monorepo 根目录：

```bash
cargo fmt --all -- --check
cargo test --no-default-features --locked
cargo test --all-features --locked
cargo clippy --all-targets --no-default-features --locked -- -D warnings
cargo clippy --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
scripts/test-release-package.sh
scripts/test-freeze-crate.sh
# Download and verify the pinned Moli runtime, then run its local fixture.
A3S_MOLI_EXECUTABLE="$(scripts/install-moli-ci.sh)" \
  cargo test --features moli --test integration -- moli --nocapture
```

确定性套件覆盖协议、认证、规范化、去重、秩融合、收据、故障注入、取消、并发与资源排空。实时可用性隔离在显式有界 canary 与 soak 测试中。

<details>
<summary><strong>发布门与可选可靠性测试</strong></summary>

显式运行有界可靠性 soak：

```bash
A3S_SEARCH_SOAK_SECONDS=300 \
  cargo test --release --test soak deterministic_reliability_soak \
  -- --ignored --nocapture --exact
```

发布任务运行 Rust 契约套件、冻结确切 `.crate` 字节，并在受保护环境可发布前复现该包。缺失、失败、取消或不匹配的包证据会使 crates.io、GitHub Release 与 Homebrew 失败即关闭。有界实时 canary 仍是显式运维工具：上游限流与打开的熔断作为审计遥测保留，而其结果由终端失败、结构充分性、回退行为、重试放大、延迟、收据完整性与资源释放决定。预发布标签可发布 GitHub CLI 归档，但从不发布 crates.io 或 Homebrew 产物。

</details>

## 捆绑的 agent Skill

每个平台发布归档包含 CLI 加上一个小型 agent 集成：

```text
a3s-search
skills/a3s-search/SKILL.md
skills/a3s-search/agents/openai.yaml
```

该 Skill 引导编码代理完成源选择、结构化检索、凭证、ACL、结构收据与部分失败。它明确将语义评估留给调用代理。

## A3S 生态

- [A3S](https://github.com/A3S-Lab/a3s) — 平台与组件入口
- [A3S Code](https://github.com/A3S-Lab/Code) — 受治理的编码代理运行时
- [Moli](https://github.com/lexmount/moli) — 默认独立无头浏览器运行时
- [A3S Browser](https://github.com/A3S-Lab/Browser) — 类型化渲染契约与显式 Chrome 后端
- [A3S Science](https://github.com/A3S-Lab/Science) — 检索内核之上的研究工作流

## 贡献

欢迎 issue 与聚焦的 pull request。将语义评估留在检索内核之外，为行为变更添加回归覆盖，并在提交前运行 no-default 与 all-feature 两组检查。

## 许可证

[MIT](./LICENSE)
