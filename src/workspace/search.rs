//! Find in Project in the sidebar: a search field, then matches grouped by file.
//! Choosing a match opens the file with the match selected.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, StyledText, Task, Window, div,
    prelude::FluentBuilder, px, uniform_list,
};

use crate::project::index;
use crate::settings::Settings;
use crate::theme::{self, alpha};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::{file_icon, icon_view};

const MAX_MATCHES: usize = 2000;
const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024;
/// Context kept before a match in its preview: short, so the match stays visible in
/// a narrow sidebar.
const PREVIEW_LEAD: usize = 20;
const PREVIEW_LENGTH: usize = 160;

pub enum SearchEvent {
    Open { path: PathBuf, range: Range<usize> },
}

#[derive(Clone)]
pub struct SearchMatch {
    /// 1-based line number.
    pub line: usize,
    /// The match in the file's text, as bytes.
    pub range: Range<usize>,
    /// The line, trimmed and shortened around the match.
    pub preview: String,
    pub preview_range: Range<usize>,
}

#[derive(Clone)]
pub struct FileResult {
    pub path: PathBuf,
    pub relative: String,
    pub matches: Vec<SearchMatch>,
}

#[derive(Default)]
struct Outcome {
    files: Vec<FileResult>,
    match_count: usize,
    truncated: bool,
}

/// The current text of edited files, searched instead of what's on disk.
pub type UnsavedTexts = Box<dyn Fn(&App) -> HashMap<PathBuf, String>>;

pub struct ProjectSearch {
    root: PathBuf,
    pub field: Entity<TextField>,
    case_sensitive: bool,
    results: Arc<Vec<FileResult>>,
    match_count: usize,
    searching: bool,
    truncated: bool,
    collapsed: HashSet<PathBuf>,
    selected: Option<(usize, usize)>,
    task: Option<Task<()>>,
    unsaved_texts: Option<UnsavedTexts>,
}

impl EventEmitter<SearchEvent> for ProjectSearch {}

pub enum SearchFieldOutcome {
    LeaveSearch,
}

impl EventEmitter<SearchFieldOutcome> for ProjectSearch {}

impl ProjectSearch {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> ProjectSearch {
        let field = cx.new(|cx| TextField::new("Search", window, cx));
        cx.subscribe(&field, |this, field, event, cx| match event {
            TextFieldEvent::Changed => this.schedule(Duration::from_millis(200), cx),
            TextFieldEvent::Cancelled => {
                if field.read(cx).text().is_empty() {
                    cx.emit(SearchFieldOutcome::LeaveSearch);
                } else {
                    field.update(cx, |f, cx| f.set_text("", cx));
                    this.schedule(Duration::ZERO, cx);
                }
            }
            TextFieldEvent::Submitted => this.open_next(cx),
            TextFieldEvent::Blurred => {}
        })
        .detach();
        ProjectSearch {
            root,
            field,
            case_sensitive: false,
            results: Arc::new(Vec::new()),
            match_count: 0,
            searching: false,
            truncated: false,
            collapsed: HashSet::new(),
            selected: None,
            task: None,
            unsaved_texts: None,
        }
    }

    pub fn set_unsaved_texts(&mut self, provider: UnsavedTexts) {
        self.unsaved_texts = Some(provider);
    }

    /// Puts the cursor in the field, with `query` typed if given.
    pub fn focus(&mut self, query: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = query {
            self.field.update(cx, |f, cx| {
                f.set_text(query, cx);
                f.select_all(cx);
            });
            self.schedule(Duration::ZERO, cx);
        } else {
            self.field.update(cx, |f, cx| f.select_all(cx));
        }
        self.field.read(cx).focus(window);
    }

    /// Searches again, e.g. after files changed on disk.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.field.read(cx).text().is_empty() {
            self.schedule(Duration::from_millis(300), cx);
        }
    }

    pub fn note_edit(&mut self, _cx: &mut Context<Self>) {}

    fn schedule(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let query = self.field.read(cx).text().to_string();
        self.selected = None;
        if query.is_empty() {
            self.task = None;
            self.results = Arc::new(Vec::new());
            self.match_count = 0;
            self.truncated = false;
            self.searching = false;
            cx.notify();
            return;
        }
        self.searching = true;
        let root = self.root.clone();
        let case_sensitive = self.case_sensitive;
        let overrides = self.unsaved_texts.as_ref().map(|f| f(cx)).unwrap_or_default();
        self.task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let outcome = cx
                .background_executor()
                .spawn(async move { run(&root, &query, case_sensitive, &overrides) })
                .await;
            this.update(cx, |this, cx| {
                this.results = Arc::new(outcome.files);
                this.match_count = outcome.match_count;
                this.truncated = outcome.truncated;
                this.searching = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Return in the field: the next match.
    fn open_next(&mut self, cx: &mut Context<Self>) {
        let mut flat = Vec::new();
        for (f, file) in self.results.iter().enumerate() {
            for m in 0..file.matches.len() {
                flat.push((f, m));
            }
        }
        if flat.is_empty() {
            return;
        }
        let next = match self.selected.and_then(|s| flat.iter().position(|x| *x == s)) {
            Some(i) => flat[(i + 1) % flat.len()],
            None => flat[0],
        };
        self.open_match(next.0, next.1, cx);
    }

    fn open_match(&mut self, file: usize, index: usize, cx: &mut Context<Self>) {
        let Some(result) = self.results.get(file) else { return };
        let Some(found) = result.matches.get(index) else { return };
        self.selected = Some((file, index));
        cx.emit(SearchEvent::Open { path: result.path.clone(), range: found.range.clone() });
        cx.notify();
    }

    fn summary(&self) -> Option<String> {
        if self.searching {
            return Some("Searching…".into());
        }
        if self.results.is_empty() && self.match_count == 0 {
            return None;
        }
        let results = if self.match_count == 1 { "1 result".to_string() } else { format!("{} results", self.match_count) };
        let files = if self.results.len() == 1 { "1 file".to_string() } else { format!("{} files", self.results.len()) };
        let more = if self.truncated { format!(" (first {MAX_MATCHES} shown)") } else { String::new() };
        Some(format!("{results} in {files}{more}"))
    }
}

/// Plain-text search through the files Quick Open lists (so .gitignore applies).
fn run(root: &Path, query: &str, case_sensitive: bool, overrides: &HashMap<PathBuf, String>) -> Outcome {
    let Ok(regex) = regex::RegexBuilder::new(&regex::escape(query)).case_insensitive(!case_sensitive).build() else {
        return Outcome::default();
    };
    let mut outcome = Outcome::default();
    for item in index::scan(root) {
        let text = match overrides.get(&item.path) {
            Some(text) => text.clone(),
            None => match read_text(&item.path) {
                Some(text) => text,
                None => continue,
            },
        };
        let matches = search(&text, &regex, MAX_MATCHES - outcome.match_count);
        if matches.is_empty() {
            continue;
        }
        outcome.match_count += matches.len();
        outcome.files.push(FileResult { path: item.path, relative: item.relative, matches });
        if outcome.match_count >= MAX_MATCHES {
            outcome.truncated = true;
            break;
        }
    }
    outcome
}

fn read_text(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_FILE_SIZE {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    if data[..data.len().min(8192)].contains(&0) {
        return None;
    }
    Some(String::from_utf8(data.clone()).unwrap_or_else(|_| data.iter().map(|b| *b as char).collect()))
}

fn search(text: &str, regex: &regex::Regex, limit: usize) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    let mut line = 1;
    let mut counted = 0;
    for found in regex.find_iter(text) {
        if matches.len() >= limit {
            break;
        }
        if found.range().is_empty() {
            continue;
        }
        line += text.as_bytes()[counted..found.start()].iter().filter(|b| **b == b'\n').count();
        counted = found.start();
        let line_start = text[..found.start()].rfind(['\n', '\r']).map_or(0, |i| i + 1);
        let line_end = text[found.start()..].find(['\n', '\r']).map_or(text.len(), |i| found.start() + i);
        // Skip the indentation, then keep a window around the match.
        let mut start = line_start + text[line_start..found.start()].len()
            - text[line_start..found.start()].trim_start_matches([' ', '\t']).len();
        let lead_start = floor_boundary(text, found.start().saturating_sub(PREVIEW_LEAD));
        let trimmed = lead_start > start;
        start = start.max(lead_start);
        let end = ceil_boundary(text, line_end.min(found.end().max(start + PREVIEW_LENGTH)));
        let end = end.max(found.end().min(line_end));
        let mut preview = text[start..end.min(line_end).max(start)].to_string();
        let mut preview_start = found.start() - start;
        if trimmed && !text[line_start..start].trim().is_empty() {
            preview.insert(0, '…');
            preview_start += '…'.len_utf8();
        }
        let preview_end = (preview_start + found.len()).min(preview.len());
        matches.push(SearchMatch { line, range: found.range(), preview, preview_range: preview_start..preview_end });
    }
    matches
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

enum Row {
    File(usize),
    Match(usize, usize),
}

impl Render for ProjectSearch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let settings = Settings::get(cx);
        let font_size = settings.sidebar_font_size;
        let icon_width = settings.sidebar_icon_width();
        let row_height = font_size + 11.;
        let results = self.results.clone();
        let mut rows = Vec::new();
        for (f, file) in results.iter().enumerate() {
            rows.push(Row::File(f));
            if !self.collapsed.contains(&file.path) {
                for m in 0..file.matches.len() {
                    rows.push(Row::Match(f, m));
                }
            }
        }
        let rows = Arc::new(rows);
        let collapsed = self.collapsed.clone();
        let selected = self.selected;
        let entity = cx.entity();
        let case_sensitive = self.case_sensitive;

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .pt(px(2.))
            .child(
                div().px(px(10.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(7.))
                        .bg(alpha(colors.primary, 0.07))
                        .child(icon_view("search", 13., colors.secondary))
                        .child(div().flex_1().text_size(px(13.)).child(self.field.clone()))
                        .child(
                            div()
                                .id("match-case")
                                .w(px(24.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(if case_sensitive { colors.accent } else { colors.secondary })
                                .when(case_sensitive, |d| d.bg(alpha(colors.accent, 0.18)))
                                .child("Aa")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.case_sensitive = !this.case_sensitive;
                                    this.schedule(Duration::ZERO, cx);
                                })),
                        ),
                ),
            )
            .when_some(self.summary(), |d, summary| {
                d.child(div().px(px(14.)).text_size(px(11.)).text_color(colors.secondary).child(summary))
            })
            .child(
                uniform_list("search-results", rows.len(), move |range, _, cx| {
                    let colors = theme::colors(cx);
                    range
                        .map(|index| {
                            let entity = entity.clone();
                            match rows[index] {
                                Row::File(f) => {
                                    let file = &results[f];
                                    let is_collapsed = collapsed.contains(&file.path);
                                    let (icon, tint) = file_icon::style(&file.path, false, false, colors.dark);
                                    let folder = Path::new(&file.relative)
                                        .parent()
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    let path = file.path.clone();
                                    div()
                                        .id(index)
                                        .h(px(row_height))
                                        .mx(px(6.))
                                        .px(px(4.))
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .rounded(px(8.))
                                        .text_size(px(font_size))
                                        .child(
                                            div()
                                                .w(px(12.))
                                                .flex()
                                                .justify_center()
                                                .child(icon_view(if is_collapsed { "chevron-right" } else { "chevron-down" }, 10., colors.secondary)),
                                        )
                                        .child(div().w(px(icon_width)).flex().justify_center().mr(px(2.)).child(icon_view(icon, icon_width * 0.85, tint)))
                                        .child(div().flex_none().child(crate::buffer::file_name(&file.path)))
                                        .when(!folder.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .text_size(px(11.))
                                                    .text_color(colors.secondary)
                                                    .child(folder),
                                            )
                                        })
                                        .child(div().flex_1())
                                        .child(
                                            div()
                                                .px(px(6.))
                                                .rounded_full()
                                                .bg(alpha(colors.primary, 0.08))
                                                .text_size(px(10.))
                                                .text_color(colors.secondary)
                                                .child(file.matches.len().to_string()),
                                        )
                                        .on_click(move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                if !this.collapsed.remove(&path) {
                                                    this.collapsed.insert(path.clone());
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .into_any_element()
                                }
                                Row::Match(f, m) => {
                                    let found = &results[f].matches[m];
                                    let is_selected = selected == Some((f, m));
                                    let preview = SharedString::from(found.preview.clone());
                                    let highlight = gpui::HighlightStyle {
                                        color: Some(colors.label),
                                        font_weight: Some(gpui::FontWeight::BOLD),
                                        background_color: Some(alpha(colors.accent, 0.25)),
                                        ..Default::default()
                                    };
                                    div()
                                        .id(index)
                                        .h(px(row_height))
                                        .mx(px(6.))
                                        .pl(px(32.))
                                        .pr(px(6.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(8.))
                                        .when(is_selected, |d| d.bg(alpha(colors.primary, 0.115)))
                                        .text_size(px(font_size + 1.))
                                        .text_color(colors.secondary)
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(StyledText::new(preview).with_highlights(vec![(found.preview_range.clone(), highlight)]))
                                        .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.open_match(f, m, cx)))
                                        .into_any_element()
                                }
                            }
                        })
                        .collect()
                })
                .flex_1(),
            )
    }
}
