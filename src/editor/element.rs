//! Lays out and draws the editor: the visible rows with their syntax colors, the
//! line-number gutter, indent guides, selection, cursor and minimap.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    LineFragment, Pixels, ShapedLine, Style, TextRun, UnderlineStyle, Window, fill, point, px,
    relative, size,
};

use super::display::DisplayLine;
use super::wrap::{RowStarts, WrapKey};
use super::{Autoscroll, Editor};
use crate::settings::Settings;
use crate::syntax::Span;
use crate::text;
use crate::theme::{self, Colors, alpha, hex};

/// Space above the first line and below the last, as NSTextView's container inset.
const TOP_INSET: f32 = 12.;
/// From the gutter (or the card's edge) to the text: inset plus line fragment padding.
const TEXT_INSET: f32 = 13.;
const GUTTER_PADDING: f32 = 6.;
/// Width of the minimap strip, including its padding.
pub const MINIMAP_WIDTH: f32 = 100.;
const MINIMAP_ROW_HEIGHT: f32 = 2.;
const MINIMAP_BLOCK_HEIGHT: f32 = 1.5;
const MINIMAP_COLUMN_WIDTH: f32 = 1.;
const MINIMAP_PADDING: f32 = 6.;
const MINIMAP_TEXT_OPACITY: f32 = 0.6;
/// Lines longer than this are drawn only around the visible columns when not wrapping.
const LONG_LINE: usize = 4096;

pub struct Metrics {
    pub line_height: f32,
    pub top_inset: f32,
    pub char_width: f32,
    pub gutter_width: f32,
    pub minimap_width: f32,
    pub visible_height: f32,
    pub font_size: f32,
}

pub struct RowLayout {
    pub line: usize,
    pub row_in_line: usize,
    /// Document byte range of the row.
    pub range: Range<usize>,
    /// Top of the row, in window coordinates.
    pub y: f32,
    /// Where the row's first character is drawn, in window coordinates.
    pub x: f32,
    pub shaped: ShapedLine,
    pub display: Rc<DisplayLine>,
    /// The row's span in the display text, which `shaped` holds.
    pub display_range: Range<usize>,
}

impl RowLayout {
    /// The window x of a document offset in this row.
    pub fn x_for(&self, offset: usize, line_start: usize) -> f32 {
        let display = self.display.to_display(offset - line_start);
        self.x + f32::from(self.shaped.x_for_index(display.saturating_sub(self.display_range.start)))
    }
}

pub struct MinimapLayout {
    pub bounds: Bounds<Pixels>,
    /// How far the minimap's rows are scrolled up.
    pub offset: f32,
    pub slider: Bounds<Pixels>,
    /// How far the slider can travel, for turning drags into editor scrolling.
    pub track: f32,
    pub line_count: usize,
}

pub struct LayoutSnapshot {
    pub bounds: Bounds<Pixels>,
    pub metrics: Metrics,
    /// Where column 0 is drawn, in window coordinates (after sideways scrolling).
    pub text_x: f32,
    pub text_bounds: Bounds<Pixels>,
    pub rows: Vec<RowLayout>,
    pub minimap: Option<MinimapLayout>,
    pub max_scroll: (f32, f32),
}

pub struct EditorElement {
    pub editor: Entity<Editor>,
}

pub struct Prepaint {
    snapshot: Rc<LayoutSnapshot>,
    text_hitbox: Hitbox,
    minimap_hitbox: Option<Hitbox>,
    paint: PaintList,
}

#[derive(Default)]
struct PaintList {
    /// Selection and find highlights, under the text.
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    guides: Vec<Bounds<Pixels>>,
    numbers: Vec<(ShapedLine, gpui::Point<Pixels>)>,
    cursor: Option<Bounds<Pixels>>,
    minimap: Vec<(Bounds<Pixels>, Hsla)>,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

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
        style.size.height = relative(1.).into();
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
    ) -> Prepaint {
        let settings = Settings::get(cx).clone();
        let colors = theme::colors(cx);
        let focused = self.editor.read(cx).focus_handle.is_focused(window);
        let (snapshot, paint) = self.editor.update(cx, |editor, cx| {
            editor.layout_rows(bounds, &settings, colors, focused, window, cx)
        });
        let snapshot = Rc::new(snapshot);
        let text_hitbox = window.insert_hitbox(snapshot.text_bounds, HitboxBehavior::Normal);
        let minimap_hitbox =
            snapshot.minimap.as_ref().map(|m| window.insert_hitbox(m.bounds, HitboxBehavior::Normal));
        self.editor.update(cx, |editor, _| editor.layout = Some(snapshot.clone()));
        Prepaint { snapshot, text_hitbox, minimap_hitbox, paint }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);
        if let Some(hitbox) = &prepaint.minimap_hitbox {
            window.set_cursor_style(CursorStyle::Arrow, hitbox);
        }
        let colors = theme::colors(cx);
        let snapshot = prepaint.snapshot.clone();
        let line_height = px(snapshot.metrics.line_height);
        let paint = std::mem::take(&mut prepaint.paint);

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // Text scrolled sideways disappears under the gutter.
            window.with_content_mask(Some(ContentMask { bounds: snapshot.text_bounds }), |window| {
                for (rect, color) in &paint.backgrounds {
                    window.paint_quad(fill(*rect, *color));
                }
                for guide in &paint.guides {
                    window.paint_quad(fill(*guide, colors.indent_guide));
                }
                for row in &snapshot.rows {
                    let _ = row.shaped.paint(point(px(row.x), px(row.y)), line_height, window, cx);
                }
                if let Some(cursor) = paint.cursor {
                    window.paint_quad(fill(cursor, colors.cursor));
                }
            });
            for (number, origin) in &paint.numbers {
                let _ = number.paint(*origin, line_height, window, cx);
            }
            for (rect, color) in &paint.minimap {
                window.paint_quad(fill(*rect, *color));
            }
        });
    }
}

impl Editor {
    /// Measures and positions everything for a frame. Also applies a pending
    /// autoscroll and keeps the scroll position in range.
    fn layout_rows(
        &mut self,
        bounds: Bounds<Pixels>,
        settings: &Settings,
        colors: &'static Colors,
        focused: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> (LayoutSnapshot, PaintList) {
        let font = Self::font(cx);
        let font_size = settings.font_size;
        let line_height = (font_size * 1.38).round();
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let char_width = text_system
            .advance(font_id, px(font_size), 'm')
            .map(|s| f32::from(s.width))
            .unwrap_or(font_size * 0.6);
        let number_size = (font_size - 2.).max(8.);
        let number_font = Self::font(cx);
        let digit_width = text_system
            .advance(text_system.resolve_font(&number_font), px(number_size), '8')
            .map(|s| f32::from(s.width))
            .unwrap_or(number_size * 0.6);
        let line_count = self.line_count();
        let gutter_width = if settings.shows_line_numbers {
            let digits = line_count.to_string().len().max(3);
            (2. * GUTTER_PADDING + digits as f32 * digit_width).ceil()
        } else {
            0.
        };
        let minimap_width = if settings.shows_minimap { MINIMAP_WIDTH } else { 0. };
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let text_area_width = (width - gutter_width - minimap_width).max(0.);
        let tab_width = settings.tab_width;

        // Rows for every line, measured once and kept until the line or width changes.
        let wrap_width = (text_area_width - 2. * TEXT_INSET).floor().max(char_width * 4.);
        let key = settings.wraps_lines.then(|| WrapKey {
            width: wrap_width,
            font_size,
            family: settings.editor_font_family(),
            tab_width,
        });
        if self.wraps.line_count() != line_count {
            self.wraps.reset(line_count);
        }
        self.wraps.set_key(key);
        if self.wraps.has_missing() {
            let missing: Vec<usize> = self.wraps.missing().collect();
            if self.wraps.is_wrapping() {
                let mut wrapper = text_system.line_wrapper(font.clone(), px(font_size));
                for line in missing {
                    let source = self.line_text(line);
                    let mut starts = RowStarts::new();
                    if source.len() as f32 * char_width * 0.5 > wrap_width || source.contains('\t') {
                        let display = DisplayLine::new(&source, tab_width);
                        for boundary in
                            wrapper.wrap_line(&[LineFragment::text(&display.text)], px(wrap_width))
                        {
                            starts.push(display.to_source(boundary.ix) as u32);
                        }
                    }
                    self.wraps.set(line, starts);
                }
            } else {
                for line in missing {
                    self.wraps.set(line, RowStarts::new());
                }
            }
        }
        let row_count = self.wraps.row_count();

        // Scrolling: how far it can go, then the requested autoscroll, then clamping.
        let content_height = TOP_INSET * 2. + row_count as f32 * line_height;
        let max_y = (content_height - height).max(0.);
        let max_x = if settings.wraps_lines {
            0.
        } else {
            (self.longest_line_columns(tab_width) as f32 * char_width + 2. * TEXT_INSET
                - text_area_width)
                .max(0.)
        };
        if let Some(autoscroll) = self.autoscroll.take() {
            let cursor = self.cursor();
            let (x, row) = self.x_and_row_for_offset(cursor, window, cx);
            let row_top = TOP_INSET + row as f32 * line_height;
            match autoscroll {
                Autoscroll::Fit => {
                    let margin = line_height;
                    if row_top - margin < self.scroll.y {
                        self.scroll.y = row_top - margin;
                    } else if row_top + line_height + margin > self.scroll.y + height {
                        self.scroll.y = row_top + line_height + margin - height;
                    }
                }
                Autoscroll::Center => {
                    self.scroll.y = row_top + line_height / 2. - height / 2.;
                }
                Autoscroll::Top(line) => {
                    self.scroll.y = TOP_INSET + self.wraps.first_row(line) as f32 * line_height - TOP_INSET;
                }
            }
            if !settings.wraps_lines && !matches!(autoscroll, Autoscroll::Top(_)) {
                let visible = text_area_width - 2. * TEXT_INSET;
                if x < self.scroll.x {
                    self.scroll.x = (x - char_width * 4.).max(0.);
                } else if x > self.scroll.x + visible - char_width {
                    self.scroll.x = x - visible + char_width * 4.;
                }
            }
        }
        self.scroll.y = self.scroll.y.clamp(0., max_y);
        self.scroll.x = self.scroll.x.clamp(0., max_x);
        if settings.wraps_lines {
            self.scroll.x = 0.;
        }

        let top = f32::from(bounds.origin.y) + TOP_INSET - self.scroll.y;
        let text_left = f32::from(bounds.origin.x) + gutter_width;
        let text_x = text_left + TEXT_INSET - self.scroll.x;
        let text_bounds = Bounds::new(
            point(px(text_left), bounds.origin.y),
            size(px(text_area_width), bounds.size.height),
        );

        // The rows on screen.
        let first_row = (((self.scroll.y - TOP_INSET) / line_height).floor().max(0.)) as usize;
        let last_row = ((((self.scroll.y + height - TOP_INSET) / line_height).ceil()) as usize)
            .min(row_count.saturating_sub(1));
        let dark = colors.dark;
        let syntax = settings.syntax_theme;
        let font_px = px(font_size);
        let mut displays: HashMap<usize, Rc<DisplayLine>> = HashMap::new();
        let mut rows = Vec::new();
        if row_count > 0 {
            for row in first_row..=last_row.max(first_row).min(row_count - 1) {
                let (line, row_in_line) = self.wraps.line_for_row(row);
                let line_range = self.line_range(line);
                let display = displays
                    .entry(line)
                    .or_insert_with(|| {
                        Rc::new(DisplayLine::new(&self.slice(line_range.clone()), tab_width))
                    })
                    .clone();
                let in_line = self.wraps.row_range(line, row_in_line, line_range.len());
                let mut display_range =
                    display.to_display(in_line.start)..display.to_display(in_line.end);
                let mut x = text_x;
                // A very long line, not wrapped: only the part around the visible columns.
                if !settings.wraps_lines && display_range.len() > LONG_LINE {
                    let first_column = (self.scroll.x / char_width) as usize;
                    let start = floor_char_boundary(&display.text, first_column.saturating_sub(64));
                    let end = floor_char_boundary(
                        &display.text,
                        (start + (text_area_width / char_width) as usize * 4 + 256).min(display.text.len()),
                    );
                    x += start as f32 * char_width;
                    display_range = start..end;
                }
                let doc_range = line_range.start + display.to_source(display_range.start)
                    ..line_range.start + display.to_source(display_range.end);
                let runs = self.text_runs(
                    &display,
                    line_range.start,
                    display_range.clone(),
                    &font,
                    colors.label,
                    syntax,
                    dark,
                );
                let text: String = display.text[display_range.clone()].to_string();
                let shaped = text_system.shape_line(text.into(), font_px, &runs, None);
                rows.push(RowLayout {
                    line,
                    row_in_line,
                    range: doc_range,
                    y: top + row as f32 * line_height,
                    x,
                    shaped,
                    display,
                    display_range,
                });
            }
        }

        let mut paint = PaintList::default();
        let selection = self.selection();
        let selection_color = if focused { colors.text_selection } else { colors.text_selection_unfocused };

        // Find matches and the selection, row by row.
        for row in &rows {
            let line_start = self.line_start(row.line);
            let row_end_x = row.x + f32::from(row.shaped.width);
            let mut highlight = |range: &Range<usize>, color: Hsla, through_break: bool| {
                if range.end < row.range.start || range.start > row.range.end || range.is_empty() {
                    return;
                }
                if range.start == row.range.end && !through_break {
                    return;
                }
                let start = range.start.max(row.range.start);
                let end = range.end.min(row.range.end);
                let x0 = row.x_for(start, line_start);
                let mut x1 = row.x_for(end, line_start);
                // A selection running on past the row's end covers the line break too.
                if through_break && range.end > row.range.end {
                    x1 = x1.max(row_end_x) + char_width * 0.5;
                }
                if x1 > x0 {
                    paint.backgrounds.push((
                        Bounds::new(point(px(x0), px(row.y)), size(px(x1 - x0), px(line_height))),
                        color,
                    ));
                }
            };
            for (index, found) in self.find_matches.iter().enumerate() {
                let color = if Some(index) == self.find_current { colors.find_current } else { colors.find_match };
                highlight(found, color, false);
            }
            if !selection.is_empty() {
                highlight(&selection, selection_color, true);
            }
        }

        // The cursor, when there's no selection.
        if focused && self.cursor_on && selection.is_empty() {
            let cursor = self.cursor();
            let cursor_line = self.line_of(cursor);
            let line_start = self.line_start(cursor_line);
            let in_line = cursor - line_start;
            let row_in_line = self.wraps.row_in_line(cursor_line, in_line);
            let row_index = self.wraps.first_row(cursor_line) + row_in_line;
            if row_index >= first_row && row_index <= last_row {
                if let Some(row) = rows.get(row_index - first_row) {
                    let x = row.x_for(cursor, line_start);
                    paint.cursor = Some(Bounds::new(point(px(x.floor()), px(row.y + 1.)), size(px(2.), px(line_height - 2.))));
                }
            }
        }

        // Indent guides: per line on screen, through all its rows.
        if settings.shows_indent_guides && !rows.is_empty() {
            let unit = self.indentation(cx).width.max(1);
            let mut previous_indent: Option<usize> = None;
            let mut index = 0;
            while index < rows.len() {
                let line = rows[index].line;
                let mut last = index;
                while last + 1 < rows.len() && rows[last + 1].line == line {
                    last += 1;
                }
                let line_text = self.line_text(line);
                let indent = match text::indent_columns(&line_text, tab_width) {
                    Some(own) => {
                        previous_indent = Some(own);
                        own
                    }
                    None => text::blank_line_indent(
                        previous_indent.or_else(|| self.indent_of_non_blank_line(line, false, tab_width)),
                        self.indent_of_non_blank_line(line, true, tab_width),
                        unit,
                    ),
                };
                let guides = indent.div_ceil(unit);
                let y0 = rows[index].y;
                let y1 = rows[last].y + line_height;
                for level in 0..guides {
                    let x = (text_x + (level * unit) as f32 * char_width).round();
                    paint.guides.push(Bounds::new(point(px(x), px(y0)), size(px(1.), px(y1 - y0))));
                }
                index = last + 1;
            }
        }

        // Line numbers, right-aligned in the gutter, on the baseline of their line.
        if settings.shows_line_numbers {
            let current_line = self.line_of(self.cursor());
            let number_font_id = text_system.resolve_font(&number_font);
            let number_metrics = text_system.ascent(number_font_id, px(number_size));
            let text_ascent = f32::from(text_system.ascent(font_id, px(font_size)));
            let text_descent = f32::from(text_system.descent(font_id, px(font_size)));
            let number_descent = text_system.descent(number_font_id, px(number_size));
            for row in &rows {
                // Wrapped continuations don't start a line.
                if row.row_in_line != 0 {
                    continue;
                }
                let label = (row.line + 1).to_string();
                let color = if row.line == current_line { colors.current_line_number } else { colors.line_number };
                let run = TextRun {
                    len: label.len(),
                    font: number_font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = text_system.shape_line(label.into(), px(number_size), &[run], None);
                let ascent = text_ascent;
                let descent = text_descent;
                let baseline = row.y + (line_height - ascent - descent) / 2. + ascent;
                let number_ascent = f32::from(number_metrics);
                let number_height = number_ascent + f32::from(number_descent);
                let y = baseline - (line_height - number_height) / 2. - number_ascent;
                let x = f32::from(bounds.origin.x) + gutter_width - GUTTER_PADDING - f32::from(shaped.width);
                paint.numbers.push((shaped, point(px(x), px(y))));
            }
        }

        let minimap = (minimap_width > 0.).then(|| {
            self.layout_minimap(bounds, minimap_width, first_row, last_row, row_count, max_y, colors, settings, &mut paint)
        });

        let snapshot = LayoutSnapshot {
            bounds,
            metrics: Metrics {
                line_height,
                top_inset: TOP_INSET,
                char_width,
                gutter_width,
                minimap_width,
                visible_height: height,
                font_size,
            },
            text_x,
            text_bounds,
            rows,
            minimap,
            max_scroll: (max_x, max_y),
        };
        (snapshot, paint)
    }

    /// Colored runs for a row's display text, from the highlight spans.
    #[allow(clippy::too_many_arguments)]
    fn text_runs(
        &self,
        display: &DisplayLine,
        line_start: usize,
        display_range: Range<usize>,
        font: &gpui::Font,
        plain: Hsla,
        syntax: crate::syntax::SyntaxTheme,
        dark: bool,
    ) -> Vec<TextRun> {
        let source_start = line_start + display.to_source(display_range.start);
        let source_end = line_start + display.to_source(display_range.end);
        let run = |len: usize, color: Hsla, underline: bool| TextRun {
            len,
            font: font.clone(),
            color,
            background_color: None,
            underline: underline.then_some(UnderlineStyle { color: Some(color), thickness: px(1.), wavy: false }),
            strikethrough: None,
        };
        // Boundaries in source offsets with a color for what follows.
        let mut pieces: Vec<(usize, Hsla)> = Vec::new();
        let spans: &[Span] = &self.spans;
        let first = spans.partition_point(|s| s.range.end <= source_start);
        let mut at = source_start;
        for span in &spans[first..] {
            if span.range.start >= source_end {
                break;
            }
            let start = span.range.start.max(source_start);
            let end = span.range.end.min(source_end);
            if start > at {
                pieces.push((at, plain));
            }
            pieces.push((start, hex(syntax.color(span.kind, dark))));
            at = end;
        }
        if at < source_end || pieces.is_empty() {
            pieces.push((at, plain));
        }
        let mut runs = Vec::with_capacity(pieces.len());
        for (index, (start, color)) in pieces.iter().enumerate() {
            let end = pieces.get(index + 1).map_or(source_end, |p| p.0);
            let d0 = display.to_display(start - line_start).max(display_range.start);
            let d1 = display.to_display(end - line_start).min(display_range.end);
            if d1 <= d0 {
                continue;
            }
            // The IME's composition is underlined.
            if let Some(marked) = self.marked.clone() {
                let m0 = display.to_display(marked.start.saturating_sub(line_start)).clamp(d0, d1);
                let m1 = display.to_display(marked.end.saturating_sub(line_start)).clamp(d0, d1);
                if marked.start >= line_start && m1 > m0 {
                    if m0 > d0 {
                        runs.push(run(m0 - d0, *color, false));
                    }
                    runs.push(run(m1 - m0, *color, true));
                    if d1 > m1 {
                        runs.push(run(d1 - m1, *color, false));
                    }
                    continue;
                }
            }
            runs.push(run(d1 - d0, *color, false));
        }
        if runs.is_empty() {
            runs.push(run(0, plain, false));
        }
        runs
    }

    /// Indent of the nearest non-blank line above or below `line`.
    fn indent_of_non_blank_line(&self, line: usize, below: bool, tab_width: usize) -> Option<usize> {
        let count = self.line_count();
        for step in 1..1000 {
            let other = if below { line.checked_add(step)? } else { line.checked_sub(step)? };
            if other >= count {
                return None;
            }
            let text = self.line_text(other);
            if let Some(indent) = text::indent_columns(&text, tab_width) {
                return Some(indent);
            }
        }
        None
    }

    /// The widest line in columns, for how far the text scrolls sideways.
    fn longest_line_columns(&mut self, tab_width: usize) -> usize {
        if self.longest_line.0 == self.version && self.longest_line.2 == tab_width {
            return self.longest_line.1;
        }
        let mut longest = 0;
        for line in self.rope.lines() {
            let mut column = 0;
            for c in line.chars() {
                match c {
                    '\n' | '\r' => break,
                    '\t' => column += tab_width - column % tab_width,
                    _ => column += 1,
                }
            }
            longest = longest.max(column);
        }
        self.longest_line = (self.version, longest, tab_width);
        longest
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_minimap(
        &mut self,
        bounds: Bounds<Pixels>,
        width: f32,
        first_row: usize,
        last_row: usize,
        row_count: usize,
        max_scroll_y: f32,
        colors: &Colors,
        settings: &Settings,
        paint: &mut PaintList,
    ) -> MinimapLayout {
        let left = f32::from(bounds.origin.x) + f32::from(bounds.size.width) - width;
        let top = f32::from(bounds.origin.y);
        let height = f32::from(bounds.size.height);
        let minimap_bounds = Bounds::new(point(px(left), bounds.origin.y), size(px(width), bounds.size.height));
        let line_count = self.line_count();
        let content_height = line_count as f32 * MINIMAP_ROW_HEIGHT;
        let first_line = if row_count > 0 { self.wraps.line_for_row(first_row).0 } else { 0 };
        let last_line = if row_count > 0 { self.wraps.line_for_row(last_row).0 } else { 0 };
        let slider_top = first_line as f32 * MINIMAP_ROW_HEIGHT;
        let slider_height = ((last_line - first_line + 1) as f32 * MINIMAP_ROW_HEIGHT).max(8.);
        // Scroll the rows by the editor's scroll fraction, but keep the slider in view.
        let fraction = if max_scroll_y > 0. { (self.scroll.y / max_scroll_y).clamp(0., 1.) } else { 0. };
        let max_offset = (content_height - height).max(0.);
        let offset = (slider_top - fraction * (height - slider_height)).clamp(0., max_offset);
        let slider = Bounds::new(
            point(px(left), px(top + slider_top - offset)),
            size(px(width), px(slider_height)),
        );
        let track = (content_height.min(height) - slider_height).max(0.);

        // The selection's rows get a faint accent wash; the cursor's row a stronger one.
        let selection = self.selection();
        let cursor_line = self.line_of(selection.start);
        if !selection.is_empty() {
            let last = self.line_of(selection.end.saturating_sub(1).max(selection.start));
            paint.minimap.push((
                Bounds::new(
                    point(px(left), px(top + cursor_line as f32 * MINIMAP_ROW_HEIGHT - offset)),
                    size(px(width), px((last - cursor_line + 1) as f32 * MINIMAP_ROW_HEIGHT)),
                ),
                alpha(colors.accent, 0.25),
            ));
        } else {
            paint.minimap.push((
                Bounds::new(
                    point(px(left), px(top + cursor_line as f32 * MINIMAP_ROW_HEIGHT - offset)),
                    size(px(width), px(MINIMAP_ROW_HEIGHT)),
                ),
                alpha(colors.accent, 0.6),
            ));
        }

        // One block per non-blank character, merged into runs of the same color.
        let max_columns = ((width - 2. * MINIMAP_PADDING) / MINIMAP_COLUMN_WIDTH) as usize;
        let first = (offset / MINIMAP_ROW_HEIGHT) as usize;
        let last = (((offset + height) / MINIMAP_ROW_HEIGHT) as usize).min(line_count.saturating_sub(1));
        let dark = colors.dark;
        let syntax = settings.syntax_theme;
        let tab_width = settings.tab_width;
        for line in first..=last.max(first) {
            if line >= line_count {
                break;
            }
            let range = self.line_range(line);
            let y = top + line as f32 * MINIMAP_ROW_HEIGHT - offset + (MINIMAP_ROW_HEIGHT - MINIMAP_BLOCK_HEIGHT) / 2.;
            let text = self.slice(range.start..(range.start + max_columns * 4).min(range.end));
            let mut column = 0;
            let mut span_start: Option<(usize, Hsla)> = None;
            let mut span_index = self.spans.partition_point(|s| s.range.end <= range.start);
            let push = |paint: &mut PaintList, from: usize, to: usize, color: Hsla| {
                paint.minimap.push((
                    Bounds::new(
                        point(px(left + MINIMAP_PADDING + from as f32 * MINIMAP_COLUMN_WIDTH), px(y)),
                        size(px((to - from) as f32 * MINIMAP_COLUMN_WIDTH), px(MINIMAP_BLOCK_HEIGHT)),
                    ),
                    alpha(color, MINIMAP_TEXT_OPACITY),
                ));
            };
            for (i, c) in text.char_indices() {
                if column >= max_columns {
                    break;
                }
                if c == ' ' || c == '\t' {
                    if let Some((start, color)) = span_start.take() {
                        push(paint, start, column, color);
                    }
                    column = if c == '\t' { column + tab_width - column % tab_width } else { column + 1 };
                    continue;
                }
                let at = range.start + i;
                while span_index < self.spans.len() && self.spans[span_index].range.end <= at {
                    span_index += 1;
                }
                let color = match self.spans.get(span_index) {
                    Some(span) if span.range.start <= at => hex(syntax.color(span.kind, dark)),
                    _ => colors.label,
                };
                match span_start {
                    Some((start, current)) if current != color => {
                        push(paint, start, column, current);
                        span_start = Some((column, color));
                    }
                    None => span_start = Some((column, color)),
                    _ => {}
                }
                column += 1;
            }
            if let Some((start, color)) = span_start {
                push(paint, start, column.min(max_columns), color);
            }
        }

        let opacity = if self.minimap_drag.is_some() {
            0.16
        } else if self.minimap_hovered {
            0.11
        } else {
            0.07
        };
        paint.minimap.push((slider, alpha(colors.label, opacity)));
        // A hairline down the leading edge, setting the minimap apart from the text.
        paint.minimap.push((
            Bounds::new(point(px(left), bounds.origin.y), size(px(1.), bounds.size.height)),
            colors.indent_guide,
        ));

        MinimapLayout { bounds: minimap_bounds, offset, slider, track, line_count }
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
