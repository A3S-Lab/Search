//! HTTP-based page fetcher using reqwest.

use async_trait::async_trait;
use reqwest::Client;

use crate::fetcher::PageFetcher;
use crate::Result;

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
                .user_agent("Mozilla/5.0 (compatible; a3s-search/0.3)")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Creates an `HttpFetcher` with a custom reqwest client.
    pub fn with_client(client: Client) -> Self {
        Self { client }
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
        let client = Client::builder()
            .user_agent("test-agent")
            .build()
            .unwrap();
        let _fetcher = HttpFetcher::with_client(client);
    }
}
