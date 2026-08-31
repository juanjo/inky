<script lang="ts">
  import { app } from "$lib/state.svelte";
  import { ScrollArea } from "$lib/components/ui/scroll-area/index.js";

  // Indent relative to the shallowest heading level present in the document.
  const minLevel = $derived(app.toc.length ? Math.min(...app.toc.map((e) => e.level)) : 1);

  function levelClass(rel: number, active: boolean): string {
    if (active) {
      return rel === 0
        ? "bg-accent text-accent-foreground text-sm font-semibold"
        : "bg-accent text-accent-foreground text-[13px] font-medium";
    }
    if (rel === 0) return "text-sm font-medium text-foreground/90 hover:bg-accent/60";
    if (rel === 1)
      return "text-[13px] text-muted-foreground hover:bg-accent/60 hover:text-foreground";
    return "text-[13px] text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground";
  }
</script>

<aside class="no-print flex h-full w-full flex-col border-l bg-background">
  <div class="px-4 pt-4 pb-3">
    <span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
      Contents
    </span>
  </div>
  <ScrollArea class="min-h-0 flex-1">
    <nav class="px-2 pb-6">
      {#each app.toc as entry (entry.id)}
        {@const rel = Math.min(entry.level - minLevel, 3)}
        <div class={rel === 0 ? "mt-4 first:mt-0" : "mt-0.5"}>
          <button
            class="relative block w-full rounded-md py-1.5 pr-2 text-left leading-normal transition-colors
              {levelClass(rel, app.activeHeadingId === entry.id)}"
            style="padding-left: {12 + rel * 16}px"
            title={entry.text}
            onclick={() => app.scrollToHeading(entry)}
          >
            {#if rel > 0}
              <span
                class="pointer-events-none absolute top-1 bottom-1 w-px bg-border"
                style="left: {4 + rel * 16}px"
              ></span>
            {/if}
            <span class="line-clamp-2">{entry.text}</span>
          </button>
        </div>
      {:else}
        <p class="px-2 py-4 text-sm text-muted-foreground">No headings in this document.</p>
      {/each}
    </nav>
  </ScrollArea>
</aside>
