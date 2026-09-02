import { describe, expect, it } from "vitest";
import { toggleTaskMarker } from "$lib/tasks";

/** Toggle within the whole string, expecting `count` checkboxes rendered. */
const toggle = (src: string, index: number, count: number) =>
  toggleTaskMarker(src, 0, src.length, index, count);

describe("toggleTaskMarker", () => {
  it("checks an open task", () => {
    expect(toggle("- [ ] buy milk\n", 0, 1)).toBe("- [x] buy milk\n");
  });

  it("unchecks a done task, either capitalisation", () => {
    expect(toggle("- [x] done\n", 0, 1)).toBe("- [ ] done\n");
    expect(toggle("- [X] done\n", 0, 1)).toBe("- [ ] done\n");
  });

  it("targets the n-th marker only", () => {
    const src = "- [ ] one\n- [x] two\n- [ ] three\n";
    expect(toggle(src, 2, 3)).toBe("- [ ] one\n- [x] two\n- [x] three\n");
  });

  it("handles nested and ordered task lists", () => {
    const src = "- [ ] parent\n  - [ ] child\n1. [ ] first\n2) [ ] second\n";
    expect(toggle(src, 1, 4)).toBe("- [ ] parent\n  - [x] child\n1. [ ] first\n2) [ ] second\n");
    expect(toggle(src, 3, 4)).toBe("- [ ] parent\n  - [ ] child\n1. [ ] first\n2) [x] second\n");
  });

  it("handles task lists inside blockquotes", () => {
    expect(toggle("> - [ ] quoted\n", 0, 1)).toBe("> - [x] quoted\n");
    expect(toggle("> > - [x] deep\n", 0, 1)).toBe("> > - [ ] deep\n");
  });

  it("only touches the given block range", () => {
    const before = "- [ ] outside\n";
    const block = "- [ ] inside\n";
    const src = before + block;
    expect(toggleTaskMarker(src, before.length, src.length, 0, 1)).toBe(
      "- [ ] outside\n- [x] inside\n",
    );
  });

  it("keeps the content length unchanged", () => {
    const src = "- [ ] a\n- [x] b\n";
    expect(toggle(src, 0, 2)).toHaveLength(src.length);
  });

  it("bails out when marker and checkbox counts disagree", () => {
    // A fenced code block renders no checkbox, but the naive line scan would
    // still count its marker — the mismatch must abort the toggle.
    const src = "- [ ] real\n";
    expect(toggle(src, 0, 2)).toBeNull();
  });

  it("bails out on an out-of-range index", () => {
    expect(toggle("- [ ] a\n", 1, 1)).toBeNull();
    expect(toggle("- [ ] a\n", -1, 1)).toBeNull();
  });

  it("ignores lines that only look like tasks", () => {
    const src = "[ ] no list marker\ntext - [ ] mid-line\n- [ ] real\n";
    expect(toggle(src, 0, 1)).toBe("[ ] no list marker\ntext - [ ] mid-line\n- [x] real\n");
  });
});
