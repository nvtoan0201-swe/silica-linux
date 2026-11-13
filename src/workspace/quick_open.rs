//! Quick Open (Ctrl+P): a search field over a fuzzy-ranked list of the project's
//! files. Typing ":42" goes to a line, "name:42" opens a file at one.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyBinding, ParentElement, Render, ScrollStrategy, SharedString, StatefulInteractiveElement,
    Styled, StyledText, TextStyleRefinement, UniformListScrollHandle, Window, actions, div,
    prelude::FluentBuilder, px, uniform_list,
};

use crate::project::fuzzy::{self, FileItem};
use crate::theme::{self, alpha};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::{file_icon, icon_view};

actions!(quick_open, [SelectNext, SelectPrevious]);

const RESULT_LIMIT: usize = 60;
const ROW_HEIGHT: f32 = 30.;
const MAX_LIST_HEIGHT: f32 = 400.;

pub enum QuickOpenEvent {
    Open { path: PathBuf, line: Option<usize> },
    GoToLine(usize),
    Dismiss,
}

struct QuickResult {
    item: FileItem,
    positions: Vec<usize>,
    recent: bool,
}

pub struct QuickOpen {
    field: Entity<TextField>,
    root: PathBuf,
    items: Arc<Vec<FileItem>>,
    indexing: bool,
    results: Vec<QuickResult>,
    selected: usize,
    /// From a trailing ":42" in the query: the line to go to.
    line_target: Option<usize>,
    /// The query is only ":" and a number: Go to Line in the current file.
    go_to_line: bool,
    active_file: Option<PathBuf>,
    /// Most recently opened files first.
    recent: Vec<PathBuf>,
    /// Lines in the active file, for the Go to Line prompt.
    line_count: Option<usize>,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<QuickOpenEvent> for QuickOpen {}

impl Focusable for QuickOpen {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl QuickOpen {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root: PathBuf,
        items: Arc<Vec<FileItem>>,
        indexing: bool,
        recent: Vec<PathBuf>,
        active_file: Option<PathBuf>,
        line_count: Option<usize>,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> QuickOpen {
        let field = cx.new(|cx| {
            let mut field = TextField::new("Search files by name (add :line to jump)", window, cx);
            field.set_text(query, cx);
            field
        });
        field.read(cx).focus(window);
        cx.subscribe(&field, |this, _, event, cx| match event {
            TextFieldEvent::Changed => this.update_results(cx),
            TextFieldEvent::Submitted => this.confirm(cx),
            TextFieldEvent::Cancelled => cx.emit(QuickOpenEvent::Dismiss),
            TextFieldEvent::Blurred => {}
        })
        .detach();
        let mut quick_open = QuickOpen {
            field,
            root,
            items,
            indexing,
            results: Vec::new(),
            selected: 0,
            line_target: None,
            go_to_line: false,
            active_file,
            recent,
            line_count,
            scroll: UniformListScrollHandle::new(),
        };
        quick_open.update_results(cx);
        quick_open
    }

    /// The fresh index arrived: rank again, keeping the highlighted row if it's still there.
    pub fn set_items(&mut self, items: Arc<Vec<FileItem>>, cx: &mut Context<Self>) {
        let selected = self.results.get(self.selected).map(|r| r.item.path.clone());
        self.items = items;
        self.indexing = false;
        self.update_results(cx);
        if let Some(selected) = selected {
            if let Some(index) = self.results.iter().position(|r| r.item.path == selected) {
                self.selected = index;
            }
        }
    }

    fn update_results(&mut self, cx: &mut Context<Self>) {
        let query = self.field.read(cx).text().to_string();
        let mut text = query.as_str();
        self.line_target = None;
        if let Some(colon) = query.rfind(':') {
            let suffix = &query[colon + 1..];
            if suffix.chars().all(|c| c.is_ascii_digit()) {
                self.line_target = suffix.parse().ok();
                text = &query[..colon];
            }
        }
        self.go_to_line = text.is_empty() && query.starts_with(':');
        self.selected = 0;
        if self.go_to_line {
            self.results.clear();
            cx.notify();
            return;
        }
        let prepared = fuzzy::prepare(text);
        let extra = self.ignored_recent_items();
        if prepared.is_empty() {
            self.results = self.default_results(&extra);
        } else {
            let mut matches: Vec<(FileItem, fuzzy::FuzzyMatch)> = self
                .items
                .iter()
                .chain(extra.iter())
                .filter_map(|item| fuzzy::fuzzy_match(&prepared, item).map(|m| (item.clone(), m)))
                .collect();
            matches.sort_by(|a, b| {
                b.1.score
                    .cmp(&a.1.score)
                    .then(a.0.relative.len().cmp(&b.0.relative.len()))
                    .then(a.0.relative.cmp(&b.0.relative))
            });
            self.results = matches
                .into_iter()
                .take(RESULT_LIMIT)
                .map(|(item, m)| QuickResult { item, positions: m.positions, recent: false })
                .collect();
        }
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// With no query: recently opened files (except the current one), then the rest by path.
    fn default_results(&self, extra: &[FileItem]) -> Vec<QuickResult> {
        let mut seen = HashSet::new();
        let mut results = Vec::new();
        for path in &self.recent {
            if Some(path) == self.active_file.as_ref() {
                continue;
            }
            if let Some(item) = self.items.iter().chain(extra.iter()).find(|i| &i.path == path) {
                seen.insert(path.clone());
                results.push(QuickResult { item: item.clone(), positions: Vec::new(), recent: true });
            }
        }
        for item in self.items.iter() {
            if results.len() >= RESULT_LIMIT {
                break;
            }
            if seen.contains(&item.path) || Some(&item.path) == self.active_file.as_ref() {
                continue;
            }
            results.push(QuickResult { item: item.clone(), positions: Vec::new(), recent: false });
        }
        results.truncate(RESULT_LIMIT);
        results
    }

    /// Recently opened files the index left out (e.g. gitignored), so they can still
    /// be found again, as in VS Code.
    fn ignored_recent_items(&self) -> Vec<FileItem> {
        let indexed: HashSet<&PathBuf> = self.items.iter().map(|i| &i.path).collect();
        self.recent
            .iter()
            .filter(|p| !indexed.contains(p) && p.is_file())
            .filter_map(|p| {
                let relative = p.strip_prefix(&self.root).ok()?.to_string_lossy().to_string();
                Some(FileItem::new(p.clone(), relative))
            })
            .collect()
    }

    fn move_selection(&mut self, offset: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let count = self.results.len() as isize;
        self.selected = ((self.selected as isize + offset + count) % count) as usize;
        self.scroll.scroll_to_item(self.selected, ScrollStrategy::Center);
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.go_to_line {
            if let Some(line) = self.line_target {
                cx.emit(QuickOpenEvent::GoToLine(line));
            }
            return;
        }
        if let Some(result) = self.results.get(self.selected) {
            cx.emit(QuickOpenEvent::Open { path: result.item.path.clone(), line: self.line_target });
        }
    }

    fn go_to_line_prompt(&self) -> String {
        let count = self.line_count.unwrap_or(1);
        match self.line_target {
            Some(line) => format!("Go to line {}", line.clamp(1, count)),
            None => format!("Type a line number between 1 and {count}"),
        }
    }
}

/// Bolds the matched characters and draws them in the primary color.
fn highlighted(text: &str, offset: usize, positions: &HashSet<usize>, base: gpui::Hsla, strong: gpui::Hsla) -> StyledText {
    let mut highlights = Vec::new();
    for (index, (byte, c)) in text.char_indices().enumerate() {
        if positions.contains(&(offset + index)) {
            highlights.push((
                byte..byte + c.len_utf8(),
                gpui::HighlightStyle {
                    color: Some(strong),
                    font_weight: Some(gpui::FontWeight::BOLD),
                    ..Default::default()
                },
            ));
        }
    }
    let _ = base;
    StyledText::new(SharedString::from(text.to_string())).with_highlights(highlights)
}

impl Render for QuickOpen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let count = self.results.len();
        let selected = self.selected;
        let list_height = (count as f32 * ROW_HEIGHT + 12.).min(MAX_LIST_HEIGHT);
        let rows: Vec<_> = self
            .results
            .iter()
            .map(|r| (r.item.clone(), r.positions.iter().copied().collect::<HashSet<usize>>(), r.recent))
            .collect();
        let rows = Arc::new(rows);
        let entity = cx.entity();

        let body = if self.go_to_line {
            div()
                .h(px(ROW_HEIGHT + 8.))
                .px(px(14.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(icon_view("hash", 14., colors.secondary))
                .child(self.go_to_line_prompt())
                .into_any_element()
        } else if count == 0 {
            div()
                .py(px(18.))
                .w_full()
                .flex()
                .justify_center()
                .text_color(colors.secondary)
                .child(if self.indexing { "Indexing files…" } else { "No matching files" })
                .into_any_element()
        } else {
            uniform_list("quick-open-results", count, move |range, _, cx| {
                let colors = theme::colors(cx);
                range
                    .map(|index| {
                        let (item, positions, recent) = &rows[index];
                        let (icon, tint) = file_icon::style(&item.path, false, false, colors.dark);
                        let name_start = item.name_start;
                        let folder: String = item.relative.chars().take(name_start.saturating_sub(1)).collect();
                        let entity = entity.clone();
                        div()
                            .id(index)
                            .h(px(ROW_HEIGHT))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .rounded(px(8.))
                            .when(index == selected, |d| d.bg(alpha(colors.accent, 0.22)))
                            .child(icon_view(icon, 14., tint))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(colors.label)
                                    .child(highlighted(item.name(), name_start, positions, colors.label, colors.label)),
                            )
                            .when(!folder.is_empty(), |d| {
                                d.child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .text_size(px(12.))
                                        .text_color(colors.secondary)
                                        .child(highlighted(&folder, 0, positions, colors.secondary, colors.label)),
                                )
                            })
                            .child(div().flex_1())
                            .when(*recent, |d| {
                                d.child(div().flex_none().text_size(px(11.)).text_color(colors.tertiary).child("recently opened"))
                            })
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.selected = index;
                                    this.confirm(cx);
                                });
                            })
                    })
                    .collect()
            })
            .track_scroll(self.scroll.clone())
            .h(px(list_height))
            .p(px(6.))
            .into_any_element()
        };

        div()
            .key_context("QuickOpen")
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.move_selection(-1, cx)))
            .w_full()
            .max_w(px(640.))
            .flex()
            .flex_col()
            .bg(colors.popover)
            .border_1()
            .border_color(colors.popover_border)
            .rounded(px(16.))
            .shadow(vec![gpui::BoxShadow {
                color: alpha(gpui::black(), if colors.dark { 0.5 } else { 0.2 }),
                offset: gpui::point(px(0.), px(10.)),
                blur_radius: px(30.),
                spread_radius: px(0.),
            }])
            .text_size(px(14.))
            .text_color(colors.label)
            .child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon_view("search", 17., colors.secondary))
                    .child(div().flex_1().text_size(px(17.)).child(self.field.clone()))
                    .when(self.indexing, |d| {
                        d.child(div().text_size(px(11.)).text_color(colors.tertiary).child("Indexing…"))
                    }),
            )
            .child(div().h(px(1.)).bg(colors.separator))
            .child(body)
            .map(|d| {
                let _ = TextStyleRefinement::default();
                d
            })
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some("QuickOpen")),
        KeyBinding::new("up", SelectPrevious, Some("QuickOpen")),
        KeyBinding::new("ctrl-n", SelectNext, Some("QuickOpen")),
        KeyBinding::new("ctrl-p", SelectPrevious, Some("QuickOpen")),
    ]);
}
