use pyo3::prelude::*;

/// A single search result returned by an engine.
#[pyclass]
#[derive(Clone, Debug)]
pub struct PySearchResult {
    /// Result URL.
    #[pyo3(get)]
    pub url: String,
    /// Result title.
    #[pyo3(get)]
    pub title: String,
    /// Result description/snippet.
    #[pyo3(get)]
    pub content: String,
    /// Type of result (e.g. "web", "image", "video", "news").
    #[pyo3(get)]
    pub result_type: String,
    /// Names of engines that returned this result.
    #[pyo3(get)]
    pub engines: Vec<String>,
    /// Calculated relevance score.
    #[pyo3(get)]
    pub score: f64,
    /// Thumbnail URL, if available.
    #[pyo3(get)]
    pub thumbnail: Option<String>,
    /// Published date, if available.
    #[pyo3(get)]
    pub published_date: Option<String>,
}

#[pymethods]
impl PySearchResult {
    fn __repr__(&self) -> String {
        format!(
            "SearchResult(title='{}', url='{}', score={:.2})",
            self.title, self.url, self.score
        )
    }
}

/// Options for configuring a search request.
#[pyclass]
#[derive(Clone, Debug)]
pub struct PySearchOptions {
    /// Engine shortcuts to use (e.g. ["ddg", "wiki", "brave"]).
    #[pyo3(get, set)]
    pub engines: Option<Vec<String>>,
    /// Maximum number of results to return.
    #[pyo3(get, set)]
    pub limit: Option<u32>,
    /// Per-engine timeout in seconds. Defaults to 10.
    #[pyo3(get, set)]
    pub timeout: Option<u32>,
    /// HTTP/SOCKS5 proxy URL.
    #[pyo3(get, set)]
    pub proxy: Option<String>,
    /// Proxy pool URLs for IP rotation (e.g. ["http://10.0.0.1:8080"]).
    /// When provided, proxies are rotated round-robin per request.
    /// Takes precedence over `proxy` if both are set.
    #[pyo3(get, set)]
    pub proxy_pool: Option<Vec<String>>,
    /// Search language (e.g. "en", "zh", "ja").
    #[pyo3(get, set)]
    pub language: Option<String>,
    /// Safe search level: "off", "moderate", or "strict".
    #[pyo3(get, set)]
    pub safesearch: Option<String>,
    /// Page number for pagination (1-indexed).
    #[pyo3(get, set)]
    pub page: Option<u32>,
    /// Time range filter: "day", "week", "month", or "year".
    #[pyo3(get, set)]
    pub time_range: Option<String>,
    /// Search category (e.g. "general", "images", "videos", "news").
    #[pyo3(get, set)]
    pub category: Option<String>,
    /// Per-engine weight multipliers (e.g. {"ddg": 1.5, "brave": 0.8}).
    #[pyo3(get, set)]
    pub engine_weights: Option<std::collections::HashMap<String, f64>>,
    /// Maximum consecutive failures before suspending an engine.
    #[pyo3(get, set)]
    pub health_max_failures: Option<u32>,
    /// Suspension duration in seconds after max failures reached.
    #[pyo3(get, set)]
    pub health_suspend_secs: Option<u64>,
    /// Browser backend for headless engines: "chrome" or "lightpanda". Defaults to "lightpanda".
    #[pyo3(get, set)]
    pub browser: Option<String>,
    /// Path to Chrome executable (only used when browser_backend is "chrome").
    #[pyo3(get, set)]
    pub chrome_path: Option<String>,
    /// Path to Lightpanda executable (only used when browser_backend is "lightpanda").
    #[pyo3(get, set)]
    pub lightpanda_path: Option<String>,
    /// Maximum concurrent browser tabs. Defaults to 4.
    #[pyo3(get, set)]
    pub max_tabs: Option<usize>,
}

#[pymethods]
impl PySearchOptions {
    #[new]
    #[pyo3(signature = (engines=None, limit=None, timeout=None, proxy=None, proxy_pool=None, language=None, safesearch=None, page=None, time_range=None, category=None, engine_weights=None, health_max_failures=None, health_suspend_secs=None, browser=None, chrome_path=None, lightpanda_path=None, max_tabs=None))]
    fn new(
        engines: Option<Vec<String>>,
        limit: Option<u32>,
        timeout: Option<u32>,
        proxy: Option<String>,
        proxy_pool: Option<Vec<String>>,
        language: Option<String>,
        safesearch: Option<String>,
        page: Option<u32>,
        time_range: Option<String>,
        category: Option<String>,
        engine_weights: Option<std::collections::HashMap<String, f64>>,
        health_max_failures: Option<u32>,
        health_suspend_secs: Option<u64>,
        browser: Option<String>,
        chrome_path: Option<String>,
        lightpanda_path: Option<String>,
        max_tabs: Option<usize>,
    ) -> Self {
        Self {
            engines,
            limit,
            timeout,
            proxy,
            proxy_pool,
            language,
            safesearch,
            page,
            time_range,
            category,
            engine_weights,
            health_max_failures,
            health_suspend_secs,
            browser,
            chrome_path,
            lightpanda_path,
            max_tabs,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SearchOptions(engines={:?}, limit={:?}, timeout={:?})",
            self.engines, self.limit, self.timeout
        )
    }
}

/// An error from a specific search engine.
#[pyclass]
#[derive(Clone, Debug)]
pub struct PyEngineError {
    /// Name of the engine that failed.
    #[pyo3(get)]
    pub engine: String,
    /// Error message.
    #[pyo3(get)]
    pub message: String,
}

#[pymethods]
impl PyEngineError {
    fn __repr__(&self) -> String {
        format!(
            "EngineError(engine='{}', message='{}')",
            self.engine, self.message
        )
    }
}

/// Aggregated search response containing results and metadata.
#[pyclass]
#[derive(Clone, Debug)]
pub struct PySearchResponse {
    /// The search results.
    #[pyo3(get)]
    pub results: Vec<PySearchResult>,
    /// Total number of results.
    #[pyo3(get)]
    pub count: u32,
    /// Search duration in milliseconds.
    #[pyo3(get)]
    pub duration_ms: u32,
    /// Engine errors that occurred during search.
    #[pyo3(get)]
    pub errors: Vec<PyEngineError>,
    /// Search suggestions (related queries).
    #[pyo3(get)]
    pub suggestions: Vec<String>,
    /// Instant answers (e.g. calculator results, definitions).
    #[pyo3(get)]
    pub answers: Vec<String>,
}

#[pymethods]
impl PySearchResponse {
    fn __repr__(&self) -> String {
        format!(
            "SearchResponse(count={}, duration_ms={}, errors={})",
            self.count,
            self.duration_ms,
            self.errors.len()
        )
    }
}
