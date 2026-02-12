//! HTTP-based page fetcher using reqwest.

use async_trait::async_trait;
use reqwest::Client;

use crate::fetcher::PageFetcher;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
