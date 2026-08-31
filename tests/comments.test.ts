import { describe, expect, it } from "vitest";
import { locateQuote } from "$lib/comments";

describe("locateQuote", () => {
  it("finds a unique quote", () => {
    expect(locateQuote("the quick brown fox", "quick", "the ", " brown")).toBe(4);
  });

  it("returns -1 when the quote is gone", () => {
    expect(locateQuote("hello world", "missing", "", "")).toBe(-1);
  });

  it("disambiguates repeated quotes via context", () => {
    const text = "alpha cat beta cat gamma";
    // Two "cat"s; the context points at the second one.
    expect(locateQuote(text, "cat", "beta ", " gamma")).toBe(15);
    expect(locateQuote(text, "cat", "alpha ", " beta")).toBe(6);
  });

  it("survives partial context after nearby edits", () => {
    const original = "one two three four";
    // Context recorded from an older revision; suffix half-matches.
    expect(locateQuote(original, "three", "XXX two ", " four")).toBe(8);
  });

  it("rejects empty quotes", () => {
    expect(locateQuote("anything", "", "", "")).toBe(-1);
  });
});
