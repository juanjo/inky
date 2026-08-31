<script lang="ts">
  import { fade } from "svelte/transition";
  import type { CommentThread } from "$lib/state.svelte";
  import { relativeTime } from "$lib/comments";

  interface Props {
    thread: CommentThread;
    x: number;
    y: number;
  }
  let { thread, x, y }: Props = $props();

  const shown = $derived(thread.comments.slice(0, 3));
  const hidden = $derived(thread.comments.length - shown.length);
</script>

<div
  class="pointer-events-none absolute z-30 w-64 rounded-lg border bg-popover p-3 shadow-lg"
  style="left: {x}px; top: {y}px"
  transition:fade={{ duration: 100 }}
>
  <div class="flex flex-col gap-2">
    {#each shown as msg, i (msg.id)}
      <div class={i > 0 ? "border-l-2 border-border pl-2" : ""}>
        <p class="line-clamp-3 text-xs leading-snug whitespace-pre-wrap">{msg.text}</p>
        <p class="mt-0.5 text-[10px] text-muted-foreground">{relativeTime(msg.createdAt)}</p>
      </div>
    {/each}
  </div>
  {#if hidden > 0}
    <p class="mt-1.5 text-[10px] text-muted-foreground">
      +{hidden} more {hidden === 1 ? "reply" : "replies"}
    </p>
  {/if}
  <p class="mt-1.5 text-[10px] text-muted-foreground/70">Click to open thread</p>
</div>
