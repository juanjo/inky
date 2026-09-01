<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { app, type CommentDraft } from "$lib/state.svelte";
  import {
    renderMarkdown,
    renderMermaidBlocks,
    resetMermaidTheme,
    extractBlocks,
    type SourceBlock,
  } from "$lib/markdown";
  import { registerScroller } from "$lib/scrollsync";
  import { wrapQuote, unwrapMarks, contextAround } from "$lib/comments";
  import { htmlToMarkdown } from "$lib/richedit";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import Code from "@lucide/svelte/icons/code";
  import Heading1 from "@lucide/svelte/icons/heading-1";
  import Heading2 from "@lucide/svelte/icons/heading-2";
  import Heading3 from "@lucide/svelte/icons/heading-3";
  import List from "@lucide/svelte/icons/list";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import TextQuote from "@lucide/svelte/icons/text-quote";
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
    // Hold re-renders while a block is being edited in place.
    if (blockEditing) return;
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
      // Remember the markdown path so rich edits write it back unchanged.
      img.dataset.mdSrc = src;
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
        computeBlockMap();
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

  function onMouseUp(e: MouseEvent) {
    if (e.target instanceof HTMLTextAreaElement) return;
    setTimeout(() => {
      const sel = window.getSelection();
      if (
        app.isMermaidDoc ||
        blockEditing ||
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

  // --- in-place block editing (reading mode) --------------------------------
  let blockEditing = $state(false);
  let blockMap = new WeakMap<Element, SourceBlock>();

  function normalizeText(s: string): string {
    return s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
  }

  function stripMdSyntax(s: string): string {
    return s
      .replace(/^(`{3,}|~{3,}).*$/gm, "")
      .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/^\s{0,3}#{1,6}\s+/gm, "")
      .replace(/^\s{0,3}>\s?/gm, "")
      .replace(/^\s*([-*+]|\d+[.)])\s+\[[ xX]\]\s+/gm, "")
      .replace(/^\s*([-*+]|\d+[.)])\s+/gm, "")
      .replace(/[*_~`|]/g, "");
  }

  /**
   * Align rendered top-level elements with source blocks, keeping only
   * pairs whose text plausibly matches — mismatches (footnote sections,
   * math, mermaid, reference defs) simply don't get an edit affordance.
   */
  function computeBlockMap() {
    blockMap = new WeakMap();
    if (!container || app.isMermaidDoc) return;
    const blocks = extractBlocks(app.content);
    let bi = 0;
    for (const el of container.children) {
      const elNorm = normalizeText(el.textContent ?? "").slice(0, 32);
      if (!elNorm) continue;
      for (let j = bi; j < Math.min(bi + 3, blocks.length); j++) {
        const raw = app.content.slice(blocks[j].start, blocks[j].end);
        const bNorm = normalizeText(stripMdSyntax(raw)).slice(0, 32);
        if (!bNorm) continue;
        if (bNorm.startsWith(elNorm.slice(0, 16)) || elNorm.startsWith(bNorm.slice(0, 16))) {
          blockMap.set(el, blocks[j]);
          bi = j + 1;
          break;
        }
      }
    }
  }

  /** A plain click on a mapped block starts editing it in place. */
  function maybeStartBlockEdit(e: MouseEvent): boolean {
    if (app.viewMode !== "preview" || blockEditing || !container) return false;
    const target = e.target as HTMLElement;
    // Leave links, comment highlights, form controls and diagrams alone.
    if (target.closest("a, mark.comment-hl, input, textarea, svg, .mermaid-block, .rich-edit"))
      return false;
    if (!window.getSelection()?.isCollapsed) return false;
    let el = target as HTMLElement | null;
    while (el && el.parentElement !== container) el = el.parentElement;
    if (!el) return false;
    const block = blockMap.get(el);
    if (!block) return false;
    // Blocks whose rendering can't round-trip through HTML→markdown (code
    // fences, math, footnote refs) get the plain source editor instead.
    if (el.tagName === "PRE" || el.querySelector(".katex, sup a")) {
      startRawBlockEdit(el, block);
    } else {
      startRichBlockEdit(el, block, e);
    }
    return true;
  }

  // --- WYSIWYG path: the block stays rendered and becomes contentEditable ---
  let richWrapper: HTMLDivElement | null = null;
  let richOriginalHtml = "";
  let richBlock: SourceBlock | null = null;
  let richBar = $state<{ x: number; y: number } | null>(null);

  function startRichBlockEdit(el: HTMLElement, block: SourceBlock, e: MouseEvent) {
    blockEditing = true;
    richBlock = block;
    richOriginalHtml = el.outerHTML;
    const wrapper = document.createElement("div");
    wrapper.className = "rich-edit";
    wrapper.contentEditable = "true";
    wrapper.spellcheck = true;
    el.replaceWith(wrapper);
    wrapper.appendChild(el);
    richWrapper = wrapper;

    if (scroller) {
      const rect = wrapper.getBoundingClientRect();
      const srect = scroller.getBoundingClientRect();
      richBar = {
        x: Math.max(8, Math.min(rect.left - srect.left, srect.width - 340)),
        y: Math.max(4, rect.top - srect.top + scroller.scrollTop - 40),
      };
    }

    wrapper.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        finishRichEdit(false);
      } else if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
        ev.preventDefault();
        finishRichEdit(true);
      }
      ev.stopPropagation();
    });
    wrapper.addEventListener("focusout", (ev) => {
      if (!wrapper.contains(ev.relatedTarget as Node | null)) finishRichEdit(true);
    });

    const { clientX, clientY } = e;
    requestAnimationFrame(() => {
      wrapper.focus();
      try {
        const point = document.caretRangeFromPoint(clientX, clientY);
        if (point && wrapper.contains(point.startContainer)) {
          const sel = window.getSelection();
          sel?.removeAllRanges();
          sel?.addRange(point);
        }
      } catch {
        // keep default caret
      }
    });
  }

  function finishRichEdit(commit: boolean) {
    const wrapper = richWrapper;
    const block = richBlock;
    if (!wrapper || !block) return;
    richWrapper = null;
    richBlock = null;
    richBar = null;
    blockEditing = false;
    const html = wrapper.innerHTML;
    wrapper.outerHTML = richOriginalHtml;
    if (!commit) return;
    const original = app.content.slice(block.start, block.end);
    const md = htmlToMarkdown(html).trim();
    if (md && md !== original.trim()) {
      app.content = app.content.slice(0, block.start) + md + app.content.slice(block.end);
      app.scheduleAutosave();
    }
  }

  /** Toolbar actions for the rich editor (mousedown is prevented, so the
   *  selection inside the contentEditable block survives the click). */
  function execCmd(command: string, value?: string) {
    document.execCommand(command, false, value);
  }

  function toggleHeading(level: number) {
    const anchor = window.getSelection()?.anchorNode;
    const current =
      anchor instanceof Element
        ? anchor.closest("h1,h2,h3,h4,h5,h6")
        : anchor?.parentElement?.closest("h1,h2,h3,h4,h5,h6");
    const tag = `h${level}`;
    execCmd("formatBlock", current?.tagName.toLowerCase() === tag ? "<p>" : `<${tag}>`);
  }

  function toggleQuote() {
    const anchor = window.getSelection()?.anchorNode;
    const inQuote = (
      anchor instanceof Element ? anchor : anchor?.parentElement
    )?.closest("blockquote");
    if (inQuote) execCmd("outdent");
    else execCmd("formatBlock", "<blockquote>");
  }

  function toggleInlineCode() {
    const sel = window.getSelection();
    if (!sel?.rangeCount) return;
    const anchor = sel.anchorNode;
    const inCode = (anchor instanceof Element ? anchor : anchor?.parentElement)?.closest("code");
    if (inCode?.parentNode) {
      const parent = inCode.parentNode;
      while (inCode.firstChild) parent.insertBefore(inCode.firstChild, inCode);
      parent.removeChild(inCode);
      parent.normalize();
    } else {
      const range = sel.getRangeAt(0);
      if (range.collapsed) return;
      const code = document.createElement("code");
      try {
        range.surroundContents(code);
      } catch {
        // selection crosses element boundaries; skip
      }
    }
  }

  // --- raw source path (code fences, math, footnotes) ------------------------
  function startRawBlockEdit(el: HTMLElement, block: SourceBlock) {
    blockEditing = true;
    const original = app.content.slice(block.start, block.end);
    // For code blocks, edit only the code — keep the fence lines out of view.
    let prefix = "";
    let suffix = "";
    let inner = original;
    if (el.tagName === "PRE") {
      const m = original.match(/^(\s{0,3}(?:`{3,}|~{3,})[^\n]*\n)([\s\S]*?)(\n\s{0,3}(?:`{3,}|~{3,})\s*)$/);
      if (m) {
        prefix = m[1];
        inner = m[2];
        suffix = m[3];
      }
    }

    const ta = document.createElement("textarea");
    ta.value = inner;
    ta.className = "block-edit";
    ta.spellcheck = false;
    el.after(ta);
    el.style.display = "none";
    const resize = () => {
      ta.style.height = "0";
      ta.style.height = ta.scrollHeight + "px";
    };
    ta.addEventListener("input", resize);

    let done = false;
    const finish = (commit: boolean) => {
      if (done) return;
      done = true;
      blockEditing = false;
      const value = prefix + ta.value + suffix;
      ta.remove();
      el.style.display = "";
      if (commit && value !== original) {
        app.content = app.content.slice(0, block.start) + value + app.content.slice(block.end);
        app.scheduleAutosave();
      }
    };
    ta.addEventListener("blur", () => finish(true));
    ta.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        finish(false);
      } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        finish(true);
      }
      e.stopPropagation();
    });
    requestAnimationFrame(() => {
      resize();
      ta.focus();
    });
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
      return;
    }
    maybeStartBlockEdit(event);
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
  {#if richBar}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="no-print absolute z-30 flex items-center gap-0.5 rounded-lg border bg-popover p-1 shadow-md"
      style="left: {richBar.x}px; top: {richBar.y}px"
      onmousedown={(e) => e.preventDefault()}
    >
      {#each [
        { icon: Bold, title: "Bold (⌘B)", action: () => execCmd("bold") },
        { icon: Italic, title: "Italic (⌘I)", action: () => execCmd("italic") },
        { icon: Strikethrough, title: "Strikethrough", action: () => execCmd("strikethrough") },
        { icon: Code, title: "Inline code", action: toggleInlineCode },
      ] as b (b.title)}
        <button
          class="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          title={b.title}
          onclick={b.action}
        >
          <b.icon class="size-3.5" />
        </button>
      {/each}
      <div class="mx-0.5 h-4 w-px bg-border"></div>
      {#each [
        { icon: Heading1, title: "Heading 1", action: () => toggleHeading(1) },
        { icon: Heading2, title: "Heading 2", action: () => toggleHeading(2) },
        { icon: Heading3, title: "Heading 3", action: () => toggleHeading(3) },
      ] as b (b.title)}
        <button
          class="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          title={b.title}
          onclick={b.action}
        >
          <b.icon class="size-3.5" />
        </button>
      {/each}
      <div class="mx-0.5 h-4 w-px bg-border"></div>
      {#each [
        { icon: List, title: "Bullet list", action: () => execCmd("insertUnorderedList") },
        { icon: ListOrdered, title: "Numbered list", action: () => execCmd("insertOrderedList") },
        { icon: TextQuote, title: "Quote", action: toggleQuote },
      ] as b (b.title)}
        <button
          class="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          title={b.title}
          onclick={b.action}
        >
          <b.icon class="size-3.5" />
        </button>
      {/each}
    </div>
  {/if}
</div>
