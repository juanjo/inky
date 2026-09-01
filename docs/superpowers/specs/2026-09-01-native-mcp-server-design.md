# Native MCP server (no Node.js dependency)

**Date:** 2026-09-01
**Status:** draft, awaiting review

## Problem

The toolbar's MCP status light spawns `node mcp/server.bundle.mjs --http 26317`
(`src-tauri/src/lib.rs` `find_node` / `start_mcp`). A friend who installs the
dmg on a Mac without Node.js sees *"Node.js not found — install it"* and can
never connect an agent. The JS server is also a ~620-line duplicate of
library logic that already exists in Rust (`snapshot`, `sidecar_for`,
`search_library`, `list_versions`, `read_version`, comments, `guard`, …), and
the two copies have quietly diverged (see *Behaviour reconciliation*).

Friends will use a mix of Claude Code, Claude Desktop, Codex and ChatGPT, so
the server must speak both **streamable HTTP** (app-hosted, for the status
light) and **stdio** (for clients that only take a `command`).

## Goal

One binary — `Inky.app` — provides the MCP server over both transports with
no external runtime, from a single implementation of the library logic that
the desktop app's own commands also use. The dmg stays small (~7.5 MB today).

Out of scope: ChatGPT. Its connectors require a publicly reachable HTTPS URL,
so a local server cannot be registered without a tunnel. The README will say
so explicitly rather than pretend.

## Architecture

```
src-tauri/src/
  main.rs        --mcp flag → mcp::serve_stdio() (never boots Tauri); else inky_lib::run()
  lib.rs         Tauri app: window, menu, thin #[tauri::command] wrappers, start/stop/status of the HTTP server
  library.rs     NEW  Library { root }: every filesystem operation on the library (pure Rust, no Tauri types)
  mcp.rs         NEW  rmcp ServerHandler (15 tools + document resources) over Library; serve_stdio(); serve_http(port)
```

### `library.rs` — the single source of truth

`Library::open(root: PathBuf)` plus two resolvers:

- `Library::from_app(&AppHandle)` — today's `resolve_root` (config → `~/Documents/Inky`, writes the config back).
- `Library::from_env()` — for stdio mode, with no Tauri handle: `INKY_LIBRARY`
  env var → `dirs::config_dir()/com.inky.app/config.json` (`library` key) →
  `~/Documents/Inky`. `dirs::config_dir()` is what Tauri's `app_config_dir`
  uses, so both modes read the same file (`~/Library/Application Support/com.inky.app/config.json` on macOS).

Operations (all take library-relative *or* absolute paths; `resolve()` joins
onto the root and applies today's `guard` canonicalisation, so nothing can
escape the root):

| Method | Notes |
| --- | --- |
| `tree()` | today's `build_tree` |
| `walk()` | flat listing `{rel_path, kind: Folder/Markdown/Mermaid}` for MCP list/search/resources |
| `read(path)` | |
| `write(path, content)` | creates parent folders; snapshots only when content actually changes |
| `patch(path, old, new)` | exact-once replacement; errors on 0 or >1 matches |
| `list_versions(path)` / `read_version(path, name)` | today's Rust logic |
| `search(query)` | today's Rust logic, cap 300 hits |
| `ensure_folder(path)` | `mkdir -p`, idempotent (MCP `create_folder`) |
| `create_folder_unique(dir, name)` / `create_doc_unique(...)` | today's app semantics (`name 2.md`) |
| `rename(path, new_name)` | keeps the *original* extension when none is given; sidecar follows |
| `move_into(path, target_dir)` | creates the target folder; picks a unique name on collision; sidecar follows |
| `delete(path)` | moves document **and** sidecar to the Trash |
| `threads(path)` / `save_threads(path, Vec<Thread>)` | typed comment model, same sidecar JSON as the frontend (`{version:1, threads:[…]}`) |
| `mtime(path)` | |

`Thread`/`Msg` structs mirror `src/lib/comments.ts` exactly (`id, quote,
prefix, suffix, resolved, createdAt, comments[{id, text, createdAt, author?}]`).

Tauri commands in `lib.rs` become one-liners: `Library::from_app(&app)?.write(&path, &content)`.
The frontend API (command names, arguments, return shapes) does not change.

### `mcp.rs` — the MCP surface

`rmcp = "3.2"` with features `server`, `macros`, `transport-io`,
`transport-streamable-http-server`; `axum` to mount the tower service; `schemars` for
parameter schemas. `InkyMcp { library: Library }` implements `ServerHandler` via
`#[tool_router]`/`#[tool_handler]`.

Tools — same names, parameter names, descriptions and result text as
`mcp/server.mjs` so existing agent prompts keep working:

`list_documents`, `read_document`, `write_document`, `patch_document`,
`list_versions`, `read_version`, `rename_document`, `move_document`,
`create_folder`, `delete_document`, `search_documents`, `list_comments`,
`create_comment`, `reply_to_comment`, `resolve_comment`.

Resources — `inky://doc/{path}` for every document (list + read, `text/markdown`),
so clients can @-mention documents, as today.

Errors are returned as tool errors (`isError: true`) with the same messages the
JS server used ("old_text not found — the document may have changed; re-read it
first.", "Path escapes the Inky library", …).

Transports:

- `serve_stdio()` — `serve_server(InkyMcp::new(Library::from_env()?), (stdin, stdout))`.
  Nothing else may write to stdout in this mode; status text goes to stderr.
- `serve_http(port) -> Result<HttpHandle>` — binds `127.0.0.1:port`
  (an `EADDRINUSE` becomes the error "port 26317 is already in use"), mounts
  `StreamableHttpService` at `/mcp` and runs `axum::serve` on
  `tauri::async_runtime` with graceful shutdown on a `CancellationToken`.
  Stateless mode (`stateful_mode: false`) to match today's server if the SDK
  version supports it; otherwise `LocalSessionManager`.

### Process entry (`main.rs`)

```rust
fn main() {
    if std::env::args().any(|a| a == "--mcp") {
        return inky_lib::mcp::serve_stdio_blocking(); // own tokio runtime, exits when stdin closes
    }
    inky_lib::run()
}
```

Clients register `/Applications/Inky.app/Contents/MacOS/Inky --mcp`. Running the
binary directly (not via `open`) does not trigger the single-instance plugin
or create a window, because `tauri::Builder` is never built in this branch.

### App-hosted server (`lib.rs`)

`McpProc(Mutex<Option<Child>>)` → `McpServer(Mutex<Option<HttpHandle>>)`.
`start_mcp(port)`, `stop_mcp()`, `mcp_status()` keep their names and
signatures, so `state.svelte.ts` and `Toolbar.svelte` are untouched except for
copy. `find_node`, the resource-dir lookup and the dev-tree fallback are
deleted. The `RunEvent::Exit` hook cancels the token instead of killing a child.

### "Connect an agent" help

Today's success toast offers only *Copy setup command* for Claude Code over
HTTP. Friends on Claude Desktop or Codex need the stdio path, which they will
not guess. Replace the toast action with a small dialog (`McpSetupDialog.svelte`,
opened from the toast and from the status light's context/right-click) showing
copyable snippets, each pre-filled with the real binary path
(new command `app_binary_path`) and the live port:

- Claude Code (HTTP): `claude mcp add --transport http inky http://127.0.0.1:26317/mcp`
- Claude Code (stdio): `claude mcp add inky -- "/Applications/Inky.app/Contents/MacOS/Inky" --mcp`
- Claude Desktop: JSON block for `claude_desktop_config.json`
- Codex: TOML block for `~/.codex/config.toml`
- A one-line note that ChatGPT connectors need a public URL and are not supported.

## Behaviour reconciliation (JS vs Rust today → one rule)

| Operation | JS server | Rust app | Chosen |
| --- | --- | --- | --- |
| Delete | permanent `unlink`, sidecar left behind | Trash, sidecar trashed | **Trash both** |
| Write snapshot | always (rate-limited) | only when content changed | **only when changed** |
| Snapshot file name | `stem.2026-09-01-10-36-00.md` | `stem.1756716960.md` | **unix seconds** — both match the `stem.*.md` filter, old files stay readable |
| Rename without extension | keep original ext | append `.md` | **keep original ext** |
| Move onto existing name | error | unique name | **unique name**, result reports the final path |
| Search cap | 200 | 300 | **300** |
| `create_folder` (MCP) | `mkdir -p` on the given path | n/a | unchanged (`ensure_folder`) |

## Build, packaging, repo hygiene

Removed: `mcp/` directory, `tests/mcp.test.ts`, `pnpm mcp` / `mcp:bundle` scripts,
`bin` entry, deps `@modelcontextprotocol/sdk`, `zod`, `esbuild` (none used
elsewhere in `src/`), `bundle.resources` in `tauri.conf.json`, the
`pnpm mcp:bundle &&` prefixes in `beforeDevCommand`/`beforeBuildCommand`, the
`server.bundle.mjs` gitignore line, `make mcp`.

Added: Cargo deps `rmcp`, `axum`, `tokio` (already transitive; `rt-multi-thread`
+ `io-std`), `tokio-util` (`CancellationToken`), `schemars`, `dirs`.

`.mcp.json` (this repo's Claude Code registration) becomes the HTTP server of
the running app: `{"inky": {"type": "http", "url": "http://127.0.0.1:26317/mcp"}}`.

`make mcp` becomes `cargo run --manifest-path src-tauri/Cargo.toml -- --mcp`.

CI `check.yml` adds `cargo test` in `src-tauri`.

## Testing

- `library.rs` unit tests against a temp dir: escape rejection (`../`, absolute
  outside root, symlink), write creates folders and snapshots only on change,
  patch 0/1/2 occurrences, rename keeps extension and moves the sidecar, move
  picks unique names, delete trashes sidecar (trash is stubbed to a rename in
  tests via `cfg(test)`), search cap, version name validation, thread
  round-trip byte-compatible with a sidecar written by the frontend.
- `mcp.rs` integration test: start `InkyMcp` on an in-process
  `tokio::io::duplex` pair with an `rmcp` client (feature `client` as a
  dev-dependency), run `initialize`, `tools/list` (asserts the 15 names),
  `resources/list`, and the same scenarios `tests/mcp.test.ts` covered today
  (write/read, escape → error, bad extension → error, ambiguous patch → error,
  comments create/list/reply/resolve).
- Manual: `make dmg`, install on a machine without Node (or `PATH=/usr/bin`),
  toggle the status light → green; `claude mcp add` both variants; Claude
  Desktop config with the `.app` path; confirm `Inky --mcp` exits when the
  client closes stdin and never opens a window.

## Docs

README "MCP server" section rewritten around the four snippets above plus the
ChatGPT caveat; the *Features* bullet drops "no terminal needed" wording that
implied a runtime.

Memory note (`inky-project.md`) updated: MCP is now Rust in-process; no Node
needed at runtime; stdio via `Inky --mcp`.
