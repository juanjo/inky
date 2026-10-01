import { describe, expect, it } from "vitest";
import { displayDir, isInside } from "$lib/paths";

describe("isInside", () => {
  it("matches the root itself and its descendants only", () => {
    expect(isInside("/lib", "/lib")).toBe(true);
    expect(isInside("/lib/a.md", "/lib")).toBe(true);
    expect(isInside("/library/a.md", "/lib")).toBe(false);
    expect(isInside("/a.md", "")).toBe(false);
  });
});

describe("displayDir", () => {
  it("shows the parent folder with ~ for home", () => {
    expect(displayDir("/Users/me/repo/README.md", "/Users/me")).toBe("~/repo");
    expect(displayDir("/Users/me/a.md", "/Users/me")).toBe("~");
    expect(displayDir("/opt/x.md", "/Users/me")).toBe("/opt");
    expect(displayDir("/opt/x.md", "")).toBe("/opt");
  });
});
