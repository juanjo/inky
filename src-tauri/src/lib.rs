use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

pub mod library;
pub mod mcp;
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

/// Keeps the Open Recent menu and Dock list in sync (filled in by Task 6).
fn on_recents_changed(_app: &tauri::AppHandle) {}

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
        return Ok(to_string(p.doc(&path)?.resolve(&path)?));
    }
    let granted = app.state::<AppPlaces>().0.lock().unwrap().grant_file(Path::new(&path))?;
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
        .manage(McpServer(std::sync::Mutex::new(None)))
        .manage(AppPlaces(std::sync::Mutex::new(Places::default())))
        .manage(LibraryWatcher(std::sync::Mutex::new(None)))
        .manage(UndoStash(std::sync::Mutex::new(std::collections::HashMap::new())))
        .setup(|app| {
            build_menu(app)?;
            purge_stash_dir(app.handle());
            start_watcher(app.handle());
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
            doc_sidecars,
            workspace_root,
            clear_workspace,
            follow_link,
            note_recent,
            recent_docs,
            open_recent,
            quit_app,
            start_mcp,
            stop_mcp,
            mcp_status,
            app_binary_path,
            undo_delete,
            purge_delete
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                stop_mcp_server(&app_handle.state::<McpServer>());
                purge_stash_dir(app_handle);
            }
        });
}
