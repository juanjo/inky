<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { app, type SearchBackend } from "$lib/state.svelte";
  import { Button } from "$lib/components/ui/button/index.js";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";

  let query = $state("");
  let count = $state(0);
  let index = $state(0); // 0-based
  let input: HTMLInputElement | undefined = $state();
  let debounce: ReturnType<typeof setTimeout> | null = null;

  function backend(): SearchBackend | undefined {
    return app.viewMode === "preview"
      ? app.searchBackends.preview
      : (app.searchBackends.editor ?? app.searchBackends.preview);
  }

  function clearAll() {
    app.searchBackends.editor?.clear();
    app.searchBackends.preview?.clear();
  }

  function run() {
    clearAll();
    if (app.viewMode === "split") {
      // Highlight the preview too; navigation is driven by the editor (the
      // rendered text has different match positions, so it only mirrors the
      // highlights, and scroll-sync keeps the panes aligned).
      app.searchBackends.preview?.update(query);
    }
    count = backend()?.update(query) ?? 0;
    index = 0;
    if (count > 0) backend()?.goto(0);
  }

  function step(dir: 1 | -1) {
    if (count === 0) return;
    index = (index + dir + count) % count;
    backend()?.goto(index);
  }

  function close() {
    clearAll();
    query = "";
    count = 0;
    app.searchOpen = false;
  }

  $effect(() => {
    if (app.searchOpen) {
      setTimeout(() => input?.select(), 30);
    }
  });

  // Re-run when the active pane changes or the document is edited/switched.
  $effect(() => {
    app.viewMode;
    app.content;
    app.currentPath;
    if (!app.searchOpen || !query) return;
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(run, 250);
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      step(e.shiftKey ? -1 : 1);
    }
  }
</script>

{#if app.searchOpen}
  <div
    class="no-print absolute top-2 right-3 z-30 flex items-center gap-1 rounded-lg border bg-popover p-1 shadow-md"
    transition:fly={{ y: -12, duration: 180, easing: cubicOut }}
  >
    <input
      bind:this={input}
      bind:value={query}
      oninput={run}
      onkeydown={onKeydown}
      placeholder="Find in document…"
      autocomplete="off"
      spellcheck="false"
      class="h-7 w-52 bg-transparent px-2 text-sm outline-none placeholder:text-muted-foreground"
    />
    <span class="min-w-12 text-center text-xs tabular-nums text-muted-foreground">
      {count > 0 ? `${index + 1}/${count}` : query ? "0/0" : ""}
    </span>
    <Button variant="ghost" size="icon" class="size-6" disabled={count === 0} onclick={() => step(-1)}>
      <ChevronUp class="size-4" />
    </Button>
    <Button variant="ghost" size="icon" class="size-6" disabled={count === 0} onclick={() => step(1)}>
      <ChevronDown class="size-4" />
    </Button>
    <Button variant="ghost" size="icon" class="size-6" onclick={close}>
      <X class="size-4" />
    </Button>
  </div>
{/if}
