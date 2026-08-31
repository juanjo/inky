# Inky 🖋️

A small, pleasant macOS app for reading and writing markdown. Built with
Tauri 2, Svelte 5, Tailwind CSS 4 and shadcn-svelte.

![Inky](src-tauri/icons/128x128.png)

## Features

- **Library sidebar** — documents live as plain `.md` files in `~/Documents/Inky`
  (changeable via *… → Change library folder*). Folders, rename, move to Trash,
  reveal in Finder (right-click or the row's `…` button), and drag & drop to
  move documents between folders (drop on empty sidebar space to move to the
  library root).
- **Three views** — Reading (⌘1), Split (⌘2), Writing (⌘3) with a CodeMirror
  markdown editor and live preview. Changes autosave.
- **Paste as document** — copy markdown anywhere, hit ⌘⇧V (or the clipboard
  button) and it becomes a new document. Pasted mermaid source is detected and
  saved as a diagram.
- **Mermaid** — ` ```mermaid ` fences render inside documents; `.mmd` files
  render as standalone diagrams.
- **PDF export** — ⌘E picks a destination and writes the PDF directly through a
  native macOS print operation with proper page margins; ⌘P opens the print
  dialog. Output is always clean black-on-white regardless of the app theme.
- **Copy** — copy the markdown source or the rendered HTML.
- **Themes** — Light, Dark, and Book (warm paper + serif typography).
- **Reading settings** — the `Aa` toolbar popover adjusts font size
  (Kindle-style steps, 85–170%) and text column width (Narrow → Full); both
  persist across launches and apply to the editor as well as the preview. PDF
  output ignores these and always uses document-natural sizing.
- **Table of contents** — ⌘T opens a right-hand panel listing every heading;
  click to jump (works in reading, split, and writing modes), and the current
  section stays highlighted while you scroll.
- **Split-view scroll sync** — editor and preview scroll together in split
  mode; toggle it from the toolbar or View menu.
- **Find in document** — ⌘F searches the open document with match
  highlighting and next/previous navigation, in both the preview and the
  editor. (For search across all documents, ask an agent via the MCP server's
  `search_documents` tool.)
- **Native menu bar** — File/Edit/View menus cover documents, export, themes,
  text width, font size, and view toggles with standard macOS shortcuts.
- **Local images** — relative image paths render in the preview, and pasting
  an image into the editor saves it to an `assets/` folder next to the
  document and inserts the link.
- **Quick open** — ⌘K fuzzy-finds any document in the library.
- **Math & footnotes** — KaTeX (`$…$`, `$$…$$`) and GFM-style footnotes
  (`[^1]`) render in the preview and in PDFs.
- **Focus mode** — ⌘⇧F hides everything but the editor and dims all but the
  paragraph you're writing.
- **Word count** — live word count and reading time next to the title.

## Development

Prerequisites: Rust (stable), Node 20+, pnpm.

```sh
pnpm install
pnpm tauri dev
```

## Packaging (share with friends)

Common actions live in the Makefile — run `make` (or `make help`) to list them:

| Target | What it does |
| --- | --- |
| `make dev` | Run the app with hot reload |
| `make check` | Type-check frontend + Rust |
| `make dmg` | Build the signed `.app`, `.dmg` and updater artifacts |
| `make release` | `make dmg` + assemble `dist/release/` ready for a GitHub release (incl. `latest.json`) |
| `make icons` | Regenerate all app icons from `assets/icon.svg` |
| `make open` | Open the last built release app |
| `make mcp` | Run the MCP server on stdio |
| `make clean` | Remove build outputs |

Builds are signed with `~/.tauri/inky.key` (override with `make dmg KEY=…`);
plain `pnpm tauri build` fails now that updater artifacts are enabled
(`pnpm bundle` still works as an alias for `make dmg`).

This produces:

- `src-tauri/target/release/bundle/macos/Inky.app`
- `src-tauri/target/release/bundle/dmg/Inky_0.1.0_aarch64.dmg`

Send the `.dmg`. Since the app is not notarized with an Apple Developer ID,
the first launch on a friend's Mac requires either **right-click → Open**, or:

```sh
xattr -cr /Applications/Inky.app
```

(To ship without that caveat you'd need an Apple Developer account and
notarization — `tauri build` supports it via the `APPLE_*` signing env vars.)

### Auto-updates

The app checks for updates on launch (silently) and via **Inky → Check for
Updates…**. To make updates live:

1. Create a GitHub repo for Inky and replace
   `REPLACE_WITH_YOUR_GITHUB_USER` in the updater endpoint in
   `src-tauri/tauri.conf.json`.
2. Bump `version` in `src-tauri/tauri.conf.json`, run `make release`.
3. Create a GitHub release tagged `v<version>` and upload the four files
   from `dist/release/` (dmg, tar.gz, sig, latest.json).

Updates are signed with `~/.tauri/inky.key` — back that file up; without it
you can't ship updates to existing installs.

## MCP server (let agents use your library)

`mcp/server.mjs` is a stdio MCP server exposing the Inky library to agents:
`list_documents`, `read_document`, `write_document`, `create_folder`,
`delete_document`, `search_documents`. It resolves the library folder from
`INKY_LIBRARY`, then the app's own config
(`~/Library/Application Support/com.inky.app/config.json`), then
`~/Documents/Inky` — so the app and agents always see the same documents.
The app refreshes its sidebar on focus, so agent-created documents just show up.

This repo ships a project-scoped `.mcp.json`, so Claude Code sessions opened
here get it automatically. To register it globally:

```sh
claude mcp add --scope user inky -- node /path/to/inky/mcp/server.mjs
```

Then ask an agent things like *"write yesterday's meeting notes into my Inky
library under Meetings/"* or *"read my Inky doc 'Ideas' and summarize it"*.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| ⌘N | New document |
| ⌘S | Save (autosave also runs) |
| ⌘E | Export PDF |
| ⌘P | Print |
| ⌘⇧V | Paste clipboard as new document |
| ⌘1 / ⌘2 / ⌘3 | Reading / Split / Writing view |
| ⌘K | Quick open |
| ⌘F | Find in document |
| ⌘⇧F | Focus mode |
| ⌘+ / ⌘− | Font size |
| ⌘B (or ⌘\) | Toggle sidebar |
| ⌘T | Toggle table of contents |
