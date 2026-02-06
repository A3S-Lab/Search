//! Dynamic proxy IP pool for anti-crawler protection.
//!
//! This module provides a flexible proxy management system that allows
//! search engines to rotate through multiple proxy IPs to avoid being
//! blocked by anti-crawler mechanisms.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Proxy as ReqwestProxy};
use tokio::sync::RwLock;
use tracing::debug;

use crate::{Result, SearchError};

/// Proxy protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyProtocol {
    /// HTTP proxy
    #[default]
    Http,
    /// HTTPS proxy
    Https,
    /// SOCKS5 proxy
    Socks5,
}

/// A single proxy configuration.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Proxy host (IP or domain)
    pub host: String,
    /// Proxy port
    pub port: u16,
    /// Proxy protocol
    pub protocol: ProxyProtocol,
    /// Optional username for authentication
    pub username: Option<String>,
    /// Optional password for authentication
    pub password: Option<String>,
}

impl ProxyConfig {
    /// Creates a new proxy configuration.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            protocol: ProxyProtocol::Http,
            username: None,
            password: None,
        }
    }

    /// Sets the proxy protocol.
    pub fn with_protocol(mut self, protocol: ProxyProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Sets authentication credentials.
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    /// Returns the proxy URL string.
    pub fn url(&self) -> String {
        let scheme = match self.protocol {
            ProxyProtocol::Http => "http",
            ProxyProtocol::Https => "https",
            ProxyProtocol::Socks5 => "socks5",
        };

        match (&self.username, &self.password) {
            (Some(user), Some(pass)) => {
                format!("{}://{}:{}@{}:{}", scheme, user, pass, self.host, self.port)
            }
            _ => format!("{}://{}:{}", scheme, self.host, self.port),
        }
    }
}

/// Proxy selection strategy.
#[derive(Debug, Clone, Copy, Default)]
pub enum ProxyStrategy {
    /// Round-robin selection
    #[default]
    RoundRobin,
    /// Random selection
    Random,
}

/// Trait for providing proxies dynamically.
#[async_trait]
pub trait ProxyProvider: Send + Sync {
    /// Fetches a list of available proxies.
    async fn fetch_proxies(&self) -> Result<Vec<ProxyConfig>>;

    /// Returns the refresh interval for proxy list.
    fn refresh_interval(&self) -> Duration {
        Duration::from_secs(300) // 5 minutes default
    }
}

/// A static proxy provider that returns a fixed list of proxies.
pub struct StaticProxyProvider {
    proxies: Vec<ProxyConfig>,
}

impl StaticProxyProvider {
    /// Creates a new static proxy provider.
    pub fn new(proxies: Vec<ProxyConfig>) -> Self {
        Self { proxies }
    }
}

#[async_trait]
impl ProxyProvider for StaticProxyProvider {
    async fn fetch_proxies(&self) -> Result<Vec<ProxyConfig>> {
        Ok(self.proxies.clone())
    }

    fn refresh_interval(&self) -> Duration {
        Duration::from_secs(u64::MAX) // Never refresh
    }
}

/// A proxy pool that manages multiple proxies with rotation.
pub struct ProxyPool {
    proxies: Arc<RwLock<Vec<ProxyConfig>>>,
    provider: Option<Arc<dyn ProxyProvider>>,
    strategy: ProxyStrategy,
    current_index: AtomicUsize,
    enabled: bool,
}

impl ProxyPool {
    /// Creates a new empty proxy pool.
    pub fn new() -> Self {
        Self {
            proxies: Arc::new(RwLock::new(Vec::new())),
            provider: None,
            strategy: ProxyStrategy::RoundRobin,
            current_index: AtomicUsize::new(0),
            enabled: false,
        }
    }

    /// Creates a proxy pool with static proxies.
    pub fn with_proxies(proxies: Vec<ProxyConfig>) -> Self {
        let enabled = !proxies.is_empty();
        Self {
            proxies: Arc::new(RwLock::new(proxies)),
            provider: None,
            strategy: ProxyStrategy::RoundRobin,
            current_index: AtomicUsize::new(0),
            enabled,
        }
    }

    /// Creates a proxy pool with a dynamic provider.
    pub fn with_provider<P: ProxyProvider + 'static>(provider: P) -> Self {
        Self {
            proxies: Arc::new(RwLock::new(Vec::new())),
            provider: Some(Arc::new(provider)),
            strategy: ProxyStrategy::RoundRobin,
            current_index: AtomicUsize::new(0),
            enabled: true,
        }
    }

    /// Sets the proxy selection strategy.
    pub fn with_strategy(mut self, strategy: ProxyStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Enables or disables the proxy pool.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether the proxy pool is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Refreshes the proxy list from the provider.
    pub async fn refresh(&self) -> Result<()> {
        if let Some(ref provider) = self.provider {
            let new_proxies = provider.fetch_proxies().await?;
            debug!("Refreshed proxy pool with {} proxies", new_proxies.len());
            let mut proxies = self.proxies.write().await;
            *proxies = new_proxies;
        }
        Ok(())
    }

    /// Returns the number of proxies in the pool.
    pub async fn len(&self) -> usize {
        self.proxies.read().await.len()
    }

    /// Returns whether the pool is empty.
    pub async fn is_empty(&self) -> bool {
        self.proxies.read().await.is_empty()
    }

    /// Gets the next proxy based on the selection strategy.
    pub async fn get_proxy(&self) -> Option<ProxyConfig> {
        if !self.enabled {
            return None;
        }

        let proxies = self.proxies.read().await;
        if proxies.is_empty() {
            return None;
        }

        let index = match self.strategy {
            ProxyStrategy::RoundRobin => {
                self.current_index.fetch_add(1, Ordering::SeqCst) % proxies.len()
            }
            ProxyStrategy::Random => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as usize;
                seed % proxies.len()
            }
        };

        proxies.get(index).cloned()
    }

    /// Adds a proxy to the pool.
    pub async fn add_proxy(&self, proxy: ProxyConfig) {
        let mut proxies = self.proxies.write().await;
        proxies.push(proxy);
    }

    /// Removes a proxy from the pool by host and port.
    pub async fn remove_proxy(&self, host: &str, port: u16) {
        let mut proxies = self.proxies.write().await;
        proxies.retain(|p| !(p.host == host && p.port == port));
    }

    /// Creates a reqwest Client configured with the next proxy.
    pub async fn create_client(&self, user_agent: &str) -> Result<Client> {
        let mut builder = Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30));

        if let Some(proxy_config) = self.get_proxy().await {
            let proxy_url = proxy_config.url();
            debug!("Using proxy: {}:{}", proxy_config.host, proxy_config.port);

            let proxy = ReqwestProxy::all(&proxy_url)
                .map_err(|e| SearchError::Other(format!("Failed to create proxy: {}", e)))?;
            builder = builder.proxy(proxy);
        }

        builder
            .build()
            .map_err(|e| SearchError::Other(format!("Failed to create HTTP client: {}", e)))
    }
}

impl Default for ProxyPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_protocol_default() {
        let protocol = ProxyProtocol::default();
        assert_eq!(protocol, ProxyProtocol::Http);
    }

    #[test]
    fn test_proxy_config_new() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080);
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 8080);
        assert_eq!(proxy.protocol, ProxyProtocol::Http);
        assert!(proxy.username.is_none());
        assert!(proxy.password.is_none());
    }

    #[test]
    fn test_proxy_config_with_protocol() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080).with_protocol(ProxyProtocol::Socks5);
        assert_eq!(proxy.protocol, ProxyProtocol::Socks5);
    }

    #[test]
    fn test_proxy_config_with_auth() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080).with_auth("user", "pass");
        assert_eq!(proxy.username, Some("user".to_string()));
        assert_eq!(proxy.password, Some("pass".to_string()));
    }

    #[test]
    fn test_proxy_config_url_http() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080);
        assert_eq!(proxy.url(), "http://127.0.0.1:8080");
    }

    #[test]
    fn test_proxy_config_url_https() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080).with_protocol(ProxyProtocol::Https);
        assert_eq!(proxy.url(), "https://127.0.0.1:8080");
    }

    #[test]
    fn test_proxy_config_url_socks5() {
        let proxy = ProxyConfig::new("127.0.0.1", 1080).with_protocol(ProxyProtocol::Socks5);
        assert_eq!(proxy.url(), "socks5://127.0.0.1:1080");
    }

    #[test]
    fn test_proxy_config_url_with_auth() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080).with_auth("user", "pass");
        assert_eq!(proxy.url(), "http://user:pass@127.0.0.1:8080");
    }

    #[test]
    fn test_proxy_strategy_default() {
        let strategy = ProxyStrategy::default();
        assert!(matches!(strategy, ProxyStrategy::RoundRobin));
    }

    #[tokio::test]
    async fn test_static_proxy_provider() {
        let proxies = vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
        ];
        let provider = StaticProxyProvider::new(proxies);
        let fetched = provider.fetch_proxies().await.unwrap();
        assert_eq!(fetched.len(), 2);
        assert_eq!(provider.refresh_interval(), Duration::from_secs(u64::MAX));
    }

    #[tokio::test]
    async fn test_proxy_pool_new() {
        let pool = ProxyPool::new();
        assert!(!pool.is_enabled());
        assert!(pool.is_empty().await);
    }

    #[tokio::test]
    async fn test_proxy_pool_default() {
        let pool = ProxyPool::default();
        assert!(!pool.is_enabled());
    }

    #[tokio::test]
    async fn test_proxy_pool_with_proxies() {
        let proxies = vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
        ];
        let pool = ProxyPool::with_proxies(proxies);
        assert!(pool.is_enabled());
        assert_eq!(pool.len().await, 2);
    }

    #[tokio::test]
    async fn test_proxy_pool_with_empty_proxies() {
        let pool = ProxyPool::with_proxies(vec![]);
        assert!(!pool.is_enabled());
        assert!(pool.is_empty().await);
    }

    #[tokio::test]
    async fn test_proxy_pool_with_strategy() {
        let pool = ProxyPool::new().with_strategy(ProxyStrategy::Random);
        assert!(matches!(pool.strategy, ProxyStrategy::Random));
    }

    #[tokio::test]
    async fn test_proxy_pool_set_enabled() {
        let mut pool = ProxyPool::new();
        assert!(!pool.is_enabled());
        pool.set_enabled(true);
        assert!(pool.is_enabled());
    }

    #[tokio::test]
    async fn test_proxy_pool_add_proxy() {
        let pool = ProxyPool::new();
        pool.add_proxy(ProxyConfig::new("127.0.0.1", 8080)).await;
        assert_eq!(pool.len().await, 1);
    }

    #[tokio::test]
    async fn test_proxy_pool_remove_proxy() {
        let proxies = vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
        ];
        let pool = ProxyPool::with_proxies(proxies);
        pool.remove_proxy("127.0.0.1", 8080).await;
        assert_eq!(pool.len().await, 1);
    }

    #[tokio::test]
    async fn test_proxy_pool_get_proxy_disabled() {
        let proxies = vec![ProxyConfig::new("127.0.0.1", 8080)];
        let mut pool = ProxyPool::with_proxies(proxies);
        pool.set_enabled(false);
        assert!(pool.get_proxy().await.is_none());
    }

    #[tokio::test]
    async fn test_proxy_pool_get_proxy_empty() {
        let mut pool = ProxyPool::new();
        pool.set_enabled(true);
        assert!(pool.get_proxy().await.is_none());
    }

    #[tokio::test]
    async fn test_proxy_pool_get_proxy_round_robin() {
        let proxies = vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
            ProxyConfig::new("127.0.0.1", 8082),
        ];
        let pool = ProxyPool::with_proxies(proxies);

        let p1 = pool.get_proxy().await.unwrap();
        let p2 = pool.get_proxy().await.unwrap();
        let p3 = pool.get_proxy().await.unwrap();
        let p4 = pool.get_proxy().await.unwrap();

        assert_eq!(p1.port, 8080);
        assert_eq!(p2.port, 8081);
        assert_eq!(p3.port, 8082);
        assert_eq!(p4.port, 8080); // Wraps around
    }

    #[tokio::test]
    async fn test_proxy_pool_get_proxy_random() {
        let proxies = vec![
            ProxyConfig::new("127.0.0.1", 8080),
            ProxyConfig::new("127.0.0.1", 8081),
        ];
        let pool = ProxyPool::with_proxies(proxies).with_strategy(ProxyStrategy::Random);

        // Just verify it returns a valid proxy
        let proxy = pool.get_proxy().await.unwrap();
        assert!(proxy.port == 8080 || proxy.port == 8081);
    }

    #[tokio::test]
    async fn test_proxy_pool_refresh_no_provider() {
        let pool = ProxyPool::new();
        // Should not error when no provider
        pool.refresh().await.unwrap();
    }

    #[tokio::test]
    async fn test_proxy_pool_with_provider() {
        let proxies = vec![ProxyConfig::new("127.0.0.1", 8080)];
        let provider = StaticProxyProvider::new(proxies);
        let pool = ProxyPool::with_provider(provider);
        assert!(pool.is_enabled());

        // Initially empty until refresh
        assert!(pool.is_empty().await);

        // After refresh, should have proxies
        pool.refresh().await.unwrap();
        assert_eq!(pool.len().await, 1);
    }

    #[tokio::test]
    async fn test_proxy_pool_create_client_no_proxy() {
        let pool = ProxyPool::new();
        let client = pool.create_client("test-agent").await.unwrap();
        // Client should be created successfully without proxy
        drop(client);
    }

    #[tokio::test]
    async fn test_proxy_pool_create_client_with_proxy() {
        let proxies = vec![ProxyConfig::new("127.0.0.1", 8080)];
        let pool = ProxyPool::with_proxies(proxies);
        let client = pool.create_client("test-agent").await.unwrap();
        // Client should be created with proxy configured
        drop(client);
    }

    #[test]
    fn test_proxy_config_debug() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080);
        let debug_str = format!("{:?}", proxy);
        assert!(debug_str.contains("127.0.0.1"));
        assert!(debug_str.contains("8080"));
    }

    #[test]
    fn test_proxy_config_clone() {
        let proxy = ProxyConfig::new("127.0.0.1", 8080)
            .with_protocol(ProxyProtocol::Socks5)
            .with_auth("user", "pass");
        let cloned = proxy.clone();
        assert_eq!(cloned.host, proxy.host);
        assert_eq!(cloned.port, proxy.port);
        assert_eq!(cloned.protocol, proxy.protocol);
        assert_eq!(cloned.username, proxy.username);
        assert_eq!(cloned.password, proxy.password);
    }

    #[test]
    fn test_proxy_protocol_debug() {
        let protocol = ProxyProtocol::Socks5;
        let debug_str = format!("{:?}", protocol);
        assert!(debug_str.contains("Socks5"));
    }

    #[test]
    fn test_proxy_protocol_clone() {
        let protocol = ProxyProtocol::Https;
        #[allow(clippy::clone_on_copy)]
        let cloned = protocol.clone();
        assert_eq!(cloned, protocol);
    }

    #[test]
    fn test_proxy_protocol_copy() {
        let protocol = ProxyProtocol::Http;
        let copied: ProxyProtocol = protocol;
        assert_eq!(copied, protocol);
    }

    #[test]
    fn test_proxy_strategy_debug() {
        let strategy = ProxyStrategy::Random;
        let debug_str = format!("{:?}", strategy);
        assert!(debug_str.contains("Random"));
    }

    #[test]
    fn test_proxy_strategy_clone() {
        let strategy = ProxyStrategy::RoundRobin;
        #[allow(clippy::clone_on_copy)]
        let cloned = strategy.clone();
        assert!(matches!(cloned, ProxyStrategy::RoundRobin));
    }

    #[test]
    fn test_proxy_strategy_copy() {
        let strategy = ProxyStrategy::Random;
        let copied: ProxyStrategy = strategy;
        assert!(matches!(copied, ProxyStrategy::Random));
    }

    #[tokio::test]
    async fn test_proxy_pool_len_after_add() {
        let pool = ProxyPool::new();
        assert_eq!(pool.len().await, 0);
        pool.add_proxy(ProxyConfig::new("127.0.0.1", 8080)).await;
        pool.add_proxy(ProxyConfig::new("127.0.0.1", 8081)).await;
        assert_eq!(pool.len().await, 2);
    }

    #[tokio::test]
    async fn test_proxy_pool_remove_nonexistent() {
        let proxies = vec![ProxyConfig::new("127.0.0.1", 8080)];
        let pool = ProxyPool::with_proxies(proxies);
        pool.remove_proxy("192.168.1.1", 9999).await;
        assert_eq!(pool.len().await, 1); // Should still have the original
    }

    #[test]
    fn test_proxy_config_url_partial_auth() {
        // Test with only username (no password)
        let mut proxy = ProxyConfig::new("127.0.0.1", 8080);
        proxy.username = Some("user".to_string());
        proxy.password = None;
        // Should not include auth when password is missing
        assert_eq!(proxy.url(), "http://127.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_proxy_provider_default_refresh_interval() {
        struct CustomProvider;

        #[async_trait]
        impl ProxyProvider for CustomProvider {
            async fn fetch_proxies(&self) -> Result<Vec<ProxyConfig>> {
                Ok(vec![])
            }
            // Don't override refresh_interval to test default
        }

        let provider = CustomProvider;
        assert_eq!(provider.refresh_interval(), Duration::from_secs(300));
    }
}
