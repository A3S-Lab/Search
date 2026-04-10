use std::sync::Arc;
use std::time::Duration;

use napi::Result;
use napi_derive::napi;

use a3s_search::engines::{
    Bing, BingParser, Brave, BraveParser, DuckDuckGo, DuckDuckGoParser, So360, So360Parser, Sogou,
    SogouParser, Wikipedia,
};
use a3s_search::proxy::{ProxyConfig, ProxyPool};
use a3s_search::{EngineCategory, HealthConfig, HttpFetcher, PageFetcher, PooledHttpFetcher, SafeSearch, Search, SearchQuery, TimeRange};

#[cfg(feature = "chromium")]
use a3s_search::engines::{Baidu, BaiduParser, BingChina, BingChinaParser, Google, GoogleParser};
#[cfg(feature = "chromium")]
use a3s_search::{BrowserBackend, BrowserFetcher, BrowserPool, BrowserPoolConfig};

use crate::types::{JsEngineError, JsSearchOptions, JsSearchResponse, JsSearchResult};
use crate::util::to_napi_error;

/// Native search engine binding.
///
/// Wraps the a3s-search Rust library, providing direct access to
/// DuckDuckGo, Brave, Bing, Wikipedia, Sogou, and 360 search engines.
///
/// Supports dynamic proxy pool management for IP rotation.
#[napi]
pub struct JsSearch {
    proxy_pool: Arc<ProxyPool>,
}

#[napi]
impl JsSearch {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            proxy_pool: Arc::new(ProxyPool::new()),
        }
    }

    /// Sets the proxy pool URLs for IP rotation.
    ///
    /// Replaces any existing proxies. Each URL should be in the format
    /// "http://host:port", "https://host:port", or "socks5://host:port".
    /// Automatically enables the proxy pool.
    #[napi]
    pub async fn set_proxy_pool(&self, urls: Vec<String>) -> Result<()> {
        // Clear existing proxies by writing directly
        let proxies: Vec<ProxyConfig> = urls
            .iter()
            .filter_map(|url| parse_proxy_url(url))
            .collect();

        if proxies.is_empty() && !urls.is_empty() {
            return Err(to_napi_error("No valid proxy URLs provided"));
        }

        // Remove all existing proxies and add new ones
        // We need to work through the pool's async API
        let current_len = self.proxy_pool.len().await;
        // Get current proxies to remove them
        for _ in 0..current_len {
            if let Some(p) = self.proxy_pool.get_proxy().await {
                self.proxy_pool.remove_proxy(&p.host, p.port).await;
            }
        }

        for proxy in proxies {
            self.proxy_pool.add_proxy(proxy).await;
        }

        self.proxy_pool.set_enabled(!urls.is_empty());
        Ok(())
    }

    /// Enables or disables the proxy pool.
    ///
    /// When disabled, requests are made directly without a proxy.
    #[napi]
    pub fn set_proxy_pool_enabled(&self, enabled: bool) {
        self.proxy_pool.set_enabled(enabled);
    }

    /// Returns whether the proxy pool is currently enabled.
    #[napi]
    pub fn is_proxy_pool_enabled(&self) -> bool {
        self.proxy_pool.is_enabled()
    }

    /// Returns the number of proxies in the pool.
    #[napi]
    pub async fn proxy_pool_size(&self) -> u32 {
        self.proxy_pool.len().await as u32
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
            proxy_pool: None,
            language: None,
            safesearch: None,
            page: None,
            time_range: None,
            category: None,
            engine_weights: None,
            health_max_failures: None,
            health_suspend_secs: None,
            browser: None,
            chrome_path: None,
            lightpanda_path: None,
            max_tabs: None,
        });

        let engine_shortcuts = opts
            .engines
            .unwrap_or_else(|| vec!["ddg".to_string(), "wiki".to_string()]);
        let timeout_secs = opts.timeout.unwrap_or(10) as u64;
        let limit = opts.limit;

        // Create Search with optional health config
        let mut search = if opts.health_max_failures.is_some() || opts.health_suspend_secs.is_some() {
            let health_config = HealthConfig {
                max_failures: opts.health_max_failures.unwrap_or(3),
                suspend_duration: Duration::from_secs(opts.health_suspend_secs.unwrap_or(300) as u64),
            };
            Search::with_health_config(health_config)
        } else {
            Search::new()
        };

        search.set_timeout(Duration::from_secs(timeout_secs));

        // Build fetcher priority: per-request proxy_pool > instance proxy_pool > per-request proxy > direct
        let http_fetcher: Arc<dyn PageFetcher> = if let Some(ref pool_urls) = opts.proxy_pool {
            let proxies: Vec<ProxyConfig> = pool_urls
                .iter()
                .filter_map(|url| parse_proxy_url(url))
                .collect();
            if proxies.is_empty() {
                return Err(to_napi_error("proxy_pool contains no valid proxy URLs"));
            }
            Arc::new(PooledHttpFetcher::new(Arc::new(ProxyPool::with_proxies(proxies))))
        } else if self.proxy_pool.is_enabled() {
            Arc::new(PooledHttpFetcher::new(Arc::clone(&self.proxy_pool)))
        } else if let Some(ref proxy) = opts.proxy {
            Arc::new(HttpFetcher::with_proxy(proxy).map_err(to_napi_error)?)
        } else {
            Arc::new(HttpFetcher::new())
        };

        // Check if any headless engines are requested
        #[cfg(feature = "chromium")]
        let needs_browser = engine_shortcuts.iter().any(|s| {
            matches!(s.as_str(), "google" | "g" | "baidu" | "bingchina")
        });

        // Create browser pool if needed
        #[cfg(feature = "chromium")]
        let browser_fetcher: Option<Arc<dyn PageFetcher>> = if needs_browser {
            let mut config = BrowserPoolConfig::default();

            // Set browser backend (default to Lightpanda)
            let backend = opts.browser.as_deref().unwrap_or("lightpanda");
            match backend {
                "chrome" => {
                    config.backend = BrowserBackend::Chrome;
                    if let Some(ref path) = opts.chrome_path {
                        config.chrome_path = Some(path.clone());
                    }
                }
                "lightpanda" => {
                    #[cfg(feature = "lightpanda")]
                    {
                        config.backend = BrowserBackend::Lightpanda;
                        if let Some(ref path) = opts.lightpanda_path {
                            config.lightpanda_path = Some(path.clone());
                        }
                    }
                    #[cfg(not(feature = "lightpanda"))]
                    {
                        return Err(to_napi_error(
                            "Lightpanda backend requested but 'lightpanda' feature is not enabled. \
                             Rebuild with --features lightpanda or use browser='chrome'."
                        ));
                    }
                }
                _ => {
                    return Err(to_napi_error(format!(
                        "Invalid browser '{}'. Must be 'chrome' or 'lightpanda'.",
                        backend
                    )));
                }
            }

            if let Some(ref proxy) = opts.proxy {
                config.proxy_url = Some(proxy.clone());
            }
            if let Some(max_tabs) = opts.max_tabs {
                config.max_tabs = max_tabs as usize;
            }

            let pool = Arc::new(BrowserPool::new(config));
            Some(Arc::new(BrowserFetcher::new(pool)))
        } else {
            None
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
                        HttpFetcher::with_proxy(proxy).map_err(to_napi_error)?
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
                #[cfg(feature = "chromium")]
                "google" | "g" => {
                    if let Some(ref fetcher) = browser_fetcher {
                        search.add_engine(Google::new(Arc::clone(fetcher)));
                    } else {
                        return Err(to_napi_error("Browser fetcher not initialized for Google engine"));
                    }
                }
                #[cfg(feature = "chromium")]
                "baidu" => {
                    if let Some(ref fetcher) = browser_fetcher {
                        search.add_engine(Baidu::new(Arc::clone(fetcher)));
                    } else {
                        return Err(to_napi_error("Browser fetcher not initialized for Baidu engine"));
                    }
                }
                #[cfg(feature = "chromium")]
                "bingchina" => {
                    if let Some(ref fetcher) = browser_fetcher {
                        search.add_engine(BingChina::new(Arc::clone(fetcher)));
                    } else {
                        return Err(to_napi_error("Browser fetcher not initialized for BingChina engine"));
                    }
                }
                unknown => {
                    #[cfg(feature = "chromium")]
                    let available = "ddg, brave, bing, wiki, sogou, 360, google, baidu, bingchina";
                    #[cfg(not(feature = "chromium"))]
                    let available = "ddg, brave, bing, wiki, sogou, 360";

                    return Err(to_napi_error(format!(
                        "Unknown engine '{}'. Available: {}",
                        unknown, available
                    )));
                }
            }
        }

        if search.engine_count() == 0 {
            return Err(to_napi_error("No valid engines specified"));
        }

        let mut search_query = SearchQuery::new(&query);

        // Apply query filters
        if let Some(ref lang) = opts.language {
            search_query = search_query.with_language(lang);
        }
        if let Some(ref ss) = opts.safesearch {
            let safesearch = match ss.to_lowercase().as_str() {
                "off" => SafeSearch::Off,
                "moderate" => SafeSearch::Moderate,
                "strict" => SafeSearch::Strict,
                _ => return Err(to_napi_error(format!(
                    "Invalid safesearch value '{}'. Must be 'off', 'moderate', or 'strict'",
                    ss
                ))),
            };
            search_query = search_query.with_safesearch(safesearch);
        }
        if let Some(page) = opts.page {
            search_query = search_query.with_page(page);
        }
        if let Some(ref tr) = opts.time_range {
            let time_range = match tr.to_lowercase().as_str() {
                "day" => TimeRange::Day,
                "week" => TimeRange::Week,
                "month" => TimeRange::Month,
                "year" => TimeRange::Year,
                _ => return Err(to_napi_error(format!(
                    "Invalid time_range value '{}'. Must be 'day', 'week', 'month', or 'year'",
                    tr
                ))),
            };
            search_query = search_query.with_time_range(time_range);
        }
        if let Some(ref cat) = opts.category {
            let category = match cat.to_lowercase().as_str() {
                "general" => EngineCategory::General,
                "images" => EngineCategory::Images,
                "videos" => EngineCategory::Videos,
                "news" => EngineCategory::News,
                "maps" => EngineCategory::Maps,
                "music" => EngineCategory::Music,
                "files" => EngineCategory::Files,
                "science" => EngineCategory::Science,
                "social" => EngineCategory::Social,
                _ => return Err(to_napi_error(format!(
                    "Invalid category value '{}'. Must be one of: general, images, videos, news, maps, music, files, science, social",
                    cat
                ))),
            };
            search_query = search_query.with_categories(vec![category]);
        }

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

        let suggestions = results.suggestions().to_vec();
        let answers = results.answers().to_vec();

        Ok(JsSearchResponse {
            count: js_results.len() as u32,
            results: js_results,
            duration_ms: results.duration_ms as u32,
            errors,
            suggestions,
            answers,
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
