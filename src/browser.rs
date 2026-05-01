//! Headless browser integration for JavaScript-rendered search engines.
//!
//! This module is only available when the `headless` Cargo feature is enabled.
//! It provides a shared browser process pool and a `PageFetcher` implementation
//! that renders pages using a headless browser via the Chrome DevTools Protocol.
//!
//! ## Architecture
//!
//! 1. **BrowserPool** - Manages browser lifecycle and tab concurrency
//! 2. **PageGuard** (RAII) - Ensures pages are closed even on errors
//! 3. **BrowserFetcher** - PageFetcher with retry logic and exponential backoff
//!
//! ## Backends
//!
//! Two backends are supported, selected via `BrowserPoolConfig::backend`:
//!
//! - **`BrowserBackend::Chrome`** (default): Launches Chrome/Chromium locally.
//!   Requires the `headless` feature. Chrome is auto-detected or downloaded.
//!
//! - **`BrowserBackend::Lightpanda`**: Spawns a Lightpanda process and connects
//!   via CDP over WebSocket. Requires the `lightpanda` feature.

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

/// Selects which headless browser backend the pool uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserBackend {
    /// Launch Chrome/Chromium locally.
    Chrome,

    /// Spawn a Lightpanda process and connect via CDP over WebSocket.
    #[cfg(feature = "lightpanda")]
    Lightpanda,
}

impl Default for BrowserBackend {
    fn default() -> Self {
        #[cfg(feature = "lightpanda")]
        return Self::Lightpanda;
        #[cfg(not(feature = "lightpanda"))]
        return Self::Chrome;
    }
}

/// Configuration for the browser pool.
#[derive(Debug, Clone)]
pub struct BrowserPoolConfig {
    /// Maximum number of concurrent browser tabs.
    pub max_tabs: usize,
    /// Whether to run Chrome in headless mode (ignored for Lightpanda).
    pub headless: bool,
    /// Path to the Chrome/Chromium executable. If `None`, auto-detected.
    pub chrome_path: Option<String>,
    /// Path to the Lightpanda executable. If `None`, auto-detected.
    #[cfg(feature = "lightpanda")]
    pub lightpanda_path: Option<String>,
    /// Proxy URL for the browser to use.
    pub proxy_url: Option<String>,
    /// Additional launch arguments for Chrome.
    pub launch_args: Vec<String>,
    /// Which browser backend to use.
    pub backend: BrowserBackend,
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            max_tabs: 4,
            headless: true,
            chrome_path: None,
            #[cfg(feature = "lightpanda")]
            lightpanda_path: None,
            proxy_url: None,
            launch_args: Vec::new(),
            backend: BrowserBackend::default(),
        }
    }
}

/// A shared pool managing a single browser process with tab concurrency control.
///
/// The browser is lazily launched on the first `acquire_browser()` call.
/// A semaphore limits the number of concurrent tabs to prevent memory exhaustion.
pub struct BrowserPool {
    config: BrowserPoolConfig,
    browser: Mutex<Option<Arc<Browser>>>,
    child: Mutex<Option<tokio::process::Child>>,
    tab_semaphore: Arc<Semaphore>,
}

impl BrowserPool {
    /// Creates a new browser pool with the given configuration.
    pub fn new(config: BrowserPoolConfig) -> Self {
        let max_tabs = config.max_tabs;
        Self {
            config,
            browser: Mutex::new(None),
            child: Mutex::new(None),
            tab_semaphore: Arc::new(Semaphore::new(max_tabs)),
        }
    }

    /// Returns the tab semaphore for acquiring permits before opening tabs.
    pub fn tab_semaphore(&self) -> &Arc<Semaphore> {
        &self.tab_semaphore
    }

    /// Lazily acquires the browser, launching it on the first call.
    pub async fn acquire_browser(&self) -> Result<Arc<Browser>> {
        #[cfg(feature = "lightpanda")]
        if self.config.backend == BrowserBackend::Lightpanda {
            return self.acquire_lightpanda().await;
        }

        self.acquire_chrome().await
    }

    async fn acquire_chrome(&self) -> Result<Arc<Browser>> {
        let mut guard = self.browser.lock().await;

        if let Some(ref browser) = *guard {
            return Ok(Arc::clone(browser));
        }

        debug!("Launching Chrome headless browser");

        let mut builder = BrowserConfig::builder();

        if self.config.headless {
            builder = builder.arg("--headless=new");
        }

        if let Some(ref path) = self.config.chrome_path {
            builder = builder.chrome_executable(path);
        } else {
            let chrome_path = crate::browser_setup::ensure_chrome().await?;
            debug!("Using Chrome at: {}", chrome_path.display());
            builder = builder.chrome_executable(chrome_path);
        }

        builder = builder.arg(
            "--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        );

        builder = builder.arg("--disable-blink-features=AutomationControlled");

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
            .map_err(|e| SearchError::Browser(format!("Failed to launch Chrome: {}", e)))?;

        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    warn!("Chrome CDP handler error: {}", e);
                }
            }
            debug!("Chrome CDP handler exited");
        });

        let browser = Arc::new(browser);
        *guard = Some(Arc::clone(&browser));

        Ok(browser)
    }

    #[cfg(feature = "lightpanda")]
    async fn acquire_lightpanda(&self) -> Result<Arc<Browser>> {
        let mut guard = self.browser.lock().await;

        if let Some(ref browser) = *guard {
            return Ok(Arc::clone(browser));
        }

        debug!("Launching Lightpanda browser");

        let lp_path = if let Some(ref path) = self.config.lightpanda_path {
            std::path::PathBuf::from(path)
        } else {
            crate::browser_setup_lp::ensure_lightpanda().await?
        };

        let port = find_free_port()?;

        let child = tokio::process::Command::new(&lp_path)
            .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                SearchError::Browser(format!(
                    "Failed to spawn Lightpanda ({}): {}",
                    lp_path.display(),
                    e
                ))
            })?;

        *self.child.lock().await = Some(child);

        wait_for_cdp_ready("127.0.0.1", port, Duration::from_secs(10)).await?;

        let ws_url = format!("ws://127.0.0.1:{}", port);
        debug!("Connecting to Lightpanda CDP at {}", ws_url);

        let (browser, mut handler) = Browser::connect(&ws_url)
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to connect to Lightpanda: {}", e)))?;

        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    warn!("Lightpanda CDP handler error: {}", e);
                }
            }
            debug!("Lightpanda CDP handler exited");
        });

        let browser = Arc::new(browser);
        *guard = Some(Arc::clone(&browser));

        Ok(browser)
    }

    /// Shuts down the browser and kills any spawned child process.
    pub async fn shutdown(&self) {
        let mut guard = self.browser.lock().await;
        if guard.take().is_some() {
            debug!("Browser connection closed");
        }

        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            if let Err(e) = child.kill().await {
                warn!("Failed to kill browser child process: {}", e);
            } else {
                debug!("Browser child process killed");
            }
        }
    }
}

#[allow(dead_code)]
fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| SearchError::Browser(format!("Failed to find a free port: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| SearchError::Browser(format!("Failed to read assigned port: {}", e)))?
        .port();
    Ok(port)
}

#[cfg(feature = "lightpanda")]
async fn wait_for_cdp_ready(host: &str, port: u16, timeout: Duration) -> Result<()> {
    let addr = format!("{}:{}", host, port);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(SearchError::Browser(format!(
                "Timed out waiting for CDP server at {} to become ready",
                addr
            )));
        }

        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            debug!("CDP server at {} is ready", addr);
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ============================================================================
// RAII PageGuard - Ensures page cleanup even on errors
// ============================================================================

/// RAII guard for browser pages.
///
/// Ensures the page is closed when the guard is dropped, even if an error occurs.
/// This prevents resource leaks from crashed or abandoned browser tabs.
struct PageGuard {
    // Option allows taking ownership without cloning
    page: Option<chromiumoxide::Page>,
}

impl PageGuard {
    fn new(page: chromiumoxide::Page) -> Self {
        Self { page: Some(page) }
    }

    /// Take ownership of the page, bypassing RAII cleanup.
    /// Use when you're about to close it manually.
    fn take(&mut self) -> Option<chromiumoxide::Page> {
        self.page.take()
    }
}

impl Drop for PageGuard {
    fn drop(&mut self) {
        if let Some(page) = self.page.take() {
            let page = page.clone();
            tokio::spawn(async move {
                if let Err(e) = page.close().await {
                    warn!("PageGuard: failed to close page on drop: {}", e);
                }
            });
        }
    }
}

// ============================================================================
// BrowserFetcher - PageFetcher with retry and RAII
// ============================================================================

/// A `PageFetcher` that uses a headless browser to render JavaScript-heavy pages.
///
/// Each `fetch()` call opens a new tab, navigates, waits according to the
/// configured `WaitStrategy`, extracts the rendered HTML, and closes the tab.
///
/// Features:
/// - RAII page cleanup via `PageGuard`
/// - Retry with exponential backoff for transient errors
/// - Configurable retry parameters
pub struct BrowserFetcher {
    pool: Arc<BrowserPool>,
    wait: WaitStrategy,
    user_agent: Option<String>,
    max_retries: u32,
    retry_delay_ms: u64,
}

impl BrowserFetcher {
    /// Creates a new browser fetcher with default settings.
    ///
    /// Default: Load strategy, no custom UA, 3 retries with 100ms base delay.
    pub fn new(pool: Arc<BrowserPool>) -> Self {
        Self {
            pool,
            wait: WaitStrategy::default(),
            user_agent: None,
            max_retries: 3,
            retry_delay_ms: 100,
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

    /// Sets retry parameters for transient failures.
    ///
    /// - `max_retries`: Maximum number of retry attempts (default: 3)
    /// - `retry_delay_ms`: Base delay between retries in milliseconds (default: 100)
    ///
    /// Uses exponential backoff: delay * 2^(attempt - 1)
    pub fn with_retries(mut self, max_retries: u32, retry_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.retry_delay_ms = retry_delay_ms;
        self
    }

    /// Perform the fetch with retry logic.
    async fn fetch_with_retry(&self, url: &str) -> Result<String> {
        let mut attempt = 0;
        let mut delay = Duration::from_millis(self.retry_delay_ms);

        loop {
            match self.do_fetch(url).await {
                Ok(html) => return Ok(html),
                Err(e) if attempt < self.max_retries && is_transient_error(&e) => {
                    attempt += 1;
                    warn!(
                        "Transient error on {}, retry {}/{} after {:?}: {}",
                        url, attempt, self.max_retries, delay, e
                    );
                    tokio::time::sleep(delay).await;
                    delay *= 2; // Exponential backoff
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Core fetch implementation without retry.
    async fn do_fetch(&self, url: &str) -> Result<String> {
        // Acquire tab permit
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

        // RAII guard ensures page cleanup on drop
        let mut guard = PageGuard::new(page);

        // Take page from guard for operations
        let page = guard.take().expect("Page already taken");

        // Set user agent if configured
        if let Some(ref ua) = self.user_agent {
            if let Err(e) = page
                .set_user_agent(SetUserAgentOverrideParams::new(ua))
                .await
            {
                debug!("Failed to set user agent (non-fatal): {}", e);
            }
        }

        // Apply wait strategy
        self.apply_wait_strategy(&page).await?;

        // Extract the rendered HTML
        let html = page
            .content()
            .await
            .map_err(|e| SearchError::Browser(format!("Failed to get page content: {}", e)))?;

        // Close the tab explicitly (PageGuard on drop is backup)
        if let Err(e) = page.close().await {
            warn!("Failed to close browser tab: {}", e);
        }

        Ok(html)
    }

    /// Apply the configured wait strategy.
    async fn apply_wait_strategy(&self, page: &chromiumoxide::Page) -> Result<()> {
        match &self.wait {
            WaitStrategy::Load => {
                page.wait_for_navigation()
                    .await
                    .map_err(|e| SearchError::Browser(format!("Navigation wait failed: {}", e)))?;
            }
            WaitStrategy::NetworkIdle { idle_ms } => {
                page.wait_for_navigation()
                    .await
                    .map_err(|e| SearchError::Browser(format!("Navigation wait failed: {}", e)))?;
                tokio::time::sleep(Duration::from_millis(*idle_ms)).await;
            }
            WaitStrategy::Selector { css, timeout_ms } => {
                let found = tokio::time::timeout(
                    Duration::from_millis(*timeout_ms),
                    page.find_element(css.as_str()),
                )
                .await;
                if let Err(_) | Ok(Err(_)) = found {
                    debug!(
                        "Selector '{}' not found within {}ms, proceeding with current page",
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
        Ok(())
    }
}

#[async_trait]
impl PageFetcher for BrowserFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        self.fetch_with_retry(url).await
    }
}

/// Determines if an error is transient and worth retrying.
/// DEPRECATED: Use SearchError::is_transient() instead.
#[allow(dead_code)]
fn is_transient_error(error: &SearchError) -> bool {
    error.is_transient()
}

// ============================================================================
// Tests
// ============================================================================

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
        #[cfg(feature = "lightpanda")]
        assert_eq!(config.backend, BrowserBackend::Lightpanda);
        #[cfg(not(feature = "lightpanda"))]
        assert_eq!(config.backend, BrowserBackend::Chrome);
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
        assert_eq!(fetcher.max_retries, 3);
        assert_eq!(fetcher.retry_delay_ms, 100);
    }

    #[test]
    fn test_browser_fetcher_builder() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = BrowserFetcher::new(pool)
            .with_wait(WaitStrategy::Selector {
                css: "div.g".to_string(),
                timeout_ms: 5000,
            })
            .with_user_agent("CustomBot/1.0")
            .with_retries(5, 200);

        assert!(matches!(fetcher.wait, WaitStrategy::Selector { .. }));
        assert_eq!(fetcher.user_agent.as_deref(), Some("CustomBot/1.0"));
        assert_eq!(fetcher.max_retries, 5);
        assert_eq!(fetcher.retry_delay_ms, 200);
    }

    #[test]
    fn test_is_transient_error() {
        assert!(is_transient_error(&SearchError::Browser("timeout".into())));
        assert!(is_transient_error(&SearchError::Browser(
            "Connection reset".into()
        )));
        assert!(is_transient_error(&SearchError::Browser(
            "net::err_connection_reset".into()
        )));
        assert!(is_transient_error(&SearchError::Browser(
            "Channel closed".into()
        )));
        assert!(!is_transient_error(&SearchError::Browser(
            "CAPTCHA detected".into()
        )));
        assert!(!is_transient_error(&SearchError::Browser(
            "Failed to parse".into()
        )));
    }

    #[tokio::test]
    async fn test_browser_pool_shutdown_no_browser() {
        let pool = BrowserPool::new(BrowserPoolConfig::default());
        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_browser_pool_shutdown_twice() {
        let pool = BrowserPool::new(BrowserPoolConfig::default());
        pool.shutdown().await;
        pool.shutdown().await;
    }

    #[test]
    fn test_find_free_port() {
        let port = find_free_port().expect("Should find a free port");
        assert!(port > 0, "Port should be non-zero");
    }

    #[cfg(not(feature = "lightpanda"))]
    #[test]
    fn test_browser_backend_default_is_chrome() {
        let backend = BrowserBackend::default();
        assert_eq!(backend, BrowserBackend::Chrome);
    }

    #[cfg(feature = "lightpanda")]
    #[test]
    fn test_browser_backend_default_is_lightpanda() {
        let backend = BrowserBackend::default();
        assert_eq!(backend, BrowserBackend::Lightpanda);
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
    fn test_browser_fetcher_with_delay_wait() {
        let pool = Arc::new(BrowserPool::new(BrowserPoolConfig::default()));
        let fetcher = BrowserFetcher::new(pool).with_wait(WaitStrategy::Delay { ms: 500 });
        assert!(matches!(fetcher.wait, WaitStrategy::Delay { ms: 500 }));
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
