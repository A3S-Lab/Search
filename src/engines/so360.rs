//! 360 Search engine implementation.

use crate::html_engine::{
    selector, validate_search_response, HtmlEngine, HtmlParser, SearchResponseSpec,
};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// 360 Search HTML parser.
pub struct So360Parser;

/// 360 Search engine (360搜索).
pub type So360 = HtmlEngine<So360Parser>;

impl So360 {
    /// Creates a new 360 Search engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(So360Parser, std::sync::Arc::new(crate::HttpFetcher::new()))
    }
}

impl Default for So360 {
    fn default() -> Self {
        So360::new()
    }
}

impl HtmlParser for So360Parser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "360 Search".to_string(),
            shortcut: "360".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 5,
            enabled: true,
            paging: true,
            safesearch: false,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        let mut url = format!(
            "https://www.so.com/s?q={}",
            urlencoding::encode(&query.query)
        );
        if query.page > 1 {
            url.push_str(&format!("&pn={}", query.page));
        }
        url
    }

    fn validate(&self, html: &str) -> Result<()> {
        validate_search_response(
            html,
            SearchResponseSpec {
                engine: "360 Search",
                result_selectors: &["li.res-list"],
                empty_selectors: &["#main .result:empty", ".no-results", ".no-result"],
                challenge_selectors: &[
                    "form[action*=\"verify\"]",
                    "iframe[src*=\"captcha\"]",
                    "script[src*=\"captcha\"]",
                ],
            },
        )
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);
        let result_sel = selector("li.res-list")?;
        let title_sel = selector("h3 a")?;
        let snippet_sel = selector(".res-desc, .res-rich")?;

        let mut results = Vec::new();

        for element in document.select(&result_sel) {
            let title_elem = match element.select(&title_sel).next() {
                Some(el) => el,
                None => continue,
            };

            let title = title_elem.text().collect::<String>().trim().to_string();

            // 360 Search stores the real URL in data-mdurl, falling back to href
            let url = title_elem
                .value()
                .attr("data-mdurl")
                .or_else(|| title_elem.value().attr("href"))
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
    use crate::Engine;
    use crate::HttpFetcher;
    use std::sync::Arc;

    #[test]
    fn test_so360_new() {
        let engine = So360::new();
        assert_eq!(engine.config().name, "360 Search");
        assert_eq!(engine.config().shortcut, "360");
        assert_eq!(engine.config().weight, 1.0);
    }

    #[test]
    fn test_so360_with_fetcher() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = So360::with_fetcher(So360Parser, fetcher);
        assert_eq!(engine.config().name, "360 Search");
    }

    #[test]
    fn test_so360_default() {
        let engine = So360::default();
        assert_eq!(engine.config().name, "360 Search");
    }

    #[test]
    fn test_so360_with_config() {
        let custom_config = EngineConfig {
            name: "Custom 360".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = So360::new().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom 360");
    }

    #[test]
    fn test_so360_engine_trait() {
        let engine = So360::new();
        assert_eq!(engine.name(), "360 Search");
        assert_eq!(engine.shortcut(), "360");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_so360_parse_results_empty() {
        let parser = So360Parser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_validate_classifies_result_empty_challenge_and_drift_fixtures() {
        let parser = So360Parser;
        let result =
            r#"<main id="main"><div class="result"><li class="res-list"></li></div></main>"#;
        let empty = r#"<main id="main"><div class="result"></div></main>"#;
        let challenge = r#"<main><form action="/verify"></form></main>"#;

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
    fn test_so360_parse_results_with_data_mdurl() {
        let parser = So360Parser;
        let html = r#"
        <html><body>
        <li class="res-list">
            <h3><a href="https://www.so.com/link?m=redirect_url" data-mdurl="https://www.rust-lang.org/">Rust Programming Language</a></h3>
            <div class="res-desc">A systems programming language focused on safety.</div>
        </li>
        <li class="res-list">
            <h3><a href="https://www.so.com/link?m=redirect_url2" data-mdurl="https://doc.rust-lang.org/book/">The Rust Book</a></h3>
            <div class="res-rich">Official Rust programming guide.</div>
        </li>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            results[0].content,
            "A systems programming language focused on safety."
        );
        assert_eq!(results[1].title, "The Rust Book");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
        assert_eq!(results[1].content, "Official Rust programming guide.");
    }

    #[test]
    fn test_so360_parse_results_fallback_to_href() {
        let parser = So360Parser;
        let html = r#"
        <html><body>
        <li class="res-list">
            <h3><a href="https://example.com/page">Example Page</a></h3>
            <div class="res-desc">A page without data-mdurl.</div>
        </li>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
    }
}
