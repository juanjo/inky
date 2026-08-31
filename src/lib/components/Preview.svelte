<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { app, type CommentDraft } from "$lib/state.svelte";
  import { renderMarkdown, renderMermaidBlocks, resetMermaidTheme } from "$lib/markdown";
  import { registerScroller } from "$lib/scrollsync";
  import { wrapQuote, unwrapMarks, contextAround } from "$lib/comments";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import CommentHoverCard from "./CommentHoverCard.svelte";

  let container: HTMLDivElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();
  let html = $state("");
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let firstRender = true;

  function toHtml(src: string, isMermaidDoc: boolean): string {
    if (isMermaidDoc) {
      return `<div class="mermaid-block" data-mermaid="${encodeURIComponent(src)}"></div>`;
    }
    return renderMarkdown(src);
  }

  // Debounced re-render while typing; instant on document switch.
  $effect(() => {
    const src = app.content;
    const isMermaid = app.isMermaidDoc;
    app.currentPath;
    if (firstRender) {
      firstRender = false;
      html = toHtml(src, isMermaid);
      return;
    }
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      html = toHtml(src, isMermaid);
    }, 200);
  });

  // Local images can't load from raw file paths inside the webview; route
  // them through Tauri's asset protocol, resolved relative to the document.
  function resolveLocalImages() {
    if (!container || !app.currentPath) return;
    const dir = app.currentPath.slice(0, app.currentPath.lastIndexOf("/"));
    for (const img of container.querySelectorAll("img")) {
      const src = img.getAttribute("src") ?? "";
      if (!src || /^(https?:|data:|blob:|asset:|\/\/)/.test(src)) continue;
      const abs = src.startsWith("/") ? src : `${dir}/${src.replace(/^\.\//, "")}`;
      img.src = convertFileSrc(abs);
    }
  }

  // --- comment highlights --------------------------------------------------
  function applyCommentHighlights() {
    if (!container) return;
    unwrapMarks(container, "mark.comment-hl");
    const order: Record<string, number> = {};
    for (const t of app.commentThreads) {
      if (t.resolved) continue;
      const idx = wrapQuote(container, t);
      if (idx >= 0) order[t.id] = idx;
    }
    app.threadOrder = order;
  }

  // Mermaid needs a second pass over the real DOM, and a full redo on theme change.
  $effect(() => {
    html;
    const theme = app.theme;
    app.commentThreads;
    if (!container) return;
    tick().then(async () => {
      if (container) {
        await renderMermaidBlocks(container, theme);
        resolveLocalImages();
        applyCommentHighlights();
      }
    });
  });

  // Emphasize the active thread's highlight.
  $effect(() => {
    const id = app.activeThreadId;
    if (!container) return;
    for (const m of container.querySelectorAll<HTMLElement>("mark.comment-hl")) {
      m.classList.toggle("active", m.dataset.threadId === id);
    }
  });

  // --- select-to-comment ---------------------------------------------------
  let selAction = $state<{ x: number; y: number; draft: CommentDraft } | null>(null);

  function onMouseUp() {
    setTimeout(() => {
      const sel = window.getSelection();
      if (
        app.isMermaidDoc ||
        !sel ||
        sel.isCollapsed ||
        !container ||
        !scroller ||
        !container.contains(sel.getRangeAt(0).commonAncestorContainer)
      ) {
        selAction = null;
        return;
      }
      const range = sel.getRangeAt(0);
      const quote = range.toString();
      if (!quote.trim() || quote.length > 1000) {
        selAction = null;
        return;
      }
      const rect = range.getBoundingClientRect();
      const srect = scroller.getBoundingClientRect();
      selAction = {
        x: Math.max(8, Math.min(rect.right - srect.left, srect.width - 130)),
        y: Math.max(4, rect.top - srect.top + scroller.scrollTop - 38),
        draft: { quote, ...contextAround(container, range) },
      };
    }, 0);
  }

  function createComment() {
    if (!selAction) return;
    app.startCommentDraft(selAction.draft);
    selAction = null;
    window.getSelection()?.removeAllRanges();
  }

  // --- hover preview of comment threads ------------------------------------
  let hoverCard = $state<{ id: string; x: number; y: number } | null>(null);
  const hoverThread = $derived.by(() => {
    const hc = hoverCard;
    return hc ? (app.commentThreads.find((t) => t.id === hc.id) ?? null) : null;
  });

  function onHover(e: MouseEvent) {
    const mark = (e.target as HTMLElement).closest?.<HTMLElement>("mark.comment-hl");
    if (mark?.dataset.threadId && scroller) {
      const rect = mark.getBoundingClientRect();
      const srect = scroller.getBoundingClientRect();
      hoverCard = {
        id: mark.dataset.threadId,
        x: Math.max(8, Math.min(rect.left - srect.left, srect.width - 280)),
        y: rect.bottom - srect.top + scroller.scrollTop + 6,
      };
    } else {
      hoverCard = null;
    }
  }

  $effect(() => {
    app.theme;
    resetMermaidTheme();
    // Force re-render of mermaid blocks with the new theme.
    untrack(() => {
      html = toHtml(app.content, app.isMermaidDoc);
    });
  });

  // --- TOC scroll spy ------------------------------------------------------
  let spyRaf = 0;
  function onScroll(e: Event) {
    const scrollEl = e.currentTarget as HTMLElement;
    if (spyRaf) return;
    spyRaf = requestAnimationFrame(() => {
      spyRaf = 0;
      if (!container) return;
      const threshold = scrollEl.getBoundingClientRect().top + 90;
      let active: string | null = null;
      for (const h of container.querySelectorAll<HTMLElement>("h1, h2, h3, h4, h5, h6")) {
        if (h.getBoundingClientRect().top <= threshold) active = h.id;
        else break;
      }
      app.activeHeadingId = active ?? app.toc[0]?.id ?? null;
    });
  }

  // --- in-document search (wraps matches in <mark>) ------------------------
  let marks: HTMLElement[] = [];

  function searchClear() {
    for (const m of marks) {
      if (!m.isConnected) continue;
      const parent = m.parentNode;
      if (!parent) continue;
      parent.replaceChild(document.createTextNode(m.textContent ?? ""), m);
      parent.normalize();
    }
    marks = [];
  }

  function searchUpdate(query: string): number {
    searchClear();
    if (!container || !query) return 0;
    const q = query.toLowerCase();
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    let n: Node | null;
    while ((n = walker.nextNode())) {
      // Wrapping <mark> inside SVG (mermaid) would corrupt the diagram.
      if (!n.parentElement?.closest("svg, mark.search-hit")) nodes.push(n as Text);
    }
    for (const node of nodes) {
      const text = node.nodeValue ?? "";
      const lower = text.toLowerCase();
      const hits: number[] = [];
      let i = 0;
      while ((i = lower.indexOf(q, i)) !== -1) {
        hits.push(i);
        i += q.length;
      }
      // Wrap from the end so earlier offsets stay valid.
      const nodeMarks: HTMLElement[] = [];
      for (let j = hits.length - 1; j >= 0; j--) {
        const range = document.createRange();
        range.setStart(node, hits[j]);
        range.setEnd(node, hits[j] + q.length);
        const mark = document.createElement("mark");
        mark.className = "search-hit";
        try {
          range.surroundContents(mark);
          nodeMarks.push(mark);
        } catch {
          // Range crossed a boundary; skip this occurrence.
        }
      }
      marks.push(...nodeMarks.reverse());
      if (marks.length > 2000) break;
    }
    return marks.length;
  }

  function searchGoto(i: number) {
    marks.forEach((m, j) => m.classList.toggle("active", j === i));
    marks[i]?.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  onMount(() => {
    app.searchBackends.preview = { update: searchUpdate, goto: searchGoto, clear: searchClear };
    const unregisterSync = scroller ? registerScroller("preview", scroller) : () => {};
    return () => {
      delete app.searchBackends.preview;
      unregisterSync();
    };
  });

  function handleClick(event: MouseEvent) {
    const mark = (event.target as HTMLElement).closest<HTMLElement>("mark.comment-hl");
    if (mark?.dataset.threadId) {
      app.openThread(mark.dataset.threadId);
      return;
    }
    // Open external links in the default browser instead of the webview.
    const anchor = (event.target as HTMLElement).closest("a");
    if (anchor?.href && /^https?:/.test(anchor.href)) {
      event.preventDefault();
      import("@tauri-apps/plugin-opener").then(({ openUrl }) => openUrl(anchor.href));
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events, a11y_mouse_events_have_key_events -->
<div
  bind:this={scroller}
  class="print-scroll relative h-full overflow-y-auto px-8 py-10"
  onclick={handleClick}
  onscroll={onScroll}
  onmouseup={onMouseUp}
  onmouseover={onHover}
  onmouseleave={() => (hoverCard = null)}
>
  <div class="prose-doc" bind:this={container}>
    <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized via DOMPurify -->
    {@html html}
  </div>
  {#if selAction}
    <button
      class="no-print absolute z-20 flex items-center gap-1.5 rounded-full border bg-popover px-3 py-1.5 text-xs font-medium shadow-md transition-colors hover:bg-accent"
      style="left: {selAction.x}px; top: {selAction.y}px"
      onmousedown={(e) => e.preventDefault()}
      onclick={(e) => {
        e.stopPropagation();
        createComment();
      }}
    >
      <MessageSquarePlus class="size-3.5" /> Comment
    </button>
  {/if}
  {#if hoverThread && hoverCard}
    <CommentHoverCard thread={hoverThread} x={hoverCard.x} y={hoverCard.y} />
  {/if}
</div>
