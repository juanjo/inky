# Open anywhere: Inky as the default Markdown app

Date: 2026-10-01 · Status: draft for review

## Goal

Make Inky the Mac's default app for Markdown. Any `.md` file anywhere on disk
opens in Inky — from Finder, "Open With", the terminal, or File → Open — and is
read and edited in place. A list of recent documents makes them easy to get
back to.

## Decisions (agreed)

1. **Outside files are edited in place.** Saving writes the original file.
   Inky writes **no sidecar files** next to them: no `.inky-history/`, no
   comment sidecar. Version History and Comments are unavailable for them.
2. **`inky <folder>` shows that folder in the sidebar, temporarily** (a
   *workspace*). The library is unchanged; "Back to Library" returns to it.
   Tree, Quick Open and library search cover whichever root is showing.
3. **Access model: a list of opened places (approach A).** The app may touch
   the library plus what the user explicitly opened. The MCP server stays
   confined to the library.

## Architecture

### Places (Rust, `src-tauri/src/places.rs`)

A `Places` state held by the app:

| Place | Source | Sidecars | Allowed operations |
|---|---|---|---|
| Library | config (as today) | on | everything (as today) |
| Workspace (0 or 1) | folder opened via Finder / CLI / Open dialog | off | read, write, create, rename, move, delete, tree, search, mtime, save image |
| Granted files | file opened via Finder / CLI / Open dialog / Open Recent / followed link | off | read, write, mtime, rename (grant follows the new name), save image |

- A workspace is a `Library` instance rooted at the folder, created with a new
  `sidecars: false` option. With sidecars off, `write`/`patch` skip
  `snapshot()`, and the comment and version functions return an error. All path
  confinement stays in `Library::resolve`, which is reused as is.
- Granted files are stored as canonical absolute paths. A path matches only if
  its canonical form is exactly a granted path.
- Every app command that takes a path resolves it through
  `Places::resolve(path) -> Resolved { lib_or_file, abs, sidecars }`, in this
  order: library, then workspace, then granted files. Anything else is rejected
  with the same "outside the library" error as today.
- **Grants are created only on the Rust side**, from native events or
  Rust-owned data. The webview can never grant itself a path:
  - macOS open events (Finder, Open With, `open -a`, the `inky` CLI)
  - File → Open… runs the native dialog *in Rust* (the blocking dialog API)
    and grants what the user picked
  - `open_recent(path)` grants a path only if it is in the persisted recents
    list
  - `follow_link(from_doc, href)` resolves `href` relative to an accessible
    document and grants the target only if it exists and is a Markdown file
    (`.md`/`.markdown`/`.mmd`). Following a relative link from an outside
    document therefore keeps working. Other file types keep today's behavior
    (reveal in Finder).
- **MCP:** `mcp.rs` keeps building its own `Library` from config. It never sees
  `Places`, so agents cannot reach outside files or the workspace.

### OS integration

- **File associations:** `bundle.fileAssociations` in `tauri.conf.json` for
  `md`, `markdown` and `mmd` (role Editor). After installing, the user picks
  Inky once in Finder → Get Info → "Open with" → "Change All…". Inky can't set
  itself as the default; macOS requires the user to confirm it.
- **Open events:** handle `RunEvent::Opened { urls }` (macOS). For each URL:
  - a file is granted, added to recents, and the event `open-path` is emitted
  - a folder becomes the workspace, and the event `open-folder` is emitted
- **Cold launch:** events that arrive before the webview is ready go into a
  pending queue, which the frontend drains at the end of `init()` through
  `take_pending_opens`. A file opened this way replaces the usual "reopen last
  document" behavior.
- **Single-instance callback:** paths passed as arguments are handled the same
  way. This is a fallback for a direct `Inky.app/Contents/MacOS/Inky <path>`
  run.

### `inky` command

- **Install:** Inky menu → "Install 'inky' Command in PATH…", next to "Check
  for Updates…". It writes `/usr/local/bin/inky` through `osascript …
  with administrator privileges`, a one-time password prompt. Running it again
  overwrites the script, and that is the whole update story.
- **Script:**

  ```sh
  #!/bin/sh
  # inky — open files or folders in Inky
  [ $# -eq 0 ] && exec open -b com.inky.app
  exec open -b com.inky.app "$@"
  ```

  `open` resolves relative paths against the current folder and delivers them
  as an open event. The CLI therefore takes the same route as Finder, and no
  argument parsing is needed in the app.
- **Missing paths:** `open` itself reports "file does not exist" and exits
  non-zero.

### Recents

- **Storage:** Rust persists the list in `config.json` as `recent: Vec<String>`
  (canonical absolute paths). The newest entry comes first, duplicates are
  removed, and the list is capped at 20. Library and outside documents share
  one list.
- **When it changes:**
  - an entry is added on every successful document open, whatever the source
  - rename and move rewrite matching entries
  - delete removes them
  - entries whose files no longer exist are pruned when the list is read
- **Where it shows:**
  - **File → Open Recent** submenu, rebuilt when the list changes, plus a
    "Clear Menu" item
  - **Quick Open (⌘K)** shows recents first while the query is empty;
    outside files show their folder as the subtitle
  - **Dock right-click menu** via `NSDocumentController
    noteNewRecentDocumentURL:` (an objc2 call, which needs the
    `NSDocumentController` feature)
- **Relaunch:** the "reopen last document" startup step goes through
  `open_recent`, so an outside document is reopened after a relaunch (it is
  re-granted because it is in the recents list).

## Frontend changes

- `app.workspace: { root: string; name: string } | null`. The sidebar header
  shows the folder name and a **Back to Library** button while it is set.
  `list_tree`, `search_library` and the file watcher use the active root on the
  Rust side, so `refreshTree` etc. need no new arguments.
- **`openDoc`** asks Rust whether the document has sidecars
  (`doc_sidecars(path) -> bool`; `read_doc` keeps returning plain text, so its
  other callers don't change). When `sidecars` is false, the comments and history
  toolbar buttons and their shortcuts are disabled, with the tooltip
  "Not available for files outside the library".
- **Title bar:** for an outside document, the toolbar shows a muted
  `~/path/to/folder` after the title, so it's clear where the file is saved.
- **Links:** `Preview.handleLink` calls `follow_link` instead of
  `path_exists` + `openDoc`.
- **File menu:** adds Open… (⌘O), Open Folder… (⌘⇧O) and the Open Recent
  submenu. These menu events are handled **in Rust** (dialog, grant, emit
  `open-request`) and never reach the webview as raw paths, so grants stay
  Rust-only.
- **Back/forward history:** works unchanged across library and outside files.
  Navigating back to an outside file still works because its grant persists
  for the whole session.

## File watching

The watcher follows the active root: the workspace when one is showing,
otherwise the library. Single outside files rely on the existing
focus-triggered `syncFromDisk` and the mtime conflict check on save; nothing
new watches them.

## Error handling

- A path that can't be opened (deleted, or no permission) shows the existing
  "Could not open document" toast and is pruned from recents.
- An open event for a non-Markdown file is ignored. macOS only routes
  associated types, but `inky foo.txt` can still send one.
- If CLI install fails (cancelled password prompt, etc.), a toast shows the
  error. Nothing is half-written, because the script is copied in a single
  `cp`.

## Testing

- **Rust unit tests (`places.rs`):**
  - library paths resolve with sidecars on
  - workspace paths resolve with sidecars off
  - granted files resolve and their siblings are rejected
  - a granted file's symlink and `..` tricks are rejected
  - `follow_link` grants Markdown targets only
  - rename moves the grant
  - workspace writes create no `.inky-history`
- **Rust unit tests (recents):** dedupe, cap, newest first, rename rewrite,
  delete removal, missing-file pruning.
- **Existing MCP tests** keep proving that agents are confined to the library.
- **Manual on a bundled build (`pnpm bundle`):**
  - Finder double-click, cold and warm
  - `inky README.md` from a git repo (edit, save, `git status` shows only that
    file changed)
  - `inky ~/some/folder`, then Back to Library
  - Open Recent after a relaunch
  - Dock recents

## Out of scope

- Multiple windows, or more than one workspace at a time
- Version history or comments for outside files
- Remembering the workspace across relaunches (Inky starts in the library)
- Windows/Linux file-association testing
