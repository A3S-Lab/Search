//! Headless browser integration for JavaScript-rendered search engines.
//!
//! This module is only available when the `headless` Cargo feature is enabled.
//! It provides a shared browser process pool and a `PageFetcher` implementation
//! that renders pages using Chrome/Chromium via the Chrome DevTools Protocol.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::SetUserAgentOverrideParams;
use futures::StreamExt;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, warn};

use crate::fetcher::{PageFetcher, WaitStrategy};
use crate::{Result, SearchError};

/// Configuration for the browser pool.
#[derive(Debug, Clone)]
pub struct BrowserPoolConfig {
    /// Maximum number of concurrent browser tabs.
    pub max_tabs: usize,
    /// Whether to run the browser in headless mode.
    pub headless: bool,
    /// Path to the Chrome/Chromium executable. If `None`, auto-detected.
    pub chrome_path: Option<String>,
    /// Proxy URL for the browser to use.
    pub proxy_url: Option<String>,
    /// Additional launch arguments for Chrome.
    pub launch_args: Vec<String>,
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            max_tabs: 4,
            headless: true,
            chrome_path: None,
            proxy_url: None,
            launch_args: Vec::new(),
        }
    }
}

/// A shared pool managing a single browser process with tab concurrency control.
///
/// The browser is lazily launched on the first `acquire_browser()` call. A
/// semaphore limits the number of concurrent tabs to prevent memory exhaustion.
pub struct BrowserPool {
    config: BrowserPoolConfig,
    browser: Mutex<Option<Arc<Browser>>>,
    tab_semaphore: Arc<Semaphore>,
}

impl BrowserPool {
    /// Creates a new browser pool with the given configuration.
    pub fn new(config: BrowserPoolConfig) -> Self {
        let max_tabs = config.max_tabs;
        Self {
            config,
            browser: Mutex::new(None),
            tab_semaphore: Arc::new(Semaphore::new(max_tabs)),
        }
    }

    /// Returns the tab semaphore for acquiring permits before opening tabs.
    pub fn tab_semaphore(&self) -> &Arc<Semaphore> {
        &self.tab_semaphore
    }

    /// Lazily launches the browser and returns a shared handle.
    pub async fn acquire_browser(&self) -> Result<Arc<Browser>> {
        let mut guard = self.browser.lock().await;

        if let Some(ref browser) = *guard {
            return Ok(Arc::clone(browser));
        }

        debug!("Launching headless browser");

        let mut builder = BrowserConfig::builder();

        if self.config.headless {
            builder = builder.arg("--headless=new");
        }

        // Resolve Chrome executable: explicit path > auto-detect > auto-download
        if let Some(ref path) = self.config.chrome_path {
            builder = builder.chrome_executable(path);
        } else {
            let chrome_path = crate::browser_setup::ensure_chrome().await?;
            debug!("Using Chrome at: {}", chrome_path.display());
            builder = builder.chrome_executable(chrome_path);
        }

        // Realistic user-agent to avoid headless detection.
        // Chrome's --headless=new mode injects "HeadlessChrome" into the UA,
        // which Google and other sites trivially detect and block.
        builder = builder.arg(
            "--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        );

        // Anti-detection: hide navigator.webdriver and automation indicators
        builder = builder.arg("--disable-blink-features=AutomationControlled");

        // Standard arguments for scraping
        builder = builder
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-extensions")
            .arg("--disable-background-networking")
            .arg("--disable-default-apps")
            .arg("--disable-sync")
            .arg("--disable-translate")
            .arg("--mute-audio")
            .arg("--no-first-run");

        if let Some(ref proxy) = self.config.proxy_url {
            builder = builder.arg(format!("--proxy-server={}", proxy));
        }

        for arg in &self.config.launch_args {
            builder = builder.arg(arg);
        }

        let browser_config = builder
            .build()
            .map_err(|e| SearchError::Browser(format!("Failed to build browser config: {}", e)))?;

        let (browser, mut handler) = Browser::launch(browser_config)
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to launch browser: {}", e)))?;

        // Spawn the CDP event handler as a background task
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    warn!("Browser CDP handler error: {}", e);
                }
            }
            debug!("Browser CDP handler exited");
        });

        let browser = Arc::new(browser);
        *guard = Some(Arc::clone(&browser));

        Ok(browser)
    }

    /// Shuts down the browser process.
    pub async fn shutdown(&self) {
        let mut guard = self.browser.lock().await;
        if guard.take().is_some() {
            debug!("Browser pool shut down");
        }
    }
}

/// A `PageFetcher` that uses a headless browser to render JavaScript-heavy pages.
///
/// Each `fetch()` call opens a new tab, navigates, waits according to the
/// configured `WaitStrategy`, extracts the rendered HTML, and closes the tab.
pub struct BrowserFetcher {
    pool: Arc<BrowserPool>,
    wait: WaitStrategy,
    user_agent: Option<String>,
}

impl BrowserFetcher {
    /// Creates a new browser fetcher with default wait strategy (`Load`).
    pub fn new(pool: Arc<BrowserPool>) -> Self {
        Self {
            pool,
            wait: WaitStrategy::default(),
            user_agent: None,
        }
    }

    /// Sets the wait strategy for page rendering.
    pub fn with_wait(mut self, wait: WaitStrategy) -> Self {
        self.wait = wait;
        self
    }

    /// Sets a custom user agent for browser requests.
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }
}

#[async_trait]
impl PageFetcher for BrowserFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        // Acquire a tab permit to limit concurrency
        let _permit = self
            .pool
            .tab_semaphore()
            .acquire()
            .await
            .map_err(|e| SearchError::Browser(format!("Tab semaphore closed: {}", e)))?;

        let browser = self.pool.acquire_browser().await?;

        let page = browser
            .new_page(url)
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to open tab: {}", e)))?;

        // Set user agent if configured
        if let Some(ref ua) = self.user_agent {
            page.set_user_agent(SetUserAgentOverrideParams::new(ua))
                .await
                .map_err(|e| SearchError::Browser(format!("Failed to set user agent: {}", e)))?;
        }

        // Apply wait strategy
        match &self.wait {
            WaitStrategy::Load => {
                page.wait_for_navigation()
                    .await
                    .map_err(|e| SearchError::Browser(format!("Navigation wait failed: {}", e)))?;
            }
            WaitStrategy::NetworkIdle { idle_ms } => {
                // Wait for load first, then an additional idle period
                page.wait_for_navigation()
                    .await
                    .map_err(|e| SearchError::Browser(format!("Navigation wait failed: {}", e)))?;
                tokio::time::sleep(Duration::from_millis(*idle_ms)).await;
            }
            WaitStrategy::Selector { css, timeout_ms } => {
                // Wait for the selector, but don't fail if it's not found.
                // The page may have loaded a CAPTCHA or error page instead;
                // let the engine's parser detect and report that.
                let found = tokio::time::timeout(Duration::from_millis(*timeout_ms), async {
                    page.find_element(css.as_str()).await
                })
                .await;
                if let Err(_) | Ok(Err(_)) = found {
                    debug!(
                        "Selector '{}' not found within {}ms, proceeding with current page content",
                        css, timeout_ms
                    );
                }
            }
            WaitStrategy::Delay { ms } => {
                page.wait_for_navigation()
                    .await
                    .map_err(|e| SearchError::Browser(format!("Navigation wait failed: {}", e)))?;
                tokio::time::sleep(Duration::from_millis(*ms)).await;
            }
        }

        // Extract the rendered HTML
        let html = page
            .content()
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to get page content: {}", e)))?;

        // Close the tab (best-effort, don't fail the fetch)
        if let Err(e) = page.close().await {
            warn!("Failed to close browser tab: {}", e);
        }

        Ok(html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_pool_config_default() {
        let config = BrowserPoolConfig::default();
        assert_eq!(config.max_tabs, 4);
        assert!(config.headless);
        assert!(config.chrome_path.is_none());
        assert!(config.proxy_url.is_none());
        assert!(config.launch_args.is_empty());
    }

    #[test]
    fn test_browser_pool_config_custom() {
        let config = BrowserPoolConfig {
            max_tabs: 8,
            headless: false,
            chrome_path: Some("/usr/bin/chromium".to_string()),
            proxy_url: Some("http://localhost:8080".to_string()),
            launch_args: vec!["--disable-web-security".to_string()],
        };
        assert_eq!(config.max_tabs, 8);
        assert!(!config.headless);
        assert_eq!(config.chrome_path.as_deref(), Some("/usr/bin/chromium"));
        assert_eq!(config.proxy_url.as_deref(), Some("http://localhost:8080"));
        assert_eq!(config.launch_args.len(), 1);
    }

    #[test]
    fn test_browser_pool_new() {
        let pool = BrowserPool::new(BrowserPoolConfig::default());
        assert_eq!(pool.tab_semaphore().available_permits(), 4);
    }

    #[test]
    fn test_browser_pool_custom_tabs() {
        let config = BrowserPoolConfig {
            max_tabs: 2,
            ..Default::default()
        };
        let pool = BrowserPool::new(config);
        assert_eq!(pool.tab_semaphore().available_permits(), 2);
    }

    #[test]
    fn test_browser_fetcher_new() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = BrowserFetcher::new(pool);
        assert!(matches!(fetcher.wait, WaitStrategy::Load));
        assert!(fetcher.user_agent.is_none());
    }

    #[test]
    fn test_browser_fetcher_with_wait() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = BrowserFetcher::new(pool).with_wait(WaitStrategy::Selector {
            css: "div.g".to_string(),
            timeout_ms: 5000,
        });
        assert!(matches!(fetcher.wait, WaitStrategy::Selector { .. }));
    }

    #[test]
    fn test_browser_fetcher_with_user_agent() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = BrowserFetcher::new(pool).with_user_agent("CustomBot/1.0");
        assert_eq!(fetcher.user_agent.as_deref(), Some("CustomBot/1.0"));
    }

    #[tokio::test]
    async fn test_browser_pool_shutdown_no_browser() {
        let pool = BrowserPool::new(BrowserPoolConfig::default());
        // Shutdown without ever launching should not panic
        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_browser_pool_shutdown_twice() {
        let pool = BrowserPool::new(BrowserPoolConfig::default());
        pool.shutdown().await;
        pool.shutdown().await;
    }

    #[test]
    fn test_browser_pool_config_with_proxy() {
        let config = BrowserPoolConfig {
            proxy_url: Some("http://localhost:8080".to_string()),
            ..Default::default()
        };
        assert_eq!(config.proxy_url.as_deref(), Some("http://localhost:8080"));
        assert!(config.headless);
    }

    #[test]
    fn test_browser_pool_config_with_launch_args() {
        let config = BrowserPoolConfig {
            launch_args: vec![
                "--disable-web-security".to_string(),
                "--ignore-certificate-errors".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(config.launch_args.len(), 2);
    }

    #[test]
    fn test_browser_pool_config_clone() {
        let config = BrowserPoolConfig {
            max_tabs: 8,
            headless: false,
            chrome_path: Some("/usr/bin/chromium".to_string()),
            proxy_url: Some("socks5://localhost:1080".to_string()),
            launch_args: vec!["--no-sandbox".to_string()],
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_tabs, 8);
        assert!(!cloned.headless);
        assert_eq!(cloned.chrome_path.as_deref(), Some("/usr/bin/chromium"));
        assert_eq!(cloned.proxy_url.as_deref(), Some("socks5://localhost:1080"));
        assert_eq!(cloned.launch_args.len(), 1);
    }

    #[test]
    fn test_browser_pool_config_debug() {
        let config = BrowserPoolConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("BrowserPoolConfig"));
        assert!(debug_str.contains("max_tabs"));
    }

    #[test]
    fn test_browser_fetcher_builder_chain() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = BrowserFetcher::new(pool)
            .with_wait(WaitStrategy::Delay { ms: 500 })
            .with_user_agent("TestBot/2.0");
        assert!(matches!(fetcher.wait, WaitStrategy::Delay { ms: 500 }));
        assert_eq!(fetcher.user_agent.as_deref(), Some("TestBot/2.0"));
    }

    #[test]
    fn test_browser_fetcher_with_network_idle_wait() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher =
            BrowserFetcher::new(pool).with_wait(WaitStrategy::NetworkIdle { idle_ms: 1000 });
        assert!(matches!(
            fetcher.wait,
            WaitStrategy::NetworkIdle { idle_ms: 1000 }
        ));
    }

    #[test]
    fn test_browser_pool_semaphore_permits() {
        let config = BrowserPoolConfig {
            max_tabs: 16,
            ..Default::default()
        };
        let pool = BrowserPool::new(config);
        assert_eq!(pool.tab_semaphore().available_permits(), 16);
    }
}
