import { describe, it, expect } from "vitest";
import {
  A3SSearch,
  A3SSearchError,
  NativeModuleError,
  SearchError,
} from "../lib";
import type { SearchOptions, SearchResponse, SearchResult, EngineError } from "../lib";

// =============================================================================
// Unit Tests — Error Classes
// =============================================================================

describe("Error Classes", () => {
  describe("A3SSearchError", () => {
    it("should be an instance of Error", () => {
      const err = new A3SSearchError("test");
      expect(err).toBeInstanceOf(Error);
      expect(err).toBeInstanceOf(A3SSearchError);
    });

    it("should have correct name", () => {
      const err = new A3SSearchError("test");
      expect(err.name).toBe("A3SSearchError");
    });

    it("should preserve message", () => {
      const err = new A3SSearchError("something went wrong");
      expect(err.message).toBe("something went wrong");
    });

    it("should have a stack trace", () => {
      const err = new A3SSearchError("test");
      expect(err.stack).toBeDefined();
    });
  });

  describe("NativeModuleError", () => {
    it("should extend A3SSearchError", () => {
      const err = new NativeModuleError("load failed");
      expect(err).toBeInstanceOf(A3SSearchError);
      expect(err).toBeInstanceOf(Error);
    });

    it("should have correct name", () => {
      const err = new NativeModuleError("load failed");
      expect(err.name).toBe("NativeModuleError");
    });

    it("should preserve message", () => {
      const err = new NativeModuleError("cannot find .node file");
      expect(err.message).toBe("cannot find .node file");
    });
  });

  describe("SearchError", () => {
    it("should extend A3SSearchError", () => {
      const err = new SearchError("search failed");
      expect(err).toBeInstanceOf(A3SSearchError);
      expect(err).toBeInstanceOf(Error);
    });

    it("should have correct name", () => {
      const err = new SearchError("search failed");
      expect(err.name).toBe("SearchError");
    });

    it("should preserve message", () => {
      const err = new SearchError("timeout exceeded");
      expect(err.message).toBe("timeout exceeded");
    });

    it("should be catchable as A3SSearchError", () => {
      try {
        throw new SearchError("test");
      } catch (e) {
        expect(e).toBeInstanceOf(A3SSearchError);
      }
    });

    it("should be distinguishable from NativeModuleError", () => {
      const searchErr = new SearchError("search");
      const nativeErr = new NativeModuleError("native");
      expect(searchErr).not.toBeInstanceOf(NativeModuleError);
      expect(nativeErr).not.toBeInstanceOf(SearchError);
    });
  });
});

// =============================================================================
// Unit Tests — A3SSearch Input Validation
// =============================================================================

describe("A3SSearch", () => {
  describe("constructor", () => {
    it("should create an instance", () => {
      const search = new A3SSearch();
      expect(search).toBeDefined();
      expect(search).toBeInstanceOf(A3SSearch);
    });

    it("should create multiple independent instances", () => {
      const a = new A3SSearch();
      const b = new A3SSearch();
      expect(a).not.toBe(b);
    });
  });

  describe("input validation", () => {
    it("should reject empty string query", async () => {
      const search = new A3SSearch();
      await expect(search.search("")).rejects.toThrow(SearchError);
    });

    it("should reject whitespace-only query", async () => {
      const search = new A3SSearch();
      await expect(search.search("   ")).rejects.toThrow(SearchError);
    });

    it("should reject tab-only query", async () => {
      const search = new A3SSearch();
      await expect(search.search("\t\t")).rejects.toThrow(SearchError);
    });

    it("should reject newline-only query", async () => {
      const search = new A3SSearch();
      await expect(search.search("\n\n")).rejects.toThrow(SearchError);
    });

    it("should reject mixed whitespace query", async () => {
      const search = new A3SSearch();
      await expect(search.search(" \t\n ")).rejects.toThrow(SearchError);
    });

    it("should include meaningful error message for empty query", async () => {
      const search = new A3SSearch();
      try {
        await search.search("");
        expect.fail("should have thrown");
      } catch (e) {
        expect(e).toBeInstanceOf(SearchError);
        expect((e as SearchError).message).toContain("empty");
      }
    });
  });

  describe("engine validation", () => {
    it("should reject unknown engine shortcut", async () => {
      const search = new A3SSearch();
      await expect(
        search.search("test", { engines: ["nonexistent"] })
      ).rejects.toThrow();
    });

    it("should reject unknown engine with descriptive message", async () => {
      const search = new A3SSearch();
      try {
        await search.search("test", { engines: ["foobar"] });
        expect.fail("should have thrown");
      } catch (e) {
        expect((e as Error).message).toContain("foobar");
      }
    });

    it("should reject empty engines array", async () => {
      const search = new A3SSearch();
      await expect(
        search.search("test", { engines: [] })
      ).rejects.toThrow();
    });

    it("should reject if all engines are unknown", async () => {
      const search = new A3SSearch();
      await expect(
        search.search("test", { engines: ["x", "y", "z"] })
      ).rejects.toThrow();
    });
  });
});

// =============================================================================
// Unit Tests — TypeScript Type Contracts
// =============================================================================

describe("Type Contracts", () => {
  describe("SearchOptions", () => {
    it("should accept empty options", () => {
      const opts: SearchOptions = {};
      expect(opts.engines).toBeUndefined();
      expect(opts.limit).toBeUndefined();
      expect(opts.timeout).toBeUndefined();
      expect(opts.proxy).toBeUndefined();
    });

    it("should accept all fields", () => {
      const opts: SearchOptions = {
        engines: ["ddg", "wiki"],
        limit: 10,
        timeout: 15,
        proxy: "http://127.0.0.1:8080",
      };
      expect(opts.engines).toEqual(["ddg", "wiki"]);
      expect(opts.limit).toBe(10);
      expect(opts.timeout).toBe(15);
      expect(opts.proxy).toBe("http://127.0.0.1:8080");
    });

    it("should accept partial options", () => {
      const opts: SearchOptions = { engines: ["brave"], limit: 5 };
      expect(opts.engines).toEqual(["brave"]);
      expect(opts.limit).toBe(5);
      expect(opts.timeout).toBeUndefined();
    });
  });

  describe("SearchResult", () => {
    it("should have all required fields", () => {
      const result: SearchResult = {
        url: "https://example.com",
        title: "Example",
        content: "Description",
        resultType: "web",
        engines: ["ddg"],
        score: 1.5,
      };
      expect(result.url).toBe("https://example.com");
      expect(result.title).toBe("Example");
      expect(result.content).toBe("Description");
      expect(result.resultType).toBe("web");
      expect(result.engines).toEqual(["ddg"]);
      expect(result.score).toBe(1.5);
      expect(result.thumbnail).toBeUndefined();
      expect(result.publishedDate).toBeUndefined();
    });

    it("should accept optional fields", () => {
      const result: SearchResult = {
        url: "https://example.com",
        title: "Example",
        content: "Description",
        resultType: "image",
        engines: ["brave"],
        score: 2.0,
        thumbnail: "https://example.com/thumb.jpg",
        publishedDate: "2024-01-15",
      };
      expect(result.thumbnail).toBe("https://example.com/thumb.jpg");
      expect(result.publishedDate).toBe("2024-01-15");
    });
  });

  describe("EngineError", () => {
    it("should have engine and message fields", () => {
      const err: EngineError = {
        engine: "DuckDuckGo",
        message: "timed out",
      };
      expect(err.engine).toBe("DuckDuckGo");
      expect(err.message).toBe("timed out");
    });
  });

  describe("SearchResponse", () => {
    it("should have all required fields", () => {
      const response: SearchResponse = {
        results: [],
        count: 0,
        durationMs: 42,
        errors: [],
      };
      expect(response.results).toEqual([]);
      expect(response.count).toBe(0);
      expect(response.durationMs).toBe(42);
      expect(response.errors).toEqual([]);
    });

    it("should hold results and errors together", () => {
      const response: SearchResponse = {
        results: [
          {
            url: "https://example.com",
            title: "Test",
            content: "Content",
            resultType: "web",
            engines: ["ddg"],
            score: 1.0,
          },
        ],
        count: 1,
        durationMs: 100,
        errors: [{ engine: "brave", message: "CAPTCHA" }],
      };
      expect(response.results).toHaveLength(1);
      expect(response.errors).toHaveLength(1);
      expect(response.count).toBe(1);
    });
  });
});

// =============================================================================
// Integration Tests — Real Search (requires network)
// =============================================================================

describe("A3SSearch Integration", () => {
  it("should search with default engines (ddg + wiki)", async () => {
    const search = new A3SSearch();
    const response = await search.search("rust programming language");

    expect(response).toBeDefined();
    expect(response.results).toBeInstanceOf(Array);
    expect(response.count).toBeGreaterThanOrEqual(0);
    expect(response.durationMs).toBeGreaterThanOrEqual(0);
    expect(response.errors).toBeInstanceOf(Array);
    expect(response.count).toBe(response.results.length);
  });

  it("should search with ddg engine", async () => {
    const search = new A3SSearch();
    const response = await search.search("typescript language", {
      engines: ["ddg"],
    });

    expect(response).toBeDefined();
    expect(response.count).toBe(response.results.length);
  });

  it("should search with duckduckgo alias", async () => {
    const search = new A3SSearch();
    const response = await search.search("javascript", {
      engines: ["duckduckgo"],
    });

    expect(response).toBeDefined();
  });

  it("should search with wiki engine", async () => {
    const search = new A3SSearch();
    const response = await search.search("Python programming", {
      engines: ["wiki"],
      limit: 3,
    });

    expect(response).toBeDefined();
    expect(response.results.length).toBeLessThanOrEqual(3);
  });

  it("should search with wikipedia alias", async () => {
    const search = new A3SSearch();
    const response = await search.search("Linux", {
      engines: ["wikipedia"],
    });

    expect(response).toBeDefined();
  });

  it("should search with brave engine", async () => {
    const search = new A3SSearch();
    const response = await search.search("open source", {
      engines: ["brave"],
    });

    expect(response).toBeDefined();
  });

  it("should search with sogou engine", async () => {
    const search = new A3SSearch();
    const response = await search.search("搜索引擎", {
      engines: ["sogou"],
    });

    expect(response).toBeDefined();
  });

  it("should search with 360 engine", async () => {
    const search = new A3SSearch();
    const response = await search.search("人工智能", {
      engines: ["360"],
    });

    expect(response).toBeDefined();
  });

  it("should search with so360 alias", async () => {
    const search = new A3SSearch();
    const response = await search.search("机器学习", {
      engines: ["so360"],
    });

    expect(response).toBeDefined();
  });

  it("should search with multiple engines", async () => {
    const search = new A3SSearch();
    const response = await search.search("web development", {
      engines: ["ddg", "wiki", "brave"],
    });

    expect(response).toBeDefined();
    expect(response.count).toBe(response.results.length);
  });

  it("should respect limit option", async () => {
    const search = new A3SSearch();
    const response = await search.search("programming", {
      engines: ["ddg"],
      limit: 2,
    });

    expect(response.results.length).toBeLessThanOrEqual(2);
    expect(response.count).toBeLessThanOrEqual(2);
  });

  it("should respect timeout option", async () => {
    const search = new A3SSearch();
    const response = await search.search("test query", {
      engines: ["ddg"],
      timeout: 20,
    });

    expect(response).toBeDefined();
  });

  it("should return properly structured results", async () => {
    const search = new A3SSearch();
    const response = await search.search("wikipedia test", {
      engines: ["wiki"],
      limit: 2,
    });

    for (const result of response.results) {
      // Required string fields
      expect(typeof result.url).toBe("string");
      expect(result.url.length).toBeGreaterThan(0);
      expect(typeof result.title).toBe("string");
      expect(typeof result.content).toBe("string");
      expect(typeof result.resultType).toBe("string");

      // Engines array
      expect(Array.isArray(result.engines)).toBe(true);
      expect(result.engines.length).toBeGreaterThan(0);

      // Score
      expect(typeof result.score).toBe("number");
      expect(result.score).toBeGreaterThanOrEqual(0);

      // Optional fields should be string or undefined
      if (result.thumbnail !== undefined) {
        expect(typeof result.thumbnail).toBe("string");
      }
      if (result.publishedDate !== undefined) {
        expect(typeof result.publishedDate).toBe("string");
      }
    }
  });

  it("should return consistent count", async () => {
    const search = new A3SSearch();
    const response = await search.search("nodejs", {
      engines: ["ddg"],
    });

    expect(response.count).toBe(response.results.length);
  });

  it("should return non-negative duration", async () => {
    const search = new A3SSearch();
    const response = await search.search("test", {
      engines: ["ddg"],
    });

    expect(response.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("should handle concurrent searches", async () => {
    const search = new A3SSearch();
    const [r1, r2, r3] = await Promise.all([
      search.search("rust", { engines: ["ddg"], limit: 2 }),
      search.search("python", { engines: ["ddg"], limit: 2 }),
      search.search("javascript", { engines: ["ddg"], limit: 2 }),
    ]);

    expect(r1).toBeDefined();
    expect(r2).toBeDefined();
    expect(r3).toBeDefined();
  });

  it("should handle concurrent searches from different instances", async () => {
    const s1 = new A3SSearch();
    const s2 = new A3SSearch();

    const [r1, r2] = await Promise.all([
      s1.search("rust", { engines: ["ddg"], limit: 2 }),
      s2.search("python", { engines: ["wiki"], limit: 2 }),
    ]);

    expect(r1).toBeDefined();
    expect(r2).toBeDefined();
  });
});
