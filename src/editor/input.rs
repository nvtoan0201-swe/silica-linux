//! Typing (including IME composition, such as Vietnamese Telex), mouse selection,
//! scrolling and the minimap's clicks and drags.

use std::ops::Range;

use gpui::{
    Bounds, Context, EntityInputHandler, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    ScrollWheelEvent, Styled, UTF16Selection, Window, div, point, px, size,
};

use super::element::EditorElement;
use super::history::EditKind;
use super::{Autoscroll, Editor, SelectMode};

impl Editor {
    fn utf16_to_byte(&self, offset: usize) -> usize {
        let offset = offset.min(self.rope.len_utf16_cu());
        self.rope.char_to_byte(self.rope.utf16_cu_to_char(offset))
    }

    fn byte_to_utf16(&self, offset: usize) -> usize {
        self.rope.char_to_utf16_cu(self.rope.byte_to_char(offset.min(self.len())))
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_to_byte(range.start)..self.utf16_to_byte(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.byte_to_utf16(range.start)..self.byte_to_utf16(range.end)
    }

    /// The document offset under a point in the window.
    fn offset_for_position(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> usize {
        let Some(layout) = self.layout.clone() else { return self.cursor() };
        let metrics = &layout.metrics;
        let y = f32::from(position.y - layout.bounds.origin.y) + self.scroll.y - metrics.top_inset;
        let row_count = self.wraps.row_count();
        if row_count == 0 {
            return 0;
        }
        if y >= row_count as f32 * metrics.line_height {
            return self.len();
        }
        let row = (y / metrics.line_height).floor().max(0.) as usize;
        let x = f32::from(position.x) - layout.text_x;
        self.offset_for_row_x(row, x, window, cx)
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let Some(layout) = self.layout.clone() else { return };
        if let Some(minimap) = &layout.minimap {
            if minimap.bounds.contains(&event.position) {
                let y = f32::from(event.position.y);
                // Off the slider, jump there first, then let the same press drag from the new spot.
                if !minimap.slider.contains(&event.position) {
                    let line = ((y - f32::from(minimap.bounds.origin.y) + minimap.offset) / 2.) as usize;
                    let line = line.min(minimap.line_count.saturating_sub(1));
                    let first_row = self.wraps.first_row(line);
                    self.scroll.y = layout.metrics.top_inset + first_row as f32 * layout.metrics.line_height
                        - layout.metrics.visible_height / 2.;
                    self.scroll.y = self.scroll.y.clamp(0., layout.max_scroll.1);
                }
                self.minimap_drag = Some((y, self.scroll.y));
                cx.notify();
                return;
            }
        }
        let offset = self.offset_for_position(event.position, window, cx);
        let mode = match event.click_count {
            2 => SelectMode::Word,
            n if n >= 3 => SelectMode::Line,
            _ => SelectMode::Character,
        };
        let unit = match mode {
            SelectMode::Character => offset..offset,
            SelectMode::Word => self.word_range(offset),
            SelectMode::Line => self.full_line_range(offset),
        };
        if event.modifiers.shift {
            let anchor = if self.reversed { self.selection.end } else { self.selection.start };
            let anchor_range = anchor..anchor;
            self.selecting = Some((mode, anchor_range));
            self.extend_selection_to(offset, cx);
        } else {
            self.selecting = Some((mode, unit.clone()));
            self.select(unit, false);
            self.history.seal();
            self.goal_x = None;
            self.selection_changed(cx);
        }
    }

    /// Grows the selection from where the drag started to the unit under `offset`.
    fn extend_selection_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some((mode, origin)) = self.selecting.clone() else { return };
        let unit = match mode {
            SelectMode::Character => offset..offset,
            SelectMode::Word => self.word_range(offset),
            SelectMode::Line => self.full_line_range(offset),
        };
        if unit.start < origin.start {
            self.select(unit.start..origin.end, true);
        } else {
            self.select(origin.start..unit.end.max(origin.end), false);
        }
        self.goal_x = None;
        self.request_autoscroll(Autoscroll::Fit);
        self.selection_changed(cx);
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.clone() else { return };
        if let Some(minimap) = &layout.minimap {
            let hovered = minimap.bounds.contains(&event.position);
            if hovered != self.minimap_hovered {
                self.minimap_hovered = hovered;
                cx.notify();
            }
            if let Some((start_y, start_scroll)) = self.minimap_drag {
                if event.pressed_button == Some(MouseButton::Left) && minimap.track > 0. {
                    let delta = f32::from(event.position.y) - start_y;
                    self.scroll.y = (start_scroll + delta * layout.max_scroll.1 / minimap.track)
                        .clamp(0., layout.max_scroll.1);
                    cx.notify();
                } else {
                    self.minimap_drag = None;
                }
                return;
            }
        }
        if self.selecting.is_some() {
            if event.pressed_button != Some(MouseButton::Left) {
                self.selecting = None;
                return;
            }
            let offset = self.offset_for_position(event.position, window, cx);
            self.extend_selection_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.selecting = None;
        if self.minimap_drag.take().is_some() {
            cx.notify();
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = self.layout.as_ref().map_or(18., |l| l.metrics.line_height);
        let delta = event.delta.pixel_delta(px(line_height));
        let (max_x, max_y) = self.layout.as_ref().map_or((0., f32::MAX), |l| l.max_scroll);
        let (mut dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        // A plain wheel with Shift held scrolls sideways.
        if event.modifiers.shift && dx == 0. {
            dx = dy;
            self.scroll.x = (self.scroll.x - dx).clamp(0., max_x);
        } else {
            self.scroll.x = (self.scroll.x - dx).clamp(0., max_x);
            self.scroll.y = (self.scroll.y - dy).clamp(0., max_y);
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The cursor blinks only while the editor has focus.
        let focused = self.focus_handle.is_focused(window);
        if focused && self._blink_task.is_none() {
            self.start_blinking(cx);
        } else if !focused && self._blink_task.is_some() {
            self._blink_task = None;
            self.selecting = None;
        }
        let root = div()
            .id("editor")
            .key_context("Editor")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_hidden();
        Self::register_actions(root, cx)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(EditorElement { editor: cx.entity() })
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.slice(range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selection), reversed: self.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        // Return arrives here on some keyboards; it should auto-indent like the key.
        if new_text == "\n" || new_text == "\r" {
            self.select(range, false);
            self.marked = None;
            cx.dispatch_action(&super::Newline);
            return;
        }
        self.transact(EditKind::Typing, cx, |this, cx| {
            this.edit(range.clone(), new_text, cx);
            let end = range.start + new_text.len();
            this.select(end..end, false);
            this.marked = None;
        });
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
            .map(|r| self.range_from_utf16(&r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        self.transact(EditKind::Typing, cx, |this, cx| {
            this.edit(range.clone(), new_text, cx);
            this.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
            let selected = new_selected_range_utf16
                .map(|r| {
                    // Relative to the composed text, in UTF-16.
                    let composed: Vec<u16> = new_text.encode_utf16().collect();
                    let to_byte = |u: usize| {
                        String::from_utf16_lossy(&composed[..u.min(composed.len())]).len()
                    };
                    range.start + to_byte(r.start)..range.start + to_byte(r.end)
                })
                .unwrap_or(range.start + new_text.len()..range.start + new_text.len());
            this.select(selected, false);
        });
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.clone()?;
        let range = self.range_from_utf16(&range_utf16);
        let row = layout
            .rows
            .iter()
            .find(|row| range.start >= row.range.start && range.start <= row.range.end)?;
        let line_start = self.line_start(row.line);
        let x0 = row.x_for(range.start, line_start);
        let x1 = row.x_for(range.end.clamp(row.range.start, row.range.end), line_start);
        Some(Bounds::new(
            point(px(x0), px(row.y)),
            size(px((x1 - x0).max(1.)), px(layout.metrics.line_height)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_for_position(position, window, cx);
        Some(self.byte_to_utf16(offset))
    }
}
