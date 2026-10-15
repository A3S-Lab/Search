//! Lazy browser-tier construction and cleanup.

use std::path::PathBuf;
use std::time::Instant;

use futures::future::{BoxFuture, Shared};

use crate::{EngineFailure, EngineOutcomeKind, SearchReport, SearchResults};

use super::run::{CascadeControls, CascadeRequest};

const RETRY_OBSERVATION_SCHEMA: &str = "a3s/search-retry-observation/v1";

fn add_retry_observation(
    results: &mut SearchResults,
    retry_attempts: u64,
    maximum_retries_per_request: u32,
) {
    let initial_attempts = results
        .outcomes()
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.kind,
                EngineOutcomeKind::Success
                    | EngineOutcomeKind::Empty
                    | EngineOutcomeKind::Failure
                    | EngineOutcomeKind::Timeout
            )
        })
        .count();
    results.add_report(
        SearchReport::new("a3s-search/browser-retry")
            .with_metadata("schema", RETRY_OBSERVATION_SCHEMA)
            .with_metadata("initial_attempts", initial_attempts)
            .with_metadata("retry_attempts", retry_attempts)
            .with_metadata("maximum_retries_per_request", maximum_retries_per_request),
    );
}

/// Browser backend used by the product headless tier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeadlessBrowser {
    /// Moli's standalone, JavaScript-capable headless browser.
    #[default]
    Moli,
    /// Installed Chrome, Chromium, or a previously managed Chrome runtime.
    Chrome,
    /// Explicit Lightpanda runtime.
    Lightpanda,
}

impl HeadlessBrowser {
    /// Stable backend identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Moli => "moli",
            Self::Chrome => "chrome",
            Self::Lightpanda => "lightpanda",
        }
    }
}

/// Browser runtime the host already provisioned. A3S Search never downloads
/// or installs a browser for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadlessRuntime {
    /// Exact executable for the selected backend; discovery when unset.
    pub executable: Option<PathBuf>,
    /// Concurrent page limit; the backend default when unset.
    pub max_tabs: Option<usize>,
    /// Extra launch arguments for Chrome-compatible backends.
    pub launch_args: Vec<String>,
}

/// A runtime the host may still be provisioning. The cascade awaits it only
/// when the headless tier runs, so provisioning can overlap earlier tiers.
pub type PendingHeadlessRuntime = Shared<BoxFuture<'static, Result<HeadlessRuntime, String>>>;

#[cfg(feature = "headless")]
use std::{sync::Arc, time::Duration};

#[cfg(feature = "headless")]
use futures::future::join_all;

#[cfg(feature = "headless")]
use crate::a3s_use_browser::{BrowserPool, BrowserPoolConfig, BrowserProvider, PageRenderer};
#[cfg(feature = "headless")]
use crate::engines::{Baidu, BingBrowser, BraveBrowser, Google};
#[cfg(feature = "headless")]
use crate::product::sources::configured_engine_config;
#[cfg(feature = "headless")]
use crate::retry::RetryBudget;
#[cfg(feature = "headless")]
use crate::{BrowserFetcher, Engine, MoliPool, MoliPoolConfig, PageFetcher, Search, WaitStrategy};

#[cfg(feature = "headless")]
use super::run::{
    configured_search, deadline_exhausted, execute_search_tier, record_disabled_engine,
};

#[cfg(feature = "headless")]
pub(super) async fn execute_headless_tier(
    request: &CascadeRequest<'_>,
    controls: &CascadeControls,
    shortcuts: &[String],
    deadline: Instant,
    remaining_tiers: usize,
) -> SearchResults {
    if deadline.saturating_duration_since(Instant::now()).is_zero() {
        let mut results = deadline_exhausted("headless");
        add_retry_observation(&mut results, 0, request.browser_max_retries);
        return results;
    }

    let waited = Instant::now();
    let runtime = match controls.headless_runtime.clone() {
        Some(pending) => match pending.await {
            Ok(runtime) => runtime,
            Err(message) => {
                let mut results = SearchResults::new();
                results.add_failure(
                    EngineFailure::new(
                        request.browser.as_str(),
                        "headless_backend_unavailable",
                        message,
                    )
                    .with_transient(true),
                );
                add_retry_observation(&mut results, 0, request.browser_max_retries);
                return results;
            }
        },
        None => HeadlessRuntime::default(),
    };
    // Host provisioning runs on its own budget, not the retrieval deadline.
    let deadline = deadline.checked_add(waited.elapsed()).unwrap_or(deadline);
    let tier = HeadlessTier {
        request,
        controls,
        runtime: &runtime,
        shortcuts,
        deadline,
        remaining_tiers,
    };

    if request.browser == HeadlessBrowser::Moli {
        return execute_moli_headless_tier(tier).await;
    }

    execute_browser_headless_tier(tier).await
}

#[cfg(feature = "headless")]
struct HeadlessTier<'a> {
    request: &'a CascadeRequest<'a>,
    controls: &'a CascadeControls,
    runtime: &'a HeadlessRuntime,
    shortcuts: &'a [String],
    deadline: Instant,
    remaining_tiers: usize,
}

#[cfg(feature = "headless")]
async fn execute_moli_headless_tier(tier: HeadlessTier<'_>) -> SearchResults {
    let HeadlessTier {
        request,
        controls,
        runtime,
        shortcuts,
        deadline,
        remaining_tiers,
    } = tier;
    let pool = Arc::new(MoliPool::new(moli_pool_config(request.proxy, runtime)));
    if let Err(error) = pool.warm_up() {
        let mut results = SearchResults::new();
        results.add_failure(moli_setup_failure(error));
        add_retry_observation(&mut results, 0, request.browser_max_retries);
        return results;
    }

    let render_budget = headless_render_budget(
        deadline.saturating_duration_since(Instant::now()),
        remaining_tiers,
    );
    let retry_budget: RetryBudget = controls.retry_budget.clone().unwrap_or_default();
    let retries_before = retry_budget.snapshot().admitted_retries;
    let mut search = configured_search(request.config, controls);
    let mut setup_results = SearchResults::new();
    let renderer: Arc<dyn PageRenderer> = pool.clone();
    configure_headless_engines(
        &mut search,
        &mut setup_results,
        request,
        shortcuts,
        renderer,
        render_budget,
        &retry_budget,
    );

    let mut results = execute_search_tier(
        search,
        setup_results,
        &request.query,
        "headless",
        render_budget,
    )
    .await;
    pool.shutdown();
    let retries = retry_budget
        .snapshot()
        .admitted_retries
        .saturating_sub(retries_before);
    add_retry_observation(&mut results, retries, request.browser_max_retries);
    results
}

#[cfg(feature = "headless")]
async fn execute_browser_headless_tier(tier: HeadlessTier<'_>) -> SearchResults {
    let HeadlessTier {
        request,
        controls,
        runtime,
        shortcuts,
        deadline,
        remaining_tiers,
    } = tier;
    let pool_config = match browser_pool_config(request.proxy, request.browser, runtime) {
        Ok(config) => config,
        Err(failure) => {
            let mut results = SearchResults::new();
            results.add_failure(failure);
            add_retry_observation(&mut results, 0, request.browser_max_retries);
            return results;
        }
    };
    let isolate_pools = request.browser == HeadlessBrowser::Lightpanda;
    let shared_pool = (!isolate_pools).then(|| Arc::new(BrowserPool::new(pool_config.clone())));
    let mut cleanup = BrowserPoolCleanup::default();
    if let Some(pool) = shared_pool.as_ref() {
        cleanup.track(Arc::clone(pool));
    }
    let render_budget = headless_render_budget(
        deadline.saturating_duration_since(Instant::now()),
        remaining_tiers,
    );
    let retry_budget: RetryBudget = controls.retry_budget.clone().unwrap_or_default();
    let retries_before = retry_budget.snapshot().admitted_retries;
    let mut search = configured_search(request.config, controls);
    let mut setup_results = SearchResults::new();
    let renderer_for_shortcuts = shared_pool.clone();
    for shortcut in shortcuts {
        if !record_disabled_engine(&mut setup_results, request.config, shortcut) {
            continue;
        }
        // Lightpanda currently supports one reliable target per process. Keep
        // engines isolated there while sharing one Chrome process elsewhere.
        let pool = renderer_for_shortcuts.clone().unwrap_or_else(|| {
            let pool = Arc::new(BrowserPool::new(pool_config.clone()));
            cleanup.track(Arc::clone(&pool));
            pool
        });
        let renderer: Arc<dyn PageRenderer> = pool;
        configure_one_headless_engine(
            &mut search,
            &mut setup_results,
            request,
            shortcut,
            renderer,
            render_budget,
            &retry_budget,
        );
    }

    let mut results = execute_search_tier(
        search,
        setup_results,
        &request.query,
        "headless",
        render_budget,
    )
    .await;
    cleanup.shutdown(deadline).await;
    let retries = retry_budget
        .snapshot()
        .admitted_retries
        .saturating_sub(retries_before);
    add_retry_observation(&mut results, retries, request.browser_max_retries);
    results
}

#[cfg(feature = "headless")]
fn configure_headless_engines(
    search: &mut Search,
    setup_results: &mut SearchResults,
    request: &CascadeRequest<'_>,
    shortcuts: &[String],
    renderer: Arc<dyn PageRenderer>,
    render_budget: Duration,
    retry_budget: &RetryBudget,
) {
    for shortcut in shortcuts {
        if !record_disabled_engine(setup_results, request.config, shortcut) {
            continue;
        }
        configure_one_headless_engine(
            search,
            setup_results,
            request,
            shortcut,
            Arc::clone(&renderer),
            render_budget,
            retry_budget,
        );
    }
}

#[cfg(feature = "headless")]
fn configure_one_headless_engine(
    search: &mut Search,
    setup_results: &mut SearchResults,
    request: &CascadeRequest<'_>,
    shortcut: &str,
    renderer: Arc<dyn PageRenderer>,
    render_budget: Duration,
    retry_budget: &RetryBudget,
) {
    let fetcher = || -> Arc<dyn PageFetcher> {
        Arc::new(
            BrowserFetcher::from_renderer(Arc::clone(&renderer))
                .with_wait(headless_wait_strategy())
                .with_timeout(render_budget)
                .with_total_timeout(render_budget)
                .with_retries(request.browser_max_retries, 100)
                .with_retry_budget(retry_budget.clone()),
        )
    };
    match shortcut {
        "g" => {
            let engine = Google::new(fetcher());
            let engine_config = configured_engine_config(request.config, engine.config().clone());
            search.add_engine(engine.with_config(engine_config));
        }
        "baidu" => {
            let engine = Baidu::new(fetcher());
            let engine_config = configured_engine_config(request.config, engine.config().clone());
            search.add_engine(engine.with_config(engine_config));
        }
        "bing_browser" => {
            let engine = BingBrowser::new(fetcher());
            let engine_config = configured_engine_config(request.config, engine.config().clone());
            search.add_engine(engine.with_config(engine_config));
        }
        "brave_browser" => {
            let engine = BraveBrowser::new(fetcher());
            let engine_config = configured_engine_config(request.config, engine.config().clone());
            search.add_engine(engine.with_config(engine_config));
        }
        _ => setup_results.add_failure(EngineFailure::new(
            shortcut,
            "unsupported_engine",
            "engine is not available in the headless tier",
        )),
    }
}

#[cfg(not(feature = "headless"))]
pub(super) async fn execute_headless_tier(
    request: &CascadeRequest<'_>,
    _controls: &CascadeControls,
    shortcuts: &[String],
    _deadline: Instant,
    _remaining_tiers: usize,
) -> SearchResults {
    let mut results = SearchResults::new();
    for shortcut in shortcuts {
        results.add_failure(EngineFailure::new(
            shortcut,
            "headless_unavailable",
            format!(
                "the {} backend requires a3s-search to be built with the headless feature",
                request.browser.as_str()
            ),
        ));
    }
    add_retry_observation(&mut results, 0, request.browser_max_retries);
    results
}

#[cfg(feature = "headless")]
fn browser_pool_config(
    proxy: Option<&str>,
    browser: HeadlessBrowser,
    runtime: &HeadlessRuntime,
) -> Result<BrowserPoolConfig, EngineFailure> {
    let provider = match browser {
        HeadlessBrowser::Moli => {
            return Err(EngineFailure::new(
                "moli",
                "headless_backend_unavailable",
                "Moli is configured through its standalone renderer, not BrowserPool",
            ));
        }
        HeadlessBrowser::Chrome => runtime.executable.clone().map_or(
            BrowserProvider::DiscoveredChrome,
            BrowserProvider::ChromeExecutable,
        ),
        HeadlessBrowser::Lightpanda => lightpanda_provider(runtime.executable.clone())?,
    };
    let defaults = BrowserPoolConfig::default();
    Ok(BrowserPoolConfig {
        proxy_url: proxy.map(str::to_string),
        provider,
        max_tabs: runtime.max_tabs.unwrap_or(defaults.max_tabs),
        launch_args: runtime.launch_args.clone(),
        ..defaults
    })
}

#[cfg(feature = "headless")]
fn moli_pool_config(proxy: Option<&str>, runtime: &HeadlessRuntime) -> MoliPoolConfig {
    let defaults = MoliPoolConfig::default();
    MoliPoolConfig {
        executable: runtime.executable.clone(),
        proxy_url: proxy.map(str::to_string),
        max_tabs: runtime.max_tabs.unwrap_or(defaults.max_tabs),
        ..defaults
    }
}

#[cfg(feature = "headless")]
fn moli_setup_failure(error: crate::a3s_use_browser::UseError) -> EngineFailure {
    let mut message = format!("Moli headless backend is unavailable: {}", error.message);
    if let Some(suggestion) = error.suggestion {
        message.push_str(" Suggestion: ");
        message.push_str(&suggestion);
    }
    EngineFailure::new("moli", "headless_backend_unavailable", message)
}

#[cfg(all(feature = "headless", feature = "lightpanda"))]
fn lightpanda_provider(executable: Option<PathBuf>) -> Result<BrowserProvider, EngineFailure> {
    Ok(executable.map_or(
        BrowserProvider::DiscoveredLightpanda,
        BrowserProvider::LightpandaExecutable,
    ))
}

#[cfg(all(feature = "headless", not(feature = "lightpanda")))]
fn lightpanda_provider(_executable: Option<PathBuf>) -> Result<BrowserProvider, EngineFailure> {
    Err(EngineFailure::new(
        "lightpanda",
        "headless_backend_unavailable",
        "Lightpanda is an explicit optional backend; rebuild with the lightpanda Cargo feature",
    ))
}

#[cfg(feature = "headless")]
fn headless_wait_strategy() -> WaitStrategy {
    WaitStrategy::Load
}

#[cfg(feature = "headless")]
fn headless_render_budget(remaining: Duration, remaining_tiers: usize) -> Duration {
    if remaining_tiers == 0 {
        remaining
    } else {
        remaining / 2
    }
}

#[cfg(feature = "headless")]
#[derive(Default)]
struct BrowserPoolCleanup {
    pools: Vec<Arc<BrowserPool>>,
}

#[cfg(feature = "headless")]
impl BrowserPoolCleanup {
    fn track(&mut self, pool: Arc<BrowserPool>) {
        if !self.pools.iter().any(|current| Arc::ptr_eq(current, &pool)) {
            self.pools.push(pool);
        }
    }

    async fn shutdown(&mut self, deadline: Instant) {
        let tasks = self
            .pools
            .drain(..)
            .map(|pool| tokio::spawn(async move { pool.shutdown().await }))
            .collect::<Vec<_>>();
        if tasks.is_empty() {
            return;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = tokio::time::timeout(remaining, join_all(tasks)).await;
    }
}

#[cfg(feature = "headless")]
impl Drop for BrowserPoolCleanup {
    fn drop(&mut self) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            for pool in self.pools.drain(..) {
                runtime.spawn(async move {
                    pool.shutdown().await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_browser_default_is_moli() {
        assert_eq!(HeadlessBrowser::default(), HeadlessBrowser::Moli);
        assert_eq!(HeadlessBrowser::default().as_str(), "moli");
    }

    #[test]
    fn retry_observation_is_structured_and_counts_only_upstream_attempts() {
        let mut results: SearchResults = serde_json::from_value(serde_json::json!({
            "results": [],
            "suggestions": [],
            "answers": [],
            "images": [],
            "errors": [],
            "failures": [],
            "reports": [],
            "outcomes": [
                {
                    "engine": "Google",
                    "shortcut": "g",
                    "kind": "success",
                    "result_count": 1,
                    "duration_ms": 10
                },
                {
                    "engine": "Baidu",
                    "shortcut": "baidu",
                    "kind": "circuit_open",
                    "result_count": 0,
                    "duration_ms": 0,
                    "failure": {
                        "engine": "Baidu",
                        "kind": "circuit_open",
                        "message": "open",
                        "transient": true
                    }
                }
            ],
            "count": 0,
            "duration_ms": 10
        }))
        .unwrap();

        add_retry_observation(&mut results, 1, 2);

        let metadata = &results.reports()[0].metadata;
        assert_eq!(
            metadata["schema"],
            serde_json::json!("a3s/search-retry-observation/v1")
        );
        assert_eq!(metadata["initial_attempts"], serde_json::json!(1));
        assert_eq!(metadata["retry_attempts"], serde_json::json!(1));
        assert_eq!(
            metadata["maximum_retries_per_request"],
            serde_json::json!(2)
        );
    }

    #[cfg(feature = "headless")]
    #[test]
    fn default_moli_pool_preserves_the_cli_proxy() {
        let config = moli_pool_config(Some("http://127.0.0.1:8080"), &HeadlessRuntime::default());

        assert_eq!(config.proxy_url.as_deref(), Some("http://127.0.0.1:8080"));
        assert!(config.executable.is_none());
    }

    #[cfg(feature = "headless")]
    #[test]
    fn explicit_chrome_pool_is_pinned_to_discovery() {
        let config = browser_pool_config(
            Some("http://127.0.0.1:8080"),
            HeadlessBrowser::Chrome,
            &HeadlessRuntime::default(),
        )
        .expect("Chrome pool configuration");

        assert!(matches!(config.provider, BrowserProvider::DiscoveredChrome));
        assert_eq!(config.proxy_url.as_deref(), Some("http://127.0.0.1:8080"));
    }

    #[cfg(feature = "headless")]
    #[test]
    fn host_prepared_runtime_reaches_both_pool_kinds() {
        let runtime = HeadlessRuntime {
            executable: Some(PathBuf::from("/opt/host/browser")),
            max_tabs: Some(2),
            launch_args: vec!["--lang=en".to_string()],
        };

        let moli = moli_pool_config(None, &runtime);
        assert_eq!(moli.executable, runtime.executable);
        assert_eq!(moli.max_tabs, 2);

        let chrome = browser_pool_config(None, HeadlessBrowser::Chrome, &runtime)
            .expect("Chrome pool configuration");
        assert_eq!(
            chrome.provider,
            BrowserProvider::ChromeExecutable(PathBuf::from("/opt/host/browser"))
        );
        assert_eq!(chrome.max_tabs, 2);
        assert_eq!(chrome.launch_args, ["--lang=en"]);
    }

    #[cfg(feature = "headless")]
    #[test]
    fn headless_search_readiness_does_not_depend_on_provider_dom_selectors() {
        assert!(matches!(headless_wait_strategy(), WaitStrategy::Load));
    }

    #[cfg(feature = "headless")]
    #[test]
    fn final_headless_tier_can_use_the_complete_remaining_budget() {
        assert_eq!(
            headless_render_budget(Duration::from_secs(15), 0),
            Duration::from_secs(15)
        );
        assert_eq!(
            headless_render_budget(Duration::from_secs(15), 2),
            Duration::from_millis(7_500)
        );
    }

    #[cfg(all(feature = "headless", not(feature = "lightpanda")))]
    #[test]
    fn lightpanda_requires_explicit_cargo_feature() {
        let failure = browser_pool_config(
            None,
            HeadlessBrowser::Lightpanda,
            &HeadlessRuntime::default(),
        )
        .expect_err("Lightpanda must not be implicit in a default build");

        assert_eq!(failure.kind, "headless_backend_unavailable");
    }

    #[cfg(all(feature = "headless", feature = "lightpanda"))]
    #[test]
    fn explicit_lightpanda_selection_uses_lightpanda_provider() {
        let config = browser_pool_config(
            None,
            HeadlessBrowser::Lightpanda,
            &HeadlessRuntime::default(),
        )
        .expect("compiled Lightpanda pool configuration");

        assert!(matches!(
            config.provider,
            BrowserProvider::DiscoveredLightpanda
        ));
    }
}
