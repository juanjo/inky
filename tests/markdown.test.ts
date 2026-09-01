import { describe, expect, it, vi } from "vitest";

vi.mock("mermaid", () => ({
  default: { initialize: vi.fn(), render: vi.fn() },
}));

import { extractBlocks, extractToc, renderMarkdown } from "$lib/markdown";

const SAMPLE = `# Title

Intro text.

## Section one

\`\`\`md
# not a heading (fenced)
\`\`\`

## Section one

### **Bold** heading [link](https://x.dev)
`;

describe("extractToc", () => {
  it("extracts levels and lines, skipping fenced code", () => {
    const toc = extractToc(SAMPLE);
    expect(toc.map((t) => [t.level, t.text])).toEqual([
      [1, "Title"],
      [2, "Section one"],
      [2, "Section one"],
      [3, "Bold heading link"],
    ]);
    expect(toc[1].line).toBe(5);
  });

  it("deduplicates repeated heading ids", () => {
    const toc = extractToc(SAMPLE);
    expect(toc[1].id).toBe("section-one");
    expect(toc[2].id).toBe("section-one-1");
  });

  it("generates the same ids the renderer puts in the HTML", () => {
    const toc = extractToc(SAMPLE);
    const html = renderMarkdown(SAMPLE);
    for (const entry of toc) {
      expect(html).toContain(`id="${entry.id}"`);
    }
  });
});

describe("extractBlocks", () => {
  it("splits top-level blocks on blank lines, fence-aware", () => {
    const src = "# Head\n\nPara one\nstill para one\n\n```js\n\nconst x = 1;\n\n```\n\nPara two\n";
    const blocks = extractBlocks(src).map((b) => src.slice(b.start, b.end));
    expect(blocks).toEqual([
      "# Head",
      "Para one\nstill para one",
      "```js\n\nconst x = 1;\n\n```",
      "Para two",
    ]);
  });

  it("merges loose list items into one block", () => {
    const src = "intro\n\n- one\n\n- two\n  continued\n\noutro\n";
    const blocks = extractBlocks(src).map((b) => src.slice(b.start, b.end));
    expect(blocks).toEqual(["intro", "- one\n\n- two\n  continued", "outro"]);
  });

  it("round-trips edits by char range", () => {
    const src = "# A\n\nfirst\n\nsecond\n";
    const blocks = extractBlocks(src);
    const b = blocks[1];
    const edited = src.slice(0, b.start) + "FIRST!" + src.slice(b.end);
    expect(edited).toBe("# A\n\nFIRST!\n\nsecond\n");
  });
});

describe("renderMarkdown", () => {
  it("renders mermaid fences as placeholders, not code blocks", () => {
    const html = renderMarkdown("```mermaid\nflowchart LR\nA-->B\n```\n");
    expect(html).toContain("data-mermaid=");
    expect(html).not.toContain("<code");
  });

  it("sanitizes script injection", () => {
    const html = renderMarkdown('hello <script>alert(1)</script> <img src=x onerror="alert(1)">');
    expect(html).not.toContain("<script");
    expect(html).not.toContain("onerror");
  });
});
