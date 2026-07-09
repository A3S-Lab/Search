//! HTTP-based page fetcher using reqwest.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use tracing::debug;

use crate::fetcher::PageFetcher;
use crate::proxy::{ProxyConfig, ProxyPool};
use crate::{Metrics, Result, SearchError};

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
    metrics: Option<Arc<Metrics>>,
}

impl HttpFetcher {
    /// Creates a new `HttpFetcher` with default settings.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .expect("Failed to create HTTP client"),
            metrics: None,
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
        Ok(Self {
            client,
            metrics: None,
        })
    }

    /// Creates an `HttpFetcher` with a custom reqwest client.
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            metrics: None,
        }
    }

    /// Attaches a metrics registry used to record fetch attempts.
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
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
        let start = Instant::now();
        let result = async {
            let response = self.client.get(url).send().await?.error_for_status()?;
            let html = response.text().await?;
            Ok(html)
        }
        .await;
        record_fetch_metrics(&self.metrics, start, &result);
        result
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
    client_cache: Mutex<HashMap<String, Client>>,
    metrics: Option<Arc<Metrics>>,
}

impl PooledHttpFetcher {
    /// Creates a new `PooledHttpFetcher` with the given proxy pool.
    pub fn new(pool: Arc<ProxyPool>) -> Self {
        Self {
            pool,
            timeout: Duration::from_secs(30),
            client_cache: Mutex::new(HashMap::new()),
            metrics: None,
        }
    }

    /// Sets the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        lock_recover(&self.client_cache).clear();
        self
    }

    /// Attaches a metrics registry used to record fetch attempts.
    pub fn with_metrics(mut self, metrics: Arc<Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    fn client_for(&self, proxy_config: Option<&ProxyConfig>) -> Result<Client> {
        let key = proxy_config
            .map(ProxyConfig::url)
            .unwrap_or_else(|| "direct://".to_string());

        let mut cache = lock_recover(&self.client_cache);
        if let Some(client) = cache.get(&key) {
            return Ok(client.clone());
        }

        let mut builder = Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout(self.timeout);

        if proxy_config.is_some() {
            let proxy = reqwest::Proxy::all(&key)
                .map_err(|e| SearchError::Other(format!("Failed to create proxy: {}", e)))?;
            builder = builder.proxy(proxy);
        }

        let client = builder
            .build()
            .map_err(|e| SearchError::Other(format!("Failed to create HTTP client: {}", e)))?;
        cache.insert(key, client.clone());
        Ok(client)
    }
}

#[async_trait]
impl PageFetcher for PooledHttpFetcher {
    async fn fetch(&self, url: &str) -> Result<String> {
        let start = Instant::now();
        let proxy_config = self.pool.get_proxy().await;

        if let Some(proxy_config) = proxy_config.as_ref() {
            debug!(
                "PooledHttpFetcher using proxy {}:{}",
                proxy_config.host, proxy_config.port
            );
        }

        let result = async {
            let client = self.client_for(proxy_config.as_ref())?;
            let response = client.get(url).send().await?.error_for_status()?;
            let html = response.text().await?;
            Ok(html)
        }
        .await;
        record_fetch_metrics(&self.metrics, start, &result);
        result
    }
}

fn record_fetch_metrics<T>(metrics: &Option<Arc<Metrics>>, start: Instant, result: &Result<T>) {
    if let Some(metrics) = metrics.as_ref() {
        match result {
            Ok(_) => metrics.record_success(start.elapsed()),
            Err(error) => metrics.record_failure(error.kind(), error.is_transient()),
        }
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
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

    #[test]
    fn test_pooled_http_fetcher_caches_direct_client() {
        let pool = Arc::new(ProxyPool::new());
        let fetcher = PooledHttpFetcher::new(pool);

        let _first = fetcher.client_for(None).unwrap();
        let _second = fetcher.client_for(None).unwrap();

        assert_eq!(lock_recover(&fetcher.client_cache).len(), 1);
    }

    #[test]
    fn test_pooled_http_fetcher_caches_proxy_clients_by_url() {
        let pool = Arc::new(ProxyPool::new());
        let fetcher = PooledHttpFetcher::new(pool);
        let proxy = ProxyConfig::new("127.0.0.1", 8080);

        let _first = fetcher.client_for(Some(&proxy)).unwrap();
        let _second = fetcher.client_for(Some(&proxy)).unwrap();
        let _direct = fetcher.client_for(None).unwrap();

        assert_eq!(lock_recover(&fetcher.client_cache).len(), 2);
    }

    #[test]
    fn test_pooled_http_fetcher_timeout_change_clears_cache() {
        let pool = Arc::new(ProxyPool::new());
        let fetcher = PooledHttpFetcher::new(pool);

        let _client = fetcher.client_for(None).unwrap();
        assert_eq!(lock_recover(&fetcher.client_cache).len(), 1);

        let fetcher = fetcher.with_timeout(Duration::from_secs(5));
        assert_eq!(lock_recover(&fetcher.client_cache).len(), 0);
    }

    #[tokio::test]
    async fn test_http_fetcher_records_failure_metrics() {
        let metrics = Arc::new(Metrics::new());
        let fetcher = HttpFetcher::new().with_metrics(Arc::clone(&metrics));

        let result = fetcher.fetch("not a url").await;
        assert!(result.is_err());

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.failures, 1);
        assert_eq!(snapshot.total_requests(), 1);
    }
}
