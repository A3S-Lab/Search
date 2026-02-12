//! Google search engine implementation using headless browser.
//!
//! This engine requires the `headless` feature because Google's search results
//! page relies on JavaScript rendering that plain HTTP requests cannot handle.

use std::sync::Arc;

use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::fetcher::PageFetcher;
use crate::{Engine, EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};

/// Google search engine.
///
/// Requires a `PageFetcher` (typically a `BrowserFetcher`) to render
/// Google's JavaScript-heavy result pages.
pub struct Google {
    config: EngineConfig,
    fetcher: Arc<dyn PageFetcher>,
}

impl Google {
    /// Creates a new Google engine with the given page fetcher.
    pub fn new(fetcher: Arc<dyn PageFetcher>) -> Self {
        Self {
            config: EngineConfig {
                name: "Google".to_string(),
                shortcut: "g".to_string(),
                categories: vec![EngineCategory::General],
                weight: 1.5,
                timeout: 10,
                enabled: true,
                paging: true,
                safesearch: true,
            },
            fetcher,
        }
    }

    /// Creates with custom configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        let container_selector = Selector::parse("div.g")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let title_selector = Selector::parse("h3")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let link_selector = Selector::parse("a[href]")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;
        let snippet_selector = Selector::parse("div[data-sncf], div.VwiC3b")
            .map_err(|e| SearchError::Parse(format!("Failed to parse selector: {:?}", e)))?;

        let mut results = Vec::new();

        for element in document.select(&container_selector) {
            let title = match element.select(&title_selector).next() {
                Some(el) => el.text().collect::<String>().trim().to_string(),
                None => continue,
            };

            let url = match element.select(&link_selector).next() {
                Some(el) => {
                    let href = el.value().attr("href").unwrap_or_default();
                    // Skip Google's internal links
                    if href.starts_with('/') && !href.starts_with("/url?") {
                        continue;
                    }
                    // Extract actual URL from /url?q= redirects
                    if let Some(q) = href.strip_prefix("/url?q=") {
                        q.split('&').next().unwrap_or(q).to_string()
                    } else {
                        href.to_string()
                    }
                }
                None => continue,
            };

            let content = element
                .select(&snippet_selector)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if !url.is_empty() && !title.is_empty() {
                results.push(SearchResult::new(url, title, content));
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl Engine for Google {
    fn config(&self) -> &EngineConfig {
        &self.config
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.google.com/search?q={}&hl=en",
            urlencoding::encode(&query.query)
        );

        let html = self.fetcher.fetch(&url).await?;

        // Detect CAPTCHA / bot-block pages before parsing
        if html.contains("/sorry/index") || html.contains("recaptcha") {
            return Err(SearchError::Other(
                "Google returned a CAPTCHA page (bot detected). Try again later or use a proxy (-p)."
                    .to_string(),
            ));
        }

        self.parse_results(&html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetcher_http::HttpFetcher;

    fn make_google() -> Google {
        Google::new(Arc::new(HttpFetcher::new()))
    }

    #[test]
    fn test_google_new() {
        let engine = make_google();
        assert_eq!(engine.config.name, "Google");
        assert_eq!(engine.config.shortcut, "g");
        assert_eq!(engine.config.categories, vec![EngineCategory::General]);
        assert_eq!(engine.config.weight, 1.5);
        assert_eq!(engine.config.timeout, 10);
        assert!(engine.config.enabled);
        assert!(engine.config.paging);
        assert!(engine.config.safesearch);
    }

    #[test]
    fn test_google_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Google".to_string(),
            shortcut: "cg".to_string(),
            weight: 2.0,
            ..Default::default()
        };
        let engine = make_google().with_config(custom_config);
        assert_eq!(engine.name(), "Custom Google");
        assert_eq!(engine.shortcut(), "cg");
        assert_eq!(engine.weight(), 2.0);
    }

    #[test]
    fn test_google_engine_trait() {
        let engine = make_google();
        assert_eq!(engine.name(), "Google");
        assert_eq!(engine.shortcut(), "g");
        assert_eq!(engine.weight(), 1.5);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_parse_results_empty_html() {
        let engine = make_google();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_results() {
        let engine = make_google();
        let html = r#"
            <html>
            <body>
                <div class="g">
                    <a href="https://www.rust-lang.org/">
                        <h3>Rust Programming Language</h3>
                    </a>
                    <div class="VwiC3b">A language empowering everyone to build reliable software.</div>
                </div>
                <div class="g">
                    <a href="https://doc.rust-lang.org/book/">
                        <h3>The Rust Programming Language Book</h3>
                    </a>
                    <div class="VwiC3b">The official Rust book.</div>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            results[0].content,
            "A language empowering everyone to build reliable software."
        );
        assert_eq!(results[1].title, "The Rust Programming Language Book");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
    }

    #[test]
    fn test_parse_results_with_redirect_url() {
        let engine = make_google();
        let html = r#"
            <html>
            <body>
                <div class="g">
                    <a href="/url?q=https://example.com/page&sa=U">
                        <h3>Example Page</h3>
                    </a>
                    <div data-sncf="1">Example snippet</div>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
        assert_eq!(results[0].content, "Example snippet");
    }

    #[test]
    fn test_parse_results_skips_internal_links() {
        let engine = make_google();
        let html = r#"
            <html>
            <body>
                <div class="g">
                    <a href="/search?q=related">
                        <h3>Related Search</h3>
                    </a>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        let engine = make_google();
        let html = r#"
            <html>
            <body>
                <div class="g">
                    <a href="https://example.com">No h3 here</a>
                </div>
            </body>
            </html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_detects_captcha_sorry_page() {
        use crate::fetcher::PageFetcher;

        struct FakeFetcher(String);
        #[async_trait]
        impl PageFetcher for FakeFetcher {
            async fn fetch(&self, _url: &str) -> crate::Result<String> {
                Ok(self.0.clone())
            }
        }

        let html = r#"<html><body>
            <a href="/sorry/index?continue=https://www.google.com/search">blocked</a>
        </body></html>"#;
        let fetcher = Arc::new(FakeFetcher(html.to_string()));
        let engine = Google::new(fetcher);
        let result = engine.search(&SearchQuery::new("test")).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("CAPTCHA"),
            "Expected CAPTCHA error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_search_detects_captcha_recaptcha() {
        use crate::fetcher::PageFetcher;

        struct FakeFetcher(String);
        #[async_trait]
        impl PageFetcher for FakeFetcher {
            async fn fetch(&self, _url: &str) -> crate::Result<String> {
                Ok(self.0.clone())
            }
        }

        let html = r#"<html><body>
            <iframe src="https://www.google.com/recaptcha/enterprise/anchor"></iframe>
        </body></html>"#;
        let fetcher = Arc::new(FakeFetcher(html.to_string()));
        let engine = Google::new(fetcher);
        let result = engine.search(&SearchQuery::new("test")).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("CAPTCHA"),
            "Expected CAPTCHA error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_search_normal_page_no_captcha() {
        use crate::fetcher::PageFetcher;

        struct FakeFetcher(String);
        #[async_trait]
        impl PageFetcher for FakeFetcher {
            async fn fetch(&self, _url: &str) -> crate::Result<String> {
                Ok(self.0.clone())
            }
        }

        let html = r#"<html><body>
            <div class="g">
                <a href="https://www.rust-lang.org/"><h3>Rust</h3></a>
                <div class="VwiC3b">A systems language.</div>
            </div>
        </body></html>"#;
        let fetcher = Arc::new(FakeFetcher(html.to_string()));
        let engine = Google::new(fetcher);
        let result = engine.search(&SearchQuery::new("test")).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }
}
