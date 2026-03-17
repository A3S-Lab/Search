/** A single search result. */
export interface SearchResult {
  /** Result URL. */
  url: string;
  /** Result title. */
  title: string;
  /** Result description/snippet. */
  content: string;
  /** Type of result (e.g. "web", "image", "video"). */
  resultType: string;
  /** Names of engines that returned this result. */
  engines: string[];
  /** Calculated relevance score. */
  score: number;
  /** Thumbnail URL, if available. */
  thumbnail?: string;
  /** Published date, if available. */
  publishedDate?: string;
}

/** Options for configuring a search request. */
export interface SearchOptions {
  /** Engine shortcuts to use. Defaults to ["ddg", "wiki"].
   * Available: ddg, brave, bing, wiki, sogou, 360, google, baidu, bingchina.
   * Note: google, baidu, bingchina require headless browser (slower but more reliable). */
  engines?: string[];
  /** Maximum number of results to return. */
  limit?: number;
  /** Per-engine timeout in seconds. Defaults to 10. */
  timeout?: number;
  /** HTTP/SOCKS5 proxy URL. */
  proxy?: string;
  /** Proxy pool URLs for IP rotation.
   * When provided, proxies are rotated round-robin per request.
   * Takes precedence over `proxy` if both are set. */
  proxyPool?: string[];
  /** Search language (e.g. "en", "zh", "ja"). */
  language?: string;
  /** Safe search level: "off", "moderate", or "strict". */
  safesearch?: string;
  /** Page number for pagination (1-indexed). */
  page?: number;
  /** Time range filter: "day", "week", "month", or "year". */
  timeRange?: string;
  /** Search category (e.g. "general", "images", "videos", "news"). */
  category?: string;
  /** Per-engine weight multipliers (e.g. {"ddg": 1.5, "brave": 0.8}). */
  engineWeights?: Record<string, number>;
  /** Maximum consecutive failures before suspending an engine. */
  healthMaxFailures?: number;
  /** Suspension duration in seconds after max failures reached. */
  healthSuspendSecs?: number;
  /** Browser backend for headless engines: "chrome" or "lightpanda". Defaults to "lightpanda". */
  browser?: string;
  /** Path to Chrome executable (only used when browser is "chrome"). */
  chromePath?: string;
  /** Path to Lightpanda executable (only used when browser is "lightpanda"). */
  lightpandaPath?: string;
  /** Maximum concurrent browser tabs. Defaults to 4. */
  maxTabs?: number;
}

/** An error from a specific search engine. */
export interface EngineError {
  /** Name of the engine that failed. */
  engine: string;
  /** Error message. */
  message: string;
}

/** Aggregated search response. */
export interface SearchResponse {
  /** The search results. */
  results: SearchResult[];
  /** Total number of results. */
  count: number;
  /** Search duration in milliseconds. */
  durationMs: number;
  /** Engine errors that occurred during search. */
  errors: EngineError[];
  /** Search suggestions (related queries). */
  suggestions: string[];
  /** Instant answers (e.g. calculator results, definitions). */
  answers: string[];
}
