/**
 * Comment threads anchored to text quotes (W3C-annotation style): each thread
 * stores the quoted text plus ~30 chars of surrounding context, so anchors
 * survive edits elsewhere in the document. A quote that disappears leaves the
 * thread "orphaned" but intact.
 */

export interface CommentMsg {
  id: string;
  text: string;
  createdAt: string;
}

export interface CommentThread {
  id: string;
  quote: string;
  prefix: string;
  suffix: string;
  resolved: boolean;
  createdAt: string;
  comments: CommentMsg[];
}

export const CONTEXT_CHARS = 30;

export function newId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Best occurrence of `quote` in `text`, disambiguated by context. */
export function locateQuote(
  text: string,
  quote: string,
  prefix: string,
  suffix: string,
): number {
  if (!quote) return -1;
  let best = -1;
  let bestScore = -1;
  let i = 0;
  while ((i = text.indexOf(quote, i)) !== -1) {
    const before = text.slice(Math.max(0, i - prefix.length), i);
    const after = text.slice(i + quote.length, i + quote.length + suffix.length);
    let score = 0;
    if (prefix) {
      if (before === prefix) score += 2;
      else if (before.endsWith(prefix.slice(-10))) score += 1;
    }
    if (suffix) {
      if (after === suffix) score += 2;
      else if (after.startsWith(suffix.slice(0, 10))) score += 1;
    }
    if (score > bestScore) {
      bestScore = score;
      best = i;
    }
    i += 1;
  }
  return best;
}

interface TextIndexEntry {
  node: Text;
  start: number;
}

function buildTextIndex(container: HTMLElement): { nodes: TextIndexEntry[]; full: string } {
  const nodes: TextIndexEntry[] = [];
  let full = "";
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  let n: Node | null;
  while ((n = walker.nextNode())) {
    const t = n as Text;
    if (t.parentElement?.closest("svg")) continue;
    nodes.push({ node: t, start: full.length });
    full += t.nodeValue ?? "";
  }
  return { nodes, full };
}

/** Context (prefix/suffix) around a DOM selection range within `container`. */
export function contextAround(
  container: HTMLElement,
  range: Range,
): { prefix: string; suffix: string } {
  const before = document.createRange();
  before.selectNodeContents(container);
  before.setEnd(range.startContainer, range.startOffset);
  const after = document.createRange();
  after.selectNodeContents(container);
  after.setStart(range.endContainer, range.endOffset);
  return {
    prefix: before.toString().slice(-CONTEXT_CHARS),
    suffix: after.toString().slice(0, CONTEXT_CHARS),
  };
}

/**
 * Wrap `thread`'s quote in <mark class="comment-hl"> elements (one per
 * intersected text node). Returns the flat-text index or -1 if not found.
 */
export function wrapQuote(container: HTMLElement, thread: CommentThread): number {
  const { nodes, full } = buildTextIndex(container);
  const idx = locateQuote(full, thread.quote, thread.prefix, thread.suffix);
  if (idx < 0) return -1;
  const end = idx + thread.quote.length;
  // Wrap segments back-to-front so earlier offsets stay valid.
  for (let k = nodes.length - 1; k >= 0; k--) {
    const { node, start } = nodes[k];
    const len = node.nodeValue?.length ?? 0;
    const s = Math.max(idx, start);
    const e = Math.min(end, start + len);
    if (s >= e) continue;
    const range = document.createRange();
    range.setStart(node, s - start);
    range.setEnd(node, e - start);
    const mark = document.createElement("mark");
    mark.className = "comment-hl";
    mark.dataset.threadId = thread.id;
    try {
      range.surroundContents(mark);
    } catch {
      // Overlapping with another wrap; skip this segment.
    }
  }
  return idx;
}

export function unwrapMarks(container: HTMLElement, selector: string) {
  for (const m of container.querySelectorAll(selector)) {
    const parent = m.parentNode;
    if (!parent) continue;
    while (m.firstChild) parent.insertBefore(m.firstChild, m);
    parent.removeChild(m);
    parent.normalize();
  }
}

export function relativeTime(iso: string): string {
  const seconds = (Date.now() - new Date(iso).getTime()) / 1000;
  if (seconds < 60) return "just now";
  const minutes = seconds / 60;
  if (minutes < 60) return `${Math.floor(minutes)}m ago`;
  const hours = minutes / 60;
  if (hours < 24) return `${Math.floor(hours)}h ago`;
  const days = hours / 24;
  if (days < 7) return `${Math.floor(days)}d ago`;
  return new Date(iso).toLocaleDateString();
}
