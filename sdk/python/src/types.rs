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
}

#[pymethods]
impl PySearchOptions {
    #[new]
    #[pyo3(signature = (engines=None, limit=None, timeout=None, proxy=None))]
    fn new(
        engines: Option<Vec<String>>,
        limit: Option<u32>,
        timeout: Option<u32>,
        proxy: Option<String>,
    ) -> Self {
        Self {
            engines,
            limit,
            timeout,
            proxy,
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
