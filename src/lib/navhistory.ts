/**
 * Browser-style back/forward history across documents. Plain data so it can
 * live inside Svelte `$state`; every function mutates `h` in place.
 */

export interface NavEntry {
  path: string;
  /** Scroll offset of the reading pane when the document was left. */
  scroll: number;
}

export interface NavHistory {
  back: NavEntry[];
  forward: NavEntry[];
}

const CAP = 100;

export function emptyHistory(): NavHistory {
  return { back: [], forward: [] };
}

/** Leaving `from` for a newly opened document. */
export function recordVisit(h: NavHistory, from: NavEntry) {
  const top = h.back[h.back.length - 1];
  if (top?.path === from.path) h.back[h.back.length - 1] = from;
  else h.back.push(from);
  if (h.back.length > CAP) h.back.splice(0, h.back.length - CAP);
  h.forward.length = 0;
}

/**
 * Step back from `current` (null when no document is open, e.g. right after
 * deleting it); returns where to go, or null at the start.
 */
export function goBack(h: NavHistory, current: NavEntry | null): NavEntry | null {
  const target = h.back.pop();
  if (!target) return null;
  if (current) h.forward.push(current);
  return target;
}

export function goForward(h: NavHistory, current: NavEntry | null): NavEntry | null {
  const target = h.forward.pop();
  if (!target) return null;
  if (current) h.back.push(current);
  return target;
}

const within = (path: string, prefix: string) =>
  path === prefix || path.startsWith(prefix + "/");

/** A file or folder moved/renamed: rewrite matching entries. */
export function renamePath(h: NavHistory, from: string, to: string) {
  for (const e of [...h.back, ...h.forward]) {
    if (within(e.path, from)) e.path = to + e.path.slice(from.length);
  }
}

/** A file or folder was deleted: drop its entries (and resulting repeats). */
export function forgetPath(h: NavHistory, path: string) {
  const prune = (stack: NavEntry[]) =>
    stack
      .filter((e) => !within(e.path, path))
      .filter((e, i, kept) => kept[i - 1]?.path !== e.path);
  h.back = prune(h.back);
  h.forward = prune(h.forward);
}
