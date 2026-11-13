//! What a project window had open, so reopening the project puts it back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionTab {
    /// Path from the project root (absolute for a file outside it).
    pub path: String,
    /// Cursor byte offset.
    pub cursor: Option<usize>,
    /// 1-based line at the top of the view.
    pub top_line: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active_tab: Option<String>,
    pub sidebar_visible: bool,
    pub saved_at: u64,
    #[serde(default)]
    pub window_size: Option<(f32, f32)>,
}

/// Projects whose sessions are kept; the least recently saved go first.
const LIMIT: usize = 40;

fn store_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("silica").join("sessions.json"))
}

fn all() -> HashMap<String, Session> {
    store_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

pub fn load(root: &Path) -> Option<Session> {
    all().remove(&root.display().to_string())
}

pub fn save(root: &Path, mut session: Session) {
    session.saved_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut sessions = all();
    sessions.insert(root.display().to_string(), session);
    if sessions.len() > LIMIT {
        let mut by_age: Vec<(String, u64)> = sessions.iter().map(|(k, v)| (k.clone(), v.saved_at)).collect();
        by_age.sort_by_key(|(_, t)| *t);
        for (key, _) in by_age.into_iter().take(sessions.len() - LIMIT) {
            sessions.remove(&key);
        }
    }
    let Some(path) = store_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_vec(&sessions) {
        let _ = std::fs::write(path, data);
    }
}
