//! Google search engine implementation using headless browser.
//!
//! This engine requires the `headless` feature because Google's search results
//! page relies on JavaScript rendering that plain HTTP requests cannot handle.

use crate::html_engine::{
    selector, validate_search_response, HtmlEngine, HtmlParser, SearchResponseSpec,
};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// Google HTML parser with CAPTCHA detection.
pub struct GoogleParser;

/// Google search engine.
pub type Google = HtmlEngine<GoogleParser>;

impl Google {
    /// Creates a new Google engine with the given page fetcher.
    pub fn new(fetcher: std::sync::Arc<dyn crate::PageFetcher>) -> Self {
        HtmlEngine::with_fetcher(GoogleParser, fetcher)
    }
}

impl HtmlParser for GoogleParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Google".to_string(),
            shortcut: "g".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.5,
            timeout: 10,
            enabled: true,
            paging: true,
            safesearch: true,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        use crate::query::{SafeSearch, TimeRange};
        let mut url = format!(
            "https://www.google.com/search?q={}&hl=en",
            urlencoding::encode(&query.query)
        );
        if query.page > 1 {
            url.push_str(&format!("&start={}", (query.page - 1) * 10));
        }
        match query.safesearch {
            SafeSearch::Off => {}
            SafeSearch::Moderate => url.push_str("&safe=medium"),
            SafeSearch::Strict => url.push_str("&safe=active"),
        }
        if let Some(range) = query.time_range {
            let tbs = match range {
                TimeRange::Day => "d",
                TimeRange::Week => "w",
                TimeRange::Month => "m",
                TimeRange::Year => "y",
            };
            url.push_str(&format!("&tbs=qdr:{}", tbs));
        }
        url
    }

    fn validate(&self, html: &str) -> Result<()> {
        validate_search_response(
            html,
            SearchResponseSpec {
                engine: "Google",
                result_selectors: &["div.g"],
                empty_selectors: &[
                    "#search:empty",
                    "#search .no-results",
                    "#topstuff .card-section",
                ],
                challenge_selectors: &[
                    "a[href*=\"/sorry/\"]",
                    "form[action*=\"/sorry/\"]",
                    "iframe[src*=\"recaptcha\"]",
                    "script[src*=\"recaptcha\"]",
                    "form[action*=\"consent\"]",
                ],
            },
        )
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);
        let container_sel = selector("div.g")?;
        let title_sel = selector("h3")?;
        let link_sel = selector("a[href]")?;
        let snippet_sel = selector("div[data-sncf], div.VwiC3b")?;

        let mut results = Vec::new();

        for element in document.select(&container_sel) {
            let title = match element.select(&title_sel).next() {
                Some(el) => el.text().collect::<String>().trim().to_string(),
                None => continue,
            };

            let url = match element.select(&link_sel).next() {
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
                .select(&snippet_sel)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetcher_http::HttpFetcher;
    use crate::Engine;
    use std::sync::Arc;

    fn make_google() -> Google {
        Google::new(Arc::new(HttpFetcher::new()))
    }

    #[test]
    fn test_google_new() {
        let engine = make_google();
        assert_eq!(engine.config().name, "Google");
        assert_eq!(engine.config().shortcut, "g");
        assert_eq!(engine.config().categories, vec![EngineCategory::General]);
        assert_eq!(engine.config().weight, 1.5);
        assert_eq!(engine.config().timeout, 10);
        assert!(engine.config().enabled);
        assert!(engine.config().paging);
        assert!(engine.config().safesearch);
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
        assert_eq!(engine.config().name, "Custom Google");
        assert_eq!(engine.config().shortcut, "cg");
        assert_eq!(engine.config().weight, 2.0);
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
        let parser = GoogleParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_results() {
        let parser = GoogleParser;
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
        let results = parser.parse(html).unwrap();
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
        let parser = GoogleParser;
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
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
        assert_eq!(results[0].content, "Example snippet");
    }

    #[test]
    fn test_parse_results_skips_internal_links() {
        let parser = GoogleParser;
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
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        let parser = GoogleParser;
        let html = r#"
            <html>
            <body>
                <div class="g">
                    <a href="https://example.com">No h3 here</a>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_validate_detects_sorry_page() {
        let parser = GoogleParser;
        let html = r#"<html><body>
            <a href="/sorry/index?continue=https://www.google.com/search">blocked</a>
        </body></html>"#;
        assert!(parser.validate(html).is_err());
    }

    #[test]
    fn test_validate_detects_recaptcha() {
        let parser = GoogleParser;
        let html = r#"<html><body>
            <iframe src="https://www.google.com/recaptcha/enterprise/anchor"></iframe>
        </body></html>"#;
        assert!(parser.validate(html).is_err());
    }

    #[test]
    fn test_validate_passes_normal_page() {
        let parser = GoogleParser;
        let html = r#"<html><body>
            <div class="g"><a href="https://example.com"><h3>Test</h3></a></div>
        </body></html>"#;
        assert!(parser.validate(html).is_ok());
    }

    #[test]
    fn test_validate_classifies_empty_consent_and_drift_fixtures() {
        let parser = GoogleParser;
        let empty = r#"<main id="search"></main>"#;
        let consent = r#"<form action="https://consent.google.com/save"></form>"#;

        assert!(parser.validate(empty).is_ok());
        assert_eq!(parser.validate(consent).unwrap_err().kind(), "challenge");
        assert_eq!(
            parser
                .validate("<html><body><main id=homepage></main></body></html>")
                .unwrap_err()
                .kind(),
            "invalid_response"
        );
    }

    #[tokio::test]
    async fn test_search_detects_captcha_sorry_page() {
        use crate::fetcher::PageFetcher;
        use async_trait::async_trait;

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
        use async_trait::async_trait;

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
        use async_trait::async_trait;

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
