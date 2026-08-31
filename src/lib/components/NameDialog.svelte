<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Label } from "$lib/components/ui/label/index.js";

  interface Props {
    open: boolean;
    title: string;
    label: string;
    initial: string;
    action: string;
    onsubmit: (name: string) => void;
    oncancel: () => void;
  }
  let { open, title, label, initial, action, onsubmit, oncancel }: Props = $props();

  let value = $state("");

  $effect(() => {
    if (open) value = initial;
  });

  function submit() {
    const name = value.trim();
    if (name) onsubmit(name);
  }
</script>

<Dialog.Root {open} onOpenChange={(o) => !o && oncancel()}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{title}</Dialog.Title>
    </Dialog.Header>
    <form
      class="grid gap-3"
      onsubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <Label for="name-input">{label}</Label>
      <!-- svelte-ignore a11y_autofocus -->
      <Input id="name-input" bind:value autofocus autocomplete="off" spellcheck={false} />
      <Dialog.Footer class="mt-2">
        <Button type="button" variant="outline" onclick={oncancel}>Cancel</Button>
        <Button type="submit" disabled={!value.trim()}>{action}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
