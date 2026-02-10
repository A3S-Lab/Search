import { describe, it, expect } from "vitest";
import { A3SSearch, SearchError } from "../lib";

describe("A3SSearch", () => {
  it("should create an instance", () => {
    const search = new A3SSearch();
    expect(search).toBeDefined();
  });

  it("should reject empty query", async () => {
    const search = new A3SSearch();
    await expect(search.search("")).rejects.toThrow(SearchError);
    await expect(search.search("   ")).rejects.toThrow(SearchError);
  });

  it("should reject unknown engine", async () => {
    const search = new A3SSearch();
    await expect(
      search.search("test", { engines: ["nonexistent"] })
    ).rejects.toThrow();
  });

  it("should search with default engines", async () => {
    const search = new A3SSearch();
    const response = await search.search("rust programming language");

    expect(response).toBeDefined();
    expect(response.results).toBeInstanceOf(Array);
    expect(response.count).toBeGreaterThanOrEqual(0);
    expect(response.durationMs).toBeGreaterThanOrEqual(0);
    expect(response.errors).toBeInstanceOf(Array);
  });

  it("should search with specific engines", async () => {
    const search = new A3SSearch();
    const response = await search.search("typescript", {
      engines: ["ddg"],
      limit: 3,
    });

    expect(response).toBeDefined();
    expect(response.results.length).toBeLessThanOrEqual(3);
  });

  it("should return properly typed results", async () => {
    const search = new A3SSearch();
    const response = await search.search("wikipedia test", {
      engines: ["wiki"],
      limit: 2,
    });

    for (const result of response.results) {
      expect(typeof result.url).toBe("string");
      expect(typeof result.title).toBe("string");
      expect(typeof result.content).toBe("string");
      expect(typeof result.resultType).toBe("string");
      expect(Array.isArray(result.engines)).toBe(true);
      expect(typeof result.score).toBe("number");
    }
  });

  it("should respect timeout option", async () => {
    const search = new A3SSearch();
    const response = await search.search("test query", {
      engines: ["ddg"],
      timeout: 15,
    });

    expect(response).toBeDefined();
  });

  it("should respect limit option", async () => {
    const search = new A3SSearch();
    const response = await search.search("javascript", {
      engines: ["ddg"],
      limit: 1,
    });

    expect(response.results.length).toBeLessThanOrEqual(1);
  });
});
