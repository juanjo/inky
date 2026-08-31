<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { app } from "$lib/state.svelte";
  import FileText from "@lucide/svelte/icons/file-text";
  import Workflow from "@lucide/svelte/icons/workflow";

  let query = $state("");
  let selected = $state(0);
  let input: HTMLInputElement | undefined = $state();

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const docs = app.flatDocs;
    if (!q) return docs.slice(0, 12);
    return docs
      .map((d) => {
        const name = d.name.toLowerCase();
        const rel = d.rel.toLowerCase();
        let score = 0;
        if (name.startsWith(q)) score = 3;
        else if (name.includes(q)) score = 2;
        else if (rel.includes(q)) score = 1;
        return { ...d, score };
      })
      .filter((d) => d.score > 0)
      .sort((a, b) => b.score - a.score || a.rel.localeCompare(b.rel))
      .slice(0, 12);
  });

  $effect(() => {
    if (app.quickOpenVisible) {
      query = "";
      selected = 0;
      setTimeout(() => input?.focus(), 30);
    }
  });

  $effect(() => {
    results;
    if (selected >= results.length) selected = 0;
  });

  function close() {
    app.quickOpenVisible = false;
  }

  function openSelected() {
    const doc = results[selected];
    if (doc) {
      app.openDoc(doc.path);
      close();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      openSelected();
    }
  }

  function stripExt(name: string) {
    return name.replace(/\.(md|markdown|mmd)$/i, "");
  }
</script>

{#if app.quickOpenVisible}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 z-40 bg-black/30"
    onclick={close}
    transition:fade={{ duration: 120 }}
  ></div>
  <div
    class="fixed top-24 left-1/2 z-50 w-[520px] -translate-x-1/2 overflow-hidden rounded-xl border bg-popover shadow-2xl"
    transition:fly={{ y: -16, duration: 160, easing: cubicOut }}
  >
    <input
      bind:this={input}
      bind:value={query}
      onkeydown={onKeydown}
      placeholder="Open document…"
      autocomplete="off"
      spellcheck="false"
      class="w-full border-b bg-transparent px-4 py-3 text-sm outline-none placeholder:text-muted-foreground"
    />
    <div class="max-h-80 overflow-y-auto p-1.5">
      {#each results as doc, i (doc.path)}
        <button
          class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm
            {i === selected ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60'}"
          onclick={openSelected}
          onmouseenter={() => (selected = i)}
        >
          {#if doc.name.toLowerCase().endsWith(".mmd")}
            <Workflow class="size-4 shrink-0 opacity-60" />
          {:else}
            <FileText class="size-4 shrink-0 opacity-60" />
          {/if}
          <span class="truncate font-medium">{stripExt(doc.name)}</span>
          {#if doc.rel.includes("/")}
            <span class="ml-auto shrink-0 truncate text-xs text-muted-foreground">
              {doc.rel.slice(0, doc.rel.lastIndexOf("/"))}
            </span>
          {/if}
        </button>
      {:else}
        <p class="px-3 py-6 text-center text-sm text-muted-foreground">No matching documents.</p>
      {/each}
    </div>
  </div>
{/if}
