import { describe, expect, it } from "vitest";
import {
  emptyHistory,
  forgetPath,
  goBack,
  goForward,
  recordVisit,
  renamePath,
} from "$lib/navhistory";

const at = (path: string, scroll = 0) => ({ path, scroll });

describe("navigation history", () => {
  it("starts empty", () => {
    const h = emptyHistory();
    expect(goBack(h, at("/a.md"))).toBeNull();
    expect(goForward(h, at("/a.md"))).toBeNull();
  });

  it("goes back to the previous document and its scroll position", () => {
    const h = emptyHistory();
    recordVisit(h, at("/a.md", 420));
    expect(goBack(h, at("/b.md", 10))).toEqual(at("/a.md", 420));
    expect(h.back).toEqual([]);
    expect(h.forward).toEqual([at("/b.md", 10)]);
  });

  it("goes forward again after going back", () => {
    const h = emptyHistory();
    recordVisit(h, at("/a.md"));
    goBack(h, at("/b.md", 99));
    expect(goForward(h, at("/a.md", 5))).toEqual(at("/b.md", 99));
    expect(h.back).toEqual([at("/a.md", 5)]);
    expect(h.forward).toEqual([]);
  });

  it("can go back with no document open (after deleting it)", () => {
    const h = emptyHistory();
    recordVisit(h, at("/a.md", 7));
    expect(goBack(h, null)).toEqual(at("/a.md", 7));
    expect(h.forward).toEqual([]);
  });

  it("a new visit clears the forward stack", () => {
    const h = emptyHistory();
    recordVisit(h, at("/a.md"));
    goBack(h, at("/b.md"));
    recordVisit(h, at("/a.md"));
    expect(h.forward).toEqual([]);
  });

  it("does not stack consecutive visits from the same document", () => {
    const h = emptyHistory();
    recordVisit(h, at("/a.md", 1));
    recordVisit(h, at("/a.md", 2));
    expect(h.back).toEqual([at("/a.md", 2)]);
  });

  it("caps the back stack", () => {
    const h = emptyHistory();
    for (let i = 0; i < 150; i++) recordVisit(h, at(`/${i}.md`));
    expect(h.back.length).toBe(100);
    expect(h.back[h.back.length - 1].path).toBe("/149.md");
  });

  it("follows renames of a file and of a folder", () => {
    const h = emptyHistory();
    recordVisit(h, at("/notes/a.md"));
    recordVisit(h, at("/notes/sub/b.md"));
    recordVisit(h, at("/other.md"));
    renamePath(h, "/notes/a.md", "/notes/z.md");
    renamePath(h, "/notes/sub", "/notes/deep");
    expect(h.back.map((e) => e.path)).toEqual([
      "/notes/z.md",
      "/notes/deep/b.md",
      "/other.md",
    ]);
  });

  it("forgets deleted documents and folders, merging neighbours left behind", () => {
    const h = emptyHistory();
    recordVisit(h, at("/a.md"));
    recordVisit(h, at("/gone/x.md"));
    recordVisit(h, at("/a.md"));
    recordVisit(h, at("/gone.md"));
    forgetPath(h, "/gone");
    expect(h.back.map((e) => e.path)).toEqual(["/a.md", "/gone.md"]);
    forgetPath(h, "/gone.md");
    expect(h.back.map((e) => e.path)).toEqual(["/a.md"]);
  });
});
