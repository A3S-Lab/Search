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
 */
export class A3SSearch {
  private native: InstanceType<typeof JsSearch>;

  constructor() {
    this.native = new JsSearch();
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
