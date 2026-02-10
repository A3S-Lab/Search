use napi_derive::napi;

/// A single search result returned by an engine.
#[napi(object)]
#[derive(Clone, Debug)]
pub struct JsSearchResult {
    /// Result URL.
    pub url: String,
    /// Result title.
    pub title: String,
    /// Result description/snippet.
    pub content: String,
    /// Type of result (e.g. "web", "image", "video", "news").
    pub result_type: String,
    /// Names of engines that returned this result.
    pub engines: Vec<String>,
    /// Calculated relevance score.
    pub score: f64,
    /// Thumbnail URL, if available.
    pub thumbnail: Option<String>,
    /// Published date, if available.
    pub published_date: Option<String>,
}

/// Options for configuring a search request.
#[napi(object)]
#[derive(Clone, Debug)]
pub struct JsSearchOptions {
    /// Engine shortcuts to use (e.g. ["ddg", "wiki", "brave"]).
    /// Defaults to ["ddg", "wiki"] if not specified.
    pub engines: Option<Vec<String>>,
    /// Maximum number of results to return.
    pub limit: Option<u32>,
    /// Per-engine timeout in seconds. Defaults to 10.
    pub timeout: Option<u32>,
    /// HTTP/SOCKS5 proxy URL (e.g. "http://127.0.0.1:8080").
    pub proxy: Option<String>,
}

/// Aggregated search response containing results and metadata.
#[napi(object)]
#[derive(Clone, Debug)]
pub struct JsSearchResponse {
    /// The search results.
    pub results: Vec<JsSearchResult>,
    /// Total number of results.
    pub count: u32,
    /// Search duration in milliseconds.
    pub duration_ms: u32,
    /// Engine errors that occurred during search (engine_name: error_message).
    pub errors: Vec<JsEngineError>,
}

/// An error from a specific search engine.
#[napi(object)]
#[derive(Clone, Debug)]
pub struct JsEngineError {
    /// Name of the engine that failed.
    pub engine: String,
    /// Error message.
    pub message: String,
}
