//! Windows and the app-wide commands: opening projects, the welcome window, quitting.

use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext, Bounds, KeyBinding, PathPromptOptions, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowHandle, WindowKind,
    WindowOptions, actions, point, px, size,
};

use crate::project::recent;
use crate::settings::Settings;
use crate::welcome::Welcome;
use crate::workspace::Workspace;

actions!(silica, [Open, Quit, ShowWelcome, ShowSettings]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-o", Open, None),
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-shift-1", ShowWelcome, None),
        KeyBinding::new("ctrl-,", ShowSettings, None),
    ]);
    cx.on_action(|_: &Open, cx| choose_and_open(cx));
    cx.on_action(|_: &Quit, cx| quit(cx));
    cx.on_action(|_: &ShowWelcome, cx| show_welcome(cx));
    cx.on_action(|_: &ShowSettings, cx| crate::settings_window::show(cx));
}

fn window_options(cx: &App, bounds: WindowBounds, min: Size<gpui::Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(bounds),
        titlebar: Some(TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: None }),
        window_decorations: Some(WindowDecorations::Client),
        window_background: if Settings::get(cx).translucent_window {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        },
        window_min_size: Some(min),
        kind: WindowKind::Normal,
        app_id: Some("silica".into()),
        ..Default::default()
    }
}

/// The open project windows.
pub fn workspaces(cx: &App) -> Vec<WindowHandle<Workspace>> {
    cx.windows().into_iter().filter_map(|w| w.downcast::<Workspace>()).collect()
}

/// Opens a folder as a project, or a file in the project it belongs to.
pub fn open_paths(paths: &[PathBuf], cx: &mut App) {
    for path in paths {
        let Ok(path) = std::fs::canonicalize(path) else { continue };
        if path.is_dir() {
            open_project(&path, None, cx);
        } else {
            // A file opens as a tab in the open project that contains it; otherwise in a
            // window for the git repository it belongs to, or else for its folder.
            let existing = workspaces(cx).into_iter().find(|handle| {
                handle.read(cx).is_ok_and(|w| path.starts_with(&w.root_path))
            });
            if let Some(handle) = existing {
                let _ = handle.update(cx, |workspace, window, cx| {
                    workspace.open(&path, window, cx);
                    workspace.reveal(&path, cx);
                    window.activate_window();
                });
                continue;
            }
            let root = project_root_for(&path);
            open_project(&root, Some(&path), cx);
        }
    }
}

fn project_root_for(file: &Path) -> PathBuf {
    let folder = file.parent().unwrap_or(Path::new("/")).to_path_buf();
    for recent in recent::load() {
        if file.starts_with(&recent.path) {
            return recent.path;
        }
    }
    let mut current = Some(folder.as_path());
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        current = dir.parent();
    }
    folder
}

pub fn open_project(root: &Path, file: Option<&Path>, cx: &mut App) {
    // Already open: bring it forward.
    if let Some(handle) = workspaces(cx).into_iter().find(|h| h.read(cx).is_ok_and(|w| w.root_path == root)) {
        let file = file.map(Path::to_path_buf);
        let _ = handle.update(cx, |workspace, window, cx| {
            if let Some(file) = file {
                workspace.open(&file, window, cx);
                workspace.reveal(&file, cx);
            }
            window.activate_window();
        });
        return;
    }
    recent::register(root);
    let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
    let options = window_options(cx, WindowBounds::Windowed(bounds), size(px(640.), px(400.)));
    let root = root.to_path_buf();
    let file = file.map(Path::to_path_buf);
    let result = cx.open_window(options, move |window, cx| {
        cx.new(|cx| {
            let mut workspace = Workspace::new(root, window, cx);
            if let Some(file) = file {
                workspace.open(&file, window, cx);
                workspace.reveal(&file, cx);
            }
            workspace
        })
    });
    if result.is_ok() {
        close_welcome(cx);
    }
    cx.activate(true);
}

pub fn choose_and_open(cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = receiver.await {
            cx.update(|cx| open_paths(&paths, cx)).ok();
        }
    })
    .detach();
}

pub fn show_welcome(cx: &mut App) {
    if let Some(handle) = cx.windows().into_iter().find_map(|w| w.downcast::<Welcome>()) {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let bounds = Bounds::centered(None, size(px(780.), px(480.)), cx);
    let mut options = window_options(cx, WindowBounds::Windowed(bounds), size(px(780.), px(480.)));
    options.is_resizable = false;
    let _ = cx.open_window(options, |window, cx| cx.new(|cx| Welcome::new(window, cx)));
    cx.activate(true);
}

fn close_welcome(cx: &mut App) {
    for handle in cx.windows().into_iter().filter_map(|w| w.downcast::<Welcome>()) {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// Asks each project window about unsaved files, then quits once they've all closed.
pub fn quit(cx: &mut App) {
    let handles = workspaces(cx);
    if handles.is_empty() {
        cx.quit();
        return;
    }
    for handle in handles {
        let _ = handle.update(cx, |workspace, window, cx| workspace.close_window(window, cx));
    }
}

/// When the last project window closes, the welcome window comes back, as on macOS;
/// when every window is gone, the app quits.
pub fn watch_windows(cx: &mut App) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static LAST_WORKSPACES: AtomicUsize = AtomicUsize::new(0);
    cx.observe_new::<Workspace>(|_, _, _| {
        LAST_WORKSPACES.fetch_add(1, Ordering::SeqCst);
    })
    .detach();
    cx.on_window_closed(|cx| {
        let count = workspaces(cx).len();
        let previous = LAST_WORKSPACES.swap(count, Ordering::SeqCst);
        if count == 0 && previous > 0 {
            show_welcome(cx);
        } else if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    let _ = point(px(0.), px(0.));
}
