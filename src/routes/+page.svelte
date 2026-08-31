<script lang="ts">
  import { onMount } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { listen } from "@tauri-apps/api/event";
  import { app } from "$lib/state.svelte";
  import { configureSync } from "$lib/scrollsync";
  import type { ReadingWidth } from "$lib/state.svelte";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Editor from "$lib/components/Editor.svelte";
  import Preview from "$lib/components/Preview.svelte";
  import TocPanel from "$lib/components/TocPanel.svelte";
  import CommentsPanel from "$lib/components/CommentsPanel.svelte";
  import SearchBar from "$lib/components/SearchBar.svelte";
  import QuickOpen from "$lib/components/QuickOpen.svelte";
  import LibrarySearch from "$lib/components/LibrarySearch.svelte";
  import * as Resizable from "$lib/components/ui/resizable/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import FilePlus from "@lucide/svelte/icons/file-plus-2";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";

  let ready = $state(false);

  function storedWidth(key: string, fallback: number, min: number, max: number): number {
    const v = parseInt(localStorage.getItem(key) ?? "", 10);
    return Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;
  }

  let sidebarWidth = $state(240);
  let tocWidth = $state(256);

  function handleMenu(id: string) {
    const actions: Record<string, () => void> = {
      new_doc: () => app.newDoc(),
      new_diagram: () => app.newDoc(undefined, "mermaid"),
      paste_new: () => app.pasteAsNewDoc(),
      save: () => app.save(),
      export_pdf: () => app.exportPdf(),
      print: () => app.printDoc(),
      find: () => app.openSearch(),
      search_library: () => (app.librarySearchVisible = true),
      view_reading: () => app.setViewMode("preview"),
      view_split: () => app.setViewMode("split"),
      view_writing: () => app.setViewMode("editor"),
      toggle_sidebar: () => app.toggleSidebar(),
      toggle_toc: () => app.toggleToc(),
      toggle_comments: () => app.toggleComments(),
      sync_scroll: () => app.toggleSyncScroll(),
      focus_mode: () => app.toggleFocusMode(),
      quick_open: () => (app.quickOpenVisible = true),
      check_updates: () => app.checkForUpdates(true),
      theme_light: () => app.setTheme("light"),
      theme_dark: () => app.setTheme("dark"),
      theme_book: () => app.setTheme("book"),
      font_plus: () => app.adjustFontScale(1),
      font_minus: () => app.adjustFontScale(-1),
    };
    if (id.startsWith("width_")) {
      app.setReadingWidth(id.slice(6) as ReadingWidth);
      return;
    }
    actions[id]?.();
  }

  onMount(() => {
    sidebarWidth = storedWidth("inky.sidebarWidth", 240, 180, 420);
    tocWidth = storedWidth("inky.tocWidth", 256, 200, 480);
    configureSync(() => app.syncScroll && app.viewMode === "split");
    const unlisten = listen<string>("menu", (e) => handleMenu(e.payload));
    app.init().then(() => (ready = true));
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  /**
   * Column-resize drag for the side panels. `sign` is +1 when dragging right
   * grows the panel (sidebar) and -1 when it shrinks it (TOC).
   */
  function startPanelDrag(
    e: PointerEvent,
    opts: { get: () => number; set: (w: number) => void; sign: 1 | -1; min: number; max: number; storageKey: string },
  ) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = opts.get();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => {
      const w = startW + opts.sign * (ev.clientX - startX);
      opts.set(Math.min(opts.max, Math.max(opts.min, w)));
    };
    const onUp = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      localStorage.setItem(opts.storageKey, String(Math.round(opts.get())));
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!event.metaKey) return;
    const key = event.key.toLowerCase();
    if (key === "s") {
      event.preventDefault();
      app.save();
    } else if (key === "n" && !event.shiftKey) {
      event.preventDefault();
      app.newDoc();
    } else if (key === "p") {
      event.preventDefault();
      app.printDoc();
    } else if (key === "e") {
      event.preventDefault();
      app.exportPdf();
    } else if (key === "v" && event.shiftKey) {
      event.preventDefault();
      app.pasteAsNewDoc();
    } else if (key === "1") {
      event.preventDefault();
      app.setViewMode("preview");
    } else if (key === "2") {
      event.preventDefault();
      app.setViewMode("split");
    } else if (key === "3") {
      event.preventDefault();
      app.setViewMode("editor");
    } else if (key === "\\" || key === "b") {
      event.preventDefault();
      app.toggleSidebar();
    } else if (key === "t") {
      event.preventDefault();
      app.toggleToc();
    } else if (key === "k" && event.shiftKey) {
      event.preventDefault();
      app.librarySearchVisible = true;
    } else if (key === "k") {
      event.preventDefault();
      app.quickOpenVisible = true;
    } else if (key === "c" && event.shiftKey) {
      event.preventDefault();
      app.toggleComments();
    } else if (key === "f" && event.shiftKey) {
      event.preventDefault();
      app.toggleFocusMode();
    } else if (key === "f") {
      event.preventDefault();
      app.openSearch();
    } else if (key === "=" || key === "+") {
      event.preventDefault();
      app.adjustFontScale(1);
    } else if (key === "-") {
      event.preventDefault();
      app.adjustFontScale(-1);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="print-root flex h-screen flex-col overflow-hidden">
  <Toolbar />

  <div class="print-root flex min-h-0 flex-1">
    {#if app.sidebarVisible}
      <div
        class="no-print relative shrink-0"
        style="width: {sidebarWidth}px"
        transition:slide={{ axis: "x", duration: 220, easing: cubicOut }}
      >
        <div class="absolute inset-y-0 left-0" style="width: {sidebarWidth}px">
          <Sidebar />
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="absolute inset-y-0 -right-[3px] z-10 w-1.5 cursor-col-resize transition-colors hover:bg-ring/50 active:bg-ring/70"
          onpointerdown={(e) =>
            startPanelDrag(e, {
              get: () => sidebarWidth,
              set: (w) => (sidebarWidth = w),
              sign: 1,
              min: 180,
              max: 420,
              storageKey: "inky.sidebarWidth",
            })}
        ></div>
      </div>
    {/if}

    <main class="print-root relative min-w-0 flex-1">
      <SearchBar />
      {#if !ready}
        <div class="flex h-full items-center justify-center text-muted-foreground">Loading…</div>
      {:else if !app.currentPath}
        <div class="flex h-full flex-col items-center justify-center gap-4 text-center">
          <h1 class="text-2xl font-semibold tracking-tight">Inky</h1>
          <p class="max-w-xs text-sm text-muted-foreground">
            Open a document from the sidebar, create a new one, or paste markdown from your
            clipboard.
          </p>
          <div class="flex gap-2">
            <Button onclick={() => app.newDoc()}>
              <FilePlus class="size-4" /> New document
            </Button>
            <Button variant="outline" onclick={() => app.pasteAsNewDoc()}>
              <ClipboardPaste class="size-4" /> Paste clipboard
            </Button>
          </div>
        </div>
      {:else if app.viewMode === "split"}
        <Resizable.PaneGroup direction="horizontal" autoSaveId="inky-split">
          <Resizable.Pane defaultSize={50} minSize={25} class="no-print">
            <Editor />
          </Resizable.Pane>
          <Resizable.Handle class="no-print" />
          <Resizable.Pane defaultSize={50} minSize={25} class="print-root">
            {#key app.currentPath}
              <Preview />
            {/key}
          </Resizable.Pane>
        </Resizable.PaneGroup>
      {:else if app.viewMode === "editor"}
        <Editor />
      {:else}
        {#key app.currentPath}
          <Preview />
        {/key}
      {/if}
    </main>

    {#if ready && app.currentPath && app.commentsVisible && !app.isMermaidDoc}
      <div
        class="no-print relative shrink-0"
        style="width: {tocWidth}px"
        transition:slide={{ axis: "x", duration: 220, easing: cubicOut }}
      >
        <div class="absolute inset-y-0 right-0" style="width: {tocWidth}px">
          <CommentsPanel />
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="absolute inset-y-0 -left-[3px] z-10 w-1.5 cursor-col-resize transition-colors hover:bg-ring/50 active:bg-ring/70"
          onpointerdown={(e) =>
            startPanelDrag(e, {
              get: () => tocWidth,
              set: (w) => (tocWidth = w),
              sign: -1,
              min: 200,
              max: 480,
              storageKey: "inky.tocWidth",
            })}
        ></div>
      </div>
    {/if}

    {#if ready && app.currentPath && app.tocVisible && !app.isMermaidDoc}
      <div
        class="no-print relative shrink-0"
        style="width: {tocWidth}px"
        transition:slide={{ axis: "x", duration: 220, easing: cubicOut }}
      >
        <div class="absolute inset-y-0 right-0" style="width: {tocWidth}px">
          <TocPanel />
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="absolute inset-y-0 -left-[3px] z-10 w-1.5 cursor-col-resize transition-colors hover:bg-ring/50 active:bg-ring/70"
          onpointerdown={(e) =>
            startPanelDrag(e, {
              get: () => tocWidth,
              set: (w) => (tocWidth = w),
              sign: -1,
              min: 200,
              max: 480,
              storageKey: "inky.tocWidth",
            })}
        ></div>
      </div>
    {/if}
  </div>
</div>

<QuickOpen />
<LibrarySearch />
