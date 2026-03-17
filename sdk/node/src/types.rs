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
    /// Engine shortcuts to use (e.g. ["ddg", "wiki", "brave", "bing"]).
    /// Defaults to ["ddg", "wiki"] if not specified.
    /// Available: ddg, brave, bing, wiki, sogou, 360.
    pub engines: Option<Vec<String>>,
    /// Maximum number of results to return.
    pub limit: Option<u32>,
    /// Per-engine timeout in seconds. Defaults to 10.
    pub timeout: Option<u32>,
    /// HTTP/SOCKS5 proxy URL (e.g. "http://127.0.0.1:8080").
    pub proxy: Option<String>,
    /// Proxy pool URLs for IP rotation (e.g. ["http://10.0.0.1:8080", "http://10.0.0.2:8080"]).
    /// When provided, proxies are rotated round-robin per request.
    /// Takes precedence over `proxy` if both are set.
    pub proxy_pool: Option<Vec<String>>,
    /// Search language (e.g. "en", "zh", "ja").
    pub language: Option<String>,
    /// Safe search level: "off", "moderate", or "strict".
    pub safesearch: Option<String>,
    /// Page number for pagination (1-indexed).
    pub page: Option<u32>,
    /// Time range filter: "day", "week", "month", or "year".
    pub time_range: Option<String>,
    /// Search category (e.g. "general", "images", "videos", "news").
    pub category: Option<String>,
    /// Per-engine weight multipliers (e.g. {"ddg": 1.5, "brave": 0.8}).
    pub engine_weights: Option<std::collections::HashMap<String, f64>>,
    /// Maximum consecutive failures before suspending an engine.
    pub health_max_failures: Option<u32>,
    /// Suspension duration in seconds after max failures reached.
    pub health_suspend_secs: Option<u32>,
    /// Browser backend for headless engines: "chrome" or "lightpanda". Defaults to "lightpanda".
    pub browser: Option<String>,
    /// Path to Chrome executable (only used when browser_backend is "chrome").
    pub chrome_path: Option<String>,
    /// Path to Lightpanda executable (only used when browser_backend is "lightpanda").
    pub lightpanda_path: Option<String>,
    /// Maximum concurrent browser tabs. Defaults to 4.
    pub max_tabs: Option<u32>,
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
    /// Search suggestions (related queries).
    pub suggestions: Vec<String>,
    /// Instant answers (e.g. calculator results, definitions).
    pub answers: Vec<String>,
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
