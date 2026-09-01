<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { invoke } from "@tauri-apps/api/core";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { toast } from "svelte-sonner";
  import Copy from "@lucide/svelte/icons/copy";
  import { app } from "$lib/state.svelte";

  let binary = $state("/Applications/Inky.app/Contents/MacOS/Inky");
  $effect(() => {
    if (app.mcpSetupOpen) {
      invoke<string>("app_binary_path")
        .then((p) => (binary = p))
        .catch(() => {});
    }
  });

  const httpUrl = $derived(app.mcpUrl ?? "http://127.0.0.1:26317/mcp");

  const snippets = $derived([
    {
      title: "Claude Code (HTTP — needs Inky running with the light on)",
      code: `claude mcp add --transport http inky ${httpUrl}`,
    },
    {
      title: "Claude Code (stdio — works even when Inky is closed)",
      code: `claude mcp add inky -- "${binary}" --mcp`,
    },
    {
      title: "Claude Desktop — add to claude_desktop_config.json",
      code: JSON.stringify({ mcpServers: { inky: { command: binary, args: ["--mcp"] } } }, null, 2),
    },
    {
      title: "Codex — add to ~/.codex/config.toml",
      code: `[mcp_servers.inky]\ncommand = "${binary}"\nargs = ["--mcp"]`,
    },
  ]);

  async function copy(code: string) {
    await writeText(code);
    toast.success("Copied");
  }
</script>

<Dialog.Root bind:open={app.mcpSetupOpen}>
  <Dialog.Content class="sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>Connect an agent</Dialog.Title>
      <Dialog.Description>
        Agents read and write your library through Inky's built-in MCP server — nothing else to
        install. Pick your client:
      </Dialog.Description>
    </Dialog.Header>
    <div class="flex flex-col gap-4">
      {#each snippets as s (s.title)}
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-medium">{s.title}</span>
            <Button
              variant="ghost"
              size="sm"
              class="h-7 gap-1 px-2 text-xs"
              onclick={() => copy(s.code)}
            >
              <Copy class="size-3.5" /> Copy
            </Button>
          </div>
          <pre class="overflow-x-auto rounded-md bg-muted px-3 py-2 text-xs"><code>{s.code}</code></pre>
        </div>
      {/each}
      <p class="text-xs text-muted-foreground">
        ChatGPT connectors need a public HTTPS URL, so they can't reach a server on your Mac.
      </p>
    </div>
  </Dialog.Content>
</Dialog.Root>
