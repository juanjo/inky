import { Marked } from "marked";
import { markedHighlight } from "marked-highlight";
import markedKatex from "marked-katex-extension";
import markedFootnote from "marked-footnote";
import hljs from "highlight.js/lib/common";
import DOMPurify from "dompurify";
import type { ThemeName } from "./types";

const marked = new Marked(
  markedHighlight({
    emptyLangClass: "hljs",
    langPrefix: "hljs language-",
    highlight(code, lang) {
      if (lang === "mermaid") return code;
      const language = hljs.getLanguage(lang) ? lang : "plaintext";
      return hljs.highlight(code, { language }).value;
    },
  }),
  {
    gfm: true,
    breaks: false,
  },
);

marked.use(markedKatex({ throwOnError: false, nonStandard: true }));
marked.use(markedFootnote());

let slugCounts = new Map<string, number>();

function slugify(text: string, counts: Map<string, number> = slugCounts): string {
  const base =
    text
      .toLowerCase()
      .trim()
      .replace(/<[^>]*>/g, "")
      .replace(/[^\p{L}\p{N}\s-]/gu, "")
      .replace(/\s+/g, "-")
      .slice(0, 80) || "section";
  const count = counts.get(base) ?? 0;
  counts.set(base, count + 1);
  return count === 0 ? base : `${base}-${count}`;
}

export interface TocEntry {
  id: string;
  text: string;
  level: number;
  /** 1-based source line of the heading. */
  line: number;
}

/** Strip inline markdown for display in the TOC. */
function cleanInline(text: string): string {
  return text
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_~`]/g, "")
    .replace(/<[^>]*>/g, "")
    .trim();
}

/**
 * Extract ATX headings from markdown source, skipping fenced code blocks.
 * IDs use the same slug algorithm (and encounter order) as the renderer, so
 * they match the elements in the rendered preview.
 */
export function extractToc(src: string): TocEntry[] {
  const counts = new Map<string, number>();
  const out: TocEntry[] = [];
  let fence: string | null = null;
  src.split("\n").forEach((line, i) => {
    const f = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (f) {
      if (fence === null) fence = f[1][0].repeat(3);
      else if (f[1].startsWith(fence)) fence = null;
      return;
    }
    if (fence !== null) return;
    const m = line.match(/^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (m) {
      out.push({
        id: slugify(m[2], counts),
        text: cleanInline(m[2]),
        level: m[1].length,
        line: i + 1,
      });
    }
  });
  return out;
}

export interface SourceBlock {
  /** Char offsets into the source, end exclusive (no trailing blank lines). */
  start: number;
  end: number;
}

/**
 * Split markdown source into top-level blocks (fence-aware, blank-line
 * separated, loose lists merged). Used for in-place block editing in the
 * preview; alignment with rendered elements is validated separately, so this
 * only needs to be right for common structures.
 */
export function extractBlocks(src: string): SourceBlock[] {
  const lines = src.split("\n");
  const offsets: number[] = [];
  let acc = 0;
  for (const line of lines) {
    offsets.push(acc);
    acc += line.length + 1;
  }
  const lineEnd = (i: number) => offsets[i] + lines[i].length;

  interface Seg {
    startLine: number;
    endLine: number;
  }
  const segments: Seg[] = [];
  let current: Seg | null = null;
  let fence: string | null = null;
  lines.forEach((line, i) => {
    if (fence !== null) {
      current!.endLine = i;
      const f = line.match(/^\s{0,3}(`{3,}|~{3,})\s*$/);
      if (f && f[1].startsWith(fence)) fence = null;
      return;
    }
    if (line.trim() === "") {
      current = null;
      return;
    }
    const f = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (!current) {
      current = { startLine: i, endLine: i };
      segments.push(current);
    } else {
      current.endLine = i;
    }
    if (f) fence = f[1][0].repeat(3);
  });

  // Merge loose lists / indented continuations into one block, matching how
  // markdown renders them as a single element.
  const isListLine = (l: string) => /^\s{0,3}([-*+]|\d+[.)])\s/.test(l);
  const merged: Seg[] = [];
  for (const seg of segments) {
    const prev = merged.at(-1);
    const first = lines[seg.startLine];
    if (
      prev &&
      (isListLine(first) || /^\s{2,}\S/.test(first)) &&
      (isListLine(lines[prev.startLine]) || isListLine(lines[prev.endLine]))
    ) {
      prev.endLine = seg.endLine;
    } else {
      merged.push({ ...seg });
    }
  }
  return merged.map((s) => ({ start: offsets[s.startLine], end: lineEnd(s.endLine) }));
}

marked.use({
  renderer: {
    heading({ tokens, depth, text }) {
      const id = slugify(text);
      return `<h${depth} id="${id}">${this.parser.parseInline(tokens)}</h${depth}>\n`;
    },
    code({ text, lang }) {
      if (lang === "mermaid") {
        return `<div class="mermaid-block" data-mermaid="${encodeURIComponent(text)}"></div>\n`;
      }
      return false;
    },
    checkbox({ checked }) {
      // Enabled (marked emits `disabled`) so reading mode can toggle tasks.
      return `<input type="checkbox" class="task-checkbox"${checked ? " checked" : ""}> `;
    },
    listitem(item) {
      if (item.task) {
        // The checkbox itself is already among the item tokens.
        const body = this.parser.parse(item.tokens).replace(/^<p>|<\/p>\n?$/g, "");
        return `<li class="task-item">${body}</li>\n`;
      }
      return false;
    },
  },
});

export function renderMarkdown(src: string): string {
  slugCounts = new Map();
  const html = marked.parse(src, { async: false }) as string;
  return DOMPurify.sanitize(html, {
    ADD_ATTR: ["data-mermaid"],
    // Without this, heading ids that collide with document/window properties
    // ("title", "location", "history", …) get stripped as DOM clobbering,
    // silently breaking TOC anchors. We render into a local app view and never
    // resolve globals through the DOM, so the clobbering vector doesn't apply.
    SANITIZE_DOM: false,
  });
}

let mermaidTheme: string | null = null;
let renderCounter = 0;
// Mermaid is ~2MB; load it only when a document actually contains a diagram.
type Mermaid = typeof import("mermaid")["default"];
let mermaidPromise: Promise<Mermaid> | null = null;

function loadMermaid(): Promise<Mermaid> {
  return (mermaidPromise ??= import("mermaid").then((m) => m.default));
}

function configureMermaid(mermaid: Mermaid, theme: ThemeName) {
  const mTheme = theme === "dark" ? "dark" : theme === "book" ? "neutral" : "default";
  if (mermaidTheme === mTheme) return;
  mermaidTheme = mTheme;
  mermaid.initialize({
    startOnLoad: false,
    theme: mTheme as "dark" | "neutral" | "default",
    securityLevel: "strict",
    fontFamily: "ui-sans-serif, -apple-system, sans-serif",
  });
}

/** Render every mermaid placeholder inside `container` into inline SVG. */
export async function renderMermaidBlocks(container: HTMLElement, theme: ThemeName) {
  const blocks = container.querySelectorAll<HTMLElement>(".mermaid-block[data-mermaid]");
  if (blocks.length === 0) return;
  const mermaid = await loadMermaid();
  configureMermaid(mermaid, theme);
  for (const block of blocks) {
    const src = decodeURIComponent(block.dataset.mermaid ?? "");
    try {
      const id = `mermaid-${++renderCounter}`;
      const { svg } = await mermaid.render(id, src);
      block.innerHTML = svg;
    } catch (err) {
      // Mermaid leaves an orphaned error element behind on failure; clean it up.
      document.getElementById(`dmermaid-${renderCounter}`)?.remove();
      const pre = document.createElement("pre");
      pre.className = "mermaid-error";
      pre.textContent = `Mermaid error:\n${err instanceof Error ? err.message : String(err)}`;
      block.replaceChildren(pre);
    }
  }
}

/** Re-render on theme change requires resetting mermaid's cached theme. */
export function resetMermaidTheme() {
  mermaidTheme = null;
}
