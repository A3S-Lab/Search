//! Baidu search engine implementation using headless browser.
//!
//! This engine requires the `headless` feature because Baidu's search results
//! page relies on JavaScript rendering that plain HTTP requests cannot handle.

use crate::html_engine::{
    selector, validate_search_response, HtmlEngine, HtmlParser, SearchResponseSpec,
};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// Baidu HTML parser.
pub struct BaiduParser;

/// Baidu search engine (百度).
pub type Baidu = HtmlEngine<BaiduParser>;

impl Baidu {
    /// Creates a new Baidu engine with the given page fetcher.
    pub fn new(fetcher: std::sync::Arc<dyn crate::PageFetcher>) -> Self {
        HtmlEngine::with_fetcher(BaiduParser, fetcher)
    }
}

impl HtmlParser for BaiduParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Baidu".to_string(),
            shortcut: "baidu".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 10,
            enabled: true,
            paging: true,
            safesearch: false,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        let mut url = format!(
            "https://www.baidu.com/s?wd={}",
            urlencoding::encode(&query.query)
        );
        if query.page > 1 {
            url.push_str(&format!("&pn={}", (query.page - 1) * 10));
        }
        url
    }

    fn validate(&self, html: &str) -> Result<()> {
        validate_search_response(
            html,
            SearchResponseSpec {
                engine: "Baidu",
                result_selectors: &["div.result", "div.c-container"],
                empty_selectors: &["#content_left:empty", "#content_none", ".no-result"],
                challenge_selectors: &[
                    "script[src*=\"mkdjump\"]",
                    "a[href*=\"/captcha/\"]",
                    "iframe[src*=\"/captcha/\"]",
                    "form[action*=\"/captcha/\"]",
                ],
            },
        )
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);
        let result_sel = selector("div.result, div.c-container")?;
        let title_sel = selector("h3 a, .t a")?;
        let snippet_sel = selector(".c-abstract, .c-span-last, .content-right_8Zs40")?;

        let mut results = Vec::new();

        for element in document.select(&result_sel) {
            let title_elem = match element.select(&title_sel).next() {
                Some(el) => el,
                None => continue,
            };

            let title = title_elem.text().collect::<String>().trim().to_string();
            let url = title_elem
                .value()
                .attr("href")
                .unwrap_or_default()
                .to_string();

            let content = element
                .select(&snippet_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
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

    fn make_baidu() -> Baidu {
        Baidu::new(Arc::new(HttpFetcher::new()))
    }

    #[test]
    fn test_baidu_new() {
        let engine = make_baidu();
        assert_eq!(engine.config().name, "Baidu");
        assert_eq!(engine.config().shortcut, "baidu");
        assert_eq!(engine.config().categories, vec![EngineCategory::General]);
        assert_eq!(engine.config().weight, 1.0);
        assert_eq!(engine.config().timeout, 10);
        assert!(engine.config().enabled);
        assert!(engine.config().paging);
        assert!(!engine.config().safesearch);
    }

    #[test]
    fn test_baidu_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Baidu".to_string(),
            shortcut: "cbaidu".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = make_baidu().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom Baidu");
        assert_eq!(engine.config().shortcut, "cbaidu");
        assert_eq!(engine.config().weight, 1.5);
    }

    #[test]
    fn test_baidu_engine_trait() {
        let engine = make_baidu();
        assert_eq!(engine.name(), "Baidu");
        assert_eq!(engine.shortcut(), "baidu");
        assert_eq!(engine.weight(), 1.0);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_parse_results_empty_html() {
        let parser = BaiduParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_validate_classifies_result_empty_challenge_and_drift_fixtures() {
        let parser = BaiduParser;
        let result = r#"<main id="content_left"><div class="c-container"></div></main>"#;
        let empty = r#"<main id="content_left"></main>"#;
        let challenge =
            r#"<script src="https://static.example/static/touch/js/mkdjump_v2.js"></script>"#;

        assert!(parser.validate(result).is_ok());
        assert!(parser.validate(empty).is_ok());
        assert_eq!(parser.validate(challenge).unwrap_err().kind(), "challenge");
        assert_eq!(
            parser
                .validate("<html><body><main id=homepage></main></body></html>")
                .unwrap_err()
                .kind(),
            "invalid_response"
        );
    }

    #[test]
    fn test_parse_results_with_results() {
        let parser = BaiduParser;
        let html = r#"
            <html>
            <body>
                <div class="c-container">
                    <h3><a href="https://www.rust-lang.org/">Rust 编程语言</a></h3>
                    <div class="c-abstract">一门赋予每个人构建可靠软件能力的语言。</div>
                </div>
                <div class="result">
                    <h3><a href="https://doc.rust-lang.org/book/">Rust 程序设计语言</a></h3>
                    <div class="c-abstract">Rust 官方教程。</div>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust 编程语言");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].content, "一门赋予每个人构建可靠软件能力的语言。");
        assert_eq!(results[1].title, "Rust 程序设计语言");
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        let parser = BaiduParser;
        let html = r#"
            <html>
            <body>
                <div class="c-container">
                    <div class="c-abstract">No title here</div>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_empty_url() {
        let parser = BaiduParser;
        let html = r#"
            <html>
            <body>
                <div class="c-container">
                    <h3><a href="">Empty URL</a></h3>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }
}
