<script lang="ts">
  import { app } from "$lib/state.svelte";
  import type { TreeNode } from "$lib/types";
  import TreeItem from "./TreeItem.svelte";
  import NameDialog from "./NameDialog.svelte";
  import { Button } from "$lib/components/ui/button/index.js";
  import * as AlertDialog from "$lib/components/ui/alert-dialog/index.js";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import { ScrollArea } from "$lib/components/ui/scroll-area/index.js";
  import FilePlus from "@lucide/svelte/icons/file-plus-2";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Workflow from "@lucide/svelte/icons/workflow";
  import Plus from "@lucide/svelte/icons/plus";

  let renameTarget = $state<TreeNode | null>(null);
  let deleteTarget = $state<TreeNode | null>(null);
  let newFolderDir = $state<string | null>(null);
  let rootDropHover = $state(false);

  const DRAG_TYPE = "application/x-inky-path";

  function onRootDragover(e: DragEvent) {
    if (!e.dataTransfer?.types.includes(DRAG_TYPE)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    rootDropHover = true;
  }

  function onRootDrop(e: DragEvent) {
    rootDropHover = false;
    const src = e.dataTransfer?.getData(DRAG_TYPE);
    if (!src) return;
    e.preventDefault();
    app.movePath(src, app.libraryRoot);
  }

  function stripExt(name: string) {
    return name.replace(/\.(md|markdown|mmd)$/i, "");
  }

  function extOf(name: string) {
    return name.match(/\.(md|markdown|mmd)$/i)?.[0] ?? "";
  }
</script>

<aside class="flex h-full flex-col bg-sidebar text-sidebar-foreground border-r border-sidebar-border">
  <div class="flex items-center justify-between px-3 pt-3 pb-1">
    <span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Library</span>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon" class="size-6" title="New…">
            <Plus class="size-4" />
          </Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end" class="w-52">
        <DropdownMenu.Item onclick={() => app.newDoc()}>
          <FilePlus class="size-4" /> New document
        </DropdownMenu.Item>
        <DropdownMenu.Item onclick={() => app.newDoc(undefined, "mermaid")}>
          <Workflow class="size-4" /> New Mermaid diagram
        </DropdownMenu.Item>
        <DropdownMenu.Item onclick={() => (newFolderDir = app.libraryRoot)}>
          <FolderPlus class="size-4" /> New folder
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <ScrollArea
    class="min-h-0 flex-1 {rootDropHover ? 'bg-accent/30' : ''}"
    ondragover={onRootDragover}
    ondragleave={() => (rootDropHover = false)}
    ondrop={onRootDrop}
  >
    <div class="min-h-full px-2 pb-4">
      {#each app.tree as node (node.path)}
        <TreeItem
          {node}
          depth={0}
          onrequestrename={(n) => (renameTarget = n)}
          onrequestdelete={(n) => (deleteTarget = n)}
          onrequestnewfolder={(dir) => (newFolderDir = dir)}
        />
      {:else}
        <p class="px-2 py-6 text-center text-xs text-muted-foreground">
          No documents yet.<br />Create one or paste from the clipboard.
        </p>
      {/each}
    </div>
  </ScrollArea>
</aside>

<NameDialog
  open={renameTarget !== null}
  title="Rename"
  label="New name"
  initial={renameTarget ? stripExt(renameTarget.name) : ""}
  action="Rename"
  onsubmit={(name) => {
    if (renameTarget) {
      const suffix = renameTarget.isDir ? "" : extOf(renameTarget.name);
      app.renamePath(renameTarget.path, name + suffix);
    }
    renameTarget = null;
  }}
  oncancel={() => (renameTarget = null)}
/>

<NameDialog
  open={newFolderDir !== null}
  title="New folder"
  label="Folder name"
  initial=""
  action="Create"
  onsubmit={(name) => {
    if (newFolderDir) app.newFolder(newFolderDir, name);
    newFolderDir = null;
  }}
  oncancel={() => (newFolderDir = null)}
/>

<AlertDialog.Root open={deleteTarget !== null} onOpenChange={(o) => !o && (deleteTarget = null)}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Move “{deleteTarget?.name}” to Trash?</AlertDialog.Title>
      <AlertDialog.Description>
        {deleteTarget?.isDir
          ? "The folder and everything inside it will be moved to the Trash."
          : "The document will be moved to the Trash."}
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action
        onclick={() => {
          if (deleteTarget) app.deletePath(deleteTarget.path);
          deleteTarget = null;
        }}
      >
        Move to Trash
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
