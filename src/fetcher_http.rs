//! HTTP-based page fetcher using reqwest.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tracing::debug;

use crate::fetcher::PageFetcher;
use crate::proxy::ProxyPool;
use crate::Result;

/// Default user agent for HTTP requests.
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// A page fetcher that uses plain HTTP requests via reqwest.
///
/// Suitable for engines that return server-rendered HTML. For engines
/// that require JavaScript rendering, use `BrowserFetcher` instead.
pub struct HttpFetcher {
    client: Client,
}

impl HttpFetcher {
    /// Creates a new `HttpFetcher` with default settings.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Creates an `HttpFetcher` with proxy support.
    pub fn with_proxy(proxy_url: &str) -> crate::Result<Self> {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| crate::SearchError::Other(format!("Failed to create proxy: {}", e)))?;
        let client = Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .proxy(proxy)
            .build()
            .map_err(|e| {
                crate::SearchError::Other(format!("Failed to create HTTP client: {}", e))
            })?;
        Ok(Self { client })
    }

    /// Creates an `HttpFetcher` with a custom reqwest client.
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }

    /// Returns a reference to the underlying reqwest client.
    ///
    /// Useful for engines like Wikipedia that need JSON parsing
    /// instead of plain HTML fetching.
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PageFetcher for HttpFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        let response = self.client.get(url).send().await?;
        let html = response.text().await?;
        Ok(html)
    }
}

/// A page fetcher that rotates proxies from a `ProxyPool` on each request.
///
/// Unlike `HttpFetcher` which uses a single static proxy, this fetcher
/// picks a different proxy for each request from the pool, enabling
/// IP rotation to avoid anti-crawler blocking.
///
/// The `ProxyPool` can be backed by any `ProxyProvider` implementation,
/// allowing external code to define how proxies are fetched and refreshed.
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use a3s_search::proxy::{ProxyPool, ProxyConfig};
/// use a3s_search::PooledHttpFetcher;
///
/// let pool = Arc::new(ProxyPool::with_proxies(vec![
///     ProxyConfig::new("10.0.0.1", 8080),
///     ProxyConfig::new("10.0.0.2", 8080),
/// ]));
/// let fetcher = PooledHttpFetcher::new(pool);
/// // Each fetch() call will use a different proxy from the pool
/// ```
pub struct PooledHttpFetcher {
    pool: Arc<ProxyPool>,
    timeout: Duration,
}

impl PooledHttpFetcher {
    /// Creates a new `PooledHttpFetcher` with the given proxy pool.
    pub fn new(pool: Arc<ProxyPool>) -> Self {
        Self {
            pool,
            timeout: Duration::from_secs(30),
        }
    }

    /// Sets the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl PageFetcher for PooledHttpFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        let client = if let Some(proxy_config) = self.pool.get_proxy().await {
            debug!(
                "PooledHttpFetcher using proxy {}:{}",
                proxy_config.host, proxy_config.port
            );
            let proxy = reqwest::Proxy::all(proxy_config.url())
                .map_err(|e| crate::SearchError::Other(format!("Failed to create proxy: {}", e)))?;
            Client::builder()
                .user_agent(DEFAULT_USER_AGENT)
                .timeout(self.timeout)
                .proxy(proxy)
                .build()
                .map_err(|e| {
                    crate::SearchError::Other(format!("Failed to create HTTP client: {}", e))
                })?
        } else {
            Client::builder()
                .user_agent(DEFAULT_USER_AGENT)
                .timeout(self.timeout)
                .build()
                .map_err(|e| {
                    crate::SearchError::Other(format!("Failed to create HTTP client: {}", e))
                })?
        };

        let response = client.get(url).send().await?;
        let html = response.text().await?;
        Ok(html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::ProxyConfig;

    #[test]
    fn test_http_fetcher_new() {
        let _fetcher = HttpFetcher::new();
    }

    #[test]
    fn test_http_fetcher_default() {
        let _fetcher = HttpFetcher::default();
    }

    #[test]
    fn test_http_fetcher_with_client() {
        let client = Client::builder().user_agent("test-agent").build().unwrap();
        let _fetcher = HttpFetcher::with_client(client);
    }

    #[test]
    fn test_http_fetcher_with_proxy_invalid() {
        // Empty string is rejected by reqwest::Proxy::all
        let result = HttpFetcher::with_proxy("");
        assert!(result.is_err());
    }

    #[test]
    fn test_http_fetcher_with_proxy_valid() {
        let fetcher = HttpFetcher::with_proxy("http://127.0.0.1:8080");
        assert!(fetcher.is_ok());
    }

    #[test]
    fn test_http_fetcher_with_proxy_socks5() {
        let fetcher = HttpFetcher::with_proxy("socks5://127.0.0.1:1080");
        assert!(fetcher.is_ok());
    }

    #[test]
    fn test_http_fetcher_client_accessor() {
        let fetcher = HttpFetcher::new();
        let _client = fetcher.client();
    }

    // --- PooledHttpFetcher tests ---

    #[test]
    fn test_pooled_http_fetcher_new() {
        let pool = Arc::new(ProxyPool::new());
        let _fetcher = PooledHttpFetcher::new(pool);
    }

    #[test]
    fn test_pooled_http_fetcher_with_timeout() {
        let pool = Arc::new(ProxyPool::new());
        let fetcher = PooledHttpFetcher::new(pool).with_timeout(Duration::from_secs(15));
        assert_eq!(fetcher.timeout, Duration::from_secs(15));
    }

    #[test]
    fn test_pooled_http_fetcher_default_timeout() {
        let pool = Arc::new(ProxyPool::new());
        let fetcher = PooledHttpFetcher::new(pool);
        assert_eq!(fetcher.timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_pooled_http_fetcher_with_proxies() {
        let pool = Arc::new(ProxyPool::with_proxies(vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
        ]));
        let _fetcher = PooledHttpFetcher::new(pool);
    }
}
