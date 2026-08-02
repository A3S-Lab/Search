//! Brave search engine implementation.

use crate::html_engine::{
    selector, validate_search_response, HtmlEngine, HtmlParser, SearchResponseSpec,
};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// Brave HTML parser.
pub struct BraveParser;

/// Brave browser-rendered HTML parser.
pub struct BraveBrowserParser;

/// Brave search engine.
pub type Brave = HtmlEngine<BraveParser>;

/// Brave Search through a browser renderer.
pub type BraveBrowser = HtmlEngine<BraveBrowserParser>;

impl Brave {
    /// Creates a new Brave engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(BraveParser, std::sync::Arc::new(crate::HttpFetcher::new()))
    }
}

impl BraveBrowser {
    /// Creates a browser-rendered Brave engine with the given page fetcher.
    pub fn new(fetcher: std::sync::Arc<dyn crate::PageFetcher>) -> Self {
        HtmlEngine::with_fetcher(BraveBrowserParser, fetcher)
    }
}

impl Default for Brave {
    fn default() -> Self {
        Brave::new()
    }
}

impl HtmlParser for BraveParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Brave".to_string(),
            shortcut: "brave".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 5,
            enabled: true,
            paging: true,
            safesearch: true,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        use crate::query::SafeSearch;
        let mut url = format!(
            "https://search.brave.com/search?q={}",
            urlencoding::encode(&query.query)
        );
        if query.page > 1 {
            url.push_str(&format!("&offset={}", query.page - 1));
        }
        match query.safesearch {
            SafeSearch::Off => {}
            SafeSearch::Moderate => url.push_str("&safesearch=moderate"),
            SafeSearch::Strict => url.push_str("&safesearch=strict"),
        }
        url
    }

    fn validate(&self, html: &str) -> Result<()> {
        let document = Html::parse_document(html);
        let title_selector = selector("title")?;
        let title = document
            .select(&title_selector)
            .next()
            .map(|element| element.text().collect::<String>().to_ascii_lowercase());
        let lowercase = html.to_ascii_lowercase();
        if title.is_some_and(|title| title.contains("captcha"))
            || lowercase.contains(r#"page:"/captcha""#)
            || lowercase.contains("schedule a captcha")
        {
            return Err(crate::SearchError::Challenge(
                "Brave returned a CAPTCHA or challenge instead of search results".to_string(),
            ));
        }

        validate_search_response(
            html,
            SearchResponseSpec {
                engine: "Brave",
                result_selectors: &[r#"div.snippet[data-type="web"]"#],
                empty_selectors: &[
                    "#results:empty",
                    "#results .no-results",
                    "[data-testid=\"no-results\"]",
                ],
                challenge_selectors: &[
                    ".captcha-wrapper",
                    "form[action*=\"challenge\"]",
                    "iframe[src*=\"captcha\"]",
                    "[data-testid*=\"captcha\"]",
                ],
            },
        )
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);
        let result_sel = selector(r#"div.snippet[data-type="web"]"#)?;
        let title_sel = selector(".search-snippet-title")?;
        let desc_sel = selector(".generic-snippet .content, .snippet-description")?;
        let url_sel = selector(r#"a[href^="http"]"#)?;

        let mut results = Vec::new();

        for element in document.select(&result_sel) {
            let title = element
                .select(&title_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let url = element
                .select(&url_sel)
                .next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or_default()
                .to_string();

            let content = element
                .select(&desc_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if !url.is_empty() && !title.is_empty() && url.starts_with("http") {
                results.push(SearchResult::new(url, title, content));
            }
        }

        Ok(results)
    }
}

impl HtmlParser for BraveBrowserParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Brave".to_string(),
            shortcut: "brave_browser".to_string(),
            ..BraveParser::default_config()
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        BraveParser.build_url(query)
    }

    fn validate(&self, html: &str) -> Result<()> {
        BraveParser.validate(html)
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        BraveParser.parse(html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use crate::HttpFetcher;
    use std::sync::Arc;

    #[test]
    fn test_brave_new() {
        let engine = Brave::new();
        assert_eq!(engine.config().name, "Brave");
        assert_eq!(engine.config().shortcut, "brave");
        assert_eq!(engine.config().weight, 1.0);
    }

    #[test]
    fn test_brave_with_fetcher() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = Brave::with_fetcher(BraveParser, fetcher);
        assert_eq!(engine.config().name, "Brave");
    }

    #[test]
    fn test_brave_default() {
        let engine = Brave::default();
        assert_eq!(engine.config().name, "Brave");
    }

    #[test]
    fn browser_variant_shares_source_identity_and_has_an_independent_shortcut() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let http = Brave::new();
        let engine = BraveBrowser::new(fetcher);

        assert_eq!(engine.name(), http.name());
        assert_eq!(engine.shortcut(), "brave_browser");
        assert_ne!(engine.shortcut(), http.shortcut());
    }

    #[test]
    fn test_brave_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Brave".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = Brave::new().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom Brave");
    }

    #[test]
    fn test_brave_engine_trait() {
        let engine = Brave::new();
        assert_eq!(engine.name(), "Brave");
        assert_eq!(engine.shortcut(), "brave");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_brave_parse_results_empty() {
        let parser = BraveParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_validate_classifies_result_empty_challenge_and_drift_fixtures() {
        let parser = BraveParser;
        let result = r#"<main id="search-page"><div id="results"><div class="snippet" data-type="web"></div></div></main>"#;
        let empty = r#"<main id="search-page"><div id="results"><div class="no-results"></div></div></main>"#;
        let challenge = r#"<main><iframe src="/captcha/challenge"></iframe></main>"#;
        let proof_of_work_challenge = r#"<title>Captcha - Brave Search</title><main class="captcha-wrapper"><h1>Verify you are human</h1></main>"#;
        let scheduled_challenge =
            r#"<title>Brave Search</title><script>data={page:"/captcha"}</script>"#;

        assert!(parser.validate(result).is_ok());
        assert!(parser.validate(empty).is_ok());
        assert_eq!(parser.validate(challenge).unwrap_err().kind(), "challenge");
        assert_eq!(
            parser.validate(proof_of_work_challenge).unwrap_err().kind(),
            "challenge"
        );
        assert_eq!(
            parser.validate(scheduled_challenge).unwrap_err().kind(),
            "challenge"
        );
        assert_eq!(
            parser
                .validate("<html><body><main id=homepage></main></body></html>")
                .unwrap_err()
                .kind(),
            "invalid_response"
        );
    }

    #[test]
    fn test_brave_parse_results_with_data() {
        let parser = BraveParser;
        let html = r#"
        <html><body>
        <div class="snippet" data-type="web">
            <a href="https://www.rust-lang.org/" class="search-snippet-title">Rust Programming Language</a>
            <div class="generic-snippet"><div class="content">A systems programming language focused on safety.</div></div>
        </div>
        <div class="snippet" data-type="web">
            <a href="https://doc.rust-lang.org/book/" class="search-snippet-title">The Rust Book</a>
            <div class="snippet-description">Official Rust programming guide.</div>
        </div>
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
    fn test_brave_parse_results_skips_non_web() {
        let parser = BraveParser;
        let html = r#"
        <html><body>
        <div class="snippet" data-type="video">
            <a href="https://example.com/video" class="search-snippet-title">A Video</a>
        </div>
        <div class="snippet" data-type="web">
            <a href="https://example.com/page" class="search-snippet-title">A Page</a>
        </div>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "A Page");
    }
}
