//! Lazy browser-tier construction and cleanup.

use std::time::Instant;

use a3s_search::{EngineFailure, SearchResults};

#[cfg(feature = "headless")]
use std::{sync::Arc, time::Duration};

#[cfg(feature = "headless")]
use futures::future::join_all;

#[cfg(feature = "headless")]
use a3s_search::{
    a3s_use_browser::{BrowserPool, BrowserPoolConfig, PageRenderer},
    engines::{Baidu, Google},
    BrowserFetcher, Engine, PageFetcher, RetryBudget, WaitStrategy,
};

#[cfg(all(feature = "headless", feature = "lightpanda"))]
use a3s_search::a3s_use_browser::{BrowserBackend, BrowserProvider};

#[cfg(feature = "headless")]
use super::tier_timeout;
#[cfg(feature = "headless")]
use super::{configured_search, deadline_exhausted, execute_search_tier, record_disabled_engine};
use super::{CascadeRequest, SharedControls};
#[cfg(feature = "headless")]
use crate::configured_engine_config;

#[cfg(feature = "headless")]
pub(super) async fn execute_headless_tier(
    request: &CascadeRequest<'_>,
    controls: &SharedControls,
    shortcuts: &[String],
    deadline: Instant,
    remaining_tiers: usize,
) -> SearchResults {
    if deadline.saturating_duration_since(Instant::now()).is_zero() {
        return deadline_exhausted("headless");
    }

    let pool_config = browser_pool_config(request.proxy, deadline).await;
    #[cfg(feature = "lightpanda")]
    let isolate_pools = pool_config.provider.backend() == BrowserBackend::Lightpanda;
    #[cfg(not(feature = "lightpanda"))]
    let isolate_pools = false;
    let shared_pool = (!isolate_pools).then(|| Arc::new(BrowserPool::new(pool_config.clone())));
    let mut cleanup = BrowserPoolCleanup::default();
    if let Some(pool) = shared_pool.as_ref() {
        cleanup.track(Arc::clone(pool));
    }
    let render_budget = tier_timeout(
        deadline.saturating_duration_since(Instant::now()),
        remaining_tiers,
    )
    .min(Duration::from_secs(5));
    let retry_budget = RetryBudget::default();
    let mut search = configured_search(request.config, controls);
    let mut setup_results = SearchResults::new();

    for shortcut in shortcuts {
        if !record_disabled_engine(&mut setup_results, request.config, shortcut) {
            continue;
        }
        // Lightpanda currently supports one reliable target per process. Keep
        // engines isolated there while sharing one Chrome process elsewhere.
        let pool = shared_pool.clone().unwrap_or_else(|| {
            let pool = Arc::new(BrowserPool::new(pool_config.clone()));
            cleanup.track(Arc::clone(&pool));
            pool
        });
        let renderer: Arc<dyn PageRenderer> = pool;
        let fetcher = || -> Arc<dyn PageFetcher> {
            Arc::new(
                BrowserFetcher::from_renderer(Arc::clone(&renderer))
                    .with_wait(WaitStrategy::Load)
                    .with_timeout(render_budget)
                    .with_total_timeout(render_budget)
                    .with_retries(1, 100)
                    .with_retry_budget(retry_budget.clone()),
            )
        };
        match shortcut.as_str() {
            "g" => {
                let engine = Google::new(fetcher());
                let engine_config =
                    configured_engine_config(request.config, engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "baidu" => {
                let engine = Baidu::new(fetcher());
                let engine_config =
                    configured_engine_config(request.config, engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            _ => setup_results.add_failure(EngineFailure::new(
                shortcut,
                "unsupported_engine",
                "engine is not available in the headless tier",
            )),
        }
    }

    let results = execute_search_tier(
        search,
        setup_results,
        &request.query,
        "headless",
        deadline,
        remaining_tiers,
    )
    .await;
    cleanup.shutdown(deadline).await;
    results
}

#[cfg(not(feature = "headless"))]
pub(super) async fn execute_headless_tier(
    _request: &CascadeRequest<'_>,
    _controls: &SharedControls,
    shortcuts: &[String],
    _deadline: Instant,
    _remaining_tiers: usize,
) -> SearchResults {
    let mut results = SearchResults::new();
    for shortcut in shortcuts {
        results.add_failure(EngineFailure::new(
            shortcut,
            "headless_unavailable",
            "rebuild a3s-search with the headless feature",
        ));
    }
    results
}

#[cfg(feature = "headless")]
async fn browser_pool_config(proxy: Option<&str>, deadline: Instant) -> BrowserPoolConfig {
    let config = BrowserPoolConfig {
        proxy_url: proxy.map(str::to_string),
        ..BrowserPoolConfig::default()
    };

    #[cfg(feature = "lightpanda")]
    {
        let fallback = config.clone();
        let detection = tokio::task::spawn_blocking(move || detect_browser_pool_config(config));
        match tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            detection,
        )
        .await
        {
            Ok(Ok(config)) => config,
            Ok(Err(_)) | Err(_) => fallback,
        }
    }

    #[cfg(not(feature = "lightpanda"))]
    {
        config
    }
}

#[cfg(all(feature = "headless", feature = "lightpanda"))]
fn detect_browser_pool_config(mut config: BrowserPoolConfig) -> BrowserPoolConfig {
    use a3s_search::a3s_use_browser::{browser_statuses, ManagedBrowser};

    if let Some(status) = browser_statuses()
        .into_iter()
        .find(|status| status.available && status.path.is_some())
    {
        if let Some(path) = status.path {
            config.provider = match status.browser {
                ManagedBrowser::Chrome => BrowserProvider::ChromeExecutable(path),
                ManagedBrowser::Lightpanda => BrowserProvider::LightpandaExecutable(path),
            };
        }
    }
    config
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
