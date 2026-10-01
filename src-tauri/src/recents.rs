//! Recently opened documents: newest first, unique, capped. Pure list
//! operations; `lib.rs` persists the list in `config.json`.

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
