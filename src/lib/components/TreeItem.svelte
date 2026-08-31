<script lang="ts">
  import type { TreeNode } from "$lib/types";
  import { app } from "$lib/state.svelte";
  import * as ContextMenu from "$lib/components/ui/context-menu/index.js";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Folder from "@lucide/svelte/icons/folder";
  import FileText from "@lucide/svelte/icons/file-text";
  import Workflow from "@lucide/svelte/icons/workflow";
  import TreeItem from "./TreeItem.svelte";

  interface Props {
    node: TreeNode;
    depth: number;
    onrequestrename: (node: TreeNode) => void;
    onrequestdelete: (node: TreeNode) => void;
    onrequestnewfolder: (dir: string) => void;
  }
  let { node, depth, onrequestrename, onrequestdelete, onrequestnewfolder }: Props = $props();

  // svelte-ignore state_referenced_locally -- only the initial depth matters here
  let expanded = $state(depth === 0);
  let dropHover = $state(false);
  let menuOpen = $state(false);
  const isActive = $derived(app.currentPath === node.path);
  const isMermaid = $derived(node.name.toLowerCase().endsWith(".mmd"));

  const DRAG_TYPE = "application/x-inky-path";

  function activate() {
    if (node.isDir) {
      expanded = !expanded;
    } else {
      app.openDoc(node.path);
    }
  }

  function ondragstart(e: DragEvent) {
    e.dataTransfer?.setData(DRAG_TYPE, node.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function ondragover(e: DragEvent) {
    if (!node.isDir || !e.dataTransfer?.types.includes(DRAG_TYPE)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "move";
    dropHover = true;
  }

  function ondrop(e: DragEvent) {
    dropHover = false;
    if (!node.isDir) return;
    const src = e.dataTransfer?.getData(DRAG_TYPE);
    if (!src || src === node.path) return;
    e.preventDefault();
    e.stopPropagation();
    expanded = true;
    app.movePath(src, node.path);
  }

  async function revealInFinder() {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    revealItemInDir(node.path);
  }
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="group relative"
      draggable="true"
      {ondragstart}
      {ondragover}
      ondragleave={() => (dropHover = false)}
      {ondrop}
    >
      <button
        class="flex w-full items-center gap-1.5 rounded-md px-2 py-1 pr-7 text-left text-sm transition-colors
          {isActive ? 'bg-accent text-accent-foreground font-medium' : 'hover:bg-accent/60'}
          {dropHover ? 'ring-2 ring-ring ring-inset bg-accent/40' : ''}"
        style="padding-left: {8 + depth * 14}px"
        onclick={activate}
        title={node.name}
      >
        {#if node.isDir}
          <ChevronRight
            class="size-3.5 shrink-0 transition-transform {expanded ? 'rotate-90' : ''}"
          />
          <Folder class="size-4 shrink-0 opacity-70" />
        {:else if isMermaid}
          <Workflow class="ml-[18px] size-4 shrink-0 opacity-70" />
        {:else}
          <FileText class="ml-[18px] size-4 shrink-0 opacity-70" />
        {/if}
        <span class="truncate">{node.name.replace(/\.(md|markdown)$/i, "")}</span>
      </button>

      <DropdownMenu.Root bind:open={menuOpen}>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon"
              class="absolute top-1/2 right-1 size-5 -translate-y-1/2 opacity-0 group-hover:opacity-100
                {menuOpen ? 'opacity-100' : ''}"
              title="Actions"
            >
              <Ellipsis class="size-3.5" />
            </Button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="start" class="w-52">
          {#if node.isDir}
            <DropdownMenu.Item onclick={() => app.newDoc(node.path)}>New document</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => app.newDoc(node.path, "mermaid")}>
              New Mermaid diagram
            </DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onrequestnewfolder(node.path)}>
              New folder
            </DropdownMenu.Item>
            <DropdownMenu.Separator />
          {/if}
          <DropdownMenu.Item onclick={() => onrequestrename(node)}>Rename…</DropdownMenu.Item>
          <DropdownMenu.Item onclick={revealInFinder}>Reveal in Finder</DropdownMenu.Item>
          <DropdownMenu.Separator />
          <DropdownMenu.Item variant="destructive" onclick={() => onrequestdelete(node)}>
            Move to Trash
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>
  </ContextMenu.Trigger>
  <ContextMenu.Content class="w-52">
    {#if node.isDir}
      <ContextMenu.Item onclick={() => app.newDoc(node.path)}>New document</ContextMenu.Item>
      <ContextMenu.Item onclick={() => app.newDoc(node.path, "mermaid")}>
        New Mermaid diagram
      </ContextMenu.Item>
      <ContextMenu.Item onclick={() => onrequestnewfolder(node.path)}>New folder</ContextMenu.Item>
      <ContextMenu.Separator />
    {/if}
    <ContextMenu.Item onclick={() => onrequestrename(node)}>Rename…</ContextMenu.Item>
    <ContextMenu.Item onclick={revealInFinder}>Reveal in Finder</ContextMenu.Item>
    <ContextMenu.Separator />
    <ContextMenu.Item variant="destructive" onclick={() => onrequestdelete(node)}>
      Move to Trash
    </ContextMenu.Item>
  </ContextMenu.Content>
</ContextMenu.Root>

{#if node.isDir && expanded}
  {#each node.children as child (child.path)}
    <TreeItem
      node={child}
      depth={depth + 1}
      {onrequestrename}
      {onrequestdelete}
      {onrequestnewfolder}
    />
  {/each}
{/if}
