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
  /** Engine shortcuts to use. Defaults to ["ddg", "wiki"]. */
  engines?: string[];
  /** Maximum number of results to return. */
  limit?: number;
  /** Per-engine timeout in seconds. Defaults to 10. */
  timeout?: number;
  /** HTTP/SOCKS5 proxy URL. */
  proxy?: string;
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
}
