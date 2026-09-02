//! Everything the app and the MCP server do to the library folder lives here.
//! No Tauri types: this module also runs in `Inky --mcp` (stdio) mode where
//! there is no app handle.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DOC_EXTENSIONS: [&str; 3] = ["md", "markdown", "mmd"];
pub const APP_IDENTIFIER: &str = "com.inky.app";
pub const HISTORY_DIR: &str = ".inky-history";
pub const SNAPSHOT_MIN_INTERVAL_SECS: u64 = 10 * 60;
pub const SNAPSHOT_KEEP: usize = 20;
pub const SEARCH_CAP: usize = 300;
pub const CONTEXT_CHARS: usize = 30;

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

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub name: String,
    pub modified_ms: u64,
    pub size: u64,
    /// True when this snapshot was taken because an agent overwrote the document.
    pub agent: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub line: u32,
    pub text: String,
}

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

/// A delete parked in the stash folder, waiting for undo or purge.
#[derive(Debug, Clone)]
pub struct StashedDelete {
    pub token: String,
    pub original: PathBuf,
    pub stashed: PathBuf,
    /// (original sidecar path, stashed sidecar path)
    pub sidecar: Option<(PathBuf, PathBuf)>,
}

#[derive(Serialize, Deserialize, Default)]
struct Sidecar {
    version: u32,
    #[serde(default)]
    threads: Vec<CommentThread>,
}

// --- free helpers ------------------------------------------------------------

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
            nodes.push(Node {
                name,
                path: path.to_string_lossy().into_owned(),
                is_dir: true,
                children,
            });
        } else if is_doc(&path) {
            nodes.push(Node {
                name,
                path: path.to_string_lossy().into_owned(),
                is_dir: false,
                children: Vec::new(),
            });
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
            let kind = if name.to_lowercase().ends_with(".mmd") {
                EntryKind::Mermaid
            } else {
                EntryKind::Markdown
            };
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

/// History files belonging to `doc`, oldest first. Sorted by modification
/// time rather than name: files written by the earlier JS server use ISO
/// timestamps (`stem.2026-09-01-10-36-00.ext`) which would otherwise sort
/// after every unix-second name (`stem.1756716960.ext`).
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
    mine.sort_by_cached_key(|p| {
        let mtime = fs::metadata(p).and_then(|m| m.modified()).ok();
        (mtime, p.clone())
    });
    mine
}

/// Before overwriting a document, keep the old version in a hidden history
/// folder next to it — at most one snapshot per 10 minutes, last 20 kept.
fn snapshot(doc: &Path, agent: bool) {
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
    let marker = if agent { ".agent" } else { "" };
    let _ = fs::write(dir.join(format!("{stem}.{ts}{marker}.{ext}")), old);
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
pub(crate) fn unique_path(dir: &Path, stem: &str, ext: Option<&str>) -> PathBuf {
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

// --- Library -----------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
    /// Marks snapshots taken by this handle as agent edits (the MCP server).
    agent_origin: bool,
}

impl Library {
    /// Open (creating if needed) a library folder. The root is canonicalised
    /// once so every later containment check compares real paths.
    pub fn open(root: impl Into<PathBuf>) -> Result<Library, String> {
        let root: PathBuf = root.into();
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        Ok(Library { root, agent_origin: false })
    }

    /// Snapshots taken through this handle are tagged as agent edits, so the
    /// history panel can say "replaced by an agent edit".
    pub fn with_agent_origin(mut self) -> Self {
        self.agent_origin = true;
        self
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
        // `symlink_metadata` (not `exists`) so a dangling symlink counts as
        // existing and `canonicalize` below fails instead of following it.
        while fs::symlink_metadata(&existing).is_err() {
            match existing.file_name() {
                Some(name) => suffix.push(name.to_os_string()),
                None => return Err("invalid path".into()),
            }
            existing = existing
                .parent()
                .ok_or_else(|| "invalid path".to_string())?
                .to_path_buf();
        }
        // `file_name()` is `None` for a trailing `..`, so the loop above has
        // already rejected any `..` in the not-yet-existing suffix.
        let mut resolved = existing.canonicalize().map_err(|e| e.to_string())?;
        for part in suffix.iter().rev() {
            resolved.push(part);
        }
        if resolved.starts_with(&self.root) {
            Ok(resolved)
        } else {
            Err(format!("Path escapes the Inky library: {path}"))
        }
    }

    // --- documents ---

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
            snapshot(&p, self.agent_origin);
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
        snapshot(&p, self.agent_origin);
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

    // --- history + search ---

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
                let name = p.file_name()?.to_string_lossy().into_owned();
                let agent = name.rsplit('.').nth(1) == Some("agent");
                Some(VersionInfo { name, modified_ms, size: meta.len(), agent })
            })
            .collect();
        out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| b.name.cmp(&a.name)));
        Ok(out)
    }

    pub fn read_version(&self, path: &str, version: &str) -> Result<String, String> {
        let doc = self.resolve(path)?;
        let (parent, stem, ext) = doc_parts(&doc)?;
        if version.contains('/')
            || !version.starts_with(&format!("{stem}."))
            || !version.ends_with(&format!(".{ext}"))
        {
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

    // --- structure ---

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
            return Err(format!(
                "{} already exists",
                target.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        fs::rename(&p, &target).map_err(|e| e.to_string())?;
        move_sidecar(&p, &target);
        Ok(target)
    }

    /// Move a file or folder into another folder (created if missing). A name
    /// clash picks "name 2.md" rather than failing.
    pub fn move_into(&self, path: &str, target_dir: &str) -> Result<PathBuf, String> {
        let src = self.resolve(path)?;
        let dst_dir = self.resolve(target_dir)?;
        if dst_dir == src || dst_dir.starts_with(&src) {
            return Err("cannot move a folder into itself".into());
        }
        let name = src.file_name().ok_or("invalid source")?;
        fs::create_dir_all(&dst_dir).map_err(|e| e.to_string())?;
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

    /// Move a document or folder (and a document's sidecar) into a stash
    /// folder so the delete can be undone. Fails (e.g. across volumes) rather
    /// than falling back; the caller then uses plain `delete`.
    pub fn stash_delete(&self, path: &str, stash_root: &Path) -> Result<StashedDelete, String> {
        let p = self.resolve(path)?;
        if !p.exists() {
            return Err(format!("{path} does not exist"));
        }
        let token = new_id("undo");
        let dir = stash_root.join(&token);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = p.file_name().ok_or("invalid path")?;
        let stashed = dir.join(name);
        fs::rename(&p, &stashed).map_err(|e| e.to_string())?;
        let mut sidecar = None;
        if let (Some(sc), Some(sc_name)) = (sidecar_for(&p), sidecar_for(&stashed)) {
            if sc.exists() && fs::rename(&sc, &sc_name).is_ok() {
                sidecar = Some((sc, sc_name));
            }
        }
        Ok(StashedDelete { token, original: p, stashed, sidecar })
    }

    /// Put a stashed delete back where it came from (a name clash picks a
    /// unique name). Returns the restored path.
    pub fn restore_stashed(&self, entry: &StashedDelete) -> Result<PathBuf, String> {
        let mut target = entry.original.clone();
        if target.exists() {
            let dir = target.parent().ok_or("invalid path")?.to_path_buf();
            let stem = target.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string();
            let ext = target.extension().and_then(|e| e.to_str()).map(str::to_string);
            target = unique_path(&dir, &stem, ext.as_deref());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::rename(&entry.stashed, &target).map_err(|e| e.to_string())?;
        if let (Some((_, from)), Some(to)) = (entry.sidecar.as_ref(), sidecar_for(&target)) {
            let _ = fs::rename(from, to);
        }
        let _ = fs::remove_dir(entry.stashed.parent().unwrap_or(&entry.stashed));
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

    // --- comments ---

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
        serde_json::from_str::<Sidecar>(&raw)
            .map(|s| s.threads)
            .map_err(|e| format!("comments sidecar for {path} is unreadable: {e}"))
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
}
#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp_lib() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        (dir, lib)
    }

    // --- Task 1: core + guarding -------------------------------------------

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
    fn resolve_rejects_dangling_symlink() {
        let (_d, lib) = temp_lib();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("x.md"), lib.root().join("evil.md")).unwrap();
        assert!(lib.resolve("evil.md").is_err());
        assert!(lib.write("evil.md", "boom").is_err());
        assert!(!outside.path().join("x.md").exists());
    }

    #[test]
    fn history_order_is_by_mtime_even_with_legacy_iso_names() {
        let (_d, lib) = temp_lib();
        lib.write("h.md", "v1").unwrap();
        let dir = lib.root().join(HISTORY_DIR);
        fs::create_dir_all(&dir).unwrap();
        // Legacy (JS-era) name that sorts *after* unix-second names bytewise.
        let legacy = dir.join("h.2026-01-01-00-00-00.md");
        fs::write(&legacy, "legacy").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::open(&legacy).unwrap().set_modified(old).unwrap();
        lib.write("h.md", "v2").unwrap(); // snapshots v1 (legacy is older than 10 min)
        lib.write("h.md", "v3").unwrap(); // must NOT snapshot: the newest is seconds old
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(lib.list_versions("h.md").unwrap()[1].name, "h.2026-01-01-00-00-00.md");
    }

    #[test]
    fn unreadable_sidecar_is_an_error_not_an_empty_list() {
        let (_d, lib) = temp_lib();
        lib.write("u.md", "").unwrap();
        fs::write(lib.root().join(".u.md.comments.json"), "{not json").unwrap();
        assert!(lib.threads("u.md").is_err());
    }

    #[test]
    fn move_into_self_does_not_create_the_target() {
        let (_d, lib) = temp_lib();
        lib.ensure_folder("F").unwrap();
        assert!(lib.move_into("F", "F/G").is_err());
        assert!(!lib.root().join("F/G").exists());
    }

    #[test]
    fn agent_snapshots_are_tagged() {
        let (_d, lib) = temp_lib();
        let agent = lib.clone().with_agent_origin();
        lib.write("t.md", "v1").unwrap();
        agent.write("t.md", "v2").unwrap();
        let versions = lib.list_versions("t.md").unwrap();
        assert_eq!(versions.len(), 1);
        assert!(versions[0].agent, "{versions:?}");
        assert!(versions[0].name.ends_with(".agent.md"));
        assert_eq!(lib.read_version("t.md", &versions[0].name).unwrap(), "v1");
        // A later user edit snapshots untagged, but rate-limiting applies; just
        // check the detection logic directly on a synthetic old-style name.
        let dir = lib.root().join(HISTORY_DIR);
        fs::write(dir.join("t.1000.md"), "x").unwrap();
        let versions = lib.list_versions("t.md").unwrap();
        let plain = versions.iter().find(|v| v.name == "t.1000.md").unwrap();
        assert!(!plain.agent);
    }

    #[test]
    fn stash_delete_round_trip() {
        let (_d, lib) = temp_lib();
        let stash = tempfile::tempdir().unwrap();
        lib.write("s.md", "keep me").unwrap();
        fs::write(lib.root().join(".s.md.comments.json"), "{}").unwrap();
        let entry = lib.stash_delete("s.md", stash.path()).unwrap();
        assert!(!lib.root().join("s.md").exists());
        assert!(!lib.root().join(".s.md.comments.json").exists());
        assert!(entry.stashed.exists());
        let restored = lib.restore_stashed(&entry).unwrap();
        assert_eq!(restored, lib.root().join("s.md"));
        assert_eq!(lib.read("s.md").unwrap(), "keep me");
        assert!(lib.root().join(".s.md.comments.json").exists());
    }

    #[test]
    fn restore_stashed_picks_unique_name_when_occupied() {
        let (_d, lib) = temp_lib();
        let stash = tempfile::tempdir().unwrap();
        lib.write("u.md", "old").unwrap();
        let entry = lib.stash_delete("u.md", stash.path()).unwrap();
        lib.write("u.md", "new").unwrap();
        let restored = lib.restore_stashed(&entry).unwrap();
        assert_eq!(lib.relative(&restored), "u 2.md");
        assert_eq!(lib.read("u 2.md").unwrap(), "old");
        assert_eq!(lib.read("u.md").unwrap(), "new");
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

    // --- Task 2: documents ---------------------------------------------------

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

    // --- Task 3: versions + search ------------------------------------------

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

    // --- Task 4: structure ---------------------------------------------------

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

    // --- Task 5: comments ----------------------------------------------------

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
        assert_eq!(iso_from_unix_millis(1_788_256_800_123), "2026-09-01T10:00:00.123Z");
    }
}
