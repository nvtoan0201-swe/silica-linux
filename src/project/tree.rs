//! The explorer tree: files and folders, loaded lazily as folders expand, with what
//! `.gitignore` excludes marked so the tree can dim it.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The ignore files that apply inside one folder, outermost first.
#[derive(Clone, Default)]
pub struct IgnoreStack {
    files: Vec<Rc<Gitignore>>,
}

impl IgnoreStack {
    /// The project root's rules: the repository's `.git/info/exclude`.
    pub fn for_root(root: &Path) -> IgnoreStack {
        let mut builder = GitignoreBuilder::new(root);
        builder.add(root.join(".git/info/exclude"));
        IgnoreStack::default().adding(builder.build().ok())
    }

    /// Adds a folder's own `.gitignore`.
    pub fn with_folder(&self, folder: &Path) -> IgnoreStack {
        let file = folder.join(".gitignore");
        if !file.is_file() {
            return self.clone();
        }
        let (gitignore, _) = Gitignore::new(file);
        self.adding(Some(gitignore))
    }

    fn adding(&self, file: Option<Gitignore>) -> IgnoreStack {
        let mut stack = self.clone();
        if let Some(file) = file.filter(|f| !f.is_empty()) {
            stack.files.push(Rc::new(file));
        }
        stack
    }

    /// Whether a path is ignored. Deeper files and later rules win.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        for file in self.files.iter().rev() {
            if !path.starts_with(file.path()) {
                continue;
            }
            match file.matched(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }
}

pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// Path from the project root, "" for the root itself.
    pub relative: String,
    /// Matched by a .gitignore rule, itself or through an ignored parent folder.
    pub ignored: bool,
    pub expanded: bool,
    /// `None` until the folder has been expanded for the first time.
    pub children: Option<Vec<FileNode>>,
    /// Ignore rules in effect for this node's parent folder.
    inherited: IgnoreStack,
}

/// Never shown in the tree.
const HIDDEN_NAMES: [&str; 2] = [".DS_Store", ".git"];

impl FileNode {
    pub fn root(path: &Path) -> FileNode {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
        let mut node = FileNode {
            path: path.to_path_buf(),
            name,
            is_dir: true,
            relative: String::new(),
            ignored: false,
            expanded: true,
            children: None,
            inherited: IgnoreStack::for_root(path),
        };
        node.load_children();
        node
    }

    /// Shown dimmed in the tree: dotfiles and gitignored items.
    pub fn is_dimmed(&self) -> bool {
        self.ignored || self.name.starts_with('.')
    }

    pub fn load_children(&mut self) {
        if self.is_dir && self.children.is_none() {
            self.children = Some(self.read_children());
        }
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
        if expanded {
            self.load_children();
        }
    }

    /// Re-reads every loaded folder, keeping existing nodes (and their expansion state).
    pub fn reload(&mut self) {
        let Some(old) = self.children.take() else { return };
        let mut old: Vec<Option<FileNode>> = old.into_iter().map(Some).collect();
        let mut fresh = self.read_children();
        for node in &mut fresh {
            if let Some(kept) = old
                .iter_mut()
                .find(|o| o.as_ref().is_some_and(|o| o.path == node.path && o.is_dir == node.is_dir))
                .and_then(Option::take)
            {
                node.expanded = kept.expanded;
                node.children = kept.children;
            }
            node.reload();
        }
        self.children = Some(fresh);
    }

    pub fn collapse_all(&mut self) {
        if let Some(children) = &mut self.children {
            for child in children {
                child.expanded = false;
                child.collapse_all();
            }
        }
    }

    fn read_children(&self) -> Vec<FileNode> {
        let Ok(entries) = std::fs::read_dir(&self.path) else { return Vec::new() };
        let rules = self.inherited.with_folder(&self.path);
        let mut nodes: Vec<FileNode> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                if HIDDEN_NAMES.contains(&name.as_str()) {
                    return None;
                }
                let path = entry.path();
                // Follows symlinks, so a linked folder opens like a folder.
                let is_dir = std::fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false);
                let relative = if self.relative.is_empty() { name.clone() } else { format!("{}/{}", self.relative, name) };
                let ignored = self.ignored || rules.is_ignored(&path, is_dir);
                Some(FileNode {
                    path,
                    name,
                    is_dir,
                    relative,
                    ignored,
                    expanded: false,
                    children: None,
                    inherited: rules.clone(),
                })
            })
            .collect();
        nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => natural_cmp(&a.name, &b.name),
        });
        nodes
    }

    /// Reads the folders along a project-relative path, without expanding them.
    pub fn load_along(&mut self, relative: &str) {
        let mut parts = relative.split('/');
        self.load_along_parts(&mut parts);
    }

    fn load_along_parts<'a>(&mut self, parts: &mut impl Iterator<Item = &'a str>) {
        self.load_children();
        let Some(name) = parts.next() else { return };
        if let Some(child) = self.children.as_mut().and_then(|c| c.iter_mut().find(|c| c.name == name)) {
            if child.is_dir {
                child.load_along_parts(parts);
            }
        }
    }

    /// The node at a path below this one, if the tree has loaded it.
    pub fn find(&self, path: &Path) -> Option<&FileNode> {
        if self.path == path {
            return Some(self);
        }
        let rest = path.strip_prefix(&self.path).ok()?;
        let mut node = self;
        for component in rest.components() {
            let name = component.as_os_str().to_string_lossy();
            node = node.children.as_ref()?.iter().find(|c| c.name == name)?;
        }
        Some(node)
    }

    pub fn find_mut(&mut self, path: &Path) -> Option<&mut FileNode> {
        if self.path == path {
            return Some(self);
        }
        let rest = path.strip_prefix(&self.path).ok()?.to_path_buf();
        let mut node = self;
        for component in rest.components() {
            let name = component.as_os_str().to_string_lossy().to_string();
            node = node.children.as_mut()?.iter_mut().find(|c| c.name == name)?;
        }
        Some(node)
    }

    /// Expands the folders above `path` so its row exists. Folders that appeared
    /// since the tree last read the disk are picked up on the way.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let Ok(rest) = path.strip_prefix(&self.path) else { return false };
        let components: Vec<String> =
            rest.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        let mut node = self;
        for (index, name) in components.iter().enumerate() {
            let is_last = index + 1 == components.len();
            node.load_children();
            if !node.children.as_ref().is_some_and(|c| c.iter().any(|c| &c.name == name)) {
                node.reload();
            }
            let Some(child) = node.children.as_mut().and_then(|c| c.iter_mut().find(|c| &c.name == name)) else {
                return false;
            };
            if !is_last {
                if !child.is_dir {
                    return false;
                }
                child.set_expanded(true);
            }
            node = child;
        }
        true
    }
}

/// Finder-like name order: case-insensitive, with runs of digits compared as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let order = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut names = vec!["file10", "File2", "file1", "a"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["a", "file1", "File2", "file10"]);
    }
}
