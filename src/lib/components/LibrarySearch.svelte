<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/state.svelte";
  import FileText from "@lucide/svelte/icons/file-text";

  interface Hit {
    path: string;
    name: string;
    line: number;
    text: string;
  }

  let query = $state("");
  let hits = $state<Hit[]>([]);
  let selected = $state(0);
  let searching = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let debounce: ReturnType<typeof setTimeout> | null = null;

  interface Group {
    path: string;
    name: string;
    hits: Hit[];
  }
  const groups = $derived.by(() => {
    const byPath = new Map<string, Group>();
    for (const h of hits) {
      let g = byPath.get(h.path);
      if (!g) {
        g = { path: h.path, name: h.name, hits: [] };
        byPath.set(h.path, g);
      }
      if (g.hits.length < 6) g.hits.push(h);
    }
    return [...byPath.values()];
  });
  const flat = $derived(groups.flatMap((g) => g.hits));

  $effect(() => {
    if (app.librarySearchVisible) {
      selected = 0;
      setTimeout(() => input?.select(), 30);
    }
  });

  function runSearch() {
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(async () => {
      const q = query.trim();
      if (!q) {
        hits = [];
        return;
      }
      searching = true;
      try {
        hits = await invoke<Hit[]>("search_library", { query: q });
        selected = 0;
      } finally {
        searching = false;
      }
    }, 200);
  }

  function close() {
    app.librarySearchVisible = false;
  }

  async function openHit(hit: Hit) {
    close();
    await app.openDoc(hit.path);
    // Hand the query to the in-document search for highlights + navigation.
    app.pendingSearchQuery = query.trim();
    app.searchOpen = true;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, flat.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (flat[selected]) openHit(flat[selected]);
    }
  }

  function stripExt(name: string) {
    return name.replace(/\.(md|markdown|mmd)$/i, "");
  }

  function highlight(text: string): { pre: string; match: string; post: string } | null {
    const i = text.toLowerCase().indexOf(query.trim().toLowerCase());
    if (i < 0) return null;
    const q = query.trim();
    return { pre: text.slice(0, i), match: text.slice(i, i + q.length), post: text.slice(i + q.length) };
  }
</script>

{#if app.librarySearchVisible}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="fixed inset-0 z-40 bg-black/30" onclick={close} transition:fade={{ duration: 120 }}></div>
  <div
    class="fixed top-20 left-1/2 z-50 w-[620px] -translate-x-1/2 overflow-hidden rounded-xl border bg-popover shadow-2xl"
    transition:fly={{ y: -16, duration: 160, easing: cubicOut }}
  >
    <input
      bind:this={input}
      bind:value={query}
      oninput={runSearch}
      onkeydown={onKeydown}
      placeholder="Search all documents…"
      autocomplete="off"
      spellcheck="false"
      class="w-full border-b bg-transparent px-4 py-3 text-sm outline-none placeholder:text-muted-foreground"
    />
    <div class="max-h-[420px] overflow-y-auto p-1.5">
      {#each groups as group (group.path)}
        <div class="mt-1 mb-0.5 flex items-center gap-1.5 px-2.5 pt-1 first:mt-0">
          <FileText class="size-3.5 shrink-0 opacity-60" />
          <span class="truncate text-xs font-semibold">{stripExt(group.name)}</span>
        </div>
        {#each group.hits as hit (hit.path + hit.line)}
          {@const i = flat.indexOf(hit)}
          {@const parts = highlight(hit.text)}
          <button
            class="flex w-full items-baseline gap-2 rounded-md px-2.5 py-1 text-left text-[13px]
              {i === selected ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60'}"
            onclick={() => openHit(hit)}
            onmouseenter={() => (selected = i)}
          >
            <span class="shrink-0 text-[11px] tabular-nums text-muted-foreground">{hit.line}</span>
            <span class="truncate">
              {#if parts}
                {parts.pre}<mark class="rounded-sm bg-yellow-400/40 text-inherit">{parts.match}</mark>{parts.post}
              {:else}
                {hit.text}
              {/if}
            </span>
          </button>
        {/each}
      {:else}
        <p class="px-3 py-8 text-center text-sm text-muted-foreground">
          {query.trim()
            ? searching
              ? "Searching…"
              : "No matches in the library."
            : "Type to search every document's content."}
        </p>
      {/each}
      {#if hits.length >= 300}
        <p class="px-3 py-1.5 text-center text-[11px] text-muted-foreground">
          Showing the first 300 matches — refine your search.
        </p>
      {/if}
    </div>
  </div>
{/if}
