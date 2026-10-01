//! What the app (not MCP) may touch besides the library: at most one
//! workspace folder shown in the sidebar, plus individually granted files.
//! Both are sidecar-less `Library` handles, so `Library::resolve` keeps doing
//! all the confinement. Grants are only ever created from Rust code paths.

use crate::library::{is_doc, Library};
use crate::opens::OpenRequest;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a path lives, and the handle to operate on it with.
pub struct Place {
    lib: Library,
    /// `Some` for a single granted file: `lib` is rooted at its folder, but
    /// only this file may be touched.
    file: Option<PathBuf>,
}

impl Place {
    pub fn sidecars(&self) -> bool {
        self.lib.has_sidecars()
    }

    /// The handle for operating on `path`. A single granted file only
    /// answers for itself — its handle is rooted at the parent folder, so
    /// every other path is refused here.
    pub fn doc(&self, path: &str) -> Result<&Library, String> {
        if let Some(file) = &self.file {
            if self.lib.resolve(path)? != *file {
                return Err(format!("Not an open document: {path}"));
            }
        }
        Ok(&self.lib)
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
    /// The document the window last opened (see `set_current`).
    current: Option<PathBuf>,
}

impl Places {
    pub fn workspace(&self) -> Option<&Library> {
        self.workspace.as_ref()
    }

    /// The root the sidebar, Quick Open and library search show.
    pub fn active<'a>(&'a self, library: &'a Library) -> &'a Library {
        self.workspace.as_ref().unwrap_or(library)
    }

    /// Remember the open document, so replacing the workspace it lives in
    /// keeps it editable.
    pub fn set_current(&mut self, path: PathBuf) {
        self.current = Some(path);
    }

    pub fn set_workspace(&mut self, dir: &Path) -> Result<PathBuf, String> {
        if !dir.is_dir() {
            return Err(format!("Not a folder: {}", dir.display()));
        }
        let lib = Library::open(dir)?.without_sidecars();
        let root = lib.root().to_path_buf();
        let current = self.current.as_ref().map(|p| p.to_string_lossy().into_owned());
        self.close_workspace(current.as_deref());
        self.workspace = Some(lib);
        Ok(root)
    }

    /// Back to the library. `keep` (the open document) stays editable if it
    /// lives in the workspace being closed.
    pub fn clear_workspace(&mut self, keep: Option<&str>) {
        self.close_workspace(keep);
    }

    /// Drop the workspace; `keep` becomes a single-file grant if it is a
    /// Markdown document inside it.
    fn close_workspace(&mut self, keep: Option<&str>) {
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

    /// A path handed over by macOS (Finder, `open`, the CLI, the Rust Open
    /// dialogs): a folder becomes the workspace, a Markdown file is opened —
    /// granted only if no place reaches it already. Anything else: `None`.
    pub fn open_path(&mut self, library: &Library, path: &Path) -> Option<OpenRequest> {
        if path.is_dir() {
            let root = self.set_workspace(path).ok()?;
            return Some(OpenRequest::Folder { path: root.to_string_lossy().into_owned() });
        }
        if !is_doc(path) {
            return None;
        }
        let canonical = path.canonicalize().ok()?;
        let canon = canonical.to_string_lossy().into_owned();
        if self.find(library, &canon).is_err() {
            self.grant_file(&canonical).ok()?;
        }
        Some(OpenRequest::File { path: canon })
    }

    /// `open_recent`: only paths in the persisted `recent` list; reachable
    /// ones pass through, outside ones are granted.
    pub fn grant_recent(&mut self, library: &Library, recent: &[String], path: &str) -> Result<PathBuf, String> {
        if !recent.iter().any(|r| r == path) {
            return Err("Not a recent document".into());
        }
        if let Ok(place) = self.find(library, path) {
            return place.doc(path)?.resolve(path);
        }
        self.grant_file(Path::new(path))
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
        self.follow_link_from(library, from_doc, href, dirs::home_dir().as_deref())
    }

    /// `follow_link` with the home folder passed in (tests use a fake one).
    fn follow_link_from(
        &mut self,
        library: &Library,
        from_doc: &str,
        href: &str,
        home: Option<&Path>,
    ) -> Result<LinkTarget, String> {
        let place = self.find(library, from_doc)?;
        let lib = place.doc(from_doc)?;
        let from = lib.resolve(from_doc)?;
        let end = href.find(['#', '?']).unwrap_or(href.len());
        let decoded = percent_decode(&href[..end]).ok_or_else(|| format!("Linked file not found: {href}"))?;
        // The renderer percent-encodes hrefs (`<My Notes.md>` → `My%20Notes.md`),
        // so the document may hold either spelling.
        let text = lib.read(from_doc)?;
        if decoded.is_empty() || !(text.contains(href) || text.contains(&decoded)) {
            return Err(format!("Not a link in this document: {href}"));
        }
        let dir = from.parent().ok_or("invalid path")?;
        let target = dir
            .join(decoded)
            .canonicalize()
            .map_err(|_| format!("Linked file not found: {href}"))?;
        let path = target.to_string_lossy().into_owned();
        let reachable = self.find(library, &path).is_ok();
        if !reachable && off_limits(&target, home) {
            return Err(format!("Linked file not found: {href}"));
        }
        if !target.is_file() || !is_doc(&target) {
            return Ok(LinkTarget { path, doc: false });
        }
        if !reachable {
            self.grant_file(&target)?;
        }
        Ok(LinkTarget { path, doc: true })
    }
}

/// Hidden folders and `~/Library` are never reached through a link grant:
/// the "href occurs in the document" check is no security boundary, since
/// the webview can write documents.
fn off_limits(target: &Path, home: Option<&Path>) -> bool {
    let hidden = target
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    hidden || home.is_some_and(|h| target.starts_with(h.join("Library")))
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

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
        let lib_dir = tempdir();
        let out_dir = tempdir();
        let library = Library::open(lib_dir.path()).unwrap();
        let outside = out_dir.path().canonicalize().unwrap();
        fs::write(outside.join("a.md"), "A").unwrap();
        fs::write(outside.join("b.md"), "B").unwrap();
        fs::write(outside.join("notes.txt"), "T").unwrap();
        fs::write(library.root().join("in.md"), "IN").unwrap();
        Fixture { _dirs: vec![lib_dir, out_dir], library, outside }
    }

    /// Not `tempfile::tempdir()`: its `.tmpXXXX` names are hidden folders,
    /// which link grants refuse.
    fn tempdir() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("inky-test").tempdir().unwrap()
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
        place.doc(&s(&a)).unwrap().write(&s(&a), "A2").unwrap();
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
        let link_dir = tempdir();
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
    fn replacing_workspace_keeps_current_doc_writable() {
        let f = fixture();
        let other = tempdir();
        let mut places = Places::default();
        places.set_workspace(&f.outside).unwrap();
        let a = f.outside.join("a.md");
        places.set_current(a.clone());
        places.set_workspace(other.path()).unwrap();
        let place = places.find(&f.library, &s(&a)).expect("open doc stays reachable");
        place.doc(&s(&a)).unwrap().write(&s(&a), "A3").unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), "A3");
        assert!(places.find(&f.library, &s(&f.outside.join("b.md"))).is_err());
    }

    #[test]
    fn clear_workspace_ignores_keep_outside_it() {
        let f = fixture();
        let ws = tempdir();
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
        let z = place.doc(&s(&a)).unwrap().rename(&s(&a), "z.md").unwrap();
        places.renamed(&a, &z);
        assert!(places.find(&f.library, &s(&z)).is_ok());
        assert!(places.find(&f.library, &s(&a)).is_err());
    }

    #[test]
    fn follow_link_grants_markdown_targets_only() {
        let f = fixture();
        let mut places = Places::default();
        fs::write(f.outside.join("a.md"), "[b](b.md) [t](notes.txt) [m](missing.md)").unwrap();
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
        fs::write(f.library.root().join("in.md"), "[o](other.md)").unwrap();
        let mut places = Places::default();
        let from = s(&f.library.root().join("in.md"));
        let t = places.follow_link(&f.library, &from, "other.md").unwrap();
        assert!(t.doc);
        assert!(places.find(&f.library, &t.path).unwrap().sidecars());
    }

    #[test]
    fn follow_link_refuses_hrefs_not_in_the_document() {
        let f = fixture();
        let mut places = Places::default();
        fs::write(f.outside.join("a.md"), "[b](b.md)").unwrap();
        let a = s(&places.grant_file(&f.outside.join("a.md")).unwrap());
        assert!(places.follow_link(&f.library, &a, "/abs/elsewhere.md").is_err());
        assert!(places.follow_link(&f.library, &a, "../x/b.md").is_err());
        assert!(places.find(&f.library, &s(&f.outside.join("b.md"))).is_err());
    }

    #[test]
    fn follow_link_strips_fragment_and_decodes() {
        let f = fixture();
        let mut places = Places::default();
        fs::write(f.outside.join("My Notes.md"), "N").unwrap();
        fs::write(f.outside.join("a.md"), "[n](My%20Notes.md#intro)").unwrap();
        let a = s(&places.grant_file(&f.outside.join("a.md")).unwrap());
        let t = places.follow_link(&f.library, &a, "My%20Notes.md#intro").unwrap();
        assert!(t.doc);
        assert!(t.path.ends_with("My Notes.md"));
        assert!(places.find(&f.library, &t.path).is_ok());
    }

    #[test]
    fn follow_link_accepts_rendered_encoding_of_angle_bracket_links() {
        // `[x](<../dir/My Notes.md>)` renders as href="../dir/My%20Notes.md".
        let f = fixture();
        let mut places = Places::default();
        fs::create_dir(f.outside.join("sub")).unwrap();
        fs::write(f.outside.join("sub/é Notes.md"), "N").unwrap();
        fs::create_dir(f.outside.join("start")).unwrap();
        fs::write(f.outside.join("start/cat.md"), "[x](<../sub/é Notes.md#top>)").unwrap();
        let cat = s(&places.grant_file(&f.outside.join("start/cat.md")).unwrap());
        let t = places
            .follow_link(&f.library, &cat, "../sub/%C3%A9%20Notes.md#top")
            .unwrap();
        assert!(t.doc);
        assert!(t.path.ends_with("sub/é Notes.md"));
    }

    #[test]
    fn follow_link_refuses_hidden_folders_outside_places() {
        let f = fixture();
        let mut places = Places::default();
        fs::create_dir(f.outside.join(".secret")).unwrap();
        fs::write(f.outside.join(".secret/x.md"), "X").unwrap();
        fs::write(f.outside.join(".secret/t.txt"), "T").unwrap();
        fs::write(f.outside.join("a.md"), "[x](.secret/x.md) [t](.secret/t.txt)").unwrap();
        let a = s(&places.grant_file(&f.outside.join("a.md")).unwrap());
        assert!(places.follow_link(&f.library, &a, ".secret/x.md").is_err());
        assert!(places.follow_link(&f.library, &a, ".secret/t.txt").is_err());
        assert!(places.find(&f.library, &s(&f.outside.join(".secret/x.md"))).is_err());
    }

    #[test]
    fn follow_link_refuses_home_library_outside_places() {
        let f = fixture();
        let mut places = Places::default();
        // `outside` plays the home folder here.
        fs::create_dir(f.outside.join("Library")).unwrap();
        fs::write(f.outside.join("Library/x.md"), "X").unwrap();
        fs::write(f.outside.join("a.md"), "[x](Library/x.md) [b](b.md)").unwrap();
        let a = s(&places.grant_file(&f.outside.join("a.md")).unwrap());
        let home = Some(f.outside.as_path());
        assert!(places.follow_link_from(&f.library, &a, "Library/x.md", home).is_err());
        assert!(places.find(&f.library, &s(&f.outside.join("Library/x.md"))).is_err());
        assert!(places.follow_link_from(&f.library, &a, "b.md", home).unwrap().doc);
    }

    #[test]
    fn follow_link_reaches_hidden_folders_inside_places() {
        let f = fixture();
        fs::create_dir(f.library.root().join(".notes")).unwrap();
        fs::write(f.library.root().join(".notes/x.md"), "X").unwrap();
        fs::write(f.library.root().join("in.md"), "[x](.notes/x.md)").unwrap();
        let mut places = Places::default();
        let from = s(&f.library.root().join("in.md"));
        let t = places.follow_link(&f.library, &from, ".notes/x.md").unwrap();
        assert!(t.doc);
        // Same for a workspace that is itself under ~/Library.
        let home = f.outside.clone();
        fs::create_dir_all(home.join("Library/ws")).unwrap();
        fs::write(home.join("Library/ws/a.md"), "[b](b.md)").unwrap();
        fs::write(home.join("Library/ws/b.md"), "B").unwrap();
        places.set_workspace(&home.join("Library/ws")).unwrap();
        let a = s(&home.join("Library/ws/a.md"));
        assert!(places.follow_link_from(&f.library, &a, "b.md", Some(&home)).unwrap().doc);
    }

    #[test]
    fn open_path_folder_becomes_the_workspace() {
        let f = fixture();
        let mut places = Places::default();
        let req = places.open_path(&f.library, &f.outside);
        assert_eq!(req, Some(OpenRequest::Folder { path: s(&f.outside) }));
        assert_eq!(places.workspace().unwrap().root(), f.outside.as_path());
    }

    #[test]
    fn open_path_outside_markdown_is_granted_alone() {
        let f = fixture();
        let mut places = Places::default();
        let a = f.outside.join("a.md");
        let req = places.open_path(&f.library, &a);
        assert_eq!(req, Some(OpenRequest::File { path: s(&a) }));
        assert!(places.find(&f.library, &s(&a)).is_ok());
        assert!(places.find(&f.library, &s(&f.outside.join("b.md"))).is_err());
        assert!(places.workspace().is_none());
    }

    #[test]
    fn open_path_library_markdown_adds_no_grant() {
        let f = fixture();
        let mut places = Places::default();
        let inside = f.library.root().join("in.md");
        let req = places.open_path(&f.library, &inside);
        assert_eq!(req, Some(OpenRequest::File { path: s(&inside) }));
        assert!(places.files.is_empty(), "the library already reaches it");
        assert!(places.find(&f.library, &s(&inside)).unwrap().sidecars());
    }

    #[test]
    fn open_path_ignores_other_files_and_missing_paths() {
        let f = fixture();
        let mut places = Places::default();
        assert_eq!(places.open_path(&f.library, &f.outside.join("notes.txt")), None);
        assert_eq!(places.open_path(&f.library, &f.outside.join("missing.md")), None);
        assert!(places.find(&f.library, &s(&f.outside.join("notes.txt"))).is_err());
        assert!(places.workspace().is_none());
    }

    #[test]
    fn grant_recent_only_grants_listed_paths() {
        let f = fixture();
        let mut places = Places::default();
        let a = s(&f.outside.join("a.md"));
        let b = s(&f.outside.join("b.md"));
        let inside = s(&f.library.root().join("in.md"));
        let recent = vec![a.clone(), inside.clone()];
        assert!(places.grant_recent(&f.library, &recent, &b).is_err());
        assert!(places.find(&f.library, &b).is_err());
        assert_eq!(places.grant_recent(&f.library, &recent, &inside).unwrap(), PathBuf::from(&inside));
        assert!(places.files.is_empty(), "library paths pass through without a grant");
        assert_eq!(places.grant_recent(&f.library, &recent, &a).unwrap(), PathBuf::from(&a));
        assert!(places.find(&f.library, &a).is_ok());
    }

    #[test]
    fn single_file_place_refuses_other_paths() {
        let f = fixture();
        let mut places = Places::default();
        let a = s(&places.grant_file(&f.outside.join("a.md")).unwrap());
        let place = places.find(&f.library, &a).unwrap();
        assert!(place.doc(&s(&f.outside.join("b.md"))).is_err());
        assert!(place.doc(&a).is_ok());
    }
}
