import { describe, expect, it } from "vitest";
import { breadcrumbs, displayDir, isInside } from "$lib/paths";

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

describe("breadcrumbs", () => {
  const roots = { library: "/Users/me/Inky", workspace: null, home: "/Users/me" };

  it("lists Library then each folder down to the document's own folder", () => {
    expect(breadcrumbs("/Users/me/Inky/Brains/00 Start/05 Catalog.md", roots)).toEqual([
      { label: "Library", dir: "/Users/me/Inky", browsable: true },
      { label: "Brains", dir: "/Users/me/Inky/Brains", browsable: true },
      { label: "00 Start", dir: "/Users/me/Inky/Brains/00 Start", browsable: true },
    ]);
  });

  it("a document at the library root has only the Library crumb", () => {
    expect(breadcrumbs("/Users/me/Inky/a.md", roots)).toEqual([
      { label: "Library", dir: "/Users/me/Inky", browsable: true },
    ]);
  });

  it("roots at the opened folder, even when it sits inside the library", () => {
    const ws = { ...roots, workspace: "/Users/me/Inky/Brains" };
    expect(breadcrumbs("/Users/me/Inky/Brains/x/a.md", ws)).toEqual([
      { label: "Brains", dir: "/Users/me/Inky/Brains", browsable: true },
      { label: "x", dir: "/Users/me/Inky/Brains/x", browsable: true },
    ]);
  });

  it("an outside document shows its folder, not browsable", () => {
    expect(breadcrumbs("/Users/me/repo/docs/README.md", roots)).toEqual([
      { label: "~/repo/docs", dir: "/Users/me/repo/docs", browsable: false },
    ]);
  });
});
