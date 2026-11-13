//! A one-line text field (filter, Quick Open, names in the tree, search), with IME
//! support so Vietnamese and other composed input works.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, Render, ShapedLine, SharedString, Style, TextRun,
    UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, prelude::*, px, relative,
    size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme;

actions!(
    text_field,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        Left,
        Right,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectToHome,
        SelectToEnd,
        Paste,
        Cut,
        Copy,
        Submit,
        Cancel,
    ]
);

pub enum TextFieldEvent {
    Changed,
    Submitted,
    Cancelled,
    Blurred,
}

pub struct TextField {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// How far the text is scrolled sideways to keep the cursor in view.
    scroll_x: f32,
    selecting: bool,
    _blur: Option<gpui::Subscription>,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> TextField {
        let focus_handle = cx.focus_handle();
        let blur = cx.on_blur(&focus_handle, window, |_, _, cx| cx.emit(TextFieldEvent::Blurred));
        TextField {
            focus_handle,
            content: String::new(),
            placeholder: placeholder.into(),
            selected: 0..0,
            reversed: false,
            marked: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: 0.,
            selecting: false,
            _blur: Some(blur),
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces the text without reporting a change.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        cx.notify();
    }

    /// Selects a byte range, e.g. a file's name without its extension.
    pub fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let end = range.end.min(self.content.len());
        self.selected = range.start.min(end)..end;
        self.reversed = false;
        cx.notify();
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected = 0..self.content.len();
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn cursor(&self) -> usize {
        if self.reversed { self.selected.start } else { self.selected.end }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).rev().find_map(|(i, _)| (i < offset).then_some(i)).unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).find_map(|(i, _)| (i > offset).then_some(i)).unwrap_or(self.content.len())
    }

    fn previous_word(&self, offset: usize) -> usize {
        let before = &self.content[..offset];
        let trimmed = before.trim_end();
        trimmed
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
            .map_or(0, |(i, c)| if trimmed.len() == i + c.len_utf8() { i } else { i + c.len_utf8() })
    }

    fn next_word(&self, offset: usize) -> usize {
        let after = &self.content[offset..];
        let skipped = after.len() - after.trim_start().len();
        let rest = &after[skipped..];
        let word = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
        offset + skipped + word.max(rest.chars().next().map_or(0, char::len_utf8).min(rest.len()))
    }

    fn replace(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        // One line: line breaks from a paste become spaces.
        let text = text.replace(['\n', '\r'], " ");
        self.content.replace_range(range.clone(), &text);
        let end = range.start + text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        cx.emit(TextFieldEvent::Changed);
        cx.notify();
    }

    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        line.closest_index_for_x(position.x - bounds.left() + px(self.scroll_x))
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for c in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += c.len_utf16();
            utf8 += c.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset.min(self.content.len())].encode_utf16().count()
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.selecting = true;
        let index = self.index_for_position(event.position);
        if event.click_count >= 2 {
            self.select_all(cx);
        } else if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_position(event.position), cx);
        }
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selected), reversed: self.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.replace(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.content.replace_range(range.clone(), new_text);
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected = new_selected_range_utf16
            .as_ref()
            .map(|r| {
                let units: Vec<u16> = new_text.encode_utf16().collect();
                let to_byte = |u: usize| String::from_utf16_lossy(&units[..u.min(units.len())]).len();
                range.start + to_byte(r.start)..range.start + to_byte(r.end)
            })
            .unwrap_or(range.start + new_text.len()..range.start + new_text.len());
        cx.emit(TextFieldEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let left = bounds.left() - px(self.scroll_x);
        Some(Bounds::from_corners(
            point(left + layout.x_for_index(range.start), bounds.top()),
            point(left + layout.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let index = self.index_for_position(point);
        Some(self.offset_to_utf16(index))
    }
}

struct FieldElement {
    field: Entity<TextField>,
}

struct FieldPrepaint {
    line: ShapedLine,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: f32,
}

impl IntoElement for FieldElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FieldElement {
    type RequestLayoutState = ();
    type PrepaintState = FieldPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> FieldPrepaint {
        let colors = theme::colors(cx);
        let field = self.field.read(cx);
        let style = window.text_style();
        let focused = field.focus_handle.is_focused(window);
        let (text, color): (SharedString, Hsla) = if field.content.is_empty() {
            (field.placeholder.clone(), colors.tertiary)
        } else {
            (field.content.clone().into(), style.color)
        };
        let base = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match field.marked.clone().filter(|_| !field.content.is_empty()) {
            Some(marked) => vec![
                TextRun { len: marked.start, ..base.clone() },
                TextRun {
                    len: marked.len(),
                    underline: Some(UnderlineStyle { color: Some(color), thickness: px(1.), wavy: false }),
                    ..base.clone()
                },
                TextRun { len: text.len() - marked.end, ..base },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            None => vec![base],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window.text_system().shape_line(text, font_size, &runs, None);
        let cursor_offset = if field.content.is_empty() { 0 } else { field.cursor() };
        let cursor_x = f32::from(line.x_for_index(cursor_offset));
        // Keep the cursor in view in a field too narrow for its text.
        let width = f32::from(bounds.size.width);
        let mut scroll_x = field.scroll_x;
        if cursor_x - scroll_x > width - 2. {
            scroll_x = cursor_x - width + 2.;
        } else if cursor_x < scroll_x {
            scroll_x = cursor_x;
        }
        scroll_x = scroll_x.clamp(0., (f32::from(line.width) - width + 2.).max(0.));
        let left = bounds.left() - px(scroll_x);
        let selection = (!field.selected.is_empty() && !field.content.is_empty()).then(|| {
            fill(
                Bounds::from_corners(
                    point(left + line.x_for_index(field.selected.start), bounds.top()),
                    point(left + line.x_for_index(field.selected.end), bounds.bottom()),
                ),
                colors.text_selection,
            )
        });
        let cursor = (focused && field.selected.is_empty()).then(|| {
            fill(
                Bounds::new(point(left + px(cursor_x), bounds.top()), size(px(1.5), bounds.size.height)),
                colors.accent,
            )
        });
        FieldPrepaint { line, cursor, selection, scroll_x }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut FieldPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.field.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.field.clone()), cx);
        let scroll_x = prepaint.scroll_x;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            let _ = prepaint.line.paint(
                point(bounds.left() - px(scroll_x), bounds.top()),
                window.line_height(),
                window,
                cx,
            );
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        let line = prepaint.line.clone();
        self.field.update(cx, |field, _| {
            field.last_layout = Some(line);
            field.last_bounds = Some(bounds);
            field.scroll_x = scroll_x;
        });
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .key_context("TextField")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|this, _: &Backspace, _, cx| {
                if this.selected.is_empty() {
                    let start = this.previous_boundary(this.cursor());
                    this.select_to(start, cx);
                }
                let range = this.selected.clone();
                this.replace(range, "", cx);
            }))
            .on_action(cx.listener(|this, _: &Delete, _, cx| {
                if this.selected.is_empty() {
                    let end = this.next_boundary(this.cursor());
                    this.select_to(end, cx);
                }
                let range = this.selected.clone();
                this.replace(range, "", cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteWordLeft, _, cx| {
                if this.selected.is_empty() {
                    let start = this.previous_word(this.cursor());
                    this.select_to(start, cx);
                }
                let range = this.selected.clone();
                this.replace(range, "", cx);
            }))
            .on_action(cx.listener(|this, _: &Left, _, cx| {
                let target = if this.selected.is_empty() { this.previous_boundary(this.cursor()) } else { this.selected.start };
                this.move_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &Right, _, cx| {
                let target = if this.selected.is_empty() { this.next_boundary(this.cursor()) } else { this.selected.end };
                this.move_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| {
                let target = this.previous_word(this.cursor());
                this.move_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| {
                let target = this.next_word(this.cursor());
                this.move_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| {
                let target = this.previous_boundary(this.cursor());
                this.select_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
                let target = this.next_boundary(this.cursor());
                this.select_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| {
                let target = this.previous_word(this.cursor());
                this.select_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| {
                let target = this.next_word(this.cursor());
                this.select_to(target, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &Home, _, cx| this.move_to(0, cx)))
            .on_action(cx.listener(|this, _: &End, _, cx| {
                let end = this.content.len();
                this.move_to(end, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectToHome, _, cx| this.select_to(0, cx)))
            .on_action(cx.listener(|this, _: &SelectToEnd, _, cx| {
                let end = this.content.len();
                this.select_to(end, cx);
            }))
            .on_action(cx.listener(|this, _: &Copy, _, cx| {
                if !this.selected.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(this.content[this.selected.clone()].to_string()));
                }
            }))
            .on_action(cx.listener(|this, _: &Cut, _, cx| {
                if !this.selected.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(this.content[this.selected.clone()].to_string()));
                    let range = this.selected.clone();
                    this.replace(range, "", cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Paste, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let range = this.selected.clone();
                    this.replace(range, &text, cx);
                }
            }))
            .on_action(cx.listener(|_, _: &Submit, _, cx| cx.emit(TextFieldEvent::Submitted)))
            .on_action(cx.listener(|_, _: &Cancel, _, cx| cx.emit(TextFieldEvent::Cancelled)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(FieldElement { field: cx.entity() })
    }
}

pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding;
    let context = Some("TextField");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("ctrl-left", WordLeft, context),
        KeyBinding::new("ctrl-right", WordRight, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("shift-home", SelectToHome, context),
        KeyBinding::new("shift-end", SelectToEnd, context),
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("enter", Submit, context),
        KeyBinding::new("escape", Cancel, context),
    ]);
}
