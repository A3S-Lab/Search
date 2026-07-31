//! Lazy browser-tier construction and cleanup.

use std::time::Instant;

use a3s_search::{EngineFailure, SearchResults};

/// Browser backend used by the CLI headless tier.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum HeadlessBrowser {
    /// Installed Chrome, Chromium, or a previously managed Chrome runtime.
    #[default]
    Chrome,
    /// Explicit Lightpanda runtime.
    Lightpanda,
}

#[cfg(not(feature = "headless"))]
impl HeadlessBrowser {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Lightpanda => "lightpanda",
        }
    }
}

#[cfg(feature = "headless")]
use std::{sync::Arc, time::Duration};

#[cfg(feature = "headless")]
use futures::future::join_all;

#[cfg(feature = "headless")]
use a3s_search::{
    a3s_use_browser::{BrowserPool, BrowserPoolConfig, BrowserProvider, PageRenderer},
    engines::{Baidu, Google},
    BrowserFetcher, Engine, PageFetcher, RetryBudget, WaitStrategy,
};

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

    let pool_config = match browser_pool_config(request.proxy, request.browser) {
        Ok(config) => config,
        Err(failure) => {
            let mut results = SearchResults::new();
            results.add_failure(failure);
            return results;
        }
    };
    let isolate_pools = request.browser == HeadlessBrowser::Lightpanda;
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
        let fetcher = |selector: &str| -> Arc<dyn PageFetcher> {
            Arc::new(
                BrowserFetcher::from_renderer(Arc::clone(&renderer))
                    .with_wait(headless_wait_strategy(
                        request.browser,
                        selector,
                        render_budget,
                    ))
                    .with_timeout(render_budget)
                    .with_total_timeout(render_budget)
                    .with_retries(1, 100)
                    .with_retry_budget(retry_budget.clone()),
            )
        };
        match shortcut.as_str() {
            "g" => {
                let engine = Google::new(fetcher("#search"));
                let engine_config =
                    configured_engine_config(request.config, engine.config().clone());
                search.add_engine(engine.with_config(engine_config));
            }
            "baidu" => {
                let engine = Baidu::new(fetcher("#content_left"));
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
    request: &CascadeRequest<'_>,
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
            format!(
                "the {} backend requires a3s-search to be built with the headless feature",
                request.browser.as_str()
            ),
        ));
    }
    results
}

#[cfg(feature = "headless")]
fn browser_pool_config(
    proxy: Option<&str>,
    browser: HeadlessBrowser,
) -> Result<BrowserPoolConfig, EngineFailure> {
    let provider = match browser {
        HeadlessBrowser::Chrome => BrowserProvider::DiscoveredChrome,
        HeadlessBrowser::Lightpanda => lightpanda_provider()?,
    };
    Ok(BrowserPoolConfig {
        proxy_url: proxy.map(str::to_string),
        provider,
        ..BrowserPoolConfig::default()
    })
}

#[cfg(all(feature = "headless", feature = "lightpanda"))]
fn lightpanda_provider() -> Result<BrowserProvider, EngineFailure> {
    Ok(BrowserProvider::DiscoveredLightpanda)
}

#[cfg(all(feature = "headless", not(feature = "lightpanda")))]
fn lightpanda_provider() -> Result<BrowserProvider, EngineFailure> {
    Err(EngineFailure::new(
        "lightpanda",
        "headless_backend_unavailable",
        "Lightpanda is an explicit optional backend; rebuild with the lightpanda Cargo feature",
    ))
}

#[cfg(feature = "headless")]
fn headless_wait_strategy(
    browser: HeadlessBrowser,
    selector: &str,
    timeout: Duration,
) -> WaitStrategy {
    match browser {
        HeadlessBrowser::Chrome => WaitStrategy::Selector {
            css: selector.to_string(),
            timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
        },
        HeadlessBrowser::Lightpanda => WaitStrategy::Load,
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
    fn cli_browser_default_is_chrome() {
        assert_eq!(HeadlessBrowser::default(), HeadlessBrowser::Chrome);
    }

    #[cfg(feature = "headless")]
    #[test]
    fn default_pool_is_pinned_to_discovered_chrome() {
        let config = browser_pool_config(Some("http://127.0.0.1:8080"), HeadlessBrowser::default())
            .expect("Chrome pool configuration");

        assert!(matches!(config.provider, BrowserProvider::DiscoveredChrome));
        assert_eq!(config.proxy_url.as_deref(), Some("http://127.0.0.1:8080"));
    }

    #[cfg(feature = "headless")]
    #[test]
    fn chrome_waits_for_search_results_but_lightpanda_uses_load() {
        let chrome = headless_wait_strategy(
            HeadlessBrowser::Chrome,
            "#search",
            Duration::from_millis(1_500),
        );
        assert!(matches!(
            chrome,
            WaitStrategy::Selector {
                css,
                timeout_ms: 1_500
            } if css == "#search"
        ));
        assert!(matches!(
            headless_wait_strategy(
                HeadlessBrowser::Lightpanda,
                "#search",
                Duration::from_secs(1)
            ),
            WaitStrategy::Load
        ));
    }

    #[cfg(all(feature = "headless", not(feature = "lightpanda")))]
    #[test]
    fn lightpanda_requires_explicit_cargo_feature() {
        let failure = browser_pool_config(None, HeadlessBrowser::Lightpanda)
            .expect_err("Lightpanda must not be implicit in a default build");

        assert_eq!(failure.kind, "headless_backend_unavailable");
    }

    #[cfg(all(feature = "headless", feature = "lightpanda"))]
    #[test]
    fn explicit_lightpanda_selection_uses_lightpanda_provider() {
        let config = browser_pool_config(None, HeadlessBrowser::Lightpanda)
            .expect("compiled Lightpanda pool configuration");

        assert!(matches!(
            config.provider,
            BrowserProvider::DiscoveredLightpanda
        ));
    }
}
