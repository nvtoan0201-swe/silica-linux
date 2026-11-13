//! A project window: the explorer tree and the open editor tabs.

mod card;
pub mod quick_open;
mod search;
mod sidebar;
mod tabs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext, Context, DismissEvent, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Point, Pixels, PromptLevel, Render,
    Styled, Subscription, Task, UniformListScrollHandle, WeakEntity, Window, actions, anchored,
    deferred, div, prelude::FluentBuilder, px,
};

use crate::buffer::{Buffer, Content, DiskChange};
use crate::editor::{Editor, EditorEvent};
use crate::project::fuzzy::FileItem;
use crate::project::index::{self, FilterResult};
use crate::project::session::{self, Session, SessionTab};
use crate::project::tree::FileNode;
use crate::project::watcher::ProjectWatcher;
use crate::settings::Settings;
use crate::theme;
use crate::ui::chrome;
use crate::ui::menu::{ContextMenu, MenuItem};
use crate::ui::text_field::{TextField, TextFieldEvent};
use quick_open::{QuickOpen, QuickOpenEvent};
use search::{ProjectSearch, SearchEvent};

actions!(
    workspace,
    [
        ToggleQuickOpen,
        GoToLine,
        FindInProject,
        ShowExplorer,
        FilterFiles,
        RefreshExplorer,
        ToggleSidebar,
        TogglePreview,
        NextTab,
        PreviousTab,
        CloseTab,
        CloseWindow,
        Save,
        SaveAs,
        SaveAll,
        NewFile,
        NewFileInProject,
        NewFolder,
        ToggleWordWrap,
        ToggleMinimap,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        CollapseFolders,
        Dismiss,
        TreeUp,
        TreeDown,
        TreeExpand,
        TreeCollapse,
        TreeOpen,
        TreeRename,
        TreeTrash,
    ]
);

/// Height of the title bar row that holds the sidebar header and the tabs.
pub const TITLE_BAR_HEIGHT: f32 = 36.;
pub const CARD_RADIUS: f32 = 10.;
pub const CARD_INSET: f32 = 6.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    Explorer,
    Search,
}

/// A rename or new file/folder name being typed in the tree.
#[derive(Clone, PartialEq, Eq)]
pub enum TreeEdit {
    Rename(PathBuf),
    Create { folder: PathBuf, is_dir: bool },
}

pub struct Workspace {
    pub root_path: PathBuf,
    pub tree: FileNode,
    pub selection: Option<PathBuf>,
    pub sidebar_visible: bool,
    pub sidebar_mode: SidebarMode,
    pub tree_edit: Option<TreeEdit>,
    name_field: Option<Entity<TextField>>,
    pub filter_field: Entity<TextField>,
    pub filter_result: Option<FilterResult>,
    filter_task: Option<Task<()>>,
    /// The project's files, for Quick Open and the tree filter.
    file_index: Arc<Vec<FileItem>>,
    index_is_fresh: bool,
    index_task: Option<Task<()>>,
    pub tabs: Vec<PathBuf>,
    pub active: Option<PathBuf>,
    pub buffers: HashMap<PathBuf, Buffer>,
    /// Most recently opened files first, for Quick Open.
    recent_files: Vec<PathBuf>,
    quick_open: Option<(Entity<QuickOpen>, Subscription)>,
    pub search: Entity<ProjectSearch>,
    context_menu: Option<(Point<Pixels>, Entity<ContextMenu>, Subscription)>,
    focus_handle: FocusHandle,
    pub tree_focus: FocusHandle,
    pub tree_scroll: UniformListScrollHandle,
    pub tab_scroll: gpui::ScrollHandle,
    _watcher: Option<ProjectWatcher>,
    _watch_task: Option<Task<()>>,
    session_task: Option<Task<()>>,
    editor_subscriptions: HashMap<PathBuf, Subscription>,
    /// Tabs (with scroll and cursor) to restore once the editor has laid out.
    pending_top_lines: HashMap<PathBuf, usize>,
    sidebar_drag: Option<(f32, f32)>,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Workspace {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Workspace {
        let tree = FileNode::root(&root);
        let filter_field = cx.new(|cx| TextField::new("Filter", window, cx));
        cx.subscribe_in(&filter_field, window, |this, _, event, window, cx| match event {
            TextFieldEvent::Changed => this.schedule_tree_filter(Duration::from_millis(120), cx),
            TextFieldEvent::Cancelled => {
                if !this.filter_text(cx).is_empty() {
                    this.filter_field.update(cx, |f, cx| f.set_text("", cx));
                    this.schedule_tree_filter(Duration::ZERO, cx);
                } else {
                    this.focus_active_editor(window, cx);
                }
            }
            TextFieldEvent::Submitted | TextFieldEvent::Blurred => {}
        })
        .detach();
        let search = cx.new(|cx| ProjectSearch::new(root.clone(), window, cx));
        cx.subscribe_in(&search, window, |this, _, event: &SearchEvent, window, cx| match event {
            SearchEvent::Open { path, range } => this.open_search_match(path.clone(), range.clone(), window, cx),
        })
        .detach();

        let mut workspace = Workspace {
            root_path: root.clone(),
            tree,
            selection: None,
            sidebar_visible: true,
            sidebar_mode: SidebarMode::Explorer,
            tree_edit: None,
            name_field: None,
            filter_field,
            filter_result: None,
            filter_task: None,
            file_index: Arc::new(Vec::new()),
            index_is_fresh: false,
            index_task: None,
            tabs: Vec::new(),
            active: None,
            buffers: HashMap::new(),
            recent_files: Vec::new(),
            quick_open: None,
            search,
            context_menu: None,
            focus_handle: cx.focus_handle(),
            tree_focus: cx.focus_handle(),
            tree_scroll: UniformListScrollHandle::new(),
            tab_scroll: gpui::ScrollHandle::new(),
            _watcher: None,
            _watch_task: None,
            session_task: None,
            editor_subscriptions: HashMap::new(),
            pending_top_lines: HashMap::new(),
            sidebar_drag: None,
        };
        workspace.start_watching(cx);
        workspace.restore_session(window, cx);
        workspace.reindex(cx);
        window.on_window_should_close(cx, {
            let this = cx.entity().downgrade();
            move |window, cx| {
                this.update(cx, |this, cx| this.should_close(window, cx)).unwrap_or(true)
            }
        });
        workspace
    }

    pub fn name(&self) -> String {
        self.tree.name.clone()
    }

    pub fn active_buffer(&self) -> Option<&Buffer> {
        self.active.as_ref().and_then(|p| self.buffers.get(p))
    }

    pub fn active_editor(&self) -> Option<Entity<Editor>> {
        self.active_buffer().and_then(|b| b.editor().cloned())
    }

    pub fn has_unsaved_changes(&self, cx: &App) -> bool {
        self.buffers.values().any(|b| b.needs_saving(cx))
    }

    pub fn filter_text(&self, cx: &App) -> String {
        self.filter_field.read(cx).text().trim().to_string()
    }

    pub fn is_filtering(&self, cx: &App) -> bool {
        !self.filter_text(cx).is_empty()
    }

    pub fn relative_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.display().to_string())
    }

    /// Whether `path` is inside the project folder, where the tree can show it.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root_path) && path != self.root_path
    }

    fn focus_active_editor(&self, window: &mut Window, cx: &App) {
        if let Some(editor) = self.active_editor() {
            window.focus(&editor.focus_handle(cx));
        } else {
            window.focus(&self.tree_focus);
        }
    }

    // MARK: Watching and indexing

    fn start_watching(&mut self, cx: &mut Context<Self>) {
        let (sender, receiver) = mpsc::channel::<()>();
        self._watcher = ProjectWatcher::new(&self.root_path, sender);
        let receiver = Arc::new(std::sync::Mutex::new(receiver));
        self._watch_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let receiver = receiver.clone();
                // Wait for a change, then let the burst settle before refreshing.
                let got = cx
                    .background_executor()
                    .spawn(async move {
                        let receiver = receiver.lock().unwrap();
                        if receiver.recv().is_err() {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(300));
                        while receiver.try_recv().is_ok() {}
                        true
                    })
                    .await;
                if !got {
                    break;
                }
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// Re-reads the tree and checks open files against the disk.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.tree.reload();
        let reopen: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|path| self.buffers.get_mut(*path).is_some_and(|b| b.sync_with_disk(cx)))
            .cloned()
            .collect();
        for path in reopen {
            // An image or other non-text file changed: read it again.
            let buffer = Buffer::open(&path, cx);
            self.buffers.insert(path, buffer);
        }
        self.index_is_fresh = false;
        self.reindex(cx);
        self.search.update(cx, |search, cx| search.refresh(cx));
        cx.notify();
    }

    fn reindex(&mut self, cx: &mut Context<Self>) {
        let root = self.root_path.clone();
        self.index_task = Some(cx.spawn(async move |this, cx| {
            let items = cx.background_executor().spawn(async move { index::scan(&root) }).await;
            this.update(cx, |this, cx| {
                this.file_index = Arc::new(items);
                this.index_is_fresh = true;
                if let Some((quick_open, _)) = &this.quick_open {
                    let items = this.file_index.clone();
                    quick_open.update(cx, |q, cx| q.set_items(items, cx));
                }
                if this.is_filtering(cx) {
                    this.schedule_tree_filter(Duration::ZERO, cx);
                }
            })
            .ok();
        }));
    }

    // MARK: Tree filter

    fn schedule_tree_filter(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let query = self.filter_text(cx);
        if query.is_empty() {
            self.filter_task = None;
            self.filter_result = None;
            cx.notify();
            return;
        }
        let items = self.file_index.clone();
        self.filter_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let result = cx.background_executor().spawn(async move { index::filter(&items, &query) }).await;
            this.update(cx, |this, cx| {
                // Reads the folders a filtered tree passes through, without expanding them,
                // so clearing the filter brings back the tree as it was.
                let mut paths: Vec<&String> = result.visible.iter().collect();
                paths.sort_by_key(|p| p.len());
                for path in paths {
                    this.tree.load_along(path);
                }
                this.filter_result = Some(result);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn focus_tree_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_explorer(cx);
        self.filter_field.read(cx).focus(window);
        cx.notify();
    }

    // MARK: Sidebar

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        self.schedule_session_save(cx);
        cx.notify();
    }

    pub fn show_explorer(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        self.sidebar_mode = SidebarMode::Explorer;
        cx.notify();
    }

    pub fn show_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        self.sidebar_mode = SidebarMode::Search;
        // A selection in the editor is what to search for, as in other editors.
        let selected = self
            .active_editor()
            .map(|e| e.read(cx).selected_text())
            .filter(|s| !s.is_empty() && !s.contains('\n'));
        self.search.update(cx, |search, cx| search.focus(selected, window, cx));
        cx.notify();
    }

    pub fn toggle_expansion(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(node) = self.tree.find_mut(path) {
            let expanded = !node.expanded;
            node.set_expanded(expanded);
        }
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.tree.collapse_all();
        cx.notify();
    }

    /// Expands the folders above `path`, selects it and scrolls the tree to it.
    pub fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.tree.reveal(path) {
            return;
        }
        self.selection = Some(path.to_path_buf());
        let rows = self.visible_rows(cx);
        if let Some(index) = rows.iter().position(|r| r.path == path) {
            self.tree_scroll.scroll_to_item(index, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Reveal in Explorer, from a tab's menu: after bringing the tree into view and
    /// clearing a filter that hides the file.
    pub fn reveal_in_tree(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.activate(path, cx);
        self.show_explorer(cx);
        let relative = self.relative_path(path);
        if self.filter_result.as_ref().is_some_and(|r| !r.visible.contains(&relative)) {
            self.filter_field.update(cx, |f, cx| f.set_text("", cx));
            self.filter_result = None;
        }
        self.reveal(path, cx);
    }

    /// A click (or Enter) on a tree row: folders open and close, files open.
    pub fn select_tree_row(&mut self, path: &Path, is_dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.selection = Some(path.to_path_buf());
        if is_dir {
            self.toggle_expansion(path, cx);
        } else {
            self.open(path, window, cx);
        }
        cx.notify();
    }

    // MARK: Tabs

    pub fn open(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_path_buf();
        if !self.buffers.contains_key(&path) {
            let buffer = Buffer::open(&path, cx);
            self.add_buffer(path.clone(), buffer, cx);
        }
        if !self.tabs.contains(&path) {
            let index = self.active.as_ref().and_then(|a| self.tabs.iter().position(|t| t == a));
            match index {
                Some(index) => self.tabs.insert(index + 1, path.clone()),
                None => self.tabs.push(path.clone()),
            }
        }
        self.activate(&path, cx);
        self.focus_active_editor(window, cx);
    }

    fn add_buffer(&mut self, path: PathBuf, buffer: Buffer, cx: &mut Context<Self>) {
        if let Some(editor) = buffer.editor() {
            let subscription = cx.subscribe(editor, |this, _, event: &EditorEvent, cx| match event {
                EditorEvent::Edited => {
                    this.search.update(cx, |s, cx| s.note_edit(cx));
                    cx.notify();
                }
                EditorEvent::SelectionChanged => cx.notify(),
            });
            self.editor_subscriptions.insert(path.clone(), subscription);
        }
        self.buffers.insert(path, buffer);
    }

    pub fn activate(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.active = Some(path.to_path_buf());
        if let Some(buffer) = self.buffers.get(path) {
            // An untitled file has no row in the tree or place in Quick Open.
            if buffer.untitled {
                self.selection = None;
            } else {
                self.recent_files.retain(|p| p != path);
                self.recent_files.insert(0, path.to_path_buf());
                self.recent_files.truncate(30);
                self.selection = Some(path.to_path_buf());
            }
        }
        if let Some(index) = self.tabs.iter().position(|t| t == path) {
            self.tab_scroll.scroll_to_item(index);
        }
        self.schedule_session_save(cx);
        cx.notify();
    }

    /// Select Next Tab / Select Previous Tab, going round from either end to the other.
    pub fn activate_tab_offset(&mut self, offset: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len() as isize;
        let Some(active) = &self.active else { return };
        let Some(index) = self.tabs.iter().position(|t| t == active) else { return };
        if count < 2 {
            return;
        }
        let next = self.tabs[((index as isize + offset) % count + count) as usize % count as usize].clone();
        self.activate(&next, cx);
        self.focus_active_editor(window, cx);
    }

    /// New File (Ctrl+N): an empty "Untitled-N" tab, saved somewhere only when asked.
    pub fn new_untitled_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let taken: Vec<String> = self.buffers.values().filter(|b| b.untitled).map(|b| b.name()).collect();
        let mut number = 1;
        while taken.contains(&format!("Untitled-{number}")) {
            number += 1;
        }
        let buffer = Buffer::untitled(&format!("Untitled-{number}"), cx);
        let path = buffer.path.clone();
        self.add_buffer(path.clone(), buffer, cx);
        let index = self.active.as_ref().and_then(|a| self.tabs.iter().position(|t| t == a));
        match index {
            Some(index) => self.tabs.insert(index + 1, path.clone()),
            None => self.tabs.push(path.clone()),
        }
        self.activate(&path, cx);
        self.focus_active_editor(window, cx);
    }

    /// Puts a tab at another place in the tab bar (dragging it there).
    pub fn move_tab(&mut self, path: &Path, to: usize, cx: &mut Context<Self>) {
        let Some(from) = self.tabs.iter().position(|t| t == path) else { return };
        let tab = self.tabs.remove(from);
        let to = to.min(self.tabs.len());
        self.tabs.insert(to, tab);
        self.schedule_session_save(cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(buffer) = self.buffers.get(path) else { return };
        if buffer.needs_saving(cx) {
            let name = buffer.name();
            let path = path.to_path_buf();
            let answer = window.prompt(
                PromptLevel::Warning,
                &format!("Do you want to save the changes you made to “{name}”?"),
                Some("Your changes will be lost if you don’t save them."),
                &["Save", "Don’t Save", "Cancel"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let Ok(answer) = answer.await else { return };
                this.update_in(cx, |this, window, cx| match answer {
                    0 => {
                        this.save_buffer(&path, window, cx, Some(Box::new({
                            let path = path.clone();
                            move |this: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>| {
                                this.remove_tab(&path, window, cx)
                            }
                        })));
                    }
                    1 => this.remove_tab(&path, window, cx),
                    _ => {}
                })
                .ok();
            })
            .detach();
            return;
        }
        self.remove_tab(path, window, cx);
    }

    fn remove_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t == path) else { return };
        self.tabs.remove(index);
        self.buffers.remove(path);
        self.editor_subscriptions.remove(path);
        if self.active.as_deref() == Some(path) {
            if self.tabs.is_empty() {
                self.active = None;
                self.selection = None;
                window.focus(&self.tree_focus);
            } else {
                let next = self.tabs[index.min(self.tabs.len() - 1)].clone();
                self.activate(&next, cx);
                self.focus_active_editor(window, cx);
            }
        }
        self.schedule_session_save(cx);
        cx.notify();
    }

    pub fn close_other_tabs(&mut self, keep: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let others: Vec<PathBuf> = self.tabs.iter().filter(|t| *t != keep).cloned().collect();
        for other in others {
            self.close_tab(&other, window, cx);
        }
    }

    // MARK: Quick Open

    pub fn toggle_quick_open(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_open.is_some() {
            self.hide_quick_open(window, cx);
            return;
        }
        let line_count = self.active_editor().map(|e| e.read(cx).line_count());
        let items = self.file_index.clone();
        let indexing = !self.index_is_fresh;
        let recent = self.recent_files.clone();
        let active = self.active.clone();
        let root = self.root_path.clone();
        let query = query.to_string();
        let quick_open =
            cx.new(|cx| QuickOpen::new(root, items, indexing, recent, active, line_count, &query, window, cx));
        let subscription = cx.subscribe_in(&quick_open, window, |this, _, event, window, cx| match event {
            QuickOpenEvent::Open { path, line } => {
                let (path, line) = (path.clone(), *line);
                this.hide_quick_open(window, cx);
                this.open(&path, window, cx);
                this.reveal(&path, cx);
                if let Some(line) = line {
                    this.go_to_line(line, window, cx);
                }
            }
            QuickOpenEvent::GoToLine(line) => {
                let line = *line;
                this.hide_quick_open(window, cx);
                this.go_to_line(line, window, cx);
            }
            QuickOpenEvent::Dismiss => this.hide_quick_open(window, cx),
        });
        self.quick_open = Some((quick_open, subscription));
        // The index may have changed since it was built.
        self.reindex(cx);
        cx.notify();
    }

    fn hide_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_open.take().is_some() {
            self.focus_active_editor(window, cx);
            cx.notify();
        }
    }

    /// Go to Line (Ctrl+L): Quick Open with ":" typed, as in VS Code.
    pub fn show_go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_editor().is_none() {
            return;
        }
        self.hide_quick_open(window, cx);
        self.toggle_quick_open(":", window, cx);
    }

    fn go_to_line(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active.clone() else { return };
        if let Some(buffer) = self.buffers.get_mut(&path) {
            buffer.showing_preview = false;
        }
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| editor.go_to_line(line, cx));
            window.focus(&editor.focus_handle(cx));
        }
    }

    fn open_search_match(&mut self, path: PathBuf, range: std::ops::Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.open(&path, window, cx);
        if let Some(buffer) = self.buffers.get_mut(&path) {
            buffer.showing_preview = false;
        }
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| editor.reveal(range, cx));
            window.focus(&editor.focus_handle(cx));
        }
    }

    // MARK: Context menus

    pub fn show_context_menu(
        &mut self,
        position: Point<Pixels>,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| ContextMenu::new(items, window, cx));
        let subscription = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.context_menu = None;
            cx.notify();
        });
        self.context_menu = Some((position, menu, subscription));
        cx.notify();
    }

    /// The menu of a file or folder in the tree.
    pub fn file_menu(&self, path: &Path, is_dir: bool, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let this = cx.entity().downgrade();
        let with = move |f: fn(&mut Workspace, &Path, &mut Window, &mut Context<Workspace>), path: PathBuf| {
            let this = this.clone();
            move |window: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| f(this, &path, window, cx)).ok();
            }
        };
        let folder = if is_dir { path.to_path_buf() } else { path.parent().unwrap_or(&self.root_path).to_path_buf() };
        let relative = self.relative_path(path);
        let absolute = path.display().to_string();
        let reveal_path = path.to_path_buf();
        let mut items = Vec::new();
        if !is_dir {
            items.push(MenuItem::new("Open", with(|this, path, window, cx| this.open(path, window, cx), path.to_path_buf())));
            items.push(MenuItem::Separator);
        }
        items.push(MenuItem::new(
            "New File…",
            with(|this, folder, window, cx| this.begin_new_item(false, Some(folder), window, cx), folder.clone()),
        ));
        items.push(MenuItem::new(
            "New Folder…",
            with(|this, folder, window, cx| this.begin_new_item(true, Some(folder), window, cx), folder),
        ));
        items.push(MenuItem::Separator);
        items.push(MenuItem::new("Rename…", with(|this, path, window, cx| this.begin_rename(path, window, cx), path.to_path_buf())));
        items.push(MenuItem::new("Duplicate", with(|this, path, window, cx| this.duplicate(path, window, cx), path.to_path_buf())));
        items.push(MenuItem::new("Move to Trash", with(|this, path, window, cx| this.move_to_trash(path, window, cx), path.to_path_buf())));
        items.push(MenuItem::Separator);
        items.push(MenuItem::new("Show in Files", move |_, cx| cx.reveal_path(&reveal_path)));
        items.push(MenuItem::new("Copy Path", move |_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(absolute.clone()))
        }));
        items.push(MenuItem::new("Copy Relative Path", move |_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(relative.clone()))
        }));
        items
    }

    // MARK: File operations

    /// Starts naming a new file or folder in the tree: inside the selected folder,
    /// next to the selected file, or at the top of the project.
    pub fn begin_new_item(&mut self, is_dir: bool, folder: Option<&Path>, window: &mut Window, cx: &mut Context<Self>) {
        let target = folder.map(Path::to_path_buf).unwrap_or_else(|| match &self.selection {
            Some(selection) if selection.is_dir() => selection.clone(),
            Some(selection) => selection.parent().unwrap_or(&self.root_path).to_path_buf(),
            None => self.root_path.clone(),
        });
        if let Some(node) = self.tree.find_mut(&target) {
            node.set_expanded(true);
        }
        self.show_explorer(cx);
        self.start_tree_edit(TreeEdit::Create { folder: target, is_dir }, "", window, cx);
    }

    pub fn begin_rename(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.show_explorer(cx);
        let name = crate::buffer::file_name(path);
        self.start_tree_edit(TreeEdit::Rename(path.to_path_buf()), &name, window, cx);
        // Renaming a file selects the name without its extension.
        if !path.is_dir() {
            if let Some(dot) = name.rfind('.').filter(|d| *d > 0) {
                if let Some(field) = &self.name_field {
                    field.update(cx, |f, cx| f.select(0..dot, cx));
                }
            }
        }
    }

    fn start_tree_edit(&mut self, edit: TreeEdit, initial: &str, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut field = TextField::new("Name", window, cx);
            field.set_text(initial, cx);
            field.select_all(cx);
            field
        });
        field.read(cx).focus(window);
        cx.subscribe_in(&field, window, |this, field, event, window, cx| match event {
            TextFieldEvent::Submitted => {
                let name = field.read(cx).text().to_string();
                this.finish_tree_edit(Some(name), window, cx);
            }
            TextFieldEvent::Cancelled => this.finish_tree_edit(None, window, cx),
            // Clicking away commits, as in Finder.
            TextFieldEvent::Blurred => {
                let name = field.read(cx).text().to_string();
                this.finish_tree_edit((!name.is_empty()).then_some(name), window, cx);
            }
            TextFieldEvent::Changed => {}
        })
        .detach();
        self.name_field = Some(field);
        self.tree_edit = Some(edit);
        cx.notify();
    }

    fn finish_tree_edit(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.tree_edit.take() else { return };
        self.name_field = None;
        cx.notify();
        let Some(name) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) else {
            window.focus(&self.tree_focus);
            return;
        };
        match edit {
            TreeEdit::Create { folder, is_dir } => self.create_item(&name, &folder, is_dir, window, cx),
            TreeEdit::Rename(path) => self.rename(&path, &name, window, cx),
        }
    }

    pub fn name_field(&self) -> Option<&Entity<TextField>> {
        self.name_field.as_ref()
    }

    /// Creates the file or folder named in the tree. A name with slashes creates the
    /// folders in between, as in VS Code.
    fn create_item(&mut self, name: &str, folder: &Path, is_dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = folder.join(name);
        if path.exists() {
            self.alert(&format!("“{}” already exists.", crate::buffer::file_name(&path)), "Choose a different name.", window, cx);
            return;
        }
        let result = if is_dir {
            std::fs::create_dir_all(&path)
        } else {
            path.parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map(|_| ()))
        };
        if let Err(error) = result {
            self.alert("The item couldn’t be created.", &error.to_string(), window, cx);
            return;
        }
        self.refresh(cx);
        self.reveal(&path, cx);
        if !is_dir {
            self.open(&path, window, cx);
        } else {
            window.focus(&self.tree_focus);
        }
    }

    /// Renames in place; open tabs for the item (or anything inside a renamed folder)
    /// follow it, keeping their edits.
    fn rename(&mut self, path: &Path, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if name == crate::buffer::file_name(path) {
            window.focus(&self.tree_focus);
            return;
        }
        if name.contains('/') {
            self.alert(&format!("“{name}” can’t be used as a name."), "Names can’t contain “/”.", window, cx);
            return;
        }
        let destination = path.with_file_name(name);
        let case_change = name.to_lowercase() == crate::buffer::file_name(path).to_lowercase();
        if destination.exists() && !case_change {
            self.alert(&format!("“{name}” already exists."), "Choose a different name.", window, cx);
            return;
        }
        if let Err(error) = std::fs::rename(path, &destination) {
            self.alert("The item couldn’t be renamed.", &error.to_string(), window, cx);
            return;
        }
        self.retarget_buffers(path, &destination, cx);
        self.refresh(cx);
        self.reveal(&destination, cx);
        window.focus(&self.tree_focus);
    }

    /// Copies the item next to itself as "name copy", like Finder.
    pub fn duplicate(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let is_dir = path.is_dir();
        let name = crate::buffer::file_name(path);
        let (base, ext) = match name.rfind('.').filter(|d| *d > 0 && !is_dir) {
            Some(dot) => (name[..dot].to_string(), name[dot..].to_string()),
            None => (name.clone(), String::new()),
        };
        let mut number = 1;
        let copy = loop {
            let suffix = if number == 1 { " copy".to_string() } else { format!(" copy {number}") };
            let candidate = path.with_file_name(format!("{base}{suffix}{ext}"));
            if !candidate.exists() {
                break candidate;
            }
            number += 1;
        };
        let result = if is_dir { copy_dir(path, &copy) } else { std::fs::copy(path, &copy).map(|_| ()) };
        if let Err(error) = result {
            self.alert("The item couldn’t be duplicated.", &error.to_string(), window, cx);
            return;
        }
        self.refresh(cx);
        self.reveal(&copy, cx);
    }

    /// Moves the item to the Trash. Unedited tabs for it close; edited ones stay open,
    /// marked as deleted, so nothing typed is lost.
    pub fn move_to_trash(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = trash::delete(path) {
            self.alert("The item couldn’t be moved to the Trash.", &error.to_string(), window, cx);
            return;
        }
        let closing: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.starts_with(path) && self.buffers.get(*t).is_some_and(|b| !b.is_dirty(cx)))
            .cloned()
            .collect();
        for tab in closing {
            self.remove_tab(&tab, window, cx);
        }
        if self.selection.as_ref().is_some_and(|s| s.starts_with(path)) {
            self.selection = None;
        }
        self.refresh(cx);
    }

    /// Points tabs at their new location after a rename.
    fn retarget_buffers(&mut self, old: &Path, new: &Path, cx: &mut Context<Self>) {
        let moved = |path: &Path| -> Option<PathBuf> { path.strip_prefix(old).ok().map(|rest| new.join(rest)) };
        let paths: Vec<PathBuf> = self.buffers.keys().cloned().collect();
        for path in paths {
            let Some(target) = moved(&path) else { continue };
            let mut buffer = self.buffers.remove(&path).unwrap();
            buffer.move_to(&target, cx);
            if let Some(subscription) = self.editor_subscriptions.remove(&path) {
                self.editor_subscriptions.insert(target.clone(), subscription);
            }
            self.buffers.insert(target, buffer);
        }
        for tab in &mut self.tabs {
            if let Some(target) = moved(tab) {
                *tab = target;
            }
        }
        if let Some(target) = self.active.as_deref().and_then(moved) {
            self.active = Some(target);
        }
        if let Some(target) = self.selection.as_deref().and_then(moved) {
            self.selection = Some(target);
        }
        for recent in &mut self.recent_files {
            if let Some(target) = moved(recent) {
                *recent = target;
            }
        }
    }

    fn alert(&self, message: &str, detail: &str, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window.prompt(PromptLevel::Warning, message, Some(detail), &["OK"], cx);
    }

    // MARK: Saving

    pub fn save_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.active.clone() {
            self.save_buffer(&path, window, cx, None);
        }
    }

    /// Saves every edited file. Untitled ones are left for their own Save.
    pub fn save_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty: Vec<PathBuf> = self
            .buffers
            .iter()
            .filter(|(_, b)| b.is_dirty(cx) && !b.untitled)
            .map(|(p, _)| p.clone())
            .collect();
        for path in dirty {
            self.save_buffer(&path, window, cx, None);
        }
    }

    /// Saves a buffer, asking where for an untitled one and what to do if another app
    /// changed the file. `then` runs once it's saved.
    #[allow(clippy::type_complexity)]
    fn save_buffer(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>>,
    ) {
        let Some(buffer) = self.buffers.get_mut(path) else { return };
        if buffer.editor().is_none() {
            return;
        }
        if buffer.untitled {
            self.save_as(path, window, cx, then);
            return;
        }
        if let DiskChange::Modified(_) = buffer.disk_change() {
            if buffer.is_dirty(cx) {
                let name = buffer.name();
                let path = path.to_path_buf();
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &format!("“{name}” was changed by another app."),
                    Some("Saving replaces those changes with yours. Revert discards your unsaved edits and shows the file as it is on disk (you can undo it)."),
                    &["Save Anyway", "Revert", "Cancel"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let Ok(answer) = answer.await else { return };
                    this.update_in(cx, |this, window, cx| match answer {
                        0 => this.write_buffer(&path, window, cx, then),
                        1 => {
                            if let Some(buffer) = this.buffers.get_mut(&path) {
                                buffer.revert_to_disk(cx);
                            }
                            cx.notify();
                        }
                        _ => {}
                    })
                    .ok();
                })
                .detach();
                return;
            }
        }
        self.write_buffer(path, window, cx, then);
    }

    #[allow(clippy::type_complexity)]
    fn write_buffer(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>>,
    ) {
        let Some(buffer) = self.buffers.get_mut(path) else { return };
        match buffer.save(cx) {
            Ok(()) => {
                cx.notify();
                if let Some(then) = then {
                    then(self, window, cx);
                }
            }
            Err(error) => self.alert("The file couldn’t be saved.", &error.to_string(), window, cx),
        }
    }

    pub fn save_active_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.active.clone() {
            self.save_as(&path, window, cx, None);
        }
    }

    /// Save As: writes the text to a new file, which takes the tab's place. The
    /// original file keeps what was last saved.
    #[allow(clippy::type_complexity)]
    fn save_as(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>>,
    ) {
        let Some(buffer) = self.buffers.get(path) else { return };
        if buffer.editor().is_none() {
            return;
        }
        let directory = if buffer.untitled {
            self.root_path.clone()
        } else {
            path.parent().unwrap_or(&self.root_path).to_path_buf()
        };
        let name = buffer.name();
        let receiver = cx.prompt_for_new_path(&directory, Some(&name));
        let source = path.to_path_buf();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(destination))) = receiver.await else { return };
            this.update_in(cx, |this, window, cx| {
                this.finish_save_as(&source, &destination, window, cx);
                if let Some(then) = then {
                    then(this, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_save_as(&mut self, source: &Path, destination: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if source == destination {
            self.write_buffer(source, window, cx, None);
            return;
        }
        let Some(buffer) = self.buffers.get(source) else { return };
        if let Err(error) = buffer.write_to(destination, cx) {
            self.alert("The file couldn’t be saved.", &error.to_string(), window, cx);
            return;
        }
        // A tab already open on the destination now shows stale text; drop it.
        if self.buffers.contains_key(destination) {
            self.tabs.retain(|t| t != destination);
            self.buffers.remove(destination);
        }
        self.buffers.remove(source);
        self.editor_subscriptions.remove(source);
        let fresh = Buffer::open(destination, cx);
        self.add_buffer(destination.to_path_buf(), fresh, cx);
        match self.tabs.iter().position(|t| t == source) {
            Some(index) => self.tabs[index] = destination.to_path_buf(),
            None => self.tabs.push(destination.to_path_buf()),
        }
        self.activate(destination, cx);
        self.refresh(cx);
        self.reveal(destination, cx);
        self.focus_active_editor(window, cx);
    }

    // MARK: Closing

    /// Asks about unsaved files before the window closes. Returns whether it may close now.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.save_session(cx);
        let unsaved: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| self.buffers.get(*t).is_some_and(|b| b.needs_saving(cx)))
            .cloned()
            .collect();
        if unsaved.is_empty() {
            return true;
        }
        let names: Vec<String> = unsaved.iter().filter_map(|p| self.buffers.get(p).map(Buffer::name)).collect();
        let (message, detail, save_label) = if unsaved.len() == 1 {
            (
                format!("Do you want to save the changes you made to “{}”?", names[0]),
                "Your changes will be lost if you don’t save them.".to_string(),
                "Save",
            )
        } else {
            let listed = names.iter().take(5).map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ");
            let more = if names.len() > 5 { format!(" and {} more", names.len() - 5) } else { String::new() };
            (
                format!("Do you want to save the changes you made to {} files in “{}”?", unsaved.len(), self.name()),
                format!("{listed}{more}\n\nYour changes will be lost if you don’t save them."),
                "Save All",
            )
        };
        let answer = window.prompt(PromptLevel::Warning, &message, Some(&detail), &[save_label, "Don’t Save", "Cancel"], cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let close = this
                .update_in(cx, |this, window, cx| match answer {
                    0 => {
                        for path in &unsaved {
                            if this.buffers.get(path).is_some_and(|b| b.untitled) {
                                continue;
                            }
                            this.write_buffer(path, window, cx, None);
                        }
                        // Untitled files still need a name: keep the window for them.
                        !this.has_unsaved_changes(cx)
                    }
                    1 => {
                        this.buffers.clear();
                        true
                    }
                    _ => false,
                })
                .unwrap_or(false);
            if close {
                cx.update(|window, _| window.remove_window()).ok();
            }
        })
        .detach();
        false
    }

    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.should_close(window, cx) {
            window.remove_window();
        }
    }

    // MARK: Session

    /// Reopens the tabs the project had last time, with their cursor and scroll positions.
    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !Settings::get(cx).restores_tabs {
            return;
        }
        let Some(session) = session::load(&self.root_path) else { return };
        self.sidebar_visible = session.sidebar_visible;
        for tab in &session.tabs {
            let path = self.session_path(&tab.path);
            if self.tabs.contains(&path) || !path.is_file() {
                continue;
            }
            let buffer = Buffer::open(&path, cx);
            if let (Some(editor), Some(cursor)) = (buffer.editor(), tab.cursor) {
                editor.update(cx, |editor, cx| {
                    let cursor = cursor.min(editor.len());
                    editor.select_range(cursor..cursor, cx);
                });
            }
            if let (Some(editor), Some(top)) = (buffer.editor(), tab.top_line) {
                editor.update(cx, |editor, cx| editor.scroll_to_line(top, false, cx));
            }
            self.add_buffer(path.clone(), buffer, cx);
            self.tabs.push(path);
        }
        let active = session
            .active_tab
            .map(|p| self.session_path(&p))
            .filter(|p| self.tabs.contains(p))
            .or_else(|| self.tabs.first().cloned());
        if let Some(active) = active {
            self.activate(&active, cx);
            self.reveal(&active, cx);
            self.focus_active_editor(window, cx);
        }
    }

    fn session_path(&self, path: &str) -> PathBuf {
        if path.starts_with('/') { PathBuf::from(path) } else { self.root_path.join(path) }
    }

    pub fn save_session(&mut self, cx: &mut Context<Self>) {
        self.session_task = None;
        let tabs = self
            .tabs
            .iter()
            .filter_map(|path| {
                let buffer = self.buffers.get(path)?;
                if buffer.untitled {
                    return None;
                }
                let (cursor, top_line) = match buffer.editor() {
                    Some(editor) => editor.update(cx, |e, _| (Some(e.cursor()), Some(e.top_visible_line()))),
                    None => (None, None),
                };
                Some(SessionTab { path: self.relative_path(path), cursor, top_line })
            })
            .collect();
        let active_tab = self
            .active
            .as_ref()
            .filter(|p| self.buffers.get(*p).is_some_and(|b| !b.untitled))
            .map(|p| self.relative_path(p));
        session::save(
            &self.root_path,
            Session { tabs, active_tab, sidebar_visible: self.sidebar_visible, saved_at: 0, window_size: None },
        );
    }

    fn schedule_session_save(&mut self, cx: &mut Context<Self>) {
        self.session_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            this.update(cx, |this, cx| this.save_session(cx)).ok();
        }));
    }

    // MARK: Preview

    pub fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active.clone() else { return };
        if let Some(buffer) = self.buffers.get_mut(&path) {
            if buffer.preview_format.is_some() && buffer.editor().is_some() {
                buffer.showing_preview = !buffer.showing_preview;
                cx.notify();
            }
        }
    }

    pub fn set_preview(&mut self, show: bool, cx: &mut Context<Self>) {
        let Some(path) = self.active.clone() else { return };
        if let Some(buffer) = self.buffers.get_mut(&path) {
            buffer.showing_preview = show && buffer.preview_format.is_some();
            cx.notify();
        }
    }

    // MARK: Actions

    fn register_actions<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        element
            .on_action(cx.listener(|this, _: &ToggleQuickOpen, window, cx| this.toggle_quick_open("", window, cx)))
            .on_action(cx.listener(|this, _: &GoToLine, window, cx| this.show_go_to_line(window, cx)))
            .on_action(cx.listener(|this, _: &FindInProject, window, cx| this.show_search(window, cx)))
            .on_action(cx.listener(|this, _: &ShowExplorer, _, cx| this.show_explorer(cx)))
            .on_action(cx.listener(|this, _: &FilterFiles, window, cx| this.focus_tree_filter(window, cx)))
            .on_action(cx.listener(|this, _: &RefreshExplorer, _, cx| this.refresh(cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &TogglePreview, _, cx| this.toggle_preview(cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.activate_tab_offset(1, window, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| this.activate_tab_offset(-1, window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                // As in VS Code: Ctrl+W closes the tab, and the window once no tabs are left.
                match this.active.clone() {
                    Some(path) => this.close_tab(&path, window, cx),
                    None => this.close_window(window, cx),
                }
            }))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| this.close_window(window, cx)))
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save_active(window, cx)))
            .on_action(cx.listener(|this, _: &SaveAs, window, cx| this.save_active_as(window, cx)))
            .on_action(cx.listener(|this, _: &SaveAll, window, cx| this.save_all(window, cx)))
            .on_action(cx.listener(|this, _: &NewFile, window, cx| this.new_untitled_file(window, cx)))
            .on_action(cx.listener(|this, _: &NewFileInProject, window, cx| this.begin_new_item(false, None, window, cx)))
            .on_action(cx.listener(|this, _: &NewFolder, window, cx| this.begin_new_item(true, None, window, cx)))
            .on_action(cx.listener(|this, _: &CollapseFolders, _, cx| this.collapse_all(cx)))
            .on_action(cx.listener(|_, _: &ToggleWordWrap, _, cx| Settings::update(cx, |s| s.wraps_lines = !s.wraps_lines)))
            .on_action(cx.listener(|_, _: &ToggleMinimap, _, cx| Settings::update(cx, |s| s.shows_minimap = !s.shows_minimap)))
            .on_action(cx.listener(|_, _: &IncreaseFontSize, _, cx| Settings::update(cx, |s| s.font_size += 1.)))
            .on_action(cx.listener(|_, _: &DecreaseFontSize, _, cx| Settings::update(cx, |s| s.font_size -= 1.)))
            .on_action(cx.listener(|_, _: &ResetFontSize, _, cx| {
                Settings::update(cx, |s| s.font_size = Settings::DEFAULT_FONT_SIZE)
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                if this.quick_open.is_some() {
                    this.hide_quick_open(window, cx);
                } else if this.context_menu.is_some() {
                    this.context_menu = None;
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
    }
}

/// Copies a folder and everything in it.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::bench::time("workspace render", || self.render_workspace(window, cx))
    }
}

impl Workspace {
    fn render_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = theme::colors(cx);
        let settings = Settings::get(cx).clone();
        window.set_window_title(&self.name());
        if std::env::var_os("SILICA_BENCH").is_some() {
            bench_frame(self, window, cx);
        }
        let sidebar_width = settings.sidebar_width.clamp(Settings::SIDEBAR_WIDTH_RANGE.0, Settings::SIDEBAR_WIDTH_RANGE.1);

        let content = div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_row()
            .relative()
            .text_color(colors.label)
            .font_family(crate::desktop::fonts().ui.clone())
            .text_size(px(13.))
            .when(self.sidebar_visible, |d| d.child(self.render_sidebar(sidebar_width, window, cx)))
            .child(self.render_editor_area(window, cx))
            .when_some(self.quick_open.as_ref().map(|(q, _)| q.clone()), |d, quick_open| {
                d.child(
                    div()
                        .id("quick-open-backdrop")
                        .absolute()
                        .size_full()
                        .flex()
                        .justify_center()
                        .pt(px(TITLE_BAR_HEIGHT + 12.))
                        .px(px(24.))
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, window, cx| {
                            this.hide_quick_open(window, cx)
                        }))
                        .child(
                            div()
                                .w_full()
                                .max_w(px(640.))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(quick_open),
                        ),
                )
            })
            .when_some(self.context_menu.as_ref().map(|(p, m, _)| (*p, m.clone())), |d, (position, menu)| {
                d.child(deferred(anchored().position(position).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(1))
            })
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                if let Some((start_x, start_width)) = this.sidebar_drag {
                    if event.pressed_button == Some(gpui::MouseButton::Left) {
                        let width = start_width + f32::from(event.position.x) - start_x;
                        Settings::update(cx, |s| s.sidebar_width = width);
                    } else {
                        this.sidebar_drag = None;
                    }
                }
            }))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, _, _, _| this.sidebar_drag = None));
        let content = self.register_actions(content, cx);
        chrome::frame(window, cx, content.into_any_element()).into_any_element()
    }
}

pub fn bind_keys(cx: &mut App) {
    let w = Some("Workspace");
    cx.bind_keys([
        KeyBinding::new("ctrl-p", ToggleQuickOpen, w),
        KeyBinding::new("ctrl-l", GoToLine, w),
        KeyBinding::new("ctrl-g", GoToLine, w),
        KeyBinding::new("ctrl-shift-f", FindInProject, w),
        KeyBinding::new("ctrl-shift-e", ShowExplorer, w),
        KeyBinding::new("ctrl-alt-j", FilterFiles, w),
        KeyBinding::new("ctrl-shift-r", RefreshExplorer, w),
        KeyBinding::new("ctrl-b", ToggleSidebar, w),
        KeyBinding::new("ctrl-shift-v", TogglePreview, w),
        KeyBinding::new("ctrl-tab", NextTab, w),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, w),
        KeyBinding::new("ctrl-pagedown", NextTab, w),
        KeyBinding::new("ctrl-pageup", PreviousTab, w),
        KeyBinding::new("ctrl-shift-]", NextTab, w),
        KeyBinding::new("ctrl-shift-[", PreviousTab, w),
        KeyBinding::new("ctrl-w", CloseTab, w),
        KeyBinding::new("ctrl-shift-w", CloseWindow, w),
        KeyBinding::new("ctrl-s", Save, w),
        KeyBinding::new("ctrl-shift-s", SaveAs, w),
        KeyBinding::new("ctrl-alt-s", SaveAll, w),
        KeyBinding::new("ctrl-n", NewFile, w),
        KeyBinding::new("ctrl-alt-n", NewFolder, w),
        KeyBinding::new("alt-z", ToggleWordWrap, w),
        KeyBinding::new("ctrl-=", IncreaseFontSize, w),
        KeyBinding::new("ctrl-+", IncreaseFontSize, w),
        KeyBinding::new("ctrl--", DecreaseFontSize, w),
        KeyBinding::new("ctrl-0", ResetFontSize, w),
        KeyBinding::new("escape", Dismiss, w),
        KeyBinding::new("up", TreeUp, Some("FileTree")),
        KeyBinding::new("down", TreeDown, Some("FileTree")),
        KeyBinding::new("right", TreeExpand, Some("FileTree")),
        KeyBinding::new("left", TreeCollapse, Some("FileTree")),
        KeyBinding::new("space", TreeOpen, Some("FileTree")),
        KeyBinding::new("enter", TreeRename, Some("FileTree")),
        KeyBinding::new("f2", TreeRename, Some("FileTree")),
        KeyBinding::new("ctrl-backspace", TreeTrash, Some("FileTree")),
        KeyBinding::new("delete", TreeTrash, Some("FileTree")),
    ]);
}

pub type WeakWorkspace = WeakEntity<Workspace>;
pub type Element = AnyElement;

/// With SILICA_BENCH set: scrolls the active editor every frame and prints the frame rate.
fn bench_frame(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    use std::cell::RefCell;
    use std::time::Instant;
    thread_local! {
        static STATS: RefCell<(Option<Instant>, Instant, Vec<f32>)> = RefCell::new((None, Instant::now(), Vec::new()));
    }
    STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        let now = Instant::now();
        if let Some(last) = stats.0 {
            let ms = now.duration_since(last).as_secs_f32() * 1000.;
            stats.2.push(ms);
        }
        stats.0 = Some(now);
        if now.duration_since(stats.1).as_secs_f32() >= 2. && !stats.2.is_empty() {
            let frames = &stats.2;
            let avg = frames.iter().sum::<f32>() / frames.len() as f32;
            let max = frames.iter().cloned().fold(0., f32::max);
            let slow = frames.iter().filter(|f| **f > 9.).count();
            crate::bench::report();
            eprintln!("fps {:.1}  avg {:.2}ms  max {:.2}ms  >9ms: {}/{}", 1000. / avg, avg, max, slow, frames.len());
            stats.2.clear();
            stats.1 = now;
        }
    });
    if let Some(editor) = workspace.active_editor() {
        editor.update(cx, |e, cx| e.bench_scroll(6., cx));
    }
    window.request_animation_frame();
}
