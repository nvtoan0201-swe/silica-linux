//! Reports changes anywhere under a project folder (inotify), batched over a short
//! delay so a `git checkout` or a build arrives as one notification.

use std::path::Path;
use std::sync::mpsc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct ProjectWatcher {
    _watcher: RecommendedWatcher,
}

impl ProjectWatcher {
    /// Sends `()` on `changes` for each relevant change; the receiver batches them.
    pub fn new(root: &Path, changes: mpsc::Sender<()>) -> Option<ProjectWatcher> {
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            if event.kind.is_access() {
                return;
            }
            // Git rewrites its own files constantly (index, locks, logs); the working
            // tree changes a checkout makes arrive as their own events.
            let relevant = event.paths.iter().any(|path| {
                !path.components().any(|c| c.as_os_str() == ".git")
            });
            if relevant {
                let _ = changes.send(());
            }
        })
        .ok()?;
        watcher.watch(root, RecursiveMode::Recursive).ok()?;
        Some(ProjectWatcher { _watcher: watcher })
    }
}
