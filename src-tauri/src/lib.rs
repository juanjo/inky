use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

pub mod library;
pub mod mcp;
pub mod opens;
pub mod places;
pub mod recents;

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
        let _ = write_config(app, &Config { library: Some(stored), ..read_config(app) });
    }
    Ok(lib)
}

fn to_string(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

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
    let state = app.state::<AppPlaces>();
    let places = state.0.lock().unwrap();
    Ok(places.active(&lib).clone())
}

fn update_recents(app: &tauri::AppHandle, f: impl FnOnce(&mut Vec<String>)) {
    let mut config = read_config(app);
    f(&mut config.recent);
    let _ = write_config(app, &config);
    on_recents_changed(app);
}

/// File → Open Recent, plus the list its `open_recent:N` items index into.
#[derive(Default)]
struct RecentMenuState {
    sub: Option<tauri::menu::Submenu<tauri::Wry>>,
    list: Vec<String>,
}

struct RecentMenu(std::sync::Mutex<RecentMenuState>);

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

/// Rebuild File → Open Recent from the persisted list. Holds the `RecentMenu`
/// guard throughout, so nothing here may call back into `update_recents`.
fn on_recents_changed(app: &tauri::AppHandle) {
    use tauri::menu::{MenuItemBuilder, PredefinedMenuItem};
    let state = app.state::<RecentMenu>();
    let mut guard = state.0.lock().unwrap();
    let list = recents::existing(&read_config(app).recent);
    guard.list = list.clone();
    let Some(sub) = guard.sub.as_ref() else { return };
    if let Ok(items) = sub.items() {
        for item in items {
            let _ = sub.remove(&item);
        }
    }
    let home = app.path().home_dir().ok();
    if list.is_empty() {
        if let Ok(item) = MenuItemBuilder::with_id("recent_none", "No Recent Documents")
            .enabled(false)
            .build(app)
        {
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
        // SAFETY: a nil sender is documented as valid.
        unsafe { NSDocumentController::sharedDocumentController(mtm).clearRecentDocuments(None) };
    });
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

use opens::{OpenQueue, OpenRequest};

struct PendingOpens(std::sync::Mutex<OpenQueue>);

/// Files become granted documents, folders become the workspace. A file that
/// is already in the library (or workspace) is opened as such — no grant.
fn handle_open_paths(app: &tauri::AppHandle, paths: Vec<PathBuf>) {
    use tauri::Emitter;
    let Ok(lib) = open_library(app) else { return };
    for path in paths {
        // Guard dropped at the end of this statement, before the watcher and emit.
        let req = app.state::<AppPlaces>().0.lock().unwrap().open_path(&lib, &path);
        let Some(req) = req else { continue };
        if matches!(req, OpenRequest::Folder { .. }) {
            start_watcher(app);
        }
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

#[tauri::command]
fn library_root(app: tauri::AppHandle) -> Result<String, String> {
    Ok(open_library(&app)?.root().to_string_lossy().into_owned())
}

#[tauri::command]
fn set_library_root(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let root = to_string(Library::open(&path)?.root().to_path_buf());
    write_config(&app, &Config { library: Some(root.clone()), ..read_config(&app) })?;
    start_watcher(&app);
    Ok(root)
}

#[tauri::command]
fn list_tree(app: tauri::AppHandle) -> Result<Vec<Node>, String> {
    Ok(active_library(&app)?.tree())
}

#[tauri::command]
fn read_doc(app: tauri::AppHandle, path: String) -> Result<String, String> {
    place(&app, &path)?.doc(&path)?.read(&path)
}

#[tauri::command]
fn write_doc(app: tauri::AppHandle, path: String, content: String) -> Result<(), String> {
    place(&app, &path)?.doc(&path)?.write(&path, &content)
}

#[tauri::command]
fn doc_sidecars(app: tauri::AppHandle, path: String) -> Result<bool, String> {
    Ok(place(&app, &path)?.sidecars())
}

#[tauri::command]
fn list_versions(app: tauri::AppHandle, path: String) -> Result<Vec<VersionInfo>, String> {
    place(&app, &path)?.doc(&path)?.list_versions(&path)
}

#[tauri::command]
fn read_version(app: tauri::AppHandle, path: String, version: String) -> Result<String, String> {
    place(&app, &path)?.doc(&path)?.read_version(&path, &version)
}

#[tauri::command]
fn search_library(app: tauri::AppHandle, query: String) -> Result<Vec<SearchHit>, String> {
    Ok(active_library(&app)?.search(&query))
}

#[tauri::command]
fn create_doc(
    app: tauri::AppHandle,
    dir: String,
    name: String,
    ext: String,
    content: String,
) -> Result<String, String> {
    place(&app, &dir)?.folder()?.create_doc_unique(&dir, &name, &ext, &content).map(to_string)
}

#[tauri::command]
fn create_folder(app: tauri::AppHandle, dir: String, name: String) -> Result<String, String> {
    place(&app, &dir)?.folder()?.create_folder_unique(&dir, &name).map(to_string)
}

#[tauri::command]
fn rename_path(app: tauri::AppHandle, path: String, new_name: String) -> Result<String, String> {
    let p = place(&app, &path)?;
    let lib = p.doc(&path)?;
    let old = lib.resolve(&path)?;
    let new = lib.rename(&path, &new_name)?;
    app.state::<AppPlaces>().0.lock().unwrap().renamed(&old, &new);
    let (from, to) = (to_string(old), to_string(new.clone()));
    update_recents(&app, |r| recents::rename(r, &from, &to));
    Ok(to_string(new))
}

#[tauri::command]
fn read_comments(app: tauri::AppHandle, doc_path: String) -> Result<String, String> {
    place(&app, &doc_path)?.doc(&doc_path)?.raw_comments(&doc_path)
}

#[tauri::command]
fn write_comments(app: tauri::AppHandle, doc_path: String, json: String) -> Result<(), String> {
    place(&app, &doc_path)?.doc(&doc_path)?.write_raw_comments(&doc_path, &json)
}

/// Returns an undo token while the delete is parked in the stash; `None` when
/// stashing wasn't possible (e.g. another volume) and it went straight to the
/// Trash.
#[tauri::command]
fn delete_path(
    app: tauri::AppHandle,
    state: tauri::State<UndoStash>,
    path: String,
) -> Result<Option<String>, String> {
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

/// Move a file or folder into another folder inside the library.
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
fn workspace_root(app: tauri::AppHandle) -> Option<String> {
    let state = app.state::<AppPlaces>();
    let places = state.0.lock().unwrap();
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
    let abs = to_string(place(&app, &path)?.doc(&path)?.resolve(&path)?);
    // Guard dropped at the end of this statement, before `update_recents`.
    app.state::<AppPlaces>().0.lock().unwrap().set_current(PathBuf::from(&abs));
    let a = abs.clone();
    update_recents(&app, |r| recents::push(r, &a));
    note_dock_recent(&app, abs);
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
    let lib = open_library(&app)?;
    let recent = read_config(&app).recent;
    let granted = app.state::<AppPlaces>().0.lock().unwrap().grant_recent(&lib, &recent, &path)?;
    Ok(to_string(granted))
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
    let doc = place(&app, &doc_path)?.doc(&doc_path)?.resolve(&doc_path)?;
    let dir = doc.parent().ok_or("no parent")?.join("assets");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let path = library::unique_path(&dir, &format!("image-{stamp}"), Some(&ext));
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    Ok(format!("assets/{name}"))
}

#[tauri::command]
fn doc_mtime(app: tauri::AppHandle, path: String) -> Result<u64, String> {
    place(&app, &path)?.doc(&path)?.mtime(&path)
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// --- live library watching ---------------------------------------------------

struct LibraryWatcher(std::sync::Mutex<Option<notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>>>);

/// Watch the library folder and tell the frontend when anything changes, so
/// agent edits show up live instead of on the next focus.
fn start_watcher(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let state = app.state::<LibraryWatcher>();
    state.0.lock().unwrap().take();
    let Ok(lib) = active_library(app) else { return };
    let handle = app.clone();
    let debouncer = notify_debouncer_mini::new_debouncer(
        std::time::Duration::from_millis(400),
        move |result: notify_debouncer_mini::DebounceEventResult| {
            if result.is_ok() {
                let _ = handle.emit("library-changed", ());
            }
        },
    );
    let Ok(mut debouncer) = debouncer else { return };
    if debouncer
        .watcher()
        .watch(lib.root(), notify_debouncer_mini::notify::RecursiveMode::Recursive)
        .is_ok()
    {
        *state.0.lock().unwrap() = Some(debouncer);
    }
}

// --- undoable delete ---------------------------------------------------------

struct UndoStash(std::sync::Mutex<std::collections::HashMap<String, library::StashedDelete>>);

fn stash_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("undo");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Trash anything still sitting in the stash (leftovers of undone timers or a
/// crashed session), so a stashed delete always ends up in the Trash.
fn purge_stash_dir(app: &tauri::AppHandle) {
    let Ok(dir) = stash_root(app) else { return };
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = trash::delete(entry.path());
        }
    }
}

#[tauri::command]
fn undo_delete(app: tauri::AppHandle, state: tauri::State<UndoStash>, token: String) -> Result<String, String> {
    let entry = state
        .0
        .lock()
        .unwrap()
        .remove(&token)
        .ok_or("nothing to undo")?;
    let restored = open_library(&app)?.restore_stashed(&entry)?;
    Ok(restored.to_string_lossy().into_owned())
}

#[tauri::command]
fn purge_delete(state: tauri::State<UndoStash>, token: String) {
    if let Some(entry) = state.0.lock().unwrap().remove(&token) {
        let _ = trash::delete(&entry.stashed);
        if let Some((_, sc)) = entry.sidecar {
            let _ = trash::delete(sc);
        }
        let _ = fs::remove_dir(entry.stashed.parent().unwrap_or(&entry.stashed));
    }
}

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
async fn start_mcp(
    app: tauri::AppHandle,
    state: tauri::State<'_, McpServer>,
    port: u16,
) -> Result<String, String> {
    if let Some(handle) = state.0.lock().unwrap().as_ref() {
        if handle.is_running() {
            return Ok(handle.url());
        }
    }
    let lib = open_library(&app)?;
    let handle = mcp::serve_http(lib, port).await?;
    let url = handle.url();
    if let Some(old) = state.0.lock().unwrap().replace(handle) {
        old.stop();
    }
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

// --- CLI installer ----------------------------------------------------------

/// `/usr/local/bin/inky`: hands files/folders to Inky through macOS `open`,
/// so they arrive exactly like a Finder double-click.
const CLI_SCRIPT: &str = "#!/bin/sh\n# inky — open Markdown files or folders in Inky\nexec open -b com.inky.app \"$@\"\n";
const CLI_PATH: &str = "/usr/local/bin/inky";

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The privileged shell command: writes `CLI_SCRIPT` straight to `CLI_PATH`.
fn install_shell_command() -> String {
    format!(
        "mkdir -p /usr/local/bin && printf '%s' {} > {CLI_PATH} && chmod 755 {CLI_PATH}",
        shell_quote(CLI_SCRIPT)
    )
}

/// AppleScript that runs `sh` as root behind the system password prompt.
fn applescript_admin(sh: &str) -> String {
    format!(
        "do shell script \"{}\" with administrator privileges",
        sh.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Install the `inky` command (one administrator prompt). Async so the
/// password dialog doesn't block the main thread.
#[tauri::command]
async fn install_cli() -> Result<String, String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(applescript_admin(&install_shell_command()))
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(CLI_PATH.into());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if err.is_empty() { "Could not install the command".into() } else { err })
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
        .item(&MenuItemBuilder::with_id("install_cli", "Install 'inky' Command in PATH…").build(handle)?)
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

    let recent_sub = SubmenuBuilder::new(handle, "Open Recent").build()?;
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
        .item(
            &MenuItemBuilder::with_id("history", "Version History…")
                .accelerator("CmdOrCtrl+Y")
                .build(handle)?,
        )
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
    app.state::<RecentMenu>().0.lock().unwrap().sub = Some(recent_sub);
    on_recents_changed(handle);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Emitter;
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            // Second launch: focus the existing window, open any paths given.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            handle_open_paths(app, paths_from_args(args.into_iter().skip(1), Path::new(&cwd)));
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(McpServer(std::sync::Mutex::new(None)))
        .manage(AppPlaces(std::sync::Mutex::new(Places::default())))
        .manage(PendingOpens(std::sync::Mutex::new(OpenQueue::default())))
        .manage(RecentMenu(std::sync::Mutex::new(RecentMenuState::default())))
        .manage(LibraryWatcher(std::sync::Mutex::new(None)))
        .manage(UndoStash(std::sync::Mutex::new(std::collections::HashMap::new())))
        .setup(|app| {
            build_menu(app)?;
            purge_stash_dir(app.handle());
            start_watcher(app.handle());
            if let Ok(cwd) = std::env::current_dir() {
                handle_open_paths(app.handle(), paths_from_args(std::env::args().skip(1), &cwd));
            }
            Ok(())
        })
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
                    // Index the list the menu was built from; clone it out so
                    // the guard is released before `handle_open_paths`.
                    let idx: usize = id["open_recent:".len()..].parse().unwrap_or(usize::MAX);
                    let path = app.state::<RecentMenu>().0.lock().unwrap().list.get(idx).cloned();
                    if let Some(path) = path {
                        handle_open_paths(app, vec![PathBuf::from(path)]);
                    }
                }
                _ => {
                    let _ = app.emit("menu", id);
                }
            }
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
            doc_sidecars,
            workspace_root,
            clear_workspace,
            follow_link,
            note_recent,
            recent_docs,
            open_recent,
            take_pending_opens,
            quit_app,
            install_cli,
            start_mcp,
            stop_mcp,
            mcp_status,
            app_binary_path,
            undo_delete,
            purge_delete
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
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
}

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

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("/a b/c"), "'/a b/c'");
        assert_eq!(shell_quote("/it's"), r"'/it'\''s'");
    }

    #[test]
    fn applescript_admin_escapes_backslashes_then_quotes() {
        let sh = r#"printf '%s' 'it'\''s "q" \n'"#;
        assert_eq!(
            applescript_admin(sh),
            r#"do shell script "printf '%s' 'it'\\''s \"q\" \\n'" with administrator privileges"#
        );
    }

    #[test]
    fn install_command_writes_the_script_verbatim() {
        let sh = install_shell_command();
        assert!(sh.starts_with("mkdir -p /usr/local/bin && printf '%s' "));
        assert!(sh.ends_with(&format!(" > {CLI_PATH} && chmod 755 {CLI_PATH}")));
        // The quoted script, fed through a real shell, is the script itself.
        let printf = &sh[sh.find("printf").unwrap()..sh.find(" > ").unwrap()];
        let out = std::process::Command::new("sh").arg("-c").arg(printf).output().unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), CLI_SCRIPT);
    }

    #[test]
    fn cli_script_opens_by_bundle_id_and_passes_all_args() {
        assert!(CLI_SCRIPT.starts_with("#!/bin/sh\n"));
        assert!(CLI_SCRIPT.contains(r#"exec open -b com.inky.app "$@""#));
    }
}
