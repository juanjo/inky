use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Serialize, Deserialize, Default)]
struct Config {
    library: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    name: String,
    path: String,
    is_dir: bool,
    children: Vec<Node>,
}

const DOC_EXTENSIONS: [&str; 3] = ["md", "markdown", "mmd"];

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

fn resolve_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let config = read_config(app);
    let root = match config.library {
        Some(ref p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let docs = app
                .path()
                .document_dir()
                .or_else(|_| app.path().home_dir())
                .map_err(|e| e.to_string())?;
            docs.join("Inky")
        }
    };
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if config.library.as_deref() != Some(root.to_string_lossy().as_ref()) {
        let _ = write_config(
            app,
            &Config {
                library: Some(root.to_string_lossy().into_owned()),
            },
        );
    }
    Ok(root)
}

/// Reject paths that escape the library root.
fn guard(app: &tauri::AppHandle, path: &str) -> Result<PathBuf, String> {
    let root = resolve_root(app)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let candidate = PathBuf::from(path);
    // Canonicalize the deepest existing ancestor so new files are checked too.
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
        resolved.push(part);
    }
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err("path is outside the Inky library".into())
    }
}

const HISTORY_DIR: &str = ".inky-history";
const SNAPSHOT_MIN_INTERVAL_SECS: u64 = 10 * 60;
const SNAPSHOT_KEEP: usize = 20;

/// Before overwriting a document, keep the old version in a hidden history
/// folder next to it — at most one snapshot per 10 minutes, last 20 kept.
fn snapshot(doc: &Path) {
    let Ok(old) = fs::read_to_string(doc) else {
        return;
    };
    let (Some(parent), Some(stem), Some(ext)) = (
        doc.parent(),
        doc.file_stem().and_then(|s| s.to_str()),
        doc.extension().and_then(|e| e.to_str()),
    ) else {
        return;
    };
    let dir = parent.join(HISTORY_DIR);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut mine: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&format!("{stem}.")) && n.ends_with(&format!(".{ext}")))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    mine.sort();
    if let Some(last) = mine.last() {
        if let Ok(meta) = fs::metadata(last) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().map(|e| e.as_secs()).unwrap_or(u64::MAX)
                    < SNAPSHOT_MIN_INTERVAL_SECS
                {
                    return;
                }
            }
        }
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = fs::write(dir.join(format!("{stem}.{ts}.{ext}")), old);
    if mine.len() >= SNAPSHOT_KEEP {
        for stale in &mine[..mine.len() + 1 - SNAPSHOT_KEEP] {
            let _ = fs::remove_file(stale);
        }
    }
}

/// Hidden sidecar file holding a document's comment threads.
fn sidecar_for(doc: &Path) -> Option<PathBuf> {
    let name = doc.file_name()?.to_str()?;
    Some(doc.parent()?.join(format!(".{name}.comments.json")))
}

fn is_doc(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| DOC_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
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

/// Pick "name.md", "name 2.md", ... — first one that doesn't exist yet.
fn unique_path(dir: &Path, stem: &str, ext: Option<&str>) -> PathBuf {
    for i in 1u32.. {
        let candidate = if i == 1 {
            stem.to_string()
        } else {
            format!("{stem} {i}")
        };
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

#[tauri::command]
fn library_root(app: tauri::AppHandle) -> Result<String, String> {
    Ok(resolve_root(&app)?.to_string_lossy().into_owned())
}

#[tauri::command]
fn set_library_root(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err("not a directory".into());
    }
    write_config(
        &app,
        &Config {
            library: Some(path.clone()),
        },
    )?;
    Ok(path)
}

#[tauri::command]
fn list_tree(app: tauri::AppHandle) -> Result<Vec<Node>, String> {
    let root = resolve_root(&app)?;
    Ok(build_tree(&root))
}

#[tauri::command]
fn read_doc(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let p = guard(&app, &path)?;
    fs::read_to_string(p).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_doc(app: tauri::AppHandle, path: String, content: String) -> Result<(), String> {
    let p = guard(&app, &path)?;
    if fs::read_to_string(&p).map(|old| old != content).unwrap_or(false) {
        snapshot(&p);
    }
    fs::write(p, content).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionInfo {
    name: String,
    modified_ms: u64,
    size: u64,
}

fn doc_parts(doc: &Path) -> Result<(&Path, &str, &str), String> {
    let parent = doc.parent().ok_or("no parent")?;
    let stem = doc
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid name")?;
    let ext = doc
        .extension()
        .and_then(|e| e.to_str())
        .ok_or("invalid extension")?;
    Ok((parent, stem, ext))
}

#[tauri::command]
fn list_versions(app: tauri::AppHandle, path: String) -> Result<Vec<VersionInfo>, String> {
    let doc = guard(&app, &path)?;
    let (parent, stem, ext) = doc_parts(&doc)?;
    let dir = parent.join(HISTORY_DIR);
    let mut out: Vec<VersionInfo> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with(&format!("{stem}.")) && name.ends_with(&format!(".{ext}"))) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            out.push(VersionInfo {
                name,
                modified_ms,
                size: meta.len(),
            });
        }
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(out)
}

#[tauri::command]
fn read_version(app: tauri::AppHandle, path: String, version: String) -> Result<String, String> {
    let doc = guard(&app, &path)?;
    let (parent, stem, ext) = doc_parts(&doc)?;
    if version.contains('/')
        || !version.starts_with(&format!("{stem}."))
        || !version.ends_with(&format!(".{ext}"))
    {
        return Err("invalid version name".into());
    }
    fs::read_to_string(parent.join(HISTORY_DIR).join(version)).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchHit {
    path: String,
    name: String,
    line: u32,
    text: String,
}

#[tauri::command]
fn search_library(app: tauri::AppHandle, query: String) -> Result<Vec<SearchHit>, String> {
    let root = resolve_root(&app)?;
    let needle = query.to_lowercase();
    if needle.trim().is_empty() {
        return Ok(Vec::new());
    }
    fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name();
                if name.to_string_lossy().starts_with('.') {
                    continue;
                }
                if p.is_dir() {
                    collect_files(&p, out);
                } else if is_doc(&p) {
                    out.push(p);
                }
            }
        }
    }
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    files.sort();
    let mut hits = Vec::new();
    'outer: for file in files {
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
                if hits.len() >= 300 {
                    break 'outer;
                }
            }
        }
    }
    Ok(hits)
}

#[tauri::command]
fn create_doc(
    app: tauri::AppHandle,
    dir: String,
    name: String,
    ext: String,
    content: String,
) -> Result<String, String> {
    let d = guard(&app, &dir)?;
    if !d.is_dir() {
        return Err("not a directory".into());
    }
    if !DOC_EXTENSIONS.contains(&ext.as_str()) {
        return Err("unsupported extension".into());
    }
    let stem = name.trim().trim_end_matches(&format!(".{ext}")).to_string();
    let stem = if stem.is_empty() { "Untitled".into() } else { stem };
    let path = unique_path(&d, &stem, Some(&ext));
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn create_folder(app: tauri::AppHandle, dir: String, name: String) -> Result<String, String> {
    let d = guard(&app, &dir)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("empty name".into());
    }
    let path = unique_path(&d, name, None);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn rename_path(app: tauri::AppHandle, path: String, new_name: String) -> Result<String, String> {
    let p = guard(&app, &path)?;
    let new_name = new_name.trim();
    if new_name.is_empty() || new_name.contains('/') {
        return Err("invalid name".into());
    }
    let parent = p.parent().ok_or("no parent")?;
    let mut target = parent.join(new_name);
    if p.is_file() && !is_doc(&target) {
        target = parent.join(format!("{new_name}.md"));
    }
    if target.exists() {
        return Err("a file with that name already exists".into());
    }
    fs::rename(&p, &target).map_err(|e| e.to_string())?;
    // Keep the comments sidecar attached to the document.
    if let (Some(old_sc), Some(new_sc)) = (sidecar_for(&p), sidecar_for(&target)) {
        if old_sc.exists() {
            let _ = fs::rename(old_sc, new_sc);
        }
    }
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
fn read_comments(app: tauri::AppHandle, doc_path: String) -> Result<String, String> {
    let doc = guard(&app, &doc_path)?;
    let Some(sc) = sidecar_for(&doc) else {
        return Ok(String::new());
    };
    match fs::read_to_string(sc) {
        Ok(s) => Ok(s),
        Err(_) => Ok(String::new()),
    }
}

#[tauri::command]
fn write_comments(app: tauri::AppHandle, doc_path: String, json: String) -> Result<(), String> {
    let doc = guard(&app, &doc_path)?;
    let sc = sidecar_for(&doc).ok_or("invalid document path")?;
    if json.is_empty() {
        if sc.exists() {
            fs::remove_file(sc).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    fs::write(sc, json).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let p = guard(&app, &path)?;
    trash::delete(&p).map_err(|e| e.to_string())?;
    if let Some(sc) = sidecar_for(&p) {
        if sc.exists() {
            let _ = trash::delete(&sc);
        }
    }
    Ok(())
}

/// Move a file or folder into another folder inside the library.
#[tauri::command]
fn move_path(app: tauri::AppHandle, path: String, target_dir: String) -> Result<String, String> {
    let src = guard(&app, &path)?;
    let dst_dir = guard(&app, &target_dir)?;
    if !dst_dir.is_dir() {
        return Err("target is not a folder".into());
    }
    if dst_dir == src || dst_dir.starts_with(&src) {
        return Err("cannot move a folder into itself".into());
    }
    let name = src.file_name().ok_or("invalid source")?;
    if src.parent() == Some(dst_dir.as_path()) {
        return Ok(src.to_string_lossy().into_owned());
    }
    let mut target = dst_dir.join(name);
    if target.exists() {
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let ext = src.extension().and_then(|e| e.to_str()).map(str::to_string);
        target = unique_path(&dst_dir, &stem, ext.as_deref());
    }
    fs::rename(&src, &target).map_err(|e| e.to_string())?;
    if let (Some(old_sc), Some(new_sc)) = (sidecar_for(&src), sidecar_for(&target)) {
        if old_sc.exists() {
            let _ = fs::rename(old_sc, new_sc);
        }
    }
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

/// Save a pasted image next to the document (in an `assets/` folder) and
/// return the relative path to reference from markdown.
#[tauri::command]
fn save_image(
    app: tauri::AppHandle,
    doc_path: String,
    data_b64: String,
    ext: String,
) -> Result<String, String> {
    use base64::Engine;
    if !["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.as_str()) {
        return Err("unsupported image type".into());
    }
    let doc = guard(&app, &doc_path)?;
    let dir = doc.parent().ok_or("no parent")?.join("assets");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let path = unique_path(&dir, &format!("image-{stamp}"), Some(&ext));
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    Ok(format!("assets/{name}"))
}

/// Print the current page with proper page margins. With `save_path` the PDF is
/// written silently; without it the native print panel opens.
#[tauri::command]
fn doc_mtime(app: tauri::AppHandle, path: String) -> Result<u64, String> {
    let p = guard(&app, &path)?;
    let meta = fs::metadata(p).map_err(|e| e.to_string())?;
    meta.modified()
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// --- app-hosted MCP server (node subprocess in HTTP mode) -------------------

/// GUI apps on macOS get a minimal PATH (no /usr/local/bin, homebrew, nvm…),
/// so `node` must be resolved explicitly.
fn find_node() -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let candidate = Path::new(dir).join("node");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let home = Path::new(&home);
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        home.join(".volta/bin/node"),
        home.join(".asdf/shims/node"),
    ];
    // nvm: pick the newest installed version.
    if let Ok(entries) = fs::read_dir(home.join(".nvm/versions/node")) {
        let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        versions.sort();
        if let Some(latest) = versions.last() {
            candidates.push(latest.join("bin/node"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

struct McpProc(std::sync::Mutex<Option<std::process::Child>>);

fn kill_mcp(state: &McpProc) {
    if let Ok(mut guard) = state.0.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tauri::command]
fn mcp_status(state: tauri::State<McpProc>) -> bool {
    let mut guard = state.0.lock().unwrap();
    match guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(None) => true,
            _ => {
                *guard = None;
                false
            }
        },
        None => false,
    }
}

#[tauri::command]
fn start_mcp(
    app: tauri::AppHandle,
    state: tauri::State<McpProc>,
    port: u16,
) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{port}/mcp");
    {
        let mut guard = state.0.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                return Ok(url);
            }
        }
    }
    let mut script = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("server.bundle.mjs");
    if !script.exists() {
        // Dev fallback: use the bundle (or raw server) from the source tree.
        for candidate in [
            concat!(env!("CARGO_MANIFEST_DIR"), "/../mcp/server.bundle.mjs"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../mcp/server.mjs"),
        ] {
            if Path::new(candidate).exists() {
                script = PathBuf::from(candidate);
                break;
            }
        }
    }
    if !script.exists() {
        return Err(format!("MCP server script not found at {}", script.display()));
    }
    let node = find_node().ok_or_else(|| {
        "Node.js not found — install it (e.g. `brew install node`) to run the MCP server"
            .to_string()
    })?;
    let child = std::process::Command::new(node)
        .arg(&script)
        .arg("--http")
        .arg(port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start node (is Node.js installed?): {e}"))?;
    *state.0.lock().unwrap() = Some(child);
    Ok(url)
}

#[tauri::command]
fn stop_mcp(state: tauri::State<McpProc>) {
    kill_mcp(&state);
}

#[tauri::command]
fn print_document(window: tauri::WebviewWindow, save_path: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        window
            .with_webview(move |webview| unsafe {
                use objc2_app_kit::{NSPrintInfo, NSPrintSaveJob, NSPrintingPaginationMode};
                use objc2_foundation::{NSCopying, NSString, NSURL};

                let wk: &objc2_web_kit::WKWebView = &*webview.inner().cast();
                let print_info = NSPrintInfo::sharedPrintInfo().copy();
                // The CSS lays content out at 6.3in wide; with these margins the
                // printable area is wider than that on both A4 and US Letter, so
                // content is never scaled and horizontal centering yields ~1in
                // effective side margins. (Points are 1/72 inch.)
                print_info.setLeftMargin(54.0);
                print_info.setRightMargin(54.0);
                print_info.setTopMargin(60.0);
                print_info.setBottomMargin(60.0);
                print_info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
                print_info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
                print_info.setHorizontallyCentered(true);
                print_info.setVerticallyCentered(false);

                let saving = save_path.is_some();
                if let Some(path) = save_path {
                    print_info.setJobDisposition(NSPrintSaveJob);
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
                    let dict = print_info.dictionary();
                    dict.setObject_forKey(
                        &url,
                        objc2::runtime::ProtocolObject::from_ref(
                            objc2_app_kit::NSPrintJobSavingURL,
                        ),
                    );
                }

                let op = wk.printOperationWithPrintInfo(&print_info);
                op.setShowsPrintPanel(!saving);
                op.setShowsProgressPanel(false);
                op.setCanSpawnSeparateThread(true);
                if let Some(win) = wk.window() {
                    op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                        &win,
                        None,
                        None,
                        std::ptr::null_mut(),
                    );
                }
            })
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = save_path;
        window.print().map_err(|e| e.to_string())
    }
}

/// Recursively find a check menu item by id and set its checked state.
#[tauri::command]
fn set_menu_checked(app: tauri::AppHandle, id: String, checked: bool) {
    use tauri::menu::MenuItemKind;
    fn walk(items: Vec<MenuItemKind<tauri::Wry>>, id: &str, checked: bool) {
        for item in items {
            match item {
                MenuItemKind::Check(c) => {
                    if c.id().0 == id {
                        let _ = c.set_checked(checked);
                    }
                }
                MenuItemKind::Submenu(s) => {
                    if let Ok(children) = s.items() {
                        walk(children, id, checked);
                    }
                }
                _ => {}
            }
        }
    }
    let _ = app.run_on_main_thread({
        let app = app.clone();
        move || {
            if let Some(menu) = app.menu() {
                if let Ok(items) = menu.items() {
                    walk(items, &id, checked);
                }
            }
        }
    });
}

fn build_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{
        AboutMetadata, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder,
    };
    let handle = app.handle();

    let app_sub = SubmenuBuilder::new(handle, "Inky")
        .about(Some(AboutMetadata::default()))
        .item(&MenuItemBuilder::with_id("check_updates", "Check for Updates…").build(handle)?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        // Custom quit so the frontend can flush unsaved changes first.
        .item(
            &MenuItemBuilder::with_id("quit_app", "Quit Inky")
                .accelerator("CmdOrCtrl+Q")
                .build(handle)?,
        )
        .build()?;

    let file_sub = SubmenuBuilder::new(handle, "File")
        .item(
            &MenuItemBuilder::with_id("new_doc", "New Document")
                .accelerator("CmdOrCtrl+N")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("quick_open", "Open Quickly…")
                .accelerator("CmdOrCtrl+K")
                .build(handle)?,
        )
        .item(&MenuItemBuilder::with_id("new_diagram", "New Mermaid Diagram").build(handle)?)
        .item(
            &MenuItemBuilder::with_id("paste_new", "New from Clipboard")
                .accelerator("CmdOrCtrl+Shift+V")
                .build(handle)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("save", "Save")
                .accelerator("CmdOrCtrl+S")
                .build(handle)?,
        )
        .item(&MenuItemBuilder::with_id("history", "Version History…").build(handle)?)
        .separator()
        .item(
            &MenuItemBuilder::with_id("export_pdf", "Export as PDF…")
                .accelerator("CmdOrCtrl+E")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("print", "Print…")
                .accelerator("CmdOrCtrl+P")
                .build(handle)?,
        )
        .separator()
        .close_window()
        .build()?;

    let edit_sub = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(
            &MenuItemBuilder::with_id("find", "Find…")
                .accelerator("CmdOrCtrl+F")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("search_library", "Search Library…")
                .accelerator("CmdOrCtrl+Shift+K")
                .build(handle)?,
        )
        .build()?;

    let theme_sub = SubmenuBuilder::new(handle, "Theme")
        .item(&CheckMenuItemBuilder::with_id("theme_light", "Light").build(handle)?)
        .item(&CheckMenuItemBuilder::with_id("theme_dark", "Dark").build(handle)?)
        .item(&CheckMenuItemBuilder::with_id("theme_book", "Book").build(handle)?)
        .build()?;

    let width_sub = SubmenuBuilder::new(handle, "Text Width")
        .item(&CheckMenuItemBuilder::with_id("width_default", "Default").build(handle)?)
        .item(&CheckMenuItemBuilder::with_id("width_wide", "Wide").build(handle)?)
        .item(&CheckMenuItemBuilder::with_id("width_xwide", "Wider").build(handle)?)
        .item(&CheckMenuItemBuilder::with_id("width_xxwide", "Widest").build(handle)?)
        .item(&CheckMenuItemBuilder::with_id("width_full", "Full").build(handle)?)
        .build()?;

    let view_sub = SubmenuBuilder::new(handle, "View")
        .item(
            &MenuItemBuilder::with_id("view_reading", "Reading")
                .accelerator("CmdOrCtrl+1")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("view_split", "Split")
                .accelerator("CmdOrCtrl+2")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("view_writing", "Writing")
                .accelerator("CmdOrCtrl+3")
                .build(handle)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("toggle_sidebar", "Toggle Sidebar")
                .accelerator("CmdOrCtrl+B")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("toggle_toc", "Toggle Table of Contents")
                .accelerator("CmdOrCtrl+T")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("toggle_comments", "Toggle Comments")
                .accelerator("CmdOrCtrl+Shift+C")
                .build(handle)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("sync_scroll", "Sync Scrolling in Split")
                .checked(true)
                .build(handle)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("focus_mode", "Focus Mode")
                .accelerator("CmdOrCtrl+Shift+F")
                .checked(false)
                .build(handle)?,
        )
        .separator()
        .items(&[&theme_sub, &width_sub])
        .separator()
        .item(
            &MenuItemBuilder::with_id("font_plus", "Increase Font Size")
                .accelerator("CmdOrCtrl+=")
                .build(handle)?,
        )
        .item(
            &MenuItemBuilder::with_id("font_minus", "Decrease Font Size")
                .accelerator("CmdOrCtrl+-")
                .build(handle)?,
        )
        .separator()
        .fullscreen()
        .build()?;

    let window_sub = SubmenuBuilder::new(handle, "Window")
        .minimize()
        .maximize()
        .build()?;

    let menu = MenuBuilder::new(handle)
        .items(&[&app_sub, &file_sub, &edit_sub, &view_sub, &window_sub])
        .build()?;
    app.set_menu(menu)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Emitter;
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch: focus the existing window instead.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(McpProc(std::sync::Mutex::new(None)))
        .setup(|app| {
            build_menu(app)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            let _ = app.emit("menu", event.id().0.clone());
        })
        .invoke_handler(tauri::generate_handler![
            library_root,
            set_library_root,
            list_tree,
            read_doc,
            write_doc,
            create_doc,
            create_folder,
            rename_path,
            delete_path,
            move_path,
            path_exists,
            print_document,
            set_menu_checked,
            save_image,
            read_comments,
            write_comments,
            search_library,
            list_versions,
            read_version,
            doc_mtime,
            quit_app,
            start_mcp,
            stop_mcp,
            mcp_status
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Don't leave the MCP node subprocess orphaned.
                kill_mcp(&app_handle.state::<McpProc>());
            }
        });
}
