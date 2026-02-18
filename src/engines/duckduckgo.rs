//! DuckDuckGo search engine implementation.

use crate::html_engine::{selector, HtmlEngine, HtmlParser};
use crate::{EngineCategory, EngineConfig, Result, SearchQuery, SearchResult};
use scraper::Html;

/// DuckDuckGo HTML parser.
pub struct DuckDuckGoParser;

/// DuckDuckGo search engine.
pub type DuckDuckGo = HtmlEngine<DuckDuckGoParser>;

impl DuckDuckGo {
    /// Creates a new DuckDuckGo engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(
            DuckDuckGoParser,
            std::sync::Arc::new(crate::HttpFetcher::new()),
        )
    }
}

impl Default for DuckDuckGo {
    fn default() -> Self {
        DuckDuckGo::new()
    }
}

impl HtmlParser for DuckDuckGoParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "DuckDuckGo".to_string(),
            shortcut: "ddg".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 5,
            enabled: true,
            paging: true,
            safesearch: true,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        use crate::query::{SafeSearch, TimeRange};
        let mut url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(&query.query)
        );
        if query.page > 1 {
            url.push_str(&format!("&s={}", (query.page - 1) * 30));
        }
        match query.safesearch {
            SafeSearch::Off => {}
            SafeSearch::Moderate => url.push_str("&kp=-1"),
            SafeSearch::Strict => url.push_str("&kp=1"),
        }
        if let Some(range) = query.time_range {
            let df = match range {
                TimeRange::Day => "d",
                TimeRange::Week => "w",
                TimeRange::Month => "m",
                TimeRange::Year => "y",
            };
            url.push_str(&format!("&df={}", df));
        }
        url
    }

    fn parse(&self, html: &str) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);
        let result_sel = selector(".result")?;
        let title_sel = selector(".result__title a")?;
        let snippet_sel = selector(".result__snippet")?;

        let mut results = Vec::new();

        for element in document.select(&result_sel) {
            let title_elem = match element.select(&title_sel).next() {
                Some(el) => el,
                None => continue,
            };

            let title = title_elem.text().collect::<String>().trim().to_string();
            let url = title_elem.value().attr("href").unwrap_or_default();

            let url = if url.starts_with("//duckduckgo.com/l/") {
                extract_redirect_url(url).unwrap_or_else(|| url.to_string())
            } else {
                url.to_string()
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

fn extract_redirect_url(url: &str) -> Option<String> {
    let url = url.trim_start_matches("//duckduckgo.com/l/?uddg=");
    let decoded = urlencoding::decode(url).ok()?;
    let end = decoded.find('&').unwrap_or(decoded.len());
    Some(decoded[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use crate::HttpFetcher;
    use std::sync::Arc;

    #[test]
    fn test_duckduckgo_new() {
        let engine = DuckDuckGo::new();
        assert_eq!(engine.config().name, "DuckDuckGo");
        assert_eq!(engine.config().shortcut, "ddg");
        assert_eq!(engine.config().categories, vec![EngineCategory::General]);
        assert_eq!(engine.config().weight, 1.0);
        assert_eq!(engine.config().timeout, 5);
        assert!(engine.config().enabled);
        assert!(engine.config().paging);
        assert!(engine.config().safesearch);
    }

    #[test]
    fn test_duckduckgo_with_fetcher() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = DuckDuckGo::with_fetcher(DuckDuckGoParser, fetcher);
        assert_eq!(engine.config().name, "DuckDuckGo");
    }

    #[test]
    fn test_duckduckgo_default() {
        let engine = DuckDuckGo::default();
        assert_eq!(engine.config().name, "DuckDuckGo");
    }

    #[test]
    fn test_duckduckgo_with_config() {
        let custom_config = EngineConfig {
            name: "Custom DDG".to_string(),
            shortcut: "cddg".to_string(),
            weight: 2.0,
            ..Default::default()
        };
        let engine = DuckDuckGo::new().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom DDG");
        assert_eq!(engine.config().shortcut, "cddg");
        assert_eq!(engine.config().weight, 2.0);
    }

    #[test]
    fn test_duckduckgo_engine_trait() {
        let engine = DuckDuckGo::new();
        assert_eq!(engine.name(), "DuckDuckGo");
        assert_eq!(engine.shortcut(), "ddg");
        assert_eq!(engine.weight(), 1.0);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_extract_redirect_url() {
        let url = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc";
        let result = extract_redirect_url(url);
        assert_eq!(result, Some("https://example.com/page".to_string()));
    }

    #[test]
    fn test_extract_redirect_url_no_params() {
        let url = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com";
        let result = extract_redirect_url(url);
        assert_eq!(result, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_parse_results_empty_html() {
        let parser = DuckDuckGoParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_with_results() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <h2 class="result__title"><a href="https://example.com">Example Title</a></h2>
                    <div class="result__snippet">Example snippet text</div>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].content, "Example snippet text");
    }

    #[test]
    fn test_parse_results_with_redirect_url() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <h2 class="result__title"><a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc">Redirected Result</a></h2>
                    <div class="result__snippet">Snippet for redirected result</div>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/page");
        assert_eq!(results[0].title, "Redirected Result");
        assert_eq!(results[0].content, "Snippet for redirected result");
    }

    #[test]
    fn test_parse_results_multiple() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <h2 class="result__title"><a href="https://first.com">First</a></h2>
                    <div class="result__snippet">First snippet</div>
                </div>
                <div class="result">
                    <h2 class="result__title"><a href="https://second.com">Second</a></h2>
                    <div class="result__snippet">Second snippet</div>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://first.com");
        assert_eq!(results[1].url, "https://second.com");
    }

    #[test]
    fn test_parse_results_no_snippet() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <h2 class="result__title"><a href="https://example.com">No Snippet</a></h2>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "");
    }

    #[test]
    fn test_parse_results_skips_empty_title() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <h2 class="result__title"><a href="https://example.com"></a></h2>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_skips_empty_url() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <h2 class="result__title"><a href="">Has Title</a></h2>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_no_title_element() {
        let parser = DuckDuckGoParser;
        let html = r#"
            <html>
            <body>
                <div class="result">
                    <div class="result__snippet">Orphan snippet</div>
                </div>
            </body>
            </html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_redirect_url_invalid_encoding() {
        let url = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com";
        let result = extract_redirect_url(url);
        assert!(result.is_some());
    }
}
