import { describe, expect, it } from "vitest";
import { matchesSearch, normalizeSearch } from "$lib/search";

describe("search helpers", () => {
  it("normalizes whitespace and casing", () => {
    expect(normalizeSearch("  Hello Mikrom  ")).toBe("hello mikrom");
  });

  it("matches any candidate value against the query", () => {
    expect(matchesSearch(["Starter", null, undefined], "star")).toBe(true);
    expect(matchesSearch(["starter", "api"], "API")).toBe(true);
    expect(matchesSearch(["starter", "api"], "missing")).toBe(false);
  });

  it("treats empty queries as a match all", () => {
    expect(matchesSearch(["starter"], "")).toBe(true);
    expect(matchesSearch(["starter"], "   ")).toBe(true);
  });
});
