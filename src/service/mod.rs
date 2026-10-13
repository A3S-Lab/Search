//! Native web search service.
//!
//! `a3s-search serve` answers `GET /health` and `POST /v1/search` from the
//! in-process HTTP engines (Wikipedia, DuckDuckGo, Brave, and Bing). The
//! unified gateway fronts that listener.

mod http;

/// Binds the search listener and serves `GET /health` and `POST /v1/search`.
/// A non-loopback address is refused unless `A3S_SEARCH_API_KEY` is set.
pub async fn serve_native(listen: &str) -> Result<()> {
    http::serve_native(listen).await
}

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::engines::{Bing, Brave, DuckDuckGo, Wikipedia};
use crate::{
    enrich_full_text, HttpFetcher, Result, Search, SearchError, SearchQuery, SearchResults,
};

const MAX_QUERY_CHARS: usize = 500;
const MAX_LIMIT: u8 = 100;
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// One accepted `POST /v1/search` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeSearchRequest {
    pub query: String,
    pub limit: u8,
    pub include_markdown: bool,
}

#[async_trait]
pub(crate) trait NativeWebSearch: Send + Sync {
    async fn search(&self, request: &NativeSearchRequest) -> Result<SearchResults>;
}

/// Searches with the built-in HTTP engines. No API key and no SearXNG.
pub(crate) struct EngineWebSearch {
    timeout: Duration,
}

impl EngineWebSearch {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    fn assemble() -> Search {
        let mut search = Search::new();
        search.add_engine(Wikipedia::new());
        search.add_engine(DuckDuckGo::new());
        search.add_engine(Brave::new());
        search.add_engine(Bing::new());
        search
    }
}

#[async_trait]
impl NativeWebSearch for EngineWebSearch {
    async fn search(&self, request: &NativeSearchRequest) -> Result<SearchResults> {
        let query = SearchQuery::new(request.query.clone());
        let mut results = tokio::time::timeout(self.timeout, Self::assemble().search(query))
            .await
            .map_err(|_| SearchError::Timeout)??;
        if request.include_markdown {
            enrich_full_text(
                &mut results,
                Arc::new(HttpFetcher::new()),
                2,
                Duration::from_secs(4),
            )
            .await;
        }
        Ok(results)
    }
}

#[derive(Debug)]
pub(crate) enum SearchReply {
    Json { status: u16, body: Value },
}

pub(crate) fn parse_search_body(
    bytes: &[u8],
) -> std::result::Result<NativeSearchRequest, SearchReply> {
    let document: Incoming = serde_json::from_slice(bytes).map_err(|_| {
        reply(
            400,
            "INVALID_REQUEST",
            "search body must be a JSON object with query",
        )
    })?;
    let query = document.query.trim();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(reply(
            400,
            "INVALID_REQUEST",
            "query must be 1 to 500 characters",
        ));
    }
    if let Some(sources) = &document.sources {
        if sources.is_empty() || sources.iter().any(|source| source != "web") {
            return Err(reply(400, "INVALID_REQUEST", "sources must be [\"web\"]"));
        }
    }
    let limit = document.limit.unwrap_or(10);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(reply(400, "INVALID_REQUEST", "limit must be 1 to 100"));
    }
    Ok(NativeSearchRequest {
        query: query.to_string(),
        limit,
        include_markdown: document
            .scrape_options
            .as_ref()
            .is_some_and(|options| options.requests_markdown()),
    })
}

pub(crate) fn reply_for_results(
    request: &NativeSearchRequest,
    results: &SearchResults,
) -> SearchReply {
    let web: Vec<Value> = results
        .items()
        .iter()
        .filter(|item| matches!(item.result_type, crate::ResultType::Web))
        .take(usize::from(request.limit))
        .map(|item| {
            let mut hit = json!({
                "url": item.url,
                "title": item.title,
                "description": item.content,
            });
            if request.include_markdown {
                if let Some(text) = item.full_text.as_deref().filter(|text| !text.is_empty()) {
                    hit["markdown"] = Value::String(text.to_string());
                }
            }
            hit
        })
        .collect();
    if web.is_empty() && !results.failures().is_empty() {
        return reply(503, "UNAVAILABLE", "search engines returned no results");
    }
    SearchReply::Json {
        status: 200,
        body: json!({
            "success": true,
            "data": { "results": { "web": web } }
        }),
    }
}

pub(crate) fn reply(status: u16, code: &str, message: &str) -> SearchReply {
    SearchReply::Json {
        status,
        body: json!({
            "success": false,
            "error": message,
            "code": code,
        }),
    }
}

#[derive(Deserialize)]
struct Incoming {
    query: String,
    limit: Option<u8>,
    sources: Option<Vec<String>>,
    #[serde(rename = "scrapeOptions")]
    scrape_options: Option<IncomingScrape>,
}

#[derive(Deserialize)]
struct IncomingScrape {
    formats: Option<Vec<IncomingFormat>>,
}

impl IncomingScrape {
    fn requests_markdown(&self) -> bool {
        self.formats
            .as_ref()
            .is_some_and(|formats| formats.iter().any(|format| format.kind == "markdown"))
    }
}

#[derive(Deserialize)]
struct IncomingFormat {
    #[serde(rename = "type")]
    kind: String,
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
