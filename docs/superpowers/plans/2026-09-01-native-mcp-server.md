# Native MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Node.js MCP subprocess with an in-process Rust MCP server (HTTP + stdio) built on a single `Library` module that the desktop app's own commands also use.

**Architecture:** `src-tauri/src/library.rs` holds every filesystem operation on the library with no Tauri types; `src-tauri/src/mcp.rs` wraps it in an `rmcp` `ServerHandler` (15 tools + `inky://doc/…` resources) and provides `serve_stdio()` / `serve_http()`; `lib.rs` keeps thin `#[tauri::command]` wrappers and hosts the HTTP server behind the existing `start_mcp`/`stop_mcp`/`mcp_status` commands; `main.rs` routes `Inky --mcp` to stdio mode without booting Tauri.

**Tech Stack:** Rust (Tauri 2), `rmcp 3.2` (`server`, `macros`, `transport-io`, `transport-streamable-http-server`), `axum 0.8`, `tokio`, `tokio-util` (CancellationToken), `schemars 1`, `dirs 6`, `serde`; Svelte 5 + shadcn-svelte `Dialog` for the setup dialog.

**Spec:** `docs/superpowers/specs/2026-09-01-native-mcp-server-design.md`

## Global Constraints

- Tool names, parameter names, descriptions and result strings must match `mcp/server.mjs` verbatim (existing agent prompts depend on them). The 15 tools: `list_documents, read_document, write_document, patch_document, list_versions, read_version, rename_document, move_document, create_folder, delete_document, search_documents, list_comments, create_comment, reply_to_comment, resolve_comment`.
- Tauri command names, argument names and return shapes in `lib.rs` do not change (the frontend is untouched except for the setup dialog).
- Comment sidecar JSON shape is exactly `{"version":1,"threads":[{id,quote,prefix,suffix,resolved,createdAt,comments:[{id,text,createdAt,author?}]}]}` — byte-compatible with `src/lib/comments.ts`.
- Config file: `dirs::config_dir()/com.inky.app/config.json`, key `library`. Env override `INKY_LIBRARY`. Default `~/Documents/Inky`.
- Stdio mode must never write anything but JSON-RPC to stdout.
- HTTP server binds `127.0.0.1` only, path `/mcp`, default port `26317`.
- Behaviour rules (spec table): delete → Trash incl. sidecar; snapshot only when content changed; rename without extension keeps the original extension; move onto an existing name picks a unique name; search cap 300.
- Use `pnpm` for JS, never `npm`.
- Verified API facts (probe compiled against rmcp 3.2.0): `CallToolResult::success/error(Vec<ContentBlock>)`, `ContentBlock::text`, `ServerInfo::new(caps).with_server_info(Implementation::new(..)).with_instructions(..)`, `ListResourcesResult::with_all_items(Vec<Resource>)`, `ListResourceTemplatesResult::with_all_items(Vec<ResourceTemplate>)`, `Resource::new(uri,name)` with pub `mime_type`, `ResourceTemplate::new(tpl,name)`, `ResourceContents::text(text, uri)` (enum variant `TextResourceContents { mime_type, .. }`), `ReadResourceResult::new(vec).into()` → `ReadResourceResponse`, `McpError::resource_not_found(msg, None)`, `CallToolRequestParams::new(name).with_arguments(JsonObject)`, `ReadResourceRequestParams::new(uri)`, `StreamableHttpService::new(factory, LocalSessionManager::default().into(), StreamableHttpServerConfig::default().with_cancellation_token(ct))`, `axum::Router::new().nest_service("/mcp", service)`, `().serve(transport)` for a bare client, `client.list_tools(None)`, `client.call_tool(..)`, `client.read_resource(..)`, `client.cancel()`.

---

### Task 1: Cargo dependencies and `Library` core with path guarding

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/library.rs`
- Modify: `src-tauri/src/lib.rs:1-5` (add `pub mod library;`)

**Interfaces:**
- Produces: `library::Library` with `open(root) -> Result<Library,String>`, `from_env() -> Result<Library,String>`, `root() -> &Path`, `resolve(&str) -> Result<PathBuf,String>`, `relative(&Path) -> String`, `is_doc(&Path) -> bool`, `config_file_path() -> Option<PathBuf>`, consts `DOC_EXTENSIONS`, `APP_IDENTIFIER`.

- [ ] **Step 1: Add dependencies**

In `src-tauri/Cargo.toml` `[dependencies]` add:

```toml
rmcp = { version = "3.2", features = ["server", "macros", "transport-io", "transport-streamable-http-server"] }
axum = "0.8"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-std", "io-util"] }
tokio-util = "0.7"
schemars = "1"
dirs = "6"
```

and a new section:

```toml
[dev-dependencies]
rmcp = { version = "3.2", features = ["client"] }
tempfile = "3"
```

- [ ] **Step 2: Write the failing tests** (bottom of the new `src-tauri/src/library.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp_lib() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        (dir, lib)
    }

    #[test]
    fn resolve_accepts_relative_and_absolute_inside_root() {
        let (_d, lib) = temp_lib();
        let rel = lib.resolve("Notes/a.md").unwrap();
        assert_eq!(rel, lib.root().join("Notes/a.md"));
        let abs = lib.resolve(lib.root().join("b.md").to_str().unwrap()).unwrap();
        assert_eq!(abs, lib.root().join("b.md"));
        assert_eq!(lib.resolve("").unwrap(), lib.root());
        assert_eq!(lib.resolve(".").unwrap(), lib.root());
    }

    #[test]
    fn resolve_rejects_escapes() {
        let (_d, lib) = temp_lib();
        assert!(lib.resolve("../outside.md").is_err());
        assert!(lib.resolve("new/../../outside.md").is_err(), "dot-dot through a missing folder");
        assert!(lib.resolve("/etc/passwd").is_err());
    }

    #[test]
    fn resolve_rejects_symlink_escape() {
        let (_d, lib) = temp_lib();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), lib.root().join("link")).unwrap();
        assert!(lib.resolve("link/x.md").is_err());
    }

    #[test]
    fn relative_strips_root() {
        let (_d, lib) = temp_lib();
        assert_eq!(lib.relative(&lib.root().join("Notes/a.md")), "Notes/a.md");
    }

    #[test]
    fn from_env_honours_inky_library() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("INKY_LIBRARY", dir.path());
        let lib = Library::from_env().unwrap();
        std::env::remove_var("INKY_LIBRARY");
        assert_eq!(lib.root(), dir.path().canonicalize().unwrap());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test library:: 2>&1 | tail -20`
Expected: compile error — `Library` not defined.

- [ ] **Step 4: Implement `library.rs` core**

```rust
//! Everything the app and the MCP server do to the library folder lives here.
//! No Tauri types: this module also runs in `Inky --mcp` (stdio) mode where
//! there is no app handle.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const DOC_EXTENSIONS: [&str; 3] = ["md", "markdown", "mmd"];
pub const APP_IDENTIFIER: &str = "com.inky.app";

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub library: Option<String>,
}

/// `~/Library/Application Support/com.inky.app/config.json` on macOS — the same
/// file Tauri's `app_config_dir()` resolves to, so app and stdio mode agree.
pub fn config_file_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_IDENTIFIER).join("config.json"))
}

pub fn is_doc(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| DOC_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    /// Open (creating if needed) a library folder. The root is canonicalised
    /// once so every later containment check compares real paths.
    pub fn open(root: impl Into<PathBuf>) -> Result<Library, String> {
        let root: PathBuf = root.into();
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        Ok(Library { root })
    }

    /// Resolution used when no app handle exists (stdio mode):
    /// `INKY_LIBRARY` → config file → `~/Documents/Inky`.
    pub fn from_env() -> Result<Library, String> {
        if let Some(p) = std::env::var_os("INKY_LIBRARY").filter(|p| !p.is_empty()) {
            return Library::open(PathBuf::from(p));
        }
        let configured = config_file_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
            .and_then(|c| c.library)
            .filter(|p| !p.is_empty());
        if let Some(p) = configured {
            return Library::open(p);
        }
        let docs = dirs::document_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| "cannot determine the home folder".to_string())?;
        Library::open(docs.join("Inky"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Library-relative form of an absolute path inside the root ("" for the root).
    pub fn relative(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| abs.to_string_lossy().into_owned())
    }

    /// Resolve a relative-or-absolute path and refuse anything that escapes
    /// the root — through `..`, absolute paths, or symlinks. Works for paths
    /// that don't exist yet by canonicalising the deepest existing ancestor.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let candidate = self.root.join(path);
        let mut existing = candidate.clone();
        let mut suffix: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            match existing.file_name() {
                Some(name) => suffix.push(name.to_os_string()),
                None => return Err("invalid path".into()),
            }
            existing = existing
                .parent()
                .ok_or_else(|| "invalid path".to_string())?
                .to_path_buf();
        }
        let mut resolved = existing.canonicalize().map_err(|e| e.to_string())?;
        for part in suffix.iter().rev() {
            match Path::new(part).components().next() {
                Some(Component::ParentDir) => {
                    resolved.pop();
                }
                Some(Component::CurDir) | None => {}
                _ => resolved.push(part),
            }
        }
        if resolved.starts_with(&self.root) {
            Ok(resolved)
        } else {
            Err(format!("Path escapes the Inky library: {path}"))
        }
    }
}
```

Then add at the top of `src-tauri/src/lib.rs` (after the `use` lines): `pub mod library;`

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test library:: 2>&1 | tail -20`
Expected: `test result: ok. 5 passed`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/library.rs src-tauri/src/lib.rs
git commit -m "Add Library core with path guarding (no Tauri types)"
```

---

### Task 2: `Library` documents — tree, walk, read, write, patch, snapshots, mtime

**Files:**
- Modify: `src-tauri/src/library.rs`

**Interfaces:**
- Produces: `Node { name, path, is_dir, children }` (serde camelCase, absolute `path`), `Entry { rel: String, kind: EntryKind }`, `EntryKind::{Folder, Markdown, Mermaid}`, `Library::tree() -> Vec<Node>`, `walk() -> Vec<Entry>`, `read(&str) -> Result<String,String>`, `write(&str, &str) -> Result<(),String>`, `patch(&str, &str, &str) -> Result<(),String>`, `mtime(&str) -> Result<u64,String>`, consts `HISTORY_DIR`, `SNAPSHOT_MIN_INTERVAL_SECS`, `SNAPSHOT_KEEP`.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests`)

```rust
    #[test]
    fn write_creates_folders_and_read_returns_content() {
        let (_d, lib) = temp_lib();
        lib.write("a/b/c.md", "# Hi\n").unwrap();
        assert_eq!(lib.read("a/b/c.md").unwrap(), "# Hi\n");
    }

    #[test]
    fn walk_lists_sorted_relative_entries_and_skips_dotfiles() {
        let (_d, lib) = temp_lib();
        lib.write("z.md", "").unwrap();
        lib.write("Notes/a.md", "").unwrap();
        lib.write("Notes/d.mmd", "").unwrap();
        fs::write(lib.root().join(".hidden.md"), "").unwrap();
        let got: Vec<(String, EntryKind)> = lib.walk().into_iter().map(|e| (e.rel, e.kind)).collect();
        assert_eq!(
            got,
            vec![
                ("Notes/".to_string(), EntryKind::Folder),
                ("Notes/a.md".to_string(), EntryKind::Markdown),
                ("Notes/d.mmd".to_string(), EntryKind::Mermaid),
                ("z.md".to_string(), EntryKind::Markdown),
            ]
        );
    }

    #[test]
    fn tree_puts_folders_first_with_absolute_paths() {
        let (_d, lib) = temp_lib();
        lib.write("b.md", "").unwrap();
        lib.write("A/x.md", "").unwrap();
        let tree = lib.tree();
        assert_eq!(tree[0].name, "A");
        assert!(tree[0].is_dir);
        assert_eq!(tree[0].children[0].path, lib.root().join("A/x.md").to_string_lossy());
        assert_eq!(tree[1].name, "b.md");
    }

    #[test]
    fn write_snapshots_only_when_content_changes() {
        let (_d, lib) = temp_lib();
        lib.write("n.md", "v1").unwrap();
        lib.write("n.md", "v1").unwrap();
        assert!(!lib.root().join(HISTORY_DIR).exists(), "identical rewrite must not snapshot");
        lib.write("n.md", "v2").unwrap();
        let names: Vec<String> = fs::read_dir(lib.root().join(HISTORY_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].starts_with("n.") && names[0].ends_with(".md"));
        assert_eq!(fs::read_to_string(lib.root().join(HISTORY_DIR).join(&names[0])).unwrap(), "v1");
    }

    #[test]
    fn patch_requires_exactly_one_match() {
        let (_d, lib) = temp_lib();
        lib.write("p.md", "alpha beta alpha\n").unwrap();
        let err = lib.patch("p.md", "alpha", "x").unwrap_err();
        assert!(err.contains("occurs 2 times"), "{err}");
        let err = lib.patch("p.md", "zzz", "x").unwrap_err();
        assert!(err.contains("old_text not found"), "{err}");
        lib.patch("p.md", "beta", "gamma").unwrap();
        assert_eq!(lib.read("p.md").unwrap(), "alpha gamma alpha\n");
    }

    #[test]
    fn mtime_is_epoch_millis() {
        let (_d, lib) = temp_lib();
        lib.write("m.md", "").unwrap();
        assert!(lib.mtime("m.md").unwrap() > 1_600_000_000_000);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test library:: 2>&1 | grep -E "^error" | head -5`
Expected: errors about missing `write`, `walk`, `EntryKind`, …

- [ ] **Step 3: Implement**

Add above `impl Library` in `library.rs`:

```rust
pub const HISTORY_DIR: &str = ".inky-history";
pub const SNAPSHOT_MIN_INTERVAL_SECS: u64 = 10 * 60;
pub const SNAPSHOT_KEEP: usize = 20;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    Markdown,
    Mermaid,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub rel: String,
    pub kind: EntryKind,
}

fn build_tree(dir: &Path) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return nodes;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            let children = build_tree(&path);
            // Image-attachment folders with no documents inside are noise.
            if name.eq_ignore_ascii_case("assets") && children.is_empty() {
                continue;
            }
            nodes.push(Node { name, path: path.to_string_lossy().into_owned(), is_dir: true, children });
        } else if is_doc(&path) {
            nodes.push(Node { name, path: path.to_string_lossy().into_owned(), is_dir: false, children: Vec::new() });
        }
    }
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

fn walk_into(dir: &Path, root: &Path, out: &mut Vec<Entry>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
        if path.is_dir() {
            out.push(Entry { rel: format!("{rel}/"), kind: EntryKind::Folder });
            walk_into(&path, root, out);
        } else if is_doc(&path) {
            let kind = if name.to_lowercase().ends_with(".mmd") { EntryKind::Mermaid } else { EntryKind::Markdown };
            out.push(Entry { rel, kind });
        }
    }
}

/// `(parent, stem, ext)` of a document path.
fn doc_parts(doc: &Path) -> Result<(&Path, &str, &str), String> {
    let parent = doc.parent().ok_or("no parent")?;
    let stem = doc.file_stem().and_then(|s| s.to_str()).ok_or("invalid name")?;
    let ext = doc.extension().and_then(|e| e.to_str()).ok_or("invalid extension")?;
    Ok((parent, stem, ext))
}

/// History files belonging to `doc`, sorted by name (`stem.<unix-secs>.ext`).
fn history_files(doc: &Path) -> Vec<PathBuf> {
    let Ok((parent, stem, ext)) = doc_parts(doc) else {
        return Vec::new();
    };
    let prefix = format!("{stem}.");
    let suffix = format!(".{ext}");
    let mut mine: Vec<PathBuf> = fs::read_dir(parent.join(HISTORY_DIR))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&prefix) && n.ends_with(&suffix))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    mine.sort();
    mine
}

/// Before overwriting a document, keep the old version in a hidden history
/// folder next to it — at most one snapshot per 10 minutes, last 20 kept.
fn snapshot(doc: &Path) {
    let Ok(old) = fs::read_to_string(doc) else {
        return;
    };
    let Ok((parent, stem, ext)) = doc_parts(doc) else {
        return;
    };
    let dir = parent.join(HISTORY_DIR);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mine = history_files(doc);
    if let Some(last) = mine.last() {
        if let Ok(modified) = fs::metadata(last).and_then(|m| m.modified()) {
            if modified.elapsed().map(|e| e.as_secs()).unwrap_or(u64::MAX) < SNAPSHOT_MIN_INTERVAL_SECS {
                return;
            }
        }
    }
    let ts = now_secs();
    let _ = fs::write(dir.join(format!("{stem}.{ts}.{ext}")), old);
    if mine.len() >= SNAPSHOT_KEEP {
        for stale in &mine[..mine.len() + 1 - SNAPSHOT_KEEP] {
            let _ = fs::remove_file(stale);
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
```

Add inside `impl Library`:

```rust
    pub fn tree(&self) -> Vec<Node> {
        build_tree(&self.root)
    }

    /// Flat, sorted listing of every folder and document (relative paths).
    pub fn walk(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        walk_into(&self.root, &self.root, &mut out);
        out
    }

    pub fn read(&self, path: &str) -> Result<String, String> {
        let p = self.resolve(path)?;
        fs::read_to_string(p).map_err(|e| e.to_string())
    }

    /// Create or overwrite a document (parent folders are created). The old
    /// version is snapshotted only when the content actually changes.
    pub fn write(&self, path: &str, content: &str) -> Result<(), String> {
        let p = self.resolve(path)?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if fs::read_to_string(&p).map(|old| old != content).unwrap_or(false) {
            snapshot(&p);
        }
        fs::write(p, content).map_err(|e| e.to_string())
    }

    /// Replace `old_text` (which must occur exactly once) with `new_text`.
    pub fn patch(&self, path: &str, old_text: &str, new_text: &str) -> Result<(), String> {
        let p = self.resolve(path)?;
        let content = fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let count = if old_text.is_empty() { 0 } else { content.matches(old_text).count() };
        if count == 0 {
            return Err("old_text not found — the document may have changed; re-read it first.".into());
        }
        if count > 1 {
            return Err(format!("old_text occurs {count} times — include more surrounding context."));
        }
        snapshot(&p);
        fs::write(p, content.replacen(old_text, new_text, 1)).map_err(|e| e.to_string())
    }

    /// Last-modified time in epoch milliseconds.
    pub fn mtime(&self, path: &str) -> Result<u64, String> {
        let p = self.resolve(path)?;
        let meta = fs::metadata(p).map_err(|e| e.to_string())?;
        meta.modified()
            .map_err(|e| e.to_string())?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .map_err(|e| e.to_string())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test library:: 2>&1 | tail -5`
Expected: `test result: ok. 11 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/library.rs
git commit -m "Library: tree, walk, read, write, patch, snapshots"
```

---

### Task 3: `Library` versions and search

**Files:**
- Modify: `src-tauri/src/library.rs`

**Interfaces:**
- Produces: `VersionInfo { name, modified_ms, size }` (serde camelCase), `SearchHit { path, name, line, text }` (serde camelCase, absolute `path`), `Library::list_versions(&str) -> Result<Vec<VersionInfo>,String>`, `read_version(&str, &str) -> Result<String,String>`, `search(&str) -> Vec<SearchHit>`, const `SEARCH_CAP = 300`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn versions_are_listed_newest_first_and_readable() {
        let (_d, lib) = temp_lib();
        lib.write("v.md", "one").unwrap();
        let dir = lib.root().join(HISTORY_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("v.1000.md"), "old").unwrap();
        fs::write(dir.join("v.2000.md"), "newer").unwrap();
        fs::write(dir.join("other.2000.md"), "not mine").unwrap();
        let versions = lib.list_versions("v.md").unwrap();
        assert_eq!(versions.len(), 2);
        assert!(versions.iter().all(|v| v.name.starts_with("v.")));
        assert!(versions[0].modified_ms >= versions[1].modified_ms);
        assert_eq!(lib.read_version("v.md", "v.1000.md").unwrap(), "old");
        assert!(lib.read_version("v.md", "other.2000.md").is_err());
        assert!(lib.read_version("v.md", "../v.md").is_err());
    }

    #[test]
    fn search_is_case_insensitive_and_reports_lines() {
        let (_d, lib) = temp_lib();
        lib.write("a.md", "Hello\nworld\n").unwrap();
        lib.write("sub/b.md", "HELLO again\n").unwrap();
        let hits = lib.search("hello");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "a.md");
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].text, "Hello");
        assert_eq!(hits[1].path, lib.root().join("sub/b.md").to_string_lossy());
        assert!(lib.search("   ").is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test library:: 2>&1 | grep -E "^error" | head -3`
Expected: missing `list_versions` / `search`.

- [ ] **Step 3: Implement**

Add types near `Node`:

```rust
pub const SEARCH_CAP: usize = 300;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub name: String,
    pub modified_ms: u64,
    pub size: u64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub line: u32,
    pub text: String,
}
```

Add inside `impl Library`:

```rust
    pub fn list_versions(&self, path: &str) -> Result<Vec<VersionInfo>, String> {
        let doc = self.resolve(path)?;
        doc_parts(&doc)?;
        let mut out: Vec<VersionInfo> = history_files(&doc)
            .into_iter()
            .filter_map(|p| {
                let meta = fs::metadata(&p).ok()?;
                let modified_ms = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                Some(VersionInfo {
                    name: p.file_name()?.to_string_lossy().into_owned(),
                    modified_ms,
                    size: meta.len(),
                })
            })
            .collect();
        out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| b.name.cmp(&a.name)));
        Ok(out)
    }

    pub fn read_version(&self, path: &str, version: &str) -> Result<String, String> {
        let doc = self.resolve(path)?;
        let (parent, stem, ext) = doc_parts(&doc)?;
        if version.contains('/') || !version.starts_with(&format!("{stem}.")) || !version.ends_with(&format!(".{ext}")) {
            return Err("invalid version name".into());
        }
        fs::read_to_string(parent.join(HISTORY_DIR).join(version)).map_err(|e| e.to_string())
    }

    /// Case-insensitive full-text search; at most `SEARCH_CAP` hits.
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let needle = query.to_lowercase();
        let mut hits = Vec::new();
        if needle.trim().is_empty() {
            return hits;
        }
        'outer: for entry in self.walk() {
            if entry.kind == EntryKind::Folder {
                continue;
            }
            let file = self.root.join(&entry.rel);
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
            for (i, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push(SearchHit {
                        path: file.to_string_lossy().into_owned(),
                        name: name.clone(),
                        line: (i + 1) as u32,
                        text: line.trim().chars().take(200).collect(),
                    });
                    if hits.len() >= SEARCH_CAP {
                        break 'outer;
                    }
                }
            }
        }
        hits
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test library:: 2>&1 | tail -5`
Expected: `test result: ok. 13 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/library.rs
git commit -m "Library: version history and search"
```

---

### Task 4: `Library` structure ops — folders, rename, move, delete

**Files:**
- Modify: `src-tauri/src/library.rs`

**Interfaces:**
- Produces: `Library::ensure_folder(&str) -> Result<PathBuf,String>`, `create_folder_unique(&str dir, &str name) -> Result<PathBuf,String>`, `create_doc_unique(&str dir, &str name, &str ext, &str content) -> Result<PathBuf,String>`, `rename(&str, &str) -> Result<PathBuf,String>`, `move_into(&str, &str) -> Result<PathBuf,String>`, `delete(&str) -> Result<(),String>`, `sidecar_for(&Path) -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn ensure_folder_is_idempotent_and_unique_variants_number() {
        let (_d, lib) = temp_lib();
        let a = lib.ensure_folder("Projects/Inky").unwrap();
        assert!(a.is_dir());
        assert_eq!(lib.ensure_folder("Projects/Inky").unwrap(), a);
        let f1 = lib.create_folder_unique("", "Ideas").unwrap();
        let f2 = lib.create_folder_unique("", "Ideas").unwrap();
        assert_eq!(f2.file_name().unwrap(), "Ideas 2");
        assert_ne!(f1, f2);
        let d1 = lib.create_doc_unique("", "Note", "md", "x").unwrap();
        let d2 = lib.create_doc_unique("", "Note.md", "md", "y").unwrap();
        assert_eq!(d1.file_name().unwrap(), "Note.md");
        assert_eq!(d2.file_name().unwrap(), "Note 2.md");
        assert!(lib.create_doc_unique("", "bad", "sh", "").is_err());
    }

    #[test]
    fn rename_keeps_original_extension_and_moves_sidecar() {
        let (_d, lib) = temp_lib();
        lib.write("d.mmd", "graph TD").unwrap();
        fs::write(lib.root().join(".d.mmd.comments.json"), "{}").unwrap();
        let target = lib.rename("d.mmd", "diagram").unwrap();
        assert_eq!(target.file_name().unwrap(), "diagram.mmd");
        assert!(lib.root().join(".diagram.mmd.comments.json").exists());
        assert!(!lib.root().join(".d.mmd.comments.json").exists());
        lib.write("e.md", "").unwrap();
        assert!(lib.rename("e.md", "diagram.mmd").is_err(), "must not overwrite");
        assert!(lib.rename("e.md", "a/b").is_err(), "no slashes");
    }

    #[test]
    fn move_into_creates_target_and_picks_unique_name() {
        let (_d, lib) = temp_lib();
        lib.write("m.md", "1").unwrap();
        fs::write(lib.root().join(".m.md.comments.json"), "{}").unwrap();
        let t = lib.move_into("m.md", "Archive").unwrap();
        assert_eq!(lib.relative(&t), "Archive/m.md");
        assert!(lib.root().join("Archive/.m.md.comments.json").exists());
        lib.write("m.md", "2").unwrap();
        let t2 = lib.move_into("m.md", "Archive").unwrap();
        assert_eq!(lib.relative(&t2), "Archive/m 2.md");
        lib.ensure_folder("F/G").unwrap();
        assert!(lib.move_into("F", "F/G").is_err(), "cannot move a folder into itself");
    }

    #[test]
    fn delete_removes_document_and_sidecar() {
        let (_d, lib) = temp_lib();
        lib.write("x.md", "").unwrap();
        fs::write(lib.root().join(".x.md.comments.json"), "{}").unwrap();
        lib.delete("x.md").unwrap();
        assert!(!lib.root().join("x.md").exists());
        assert!(!lib.root().join(".x.md.comments.json").exists());
        assert!(lib.delete("x.md").is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test library:: 2>&1 | grep -E "^error" | head -3`

- [ ] **Step 3: Implement**

Free functions:

```rust
/// Hidden sidecar file holding a document's comment threads.
pub fn sidecar_for(doc: &Path) -> Option<PathBuf> {
    let name = doc.file_name()?.to_str()?;
    Some(doc.parent()?.join(format!(".{name}.comments.json")))
}

fn move_sidecar(from: &Path, to: &Path) {
    if let (Some(old), Some(new)) = (sidecar_for(from), sidecar_for(to)) {
        if old.exists() {
            let _ = fs::rename(old, new);
        }
    }
}

/// Pick "name.md", "name 2.md", ... — first one that doesn't exist yet.
fn unique_path(dir: &Path, stem: &str, ext: Option<&str>) -> PathBuf {
    for i in 1u32.. {
        let candidate = if i == 1 { stem.to_string() } else { format!("{stem} {i}") };
        let full = match ext {
            Some(e) => dir.join(format!("{candidate}.{e}")),
            None => dir.join(candidate),
        };
        if !full.exists() {
            return full;
        }
    }
    unreachable!()
}

/// The app deletes to the Trash; tests remove for real so they don't litter it.
#[cfg(not(test))]
fn remove(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}
#[cfg(test)]
fn remove(path: &Path) -> Result<(), String> {
    if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }.map_err(|e| e.to_string())
}
```

Inside `impl Library`:

```rust
    /// `mkdir -p` inside the library.
    pub fn ensure_folder(&self, path: &str) -> Result<PathBuf, String> {
        let p = self.resolve(path)?;
        fs::create_dir_all(&p).map_err(|e| e.to_string())?;
        Ok(p)
    }

    pub fn create_folder_unique(&self, dir: &str, name: &str) -> Result<PathBuf, String> {
        let d = self.resolve(dir)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("empty name".into());
        }
        let path = unique_path(&d, name, None);
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn create_doc_unique(&self, dir: &str, name: &str, ext: &str, content: &str) -> Result<PathBuf, String> {
        let d = self.resolve(dir)?;
        if !d.is_dir() {
            return Err("not a directory".into());
        }
        if !DOC_EXTENSIONS.contains(&ext) {
            return Err("unsupported extension".into());
        }
        let stem = name.trim().trim_end_matches(&format!(".{ext}")).to_string();
        let stem = if stem.is_empty() { "Untitled".to_string() } else { stem };
        let path = unique_path(&d, &stem, Some(ext));
        fs::write(&path, content).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// Rename in place. A document renamed without an extension keeps its own.
    pub fn rename(&self, path: &str, new_name: &str) -> Result<PathBuf, String> {
        let p = self.resolve(path)?;
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name.contains('/') {
            return Err("invalid name".into());
        }
        let parent = p.parent().ok_or("no parent")?;
        let mut target = parent.join(new_name);
        if p.is_file() && !is_doc(&target) {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("md");
            target = parent.join(format!("{new_name}.{ext}"));
        }
        if target.exists() {
            return Err(format!("{} already exists", target.file_name().unwrap_or_default().to_string_lossy()));
        }
        fs::rename(&p, &target).map_err(|e| e.to_string())?;
        move_sidecar(&p, &target);
        Ok(target)
    }

    /// Move a file or folder into another folder (created if missing). A name
    /// clash picks "name 2.md" rather than failing.
    pub fn move_into(&self, path: &str, target_dir: &str) -> Result<PathBuf, String> {
        let src = self.resolve(path)?;
        let dst_dir = self.ensure_folder(target_dir)?;
        if dst_dir == src || dst_dir.starts_with(&src) {
            return Err("cannot move a folder into itself".into());
        }
        let name = src.file_name().ok_or("invalid source")?;
        if src.parent() == Some(dst_dir.as_path()) {
            return Ok(src);
        }
        let mut target = dst_dir.join(name);
        if target.exists() {
            let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string();
            let ext = src.extension().and_then(|e| e.to_str()).map(str::to_string);
            target = unique_path(&dst_dir, &stem, ext.as_deref());
        }
        fs::rename(&src, &target).map_err(|e| e.to_string())?;
        move_sidecar(&src, &target);
        Ok(target)
    }

    /// Move a document or folder (and a document's sidecar) to the Trash.
    pub fn delete(&self, path: &str) -> Result<(), String> {
        let p = self.resolve(path)?;
        if !p.exists() {
            return Err(format!("{path} does not exist"));
        }
        remove(&p)?;
        if let Some(sc) = sidecar_for(&p) {
            if sc.exists() {
                let _ = remove(&sc);
            }
        }
        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test library:: 2>&1 | tail -5`
Expected: `test result: ok. 17 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/library.rs
git commit -m "Library: folders, rename, move, delete"
```

---

### Task 5: `Library` comment threads (typed, sidecar-compatible)

**Files:**
- Modify: `src-tauri/src/library.rs`

**Interfaces:**
- Produces: `CommentMsg { id, text, created_at, author: Option<String> }`, `CommentThread { id, quote, prefix, suffix, resolved, created_at, comments }` (both serde camelCase), `Library::threads(&str) -> Result<Vec<CommentThread>,String>`, `save_threads(&str, &[CommentThread]) -> Result<(),String>`, `raw_comments(&str) -> Result<String,String>`, `write_raw_comments(&str, &str) -> Result<(),String>`, `new_id(&str) -> String`, `iso_now() -> String`, const `CONTEXT_CHARS = 30`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn threads_round_trip_frontend_sidecar_format() {
        let (_d, lib) = temp_lib();
        lib.write("c.md", "hello world").unwrap();
        let frontend_json = r#"{
  "version": 1,
  "threads": [
    {
      "id": "thread-abc",
      "quote": "world",
      "prefix": "hello ",
      "suffix": "",
      "resolved": false,
      "createdAt": "2026-09-01T10:00:00.000Z",
      "comments": [
        { "id": "msg-1", "text": "why?", "createdAt": "2026-09-01T10:00:00.000Z" }
      ]
    }
  ]
}"#;
        fs::write(lib.root().join(".c.md.comments.json"), frontend_json).unwrap();
        let mut threads = lib.threads("c.md").unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].comments[0].author, None);
        threads[0].comments.push(CommentMsg {
            id: new_id("msg"),
            text: "because".into(),
            created_at: iso_now(),
            author: Some("Claude".into()),
        });
        lib.save_threads("c.md", &threads).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&lib.raw_comments("c.md").unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["threads"][0]["comments"][1]["author"], "Claude");
        assert!(raw["threads"][0]["comments"][0].get("author").is_none(), "absent, not null");
        lib.save_threads("c.md", &[]).unwrap();
        assert!(!lib.root().join(".c.md.comments.json").exists(), "empty list removes the sidecar");
        assert_eq!(lib.threads("c.md").unwrap().len(), 0);
    }

    #[test]
    fn ids_and_timestamps_look_like_the_frontends() {
        let a = new_id("thread");
        let b = new_id("thread");
        assert!(a.starts_with("thread-"));
        assert_ne!(a, b);
        let ts = iso_now();
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(ts.ends_with('Z') && &ts[10..11] == "T");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test library:: 2>&1 | grep -E "^error" | head -3`

- [ ] **Step 3: Implement**

```rust
pub const CONTEXT_CHARS: usize = 30;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentMsg {
    pub id: String,
    pub text: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentThread {
    pub id: String,
    pub quote: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub resolved: bool,
    pub created_at: String,
    #[serde(default)]
    pub comments: Vec<CommentMsg>,
}

#[derive(Serialize, Deserialize, Default)]
struct Sidecar {
    version: u32,
    #[serde(default)]
    threads: Vec<CommentThread>,
}

/// `prefix-<base36 millis>-<6 base36 chars>`, same shape as the frontend's ids.
pub fn new_id(prefix: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(now);
    let rand = base36(h.finish() as u128);
    format!("{prefix}-{}-{}", base36(now), &rand[rand.len().saturating_sub(6)..])
}

fn base36(mut n: u128) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SS.mmmZ` (what `Date#toISOString` emits).
pub fn iso_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    iso_from_unix_millis(d.as_millis() as u64)
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for an epoch-millisecond instant.
pub fn iso_from_unix_millis(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let millis = ms % 1000;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}
```

Inside `impl Library`:

```rust
    /// Raw sidecar JSON ("" when there is none) — the frontend owns the typed model.
    pub fn raw_comments(&self, path: &str) -> Result<String, String> {
        let doc = self.resolve(path)?;
        let Some(sc) = sidecar_for(&doc) else {
            return Ok(String::new());
        };
        Ok(fs::read_to_string(sc).unwrap_or_default())
    }

    pub fn write_raw_comments(&self, path: &str, json: &str) -> Result<(), String> {
        let doc = self.resolve(path)?;
        let sc = sidecar_for(&doc).ok_or("invalid document path")?;
        if json.is_empty() {
            if sc.exists() {
                fs::remove_file(sc).map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        fs::write(sc, json).map_err(|e| e.to_string())
    }

    pub fn threads(&self, path: &str) -> Result<Vec<CommentThread>, String> {
        let raw = self.raw_comments(path)?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str::<Sidecar>(&raw).map(|s| s.threads).unwrap_or_default())
    }

    /// Persist threads in the frontend's sidecar format; an empty list removes the file.
    pub fn save_threads(&self, path: &str, threads: &[CommentThread]) -> Result<(), String> {
        if threads.is_empty() {
            return self.write_raw_comments(path, "");
        }
        let json = serde_json::to_string_pretty(&Sidecar { version: 1, threads: threads.to_vec() })
            .map_err(|e| e.to_string())?;
        self.write_raw_comments(path, &json)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test library:: 2>&1 | tail -5`
Expected: `test result: ok. 19 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/library.rs
git commit -m "Library: typed comment threads, ids and ISO timestamps"
```

---

### Task 6: Route the Tauri commands through `Library`

**Files:**
- Modify: `src-tauri/src/lib.rs` (lines 1–570: helpers and commands)

**Interfaces:**
- Consumes: everything from Tasks 1–5.
- Produces: `fn open_library(app: &tauri::AppHandle) -> Result<Library, String>` (config-aware root resolution, writes the config back like `resolve_root` did). Command names/args/returns unchanged.

- [ ] **Step 1: Replace the helpers**

Delete from `lib.rs`: `Config`, `Node`, `DOC_EXTENSIONS`, `config_path`, `read_config`, `write_config`, `resolve_root`, `guard`, `HISTORY_DIR`/`SNAPSHOT_*`, `snapshot`, `sidecar_for`, `is_doc`, `build_tree`, `unique_path`, `VersionInfo`, `doc_parts`, `SearchHit`. Replace with:

```rust
use library::{Config, Library, Node, SearchHit, VersionInfo};

fn config_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("config.json"))
}

fn read_config(app: &tauri::AppHandle) -> Config {
    config_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_config(app: &tauri::AppHandle, config: &Config) -> Result<(), String> {
    let path = config_path(app)?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// The app's library: configured folder or `~/Documents/Inky`, persisted so
/// stdio-mode MCP (`Library::from_env`) sees the same choice.
fn open_library(app: &tauri::AppHandle) -> Result<Library, String> {
    let config = read_config(app);
    let root = match config.library {
        Some(ref p) if !p.is_empty() => PathBuf::from(p),
        _ => app
            .path()
            .document_dir()
            .or_else(|_| app.path().home_dir())
            .map_err(|e| e.to_string())?
            .join("Inky"),
    };
    let lib = Library::open(&root)?;
    let stored = root.to_string_lossy().into_owned();
    if config.library.as_deref() != Some(stored.as_str()) {
        let _ = write_config(app, &Config { library: Some(stored) });
    }
    Ok(lib)
}
```

- [ ] **Step 2: Rewrite the commands as thin wrappers**

```rust
#[tauri::command]
fn library_root(app: tauri::AppHandle) -> Result<String, String> {
    Ok(open_library(&app)?.root().to_string_lossy().into_owned())
}

#[tauri::command]
fn set_library_root(app: tauri::AppHandle, path: String) -> Result<String, String> {
    Library::open(&path)?;
    write_config(&app, &Config { library: Some(path.clone()) })?;
    Ok(path)
}

#[tauri::command]
fn list_tree(app: tauri::AppHandle) -> Result<Vec<Node>, String> {
    Ok(open_library(&app)?.tree())
}

#[tauri::command]
fn read_doc(app: tauri::AppHandle, path: String) -> Result<String, String> {
    open_library(&app)?.read(&path)
}

#[tauri::command]
fn write_doc(app: tauri::AppHandle, path: String, content: String) -> Result<(), String> {
    open_library(&app)?.write(&path, &content)
}

#[tauri::command]
fn list_versions(app: tauri::AppHandle, path: String) -> Result<Vec<VersionInfo>, String> {
    open_library(&app)?.list_versions(&path)
}

#[tauri::command]
fn read_version(app: tauri::AppHandle, path: String, version: String) -> Result<String, String> {
    open_library(&app)?.read_version(&path, &version)
}

#[tauri::command]
fn search_library(app: tauri::AppHandle, query: String) -> Result<Vec<SearchHit>, String> {
    Ok(open_library(&app)?.search(&query))
}

#[tauri::command]
fn create_doc(app: tauri::AppHandle, dir: String, name: String, ext: String, content: String) -> Result<String, String> {
    Ok(open_library(&app)?.create_doc_unique(&dir, &name, &ext, &content)?.to_string_lossy().into_owned())
}

#[tauri::command]
fn create_folder(app: tauri::AppHandle, dir: String, name: String) -> Result<String, String> {
    Ok(open_library(&app)?.create_folder_unique(&dir, &name)?.to_string_lossy().into_owned())
}

#[tauri::command]
fn rename_path(app: tauri::AppHandle, path: String, new_name: String) -> Result<String, String> {
    Ok(open_library(&app)?.rename(&path, &new_name)?.to_string_lossy().into_owned())
}

#[tauri::command]
fn read_comments(app: tauri::AppHandle, doc_path: String) -> Result<String, String> {
    open_library(&app)?.raw_comments(&doc_path)
}

#[tauri::command]
fn write_comments(app: tauri::AppHandle, doc_path: String, json: String) -> Result<(), String> {
    open_library(&app)?.write_raw_comments(&doc_path, &json)
}

#[tauri::command]
fn delete_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    open_library(&app)?.delete(&path)
}

#[tauri::command]
fn move_path(app: tauri::AppHandle, path: String, target_dir: String) -> Result<String, String> {
    Ok(open_library(&app)?.move_into(&path, &target_dir)?.to_string_lossy().into_owned())
}

#[tauri::command]
fn doc_mtime(app: tauri::AppHandle, path: String) -> Result<u64, String> {
    open_library(&app)?.mtime(&path)
}
```

`save_image` keeps its body but replaces `guard(&app, &doc_path)?` with `open_library(&app)?.resolve(&doc_path)?` and `unique_path` with `library::unique_path` (make that function `pub(crate)` in `library.rs`). `path_exists`, `quit_app`, `print_document`, `set_menu_checked`, `build_menu`, the MCP commands and `run()` are untouched in this task.

- [ ] **Step 3: Type-check and run all Rust tests**

Run: `cd src-tauri && cargo check 2>&1 | tail -3 && cargo test 2>&1 | tail -3`
Expected: no errors; `19 passed`.

- [ ] **Step 4: Smoke-test the app**

Run: `make dev` — open a document, edit, create a folder, rename, move, trash, add a comment, view history. Everything behaves as before. Stop the app.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/library.rs
git commit -m "Route Tauri commands through Library"
```

---

### Task 7: `mcp.rs` — tools and resources over `Library`

**Files:**
- Create: `src-tauri/src/mcp.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod mcp;`)

**Interfaces:**
- Consumes: `Library` API from Tasks 1–5.
- Produces: `mcp::InkyMcp::new(Library) -> InkyMcp` (Clone, implements `rmcp::ServerHandler`), `mcp::TOOL_SUMMARY: &str`.

- [ ] **Step 1: Write the failing test** (bottom of `mcp.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};
    use rmcp::ServiceExt;

    struct Fixture {
        _dir: tempfile::TempDir,
        lib: Library,
        client: rmcp::service::RunningService<rmcp::RoleClient, ()>,
        server: tokio::task::JoinHandle<()>,
    }

    async fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        let handler = InkyMcp::new(lib.clone());
        let server = tokio::spawn(async move {
            let s = handler.serve(server_io).await.unwrap();
            s.waiting().await.ok();
        });
        let client = ().serve(client_io).await.unwrap();
        Fixture { _dir: dir, lib, client, server }
    }

    async fn call(f: &Fixture, name: &str, args: serde_json::Value) -> (bool, String) {
        let r = f
            .client
            .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(args.as_object().unwrap().clone()))
            .await
            .unwrap();
        let text = r.content.iter().filter_map(|c| c.as_text()).map(|t| t.text.clone()).collect::<String>();
        (r.is_error == Some(true), text)
    }

    #[tokio::test]
    async fn lists_all_fifteen_tools() {
        let f = fixture().await;
        let tools = f.client.list_tools(None).await.unwrap();
        let mut names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
        names.sort();
        let mut expected: Vec<String> = TOOL_SUMMARY.split(',').map(|s| s.trim().to_string()).collect();
        expected.sort();
        assert_eq!(names, expected);
        f.server.abort();
    }

    #[tokio::test]
    async fn writes_reads_lists_and_searches() {
        let f = fixture().await;
        let (err, msg) = call(&f, "write_document", serde_json::json!({"path": "a/b.md", "content": "# Hi\nneedle\n"})).await;
        assert!(!err && msg.contains("Saved a/b.md"));
        let (_, body) = call(&f, "read_document", serde_json::json!({"path": "a/b.md"})).await;
        assert_eq!(body, "# Hi\nneedle\n");
        let (_, listing) = call(&f, "list_documents", serde_json::json!({})).await;
        assert!(listing.contains("📁 a/") && listing.contains("📄 a/b.md"), "{listing}");
        let (_, hits) = call(&f, "search_documents", serde_json::json!({"query": "NEEDLE"})).await;
        assert_eq!(hits, "a/b.md:2: needle");
        f.server.abort();
    }

    #[tokio::test]
    async fn rejects_escapes_and_bad_extensions_as_tool_errors() {
        let f = fixture().await;
        let (err, msg) = call(&f, "read_document", serde_json::json!({"path": "../outside.md"})).await;
        assert!(err && msg.contains("escapes"), "{msg}");
        let (err, msg) = call(&f, "write_document", serde_json::json!({"path": "evil.sh", "content": "x"})).await;
        assert!(err && msg.contains("Unsupported extension"), "{msg}");
        f.server.abort();
    }

    #[tokio::test]
    async fn patches_with_exact_match_safety() {
        let f = fixture().await;
        call(&f, "write_document", serde_json::json!({"path": "p.md", "content": "alpha beta alpha\n"})).await;
        let (err, msg) = call(&f, "patch_document", serde_json::json!({"path": "p.md", "old_text": "alpha", "new_text": "x"})).await;
        assert!(err && msg.contains("occurs 2 times"));
        let (err, _) = call(&f, "patch_document", serde_json::json!({"path": "p.md", "old_text": "beta", "new_text": "gamma"})).await;
        assert!(!err);
        assert_eq!(f.lib.read("p.md").unwrap(), "alpha gamma alpha\n");
        f.server.abort();
    }

    #[tokio::test]
    async fn manages_structure() {
        let f = fixture().await;
        call(&f, "write_document", serde_json::json!({"path": "m.md", "content": "x"})).await;
        let (_, msg) = call(&f, "rename_document", serde_json::json!({"path": "m.md", "new_name": "renamed"})).await;
        assert_eq!(msg, "Renamed to renamed.md");
        let (_, msg) = call(&f, "move_document", serde_json::json!({"path": "renamed.md", "target_folder": "Archive"})).await;
        assert_eq!(msg, "Moved to Archive/renamed.md");
        let (_, msg) = call(&f, "create_folder", serde_json::json!({"path": "Projects/Inky"})).await;
        assert_eq!(msg, "Created folder Projects/Inky");
        let (err, msg) = call(&f, "delete_document", serde_json::json!({"path": "Archive"})).await;
        assert!(err && msg.contains("Only documents"), "{msg}");
        let (err, msg) = call(&f, "delete_document", serde_json::json!({"path": "Archive/renamed.md"})).await;
        assert!(!err && msg == "Deleted Archive/renamed.md");
        f.server.abort();
    }

    #[tokio::test]
    async fn versions_are_reachable() {
        let f = fixture().await;
        call(&f, "write_document", serde_json::json!({"path": "v.md", "content": "one"})).await;
        call(&f, "write_document", serde_json::json!({"path": "v.md", "content": "two"})).await;
        let (_, list) = call(&f, "list_versions", serde_json::json!({"path": "v.md"})).await;
        let name = list.split_whitespace().next().unwrap().to_string();
        assert!(name.starts_with("v.") && name.ends_with(".md"), "{list}");
        let (_, body) = call(&f, "read_version", serde_json::json!({"path": "v.md", "version": name})).await;
        assert_eq!(body, "one");
        f.server.abort();
    }

    #[tokio::test]
    async fn comment_threads_lifecycle() {
        let f = fixture().await;
        call(&f, "write_document", serde_json::json!({"path": "c.md", "content": "hello brave world"})).await;
        let (err, msg) = call(&f, "create_comment", serde_json::json!({"path": "c.md", "quote": "nope", "text": "?"})).await;
        assert!(err && msg.contains("Quote not found"));
        let (_, msg) = call(&f, "create_comment", serde_json::json!({"path": "c.md", "quote": "brave", "text": "Why brave?"})).await;
        let id = msg.strip_prefix("Created ").unwrap().split(' ').next().unwrap().to_string();
        assert!(id.starts_with("thread-"));
        let threads = f.lib.threads("c.md").unwrap();
        assert_eq!(threads[0].prefix, "hello ");
        assert_eq!(threads[0].suffix, " world");
        assert_eq!(threads[0].comments[0].author.as_deref(), Some("Claude"));
        call(&f, "reply_to_comment", serde_json::json!({"path": "c.md", "thread_id": id, "text": "Because.", "author": "GPT"})).await;
        let (_, listing) = call(&f, "list_comments", serde_json::json!({"path": "c.md", "filter": "open"})).await;
        assert!(listing.contains("[Claude] Why brave?") && listing.contains("[GPT] Because."), "{listing}");
        let (_, msg) = call(&f, "resolve_comment", serde_json::json!({"path": "c.md", "thread_id": id})).await;
        assert_eq!(msg, format!("Resolved {id}"));
        let (_, listing) = call(&f, "list_comments", serde_json::json!({"path": "c.md", "filter": "open"})).await;
        assert_eq!(listing, "No open comments on c.md");
        f.server.abort();
    }

    #[tokio::test]
    async fn exposes_documents_as_resources() {
        let f = fixture().await;
        f.lib.write("r.md", "resource body").unwrap();
        let list = f.client.list_resources(None).await.unwrap();
        assert_eq!(list.resources[0].uri, "inky://doc/r.md");
        let read = f.client.read_resource(ReadResourceRequestParams::new("inky://doc/r.md")).await.unwrap();
        let rmcp::model::ResourceContents::TextResourceContents { text, mime_type, .. } = &read.contents[0] else {
            panic!("expected text");
        };
        assert_eq!(text, "resource body");
        assert_eq!(mime_type.as_deref(), Some("text/markdown"));
        assert!(f.client.read_resource(ReadResourceRequestParams::new("inky://doc/../x")).await.is_err());
        f.server.abort();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp:: 2>&1 | grep -E "^error" | head -3`
Expected: `InkyMcp` not found.

- [ ] **Step 3: Implement `mcp.rs`**

```rust
//! MCP server exposing the Inky library to agents. Tool names, parameters and
//! messages match the previous JS server so existing agent prompts keep working.

use crate::library::{self, CommentMsg, CommentThread, EntryKind, Library, CONTEXT_CHARS};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler};
use std::future::Future;

pub const TOOL_SUMMARY: &str = "list_documents, read_document, write_document, patch_document, \
rename_document, move_document, create_folder, delete_document, search_documents, \
list_versions, read_version, list_comments, create_comment, reply_to_comment, resolve_comment";

const INSTRUCTIONS: &str = "Inky is the user's markdown library. Paths are relative to the library root. \
Prefer patch_document over write_document for edits. Comment threads marked open usually need an answer; \
resolve a thread only after addressing it.";

#[derive(Clone)]
pub struct InkyMcp {
    lib: Library,
    tool_router: ToolRouter<Self>,
}

fn ok(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text.into())])
}

fn reply(result: Result<String, String>) -> CallToolResult {
    match result {
        Ok(text) => ok(text),
        Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
    }
}

// --- parameter shapes (field docs become the JSON-schema descriptions) ------

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct PathParams {
    /// Library-relative path, e.g. 'Notes/ideas.md'
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WriteParams {
    /// Library-relative path, e.g. 'Meetings/2026-08-31 standup.md'
    pub path: String,
    /// Full document content (markdown or mermaid source)
    pub content: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct PatchParams {
    /// Library-relative document path
    pub path: String,
    /// Exact text to replace (must occur exactly once)
    pub old_text: String,
    /// Replacement text
    pub new_text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReadVersionParams {
    /// Library-relative document path
    pub path: String,
    /// Version file name from list_versions
    pub version: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RenameParams {
    /// Library-relative document path
    pub path: String,
    /// New file name, e.g. 'Better title.md'
    pub new_name: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct MoveParams {
    /// Library-relative document path
    pub path: String,
    /// Library-relative destination folder ('' for the root)
    pub target_folder: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct FolderParams {
    /// Library-relative folder path, e.g. 'Projects/Inky'
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// Text to search for
    pub query: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommentFilter {
    All,
    Open,
    Resolved,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListCommentsParams {
    /// Library-relative document path
    pub path: String,
    /// Default: all
    #[serde(default)]
    pub filter: Option<CommentFilter>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreateCommentParams {
    /// Library-relative document path
    pub path: String,
    /// Exact text from the document to anchor the comment to
    pub quote: String,
    /// The comment
    pub text: String,
    /// Author label shown in Inky (default: Claude)
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReplyParams {
    /// Library-relative document path
    pub path: String,
    /// Thread id from list_comments
    pub thread_id: String,
    /// The reply
    pub text: String,
    /// Author label shown in Inky (default: Claude)
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ResolveParams {
    /// Library-relative document path
    pub path: String,
    /// Thread id from list_comments
    pub thread_id: String,
    /// Default true; false reopens
    #[serde(default)]
    pub resolved: Option<bool>,
}

fn render_thread(t: &CommentThread) -> String {
    let status = if t.resolved { "resolved" } else { "open" };
    let quote: String = if t.quote.chars().count() > 120 {
        format!("{}…", t.quote.chars().take(120).collect::<String>())
    } else {
        t.quote.clone()
    };
    let msgs = t
        .comments
        .iter()
        .map(|m| format!("    [{}] {}", m.author.as_deref().unwrap_or("User"), m.text.replace('\n', "\n    ")))
        .collect::<Vec<_>>()
        .join("\n");
    format!("- {} ({status})\n  quote: \"{quote}\"\n{msgs}", t.id)
}

#[tool_router]
impl InkyMcp {
    pub fn new(lib: Library) -> Self {
        Self { lib, tool_router: Self::tool_router() }
    }

    #[tool(
        name = "list_documents",
        description = "List every document and folder in the user's Inky library. Paths are relative to the library root. Documents are markdown (.md) or standalone mermaid diagrams (.mmd)."
    )]
    async fn list_documents(&self) -> CallToolResult {
        let listing = self
            .lib
            .walk()
            .into_iter()
            .map(|e| {
                let icon = match e.kind {
                    EntryKind::Folder => "📁",
                    EntryKind::Mermaid => "🧜",
                    EntryKind::Markdown => "📄",
                };
                format!("{icon} {}", e.rel)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let listing = if listing.is_empty() { "(library is empty)".to_string() } else { listing };
        ok(format!("Library root: {}\n\n{listing}", self.lib.root().display()))
    }

    #[tool(name = "read_document", description = "Read the raw markdown (or mermaid) source of a document in the Inky library.")]
    async fn read_document(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        reply(self.lib.read(&p.path))
    }

    #[tool(
        name = "write_document",
        description = "Write a markdown document into the Inky library. Creates the document (and any missing folders) if it does not exist, otherwise overwrites it. Use a .md extension for markdown and .mmd for standalone mermaid diagrams. Mermaid code fences inside .md files render as diagrams in Inky."
    )]
    async fn write_document(&self, Parameters(p): Parameters<WriteParams>) -> CallToolResult {
        if !library::is_doc(std::path::Path::new(&p.path)) {
            return reply(Err(format!(
                "Unsupported extension — use one of: {}",
                library::DOC_EXTENSIONS.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", ")
            )));
        }
        reply(self.lib.write(&p.path, &p.content).map(|_| format!("Saved {} ({} chars)", p.path, p.content.chars().count())))
    }

    #[tool(
        name = "patch_document",
        description = "Replace an exact text snippet inside a document. Prefer this over write_document for edits — it fails safely if the document changed since you read it. old_text must appear exactly once (include surrounding context to disambiguate)."
    )]
    async fn patch_document(&self, Parameters(p): Parameters<PatchParams>) -> CallToolResult {
        reply(self.lib.patch(&p.path, &p.old_text, &p.new_text).map(|_| format!("Patched {}", p.path)))
    }

    #[tool(
        name = "list_versions",
        description = "List snapshots of a document from its hidden history (kept automatically before content-changing overwrites, at most one per 10 minutes). Use read_version to fetch one — e.g. to report what changed, or to recover lost text."
    )]
    async fn list_versions(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        reply(self.lib.list_versions(&p.path).map(|versions| {
            if versions.is_empty() {
                return format!("No saved versions for {}", p.path);
            }
            versions
                .iter()
                .map(|v| format!("{}  (saved {})", v.name, library::iso_from_unix_millis(v.modified_ms)))
                .collect::<Vec<_>>()
                .join("\n")
        }))
    }

    #[tool(
        name = "read_version",
        description = "Read the full content of one snapshot from a document's history. Get version names from list_versions. To restore it, write the content back with write_document."
    )]
    async fn read_version(&self, Parameters(p): Parameters<ReadVersionParams>) -> CallToolResult {
        reply(self.lib.read_version(&p.path, &p.version))
    }

    #[tool(
        name = "rename_document",
        description = "Rename a document in place (comments follow the document). new_name keeps the original extension if none is given."
    )]
    async fn rename_document(&self, Parameters(p): Parameters<RenameParams>) -> CallToolResult {
        if p.new_name.contains('/') {
            return reply(Err("new_name must not contain '/'".into()));
        }
        reply(self.lib.rename(&p.path, &p.new_name).map(|t| format!("Renamed to {}", self.lib.relative(&t))))
    }

    #[tool(
        name = "move_document",
        description = "Move a document into another folder of the library (folders are created if missing; comments follow the document)."
    )]
    async fn move_document(&self, Parameters(p): Parameters<MoveParams>) -> CallToolResult {
        let folder = if p.target_folder.is_empty() { "." } else { p.target_folder.as_str() };
        reply(self.lib.move_into(&p.path, folder).map(|t| format!("Moved to {}", self.lib.relative(&t))))
    }

    #[tool(name = "create_folder", description = "Create a folder (and any missing parents) inside the Inky library.")]
    async fn create_folder(&self, Parameters(p): Parameters<FolderParams>) -> CallToolResult {
        reply(self.lib.ensure_folder(&p.path).map(|_| format!("Created folder {}", p.path)))
    }

    #[tool(name = "delete_document", description = "Delete a single document from the Inky library. Folders cannot be deleted.")]
    async fn delete_document(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        let abs = match self.lib.resolve(&p.path) {
            Ok(a) => a,
            Err(e) => return reply(Err(e)),
        };
        if !abs.is_file() || !library::is_doc(&abs) {
            return reply(Err("Only documents can be deleted".into()));
        }
        reply(self.lib.delete(&p.path).map(|_| format!("Deleted {}", p.path)))
    }

    #[tool(
        name = "search_documents",
        description = "Case-insensitive full-text search across every document in the Inky library. Returns matching lines with their document path and line number."
    )]
    async fn search_documents(&self, Parameters(p): Parameters<SearchParams>) -> CallToolResult {
        let hits = self.lib.search(&p.query);
        if hits.is_empty() {
            return ok(format!("No matches for \"{}\"", p.query));
        }
        ok(hits
            .iter()
            .map(|h| format!("{}:{}: {}", self.lib.relative(std::path::Path::new(&h.path)), h.line, h.text))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[tool(
        name = "list_comments",
        description = "List the comment threads on an Inky document (the user's questions and notes, Google-Docs style). Each thread has an id, a quoted text anchor, open/resolved status, and messages. Threads marked open usually need an answer."
    )]
    async fn list_comments(&self, Parameters(p): Parameters<ListCommentsParams>) -> CallToolResult {
        let filter = p.filter.unwrap_or(CommentFilter::All);
        reply(self.lib.threads(&p.path).map(|threads| {
            let threads: Vec<&CommentThread> = threads
                .iter()
                .filter(|t| match filter {
                    CommentFilter::All => true,
                    CommentFilter::Open => !t.resolved,
                    CommentFilter::Resolved => t.resolved,
                })
                .collect();
            if threads.is_empty() {
                let label = match filter {
                    CommentFilter::All => "",
                    CommentFilter::Open => "open ",
                    CommentFilter::Resolved => "resolved ",
                };
                return format!("No {label}comments on {}", p.path);
            }
            threads.iter().map(|t| render_thread(t)).collect::<Vec<_>>().join("\n\n")
        }))
    }

    #[tool(
        name = "create_comment",
        description = "Start a new comment thread on an Inky document, anchored to an exact quote from the document's text. The quote must appear verbatim in the document. Use this to leave feedback, questions, or suggestions the user will see highlighted in Inky."
    )]
    async fn create_comment(&self, Parameters(p): Parameters<CreateCommentParams>) -> CallToolResult {
        reply((|| {
            let doc = self.lib.read(&p.path)?;
            let idx = doc
                .find(&p.quote)
                .ok_or_else(|| "Quote not found in the document — it must match the text exactly.".to_string())?;
            let before = &doc[..idx];
            let after = &doc[idx + p.quote.len()..];
            let prefix: String = before.chars().rev().take(CONTEXT_CHARS).collect::<Vec<_>>().into_iter().rev().collect();
            let suffix: String = after.chars().take(CONTEXT_CHARS).collect();
            let now = library::iso_now();
            let thread = CommentThread {
                id: library::new_id("thread"),
                quote: p.quote.clone(),
                prefix,
                suffix,
                resolved: false,
                created_at: now.clone(),
                comments: vec![CommentMsg {
                    id: library::new_id("msg"),
                    text: p.text.clone(),
                    created_at: now,
                    author: Some(p.author.clone().unwrap_or_else(|| "Claude".into())),
                }],
            };
            let mut threads = self.lib.threads(&p.path)?;
            let id = thread.id.clone();
            threads.push(thread);
            self.lib.save_threads(&p.path, &threads)?;
            Ok(format!("Created {id} on {}", p.path))
        })())
    }

    #[tool(
        name = "reply_to_comment",
        description = "Add a reply to an existing comment thread on an Inky document. Use list_comments first to get thread ids. The user sees replies in Inky's comments panel."
    )]
    async fn reply_to_comment(&self, Parameters(p): Parameters<ReplyParams>) -> CallToolResult {
        reply((|| {
            let mut threads = self.lib.threads(&p.path)?;
            let thread = threads
                .iter_mut()
                .find(|t| t.id == p.thread_id)
                .ok_or_else(|| format!("No thread {} on {}", p.thread_id, p.path))?;
            thread.comments.push(CommentMsg {
                id: library::new_id("msg"),
                text: p.text.clone(),
                created_at: library::iso_now(),
                author: Some(p.author.clone().unwrap_or_else(|| "Claude".into())),
            });
            self.lib.save_threads(&p.path, &threads)?;
            Ok(format!("Replied to {}", p.thread_id))
        })())
    }

    #[tool(
        name = "resolve_comment",
        description = "Mark a comment thread on an Inky document as resolved (or reopen it). Only resolve a thread after actually addressing it — e.g. after replying or updating the document."
    )]
    async fn resolve_comment(&self, Parameters(p): Parameters<ResolveParams>) -> CallToolResult {
        reply((|| {
            let mut threads = self.lib.threads(&p.path)?;
            let thread = threads
                .iter_mut()
                .find(|t| t.id == p.thread_id)
                .ok_or_else(|| format!("No thread {} on {}", p.thread_id, p.path))?;
            thread.resolved = p.resolved.unwrap_or(true);
            let verb = if thread.resolved { "Resolved" } else { "Reopened" };
            self.lib.save_threads(&p.path, &threads)?;
            Ok(format!("{verb} {}", p.thread_id))
        })())
    }
}

const DOC_URI_PREFIX: &str = "inky://doc/";

#[tool_handler(router = self.tool_router)]
impl ServerHandler for InkyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("inky", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        async move {
            let resources = self
                .lib
                .walk()
                .into_iter()
                .filter(|e| e.kind != EntryKind::Folder)
                .map(|e| {
                    let mut r = Resource::new(format!("{DOC_URI_PREFIX}{}", e.rel), e.rel.clone());
                    r.mime_type = Some("text/markdown".into());
                    r
                })
                .collect();
            Ok(ListResourcesResult::with_all_items(resources))
        }
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, McpError>> + Send + '_ {
        async move {
            let mut t = ResourceTemplate::new(format!("{DOC_URI_PREFIX}{{+path}}"), "document");
            t.title = Some("Inky documents".into());
            t.description = Some("Markdown documents in the user's Inky library".into());
            t.mime_type = Some("text/markdown".into());
            Ok(ListResourceTemplatesResult::with_all_items(vec![t]))
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, McpError>> + Send + '_ {
        async move {
            let rel = request
                .uri
                .strip_prefix(DOC_URI_PREFIX)
                .ok_or_else(|| McpError::resource_not_found(format!("unknown resource {}", request.uri), None))?;
            let text = self
                .lib
                .read(rel)
                .map_err(|e| McpError::resource_not_found(e, None))?;
            let mut contents = ResourceContents::text(text, request.uri.clone());
            if let ResourceContents::TextResourceContents { mime_type, .. } = &mut contents {
                *mime_type = Some("text/markdown".into());
            }
            Ok(ReadResourceResult::new(vec![contents]).into())
        }
    }
}
```

Add `pub mod mcp;` to `lib.rs` next to `pub mod library;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test 2>&1 | tail -5`
Expected: `27 passed` (19 library + 8 mcp).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/mcp.rs src-tauri/src/lib.rs src-tauri/src/library.rs
git commit -m "Rust MCP server: 15 tools and document resources over Library"
```

---

### Task 8: Transports — stdio entry point and app-hosted HTTP server

**Files:**
- Modify: `src-tauri/src/mcp.rs` (append transport functions + HTTP test)
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/lib.rs:572-686` (replace `find_node`/`McpProc`/`start_mcp`/`stop_mcp`/`mcp_status`) and the `RunEvent::Exit` hook in `run()`

**Interfaces:**
- Produces: `mcp::serve_stdio_blocking() -> i32` (exit code), `mcp::HttpHandle { addr: SocketAddr, .. }` with `url() -> String`, `is_running() -> bool`, `stop()`, `mcp::serve_http(lib: Library, port: u16) -> impl Future<Output = Result<HttpHandle, String>>`.

- [ ] **Step 1: Write the failing HTTP test** (append to `mod tests` in `mcp.rs`)

```rust
    #[tokio::test]
    async fn http_server_answers_initialize_and_stops() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let handle = serve_http(lib, 0).await.unwrap();
        assert!(handle.is_running());
        assert!(handle.url().starts_with("http://127.0.0.1:") && handle.url().ends_with("/mcp"));
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
        let mut stream = tokio::net::TcpStream::connect(handle.addr).await.unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"name\":\"inky\""), "{response}");
        handle.stop();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while handle.is_running() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("server task should finish after stop()");
        assert!(tokio::net::TcpStream::connect(handle.addr).await.is_err(), "port released");
    }

    #[tokio::test]
    async fn http_port_in_use_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let first = serve_http(lib.clone(), 0).await.unwrap();
        let err = serve_http(lib, first.addr.port()).await.unwrap_err();
        assert!(err.contains("already in use"), "{err}");
        first.stop();
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test mcp::tests::http 2>&1 | grep -E "^error" | head -3`

- [ ] **Step 3: Implement the transports** (append to `mcp.rs`, above `mod tests`)

```rust
// --- transports -------------------------------------------------------------

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::ServiceExt;
use std::net::SocketAddr;
use tokio_util::sync::CancellationToken;

/// Serve MCP over stdin/stdout until the client closes the pipe. Runs on its own
/// runtime because it is used from `main` before (instead of) Tauri.
/// Returns the process exit code. Only JSON-RPC goes to stdout.
pub fn serve_stdio_blocking() -> i32 {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Inky MCP: cannot start runtime: {e}");
            return 1;
        }
    };
    rt.block_on(async {
        let lib = match Library::from_env() {
            Ok(lib) => lib,
            Err(e) => {
                eprintln!("Inky MCP: cannot open the library: {e}");
                return 1;
            }
        };
        eprintln!(
            "Inky MCP server running on stdio\n  Library: {}\n  Tools:   {TOOL_SUMMARY}\n\nThis process is meant to be launched by an MCP client (it waits for JSON-RPC on stdin).",
            lib.root().display()
        );
        let service = match InkyMcp::new(lib).serve(rmcp::transport::stdio()).await {
            Ok(s) => s,
            // The client went away before initialising — not an error worth a non-zero exit.
            Err(_) => return 0,
        };
        match service.waiting().await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Inky MCP: {e}");
                1
            }
        }
    })
}

/// A running app-hosted HTTP server. Dropping the handle does not stop it; call `stop()`.
pub struct HttpHandle {
    pub addr: SocketAddr,
    token: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl HttpHandle {
    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    pub fn stop(&self) {
        self.token.cancel();
    }
}

/// Bind `127.0.0.1:port` (0 = any free port) and serve streamable-HTTP MCP at `/mcp`
/// on the current tokio runtime.
pub async fn serve_http(lib: Library, port: u16) -> Result<HttpHandle, String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("port {port} is already in use — is another Inky (or something else) listening?")
        } else {
            format!("cannot listen on 127.0.0.1:{port}: {e}")
        }
    })?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let token = CancellationToken::new();
    let service = StreamableHttpService::new(
        move || Ok(InkyMcp::new(lib.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(token.child_token()),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let shutdown = token.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await;
    });
    Ok(HttpHandle { addr, token, task })
}
```

- [ ] **Step 4: `main.rs`**

```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `Inky --mcp`: serve MCP over stdio for agents and exit — no window, no Tauri.
    if std::env::args().skip(1).any(|a| a == "--mcp") {
        std::process::exit(inky_lib::mcp::serve_stdio_blocking());
    }
    inky_lib::run()
}
```

- [ ] **Step 5: Replace the node subprocess in `lib.rs`**

Delete `find_node`, `McpProc`, `kill_mcp`, and the three commands. Replace with:

```rust
// --- app-hosted MCP server (in-process, streamable HTTP) --------------------

struct McpServer(std::sync::Mutex<Option<mcp::HttpHandle>>);

fn stop_mcp_server(state: &McpServer) {
    if let Ok(mut guard) = state.0.lock() {
        if let Some(handle) = guard.take() {
            handle.stop();
        }
    }
}

#[tauri::command]
fn mcp_status(state: tauri::State<McpServer>) -> bool {
    let mut guard = state.0.lock().unwrap();
    match guard.as_ref() {
        Some(handle) if handle.is_running() => true,
        _ => {
            *guard = None;
            false
        }
    }
}

#[tauri::command]
async fn start_mcp(app: tauri::AppHandle, state: tauri::State<'_, McpServer>, port: u16) -> Result<String, String> {
    if let Some(handle) = state.0.lock().unwrap().as_ref() {
        if handle.is_running() {
            return Ok(handle.url());
        }
    }
    let lib = open_library(&app)?;
    let handle = mcp::serve_http(lib, port).await?;
    let url = handle.url();
    *state.0.lock().unwrap() = Some(handle);
    Ok(url)
}

#[tauri::command]
fn stop_mcp(state: tauri::State<McpServer>) {
    stop_mcp_server(&state);
}

/// Absolute path of the running binary — what MCP clients register for stdio mode.
#[tauri::command]
fn app_binary_path() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}
```

In `run()`: `.manage(McpProc(...))` → `.manage(McpServer(std::sync::Mutex::new(None)))`; add `app_binary_path` to `generate_handler!`; the exit hook becomes `stop_mcp_server(&app_handle.state::<McpServer>());`. Remove the now-unused `use std::path::Path` import if the compiler flags it.

- [ ] **Step 6: Run all tests and type-check**

Run: `cd src-tauri && cargo test 2>&1 | tail -3 && cargo check 2>&1 | tail -2`
Expected: `29 passed`, no warnings about unused items.

- [ ] **Step 7: Manual stdio check**

Run:
```sh
cd src-tauri && cargo build 2>&1 | tail -1
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | INKY_LIBRARY=/tmp/inky-stdio-test ./target/debug/inky --mcp 2>/dev/null | head -c 600; echo; echo "exit: ${PIPESTATUS[1]}"
```
Expected: two JSON lines (`serverInfo.name == "inky"`, then the tools list), exit 0, no window opened.

- [ ] **Step 8: Manual app check**

Run `make dev`, click the MCP light → green, toast shows `http://127.0.0.1:26317/mcp`. In another terminal: `curl -s -X POST http://127.0.0.1:26317/mcp -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"0"}}}'` → contains `"name":"inky"`. Click the light again → red, curl now fails to connect. Quit the app with the server running → process exits cleanly (no orphan).

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/mcp.rs src-tauri/src/main.rs src-tauri/src/lib.rs
git commit -m "Serve MCP in-process: stdio via 'Inky --mcp', HTTP from the status light"
```

---

### Task 9: "Connect an agent" setup dialog

**Files:**
- Create: `src/lib/components/McpSetupDialog.svelte`
- Modify: `src/lib/state.svelte.ts:69` (state) and `:185-216` (`startMcpServer`, `toggleMcpServer`)
- Modify: `src/lib/components/Toolbar.svelte:345-369` (status light) and where dialogs are mounted (`src/routes/+page.svelte` or wherever `NameDialog` is rendered — check with `grep -rn "NameDialog" src/routes src/lib`)

**Interfaces:**
- Consumes: Tauri command `app_binary_path` (Task 8).
- Produces: `app.mcpSetupOpen: boolean`, `app.openMcpSetup()`.

- [ ] **Step 1: State**

In `state.svelte.ts` next to `mcpUrl`:

```ts
  mcpSetupOpen = $state(false);
```

Replace the toast in `startMcpServer` and add `openMcpSetup`:

```ts
  async startMcpServer(announce: boolean) {
    try {
      const url = await invoke<string>("start_mcp", { port: 26317 });
      this.mcpUrl = url;
      localStorage.setItem("inky.mcpAutostart", "true");
      if (announce) {
        toast.success(`MCP server running at ${url}`, {
          duration: 12000,
          action: { label: "Connect an agent…", onClick: () => (this.mcpSetupOpen = true) },
        });
      }
    } catch (e) {
      this.mcpUrl = null;
      toast.error(`Could not start MCP server: ${e}`);
    }
  }

  openMcpSetup() {
    this.mcpSetupOpen = true;
  }
```

- [ ] **Step 2: Dialog component**

`src/lib/components/McpSetupDialog.svelte`:

```svelte
<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { invoke } from "@tauri-apps/api/core";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { toast } from "svelte-sonner";
  import { Copy } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";

  let binary = $state("/Applications/Inky.app/Contents/MacOS/Inky");
  $effect(() => {
    if (app.mcpSetupOpen) invoke<string>("app_binary_path").then((p) => (binary = p)).catch(() => {});
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
            <Button variant="ghost" size="sm" class="h-7 gap-1 px-2 text-xs" onclick={() => copy(s.code)}>
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
```

- [ ] **Step 3: Mount and wire the light**

Mount `<McpSetupDialog />` next to where `NameDialog` is mounted (find it with `grep -rn "NameDialog" src/routes src/lib --include=*.svelte`). In `Toolbar.svelte`, add to the MCP `Button`: `oncontextmenu={(e) => { e.preventDefault(); app.openMcpSetup(); }}` and change the tooltip strings to `… — click to stop, right-click to connect an agent` / `MCP server off — click to start, right-click to connect an agent`.

- [ ] **Step 4: Verify**

Run: `pnpm check 2>&1 | tail -3` → 0 errors. `make dev`: start the server, click *Connect an agent…* in the toast → dialog shows four snippets with the real binary path; Copy works; right-click the light opens it too.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/McpSetupDialog.svelte src/lib/state.svelte.ts src/lib/components/Toolbar.svelte src/routes
git commit -m "Connect-an-agent dialog with per-client MCP setup snippets"
```

---

### Task 10: Remove the Node server and its build plumbing

**Files:**
- Delete: `mcp/server.mjs`, `mcp/server.bundle.mjs`, `tests/mcp.test.ts`
- Modify: `package.json`, `pnpm-lock.yaml` (via `pnpm install`), `src-tauri/tauri.conf.json:6-10,42-47`, `.gitignore`, `Makefile`, `.mcp.json`, `.github/workflows/check.yml`

- [ ] **Step 1: Delete files**

```bash
git rm -r mcp tests/mcp.test.ts
```

- [ ] **Step 2: `package.json`**

Remove scripts `mcp` and `mcp:bundle`, the `bin` block, dependencies `@modelcontextprotocol/sdk` and `zod`, devDependency `esbuild`. Then `pnpm install` to refresh the lockfile.

- [ ] **Step 3: `tauri.conf.json`**

`"beforeDevCommand": "pnpm dev"`, `"beforeBuildCommand": "pnpm build"`, delete the `"resources": [...]` line.

- [ ] **Step 4: `.gitignore`, `Makefile`, `.mcp.json`, CI**

- `.gitignore`: delete the `mcp/server.bundle.mjs` line.
- `Makefile`: `mcp` target becomes
  ```make
  mcp: ## Run the MCP server on stdio (for manual testing)
  	cargo run --manifest-path src-tauri/Cargo.toml -- --mcp
  ```
  and add `cargo test` to `check`:
  ```make
  check: ## Type-check the frontend, then check and test the Rust backend
  	pnpm check
  	cd src-tauri && cargo check && cargo test
  ```
- `.mcp.json`:
  ```json
  {
    "mcpServers": {
      "inky": {
        "type": "http",
        "url": "http://127.0.0.1:26317/mcp"
      }
    }
  }
  ```
- `.github/workflows/check.yml`: after the `cargo check` step add
  ```yaml
      - run: cargo test
        working-directory: src-tauri
  ```

- [ ] **Step 5: Verify nothing references the old server**

Run: `grep -rn "server.mjs\|server.bundle\|mcp:bundle\|find_node\|@modelcontextprotocol" --exclude-dir=node_modules --exclude-dir=target --exclude-dir=docs . ; pnpm check 2>&1 | tail -2; pnpm test 2>&1 | tail -3; cd src-tauri && cargo check 2>&1 | tail -1`
Expected: grep finds nothing outside `docs/`; all checks/tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Drop the Node MCP server and its bundling step"
```

---

### Task 11: README and memory

**Files:**
- Modify: `README.md:82-83` (features bullet), `:115` (Makefile table), `:153-183` (MCP section)
- Modify: `/Users/energy/.claude/projects/-Users-energy-W-inky/memory/inky-project.md`

- [ ] **Step 1: README**

Features bullet:
```markdown
- **MCP server** — agents can read and write your library through Inky's
  built-in MCP server (see below). Nothing to install: toggle it from the
  toolbar's red/green **MCP** status light, or point stdio clients at
  `Inky --mcp`.
```

Makefile row: `| \`make mcp\` | Run the MCP server on stdio (dev build) |`.

Replace the "MCP server" section:

````markdown
## MCP server (let agents use your library)

Inky ships an MCP server inside the app binary — no Node.js or other runtime
needed. It exposes `list_documents`, `read_document`, `write_document`,
`patch_document`, `rename_document`, `move_document`, `create_folder`,
`delete_document`, `search_documents`, history tools (`list_versions`,
`read_version`) and comment tools (`list_comments`, `create_comment`,
`reply_to_comment`, `resolve_comment`), plus every document as an
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
````

Also drop "Node 20+" from *Prerequisites* only if nothing else needs it — the frontend build still does, so leave it.

- [ ] **Step 2: Memory**

In `inky-project.md`, replace the sentence about `mcp/server.mjs` with: "The MCP server is Rust, in-process (`src-tauri/src/mcp.rs` over `library.rs`): HTTP on 127.0.0.1:26317 from the toolbar light, stdio via `Inky.app/Contents/MacOS/Inky --mcp`. No Node needed at runtime. Project `.mcp.json` points at the HTTP server (app must be running)."

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "Document the built-in MCP server and per-client setup"
```

---

### Task 12: Release verification on a Node-less PATH

**Files:** none (verification only)

- [ ] **Step 1: Build**

Run: `make dmg 2>&1 | tail -5` → `Inky.app`, `.dmg`, `.tar.gz` + `.sig` produced. Check the size: `du -sh src-tauri/target/release/bundle/dmg/*.dmg` — expect ≈ 8–10 MB (no Node).

- [ ] **Step 2: Confirm no runtime dependency**

Run: `ls src-tauri/target/release/bundle/macos/Inky.app/Contents/Resources/ | grep -c bundle.mjs` → `0`.
Run with an empty PATH: `env -i HOME=$HOME PATH=/usr/bin:/bin open src-tauri/target/release/bundle/macos/Inky.app` → click the MCP light → green.
Run: `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' | env -i HOME=$HOME PATH=/usr/bin:/bin src-tauri/target/release/bundle/macos/Inky.app/Contents/MacOS/Inky --mcp 2>/dev/null | head -c 200` → JSON with `"name":"inky"`; no window appears.

- [ ] **Step 3: Client round-trip**

`claude mcp add inky-stdio -- "$PWD/src-tauri/target/release/bundle/macos/Inky.app/Contents/MacOS/Inky" --mcp`, then in a new `claude` session ask it to list Inky documents; remove the registration afterwards with `claude mcp remove inky-stdio`.
