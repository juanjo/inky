# Open Anywhere Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inky opens and edits any Markdown file or folder on disk — from Finder, the `inky` terminal command or File → Open — and remembers recent documents.

**Architecture:** A new Rust `Places` state sits beside the library: one optional *workspace* folder and a set of *granted* files, both opened as sidecar-less `Library` handles so the existing path confinement (`Library::resolve`) is reused. Every app command resolves paths through `Places`; grants are created only by Rust (macOS open events, native dialogs, the persisted recents list, followed links). The MCP server is untouched and stays confined to the library.

**Tech Stack:** Tauri 2 (Rust), SvelteKit + Svelte 5 runes, Vitest (jsdom), `cargo test`, objc2 for macOS AppKit calls. Package manager: **pnpm**.

**Spec:** `docs/superpowers/specs/2026-10-01-open-anywhere-design.md`

## Global Constraints

- Document extensions: `md`, `markdown`, `mmd` (`library::DOC_EXTENSIONS`).
- Outside the library Inky writes **no** `.inky-history/` folder and **no** comment sidecar.
- The MCP server (`mcp.rs`, `Inky --mcp`) must keep using only the configured library; it never sees `Places`.
- The webview can never create a grant. Grants come only from Rust code paths: open events, Rust-run dialogs, `open_recent` (path must already be in the persisted list), `follow_link` (Markdown targets of an accessible document).
- Recents: max 20, newest first, deduplicated, canonical absolute paths, stored in `config.json` as `recent`.
- Bundle id `com.inky.app`; CLI installs to `/usr/local/bin/inky`.
- No Node at runtime. Use `pnpm`, never `npm`.
- Build a distributable with `pnpm bundle` (not `pnpm tauri build`).
- Commit messages end with:
  `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`
- Stage only the files each task names — the working tree holds unrelated uncommitted work (`src-tauri/capabilities/default.json`, `tests/window-drag.test.ts`, back/forward + zoom files) that must not be swept into these commits.

## Review Focus

1. **Paths with spaces, unicode or quotes** (`inky "My Notes/é.md"`, Finder URLs with `%20`) — expect them to open and save normally. Pinned in Task 3 (`grant_file_with_spaces_and_unicode`) and Task 7 (`shell_quote_escapes_single_quotes`).
2. **Symlinked / non-canonical paths** (`/tmp/x.md` vs `/private/tmp/x.md`) — a granted file must be found under either spelling. Pinned in Task 3 (`granted_file_found_by_non_canonical_path`).
3. **Opening a file that is already inside the library** via Finder or CLI — it must behave as a library document (history and comments on), not as an outside file. Pinned in Task 3 (`library_wins_over_grants`) and handled in Task 5's `handle_open_paths`.
4. **"Back to Library" while editing a document from the workspace** — the open document must keep saving. Pinned in Task 3 (`clear_workspace_keeps_open_doc`).
5. **Cold launch by double-clicking a file** — the file opens (not the last document), exactly once. Pinned in Task 5 (`open_queue_*` tests) and checked manually in Task 10.

---

## File Structure

| File | Responsibility |
|---|---|
| `src-tauri/src/library.rs` (modify) | `sidecars` flag on `Library`; `Config.recent` |
| `src-tauri/src/recents.rs` (create) | Pure recents-list operations |
| `src-tauri/src/places.rs` (create) | Workspace + granted files, path → `Place` resolution, link following |
| `src-tauri/src/opens.rs` (create) | `OpenRequest` type and the cold-launch queue |
| `src-tauri/src/lib.rs` (modify) | Commands route through `Places`; open events; menus; recents; CLI install |
| `src-tauri/tauri.conf.json` (modify) | File associations |
| `src-tauri/Cargo.toml` (modify) | `NSDocumentController` feature |
| `src/lib/paths.ts` (create) | `isInside`, `displayDir` helpers |
| `src/lib/types.ts` (modify) | `OpenRequest` type |
| `src/lib/state.svelte.ts` (modify) | workspace, sidecars, open requests, recents, CLI install |
| `src/routes/+page.svelte` (modify) | menu ids `install_cli`; history guard |
| `src/lib/components/Sidebar.svelte` (modify) | Workspace header + Back to Library |
| `src/lib/components/Toolbar.svelte` (modify) | Outside-folder hint; disable comments/history |
| `src/lib/components/QuickOpen.svelte` (modify) | Recents first |
| `src/lib/components/Preview.svelte`, `Editor.svelte` (modify) | `follow_link`; no comment pill without sidecars |

---

### Task 0: Branch

- [ ] **Step 1: Create the feature branch** (we are on `main`)

```bash
git switch -c open-anywhere
```

---

### Task 1: Sidecar-less `Library` handles and `Config.recent`

**Files:**
- Modify: `src-tauri/src/library.rs` (struct `Library` ~line 367, `open` ~376, `write` ~473, `patch` ~485, `list_versions` ~512, `read_version` ~534, `raw_comments` ~724, `write_raw_comments` ~732, `threads` ~744, `save_threads` ~755, `Config` ~18)
- Modify: `src-tauri/src/lib.rs` (two `Config { library: … }` literals)
- Test: `src-tauri/src/library.rs` `mod tests`

**Interfaces:**
- Produces: `Library::without_sidecars(self) -> Library`, `Library::has_sidecars(&self) -> bool`, `library::NO_SIDECARS: &str`, `Config { library: Option<String>, recent: Vec<String> }` (`recent` is `#[serde(default)]`).

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `library.rs`)

```rust
    // --- sidecar-less handles (outside documents) ---------------------------

    #[test]
    fn without_sidecars_writes_no_history() {
        let (_d, lib) = temp_lib();
        let lib = lib.without_sidecars();
        assert!(!lib.has_sidecars());
        lib.write("n.md", "v1").unwrap();
        lib.write("n.md", "v2").unwrap();
        lib.patch("n.md", "v2", "v3").unwrap();
        assert_eq!(lib.read("n.md").unwrap(), "v3");
        assert!(!lib.root().join(HISTORY_DIR).exists());
    }

    #[test]
    fn without_sidecars_refuses_comments_and_versions() {
        let (_d, lib) = temp_lib();
        let lib = lib.without_sidecars();
        lib.write("n.md", "x").unwrap();
        assert_eq!(lib.raw_comments("n.md").unwrap_err(), NO_SIDECARS);
        assert!(lib.write_raw_comments("n.md", "{}").is_err());
        assert!(lib.list_versions("n.md").is_err());
        assert!(lib.read_version("n.md", "n.x.md").is_err());
        assert!(lib.threads("n.md").is_err());
        assert!(lib.save_threads("n.md", &[]).is_err());
        assert!(sidecar_for(&lib.root().join("n.md")).map(|p| !p.exists()).unwrap_or(true));
    }

    #[test]
    fn config_without_recent_still_parses() {
        let c: Config = serde_json::from_str(r#"{"library":"/x"}"#).unwrap();
        assert_eq!(c.library.as_deref(), Some("/x"));
        assert!(c.recent.is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib without_sidecars config_without_recent`
Expected: compile errors — `without_sidecars`, `has_sidecars`, `NO_SIDECARS`, `recent` not found.

- [ ] **Step 3: Implement**

In `Config`:

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub library: Option<String>,
    /// Recently opened documents, newest first (see `recents`).
    #[serde(default)]
    pub recent: Vec<String>,
}
```

Near `HISTORY_DIR`:

```rust
/// Error for history/comment calls on documents outside the library.
pub const NO_SIDECARS: &str =
    "Version history and comments are only available for documents in the Inky library";
```

`Library` struct and constructors:

```rust
pub struct Library {
    root: PathBuf,
    /// Marks snapshots taken by this handle as agent edits (the MCP server).
    agent_origin: bool,
    /// Off for folders and files opened from outside the library: nothing but
    /// the document itself is ever written there.
    sidecars: bool,
}
```

In `open`: `Ok(Library { root, agent_origin: false, sidecars: true })`. Add after `with_agent_origin`:

```rust
    /// A handle that never writes `.inky-history/` or comment sidecars.
    pub fn without_sidecars(mut self) -> Self {
        self.sidecars = false;
        self
    }

    pub fn has_sidecars(&self) -> bool {
        self.sidecars
    }
```

In `write`, change the snapshot condition to:

```rust
        if self.sidecars && fs::read_to_string(&p).map(|old| old != content).unwrap_or(false) {
```

In `patch`, change `snapshot(&p, self.agent_origin);` to:

```rust
        if self.sidecars {
            snapshot(&p, self.agent_origin);
        }
```

As the **first line** of each of `list_versions`, `read_version`, `raw_comments`, `write_raw_comments`, `threads`, `save_threads`:

```rust
        if !self.sidecars {
            return Err(NO_SIDECARS.into());
        }
```

In `lib.rs`, replace both `Config { library: Some(...) }` literals so they keep the recents:

```rust
// open_library:
        let _ = write_config(app, &Config { library: Some(stored), ..read_config(app) });
// set_library_root:
    write_config(&app, &Config { library: Some(root.clone()), ..read_config(&app) })?;
```

Run `grep -rn "Config {" src-tauri/src` and fix any other literal the same way.

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib`
Expected: all pass (37 existing + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/library.rs src-tauri/src/lib.rs
git commit -m "Library handles without sidecars; recents field in config

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Recents list operations

**Files:**
- Create: `src-tauri/src/recents.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod recents;`)

**Interfaces:**
- Produces (all on `Vec<String>` of absolute paths):
  - `recents::CAP: usize = 20`
  - `recents::push(list: &mut Vec<String>, path: &str)`
  - `recents::rename(list: &mut Vec<String>, from: &str, to: &str)`
  - `recents::forget(list: &mut Vec<String>, path: &str)`
  - `recents::existing(list: &[String]) -> Vec<String>`

- [ ] **Step 1: Write the failing tests** — create `src-tauri/src/recents.rs` with just the module doc and this test module:

```rust
//! Recently opened documents: newest first, unique, capped. Pure list
//! operations; `lib.rs` persists the list in `config.json`.

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn push_puts_newest_first_without_duplicates() {
        let mut l = list(&["/a.md", "/b.md"]);
        push(&mut l, "/b.md");
        assert_eq!(l, list(&["/b.md", "/a.md"]));
        push(&mut l, "/c.md");
        assert_eq!(l, list(&["/c.md", "/b.md", "/a.md"]));
    }

    #[test]
    fn push_caps_the_list() {
        let mut l = Vec::new();
        for i in 0..30 {
            push(&mut l, &format!("/{i}.md"));
        }
        assert_eq!(l.len(), CAP);
        assert_eq!(l[0], "/29.md");
    }

    #[test]
    fn rename_rewrites_files_and_folder_contents() {
        let mut l = list(&["/n/a.md", "/n/sub/b.md", "/nother.md"]);
        rename(&mut l, "/n/a.md", "/n/z.md");
        rename(&mut l, "/n/sub", "/n/deep");
        assert_eq!(l, list(&["/n/z.md", "/n/deep/b.md", "/nother.md"]));
    }

    #[test]
    fn rename_onto_an_existing_entry_dedupes() {
        let mut l = list(&["/a.md", "/b.md"]);
        rename(&mut l, "/b.md", "/a.md");
        assert_eq!(l, list(&["/a.md"]));
    }

    #[test]
    fn forget_drops_files_and_folder_contents() {
        let mut l = list(&["/a.md", "/gone/x.md", "/gone.md"]);
        forget(&mut l, "/gone");
        assert_eq!(l, list(&["/a.md", "/gone.md"]));
    }

    #[test]
    fn existing_skips_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("here.md");
        std::fs::write(&here, "x").unwrap();
        let here = here.to_string_lossy().into_owned();
        let l = vec![here.clone(), "/definitely/not/here.md".to_string()];
        assert_eq!(existing(&l), vec![here]);
    }
}
```

Add `pub mod recents;` next to `pub mod library;` in `lib.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib recents`
Expected: FAIL to compile — `push`, `rename`, `forget`, `existing`, `CAP` not found.

- [ ] **Step 3: Implement** (above the test module)

```rust
use std::path::Path;

pub const CAP: usize = 20;

fn within(path: &str, prefix: &str) -> bool {
    path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

pub fn push(list: &mut Vec<String>, path: &str) {
    list.retain(|p| p != path);
    list.insert(0, path.to_string());
    list.truncate(CAP);
}

/// A file or folder was renamed or moved.
pub fn rename(list: &mut Vec<String>, from: &str, to: &str) {
    for p in list.iter_mut() {
        if within(p, from) {
            *p = format!("{to}{}", &p[from.len()..]);
        }
    }
    let mut seen = std::collections::HashSet::new();
    list.retain(|p| seen.insert(p.clone()));
}

/// A file or folder was deleted.
pub fn forget(list: &mut Vec<String>, path: &str) {
    list.retain(|p| !within(p, path));
}

/// Entries whose file still exists.
pub fn existing(list: &[String]) -> Vec<String> {
    list.iter().filter(|p| Path::new(p).is_file()).cloned().collect()
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib recents`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recents.rs src-tauri/src/lib.rs
git commit -m "Recents list operations

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `Places` — workspace, granted files, link following

**Files:**
- Create: `src-tauri/src/places.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod places;`)

**Interfaces:**
- Consumes: `Library::open`, `Library::resolve`, `Library::without_sidecars`, `Library::has_sidecars`, `library::is_doc` (Task 1).
- Produces:

```rust
pub struct Place { pub lib: Library, /* private: file: Option<PathBuf> */ }
impl Place {
    pub fn sidecars(&self) -> bool;
    /// The handle for folder-level operations (tree, create, move, delete);
    /// errors for a single granted file.
    pub fn folder(&self) -> Result<&Library, String>;
}
#[derive(Serialize)] pub struct LinkTarget { pub path: String, pub doc: bool }
#[derive(Default)] pub struct Places { /* workspace, files */ }
impl Places {
    pub fn workspace(&self) -> Option<&Library>;
    pub fn active<'a>(&'a self, library: &'a Library) -> &'a Library;
    pub fn set_workspace(&mut self, dir: &Path) -> Result<PathBuf, String>;
    pub fn clear_workspace(&mut self, keep: Option<&str>);
    pub fn grant_file(&mut self, path: &Path) -> Result<PathBuf, String>;
    pub fn find(&self, library: &Library, path: &str) -> Result<Place, String>;
    pub fn renamed(&mut self, from: &Path, to: &Path);
    pub fn follow_link(&mut self, library: &Library, from_doc: &str, href: &str) -> Result<LinkTarget, String>;
}
```

- [ ] **Step 1: Write failing tests** — create `src-tauri/src/places.rs` with the module doc and this test module:

```rust
//! What the app (not MCP) may touch besides the library: at most one
//! workspace folder shown in the sidebar, plus individually granted files.
//! Both are sidecar-less `Library` handles, so `Library::resolve` keeps doing
//! all the confinement. Grants are only ever created from Rust code paths.

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        _dirs: Vec<tempfile::TempDir>,
        library: Library,
        outside: PathBuf,
    }

    /// A library plus an unrelated "outside" folder with a.md, b.md, notes.txt.
    fn fixture() -> Fixture {
        let lib_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        let library = Library::open(lib_dir.path()).unwrap();
        let outside = out_dir.path().canonicalize().unwrap();
        fs::write(outside.join("a.md"), "A").unwrap();
        fs::write(outside.join("b.md"), "B").unwrap();
        fs::write(outside.join("notes.txt"), "T").unwrap();
        fs::write(library.root().join("in.md"), "IN").unwrap();
        Fixture { _dirs: vec![lib_dir, out_dir], library, outside }
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn library_paths_resolve_with_sidecars() {
        let f = fixture();
        let places = Places::default();
        let place = places.find(&f.library, &s(&f.library.root().join("in.md"))).unwrap();
        assert!(place.sidecars());
        assert!(place.folder().is_ok());
    }

    #[test]
    fn outside_paths_are_rejected_until_granted() {
        let f = fixture();
        let mut places = Places::default();
        let a = f.outside.join("a.md");
        assert!(places.find(&f.library, &s(&a)).is_err());
        places.grant_file(&a).unwrap();
        let place = places.find(&f.library, &s(&a)).unwrap();
        assert!(!place.sidecars());
        assert!(place.folder().is_err(), "a single file is not a folder place");
        place.lib.write(&s(&a), "A2").unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), "A2");
        assert!(!f.outside.join(crate::library::HISTORY_DIR).exists());
    }

    #[test]
    fn granting_a_file_does_not_grant_its_siblings() {
        let f = fixture();
        let mut places = Places::default();
        places.grant_file(&f.outside.join("a.md")).unwrap();
        assert!(places.find(&f.library, &s(&f.outside.join("b.md"))).is_err());
        let sneaky = format!("{}/../{}/b.md", s(&f.outside), f.outside.file_name().unwrap().to_string_lossy());
        assert!(places.find(&f.library, &sneaky).is_err());
    }

    #[test]
    fn only_markdown_files_can_be_granted() {
        let f = fixture();
        let mut places = Places::default();
        assert!(places.grant_file(&f.outside.join("notes.txt")).is_err());
        assert!(places.grant_file(&f.outside).is_err(), "a folder is not a file grant");
        assert!(places.grant_file(&f.outside.join("missing.md")).is_err());
    }

    #[test]
    fn grant_file_with_spaces_and_unicode() {
        let f = fixture();
        let odd = f.outside.join("My Notes é 'q'.md");
        fs::write(&odd, "x").unwrap();
        let mut places = Places::default();
        let granted = places.grant_file(&odd).unwrap();
        assert!(places.find(&f.library, &s(&granted)).is_ok());
    }

    #[test]
    fn granted_file_found_by_non_canonical_path() {
        let f = fixture();
        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path().join("alias");
        std::os::unix::fs::symlink(&f.outside, &link).unwrap();
        let mut places = Places::default();
        places.grant_file(&f.outside.join("a.md")).unwrap();
        assert!(places.find(&f.library, &s(&link.join("a.md"))).is_ok());
    }

    #[test]
    fn library_wins_over_grants() {
        let f = fixture();
        let mut places = Places::default();
        let inside = f.library.root().join("in.md");
        places.grant_file(&inside).unwrap();
        assert!(places.find(&f.library, &s(&inside)).unwrap().sidecars());
    }

    #[test]
    fn workspace_is_a_sidecar_less_folder_place() {
        let f = fixture();
        let mut places = Places::default();
        let root = places.set_workspace(&f.outside).unwrap();
        assert_eq!(root, f.outside);
        assert_eq!(places.active(&f.library).root(), f.outside.as_path());
        let place = places.find(&f.library, &s(&f.outside.join("b.md"))).unwrap();
        assert!(!place.sidecars());
        let lib = place.folder().unwrap();
        lib.create_doc_unique(&s(&f.outside), "new", "md", "x").unwrap();
        assert!(f.outside.join("new.md").exists());
        assert!(places.set_workspace(&f.outside.join("a.md")).is_err(), "files are not workspaces");
    }

    #[test]
    fn clear_workspace_keeps_open_doc() {
        let f = fixture();
        let mut places = Places::default();
        places.set_workspace(&f.outside).unwrap();
        let a = s(&f.outside.join("a.md"));
        places.clear_workspace(Some(&a));
        assert!(places.workspace().is_none());
        assert_eq!(places.active(&f.library).root(), f.library.root());
        assert!(places.find(&f.library, &a).is_ok(), "open doc stays editable");
        assert!(places.find(&f.library, &s(&f.outside.join("b.md"))).is_err());
    }

    #[test]
    fn clear_workspace_ignores_keep_outside_it() {
        let f = fixture();
        let ws = tempfile::tempdir().unwrap();
        let mut places = Places::default();
        places.set_workspace(ws.path()).unwrap();
        // `keep` is not in the closing workspace, so it must not become a grant.
        places.clear_workspace(Some(&s(&f.outside.join("a.md"))));
        assert!(places.find(&f.library, &s(&f.outside.join("a.md"))).is_err());
    }

    #[test]
    fn renamed_moves_the_grant() {
        let f = fixture();
        let mut places = Places::default();
        let a = places.grant_file(&f.outside.join("a.md")).unwrap();
        let place = places.find(&f.library, &s(&a)).unwrap();
        let z = place.lib.rename(&s(&a), "z.md").unwrap();
        places.renamed(&a, &z);
        assert!(places.find(&f.library, &s(&z)).is_ok());
        assert!(places.find(&f.library, &s(&a)).is_err());
    }

    #[test]
    fn follow_link_grants_markdown_targets_only() {
        let f = fixture();
        let mut places = Places::default();
        let a = places.grant_file(&f.outside.join("a.md")).unwrap();
        let t = places.follow_link(&f.library, &s(&a), "b.md").unwrap();
        assert!(t.doc);
        assert_eq!(t.path, s(&f.outside.join("b.md")));
        assert!(places.find(&f.library, &t.path).is_ok());

        let txt = places.follow_link(&f.library, &s(&a), "notes.txt").unwrap();
        assert!(!txt.doc);
        assert!(places.find(&f.library, &txt.path).is_err(), "non-markdown is not granted");

        assert!(places.follow_link(&f.library, &s(&a), "missing.md").is_err());
    }

    #[test]
    fn follow_link_requires_an_accessible_source() {
        let f = fixture();
        let mut places = Places::default();
        let b = s(&f.outside.join("b.md"));
        assert!(places.follow_link(&f.library, &b, "a.md").is_err());
        assert!(places.find(&f.library, &s(&f.outside.join("a.md"))).is_err());
    }

    #[test]
    fn follow_link_inside_library_does_not_grant() {
        let f = fixture();
        fs::write(f.library.root().join("other.md"), "O").unwrap();
        let mut places = Places::default();
        let from = s(&f.library.root().join("in.md"));
        let t = places.follow_link(&f.library, &from, "other.md").unwrap();
        assert!(t.doc);
        assert!(places.find(&f.library, &t.path).unwrap().sidecars());
    }
}
```

Add `pub mod places;` in `lib.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib places`
Expected: FAIL to compile — `Places`, `Place`, `LinkTarget` not found.

- [ ] **Step 3: Implement** (between the module doc and the tests)

```rust
use crate::library::{is_doc, Library};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a path lives, and the handle to operate on it with.
pub struct Place {
    pub lib: Library,
    /// `Some` for a single granted file: `lib` is rooted at its folder, but
    /// only this file may be touched.
    file: Option<PathBuf>,
}

impl Place {
    pub fn sidecars(&self) -> bool {
        self.lib.has_sidecars()
    }

    pub fn folder(&self) -> Result<&Library, String> {
        match self.file {
            None => Ok(&self.lib),
            Some(_) => Err("Not available for a single document opened from outside the library".into()),
        }
    }
}

#[derive(Serialize, Debug)]
pub struct LinkTarget {
    pub path: String,
    /// Markdown document (open it) vs. any other file (reveal in Finder).
    pub doc: bool,
}

#[derive(Default)]
pub struct Places {
    workspace: Option<Library>,
    files: HashSet<PathBuf>,
}

impl Places {
    pub fn workspace(&self) -> Option<&Library> {
        self.workspace.as_ref()
    }

    /// The root the sidebar, Quick Open and library search show.
    pub fn active<'a>(&'a self, library: &'a Library) -> &'a Library {
        self.workspace.as_ref().unwrap_or(library)
    }

    pub fn set_workspace(&mut self, dir: &Path) -> Result<PathBuf, String> {
        if !dir.is_dir() {
            return Err(format!("Not a folder: {}", dir.display()));
        }
        let lib = Library::open(dir)?.without_sidecars();
        let root = lib.root().to_path_buf();
        self.workspace = Some(lib);
        Ok(root)
    }

    /// Back to the library. `keep` (the open document) stays editable if it
    /// lives in the workspace being closed.
    pub fn clear_workspace(&mut self, keep: Option<&str>) {
        if let (Some(ws), Some(keep)) = (self.workspace.take(), keep) {
            if let Ok(p) = ws.resolve(keep) {
                if p.is_file() && is_doc(&p) {
                    self.files.insert(p);
                }
            }
        }
    }

    pub fn grant_file(&mut self, path: &Path) -> Result<PathBuf, String> {
        let p = path.canonicalize().map_err(|_| format!("Not found: {}", path.display()))?;
        if !p.is_file() || !is_doc(&p) {
            return Err(format!("Not a Markdown document: {}", p.display()));
        }
        self.files.insert(p.clone());
        Ok(p)
    }

    pub fn find(&self, library: &Library, path: &str) -> Result<Place, String> {
        if library.resolve(path).is_ok() {
            return Ok(Place { lib: library.clone(), file: None });
        }
        if let Some(ws) = &self.workspace {
            if ws.resolve(path).is_ok() {
                return Ok(Place { lib: ws.clone(), file: None });
            }
        }
        let p = Path::new(path);
        if p.is_absolute() {
            if let Ok(c) = p.canonicalize() {
                if self.files.contains(&c) {
                    let parent = c.parent().ok_or("invalid path")?;
                    let lib = Library::open(parent)?.without_sidecars();
                    return Ok(Place { lib, file: Some(c) });
                }
            }
        }
        Err(format!("Not an open document: {path}"))
    }

    /// A granted file was renamed or moved: the grant follows it.
    pub fn renamed(&mut self, from: &Path, to: &Path) {
        if self.files.remove(from) {
            self.files.insert(to.to_path_buf());
        }
    }

    /// Resolve `href` relative to an accessible document; Markdown targets
    /// outside every place are granted so the link can be opened.
    pub fn follow_link(&mut self, library: &Library, from_doc: &str, href: &str) -> Result<LinkTarget, String> {
        let from = self.find(library, from_doc)?.lib.resolve(from_doc)?;
        let dir = from.parent().ok_or("invalid path")?;
        let target = dir
            .join(href)
            .canonicalize()
            .map_err(|_| format!("Linked file not found: {href}"))?;
        let path = target.to_string_lossy().into_owned();
        if !target.is_file() || !is_doc(&target) {
            return Ok(LinkTarget { path, doc: false });
        }
        if self.find(library, &path).is_err() {
            self.grant_file(&target)?;
        }
        Ok(LinkTarget { path, doc: true })
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib places`
Expected: 14 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/places.rs src-tauri/src/lib.rs
git commit -m "Places: workspace folder and granted outside files

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Route app commands through `Places`; recents + workspace commands

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 1–3 APIs.
- Produces (Tauri commands, camelCase args from JS):
  - unchanged signatures, now `Places`-aware: `list_tree`, `read_doc`, `write_doc`, `list_versions`, `read_version`, `search_library`, `create_doc`, `create_folder`, `rename_path`, `read_comments`, `write_comments`, `delete_path`, `move_path`, `save_image`, `doc_mtime`
  - `doc_sidecars(path: String) -> Result<bool, String>`
  - `workspace_root() -> Option<String>`
  - `clear_workspace(keep: Option<String>)`
  - `follow_link(fromDoc: String, href: String) -> Result<LinkTarget, String>`
  - `note_recent(path: String) -> Result<(), String>`
  - `recent_docs() -> Vec<String>`
  - `open_recent(path: String) -> Result<String, String>` (returns canonical path)
  - Rust helpers for later tasks: `struct AppPlaces(Mutex<Places>)`, `fn place(app, path) -> Result<Place, String>`, `fn active_library(app) -> Result<Library, String>`, `fn update_recents(app, f: impl FnOnce(&mut Vec<String>))`, `fn on_recents_changed(app)` (no-op until Task 6).

This task is glue over already-tested modules; it's verified by `cargo build`, the full Rust test suite (MCP confinement tests must still pass) and the app smoke test at the end.

- [ ] **Step 1: Add state and helpers** (after `fn to_string`)

```rust
use places::{LinkTarget, Place, Places};

/// Workspace folder + granted outside files (see `places`). App-only: the
/// MCP server never sees this.
struct AppPlaces(std::sync::Mutex<Places>);

fn place(app: &tauri::AppHandle, path: &str) -> Result<Place, String> {
    let lib = open_library(app)?;
    app.state::<AppPlaces>().0.lock().unwrap().find(&lib, path)
}

/// The library, or the workspace while one is showing.
fn active_library(app: &tauri::AppHandle) -> Result<Library, String> {
    let lib = open_library(app)?;
    let places = app.state::<AppPlaces>().0.lock().unwrap();
    Ok(places.active(&lib).clone())
}

fn update_recents(app: &tauri::AppHandle, f: impl FnOnce(&mut Vec<String>)) {
    let mut config = read_config(app);
    f(&mut config.recent);
    let _ = write_config(app, &config);
    on_recents_changed(app);
}

/// Keeps the Open Recent menu and Dock list in sync (filled in by Task 6).
fn on_recents_changed(_app: &tauri::AppHandle) {}
```

Register in the builder: `.manage(AppPlaces(std::sync::Mutex::new(Places::default())))`.

- [ ] **Step 2: Rewrite the path commands**

```rust
#[tauri::command]
fn list_tree(app: tauri::AppHandle) -> Result<Vec<Node>, String> {
    Ok(active_library(&app)?.tree())
}

#[tauri::command]
fn read_doc(app: tauri::AppHandle, path: String) -> Result<String, String> {
    place(&app, &path)?.lib.read(&path)
}

#[tauri::command]
fn write_doc(app: tauri::AppHandle, path: String, content: String) -> Result<(), String> {
    place(&app, &path)?.lib.write(&path, &content)
}

#[tauri::command]
fn doc_sidecars(app: tauri::AppHandle, path: String) -> Result<bool, String> {
    Ok(place(&app, &path)?.sidecars())
}

#[tauri::command]
fn list_versions(app: tauri::AppHandle, path: String) -> Result<Vec<VersionInfo>, String> {
    place(&app, &path)?.lib.list_versions(&path)
}

#[tauri::command]
fn read_version(app: tauri::AppHandle, path: String, version: String) -> Result<String, String> {
    place(&app, &path)?.lib.read_version(&path, &version)
}

#[tauri::command]
fn search_library(app: tauri::AppHandle, query: String) -> Result<Vec<SearchHit>, String> {
    Ok(active_library(&app)?.search(&query))
}

#[tauri::command]
fn create_doc(app: tauri::AppHandle, dir: String, name: String, ext: String, content: String) -> Result<String, String> {
    place(&app, &dir)?.folder()?.create_doc_unique(&dir, &name, &ext, &content).map(to_string)
}

#[tauri::command]
fn create_folder(app: tauri::AppHandle, dir: String, name: String) -> Result<String, String> {
    place(&app, &dir)?.folder()?.create_folder_unique(&dir, &name).map(to_string)
}

#[tauri::command]
fn rename_path(app: tauri::AppHandle, path: String, new_name: String) -> Result<String, String> {
    let p = place(&app, &path)?;
    let old = p.lib.resolve(&path)?;
    let new = p.lib.rename(&path, &new_name)?;
    app.state::<AppPlaces>().0.lock().unwrap().renamed(&old, &new);
    let (from, to) = (to_string(old), to_string(new.clone()));
    update_recents(&app, |r| recents::rename(r, &from, &to));
    Ok(to_string(new))
}

#[tauri::command]
fn read_comments(app: tauri::AppHandle, doc_path: String) -> Result<String, String> {
    place(&app, &doc_path)?.lib.raw_comments(&doc_path)
}

#[tauri::command]
fn write_comments(app: tauri::AppHandle, doc_path: String, json: String) -> Result<(), String> {
    place(&app, &doc_path)?.lib.write_raw_comments(&doc_path, &json)
}

#[tauri::command]
fn delete_path(app: tauri::AppHandle, state: tauri::State<UndoStash>, path: String) -> Result<Option<String>, String> {
    let p = place(&app, &path)?;
    let lib = p.folder()?;
    let abs = to_string(lib.resolve(&path)?);
    let result = match stash_root(&app).and_then(|root| lib.stash_delete(&path, &root)) {
        Ok(entry) => {
            let token = entry.token.clone();
            state.0.lock().unwrap().insert(token.clone(), entry);
            Ok(Some(token))
        }
        Err(_) => lib.delete(&path).map(|_| None),
    };
    if result.is_ok() {
        update_recents(&app, |r| recents::forget(r, &abs));
    }
    result
}

#[tauri::command]
fn move_path(app: tauri::AppHandle, path: String, target_dir: String) -> Result<String, String> {
    let p = place(&app, &path)?;
    let lib = p.folder()?;
    let old = to_string(lib.resolve(&path)?);
    let new = to_string(lib.move_into(&path, &target_dir)?);
    update_recents(&app, |r| recents::rename(r, &old, &new));
    Ok(new)
}

#[tauri::command]
fn doc_mtime(app: tauri::AppHandle, path: String) -> Result<u64, String> {
    place(&app, &path)?.lib.mtime(&path)
}
```

In `save_image`, replace `let doc = open_library(&app)?.resolve(&doc_path)?;` with:

```rust
    let doc = place(&app, &doc_path)?.lib.resolve(&doc_path)?;
```

- [ ] **Step 3: Workspace, link and recents commands**

```rust
#[tauri::command]
fn workspace_root(app: tauri::AppHandle) -> Option<String> {
    let places = app.state::<AppPlaces>().0.lock().unwrap();
    places.workspace().map(|l| to_string(l.root().to_path_buf()))
}

/// Back to Library. `keep` is the open document, which stays editable.
#[tauri::command]
fn clear_workspace(app: tauri::AppHandle, keep: Option<String>) {
    app.state::<AppPlaces>().0.lock().unwrap().clear_workspace(keep.as_deref());
    start_watcher(&app);
}

#[tauri::command]
fn follow_link(app: tauri::AppHandle, from_doc: String, href: String) -> Result<LinkTarget, String> {
    let lib = open_library(&app)?;
    app.state::<AppPlaces>().0.lock().unwrap().follow_link(&lib, &from_doc, &href)
}

/// Record a successful open. Only documents the app can already reach are
/// accepted, so this can't be used to smuggle in a grant via `open_recent`.
#[tauri::command]
fn note_recent(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let abs = to_string(place(&app, &path)?.lib.resolve(&path)?);
    update_recents(&app, |r| recents::push(r, &abs));
    Ok(())
}

#[tauri::command]
fn recent_docs(app: tauri::AppHandle) -> Vec<String> {
    recents::existing(&read_config(&app).recent)
}

/// Re-open a document from the recents list, granting it if it lives outside
/// the library. Paths not in the persisted list are refused.
#[tauri::command]
fn open_recent(app: tauri::AppHandle, path: String) -> Result<String, String> {
    if !read_config(&app).recent.contains(&path) {
        return Err("Not a recent document".into());
    }
    if let Ok(p) = place(&app, &path) {
        return Ok(to_string(p.lib.resolve(&path)?));
    }
    let granted = app.state::<AppPlaces>().0.lock().unwrap().grant_file(Path::new(&path))?;
    Ok(to_string(granted))
}
```

Add `doc_sidecars, workspace_root, clear_workspace, follow_link, note_recent, recent_docs, open_recent` to `generate_handler![…]`.

- [ ] **Step 4: Watcher follows the active root** — in `start_watcher`, replace `let Ok(lib) = open_library(app) else { return };` with:

```rust
    let Ok(lib) = active_library(app) else { return };
```

and drop the previous watcher before installing the new one (so switching roots doesn't leave two running): at the top of `start_watcher` add `state.0.lock().unwrap().take();` right after `let state = …;`.

- [ ] **Step 5: Build and test**

Run: `cd src-tauri && cargo build && cargo test`
Expected: builds; all unit tests and the MCP integration tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "Route app commands through Places; recents and workspace commands

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: macOS open events, cold-launch queue, file associations

**Files:**
- Create: `src-tauri/src/opens.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod opens;`, `handle_open_paths`, run loop, single-instance callback, setup)
- Modify: `src-tauri/tauri.conf.json` (`bundle.fileAssociations`)

**Interfaces:**
- Consumes: `AppPlaces`, `open_library`, `start_watcher`, `update_recents` (Task 4).
- Produces:
  - `opens::OpenRequest` — serialized as `{ "kind": "file" | "folder", "path": string }`
  - `opens::OpenQueue` with `push(&mut self, req) -> Option<OpenRequest>` (returns the request when it should be emitted now) and `drain(&mut self) -> Vec<OpenRequest>`
  - `fn handle_open_paths(app: &tauri::AppHandle, paths: Vec<PathBuf>)` in `lib.rs`
  - command `take_pending_opens() -> Vec<OpenRequest>`
  - event `open-request` with an `OpenRequest` payload

- [ ] **Step 1: Write failing tests** — create `src-tauri/src/opens.rs`:

```rust
//! Files and folders handed to Inky by macOS (Finder, `open`, the `inky`
//! CLI). Requests that arrive before the webview is listening are queued and
//! drained once by the frontend.

#[cfg(test)]
mod tests {
    use super::*;

    fn file(p: &str) -> OpenRequest {
        OpenRequest::File { path: p.into() }
    }

    #[test]
    fn open_queue_holds_requests_until_drained() {
        let mut q = OpenQueue::default();
        assert!(q.push(file("/a.md")).is_none());
        assert!(q.push(file("/b.md")).is_none());
        assert_eq!(q.drain(), vec![file("/a.md"), file("/b.md")]);
    }

    #[test]
    fn open_queue_emits_directly_after_drain() {
        let mut q = OpenQueue::default();
        q.drain();
        assert_eq!(q.push(file("/c.md")), Some(file("/c.md")));
        assert!(q.drain().is_empty(), "nothing is delivered twice");
    }

    #[test]
    fn open_request_serializes_with_kind_tag() {
        let json = serde_json::to_string(&OpenRequest::Folder { path: "/x".into() }).unwrap();
        assert_eq!(json, r#"{"kind":"folder","path":"/x"}"#);
    }
}
```

Add `pub mod opens;` in `lib.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --lib opens`
Expected: FAIL to compile — `OpenRequest`, `OpenQueue` not found.

- [ ] **Step 3: Implement `opens.rs`** (above the tests)

```rust
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum OpenRequest {
    File { path: String },
    Folder { path: String },
}

/// `pending` is `Some` until the frontend drains it; afterwards requests are
/// emitted as events straight away.
pub struct OpenQueue {
    pending: Option<Vec<OpenRequest>>,
}

impl Default for OpenQueue {
    fn default() -> Self {
        OpenQueue { pending: Some(Vec::new()) }
    }
}

impl OpenQueue {
    pub fn push(&mut self, req: OpenRequest) -> Option<OpenRequest> {
        match self.pending.as_mut() {
            Some(list) => {
                list.push(req);
                None
            }
            None => Some(req),
        }
    }

    pub fn drain(&mut self) -> Vec<OpenRequest> {
        self.pending.take().unwrap_or_default()
    }
}
```

Run: `cd src-tauri && cargo test --lib opens` → 3 passed.

- [ ] **Step 4: Wire into `lib.rs`**

```rust
use opens::{OpenQueue, OpenRequest};

struct PendingOpens(std::sync::Mutex<OpenQueue>);

/// Files become granted documents, folders become the workspace. A file that
/// is already in the library (or workspace) is opened as such — no grant.
fn handle_open_paths(app: &tauri::AppHandle, paths: Vec<PathBuf>) {
    use tauri::Emitter;
    for path in paths {
        let req = if path.is_dir() {
            let root = app.state::<AppPlaces>().0.lock().unwrap().set_workspace(&path);
            let Ok(root) = root else { continue };
            start_watcher(app);
            OpenRequest::Folder { path: to_string(root) }
        } else if library::is_doc(&path) {
            let Ok(canonical) = path.canonicalize() else { continue };
            let canon = to_string(canonical.clone());
            let reachable = place(app, &canon).is_ok();
            if !reachable
                && app.state::<AppPlaces>().0.lock().unwrap().grant_file(&canonical).is_err()
            {
                continue;
            }
            OpenRequest::File { path: canon }
        } else {
            continue;
        };
        let emit_now = app.state::<PendingOpens>().0.lock().unwrap().push(req);
        if let Some(req) = emit_now {
            let _ = app.emit("open-request", req);
        }
    }
}

#[tauri::command]
fn take_pending_opens(state: tauri::State<PendingOpens>) -> Vec<OpenRequest> {
    state.0.lock().unwrap().drain()
}

/// Paths passed on a command line (`Inky <path>…`), resolved against `cwd`.
fn paths_from_args(args: impl IntoIterator<Item = String>, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .filter(|a| !a.starts_with('-'))
        .map(|a| cwd.join(a))
        .collect()
}
```

Builder changes:

```rust
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            // Second launch: focus the existing window, open any paths given.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            handle_open_paths(app, paths_from_args(args.into_iter().skip(1), Path::new(&cwd)));
        }))
```

```rust
        .manage(PendingOpens(std::sync::Mutex::new(OpenQueue::default())))
```

In `.setup(|app| { … })`, before `Ok(())`:

```rust
            if let Ok(cwd) = std::env::current_dir() {
                handle_open_paths(app.handle(), paths_from_args(std::env::args().skip(1), &cwd));
            }
```

Add `take_pending_opens` to `generate_handler!`. Replace the `.run(…)` closure:

```rust
        .run(|app_handle, event| match event {
            tauri::RunEvent::Exit => {
                stop_mcp_server(&app_handle.state::<McpServer>());
                purge_stash_dir(app_handle);
            }
            // Finder double-click, "Open With", `open -a Inky …`, the `inky` CLI.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                let paths = urls.into_iter().filter_map(|u| u.to_file_path().ok()).collect();
                handle_open_paths(app_handle, paths);
            }
            _ => {}
        });
```

- [ ] **Step 5: File associations** — in `tauri.conf.json` `bundle`, add:

```json
    "fileAssociations": [
      {
        "ext": ["md", "markdown"],
        "name": "Markdown Document",
        "description": "Markdown document",
        "role": "Editor",
        "mimeType": "text/markdown"
      },
      {
        "ext": ["mmd"],
        "name": "Mermaid Diagram",
        "description": "Mermaid diagram",
        "role": "Editor"
      }
    ],
```

- [ ] **Step 6: Build and test**

Run: `cd src-tauri && cargo build && cargo test`
Expected: builds; all tests pass. Then `pnpm tauri build --bundles app` is **not** needed yet — the Info.plist check happens in Task 10.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/opens.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "Open files and folders from Finder and the command line

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: File menu — Open…, Open Folder…, Open Recent; Dock recents

**Files:**
- Modify: `src-tauri/src/lib.rs` (`build_menu`, `on_menu_event`, `on_recents_changed`)
- Modify: `src-tauri/Cargo.toml` (objc2-app-kit feature `NSDocumentController`)

**Interfaces:**
- Consumes: `handle_open_paths`, `read_config`, `update_recents`, `recents::existing`.
- Produces: menu ids `open_file`, `open_folder`, `open_recent:<index>`, `clear_recent` (handled in Rust; not forwarded to the webview); `struct RecentMenu(Mutex<Option<tauri::menu::Submenu<tauri::Wry>>>)`; `fn recent_label(path: &str, home: Option<&Path>) -> String`.

- [ ] **Step 1: Failing test for the menu label** (add a `#[cfg(test)] mod tests` at the bottom of `lib.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_label_shows_name_and_short_folder() {
        let home = Path::new("/Users/me");
        assert_eq!(recent_label("/Users/me/repo/README.md", Some(home)), "README.md — ~/repo");
        assert_eq!(recent_label("/opt/x.md", Some(home)), "x.md — /opt");
        assert_eq!(recent_label("/Users/me/a.md", Some(home)), "a.md — ~");
    }
}
```

Run: `cd src-tauri && cargo test --lib recent_label` → FAIL (`recent_label` not found).

- [ ] **Step 2: Implement the label**

```rust
/// "README.md — ~/repo" for the Open Recent menu.
fn recent_label(path: &str, home: Option<&Path>) -> String {
    let p = Path::new(path);
    let name = p.file_name().unwrap_or_default().to_string_lossy();
    let dir = p.parent().unwrap_or(Path::new("/"));
    let dir = match home.and_then(|h| dir.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    };
    format!("{name} — {dir}")
}
```

Run: `cd src-tauri && cargo test --lib recent_label` → PASS.

- [ ] **Step 3: Menu items** — in `build_menu`, at the top of `file_sub` after "Open Quickly…":

```rust
        .item(
            &MenuItemBuilder::with_id("open_file", "Open…")
                .accelerator("CmdOrCtrl+O")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("open_folder", "Open Folder…")
                .accelerator("CmdOrCtrl+Shift+O")
                .build(handle)?,
        )
        .item(&recent_sub)
```

with, before `file_sub` is built:

```rust
    let recent_sub = SubmenuBuilder::new(handle, "Open Recent").build()?;
```

and after `app.set_menu(menu)?;`:

```rust
    *app.state::<RecentMenu>().0.lock().unwrap() = Some(recent_sub);
    on_recents_changed(handle);
```

`build_menu` runs in `setup`, after `.manage(...)`, so add `.manage(RecentMenu(std::sync::Mutex::new(None)))` to the builder.

- [ ] **Step 4: Fill `on_recents_changed`** (replace the Task 4 no-op)

```rust
struct RecentMenu(std::sync::Mutex<Option<tauri::menu::Submenu<tauri::Wry>>>);

/// Rebuild File → Open Recent from the persisted list.
fn on_recents_changed(app: &tauri::AppHandle) {
    use tauri::menu::{MenuItemBuilder, PredefinedMenuItem};
    let guard = app.state::<RecentMenu>().0.lock().unwrap();
    let Some(sub) = guard.as_ref() else { return };
    if let Ok(items) = sub.items() {
        for item in items {
            let _ = sub.remove(&item);
        }
    }
    let list = recents::existing(&read_config(app).recent);
    let home = app.path().home_dir().ok();
    if list.is_empty() {
        if let Ok(item) = MenuItemBuilder::with_id("recent_none", "No Recent Documents").enabled(false).build(app) {
            let _ = sub.append(&item);
        }
        return;
    }
    for (i, path) in list.iter().enumerate() {
        let label = recent_label(path, home.as_deref());
        if let Ok(item) = MenuItemBuilder::with_id(format!("open_recent:{i}"), label).build(app) {
            let _ = sub.append(&item);
        }
    }
    if let Ok(sep) = PredefinedMenuItem::separator(app) {
        let _ = sub.append(&sep);
    }
    if let Ok(item) = MenuItemBuilder::with_id("clear_recent", "Clear Menu").build(app) {
        let _ = sub.append(&item);
    }
}
```

- [ ] **Step 5: Handle the menu ids in Rust** — replace `.on_menu_event(…)`:

```rust
        .on_menu_event(|app, event| {
            use tauri_plugin_dialog::DialogExt;
            let id = event.id().0.clone();
            match id.as_str() {
                "open_file" | "open_folder" => {
                    let app = app.clone();
                    // The blocking dialog must not run on the main thread.
                    std::thread::spawn(move || {
                        let picker = app.dialog().file();
                        let picked = if id == "open_file" {
                            picker
                                .add_filter("Markdown", &library::DOC_EXTENSIONS)
                                .blocking_pick_file()
                        } else {
                            picker.blocking_pick_folder()
                        };
                        if let Some(path) = picked.and_then(|p| p.into_path().ok()) {
                            handle_open_paths(&app, vec![path]);
                        }
                    });
                }
                "clear_recent" => {
                    update_recents(app, |r| r.clear());
                    clear_dock_recents(app);
                }
                _ if id.starts_with("open_recent:") => {
                    let list = recents::existing(&read_config(app).recent);
                    let idx: usize = id["open_recent:".len()..].parse().unwrap_or(usize::MAX);
                    if let Some(path) = list.get(idx) {
                        handle_open_paths(app, vec![PathBuf::from(path)]);
                    }
                }
                _ => {
                    let _ = app.emit("menu", id);
                }
            }
        })
```

- [ ] **Step 6: Dock recents** — `Cargo.toml`, add `"NSDocumentController",` to the `objc2-app-kit` features list. In `lib.rs`:

```rust
/// Add a document to the Dock icon's right-click list (macOS keeps that list).
fn note_dock_recent(app: &tauri::AppHandle, path: String) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(move || {
        use objc2_app_kit::NSDocumentController;
        use objc2_foundation::{NSString, NSURL};
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
        NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (app, path);
}

fn clear_dock_recents(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(|| {
        use objc2_app_kit::NSDocumentController;
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        NSDocumentController::sharedDocumentController(mtm).clearRecentDocuments(None);
    });
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}
```

If the objc2 method names differ in the installed version, check `~/.cargo/registry/src/*/objc2-app-kit-0.3*/src/generated/NSDocumentController.rs` for the exact signatures (`sharedDocumentController`, `noteNewRecentDocumentURL`, `clearRecentDocuments`).

Call it from `note_recent` (Task 4) after `update_recents`:

```rust
    note_dock_recent(&app, abs);
```

(move the `update_recents` closure to clone `abs` first: `let a = abs.clone(); update_recents(&app, |r| recents::push(r, &a));`).

- [ ] **Step 7: Build and test**

Run: `cd src-tauri && cargo build && cargo test`
Expected: builds; all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "File menu: Open, Open Folder, Open Recent; Dock recents

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: `inky` command installer

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: command `install_cli() -> Result<String, String>` (async; returns the install path); menu id `install_cli` (forwarded to the webview as a `menu` event); `const CLI_SCRIPT: &str`; `fn shell_quote(p: &Path) -> String`.

- [ ] **Step 1: Failing tests** (add to `lib.rs` `mod tests`)

```rust
    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote(Path::new("/a b/c")), "'/a b/c'");
        assert_eq!(shell_quote(Path::new("/it's")), r"'/it'\''s'");
    }

    #[test]
    fn cli_script_opens_by_bundle_id_and_passes_all_args() {
        assert!(CLI_SCRIPT.starts_with("#!/bin/sh\n"));
        assert!(CLI_SCRIPT.contains(r#"exec open -b com.inky.app "$@""#));
    }
```

Run: `cd src-tauri && cargo test --lib shell_quote cli_script` → FAIL (not found).

- [ ] **Step 2: Implement**

```rust
/// `/usr/local/bin/inky`: hands files/folders to Inky through macOS `open`,
/// so they arrive exactly like a Finder double-click.
const CLI_SCRIPT: &str = "#!/bin/sh\n# inky — open Markdown files or folders in Inky\nexec open -b com.inky.app \"$@\"\n";
const CLI_PATH: &str = "/usr/local/bin/inky";

fn shell_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// Install the `inky` command (one administrator prompt). Async so the
/// password dialog doesn't block the main thread.
#[tauri::command]
async fn install_cli(app: tauri::AppHandle) -> Result<String, String> {
    let tmp = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("inky-cli");
    fs::create_dir_all(tmp.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&tmp, CLI_SCRIPT).map_err(|e| e.to_string())?;
    let sh = format!(
        "mkdir -p /usr/local/bin && cp {} {CLI_PATH} && chmod 755 {CLI_PATH}",
        shell_quote(&tmp)
    );
    let apple = format!(
        "do shell script \"{}\" with administrator privileges",
        sh.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(apple)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(CLI_PATH.into())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
```

Add `install_cli` to `generate_handler!`. Menu item in `app_sub`, after "Check for Updates…":

```rust
        .item(&MenuItemBuilder::with_id("install_cli", "Install 'inky' Command in PATH…").build(handle)?)
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test --lib`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "Install an 'inky' terminal command

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Frontend state — workspace, sidecars, open requests, recents, links

**Files:**
- Create: `src/lib/paths.ts`
- Test: `tests/paths.test.ts`
- Modify: `src/lib/types.ts`, `src/lib/state.svelte.ts`, `src/routes/+page.svelte`, `src/lib/components/Preview.svelte`, `src/lib/components/Editor.svelte`

**Interfaces:**
- Consumes: commands `doc_sidecars`, `workspace_root`, `clear_workspace`, `follow_link`, `note_recent`, `recent_docs`, `open_recent`, `take_pending_opens`, `install_cli`; event `open-request`.
- Produces:
  - `paths.ts`: `isInside(path: string, root: string): boolean`, `displayDir(path: string, home: string): string`
  - `types.ts`: `export interface OpenRequest { kind: "file" | "folder"; path: string }`
  - `AppState`: `workspace: string | null` ($state), `activeRoot` ($derived), `docSidecars: boolean` ($state), `homeDir: string` ($state), `recentDocs: string[]` ($state), `refreshRecents(): Promise<void>`, `openRecent(path: string): Promise<void>`, `handleOpenRequest(req: OpenRequest): Promise<void>`, `closeWorkspace(): Promise<void>`, `installCli(): Promise<void>`

- [ ] **Step 1: Failing tests** — `tests/paths.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { displayDir, isInside } from "$lib/paths";

describe("isInside", () => {
  it("matches the root itself and its descendants only", () => {
    expect(isInside("/lib", "/lib")).toBe(true);
    expect(isInside("/lib/a.md", "/lib")).toBe(true);
    expect(isInside("/library/a.md", "/lib")).toBe(false);
    expect(isInside("/a.md", "")).toBe(false);
  });
});

describe("displayDir", () => {
  it("shows the parent folder with ~ for home", () => {
    expect(displayDir("/Users/me/repo/README.md", "/Users/me")).toBe("~/repo");
    expect(displayDir("/Users/me/a.md", "/Users/me")).toBe("~");
    expect(displayDir("/opt/x.md", "/Users/me")).toBe("/opt");
    expect(displayDir("/opt/x.md", "")).toBe("/opt");
  });
});
```

Run: `pnpm vitest run tests/paths.test.ts` → FAIL (module not found).

- [ ] **Step 2: Implement `src/lib/paths.ts`**

```ts
/** `path` is `root` or lies beneath it. */
export function isInside(path: string, root: string): boolean {
  if (!root) return false;
  return path === root || path.startsWith(root + "/");
}

/** Parent folder of `path`, with the home folder shortened to `~`. */
export function displayDir(path: string, home: string): string {
  const dir = path.slice(0, path.lastIndexOf("/")) || "/";
  if (home && isInside(dir, home)) return "~" + dir.slice(home.length);
  return dir;
}
```

Run: `pnpm vitest run tests/paths.test.ts` → PASS.

- [ ] **Step 3: `types.ts`** — append:

```ts
/** A file or folder handed to Inky by macOS (Finder, `inky`, File → Open). */
export interface OpenRequest {
  kind: "file" | "folder";
  path: string;
}
```

- [ ] **Step 4: `state.svelte.ts` fields** — imports:

```ts
import { homeDir } from "@tauri-apps/api/path";
import type { OpenRequest, ThemeName, TreeNode, ViewMode } from "./types";
import { isInside } from "./paths";
```

(merge `OpenRequest` into the existing `./types` import). Fields, after `libraryRoot`:

```ts
  /** Folder opened from outside the library, shown in the sidebar instead of it. */
  workspace = $state<string | null>(null);
  activeRoot = $derived(this.workspace ?? this.libraryRoot);
  /** False for documents outside the library: no comments, no history. */
  docSidecars = $state(true);
  homeDir = $state("");
  recentDocs = $state<string[]>([]);
```

In `flatDocs`, change `const prefix = this.libraryRoot.length + 1;` to `const prefix = this.activeRoot.length + 1;`. In `newDoc`, change `dir: dir ?? this.libraryRoot,` to `dir: dir ?? this.activeRoot,`.

- [ ] **Step 5: `state.svelte.ts` methods**

```ts
  async refreshRecents() {
    this.recentDocs = await invoke<string[]>("recent_docs").catch(() => []);
  }

  /** Open from the recents list (re-grants documents outside the library). */
  async openRecent(path: string) {
    try {
      const resolved = await invoke<string>("open_recent", { path });
      await this.openDoc(resolved);
    } catch (e) {
      toast.error(`Could not open document: ${e}`);
    }
  }

  async handleOpenRequest(req: OpenRequest) {
    if (req.kind === "folder") {
      this.workspace = req.path;
      this.sidebarVisible = true;
      await this.refreshTree();
    } else {
      await this.openDoc(req.path);
    }
    await getCurrentWindow().setFocus();
  }

  async closeWorkspace() {
    await invoke("clear_workspace", { keep: this.currentPath });
    this.workspace = null;
    await this.refreshTree();
  }

  async installCli() {
    try {
      const path = await invoke<string>("install_cli");
      toast.success(`Installed ${path}. Try: inky README.md`);
    } catch (e) {
      if (!String(e).includes("User canceled")) toast.error(`Could not install the command: ${e}`);
    }
  }
```

`setFocus` needs `core:window:allow-set-focus` — add that string to `src-tauri/capabilities/default.json` `permissions` (that file has the user's uncommitted `allow-start-dragging` line: edit it, but in Step 9 stage it with `git add -p` and take only the `allow-set-focus` hunk).

In `openDoc`, replace the line `await this.loadComments();` with:

```ts
      this.docSidecars = await invoke<boolean>("doc_sidecars", { path }).catch(() => false);
      if (!this.docSidecars) {
        this.commentsVisible = false;
        this.historyVisible = false;
      }
      await this.loadComments();
      invoke("note_recent", { path }).catch(() => {});
```

At the top of `loadComments`, after the resets and the `if (!this.currentPath) return;` line, add:

```ts
    if (!this.docSidecars) return;
```

`toggleComments` first line: `if (!this.docSidecars && !this.commentsVisible) return;`

In `closeDoc`, add `this.docSidecars = true;`.

- [ ] **Step 6: `init()` startup order** — after `this.libraryRoot = …` is set, add:

```ts
    this.homeDir = await homeDir().catch(() => "");
    this.workspace = await invoke<string | null>("workspace_root");
```

Replace the block

```ts
    if (this.tree.length === 0) {
      …create welcome…
      await this.openDoc(path);
    } else {
      const last = localStorage.getItem("inky.lastDoc");
      if (last) await this.openDoc(last, { silent: true });
    }
```

with

```ts
    // Listen before draining so nothing falls between the two.
    await listen<OpenRequest>("open-request", (e) => this.handleOpenRequest(e.payload));
    const pending = await invoke<OpenRequest[]>("take_pending_opens");
    let welcome: string | null = null;
    if (this.tree.length === 0 && !this.workspace) {
      welcome = await invoke<string>("create_doc", {
        dir: this.libraryRoot,
        name: "Welcome to Inky",
        ext: "md",
        content: WELCOME_DOC,
      });
      await this.refreshTree();
    }
    if (pending.length > 0) {
      for (const req of pending) await this.handleOpenRequest(req);
    } else if (welcome) {
      await this.openDoc(welcome);
    } else {
      const last = localStorage.getItem("inky.lastDoc");
      if (last) {
        // Outside documents need re-granting; library ones pass straight through.
        const resolved = await invoke<string>("open_recent", { path: last }).catch(() => last);
        await this.openDoc(resolved, { silent: true });
      }
    }
```

In `chooseLibrary`, after `this.closeDoc();` add `this.workspace = null;` and before `set_library_root` add `await invoke("clear_workspace", { keep: null });`.

- [ ] **Step 7: `+page.svelte`** — in `handleMenu` actions:

```ts
      install_cli: () => app.installCli(),
```

and change the `history` action to:

```ts
      history: () => {
        if (app.currentPath && app.docSidecars) app.historyVisible = true;
      },
```

- [ ] **Step 8: Links and comment pills**

`Preview.svelte` `handleLink`: replace everything from `// Relative link: resolve against the current document's folder.` to the end of the function with:

```ts
    // Relative link: Rust resolves it against the current document (and
    // grants Markdown targets outside the library).
    if (!app.currentPath) return;
    invoke<{ path: string; doc: boolean }>("follow_link", {
      fromDoc: app.currentPath,
      href: decodeURIComponent(href),
    })
      .then(async ({ path, doc }) => {
        if (doc) {
          app.openDoc(path);
        } else {
          const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
          revealItemInDir(path);
        }
      })
      .catch((e) => toast.error(String(e)));
```

`Preview.svelte` `onMouseUp` condition: add `!app.docSidecars ||` before `app.isMermaidDoc ||`.
`Editor.svelte` `updateSelAction`: change the first line to `if (!view || app.isMermaidDoc || !app.docSidecars) return;`.

- [ ] **Step 9: Check, test, commit**

Run: `pnpm check && pnpm vitest run`
Expected: 0 errors; all tests pass.

```bash
git add src/lib/paths.ts tests/paths.test.ts src/lib/types.ts src/lib/state.svelte.ts \
  src/routes/+page.svelte src/lib/components/Preview.svelte src/lib/components/Editor.svelte
git add -p src-tauri/capabilities/default.json   # take only the allow-set-focus hunk
git commit -m "Frontend: open requests, workspace, outside documents, recents

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: UI — sidebar workspace header, toolbar hints, Quick Open recents

**Files:**
- Modify: `src/lib/components/Sidebar.svelte`, `src/lib/components/Toolbar.svelte`, `src/lib/components/QuickOpen.svelte`

**Interfaces:**
- Consumes: `app.workspace`, `app.activeRoot`, `app.docSidecars`, `app.homeDir`, `app.recentDocs`, `app.refreshRecents()`, `app.openRecent()`, `app.closeWorkspace()`, `isInside`, `displayDir`.

- [ ] **Step 1: Sidebar** — imports:

```ts
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
```

Replace `app.libraryRoot` with `app.activeRoot` in `onRootDrop` and the "New folder" item. Replace the `<span …>Library</span>` header with:

```svelte
    {#if app.workspace}
      <div class="flex min-w-0 items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          class="size-6 shrink-0"
          title="Back to Library"
          onclick={() => app.closeWorkspace()}
        >
          <ArrowLeft class="size-4" />
        </Button>
        <FolderOpen class="size-3.5 shrink-0 text-muted-foreground" />
        <span class="truncate text-xs font-semibold text-muted-foreground" title={app.workspace}>
          {app.workspace.split("/").pop()}
        </span>
      </div>
    {:else}
      <span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Library</span>
    {/if}
```

- [ ] **Step 2: Toolbar** — import `import { displayDir, isInside } from "$lib/paths";` and add:

```ts
  /** Folder of a document opened from outside the library/workspace. */
  const outsideDir = $derived(
    app.currentPath &&
      !isInside(app.currentPath, app.libraryRoot) &&
      !(app.workspace && isInside(app.currentPath, app.workspace))
      ? displayDir(app.currentPath, app.homeDir)
      : null,
  );
```

After the title `{/if}` (before the dirty dot), add:

```svelte
      {#if outsideDir}
        <span
          class="max-w-56 shrink truncate text-xs text-muted-foreground/80"
          title={app.currentPath}
        >
          {outsideDir}
        </span>
      {/if}
```

Comments button: `disabled={!app.currentPath || app.isMermaidDoc || !app.docSidecars}`; its tooltip content:

```svelte
      <Tooltip.Content>
        {app.docSidecars ? "Comments (⌘⇧C)" : "Not available for files outside the library"}
      </Tooltip.Content>
```

History button and the "Version history…" dropdown item: `disabled={!app.currentPath || !app.docSidecars}`; history tooltip: `{app.docSidecars ? "Version history (⌘Y)" : "Not available for files outside the library"}`.

`revealLibrary()` reveals `app.activeRoot` instead of `app.libraryRoot`.

- [ ] **Step 3: Quick Open recents** — add `import { displayDir, isInside } from "$lib/paths";` (all Rust calls go through `app`, so no `invoke` import). Replace the `results` derivation:

```ts
  interface Item {
    name: string;
    path: string;
    /** Subtitle: folder within the root, or ~/dir for outside documents. */
    where: string;
    recent: boolean;
  }

  const items = $derived.by((): Item[] => {
    const seen = new Set<string>();
    const out: Item[] = [];
    for (const path of app.recentDocs) {
      if (path === app.currentPath) continue;
      seen.add(path);
      const inRoot = isInside(path, app.activeRoot);
      const rel = inRoot ? path.slice(app.activeRoot.length + 1) : "";
      out.push({
        name: path.split("/").pop() ?? path,
        path,
        where: inRoot ? rel.slice(0, Math.max(0, rel.lastIndexOf("/"))) : displayDir(path, app.homeDir),
        recent: true,
      });
    }
    for (const d of app.flatDocs) {
      if (seen.has(d.path)) continue;
      out.push({
        name: d.name,
        path: d.path,
        where: d.rel.includes("/") ? d.rel.slice(0, d.rel.lastIndexOf("/")) : "",
        recent: false,
      });
    }
    return out;
  });

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items.slice(0, 12);
    return items
      .map((d) => {
        const name = d.name.toLowerCase();
        let score = 0;
        if (name.startsWith(q)) score = 3;
        else if (name.includes(q)) score = 2;
        else if (`${d.where}/${name}`.toLowerCase().includes(q)) score = 1;
        return { ...d, score };
      })
      .filter((d) => d.score > 0)
      .sort((a, b) => b.score - a.score) // stable: recents keep their lead on ties
      .slice(0, 12);
  });
```

In the open `$effect`, add `app.refreshRecents();` next to `query = "";`. `openSelected`:

```ts
  function openSelected() {
    const doc = results[selected];
    if (!doc) return;
    if (doc.recent) app.openRecent(doc.path);
    else app.openDoc(doc.path);
    close();
  }
```

In the markup, replace the `{#if doc.rel.includes("/")}…{/if}` subtitle with:

```svelte
          {#if doc.where}
            <span class="ml-auto shrink-0 truncate text-xs text-muted-foreground">{doc.where}</span>
          {/if}
```

- [ ] **Step 4: Check, test, commit**

Run: `pnpm check && pnpm vitest run && pnpm build`
Expected: 0 errors; tests pass; build succeeds.

```bash
git add src/lib/components/Sidebar.svelte src/lib/components/Toolbar.svelte src/lib/components/QuickOpen.svelte
git commit -m "Sidebar workspace header, outside-file hints, recents in Quick Open

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: End-to-end verification on a bundled app

No code unless a check fails (then fix in the owning task's files and commit).

- [ ] **Step 1: Bundle and install**

```bash
pnpm bundle
plutil -p src-tauri/target/release/bundle/macos/Inky.app/Contents/Info.plist | grep -A12 CFBundleDocumentTypes
```

Expected: `CFBundleDocumentTypes` lists `md`, `markdown`, `mmd`. Copy `Inky.app` to `/Applications`, launch once.

- [ ] **Step 2: Default app** — Finder → any `.md` → Get Info → Open with: Inky → Change All…

- [ ] **Step 3: Checklist** (each must hold):
  1. Quit Inky. Double-click `~/some/repo/README.md` → Inky launches showing README (not the last doc); toolbar shows `~/some/repo`; comments/history disabled.
  2. Edit + save → `git -C ~/some/repo status` shows only `README.md` modified; no `.inky-history`, no sidecar.
  3. With Inky running, double-click another `.md` → it opens in the same window.
  4. Inky menu → Install 'inky' Command… → password → `inky ~/some/repo/docs` in Terminal → sidebar shows `docs` with Back to Library; Quick Open and ⌘⇧K search cover that folder.
  5. While a workspace doc is open, Back to Library → sidebar shows library; the doc keeps saving.
  6. `inky "file with spaces.md"` and `inky ./relative.md` open correctly.
  7. Follow a relative link from an outside doc to a sibling `.md` → opens; ⌘[ returns.
  8. File → Open Recent lists the docs with `name — ~/folder`; picking one opens it; Clear Menu empties it.
  9. Dock icon right-click lists recent documents.
  10. Relaunch → the last outside document reopens.
  11. A file inside the library double-clicked from Finder → comments/history enabled (library doc).
  12. MCP: with the toolbar light on, `list_documents` shows only library documents.

- [ ] **Step 4: Final full test run**

Run: `pnpm vitest run && (cd src-tauri && cargo test)`
Expected: all green.
