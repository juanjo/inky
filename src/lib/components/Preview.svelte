<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { app } from "$lib/state.svelte";
  import { renderMarkdown, renderMermaidBlocks, resetMermaidTheme } from "$lib/markdown";
  import { registerScroller } from "$lib/scrollsync";

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

  // Mermaid needs a second pass over the real DOM, and a full redo on theme change.
  $effect(() => {
    html;
    const theme = app.theme;
    if (!container) return;
    tick().then(() => {
      if (container) {
        renderMermaidBlocks(container, theme);
        resolveLocalImages();
      }
    });
  });

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
      if (!(n.parentElement?.closest("svg, mark"))) nodes.push(n as Text);
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
    // Open external links in the default browser instead of the webview.
    const anchor = (event.target as HTMLElement).closest("a");
    if (anchor?.href && /^https?:/.test(anchor.href)) {
      event.preventDefault();
      import("@tauri-apps/plugin-opener").then(({ openUrl }) => openUrl(anchor.href));
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div
  bind:this={scroller}
  class="print-scroll h-full overflow-y-auto px-8 py-10"
  onclick={handleClick}
  onscroll={onScroll}
>
  <div class="prose-doc" bind:this={container}>
    <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized via DOMPurify -->
    {@html html}
  </div>
</div>
