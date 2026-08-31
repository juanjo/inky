<script lang="ts">
  import { tick } from "svelte";
  import { app, type CommentThread } from "$lib/state.svelte";
  import { relativeTime } from "$lib/comments";
  import { Button } from "$lib/components/ui/button/index.js";
  import { ScrollArea } from "$lib/components/ui/scroll-area/index.js";
  import Check from "@lucide/svelte/icons/check";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CornerDownRight from "@lucide/svelte/icons/corner-down-right";

  const filters = [
    { key: "all", label: "All" },
    { key: "open", label: "Open" },
    { key: "resolved", label: "Resolved" },
  ] as const;

  const counts = $derived({
    all: app.commentThreads.length,
    open: app.commentThreads.filter((t) => !t.resolved).length,
    resolved: app.commentThreads.filter((t) => t.resolved).length,
  });

  const visibleThreads = $derived.by(() => {
    const filtered = app.commentThreads.filter((t) =>
      app.commentFilter === "all" ? true : app.commentFilter === "open" ? !t.resolved : t.resolved,
    );
    const posOf = (t: CommentThread) =>
      app.threadOrder[t.id] ?? app.editorThreadPos[t.id]?.from ?? Number.MAX_SAFE_INTEGER;
    return [...filtered].sort(
      (a, b) => posOf(a) - posOf(b) || a.createdAt.localeCompare(b.createdAt),
    );
  });

  function isOrphaned(t: CommentThread): boolean {
    return (
      !t.resolved &&
      app.threadOrder[t.id] === undefined &&
      app.editorThreadPos[t.id] === undefined
    );
  }

  let draftText = $state("");
  let replyText = $state("");
  let replyingTo = $state<string | null>(null);
  let draftBox = $state<HTMLTextAreaElement | undefined>();
  let panel = $state<HTMLElement | undefined>();

  $effect(() => {
    if (app.commentDraft) {
      draftText = "";
      tick().then(() => draftBox?.focus());
    }
  });

  // Keep the active card in view when a highlight is clicked in the document.
  $effect(() => {
    const id = app.activeThreadId;
    if (!id || !panel) return;
    tick().then(() => {
      panel
        ?.querySelector(`[data-card="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    });
  });

  function submitDraft() {
    if (draftText.trim()) app.addThread(draftText);
    draftText = "";
  }

  function submitReply(threadId: string) {
    if (replyText.trim()) app.replyToThread(threadId, replyText);
    replyText = "";
    replyingTo = null;
  }

  function selectCard(t: CommentThread) {
    app.revealThread(t.id);
    if (replyingTo !== t.id) {
      replyingTo = null;
      replyText = "";
    }
  }
</script>

<aside class="no-print flex h-full w-full flex-col border-l bg-background">
  <div class="flex items-center justify-between px-4 pt-4 pb-2">
    <span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
      Comments
    </span>
  </div>

  <div class="mx-3 mb-2 flex items-center rounded-lg bg-muted p-0.5">
    {#each filters as f (f.key)}
      <button
        class="flex-1 rounded-md px-1 py-1 text-xs transition-colors
          {app.commentFilter === f.key
          ? 'bg-background font-medium shadow-sm'
          : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => (app.commentFilter = f.key)}
      >
        {f.label}
        <span class="ml-0.5 tabular-nums opacity-60">{counts[f.key]}</span>
      </button>
    {/each}
  </div>

  <ScrollArea class="min-h-0 flex-1">
    <div class="flex flex-col gap-2 px-3 pb-6" bind:this={panel}>
      {#if app.commentDraft}
        <div class="rounded-xl bg-foreground/[0.03] p-3">
          <blockquote
            class="mb-2 line-clamp-2 border-l-2 border-yellow-500/70 pl-2 text-xs text-muted-foreground italic"
          >
            {app.commentDraft.quote}
          </blockquote>
          <textarea
            bind:this={draftBox}
            bind:value={draftText}
            rows="3"
            placeholder="Add a comment…"
            class="w-full resize-none rounded-md bg-background/80 px-2 py-1.5 text-sm outline-none placeholder:text-muted-foreground/70"
            onkeydown={(e) => {
              if (e.key === "Enter" && (e.metaKey || !e.shiftKey)) {
                e.preventDefault();
                submitDraft();
              } else if (e.key === "Escape") {
                app.commentDraft = null;
              }
            }}
          ></textarea>
          <div class="mt-2 flex justify-end gap-2">
            <Button variant="ghost" size="sm" class="h-7" onclick={() => (app.commentDraft = null)}>
              Cancel
            </Button>
            <Button size="sm" class="h-7" disabled={!draftText.trim()} onclick={submitDraft}>
              Comment
            </Button>
          </div>
        </div>
      {/if}

      {#each visibleThreads as thread (thread.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
        <div
          data-card={thread.id}
          class="group cursor-pointer rounded-xl p-3 transition-colors
            {app.activeThreadId === thread.id
            ? 'bg-foreground/[0.06]'
            : 'bg-foreground/[0.03] hover:bg-foreground/[0.05]'}
            {thread.resolved ? 'opacity-70' : ''}"
          onclick={() => selectCard(thread)}
        >
          <div class="mb-1.5 flex items-start justify-between gap-2">
            <blockquote
              class="line-clamp-2 border-l-2 pl-2 text-xs italic
                {thread.resolved
                ? 'border-border text-muted-foreground/70 line-through'
                : 'border-yellow-500/70 text-muted-foreground'}"
            >
              {thread.quote}
            </blockquote>
            <div class="flex shrink-0 gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
              {#if thread.resolved}
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-6"
                  title="Reopen"
                  onclick={(e) => {
                    e.stopPropagation();
                    app.setThreadResolved(thread.id, false);
                  }}
                >
                  <RotateCcw class="size-3.5" />
                </Button>
              {:else}
                <Button
                  variant="ghost"
                  size="icon"
                  class="size-6"
                  title="Resolve"
                  onclick={(e) => {
                    e.stopPropagation();
                    app.setThreadResolved(thread.id, true);
                  }}
                >
                  <Check class="size-3.5" />
                </Button>
              {/if}
              <Button
                variant="ghost"
                size="icon"
                class="size-6 hover:text-destructive"
                title="Delete thread"
                onclick={(e) => {
                  e.stopPropagation();
                  app.deleteThread(thread.id);
                }}
              >
                <Trash2 class="size-3.5" />
              </Button>
            </div>
          </div>

          {#if isOrphaned(thread)}
            <p class="mb-1 text-[11px] text-amber-600 dark:text-amber-400">
              Original text no longer found
            </p>
          {/if}
          {#if thread.resolved}
            <p class="mb-1 text-[11px] font-medium text-green-700 dark:text-green-400">Resolved</p>
          {/if}

          <div class="flex flex-col gap-2">
            {#each thread.comments as msg, i (msg.id)}
              <div class={i > 0 ? "border-l-2 border-border pl-2" : ""}>
                {#if msg.author}
                  <span
                    class="mb-0.5 inline-block rounded bg-foreground/10 px-1 py-px text-[10px] font-semibold"
                  >
                    {msg.author}
                  </span>
                {/if}
                <p class="text-sm leading-snug whitespace-pre-wrap select-text">{msg.text}</p>
                <p class="mt-0.5 text-[11px] text-muted-foreground">{relativeTime(msg.createdAt)}</p>
              </div>
            {/each}
          </div>

          {#if replyingTo === thread.id}
            <!-- svelte-ignore a11y_autofocus -->
            <textarea
              bind:value={replyText}
              rows="2"
              autofocus
              placeholder="Reply…"
              class="mt-2 w-full resize-none rounded-md bg-background/80 px-2 py-1.5 text-sm outline-none placeholder:text-muted-foreground/70"
              onclick={(e) => e.stopPropagation()}
              onkeydown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  submitReply(thread.id);
                } else if (e.key === "Escape") {
                  replyingTo = null;
                  replyText = "";
                }
              }}
            ></textarea>
          {:else}
            <button
              class="mt-2 flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
              onclick={(e) => {
                e.stopPropagation();
                replyingTo = thread.id;
                replyText = "";
                app.activeThreadId = thread.id;
              }}
            >
              <CornerDownRight class="size-3" /> Reply
            </button>
          {/if}
        </div>
      {:else}
        {#if !app.commentDraft}
          <p class="px-2 py-8 text-center text-sm text-muted-foreground">
            {app.commentFilter === "resolved"
              ? "No resolved comments."
              : "No comments yet. Select some text in the document and click “Comment”."}
          </p>
        {/if}
      {/each}
    </div>
  </ScrollArea>
</aside>
