<script lang="ts">
  import "../app.css";
  import "katex/dist/katex.min.css";
  import { Toaster } from "$lib/components/ui/sonner/index.js";
  import { app } from "$lib/state.svelte";

  let { children } = $props();
</script>

<Toaster theme={app.theme === "dark" ? "dark" : "light"} position="bottom-right" />
{@render children()}
