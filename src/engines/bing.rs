//! Bing International search engine implementation.
//!
//! Bing's RSS endpoint is used instead of the bot-sensitive HTML results page.

use crate::html_engine::{selector, HtmlEngine, HtmlParser};
use crate::{EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};
use scraper::Html;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct BingRss {
    channel: BingRssChannel,
}

#[derive(Debug, Deserialize)]
struct BingRssChannel {
    #[serde(default)]
    item: Vec<BingRssItem>,
}

#[derive(Debug, Deserialize)]
struct BingRssItem {
    #[serde(default)]
    title: String,
    #[serde(default)]
    link: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "pubDate", default)]
    published_date: String,
}

/// Bing RSS/HTML response parser.
pub struct BingParser;

/// Bing International search engine.
pub type Bing = HtmlEngine<BingParser>;

impl Bing {
    /// Creates a new Bing engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(BingParser, std::sync::Arc::new(crate::HttpFetcher::new()))
    }
}

impl Default for Bing {
    fn default() -> Self {
        Bing::new()
    }
}

impl HtmlParser for BingParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Bing".to_string(),
            shortcut: "bing".to_string(),
            categories: vec![EngineCategory::General],
            weight: 1.0,
            timeout: 5,
            enabled: true,
            paging: true,
            safesearch: false,
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        build_bing_rss_url(query)
    }

    fn parse(&self, response: &str) -> Result<Vec<SearchResult>> {
        parse_bing_response(response)
    }
}

pub(crate) fn build_bing_rss_url(query: &SearchQuery) -> String {
    let mut url = format!(
        "https://www.bing.com/search?q={}&format=rss",
        urlencoding::encode(&query.query)
    );
    if query.page > 1 {
        let first = (query.page - 1) * 10 + 1;
        url.push_str(&format!("&first={first}"));
    }
    if let Some(range) = query.time_range {
        use crate::query::TimeRange;
        let filter = match range {
            TimeRange::Day => "ex1:\"ez1\"",
            TimeRange::Week => "ex1:\"ez2\"",
            TimeRange::Month => "ex1:\"ez3\"",
            TimeRange::Year => "ex1:\"ez5\"",
        };
        url.push_str(&format!("&filters={}", urlencoding::encode(filter)));
    }
    url
}

pub(crate) fn parse_bing_response(response: &str) -> Result<Vec<SearchResult>> {
    if response.trim_start().starts_with("<?xml") || response.contains("<rss") {
        return parse_bing_rss(response);
    }

    if is_bing_challenge_or_home_page(response) {
        return Err(SearchError::Parse(
            "Bing returned a challenge or search home page instead of results".to_string(),
        ));
    }

    parse_bing_html(response)
}

fn parse_bing_rss(xml: &str) -> Result<Vec<SearchResult>> {
    let feed: BingRss = quick_xml::de::from_str(xml)
        .map_err(|error| SearchError::Parse(format!("Failed to parse Bing RSS: {error}")))?;

    Ok(feed
        .channel
        .item
        .into_iter()
        .filter_map(|item| {
            let title = item.title.trim();
            let url = item.link.trim();
            if title.is_empty() || !url.starts_with("http") {
                return None;
            }
            let result = SearchResult::new(url, title, item.description.trim());
            Some(if item.published_date.trim().is_empty() {
                result
            } else {
                result.with_published_date(item.published_date.trim())
            })
        })
        .collect())
}

fn parse_bing_html(html: &str) -> Result<Vec<SearchResult>> {
    let document = Html::parse_document(html);
    let result_sel = selector("li.b_algo")?;
    let title_sel = selector("h2 a")?;
    let snippet_sel = selector("p, .b_caption p, .b_algoSlug")?;

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

        if !url.is_empty() && !title.is_empty() && url.starts_with("http") {
            results.push(SearchResult::new(url, title, content));
        }
    }

    Ok(results)
}

fn is_bing_challenge_or_home_page(html: &str) -> bool {
    let lowercase = html.to_ascii_lowercase();
    lowercase.contains("b_captcha")
        || lowercase.contains("captcha")
        || (lowercase.contains("id=\"b_header\"") && !lowercase.contains("class=\"b_algo"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{SafeSearch, TimeRange};
    use crate::Engine;
    use crate::HttpFetcher;
    use std::sync::Arc;

    #[test]
    fn test_bing_new() {
        let engine = Bing::new();
        assert_eq!(engine.config().name, "Bing");
        assert_eq!(engine.config().shortcut, "bing");
        assert_eq!(engine.config().categories, vec![EngineCategory::General]);
        assert_eq!(engine.config().weight, 1.0);
        assert_eq!(engine.config().timeout, 5);
        assert!(engine.config().enabled);
        assert!(engine.config().paging);
    }

    #[test]
    fn test_bing_with_fetcher() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let engine = Bing::with_fetcher(BingParser, fetcher);
        assert_eq!(engine.config().name, "Bing");
    }

    #[test]
    fn test_bing_default() {
        let engine = Bing::default();
        assert_eq!(engine.config().name, "Bing");
    }

    #[test]
    fn test_bing_engine_trait() {
        let engine = Bing::new();
        assert_eq!(engine.name(), "Bing");
        assert_eq!(engine.shortcut(), "bing");
        assert_eq!(engine.weight(), 1.0);
        assert!(engine.is_enabled());
    }

    #[test]
    fn test_bing_build_url_basic() {
        let parser = BingParser;
        let query = SearchQuery::new("rust programming");
        let url = parser.build_url(&query);
        assert!(url.starts_with("https://www.bing.com/search?q=rust%20programming"));
        assert!(url.contains("format=rss"));
    }

    #[test]
    fn test_bing_build_url_page_2() {
        let parser = BingParser;
        let query = SearchQuery::new("test").with_page(2);
        let url = parser.build_url(&query);
        assert!(url.contains("&first=11"));
    }

    #[test]
    fn test_bing_build_url_page_3() {
        let parser = BingParser;
        let query = SearchQuery::new("test").with_page(3);
        let url = parser.build_url(&query);
        assert!(url.contains("&first=21"));
    }

    #[test]
    fn test_bing_build_url_page_1_no_first() {
        let parser = BingParser;
        let query = SearchQuery::new("test").with_page(1);
        let url = parser.build_url(&query);
        assert!(!url.contains("&first="));
    }

    #[test]
    fn test_bing_build_url_time_range_day() {
        let parser = BingParser;
        let query = SearchQuery::new("test").with_time_range(TimeRange::Day);
        let url = parser.build_url(&query);
        assert!(url.contains("&filters="));
    }

    #[test]
    fn test_bing_build_url_time_range_week() {
        let parser = BingParser;
        let query = SearchQuery::new("test").with_time_range(TimeRange::Week);
        let url = parser.build_url(&query);
        assert!(url.contains("&filters="));
    }

    #[test]
    fn test_bing_build_url_no_safesearch_param() {
        let parser = BingParser;
        let query = SearchQuery::new("test").with_safesearch(SafeSearch::Strict);
        let url = parser.build_url(&query);
        // Bing doesn't use URL-based safe search
        assert!(!url.contains("safesearch"));
    }

    #[test]
    fn test_bing_parse_empty_html() {
        let parser = BingParser;
        let results = parser.parse("<html><body></body></html>").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_bing_parse_results() {
        let parser = BingParser;
        let html = r#"
        <html><body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="https://www.rust-lang.org/">Rust Programming Language</a></h2>
                <p>A systems programming language focused on safety and performance.</p>
            </li>
            <li class="b_algo">
                <h2><a href="https://doc.rust-lang.org/book/">The Rust Book</a></h2>
                <div class="b_caption"><p>Official Rust programming guide.</p></div>
            </li>
        </ol>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            results[0].content,
            "A systems programming language focused on safety and performance."
        );
        assert_eq!(results[1].title, "The Rust Book");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/book/");
    }

    #[test]
    fn test_bing_parse_skips_no_title() {
        let parser = BingParser;
        let html = r#"
        <html><body>
        <ol id="b_results">
            <li class="b_algo">
                <p>Orphan snippet without title</p>
            </li>
        </ol>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_bing_parse_skips_non_http_urls() {
        let parser = BingParser;
        let html = r#"
        <html><body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="javascript:void(0)">Bad Link</a></h2>
                <p>Content</p>
            </li>
        </ol>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_bing_parse_multiple_results() {
        let parser = BingParser;
        let html = r#"
        <html><body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="https://first.com">First</a></h2>
                <p>First snippet</p>
            </li>
            <li class="b_algo">
                <h2><a href="https://second.com">Second</a></h2>
                <p>Second snippet</p>
            </li>
            <li class="b_algo">
                <h2><a href="https://third.com">Third</a></h2>
                <p>Third snippet</p>
            </li>
        </ol>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].url, "https://first.com");
        assert_eq!(results[1].url, "https://second.com");
        assert_eq!(results[2].url, "https://third.com");
    }

    #[test]
    fn test_bing_parse_no_snippet() {
        let parser = BingParser;
        let html = r#"
        <html><body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="https://example.com">No Snippet</a></h2>
            </li>
        </ol>
        </body></html>
        "#;
        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "");
    }

    #[test]
    fn test_bing_with_config() {
        let custom_config = EngineConfig {
            name: "Custom Bing".to_string(),
            weight: 1.5,
            ..Default::default()
        };
        let engine = Bing::new().with_config(custom_config);
        assert_eq!(engine.config().name, "Custom Bing");
        assert_eq!(engine.config().weight, 1.5);
    }
}
