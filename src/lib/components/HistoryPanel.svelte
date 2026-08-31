<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { diffLines, type ChangeObject } from "diff";
  import { toast } from "svelte-sonner";
  import { app } from "$lib/state.svelte";
  import { relativeTime } from "$lib/comments";
  import { Button } from "$lib/components/ui/button/index.js";
  import History from "@lucide/svelte/icons/history";

  interface VersionInfo {
    name: string;
    modifiedMs: number;
    size: number;
  }

  let versions = $state<VersionInfo[]>([]);
  let selected = $state<string | null>(null);
  let versionText = $state("");
  let loading = $state(false);

  const diff = $derived<ChangeObject<string>[]>(
    selected ? diffLines(versionText, app.content) : [],
  );
  const addedLines = $derived(
    diff.filter((p) => p.added).reduce((n, p) => n + (p.count ?? 0), 0),
  );
  const removedLines = $derived(
    diff.filter((p) => p.removed).reduce((n, p) => n + (p.count ?? 0), 0),
  );

  $effect(() => {
    if (app.historyVisible) load();
  });

  async function load() {
    if (!app.currentPath) return;
    loading = true;
    try {
      versions = await invoke<VersionInfo[]>("list_versions", { path: app.currentPath });
      if (versions.length > 0) await select(versions[0].name);
      else selected = null;
    } catch (e) {
      toast.error(`Could not load history: ${e}`);
    } finally {
      loading = false;
    }
  }

  async function select(name: string) {
    if (!app.currentPath) return;
    try {
      versionText = await invoke<string>("read_version", {
        path: app.currentPath,
        version: name,
      });
      selected = name;
    } catch (e) {
      toast.error(`Could not read version: ${e}`);
    }
  }

  async function restore() {
    if (!selected) return;
    // Route through the normal edit path: write_doc snapshots the current
    // content before it's replaced, so restoring is itself undoable.
    app.content = versionText;
    await app.save();
    toast.success("Version restored (the replaced version was kept in history)");
    close();
  }

  function close() {
    app.historyVisible = false;
  }

  function fmtDate(ms: number): string {
    return new Date(ms).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (app.historyVisible && e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }}
/>

{#if app.historyVisible}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="fixed inset-0 z-40 bg-black/30" onclick={close} transition:fade={{ duration: 120 }}></div>
  <div
    class="fixed top-[8vh] left-1/2 z-50 flex h-[76vh] w-[860px] max-w-[94vw] -translate-x-1/2 overflow-hidden rounded-xl border bg-popover shadow-2xl"
    transition:fly={{ y: -16, duration: 160, easing: cubicOut }}
  >
    <aside class="flex w-56 shrink-0 flex-col border-r">
      <div class="flex items-center gap-2 px-4 pt-4 pb-2">
        <History class="size-4 opacity-60" />
        <span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          History
        </span>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto p-1.5">
        {#each versions as v (v.name)}
          <button
            class="block w-full rounded-md px-2.5 py-2 text-left transition-colors
              {selected === v.name ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60'}"
            onclick={() => select(v.name)}
          >
            <span class="block text-sm font-medium">{relativeTime(new Date(v.modifiedMs).toISOString())}</span>
            <span class="block text-[11px] text-muted-foreground">{fmtDate(v.modifiedMs)}</span>
          </button>
        {:else}
          <p class="px-3 py-6 text-center text-xs text-muted-foreground">
            {loading ? "Loading…" : "No snapshots yet. Versions are kept automatically when the document changes."}
          </p>
        {/each}
      </div>
    </aside>

    <section class="flex min-w-0 flex-1 flex-col">
      <header class="flex items-center gap-3 border-b px-4 py-2.5">
        {#if selected}
          <span class="text-sm font-medium">Changes since this version</span>
          <span class="text-xs tabular-nums text-muted-foreground">
            <span class="text-green-600 dark:text-green-400">+{addedLines}</span>
            <span class="ml-1 text-red-600 dark:text-red-400">−{removedLines}</span>
          </span>
          <div class="flex-1"></div>
          <Button size="sm" class="h-7" onclick={restore}>Restore this version</Button>
        {:else}
          <span class="text-sm text-muted-foreground">No version selected</span>
          <div class="flex-1"></div>
        {/if}
        <Button variant="ghost" size="sm" class="h-7" onclick={close}>Close</Button>
      </header>
      <div class="min-h-0 flex-1 overflow-auto p-4 font-mono text-[12.5px] leading-relaxed">
        {#if selected}
          {#if addedLines === 0 && removedLines === 0}
            <p class="text-sm font-sans text-muted-foreground">
              This version is identical to the current document.
            </p>
          {:else}
            {#each diff as part, i (i)}
              <pre
                class="whitespace-pre-wrap {part.added
                  ? 'bg-green-500/15 text-green-900 dark:text-green-200'
                  : part.removed
                    ? 'bg-red-500/15 text-red-900 line-through decoration-red-900/30 dark:text-red-200'
                    : 'text-muted-foreground'}">{part.value}</pre>
            {/each}
          {/if}
        {/if}
      </div>
    </section>
  </div>
{/if}
