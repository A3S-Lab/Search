import { JsSearch } from "./native";
import { SearchError } from "./errors";
import type { SearchOptions, SearchResponse, SearchResult } from "./types";

/**
 * A3S Search client.
 *
 * Provides an ergonomic TypeScript wrapper around the native Rust search engine.
 *
 * @example
 * ```typescript
 * const search = new A3SSearch();
 * const response = await search.search("rust programming");
 * for (const r of response.results) {
 *   console.log(`${r.title}: ${r.url}`);
 * }
 * ```
 *
 * @example Proxy pool
 * ```typescript
 * const search = new A3SSearch();
 * await search.setProxyPool([
 *   "http://10.0.0.1:8080",
 *   "http://10.0.0.2:8080",
 * ]);
 * const response = await search.search("rust programming");
 * ```
 */
export class A3SSearch {
  private native: InstanceType<typeof JsSearch>;

  constructor() {
    this.native = new JsSearch();
  }

  /**
   * Set the proxy pool URLs for IP rotation.
   *
   * Replaces any existing proxies. Automatically enables the proxy pool.
   * Each URL should be "http://host:port", "https://host:port", or "socks5://host:port".
   */
  async setProxyPool(urls: string[]): Promise<void> {
    await this.native.setProxyPool(urls);
  }

  /**
   * Enable or disable the proxy pool.
   *
   * When disabled, requests are made directly without a proxy.
   */
  setProxyPoolEnabled(enabled: boolean): void {
    this.native.setProxyPoolEnabled(enabled);
  }

  /** Returns whether the proxy pool is currently enabled. */
  isProxyPoolEnabled(): boolean {
    return this.native.isProxyPoolEnabled();
  }

  /** Returns the number of proxies in the pool. */
  async proxyPoolSize(): Promise<number> {
    return this.native.proxyPoolSize();
  }

  /**
   * Perform a search query.
   *
   * @param query - The search query string.
   * @param options - Optional search configuration.
   * @returns A promise resolving to the search response.
   */
  async search(query: string, options?: SearchOptions): Promise<SearchResponse> {
    if (!query || query.trim().length === 0) {
      throw new SearchError("Query cannot be empty");
    }

    try {
      const nativeOpts = options
        ? {
            engines: options.engines,
            limit: options.limit,
            timeout: options.timeout,
            proxy: options.proxy,
            proxyPool: options.proxyPool,
          }
        : undefined;

      const response = await this.native.search(query, nativeOpts);

      // Map native response to TypeScript types
      const results: SearchResult[] = response.results.map(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (r: any) => ({
          url: r.url,
          title: r.title,
          content: r.content,
          resultType: r.resultType,
          engines: r.engines,
          score: r.score,
          thumbnail: r.thumbnail ?? undefined,
          publishedDate: r.publishedDate ?? undefined,
        })
      );

      return {
        results,
        count: response.count,
        durationMs: response.durationMs,
        errors: response.errors.map(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (e: any) => ({
            engine: e.engine,
            message: e.message,
          })
        ),
      };
    } catch (err) {
      if (err instanceof SearchError) {
        throw err;
      }
      throw new SearchError(
        `Search failed: ${err instanceof Error ? err.message : String(err)}`
      );
    }
  }
}
