<script lang="ts">
  import { app, FONT_SCALES, READING_WIDTHS, type ReadingWidth } from "$lib/state.svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { Button } from "$lib/components/ui/button/index.js";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import * as Popover from "$lib/components/ui/popover/index.js";
  import * as Tooltip from "$lib/components/ui/tooltip/index.js";
  import { Separator } from "$lib/components/ui/separator/index.js";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import Copy from "@lucide/svelte/icons/copy";
  import FileDown from "@lucide/svelte/icons/file-down";
  import Minus from "@lucide/svelte/icons/minus";
  import Plus from "@lucide/svelte/icons/plus";
  import SunMedium from "@lucide/svelte/icons/sun-medium";
  import Moon from "@lucide/svelte/icons/moon";
  import BookOpenText from "@lucide/svelte/icons/book-open-text";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import TableOfContents from "@lucide/svelte/icons/table-of-contents";
  import MessageSquareText from "@lucide/svelte/icons/message-square-text";
  import History from "@lucide/svelte/icons/history";
  import UnfoldVertical from "@lucide/svelte/icons/unfold-vertical";
  import Search from "@lucide/svelte/icons/search";
  import Folder from "@lucide/svelte/icons/folder";
  import FileText from "@lucide/svelte/icons/file-text";
  import type { TreeNode, ViewMode } from "$lib/types";
  import { breadcrumbs, type Crumb } from "$lib/paths";

  /** Folder trail to the open document; each folder lists its contents. */
  const crumbs = $derived(
    app.currentPath
      ? breadcrumbs(app.currentPath, {
          library: app.libraryRoot,
          workspace: app.workspace,
          home: app.homeDir,
        })
      : [],
  );
  /** Deep trails keep the root and the last two folders; `null` marks the gap. */
  const shownCrumbs = $derived<(Crumb | null)[]>(
    crumbs.length > 3 ? [crumbs[0], null, ...crumbs.slice(-2)] : crumbs,
  );

  function childrenOf(dir: string): TreeNode[] {
    if (dir === app.activeRoot) return app.tree;
    const find = (nodes: TreeNode[]): TreeNode[] | null => {
      for (const n of nodes) {
        if (!n.isDir) continue;
        if (n.path === dir) return n.children;
        if (dir.startsWith(n.path + "/")) return find(n.children);
      }
      return null;
    };
    return find(app.tree) ?? [];
  }

  const viewModes: { mode: ViewMode; label: string; icon: typeof BookOpen }[] = [
    { mode: "preview", label: "Reading (⌘1)", icon: BookOpen },
    { mode: "split", label: "Split (⌘2)", icon: Columns2 },
    { mode: "editor", label: "Writing (⌘3)", icon: PenLine },
  ];

  const ThemeIcon = $derived(
    app.theme === "dark" ? Moon : app.theme === "book" ? BookOpenText : SunMedium,
  );

  let editingTitle = $state(false);
  let titleDraft = $state("");

  function docStem() {
    return app.docName.replace(/\.(md|markdown|mmd)$/i, "");
  }

  function startTitleEdit() {
    if (!app.currentPath) return;
    titleDraft = docStem();
    editingTitle = true;
  }

  function commitTitle() {
    if (!editingTitle) return;
    editingTitle = false;
    app.renameCurrentDoc(titleDraft);
  }

  async function revealLibrary() {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    revealItemInDir(app.activeRoot);
  }
</script>

{#snippet folderItems(nodes: TreeNode[])}
  {#each nodes as node (node.path)}
    {#if node.isDir}
      <DropdownMenu.Sub>
        <DropdownMenu.SubTrigger>
          <Folder class="size-4 opacity-60" />
          <span class="truncate">{node.name}</span>
        </DropdownMenu.SubTrigger>
        <DropdownMenu.SubContent class="max-h-96 w-64 overflow-y-auto">
          {@render folderItems(node.children)}
        </DropdownMenu.SubContent>
      </DropdownMenu.Sub>
    {:else}
      <DropdownMenu.Item
        class={node.path === app.currentPath ? "font-medium" : ""}
        onclick={() => app.openDoc(node.path)}
      >
        <FileText class="size-4 opacity-60" />
        <span class="truncate">{node.name.replace(/\.(md|markdown|mmd)$/i, "")}</span>
      </DropdownMenu.Item>
    {/if}
  {:else}
    <DropdownMenu.Item disabled>No documents</DropdownMenu.Item>
  {/each}
{/snippet}

<!-- pl-[76px] clears the macOS traffic lights (overlay title bar) -->
<header
  class="no-print flex h-12 shrink-0 items-center gap-1 border-b bg-background pr-2 pl-[76px]"
  data-tauri-drag-region
>
  <Tooltip.Provider delayDuration={400}>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" onclick={() => app.toggleSidebar()}>
            <PanelLeft class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Toggle sidebar (⌘B)</Tooltip.Content>
    </Tooltip.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            disabled={!app.canGoBack}
            onclick={() => app.goBack()}
          >
            <ChevronLeft class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Back (⌘[)</Tooltip.Content>
    </Tooltip.Root>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            disabled={!app.canGoForward}
            onclick={() => app.goForward()}
          >
            <ChevronRight class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Forward (⌘])</Tooltip.Content>
    </Tooltip.Root>

    <div class="ml-1 flex min-w-0 items-center gap-2">
      {#if shownCrumbs.length > 0}
        <nav class="flex min-w-0 shrink items-center text-sm text-muted-foreground" aria-label="Breadcrumbs">
          {#each shownCrumbs as crumb (crumb?.dir ?? "gap")}
            {#if crumb === null}
              <span class="px-1" title={crumbs.map((c) => c.label).join(" › ")}>…</span>
            {:else if crumb.browsable}
              <DropdownMenu.Root>
                <DropdownMenu.Trigger>
                  {#snippet child({ props })}
                    <button
                      {...props}
                      class="max-w-40 truncate rounded-sm px-1 hover:bg-accent/60 hover:text-foreground"
                      title={crumb.dir}
                    >
                      {crumb.label}
                    </button>
                  {/snippet}
                </DropdownMenu.Trigger>
                <DropdownMenu.Content align="start" class="max-h-96 w-64 overflow-y-auto">
                  {@render folderItems(childrenOf(crumb.dir))}
                </DropdownMenu.Content>
              </DropdownMenu.Root>
            {:else}
              <span class="max-w-56 truncate px-1" title={app.currentPath}>{crumb.label}</span>
            {/if}
            <span class="shrink-0 text-muted-foreground/60" aria-hidden="true">›</span>
          {/each}
        </nav>
      {/if}
      {#if editingTitle}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          bind:value={titleDraft}
          autofocus
          autocomplete="off"
          spellcheck="false"
          class="w-64 rounded-sm border-b border-ring bg-transparent text-sm font-medium outline-none"
          onkeydown={(e) => {
            if (e.key === "Enter") commitTitle();
            else if (e.key === "Escape") (editingTitle = false);
          }}
          onblur={commitTitle}
        />
      {:else}
        <button
          class="truncate rounded-sm text-sm font-medium {app.currentPath
            ? 'cursor-text hover:bg-accent/60 px-1 -mx-1'
            : ''}"
          title={app.currentPath ? "Click to rename" : undefined}
          onclick={startTitleEdit}
        >
          {app.currentPath ? docStem() : "Inky"}
        </button>
      {/if}
      {#if app.dirty}
        <span class="size-1.5 shrink-0 rounded-full bg-muted-foreground" title="Unsaved changes"></span>
      {/if}
      {#if app.currentPath && !app.isMermaidDoc}
        <span class="ml-1 shrink-0 text-xs text-muted-foreground/80 tabular-nums">
          {app.wordCount.toLocaleString()} words · {app.readingMinutes} min
        </span>
      {/if}
    </div>

    <div class="flex-1" data-tauri-drag-region></div>

    <div class="flex items-center rounded-lg bg-muted p-0.5">
      {#each viewModes as { mode, label, icon: Icon } (mode)}
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon"
                class="size-7 {app.viewMode === mode
                  ? 'bg-background shadow-sm hover:bg-background'
                  : 'hover:bg-transparent opacity-60'}"
                onclick={() => app.setViewMode(mode)}
              >
                <Icon class="size-4" />
              </Button>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content>{label}</Tooltip.Content>
        </Tooltip.Root>
      {/each}
    </div>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            class="ml-1 {app.tocVisible ? 'bg-accent' : ''}"
            disabled={!app.currentPath || app.isMermaidDoc}
            onclick={() => app.toggleToc()}
          >
            <TableOfContents class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Table of contents (⌘T)</Tooltip.Content>
    </Tooltip.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            class="relative {app.commentsVisible ? 'bg-accent' : ''}"
            disabled={!app.currentPath || app.isMermaidDoc || !app.docSidecars}
            onclick={() => app.toggleComments()}
          >
            <MessageSquareText class="size-4" />
            {#if app.openCommentCount > 0}
              <span
                class="absolute -top-0.5 -right-0.5 flex size-4 items-center justify-center rounded-full bg-primary text-[9px] font-semibold text-primary-foreground tabular-nums"
              >
                {app.openCommentCount > 9 ? "9+" : app.openCommentCount}
              </span>
            {/if}
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>
        {app.docSidecars ? "Comments (⌘⇧C)" : "Not available for files outside the library"}
      </Tooltip.Content>
    </Tooltip.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            class={app.historyVisible ? "bg-accent" : ""}
            disabled={!app.currentPath || !app.docSidecars}
            onclick={() => (app.historyVisible = true)}
          >
            <History class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>
        {app.docSidecars ? "Version history (⌘Y)" : "Not available for files outside the library"}
      </Tooltip.Content>
    </Tooltip.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            class={app.syncScroll && app.viewMode === "split" ? "bg-accent" : ""}
            disabled={app.viewMode !== "split"}
            onclick={() => app.toggleSyncScroll()}
          >
            <UnfoldVertical class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>
        {app.viewMode === "split"
          ? `Sync scrolling ${app.syncScroll ? "on" : "off"}`
          : "Sync scrolling (split view only)"}
      </Tooltip.Content>
    </Tooltip.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            disabled={!app.currentPath}
            onclick={() => app.openSearch()}
          >
            <Search class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Find in document (⌘F)</Tooltip.Content>
    </Tooltip.Root>

    <Separator orientation="vertical" class="mx-1 !h-5" />

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" onclick={() => app.pasteAsNewDoc()}>
            <ClipboardPaste class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Paste clipboard as new document (⌘⇧V)</Tooltip.Content>
    </Tooltip.Root>

    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" disabled={!app.currentPath} title="Copy">
            <Copy class="size-4" />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Item onclick={() => app.copyMarkdown()}>Copy Markdown</DropdownMenu.Item>
        <DropdownMenu.Item onclick={() => app.copyHtml(renderMarkdown(app.content))}>
          Copy HTML
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="icon"
            disabled={!app.currentPath}
            onclick={() => app.exportPdf()}
          >
            <FileDown class="size-4" />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Export PDF (⌘E)</Tooltip.Content>
    </Tooltip.Root>

    <Popover.Root>
      <Popover.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" title="Reading settings">
            <span class="text-[15px] font-serif leading-none">Aa</span>
          </Button>
        {/snippet}
      </Popover.Trigger>
      <Popover.Content align="end" class="w-72">
        <div class="grid gap-4">
          <div class="grid gap-2">
            <span class="text-xs font-medium text-muted-foreground">Font size</span>
            <div class="flex items-center gap-2">
              <Button
                variant="outline"
                size="icon"
                class="size-8"
                disabled={app.fontScale <= FONT_SCALES[0]}
                onclick={() => app.adjustFontScale(-1)}
              >
                <Minus class="size-4" />
              </Button>
              <div class="flex-1 text-center text-sm tabular-nums text-muted-foreground">
                {Math.round(app.fontScale * 100)}%
              </div>
              <Button
                variant="outline"
                size="icon"
                class="size-8"
                disabled={app.fontScale >= FONT_SCALES[FONT_SCALES.length - 1]}
                onclick={() => app.adjustFontScale(1)}
              >
                <Plus class="size-4" />
              </Button>
            </div>
          </div>
          <div class="grid gap-2">
            <span class="text-xs font-medium text-muted-foreground">Text width</span>
            <div class="grid grid-cols-5 gap-1">
              {#each Object.entries(READING_WIDTHS) as [key, w] (key)}
                <Button
                  variant={app.readingWidth === key ? "secondary" : "ghost"}
                  size="sm"
                  class="h-7 px-0 text-xs {app.readingWidth === key ? 'font-semibold' : ''}"
                  onclick={() => app.setReadingWidth(key as ReadingWidth)}
                >
                  {w.label}
                </Button>
              {/each}
            </div>
          </div>
        </div>
      </Popover.Content>
    </Popover.Root>

    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" title="Theme">
            <ThemeIcon class="size-4" />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Item onclick={() => app.setTheme("light")}>
          <SunMedium class="size-4" /> Light
        </DropdownMenu.Item>
        <DropdownMenu.Item onclick={() => app.setTheme("dark")}>
          <Moon class="size-4" /> Dark
        </DropdownMenu.Item>
        <DropdownMenu.Item onclick={() => app.setTheme("book")}>
          <BookOpenText class="size-4" /> Book
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="ghost"
            size="sm"
            class="h-7 gap-1.5 px-2 text-xs font-medium text-muted-foreground"
            onclick={() => app.toggleMcpServer()}
            oncontextmenu={(e) => {
              e.preventDefault();
              app.openMcpSetup();
            }}
          >
            <span
              class="size-2 rounded-full {app.mcpUrl
                ? 'bg-green-500 shadow-[0_0_5px_rgb(34_197_94_/_0.8)]'
                : 'bg-red-400/80'}"
            ></span>
            MCP
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>
        {app.mcpUrl
          ? `MCP server live at ${app.mcpUrl} — click to stop, right-click to connect an agent`
          : "MCP server off — click to start, right-click to connect an agent"}
      </Tooltip.Content>
    </Tooltip.Root>

    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" title="More">
            <Ellipsis class="size-4" />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end" class="w-56">
        <DropdownMenu.Item disabled={!app.currentPath} onclick={() => app.printDoc()}>
          Print… <DropdownMenu.Shortcut>⌘P</DropdownMenu.Shortcut>
        </DropdownMenu.Item>
        <DropdownMenu.Item
          disabled={!app.currentPath || !app.docSidecars}
          onclick={() => (app.historyVisible = true)}
        >
          Version history…
        </DropdownMenu.Item>
        <DropdownMenu.Separator />
        <DropdownMenu.Item onclick={() => app.refreshTree()}>Refresh library</DropdownMenu.Item>
        <DropdownMenu.Item onclick={revealLibrary}>Reveal library in Finder</DropdownMenu.Item>
        <DropdownMenu.Separator />
        <DropdownMenu.Item onclick={() => app.chooseLibrary()}>
          Change library folder…
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </Tooltip.Provider>
</header>
