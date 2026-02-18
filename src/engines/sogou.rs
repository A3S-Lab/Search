//! Sogou search engine implementation.

use crate::html_engine::{selector, HtmlEngine, HtmlParser};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// Sogou HTML parser.
pub struct SogouParser;

/// Sogou search engine (搜狗).
pub type Sogou = HtmlEngine<SogouParser>;

impl Sogou {
    /// Creates a new Sogou engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(SogouParser, std::sync::Arc::new(crate::HttpFetcher::new()))
    }
}

impl Default for Sogou {
    fn default() -> Self {
        Sogou::new()
    }
}

impl HtmlParser for SogouParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Sogou".to_string(),
            shortcut: "sogou".to_string(),
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
            "https://www.sogou.com/web?query={}",
            urlencoding::encode(&query.query)
        );
        if query.page > 1 {
            url.push_str(&format!("&page={}", query.page));
        }
        url
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);
        let result_sel = selector("div.vrwrap, div.rb")?;
        let title_sel = selector("h3 a, .vr-title a")?;
        let snippet_sel = selector(".str-text, .str_info, .space-txt")?;

        let mut results = Vec::new();

        for element in document.select(&result_sel) {
            let title_elem = match element.select(&title_sel).next() {
                Some(el) => el,
                None => continue,
            };

            let title = title_elem.text().collect::<String>().trim().to_string();
            let raw_url = title_elem.value().attr("href").unwrap_or_default();

            // Sogou returns relative redirect URLs like /link?url=...
            let url = if raw_url.starts_with('/') {
                format!("https://www.sogou.com{}", raw_url)
            } else {
                raw_url.to_string()
            };

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
    fn test_sogou_new() {
        let engine = Sogou::new();
        assert_eq!(engine.config().name, "Sogou");
        assert_eq!(engine.config().shortcut, "sogou");
        assert_eq!(engine.config().weight, 1.0);
    }

    #[test]
    fn test_sogou_with_fetcher() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = Sogou::with_fetcher(SogouParser, fetcher);
        assert_eq!(engine.config().name, "Sogou");
    }

    #[test]
    fn test_sogou_default() {
        let engine = Sogou::default();
        assert_eq!(engine.config().name, "Sogou");
    }

    #[test]
    fn test_sogou_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Sogou".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = Sogou::new().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom Sogou");
    }

    #[test]
    fn test_sogou_engine_trait() {
        let engine = Sogou::new();
        assert_eq!(engine.name(), "Sogou");
        assert_eq!(engine.shortcut(), "sogou");
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_sogou_parse_results_empty() {
        let parser = SogouParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_sogou_parse_results_with_data() {
        let parser = SogouParser;
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="/link?url=abc123">Rust Programming</a></h3>
            <div class="str-text">A systems programming language.</div>
        </div>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://example.com/page">Example Page</a></h3>
            <div class="str_info">Some description here.</div>
        </div>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming");
        assert_eq!(results[0].url, "https://www.sogou.com/link?url=abc123");
        assert_eq!(results[0].content, "A systems programming language.");
        assert_eq!(results[1].title, "Example Page");
        assert_eq!(results[1].url, "https://example.com/page");
    }

    #[test]
    fn test_sogou_parse_results_relative_url() {
        let parser = SogouParser;
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3><a href="/link?url=xyz789">Test Result</a></h3>
            <div class="space-txt">Test snippet.</div>
        </div>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.sogou.com/link?url=xyz789");
    }
}
