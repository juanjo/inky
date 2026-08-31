export const MERMAID_TEMPLATE = `flowchart TD
    A[Start] --> B{Question?}
    B -- Yes --> C[Do the thing]
    B -- No --> D[Do the other thing]
    C --> E[Done]
    D --> E
`;

export const WELCOME_DOC = `# Welcome to Inky 🖋️

Inky is a small, pleasant home for your markdown. Everything you see in the
sidebar lives as plain \`.md\` files in your **Inky library folder**, so your
notes are always yours.

## The basics

- **Paste** — copy any markdown, hit the clipboard button (or ⌘⇧V) and it
  becomes a new document.
- **Edit** — switch between *Reading*, *Split* and *Writing* views from the
  toolbar (or ⌘1 / ⌘2 / ⌘3). Changes autosave.
- **Organize** — right-click the sidebar to create folders, rename or delete.
- **Export** — ⌘P opens the print dialog; choose *Save as PDF*.
- **Themes** — light, dark and book mode, for easy reading.

## Everything renders nicely

> Blockquotes look like this. Perfect for profound thoughts you found
> somewhere else.

Inline \`code\`, **bold**, *italics*, ~~strikethrough~~ and [links](https://example.com) all work.

### Code

\`\`\`ts
function greet(name: string): string {
  return \`Hello, \${name}!\`;
}
\`\`\`

### Tables

| Feature | Status |
| ------- | ------ |
| Markdown rendering | ✅ |
| Mermaid diagrams | ✅ |
| PDF export | ✅ |

### Task lists

- [x] Install Inky
- [ ] Write something wonderful

### Mermaid diagrams

Fenced \`mermaid\` blocks render as diagrams — and files ending in \`.mmd\`
are treated as standalone diagrams:

\`\`\`mermaid
flowchart LR
    md[Markdown] --> inky{Inky}
    inky --> pretty[Beautiful document]
    inky --> pdf[PDF]
\`\`\`

Happy writing!
`;
