//! Lists a project's files for Quick Open and the tree filter, leaving out what git
//! would ignore.

use std::collections::HashSet;
use std::path::Path;

use ignore::WalkBuilder;

use super::fuzzy::FileItem;
use super::tree::natural_cmp;

pub const MAX_FILES: usize = 50_000;

/// Always skipped, even without a .gitignore: version-control metadata,
/// dependencies and build output.
const SKIPPED_FOLDERS: [&str; 8] =
    [".git", ".hg", ".svn", "node_modules", ".build", ".swiftpm", "DerivedData", "Pods"];

pub fn scan(root: &Path) -> Vec<FileItem> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .require_git(false)
        .ignore(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|entry| {
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            !(is_dir && entry.depth() > 0 && SKIPPED_FOLDERS.contains(&&*entry.file_name().to_string_lossy()))
        })
        .build();
    let mut items = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file() || t.is_symlink()) {
            continue;
        }
        if entry.file_name() == ".DS_Store" {
            continue;
        }
        let path = entry.into_path();
        let Ok(relative) = path.strip_prefix(root) else { continue };
        let relative = relative.to_string_lossy().to_string();
        items.push(FileItem::new(path, relative));
        if items.len() >= MAX_FILES {
            break;
        }
    }
    items.sort_by(|a, b| natural_cmp(&a.relative, &b.relative));
    items
}

/// The tree filter's result: matching files and every folder above them.
pub struct FilterResult {
    pub visible: HashSet<String>,
    pub match_count: usize,
}

/// Case-insensitive substring match on names, as in Xcode's navigator filter: files
/// whose names contain the text, plus everything inside folders whose names do.
pub fn filter(items: &[FileItem], query: &str) -> FilterResult {
    let needle = query.to_lowercase();
    let mut visible = HashSet::new();
    let mut match_count = 0;
    for item in items {
        let components: Vec<&str> = item.relative.split('/').collect();
        let Some((name, folders)) = components.split_last() else { continue };
        let matches = name.to_lowercase().contains(&needle)
            || folders.iter().any(|f| f.to_lowercase().contains(&needle));
        if !matches {
            continue;
        }
        match_count += 1;
        let mut path = String::new();
        for component in &components {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(component);
            visible.insert(path.clone());
        }
    }
    FilterResult { visible, match_count }
}

/// Where `query` appears in `name` (byte range), for highlighting.
pub fn match_range(query: &str, name: &str) -> Option<std::ops::Range<usize>> {
    if query.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    // Lowercasing can change byte lengths outside ASCII; only highlight when it doesn't.
    if lower.len() != name.len() {
        return None;
    }
    let start = lower.find(&query.to_lowercase())?;
    Some(start..start + query.len())
}
