use std::sync::Arc;
use std::time::Duration;

use pyo3::prelude::*;

use a3s_search::engines::{
    Bing, BingParser, Brave, BraveParser, DuckDuckGo, DuckDuckGoParser, So360, So360Parser, Sogou,
    SogouParser, Wikipedia,
};
use a3s_search::proxy::{ProxyConfig, ProxyPool};
use a3s_search::{HttpFetcher, PageFetcher, PooledHttpFetcher, Search, SearchQuery};

use crate::types::{PyEngineError, PySearchOptions, PySearchResponse, PySearchResult};
use crate::util::to_py_error;

/// Native search engine binding.
///
/// Wraps the a3s-search Rust library, providing direct access to
/// DuckDuckGo, Brave, Bing, Wikipedia, Sogou, and 360 search engines.
///
/// Supports dynamic proxy pool management for IP rotation.
#[pyclass]
pub struct PySearch {
    proxy_pool: Arc<ProxyPool>,
}

#[pymethods]
impl PySearch {
    #[new]
    fn new() -> Self {
        Self {
            proxy_pool: Arc::new(ProxyPool::new()),
        }
    }

    /// Sets the proxy pool URLs for IP rotation.
    ///
    /// Replaces any existing proxies. Each URL should be in the format
    /// "http://host:port", "https://host:port", or "socks5://host:port".
    /// Automatically enables the proxy pool.
    fn set_proxy_pool<'py>(&self, py: Python<'py>, urls: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
        let pool = Arc::clone(&self.proxy_pool);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let proxies: Vec<ProxyConfig> = urls
                .iter()
                .filter_map(|url| parse_proxy_url(url))
                .collect();

            if proxies.is_empty() && !urls.is_empty() {
                return Err(to_py_error("No valid proxy URLs provided"));
            }

            // Clear existing proxies
            let current_len = pool.len().await;
            for _ in 0..current_len {
                if let Some(p) = pool.get_proxy().await {
                    pool.remove_proxy(&p.host, p.port).await;
                }
            }

            for proxy in proxies {
                pool.add_proxy(proxy).await;
            }

            pool.set_enabled(!urls.is_empty());
            Ok(())
        })
    }

    /// Enables or disables the proxy pool.
    ///
    /// When disabled, requests are made directly without a proxy.
    fn set_proxy_pool_enabled(&self, enabled: bool) {
        self.proxy_pool.set_enabled(enabled);
    }

    /// Returns whether the proxy pool is currently enabled.
    fn is_proxy_pool_enabled(&self) -> bool {
        self.proxy_pool.is_enabled()
    }

    /// Returns the number of proxies in the pool.
    fn proxy_pool_size<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let pool = Arc::clone(&self.proxy_pool);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(pool.len().await as u32)
        })
    }

    /// Perform a search query across configured engines.
    ///
    /// Returns a coroutine that resolves to a PySearchResponse.
    #[pyo3(signature = (query, options=None))]
    fn search<'py>(
        &self,
        py: Python<'py>,
        query: String,
        options: Option<PySearchOptions>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let proxy_pool = Arc::clone(&self.proxy_pool);
        let proxy_pool_enabled = self.proxy_pool.is_enabled();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let opts = options.unwrap_or(PySearchOptions {
                engines: None,
                limit: None,
                timeout: None,
                proxy: None,
                proxy_pool: None,
            });

            let engine_shortcuts = opts
                .engines
                .unwrap_or_else(|| vec!["ddg".to_string(), "wiki".to_string()]);
            let timeout_secs = opts.timeout.unwrap_or(10) as u64;
            let limit = opts.limit;

            let mut search = Search::new();
            search.set_timeout(Duration::from_secs(timeout_secs));

            // Build fetcher priority: per-request proxy_pool > instance proxy_pool > per-request proxy > direct
            let http_fetcher: Arc<dyn PageFetcher> = if let Some(ref pool_urls) = opts.proxy_pool {
                let proxies: Vec<ProxyConfig> = pool_urls
                    .iter()
                    .filter_map(|url| parse_proxy_url(url))
                    .collect();
                if proxies.is_empty() {
                    return Err(to_py_error("proxy_pool contains no valid proxy URLs"));
                }
                Arc::new(PooledHttpFetcher::new(Arc::new(ProxyPool::with_proxies(proxies))))
            } else if proxy_pool_enabled {
                Arc::new(PooledHttpFetcher::new(proxy_pool))
            } else if let Some(ref proxy) = opts.proxy {
                Arc::new(HttpFetcher::with_proxy(proxy).map_err(to_py_error)?)
            } else {
                Arc::new(HttpFetcher::new())
            };

            for shortcut in &engine_shortcuts {
                match shortcut.as_str() {
                    "ddg" | "duckduckgo" => {
                        search.add_engine(DuckDuckGo::with_fetcher(
                            DuckDuckGoParser,
                            Arc::clone(&http_fetcher),
                        ));
                    }
                    "brave" => {
                        search.add_engine(Brave::with_fetcher(
                            BraveParser,
                            Arc::clone(&http_fetcher),
                        ));
                    }
                    "bing" => {
                        search.add_engine(Bing::with_fetcher(
                            BingParser,
                            Arc::clone(&http_fetcher),
                        ));
                    }
                    "wiki" | "wikipedia" => {
                        let fetcher = if let Some(ref proxy) = opts.proxy {
                            HttpFetcher::with_proxy(proxy).map_err(to_py_error)?
                        } else {
                            HttpFetcher::new()
                        };
                        search.add_engine(Wikipedia::with_http_fetcher(fetcher));
                    }
                    "sogou" => {
                        search.add_engine(Sogou::with_fetcher(
                            SogouParser,
                            Arc::clone(&http_fetcher),
                        ));
                    }
                    "360" | "so360" => {
                        search.add_engine(So360::with_fetcher(
                            So360Parser,
                            Arc::clone(&http_fetcher),
                        ));
                    }
                    unknown => {
                        return Err(to_py_error(format!(
                            "Unknown engine '{}'. Available: ddg, brave, bing, wiki, sogou, 360",
                            unknown
                        )));
                    }
                }
            }

            if search.engine_count() == 0 {
                return Err(to_py_error("No valid engines specified"));
            }

            let search_query = SearchQuery::new(&query);
            let results = search.search(search_query).await.map_err(to_py_error)?;

            let mut py_results: Vec<PySearchResult> = results
                .items()
                .iter()
                .map(|r| PySearchResult {
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
                py_results.truncate(max as usize);
            }

            let errors: Vec<PyEngineError> = results
                .errors()
                .iter()
                .map(|(engine, message)| PyEngineError {
                    engine: engine.clone(),
                    message: message.clone(),
                })
                .collect();

            Ok(PySearchResponse {
                count: py_results.len() as u32,
                results: py_results,
                duration_ms: results.duration_ms as u32,
                errors,
            })
        })
    }
}

/// Parses a proxy URL string into a ProxyConfig.
fn parse_proxy_url(url: &str) -> Option<ProxyConfig> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    let parsed = url::Url::parse(url).ok()?;
    let protocol = match parsed.scheme() {
        "http" => a3s_search::proxy::ProxyProtocol::Http,
        "https" => a3s_search::proxy::ProxyProtocol::Https,
        "socks5" => a3s_search::proxy::ProxyProtocol::Socks5,
        _ => return None,
    };

    let host = parsed.host_str()?;
    let port = parsed.port().unwrap_or(match protocol {
        a3s_search::proxy::ProxyProtocol::Http => 8080,
        a3s_search::proxy::ProxyProtocol::Https => 443,
        a3s_search::proxy::ProxyProtocol::Socks5 => 1080,
    });

    let mut config = ProxyConfig::new(host, port).with_protocol(protocol);
    if let Some(password) = parsed.password() {
        config = config.with_auth(parsed.username(), password);
    }
    Some(config)
}
