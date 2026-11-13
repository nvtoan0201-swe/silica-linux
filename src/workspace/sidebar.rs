//! The sidebar: a header on the title bar row, then the file tree or, in search mode,
//! Find in Project.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    App, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, StyledText, Window, div, prelude::FluentBuilder, px,
    uniform_list,
};

use super::{
    CollapseFolders, FindInProject, ShowExplorer, SidebarMode, TITLE_BAR_HEIGHT, TreeCollapse,
    TreeDown, TreeEdit, TreeExpand, TreeOpen, TreeRename, TreeTrash, TreeUp, Workspace,
};
use crate::project::index;
use crate::settings::Settings;
use crate::theme::{self, alpha};
use crate::ui::chrome::{self, TrafficLightActions};
use crate::ui::menu::MenuItem;
use crate::ui::{capsule, file_icon, icon_view, title_bar_button};

/// The extra leading space for each nesting level.
const INDENT_WIDTH: f32 = 14.;
/// The filter field's height plus its padding, so the last row can scroll clear of it.
const FILTER_CLEARANCE: f32 = 44.;

#[derive(Clone)]
pub enum RowKind {
    Node { is_dir: bool, open: bool, dimmed: bool, name: String },
    /// The name field for a new file or folder.
    Draft { is_dir: bool },
}

#[derive(Clone)]
pub struct TreeRow {
    pub path: PathBuf,
    pub depth: usize,
    pub kind: RowKind,
}

impl Workspace {
    /// The rows the tree shows: open folders' contents, or only matches while filtering.
    pub fn visible_rows(&self, cx: &App) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        let draft = match &self.tree_edit {
            Some(TreeEdit::Create { folder, is_dir }) => Some((folder.clone(), *is_dir)),
            _ => None,
        };
        let filtering = self.is_filtering(cx);
        let filtered = if filtering {
            match &self.filter_result {
                Some(result) => Some(&result.visible),
                None => return rows,
            }
        } else {
            None
        };

        fn add(
            nodes: &[crate::project::tree::FileNode],
            depth: usize,
            folder: &Path,
            draft: &Option<(PathBuf, bool)>,
            filtered: Option<&std::collections::HashSet<String>>,
            rows: &mut Vec<TreeRow>,
        ) {
            // A new item's name field goes first in its folder.
            if let Some((draft_folder, is_dir)) = draft {
                if draft_folder == folder {
                    rows.push(TreeRow {
                        path: folder.join("\u{0}new"),
                        depth,
                        kind: RowKind::Draft { is_dir: *is_dir },
                    });
                }
            }
            for node in nodes {
                if let Some(filtered) = filtered {
                    if !filtered.contains(&node.relative) {
                        continue;
                    }
                }
                let open = node.is_dir && (filtered.is_some() || node.expanded);
                rows.push(TreeRow {
                    path: node.path.clone(),
                    depth,
                    kind: RowKind::Node { is_dir: node.is_dir, open, dimmed: node.is_dimmed(), name: node.name.clone() },
                });
                if open {
                    if let Some(children) = &node.children {
                        add(children, depth + 1, &node.path, draft, filtered, rows);
                    }
                }
            }
        }
        if let Some(children) = &self.tree.children {
            add(children, 0, &self.tree.path, &draft, filtered, &mut rows);
        }
        rows
    }

    fn move_tree_selection(&mut self, offset: isize, cx: &mut Context<Self>) {
        let rows: Vec<TreeRow> =
            self.visible_rows(cx).into_iter().filter(|r| matches!(r.kind, RowKind::Node { .. })).collect();
        if rows.is_empty() {
            return;
        }
        let current = self.selection.as_ref().and_then(|s| rows.iter().position(|r| &r.path == s));
        let next = match current {
            Some(index) => (index as isize + offset).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.selection = Some(rows[next].path.clone());
        let all = self.visible_rows(cx);
        if let Some(index) = all.iter().position(|r| r.path == rows[next].path) {
            self.tree_scroll.scroll_to_item(index, gpui::ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// ← / → collapse and expand the selected folder; on a file, ← goes to its folder.
    fn set_selected_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.clone() else { return };
        let root = self.tree.path.clone();
        match self.tree.find_mut(&selection) {
            Some(node) if node.is_dir && node.expanded != expanded => node.set_expanded(expanded),
            _ if !expanded => {
                if let Some(parent) = selection.parent().filter(|p| *p != root) {
                    self.selection = Some(parent.to_path_buf());
                    let rows = self.visible_rows(cx);
                    if let Some(index) = rows.iter().position(|r| r.path == parent) {
                        self.tree_scroll.scroll_to_item(index, gpui::ScrollStrategy::Top);
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn render_sidebar(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let this = cx.entity().downgrade();
        let traffic = chrome::traffic_lights(
            window,
            cx,
            TrafficLightActions {
                close: Box::new(move |window, cx| {
                    this.update(cx, |this, cx| this.close_window(window, cx)).ok();
                }),
            },
        );
        let search_mode = self.sidebar_mode == SidebarMode::Search;
        let header = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_size(px(13.))
                    .child(if search_mode { "Search".to_string() } else { self.name() }),
            )
            .child(div().w(px(8.)))
            .when(!search_mode, |d| {
                d.child(title_bar_button("collapse", "fold-vertical", colors.secondary, cx, |_, window, cx| {
                    window.dispatch_action(Box::new(CollapseFolders), cx)
                }))
            })
            .child(title_bar_button(
                "search",
                "search",
                if search_mode { colors.accent } else { colors.secondary },
                cx,
                move |_, window, cx| {
                    if search_mode {
                        window.dispatch_action(Box::new(ShowExplorer), cx)
                    } else {
                        window.dispatch_action(Box::new(FindInProject), cx)
                    }
                },
            ))
            .child(title_bar_button("toggle-sidebar", "panel-left", colors.secondary, cx, |_, window, cx| {
                window.dispatch_action(Box::new(super::ToggleSidebar), cx)
            }));

        let title_row = chrome::drag_area(
            div()
                .id("sidebar-title")
                .h(px(TITLE_BAR_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .pl(px(16.))
                .pr(px(4.))
                .child(div().w(px(super::TRAFFIC_LIGHTS_WIDTH - 16.)).flex_none().child(traffic))
                .child(header),
        );

        let body = match self.sidebar_mode {
            SidebarMode::Explorer => self.render_tree(window, cx).into_any_element(),
            SidebarMode::Search => div().flex_1().min_h_0().child(self.search.clone()).into_any_element(),
        };

        div()
            .id("sidebar")
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .child(title_row)
            .child(body)
            // The invisible strip on the trailing edge that resizes the sidebar.
            .child(
                div()
                    .id("sidebar-resize")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(-4.))
                    .w(px(8.))
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.sidebar_drag = Some((f32::from(event.position.x), Settings::get(cx).sidebar_width));
                    })),
            )
    }

    fn render_tree(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let settings = Settings::get(cx);
        let font_size = settings.sidebar_font_size;
        let icon_width = settings.sidebar_icon_width();
        let row_height = font_size + 11.;
        let rows = Arc::new(self.visible_rows(cx));
        let filtering = self.is_filtering(cx);
        let filter_text = self.filter_text(cx);
        let selection = self.selection.clone();
        let name_field = self.name_field().cloned();
        let renaming = match &self.tree_edit {
            Some(TreeEdit::Rename(path)) => Some(path.clone()),
            _ => None,
        };
        let dirty: std::collections::HashSet<PathBuf> =
            self.buffers.iter().filter(|(_, b)| b.is_dirty(cx)).map(|(p, _)| p.clone()).collect();
        let entity = cx.entity();
        let row_count = rows.len();
        let empty_message = if filtering {
            Some(format!("No files match “{filter_text}”"))
        } else if row_count == 0 {
            Some("This project is empty".to_string())
        } else {
            None
        };

        let list = uniform_list("file-tree", row_count + 1, move |range, _, cx| {
            let colors = theme::colors(cx);
            range
                .map(|index| {
                    // Room to scroll the last rows up from under the filter field.
                    if index == rows.len() {
                        return div().id(index).h(px(row_height)).into_any_element();
                    }
                    let row = &rows[index];
                    let entity = entity.clone();
                    let leading = 4. + row.depth as f32 * INDENT_WIDTH;
                    let base = div()
                        .id(index)
                        .h(px(row_height))
                        .w_full()
                        .pl(px(leading))
                        .pr(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .rounded(px(8.))
                        .text_size(px(font_size));
                    match &row.kind {
                        RowKind::Draft { is_dir } => base
                            .child(div().w(px(12.)))
                            .child(
                                div()
                                    .w(px(icon_width))
                                    .mr(px(2.))
                                    .flex()
                                    .justify_center()
                                    .child({
                                        let (icon, tint) = file_icon::style(Path::new("untitled"), *is_dir, false, colors.dark);
                                        icon_view(icon, icon_width * 0.9, tint)
                                    }),
                            )
                            .child(name_box(name_field.clone(), &colors))
                            .into_any_element(),
                        RowKind::Node { is_dir, open, dimmed, name } => {
                            let is_selected = selection.as_ref() == Some(&row.path);
                            let is_renaming = renaming.as_ref() == Some(&row.path);
                            let (icon, tint) = file_icon::style(&row.path, *is_dir, *open, colors.dark);
                            let path = row.path.clone();
                            let is_dir = *is_dir;
                            let chevron = if is_dir {
                                let path = path.clone();
                                let entity = entity.clone();
                                div()
                                    .id(("chevron", index))
                                    .w(px(12.))
                                    .h(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon_view(if *open { "chevron-down" } else { "chevron-right" }, 10., colors.secondary))
                                    .when(!filtering, |d| {
                                        d.on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            entity.update(cx, |this, cx| this.toggle_expansion(&path, cx));
                                        })
                                    })
                                    .into_any_element()
                            } else {
                                div().w(px(12.)).into_any_element()
                            };
                            let label = if is_renaming {
                                name_box(name_field.clone(), &colors).into_any_element()
                            } else {
                                let mut highlights = Vec::new();
                                if filtering {
                                    if let Some(range) = index::match_range(&filter_text, name) {
                                        highlights.push((
                                            range,
                                            gpui::HighlightStyle {
                                                font_weight: Some(gpui::FontWeight::BOLD),
                                                background_color: Some(alpha(colors.accent, 0.25)),
                                                ..Default::default()
                                            },
                                        ));
                                    }
                                }
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(StyledText::new(name.clone()).with_highlights(highlights))
                                    .into_any_element()
                            };
                            let menu_path = path.clone();
                            let menu_entity = entity.clone();
                            base.when(is_selected, |d| d.bg(alpha(colors.primary, 0.115)))
                                .child(chevron)
                                .child(
                                    div()
                                        .flex()
                                        .flex_1()
                                        .min_w_0()
                                        .items_center()
                                        .gap(px(4.))
                                        // Dotfiles and gitignored items fade back but stay clickable.
                                        .when(*dimmed && !is_renaming, |d| d.opacity(0.5))
                                        .child(div().w(px(icon_width)).mr(px(2.)).flex().justify_center().child(icon_view(icon, icon_width * 0.9, tint)))
                                        .child(label),
                                )
                                .when(dirty.contains(&row.path), |d| {
                                    d.child(div().size(px(7.)).flex_none().rounded_full().bg(alpha(colors.primary, 0.6)))
                                })
                                .on_click(move |event, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        window.focus(&this.tree_focus);
                                        if event.click_count() >= 2 && !is_dir {
                                            this.open(&path, window, cx);
                                        } else {
                                            this.select_tree_row(&path, is_dir, window, cx);
                                        }
                                    });
                                })
                                .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                    let position = event.position;
                                    menu_entity.update(cx, |this, cx| {
                                        this.selection = Some(menu_path.clone());
                                        let items = this.file_menu(&menu_path, is_dir, cx);
                                        this.show_context_menu(position, items, window, cx);
                                    });
                                })
                                .into_any_element()
                        }
                    }
                })
                .collect()
        })
        .track_scroll(self.tree_scroll.clone())
        .px(px(6.))
        .size_full();

        let root_menu = cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
            let root = this.root_path.clone();
            let entity = cx.entity().downgrade();
            let entity2 = entity.clone();
            let root2 = root.clone();
            let items = vec![
                MenuItem::new("New File…", move |window, cx| {
                    entity.update(cx, |this, cx| this.begin_new_item(false, Some(&root), window, cx)).ok();
                }),
                MenuItem::new("New Folder…", move |window, cx| {
                    entity2.update(cx, |this, cx| this.begin_new_item(true, Some(&root2), window, cx)).ok();
                }),
            ];
            this.show_context_menu(event.position, items, window, cx);
        });

        let filter = self.render_filter_field(cx);
        div()
            .id("tree")
            .key_context("FileTree")
            .track_focus(&self.tree_focus)
            .flex_1()
            .min_h_0()
            .relative()
            .on_action(cx.listener(|this, _: &TreeUp, _, cx| this.move_tree_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &TreeDown, _, cx| this.move_tree_selection(1, cx)))
            .on_action(cx.listener(|this, _: &TreeExpand, _, cx| this.set_selected_expanded(true, cx)))
            .on_action(cx.listener(|this, _: &TreeCollapse, _, cx| this.set_selected_expanded(false, cx)))
            .on_action(cx.listener(|this, _: &TreeOpen, window, cx| {
                if let Some(selection) = this.selection.clone() {
                    let is_dir = selection.is_dir();
                    this.select_tree_row(&selection, is_dir, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &TreeRename, window, cx| {
                if this.tree_edit.is_none() {
                    if let Some(selection) = this.selection.clone() {
                        this.begin_rename(&selection, window, cx);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &TreeTrash, window, cx| {
                if this.tree_edit.is_none() {
                    if let Some(selection) = this.selection.clone() {
                        this.move_to_trash(&selection, window, cx);
                    }
                }
            }))
            .on_mouse_down(MouseButton::Right, root_menu)
            .child(list)
            .when_some(empty_message, |d, message| {
                d.child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .left(px(20.))
                        .right(px(10.))
                        .text_size(px(font_size))
                        .text_color(colors.secondary)
                        .child(message),
                )
            })
            // The filter floats over the bottom of the list, and rows scroll under it.
            .child(div().absolute().bottom_0().left_0().right_0().p(px(8.)).child(filter))
            .child(div().h(px(FILTER_CLEARANCE - row_height).max(px(0.))))
    }

    /// The filter field under the explorer tree: shows only files whose names contain
    /// the text, in their folders. Ctrl+Alt+J focuses it; Escape clears it.
    fn render_filter_field(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let colors = theme::colors(cx);
        let filtering = self.is_filtering(cx);
        let count = self.filter_result.as_ref().filter(|_| filtering).map(|r| r.match_count);
        let has_text = !self.filter_field.read(cx).text().is_empty();
        capsule(cx)
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .text_size(px(13.))
            .child(icon_view("list-filter", 14., if filtering { colors.accent } else { colors.secondary }))
            .child(self.filter_field.clone())
            .when_some(count, |d, count| {
                d.child(div().text_size(px(11.)).text_color(colors.secondary).child(count.to_string()))
            })
            .when(has_text, |d| {
                d.child(
                    div()
                        .id("clear-filter")
                        .child(icon_view("circle-x", 14., colors.secondary))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter_field.update(cx, |f, cx| f.set_text("", cx));
                            this.filter_result = None;
                            cx.notify();
                        })),
                )
            })
    }
}

/// The inline name field for renaming or naming a new item.
fn name_box(field: Option<gpui::Entity<crate::ui::text_field::TextField>>, colors: &theme::Colors) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .px(px(4.))
        .h(px(20.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(if colors.dark { alpha(gpui::black(), 0.3) } else { gpui::white().into() })
        .border_1()
        .border_color(colors.accent)
        .when_some(field, |d, field| d.child(field))
}
