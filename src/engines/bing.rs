//! Bing International search engine implementation.
//!
//! Bing's RSS endpoint is used instead of the bot-sensitive HTML results page.

use crate::html_engine::{
    selector, validate_search_response, HtmlEngine, HtmlParser, SearchResponseSpec,
};
use crate::{EngineCategory, EngineConfig, Result, SearchError, SearchQuery, SearchResult};
use base64::Engine as _;
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

/// Bing browser-rendered HTML response parser.
pub struct BingBrowserParser;

/// Bing International search engine.
pub type Bing = HtmlEngine<BingParser>;

/// Bing International search through a browser renderer.
pub type BingBrowser = HtmlEngine<BingBrowserParser>;

impl Bing {
    /// Creates a new Bing engine with a default HTTP fetcher.
    pub fn new() -> Self {
        HtmlEngine::with_fetcher(BingParser, std::sync::Arc::new(crate::HttpFetcher::new()))
    }
}

impl BingBrowser {
    /// Creates a browser-rendered Bing engine with the given page fetcher.
    pub fn new(fetcher: std::sync::Arc<dyn crate::PageFetcher>) -> Self {
        HtmlEngine::with_fetcher(BingBrowserParser, fetcher)
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

    fn validate(&self, response: &str) -> Result<()> {
        validate_bing_response(response)
    }

    fn parse(&self, response: &str) -> Result<Vec<SearchResult>> {
        parse_bing_response(response)
    }
}

impl HtmlParser for BingBrowserParser {
    fn default_config() -> EngineConfig {
        EngineConfig {
            name: "Bing".to_string(),
            shortcut: "bing_browser".to_string(),
            ..BingParser::default_config()
        }
    }

    fn build_url(&self, query: &SearchQuery) -> String {
        build_bing_html_url(query)
    }

    fn validate(&self, response: &str) -> Result<()> {
        validate_bing_html_response(response)
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

fn build_bing_html_url(query: &SearchQuery) -> String {
    let mut url = format!(
        "https://www.bing.com/search?q={}",
        urlencoding::encode(&query.query)
    );
    if query.page > 1 {
        let first = (query.page - 1) * 10 + 1;
        url.push_str(&format!("&first={first}"));
    }
    if let Some(language) = query.language.as_deref().map(str::trim) {
        if !language.is_empty() {
            url.push_str("&setlang=");
            url.push_str(&urlencoding::encode(language));
        }
    }
    if let Some(range) = query.time_range {
        use crate::query::TimeRange;
        let filter = match range {
            TimeRange::Day => "ex1:\"ez1\"",
            TimeRange::Week => "ex1:\"ez2\"",
            TimeRange::Month => "ex1:\"ez3\"",
            TimeRange::Year => "ex1:\"ez5\"",
        };
        url.push_str("&filters=");
        url.push_str(&urlencoding::encode(filter));
    }
    url
}

pub(crate) fn parse_bing_response(response: &str) -> Result<Vec<SearchResult>> {
    if is_bing_rss(response) {
        return parse_bing_rss(response);
    }

    let lowercase = response.to_ascii_lowercase();
    if lowercase.contains("b_captcha") || lowercase.contains("captcha") {
        return Err(SearchError::Challenge(
            "Bing returned a CAPTCHA or challenge instead of results".to_string(),
        ));
    }
    if lowercase.contains("id=\"b_header\"") && !lowercase.contains("class=\"b_algo") {
        return Err(SearchError::InvalidResponse(
            "Bing returned its home page instead of search results".to_string(),
        ));
    }

    parse_bing_html(response)
}

fn is_bing_rss(response: &str) -> bool {
    response.trim_start().starts_with("<?xml") || response.contains("<rss")
}

pub(crate) fn validate_bing_response(response: &str) -> Result<()> {
    if is_bing_rss(response) {
        Ok(())
    } else {
        validate_bing_html_response(response)
    }
}

fn validate_bing_html_response(html: &str) -> Result<()> {
    validate_search_response(
        html,
        SearchResponseSpec {
            engine: "Bing",
            result_selectors: &["li.b_algo"],
            empty_selectors: &["#b_results:empty", "#b_results .b_no"],
            challenge_selectors: &[
                "#b_captcha",
                ".b_captcha",
                "form[action*=\"captcha\"]",
                "iframe[src*=\"captcha\"]",
            ],
        },
    )
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
            .map(resolve_bing_result_url)
            .unwrap_or_default();

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

fn resolve_bing_result_url(value: &str) -> String {
    let Ok(url) = url::Url::parse(value) else {
        return value.to_string();
    };
    let is_bing_redirect = url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("bing.com") || host.ends_with(".bing.com"))
        && url.path().starts_with("/ck/");
    if !is_bing_redirect {
        return value.to_string();
    }

    let Some(target) = url
        .query_pairs()
        .find_map(|(key, value)| (key == "u").then_some(value.into_owned()))
    else {
        return value.to_string();
    };
    let decoded = if let Some(encoded) = target.strip_prefix("a1") {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    } else {
        Some(target)
    };
    decoded
        .filter(|target| {
            url::Url::parse(target).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none()
            })
        })
        .unwrap_or_else(|| value.to_string())
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
    fn browser_variant_shares_source_identity_and_has_an_independent_shortcut() {
        let fetcher: Arc<dyn crate::PageFetcher> = Arc::new(HttpFetcher::new());
        let http = Bing::new();
        let browser = BingBrowser::new(fetcher);

        assert_eq!(browser.name(), http.name());
        assert_eq!(browser.shortcut(), "bing_browser");
        assert_ne!(browser.shortcut(), http.shortcut());
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
    fn browser_url_uses_html_and_propagates_the_requested_locale() {
        let parser = BingBrowserParser;
        let query = SearchQuery::new("portable evidence")
            .with_page(2)
            .with_language("fr-FR")
            .with_time_range(TimeRange::Month);
        let url = parser.build_url(&query);

        assert!(url.starts_with("https://www.bing.com/search?q=portable%20evidence"));
        assert!(!url.contains("format=rss"));
        assert!(url.contains("first=11"));
        assert!(url.contains("setlang=fr-FR"));
        assert!(url.contains("filters="));
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
    fn html_parser_decodes_bing_click_redirects_to_canonical_hosts() {
        let parser = BingBrowserParser;
        let html = r#"
        <html><body>
        <ol id="b_results">
            <li class="b_algo">
                <h2><a href="https://www.bing.com/ck/a?x=1&amp;u=a1aHR0cHM6Ly9kb2NzLnJ1c3QtbGFuZy5vcmcvcmVmZXJlbmNlLw&amp;ntb=1">Rust Reference</a></h2>
                <p>Official Rust language reference.</p>
            </li>
        </ol>
        </body></html>
        "#;

        let results = parser.parse(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://docs.rust-lang.org/reference/");
    }

    #[test]
    fn browser_parser_classifies_result_empty_challenge_and_drift_pages() {
        let parser = BingBrowserParser;
        let result = r#"<ol id="b_results"><li class="b_algo"></li></ol>"#;
        let empty = r#"<ol id="b_results"></ol>"#;
        let challenge = r#"<main id="b_captcha"><form action="/captcha"></form></main>"#;
        let home = r#"<html><body><header id="b_header"></header></body></html>"#;

        assert!(parser.validate(result).is_ok());
        assert!(parser.validate(empty).is_ok());
        assert_eq!(parser.validate(challenge).unwrap_err().kind(), "challenge");
        assert_eq!(
            parser.validate(home).unwrap_err().kind(),
            "invalid_response"
        );
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
