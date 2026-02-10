use std::sync::Arc;
use std::time::Duration;

use napi::Result;
use napi_derive::napi;

use a3s_search::engines::{Brave, DuckDuckGo, So360, Sogou, Wikipedia};
use a3s_search::{HttpFetcher, Search, SearchQuery};

use crate::types::{JsEngineError, JsSearchOptions, JsSearchResponse, JsSearchResult};
use crate::util::to_napi_error;

/// Native search engine binding.
///
/// Wraps the a3s-search Rust library, providing direct access to
/// DuckDuckGo, Brave, Wikipedia, Sogou, and 360 search engines.
#[napi]
pub struct JsSearch {}

#[napi]
impl JsSearch {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {}
    }

    /// Perform a search query across configured engines.
    ///
    /// Returns a Promise that resolves to a JsSearchResponse.
    #[napi]
    pub async fn search(
        &self,
        query: String,
        options: Option<JsSearchOptions>,
    ) -> Result<JsSearchResponse> {
        let opts = options.unwrap_or(JsSearchOptions {
            engines: None,
            limit: None,
            timeout: None,
            proxy: None,
        });

        let engine_shortcuts = opts
            .engines
            .unwrap_or_else(|| vec!["ddg".to_string(), "wiki".to_string()]);
        let timeout_secs = opts.timeout.unwrap_or(10) as u64;
        let limit = opts.limit;

        let mut search = Search::new();
        search.set_timeout(Duration::from_secs(timeout_secs));

        let http_fetcher: Arc<dyn a3s_search::PageFetcher> = if let Some(ref proxy) = opts.proxy {
            Arc::new(HttpFetcher::with_proxy(proxy).map_err(to_napi_error)?)
        } else {
            Arc::new(HttpFetcher::new())
        };

        for shortcut in &engine_shortcuts {
            match shortcut.as_str() {
                "ddg" | "duckduckgo" => {
                    search.add_engine(DuckDuckGo::with_fetcher(Arc::clone(&http_fetcher)));
                }
                "brave" => {
                    search.add_engine(Brave::with_fetcher(Arc::clone(&http_fetcher)));
                }
                "wiki" | "wikipedia" => {
                    let fetcher = if let Some(ref proxy) = opts.proxy {
                        HttpFetcher::with_proxy(proxy).map_err(to_napi_error)?
                    } else {
                        HttpFetcher::new()
                    };
                    search.add_engine(Wikipedia::with_http_fetcher(fetcher));
                }
                "sogou" => {
                    search.add_engine(Sogou::with_fetcher(Arc::clone(&http_fetcher)));
                }
                "360" | "so360" => {
                    search.add_engine(So360::with_fetcher(Arc::clone(&http_fetcher)));
                }
                unknown => {
                    return Err(to_napi_error(format!(
                        "Unknown engine '{}'. Available: ddg, brave, wiki, sogou, 360",
                        unknown
                    )));
                }
            }
        }

        if search.engine_count() == 0 {
            return Err(to_napi_error("No valid engines specified"));
        }

        let search_query = SearchQuery::new(&query);
        let results = search.search(search_query).await.map_err(to_napi_error)?;

        let mut js_results: Vec<JsSearchResult> = results
            .items()
            .iter()
            .map(|r| JsSearchResult {
                url: r.url.clone(),
                title: r.title.clone(),
                content: r.content.clone(),
                result_type: format!("{:?}", r.result_type).to_lowercase(),
                engines: r.engines.iter().cloned().collect(),
                score: r.score,
                thumbnail: r.thumbnail.clone(),
                published_date: r.published_date.clone(),
            })
            .collect();

        if let Some(max) = limit {
            js_results.truncate(max as usize);
        }

        let errors: Vec<JsEngineError> = results
            .errors()
            .iter()
            .map(|(engine, message)| JsEngineError {
                engine: engine.clone(),
                message: message.clone(),
            })
            .collect();

        Ok(JsSearchResponse {
            count: js_results.len() as u32,
            results: js_results,
            duration_ms: results.duration_ms as u32,
            errors,
        })
    }
}
