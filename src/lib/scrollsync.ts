/**
 * Fractional scroll synchronization between the editor and the preview in
 * split view. Panes register their scrollable element; when one scrolls, the
 * others follow proportionally. Programmatic scrolls mark the receiving pane
 * as "driven" briefly so the echo doesn't bounce back.
 */

const panes = new Map<string, HTMLElement>();
let enabledFn: () => boolean = () => false;
let driven: { name: string; until: number } | null = null;

export function configureSync(enabled: () => boolean) {
  enabledFn = enabled;
}

/** Suppress syncing briefly (e.g. while both panes are scrolled explicitly). */
export function lockSync(ms = 400) {
  driven = { name: "*", until: Date.now() + ms };
}

export function registerScroller(name: string, el: HTMLElement): () => void {
  panes.set(name, el);
  const onScroll = () => {
    if (!enabledFn()) return;
    if (driven && Date.now() < driven.until && (driven.name === name || driven.name === "*")) {
      return;
    }
    const max = el.scrollHeight - el.clientHeight;
    const frac = max > 0 ? el.scrollTop / max : 0;
    for (const [otherName, other] of panes) {
      if (otherName === name || !other.isConnected) continue;
      const otherMax = other.scrollHeight - other.clientHeight;
      const target = frac * otherMax;
      if (Math.abs(other.scrollTop - target) > 1) {
        driven = { name: otherName, until: Date.now() + 150 };
        other.scrollTop = target;
      }
    }
  };
  el.addEventListener("scroll", onScroll, { passive: true });
  return () => {
    el.removeEventListener("scroll", onScroll);
    if (panes.get(name) === el) panes.delete(name);
  };
}
