//! Brave search engine implementation.

use crate::html_engine::{selector, HtmlEngine, HtmlParser};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// Brave HTML parser.
pub struct BraveParser;

/// Brave search engine.
pub type Brave = HtmlEngine<BraveParser>;

impl Brave {
    /// Creates a new Brave engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(BraveParser, std::sync::Arc::new(crate::HttpFetcher::new()))
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
