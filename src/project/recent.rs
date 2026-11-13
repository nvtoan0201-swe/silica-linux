//! The projects the user has opened, most recent first, for the welcome window and
//! the Open Recent list.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentProject {
    pub name: String,
    pub path: PathBuf,
    /// Seconds since 1970.
    pub last_opened: u64,
}

impl RecentProject {
    /// Path with the home folder abbreviated to `~`.
    pub fn display_path(&self) -> String {
        display_path(&self.path)
    }
}

pub fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if path == home {
            return "~".into();
        }
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

const MAX_COUNT: usize = 15;

fn store_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("silica").join("recent-projects.json"))
}

pub fn load() -> Vec<RecentProject> {
    store_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn save(projects: &[RecentProject]) {
    let Some(path) = store_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_vec_pretty(projects) {
        let _ = std::fs::write(path, data);
    }
}

/// Adds (or bumps) a project the user just opened.
pub fn register(path: &Path) -> Vec<RecentProject> {
    let mut projects = load();
    projects.retain(|p| p.path != path);
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    projects.insert(0, RecentProject { name, path: path.to_path_buf(), last_opened: now });
    projects.truncate(MAX_COUNT);
    save(&projects);
    projects
}

pub fn remove(path: &Path) -> Vec<RecentProject> {
    let mut projects = load();
    projects.retain(|p| p.path != path);
    save(&projects);
    projects
}

pub fn clear() {
    save(&[]);
}
