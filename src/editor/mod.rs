//! The code editor: a text view drawn by GPUI, with syntax colors, a line-number gutter,
//! indent guides, word wrap, a minimap and the editing commands Silica has on macOS.

mod commands;
mod display;
mod element;
mod history;
mod input;
mod wrap;

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, Font, Pixels, Point, ShapedLine, Task,
    TextRun, Window, font, point, px,
};
use ropey::Rope;

use crate::settings::Settings;
use crate::syntax::{Language, Span};
use crate::text::{self, Indentation};

pub use commands::*;
pub use element::MINIMAP_WIDTH;
use display::DisplayLine;
use element::LayoutSnapshot;
use history::{Edit, EditKind, History, Selection};
use wrap::WrapMap;

/// Above this many bytes, highlighting is skipped entirely.
const MAX_HIGHLIGHT_LEN: usize = 2_000_000;
/// Up to this size the first pass runs synchronously, so files open already colored.
const SYNCHRONOUS_HIGHLIGHT_LEN: usize = 100_000;
const HIGHLIGHT_DELAY: Duration = Duration::from_millis(120);
const BLINK_INTERVAL: Duration = Duration::from_millis(530);

/// Where the cursor is, for the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorStatus {
    pub line: usize,
    pub column: usize,
    /// Characters selected.
    pub selected: usize,
}

pub enum EditorEvent {
    /// The text changed (typed, pasted, undone…).
    Edited,
    SelectionChanged,
}

/// How to bring the cursor into view at the next layout.
#[derive(Clone, Copy, PartialEq)]
enum Autoscroll {
    /// Scroll just enough to show it.
    Fit,
    /// Put it in the middle of the editor.
    Center,
    /// Put the line at the top.
    Top(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum SelectMode {
    Character,
    Word,
    Line,
}

pub struct Editor {
    focus_handle: FocusHandle,
    rope: Rope,
    language: Option<Language>,
    line_ending: &'static str,
    indentation: Option<Indentation>,
    selection: Range<usize>,
    reversed: bool,
    /// IME text being composed.
    marked: Option<Range<usize>>,
    /// The x the cursor keeps while moving up and down.
    goal_x: Option<f32>,
    history: History,
    /// How far the text is scrolled, in pixels from the top-left.
    scroll: Point<f32>,
    autoscroll: Option<Autoscroll>,
    spans: Vec<Span>,
    highlight_generation: u64,
    highlight_task: Option<Task<()>>,
    wraps: WrapMap,
    layout: Option<Rc<LayoutSnapshot>>,
    /// Bumped by every change to the text.
    version: u64,
    /// The drag that is selecting text, and where it started.
    selecting: Option<(SelectMode, Range<usize>)>,
    minimap_drag: Option<(f32, f32)>,
    minimap_hovered: bool,
    cursor_on: bool,
    blink_epoch: usize,
    _blink_task: Option<Task<()>>,
    /// Find-in-file matches to mark in the text and minimap.
    find_matches: Vec<Range<usize>>,
    find_current: Option<usize>,
    /// Edits made in the current `transact`, for its undo step.
    pending_edits: Option<Vec<Edit>>,
    /// The widest line in columns, with the text version and tab width it was measured at.
    longest_line: (u64, usize, usize),
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Editor {
    pub fn new(text: &str, language: Option<Language>, cx: &mut Context<Editor>) -> Editor {
        let rope = Rope::from_str(text);
        let line_count = rope.len_lines();
        let focus_handle = cx.focus_handle();
        let mut editor = Editor {
            focus_handle,
            line_ending: text::detect_line_ending(text),
            rope,
            language,
            indentation: None,
            selection: 0..0,
            reversed: false,
            marked: None,
            goal_x: None,
            history: History::default(),
            scroll: Point::default(),
            autoscroll: None,
            spans: Vec::new(),
            highlight_generation: 0,
            highlight_task: None,
            wraps: WrapMap::new(line_count),
            layout: None,
            version: 0,
            selecting: None,
            minimap_drag: None,
            minimap_hovered: false,
            cursor_on: true,
            blink_epoch: 0,
            _blink_task: None,
            find_matches: Vec::new(),
            find_current: None,
            pending_edits: None,
            longest_line: (u64::MAX, 0, 0),
        };
        editor.highlight_now(cx);
        editor
    }

    // MARK: Text

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_ending(&self) -> &'static str {
        self.line_ending
    }

    pub fn language(&self) -> Option<Language> {
        self.language
    }

    /// Switches highlighting and comment markers, e.g. after a rename changes the extension.
    pub fn set_language(&mut self, language: Option<Language>, cx: &mut Context<Self>) {
        if self.language == language {
            return;
        }
        self.language = language;
        self.spans.clear();
        self.highlight_now(cx);
        cx.notify();
    }

    /// How the file indents; detected when it opens and kept, as in VS Code.
    pub fn indentation(&mut self, cx: &App) -> Indentation {
        if let Some(indentation) = self.indentation {
            return indentation;
        }
        let settings = Settings::get(cx);
        let detected =
            text::detect_indentation(&self.rope, settings.tab_width, settings.default_indentation());
        self.indentation = Some(detected);
        detected
    }

    /// Detects the indentation again (after a reload, or a new tab width).
    pub fn redetect_indentation(&mut self) {
        self.indentation = None;
    }

    pub fn tab_width(cx: &App) -> usize {
        Settings::get(cx).tab_width
    }

    /// Replaces the whole text (the file as changed on disk) as one undoable edit. Only
    /// the differing middle is replaced, so the cursor and scroll stay put when the
    /// change is elsewhere.
    pub fn replace_all(&mut self, new_text: &str, cx: &mut Context<Self>) {
        let old = self.rope.to_string();
        let mut prefix = old
            .as_bytes()
            .iter()
            .zip(new_text.as_bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(prefix) || !new_text.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let max_suffix = old.len().min(new_text.len()) - prefix;
        let mut suffix = old
            .as_bytes()
            .iter()
            .rev()
            .zip(new_text.as_bytes().iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(old.len() - suffix)
            || !new_text.is_char_boundary(new_text.len() - suffix)
        {
            suffix -= 1;
        }
        let range = prefix..old.len() - suffix;
        let replacement = &new_text[prefix..new_text.len() - suffix];
        if range.is_empty() && replacement.is_empty() {
            return;
        }
        self.redetect_indentation();
        let selection = self.selection.clone();
        self.transact(EditKind::Other, cx, |this, cx| {
            this.edit(range.clone(), replacement, cx);
            let len = this.len();
            let map = |offset: usize| {
                if offset <= range.start {
                    offset
                } else if offset >= range.end {
                    offset - range.end + range.start + replacement.len()
                } else {
                    range.start + replacement.len()
                }
                .min(len)
            };
            this.select(map(selection.start)..map(selection.end), false);
        });
    }

    // MARK: Selection

    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub fn cursor(&self) -> usize {
        if self.reversed { self.selection.start } else { self.selection.end }
    }

    fn anchor(&self) -> usize {
        if self.reversed { self.selection.end } else { self.selection.start }
    }

    pub fn selected_text(&self) -> String {
        self.slice(self.selection.clone())
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        self.rope.byte_slice(range).to_string()
    }

    /// Sets the selection without touching the scroll position.
    fn select(&mut self, range: Range<usize>, reversed: bool) {
        let len = self.len();
        let start = self.clip(range.start.min(len));
        let end = self.clip(range.end.min(len));
        if start <= end {
            self.selection = start..end;
            self.reversed = reversed && start != end;
        } else {
            self.selection = end..start;
            self.reversed = true;
        }
    }

    /// Moves the cursor and scrolls to it, ending any undo grouping.
    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.select(range, false);
        self.history.seal();
        self.goal_x = None;
        self.request_autoscroll(Autoscroll::Fit);
        self.selection_changed(cx);
    }

    /// Selects a range (a search match) and scrolls it to the middle of the editor.
    pub fn reveal(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.select(range, false);
        self.history.seal();
        self.goal_x = None;
        self.request_autoscroll(Autoscroll::Center);
        self.selection_changed(cx);
    }

    fn move_cursor_to(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        if extend {
            let anchor = self.anchor();
            self.select(anchor..offset, offset < anchor);
        } else {
            self.select(offset..offset, false);
        }
        self.history.seal();
        self.request_autoscroll(Autoscroll::Fit);
        self.selection_changed(cx);
    }

    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.cursor_on = true;
        self.blink_epoch += 1;
        cx.emit(EditorEvent::SelectionChanged);
        cx.notify();
    }

    /// The nearest character boundary at or before `offset`.
    fn clip(&self, offset: usize) -> usize {
        let offset = offset.min(self.len());
        let char_index = self.rope.byte_to_char(offset);
        self.rope.char_to_byte(char_index)
    }

    pub fn cursor_status(&self, cx: &App) -> CursorStatus {
        let cursor = self.cursor();
        let line = self.rope.byte_to_line(cursor);
        let line_start = self.rope.line_to_byte(line);
        let line_text = self.slice(line_start..cursor);
        let column = text::visual_column(&line_text, line_text.len(), Self::tab_width(cx)) + 1;
        let selected = self.rope.byte_slice(self.selection.clone()).len_chars();
        CursorStatus { line: line + 1, column, selected }
    }

    // MARK: Lines

    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.byte_to_line(offset.min(self.len()))
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.rope.line_to_byte(line.min(self.rope.len_lines()))
    }

    /// Byte range of a line, without its break.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        text::line_content_range(&self.rope, line.min(self.rope.len_lines() - 1))
    }

    pub fn line_text(&self, line: usize) -> String {
        let range = self.line_range(line);
        self.slice(range)
    }

    /// Puts the cursor at the start of the 1-based `line` and centers it.
    pub fn go_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let line = line.clamp(1, self.line_count()) - 1;
        let offset = self.line_start(line);
        self.select(offset..offset, false);
        self.history.seal();
        self.goal_x = None;
        self.request_autoscroll(Autoscroll::Center);
        self.selection_changed(cx);
    }

    /// Scrolls so the 1-based `line` is at the top.
    pub fn scroll_to_line(&mut self, line: usize, place_cursor: bool, cx: &mut Context<Self>) {
        let line = line.clamp(1, self.line_count()) - 1;
        if place_cursor {
            let offset = self.line_start(line);
            self.select(offset..offset, false);
            self.selection_changed(cx);
        }
        self.autoscroll = Some(Autoscroll::Top(line));
        cx.notify();
    }

    /// The 1-based line shown at the top of the editor.
    pub fn top_visible_line(&mut self) -> usize {
        let Some(layout) = self.layout.clone() else { return 1 };
        let row = ((self.scroll.y - layout.metrics.top_inset).max(0.) / layout.metrics.line_height)
            as usize;
        let row = row.min(self.wraps.row_count().saturating_sub(1));
        self.wraps.line_for_row(row).0 + 1
    }

    // MARK: Editing

    /// Runs `f`, which edits through `edit`, as one undo step.
    fn transact(
        &mut self,
        kind: EditKind,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        let before = Selection { range: self.selection.clone(), reversed: self.reversed };
        self.pending_edits = Some(Vec::new());
        f(self, cx);
        let edits = self.pending_edits.take().unwrap_or_default();
        if edits.is_empty() {
            cx.notify();
            return;
        }
        let after = Selection { range: self.selection.clone(), reversed: self.reversed };
        self.history.push(edits, before, after, kind);
        self.goal_x = None;
        self.request_autoscroll(Autoscroll::Fit);
        self.did_edit(cx);
    }

    /// Replaces a byte range; call inside `transact`.
    fn edit(&mut self, range: Range<usize>, new_text: &str, _cx: &mut Context<Self>) {
        let range = self.clip(range.start)..self.clip(range.end.max(range.start));
        if range.is_empty() && new_text.is_empty() {
            return;
        }
        let old = self.slice(range.clone());
        if let Some(edits) = self.pending_edits.as_mut() {
            edits.push(Edit { range: range.clone(), old, new: new_text.to_string() });
        }
        self.apply_edit(range, new_text);
    }

    /// The raw change: text, line rows, colors and marks follow it.
    fn apply_edit(&mut self, range: Range<usize>, new_text: &str) {
        let start_line = self.rope.byte_to_line(range.start);
        let old_end_line = self.rope.byte_to_line(range.end);
        let start_char = self.rope.byte_to_char(range.start);
        let end_char = self.rope.byte_to_char(range.end);
        self.rope.remove(start_char..end_char);
        self.rope.insert(start_char, new_text);
        let new_end_line = self.rope.byte_to_line(range.start + new_text.len());
        self.wraps.splice(start_line, old_end_line - start_line + 1, new_end_line - start_line + 1);
        // A line break joining "\r" and "\n" across the edit changes the line after it too.
        self.wraps.invalidate(new_end_line + 1);
        if self.wraps.line_count() != self.rope.len_lines() {
            self.wraps.reset(self.rope.len_lines());
        }
        shift_spans(&mut self.spans, &range, new_text.len());
        let delta = new_text.len() as isize - range.len() as isize;
        let shift = |offset: usize| -> usize {
            if offset >= range.end {
                (offset as isize + delta) as usize
            } else if offset > range.start {
                range.start + new_text.len()
            } else {
                offset
            }
        };
        self.marked = self.marked.take().map(|m| shift(m.start)..shift(m.end));
        self.find_matches.clear();
        self.find_current = None;
        self.version += 1;
    }

    fn did_edit(&mut self, cx: &mut Context<Self>) {
        self.cursor_on = true;
        self.blink_epoch += 1;
        self.schedule_highlight(HIGHLIGHT_DELAY, cx);
        cx.emit(EditorEvent::Edited);
        cx.emit(EditorEvent::SelectionChanged);
        cx.notify();
    }

    /// Inserts text over the selection (typing, pasting).
    pub fn insert(&mut self, new_text: &str, cx: &mut Context<Self>) {
        let range = self.marked.clone().unwrap_or(self.selection.clone());
        let kind = if new_text.contains('\n') || new_text.len() > 1 && self.marked.is_none() {
            EditKind::Other
        } else {
            EditKind::Typing
        };
        self.transact(kind, cx, |this, cx| {
            this.edit(range.clone(), new_text, cx);
            let end = range.start + new_text.len();
            this.select(end..end, false);
            this.marked = None;
        });
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(transaction) = self.history.pop_undo() else { return };
        for edit in transaction.edits.iter().rev() {
            let range = edit.range.start..edit.range.start + edit.new.len();
            self.apply_edit(range, &edit.old);
        }
        self.select(transaction.before.range.clone(), transaction.before.reversed);
        self.history.push_redo(transaction);
        self.marked = None;
        self.request_autoscroll(Autoscroll::Fit);
        self.did_edit(cx);
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(transaction) = self.history.pop_redo() else { return };
        for edit in &transaction.edits {
            self.apply_edit(edit.range.clone(), &edit.new);
        }
        self.select(transaction.after.range.clone(), transaction.after.reversed);
        self.history.push_undo_from_redo(transaction);
        self.marked = None;
        self.request_autoscroll(Autoscroll::Fit);
        self.did_edit(cx);
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    // MARK: Highlighting

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Highlights the current text, synchronously when it's small (a file just opened).
    pub fn highlight_now(&mut self, cx: &mut Context<Self>) {
        if self.len() <= SYNCHRONOUS_HIGHLIGHT_LEN {
            self.highlight_generation += 1;
            self.highlight_task = None;
            self.spans = match self.language {
                Some(language) => language.grammar().highlight(&self.text()),
                None => Vec::new(),
            };
            cx.notify();
        } else {
            self.schedule_highlight(Duration::ZERO, cx);
        }
    }

    /// Re-highlights the whole text off the main thread once typing pauses.
    fn schedule_highlight(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.highlight_generation += 1;
        let generation = self.highlight_generation;
        let Some(language) = self.language else {
            self.spans.clear();
            self.highlight_task = None;
            return;
        };
        if self.len() > MAX_HIGHLIGHT_LEN {
            self.spans.clear();
            self.highlight_task = None;
            return;
        }
        self.highlight_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Some(text) = this.update(cx, |this, _| this.rope.clone()).ok() else { return };
            let spans = cx
                .background_executor()
                .spawn(async move { language.grammar().highlight(&text.to_string()) })
                .await;
            this.update(cx, |this, cx| {
                if this.highlight_generation == generation {
                    this.spans = spans;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    // MARK: Find

    /// Marks matches of a find in the text; `current` is the one selected.
    pub fn set_find_matches(
        &mut self,
        matches: Vec<Range<usize>>,
        current: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.find_matches = matches;
        self.find_current = current;
        cx.notify();
    }

    // MARK: Scrolling

    fn request_autoscroll(&mut self, kind: Autoscroll) {
        self.autoscroll = Some(kind);
    }

    /// Starts the cursor blinking; it stops when the editor loses focus (see `render`).
    fn start_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_on = true;
        self.blink_epoch += 1;
        let epoch = self.blink_epoch;
        self._blink_task = Some(cx.spawn(async move |this, cx| {
            let mut last_epoch = epoch;
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        if this.blink_epoch != last_epoch {
                            // Moved or typed since: stay on for a full interval.
                            last_epoch = this.blink_epoch;
                            this.cursor_on = true;
                        } else {
                            this.cursor_on = !this.cursor_on;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        }));
    }

    // MARK: Fonts

    fn font(cx: &App) -> Font {
        font(Settings::get(cx).editor_font_family())
    }

    fn font_size(cx: &App) -> Pixels {
        px(Settings::get(cx).font_size)
    }

    /// Shapes one visual row for measuring (not drawing): its display text and layout.
    fn shape_row(
        &mut self,
        line: usize,
        row: usize,
        window: &mut Window,
        cx: &App,
    ) -> (ShapedLine, DisplayLine, Range<usize>) {
        let line_range = self.line_range(line);
        let source = self.slice(line_range.clone());
        let display = DisplayLine::new(&source, Self::tab_width(cx));
        let row_range = self.wraps.row_range(line, row, source.len());
        let display_range = display.to_display(row_range.start)..display.to_display(row_range.end);
        let text = display.text[display_range.clone()].to_string();
        let run = TextRun {
            len: text.len(),
            font: Self::font(cx),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(text.into(), Self::font_size(cx), &[run], None);
        (shaped, display, display_range)
    }

    /// The x (from the text's left edge) where a byte offset is drawn, and its visual row.
    fn x_and_row_for_offset(&mut self, offset: usize, window: &mut Window, cx: &App) -> (f32, usize) {
        let line = self.line_of(offset);
        let in_line = offset - self.line_start(line);
        let row = self.wraps.row_in_line(line, in_line);
        let row = row.min(self.wraps.rows_in_line(line) - 1);
        let (shaped, display, display_range) = self.shape_row(line, row, window, cx);
        let x = shaped.x_for_index(display.to_display(in_line).saturating_sub(display_range.start));
        (f32::from(x), self.wraps.first_row(line) + row)
    }

    /// The byte offset nearest a point in a visual row.
    fn offset_for_row_x(&mut self, row: usize, x: f32, window: &mut Window, cx: &App) -> usize {
        let row = row.min(self.wraps.row_count().saturating_sub(1));
        let (line, row_in_line) = self.wraps.line_for_row(row);
        let (shaped, display, display_range) = self.shape_row(line, row_in_line, window, cx);
        let index = shaped.closest_index_for_x(px(x.max(0.)));
        let mut in_line = display.to_source(display_range.start + index);
        // At the end of a wrapped row, stay on it rather than jump to the next.
        let row_end = display.to_source(display_range.end);
        let rows = self.wraps.rows_in_line(line);
        if row_in_line + 1 < rows && in_line >= row_end {
            let text = self.line_text(line);
            in_line = text[..row_end].char_indices().last().map_or(row_end, |(i, _)| i);
        }
        self.line_start(line) + in_line
    }
}

/// Keeps colors in step with an edit until the next highlight: text after it moves,
/// and inserted text takes the color of the text it was typed into.
fn shift_spans(spans: &mut Vec<Span>, range: &Range<usize>, inserted: usize) {
    let delta = inserted as isize - range.len() as isize;
    let shift = |offset: usize| (offset as isize + delta) as usize;
    let mut result = Vec::with_capacity(spans.len() + 1);
    for span in spans.drain(..) {
        if span.range.end < range.start || (span.range.end == range.start && inserted == 0) {
            result.push(span);
        } else if span.range.start > range.end || (span.range.start == range.end && span.range.start > range.start) {
            result.push(Span { range: shift(span.range.start)..shift(span.range.end), kind: span.kind });
        } else {
            // Overlaps the edit: keep the parts outside it, joined by the new text.
            let start = span.range.start.min(range.start);
            let end = if span.range.end > range.end { shift(span.range.end) } else { range.start + inserted };
            if end > start {
                result.push(Span { range: start..end, kind: span.kind });
            }
        }
    }
    // Overlapping joins can touch; keep them sorted and apart.
    result.sort_by_key(|s| s.range.start);
    let mut last_end = 0;
    result.retain_mut(|span| {
        if span.range.start < last_end {
            span.range.start = last_end;
        }
        last_end = last_end.max(span.range.end);
        span.range.start < span.range.end
    });
    *spans = result;
}
