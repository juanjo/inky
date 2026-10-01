<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { app } from "$lib/state.svelte";
  import { displayDir, isInside } from "$lib/paths";
  import FileText from "@lucide/svelte/icons/file-text";
  import Workflow from "@lucide/svelte/icons/workflow";

  let query = $state("");
  let selected = $state(0);
  let input: HTMLInputElement | undefined = $state();

  interface Item {
    name: string;
    path: string;
    /** Subtitle: folder within the root, or ~/dir for outside documents. */
    where: string;
    recent: boolean;
  }

  const items = $derived.by((): Item[] => {
    const seen = new Set<string>();
    const out: Item[] = [];
    for (const path of app.recentDocs) {
      if (path === app.currentPath) continue;
      seen.add(path);
      const inRoot = isInside(path, app.activeRoot);
      const rel = inRoot ? path.slice(app.activeRoot.length + 1) : "";
      out.push({
        name: path.split("/").pop() ?? path,
        path,
        where: inRoot ? rel.slice(0, Math.max(0, rel.lastIndexOf("/"))) : displayDir(path, app.homeDir),
        recent: true,
      });
    }
    for (const d of app.flatDocs) {
      if (seen.has(d.path)) continue;
      out.push({
        name: d.name,
        path: d.path,
        where: d.rel.includes("/") ? d.rel.slice(0, d.rel.lastIndexOf("/")) : "",
        recent: false,
      });
    }
    return out;
  });

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items.slice(0, 12);
    return items
      .map((d) => {
        const name = d.name.toLowerCase();
        let score = 0;
        if (name.startsWith(q)) score = 3;
        else if (name.includes(q)) score = 2;
        else if (`${d.where}/${name}`.toLowerCase().includes(q)) score = 1;
        return { ...d, score };
      })
      .filter((d) => d.score > 0)
      .sort((a, b) => b.score - a.score) // stable: recents keep their lead on ties
      .slice(0, 12);
  });

  $effect(() => {
    if (app.quickOpenVisible) {
      query = "";
      app.refreshRecents();
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
    if (!doc) return;
    if (doc.recent) app.openRecent(doc.path);
    else app.openDoc(doc.path);
    close();
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
          {#if doc.where}
            <span class="ml-auto shrink-0 truncate text-xs text-muted-foreground">{doc.where}</span>
          {/if}
        </button>
      {:else}
        <p class="px-3 py-6 text-center text-sm text-muted-foreground">No matching documents.</p>
      {/each}
    </div>
  </div>
{/if}
