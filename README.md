# Inky 🖋️

A small, pleasant macOS app for reading and writing markdown. Built with
Tauri 2, Svelte 5, Tailwind CSS 4 and shadcn-svelte.

![Inky](src-tauri/icons/128x128.png)

## Features

### Library

- Documents live as plain `.md` files in `~/Documents/Inky` (changeable via
  *… → Change library folder*) — no database, no lock-in.
- Folders, rename (right-click, the row's `…` button, or just click the
  title in the toolbar), move to Trash, reveal in Finder.
- Drag & drop documents and folders between folders; drop on empty sidebar
  space to move to the library root.
- **Quick open** — ⌘K fuzzy-finds any document.
- **Paste as document** — copy markdown anywhere, hit ⌘⇧V and it becomes a
  new document. Pasted mermaid source is detected and saved as a diagram.

### Reading & writing

- **Three views** — Reading (⌘1), Split (⌘2), Writing (⌘3) with a CodeMirror
  markdown editor and live preview. Changes autosave; live word count and
  reading time sit next to the title.
- **Edit while reading** — click into any paragraph, heading or list in
  reading view and just type: the block stays rendered (no markdown syntax)
  with a floating bar for bold/italic/strike/code, headings, lists and
  quotes. Click away or ⌘↩ applies, Esc cancels. Code blocks edit their code
  in place (fences stay hidden); math and footnote blocks fall back to
  source editing.
- **Split-view scroll sync** — editor and preview scroll together; toggle it
  from the toolbar or the View menu.
- **Table of contents** — ⌘T opens a right-hand panel listing every heading;
  click to jump (in any view), and the current section stays highlighted
  while you scroll.
- **Find in document** — ⌘F with match highlighting and next/previous
  navigation, in both the preview and the editor.
- **Focus mode** — ⌘⇧F hides everything but the editor and dims all but the
  paragraph you're writing.
- **Themes** — Light, Dark (anthracite, not black), and Book (warm paper +
  serif typography).
- **Reading settings** — the `Aa` popover adjusts font size (Kindle-style
  steps, 85–170%) and text column width (Default → Full in five steps); both
  persist and apply to the editor as well as the preview.
- Sidebar, TOC and comments panels are drag-resizable.

### Comments

- Select text in reading or writing mode and click **Comment** to start a
  thread — your questions and ideas, Google-Docs style.
- Reply to threads, **resolve/reopen** them, delete them; filter the panel
  (⌘⇧C) by **All / Open / Resolved**.
- Open threads highlight their text in both preview and editor; hover a
  highlight for a preview card, click it to open the thread.
- Comments are anchored to the quoted text plus context, so they survive
  edits elsewhere; if the text is deleted the thread is kept and flagged.
- Stored in a hidden sidecar (`.«name».comments.json`) next to each document
  — your markdown stays clean, and rename/move/trash carries comments along.

### Rendering

- **Mermaid** — ` ```mermaid ` fences render inside documents; `.mmd` files
  render as standalone diagrams.
- **Math & footnotes** — KaTeX (`$…$`, `$$…$$`) and GFM-style footnotes
  (`[^1]`).
- **Local images** — relative image paths render in the preview; pasting an
  image into the editor saves it to an `assets/` folder next to the document
  and inserts the link.
- **PDF export** — ⌘E writes a PDF through a native macOS print operation
  with proper page margins and document-natural type, regardless of theme or
  reading settings; ⌘P opens the print dialog.
- **Copy** — the markdown source or the rendered HTML.

### App

- **Native menu bar** — File/Edit/View menus cover documents, export,
  themes, text width, font size and view toggles with standard shortcuts.
- **Auto-updates** — checks on launch and via *Inky → Check for Updates…*
  (see below).
- **MCP server** — agents can read and write your library through Inky's
  built-in MCP server (see below). Nothing to install: toggle it from the
  toolbar's red/green **MCP** status light, or point stdio clients at
  `Inky --mcp`.
- **Safety & history** — unsaved changes are flushed on window blur, close,
  and ⌘Q; every content-changing overwrite keeps the previous version in a
  hidden `.inky-history/` folder next to the document (max one per 10
  minutes, last 20 kept). *… → Version history* (or File → Version History…)
  shows each snapshot with a colored diff against the current document and
  one-click restore — restoring snapshots the replaced version too, so it's
  always reversible. Edits that collide with on-disk changes warn instead of
  silently losing either side. Single-instance guard and window-state
  restore included.

## Development

Prerequisites: Rust (stable), Node 20+, pnpm.

```sh
pnpm install
make dev
```

## Packaging (share with friends)

Common actions live in the Makefile — run `make` (or `make help`) to list them:

| Target | What it does |
| --- | --- |
| `make dev` | Run the app with hot reload |
| `make check` | Type-check frontend + Rust, run the Rust tests |
| `make dmg` | Build the signed `.app`, `.dmg` and updater artifacts |
| `make release` | `make dmg` + assemble `dist/release/` ready for a GitHub release (incl. `latest.json`) |
| `make icons` | Regenerate all app icons from `assets/icon.svg` |
| `make open` | Open the last built release app |
| `make mcp` | Run the MCP server on stdio (dev build) |
| `make clean` | Remove build outputs |

Builds are signed with `~/.tauri/inky.key` (override with `make dmg KEY=…`);
plain `pnpm tauri build` fails now that updater artifacts are enabled
(`pnpm bundle` still works as an alias for `make dmg`).

`make dmg` produces:

- `src-tauri/target/release/bundle/macos/Inky.app`
- `src-tauri/target/release/bundle/dmg/Inky_<version>_aarch64.dmg`

Send the `.dmg`. Since the app is not notarized with an Apple Developer ID,
the first launch on a friend's Mac requires either **right-click → Open**, or:

```sh
xattr -cr /Applications/Inky.app
```

(To ship without that caveat you'd need an Apple Developer account and
notarization — `tauri build` supports it via the `APPLE_*` signing env vars.)

### Auto-updates

The app checks for updates on launch (silently) and via **Inky → Check for
Updates…**. The updater endpoint in `src-tauri/tauri.conf.json` points at the
latest GitHub release of `juanjo/inky`. Shipping an update:

1. Bump `version` in `src-tauri/tauri.conf.json` and commit.
2. `git tag v<version> && git push --tags` — CI
   (`.github/workflows/release.yml`) builds, signs, and publishes the release
   automatically. (`make release` still works for a manual, local release.)
3. Existing installs pick it up on next launch.

Updates are signed with `~/.tauri/inky.key` (mirrored in the repo's
`TAURI_SIGNING_PRIVATE_KEY` Actions secret) — back that file up; without it
you can't ship updates to existing installs.

## MCP server (let agents use your library)

Inky ships an MCP server inside the app binary — no Node.js or other runtime
needed. It exposes `list_documents`, `read_document`, `write_document`,
`patch_document`, `rename_document`, `move_document`, `create_folder`,
`delete_document`, `search_documents`, history tools (`list_versions`,
`read_version`) and comment tools (`list_comments`, `create_comment`,
`reply_to_comment`, `resolve_comment`), `open_document` (shows a document in
the Inky app), plus every document as an
`inky://doc/…` resource. The library folder is resolved from `INKY_LIBRARY`,
then the app's config (`~/Library/Application Support/com.inky.app/config.json`),
then `~/Documents/Inky` — so the app and agents always see the same documents.
The app refreshes on focus, so agent-created documents just show up.

Two ways to connect (right-click the toolbar's **MCP** light for copyable,
pre-filled snippets):

**HTTP** — the toolbar light runs a streamable-HTTP server on
`http://127.0.0.1:26317/mcp` while the app is open (green = live; it
restarts on the next launch until you stop it):

```sh
claude mcp add --transport http inky http://127.0.0.1:26317/mcp
```

**stdio** — any client that takes a `command` can launch the binary directly,
even when Inky isn't running:

```sh
claude mcp add inky -- "/Applications/Inky.app/Contents/MacOS/Inky" --mcp
```

Claude Desktop (`claude_desktop_config.json`):

```json
{ "mcpServers": { "inky": { "command": "/Applications/Inky.app/Contents/MacOS/Inky", "args": ["--mcp"] } } }
```

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.inky]
command = "/Applications/Inky.app/Contents/MacOS/Inky"
args = ["--mcp"]
```

ChatGPT's connectors only accept public HTTPS URLs, so they can't reach a
server on your Mac; Inky doesn't support ChatGPT.

This repo's `.mcp.json` registers the HTTP server, so Claude Code sessions
opened here use the running app. Then ask an agent things like *"write
yesterday's meeting notes into my Inky library under Meetings/"* or *"read my
Inky doc 'Ideas' and summarize it"*.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| ⌘N | New document |
| ⌘K | Quick open |
| ⌘S | Save (autosave also runs) |
| ⌘E | Export PDF |
| ⌘P | Print |
| ⌘⇧V | Paste clipboard as new document |
| ⌘1 / ⌘2 / ⌘3 | Reading / Split / Writing view |
| ⌘F | Find in document |
| ⌘⇧F | Focus mode |
| ⌘⇧C | Toggle comments |
| ⌘T | Toggle table of contents |
| ⌘B (or ⌘\) | Toggle sidebar |
| ⌘+ / ⌘− | Font size |
