//! Cursor movement and the code-editing commands: auto-indent on Return, Tab to the
//! next indent stop, and line commands (indent, outdent, toggle comment, move and
//! duplicate lines). Each command is a single undo step.

use std::ops::Range;

use gpui::{ClipboardItem, Context, Window, actions};
use unicode_segmentation::UnicodeSegmentation;

use super::history::EditKind;
use super::{Autoscroll, Editor};
use crate::text::{self, CharClass, char_class};

actions!(
    editor,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        MoveToLineStart,
        MoveToLineEnd,
        MoveToStart,
        MoveToEnd,
        PageUp,
        PageDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectToLineStart,
        SelectToLineEnd,
        SelectToStart,
        SelectToEnd,
        SelectPageUp,
        SelectPageDown,
        SelectAll,
        SelectLine,
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        Newline,
        Tab,
        Backtab,
        Undo,
        Redo,
        Copy,
        Cut,
        Paste,
        ToggleComment,
        IndentLines,
        OutdentLines,
        MoveLinesUp,
        MoveLinesDown,
        DuplicateLinesDown,
        DuplicateLinesUp,
        CollapseSelection,
    ]
);

/// A change to one line: remove `remove` bytes at `column` and insert `insert`.
struct LineEdit {
    column: usize,
    remove: usize,
    insert: String,
}

impl Editor {
    // MARK: Boundaries

    fn prev_boundary(&self, offset: usize) -> usize {
        let line = self.line_of(offset);
        let start = self.line_start(line);
        if offset <= start {
            return if line == 0 { 0 } else { self.line_range(line - 1).end };
        }
        let text = self.slice(start..offset);
        start + text.grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let line = self.line_of(offset);
        let content = self.line_range(line);
        if offset >= content.end {
            return if line + 1 < self.line_count() { self.line_start(line + 1) } else { self.len() };
        }
        let text = self.slice(offset..content.end);
        offset + text.graphemes(true).next().map_or(0, str::len)
    }

    /// Back over spaces, then over a word (or a run of punctuation).
    fn prev_word_boundary(&self, offset: usize) -> usize {
        let line = self.line_of(offset);
        let start = self.line_start(line);
        if offset <= start {
            return self.prev_boundary(offset);
        }
        let text = self.slice(start..offset);
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut i = chars.len();
        while i > 0 && char_class(chars[i - 1].1) == CharClass::Space {
            i -= 1;
        }
        if i > 0 {
            let class = char_class(chars[i - 1].1);
            while i > 0 && char_class(chars[i - 1].1) == class {
                i -= 1;
            }
        }
        start + chars.get(i).map_or(text.len(), |(o, _)| *o)
    }

    /// Forward over spaces, then over a word (or a run of punctuation).
    fn next_word_boundary(&self, offset: usize) -> usize {
        let line = self.line_of(offset);
        let content = self.line_range(line);
        if offset >= content.end {
            return self.next_boundary(offset);
        }
        let text = self.slice(offset..content.end);
        let mut chars = text.char_indices().peekable();
        while let Some((_, c)) = chars.peek() {
            if char_class(*c) == CharClass::Space {
                chars.next();
            } else {
                break;
            }
        }
        if let Some((_, c)) = chars.peek() {
            let class = char_class(*c);
            while let Some((_, c)) = chars.peek() {
                if char_class(*c) == class {
                    chars.next();
                } else {
                    break;
                }
            }
        }
        offset + chars.peek().map_or(text.len(), |(o, _)| *o)
    }

    /// The word (or run of spaces or punctuation) around an offset, for double-click.
    pub(super) fn word_range(&self, offset: usize) -> Range<usize> {
        let line = self.line_of(offset);
        let content = self.line_range(line);
        let text = self.slice(content.clone());
        let at = offset.min(content.end) - content.start;
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        if chars.is_empty() {
            return offset..offset;
        }
        let index = chars.partition_point(|(o, _)| *o < at).min(chars.len() - 1);
        // Clicking just past a word's end picks that word.
        let index = if chars[index].0 > at || (at == text.len() && index > 0) {
            index.saturating_sub(usize::from(chars[index].0 >= at))
        } else {
            index
        };
        let class = char_class(chars[index].1);
        let mut first = index;
        while first > 0 && char_class(chars[first - 1].1) == class {
            first -= 1;
        }
        let mut last = index;
        while last + 1 < chars.len() && char_class(chars[last + 1].1) == class {
            last += 1;
        }
        let end = chars[last].0 + chars[last].1.len_utf8();
        content.start + chars[first].0..content.start + end
    }

    /// The whole line around an offset, with its break, for triple-click.
    pub(super) fn full_line_range(&self, offset: usize) -> Range<usize> {
        let line = self.line_of(offset);
        self.line_start(line)..self.line_start(line + 1)
    }

    // MARK: Movement

    fn horizontal(&mut self, forward: bool, word: bool, extend: bool, cx: &mut Context<Self>) {
        self.goal_x = None;
        if !extend && !self.selection.is_empty() && !word {
            let target = if forward { self.selection.end } else { self.selection.start };
            self.move_cursor_to(target, false, cx);
            return;
        }
        let cursor = self.cursor();
        let target = match (forward, word) {
            (false, false) => self.prev_boundary(cursor),
            (true, false) => self.next_boundary(cursor),
            (false, true) => self.prev_word_boundary(cursor),
            (true, true) => self.next_word_boundary(cursor),
        };
        self.move_cursor_to(target, extend, cx);
    }

    fn vertical(&mut self, rows: isize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !extend && !self.selection.is_empty() && rows.abs() == 1 {
            // Up and down from a selection start at its matching end.
            let edge = if rows < 0 { self.selection.start } else { self.selection.end };
            self.select(edge..edge, false);
        }
        let cursor = self.cursor();
        let (x, row) = self.x_and_row_for_offset(cursor, window, cx);
        let goal = *self.goal_x.get_or_insert(x);
        let last_row = self.wraps.row_count().saturating_sub(1) as isize;
        let target_row = row as isize + rows;
        let target = if target_row < 0 {
            0
        } else if target_row > last_row {
            self.len()
        } else {
            self.offset_for_row_x(target_row as usize, goal, window, cx)
        };
        let goal_x = self.goal_x;
        self.move_cursor_to(target, extend, cx);
        self.goal_x = goal_x;
    }

    fn page_rows(&self) -> isize {
        self.layout
            .as_ref()
            .map_or(20, |l| ((l.metrics.visible_height / l.metrics.line_height) as isize - 1).max(1))
    }

    fn page(&mut self, down: bool, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.page_rows();
        let line_height = self.layout.as_ref().map_or(18., |l| l.metrics.line_height);
        self.scroll.y += if down { 1. } else { -1. } * rows as f32 * line_height;
        self.vertical(if down { rows } else { -rows }, extend, window, cx);
    }

    /// Home: to the first non-blank character, or to the line's start if already there.
    fn line_start_target(&self) -> usize {
        let cursor = self.cursor();
        let line = self.line_of(cursor);
        let start = self.line_start(line);
        let text = self.line_text(line);
        let first = start + text::indent_len(&text);
        if cursor == first { start } else { first }
    }

    fn line_end_target(&self) -> usize {
        self.line_range(self.line_of(self.cursor())).end
    }

    pub(super) fn register_actions<E: gpui::InteractiveElement>(element: E, cx: &mut Context<Self>) -> E {
        element.on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.horizontal(false, false, false, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.horizontal(true, false, false, cx)))
            .on_action(cx.listener(|this, _: &MoveWordLeft, _, cx| this.horizontal(false, true, false, cx)))
            .on_action(cx.listener(|this, _: &MoveWordRight, _, cx| this.horizontal(true, true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.horizontal(false, false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.horizontal(true, false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| this.horizontal(false, true, true, cx)))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| this.horizontal(true, true, true, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, w, cx| this.vertical(-1, false, w, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, w, cx| this.vertical(1, false, w, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, w, cx| this.vertical(-1, true, w, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, w, cx| this.vertical(1, true, w, cx)))
            .on_action(cx.listener(|this, _: &PageUp, w, cx| this.page(false, false, w, cx)))
            .on_action(cx.listener(|this, _: &PageDown, w, cx| this.page(true, false, w, cx)))
            .on_action(cx.listener(|this, _: &SelectPageUp, w, cx| this.page(false, true, w, cx)))
            .on_action(cx.listener(|this, _: &SelectPageDown, w, cx| this.page(true, true, w, cx)))
            .on_action(cx.listener(|this, _: &MoveToLineStart, _, cx| {
                let target = this.line_start_target();
                this.goal_x = None;
                this.move_cursor_to(target, false, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectToLineStart, _, cx| {
                let target = this.line_start_target();
                this.goal_x = None;
                this.move_cursor_to(target, true, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveToLineEnd, _, cx| {
                let target = this.line_end_target();
                this.goal_x = None;
                this.move_cursor_to(target, false, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectToLineEnd, _, cx| {
                let target = this.line_end_target();
                this.goal_x = None;
                this.move_cursor_to(target, true, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveToStart, _, cx| this.move_cursor_to(0, false, cx)))
            .on_action(cx.listener(|this, _: &SelectToStart, _, cx| this.move_cursor_to(0, true, cx)))
            .on_action(cx.listener(|this, _: &MoveToEnd, _, cx| {
                let end = this.len();
                this.move_cursor_to(end, false, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectToEnd, _, cx| {
                let end = this.len();
                this.move_cursor_to(end, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                let end = this.len();
                this.select(0..end, false);
                this.history.seal();
                this.selection_changed(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLine, _, cx| {
                let lines = this.selected_lines_range();
                this.select(lines, false);
                this.selection_changed(cx);
            }))
            .on_action(cx.listener(|this, _: &CollapseSelection, _, cx| {
                let cursor = this.cursor();
                if !this.selection.is_empty() {
                    this.move_cursor_to(cursor, false, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Backspace, _, cx| this.delete_toward(false, false, cx)))
            .on_action(cx.listener(|this, _: &Delete, _, cx| this.delete_toward(true, false, cx)))
            .on_action(cx.listener(|this, _: &DeleteWordLeft, _, cx| this.delete_toward(false, true, cx)))
            .on_action(cx.listener(|this, _: &DeleteWordRight, _, cx| this.delete_toward(true, true, cx)))
            .on_action(cx.listener(|this, _: &DeleteToLineStart, _, cx| {
                let cursor = this.cursor();
                let start = this.line_start(this.line_of(cursor));
                let range = if this.selection.is_empty() { start..cursor } else { this.selection() };
                this.delete_range(range, cx);
            }))
            .on_action(cx.listener(|this, _: &Newline, _, cx| this.newline(cx)))
            .on_action(cx.listener(|this, _: &Tab, _, cx| this.tab(cx)))
            .on_action(cx.listener(|this, _: &Backtab, _, cx| this.outdent_lines(cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.redo(cx)))
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &Cut, _, cx| {
                this.copy(cx);
                let range = this.selection();
                this.delete_range(range, cx);
            }))
            .on_action(cx.listener(|this, _: &Paste, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    this.insert(&text, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleComment, _, cx| this.toggle_comment(cx)))
            .on_action(cx.listener(|this, _: &IndentLines, _, cx| this.indent_lines(cx)))
            .on_action(cx.listener(|this, _: &OutdentLines, _, cx| this.outdent_lines(cx)))
            .on_action(cx.listener(|this, _: &MoveLinesUp, _, cx| this.move_lines(false, cx)))
            .on_action(cx.listener(|this, _: &MoveLinesDown, _, cx| this.move_lines(true, cx)))
            .on_action(cx.listener(|this, _: &DuplicateLinesDown, _, cx| this.duplicate_lines(true, cx)))
            .on_action(cx.listener(|this, _: &DuplicateLinesUp, _, cx| this.duplicate_lines(false, cx)))
    }

    // MARK: Deleting

    fn delete_toward(&mut self, forward: bool, word: bool, cx: &mut Context<Self>) {
        let range = if !self.selection.is_empty() {
            self.selection()
        } else {
            let cursor = self.cursor();
            let target = match (forward, word) {
                (false, false) => self.prev_boundary(cursor),
                (true, false) => self.next_boundary(cursor),
                (false, true) => self.prev_word_boundary(cursor),
                (true, true) => self.next_word_boundary(cursor),
            };
            cursor.min(target)..cursor.max(target)
        };
        if range.is_empty() {
            return;
        }
        self.transact(EditKind::Deleting, cx, |this, cx| {
            this.edit(range.clone(), "", cx);
            this.select(range.start..range.start, false);
        });
    }

    fn delete_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        if range.is_empty() {
            return;
        }
        self.transact(EditKind::Other, cx, |this, cx| {
            this.edit(range.clone(), "", cx);
            this.select(range.start..range.start, false);
        });
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text()));
        }
    }

    // MARK: Return and Tab

    /// Keeps the current line's indentation, adds a level after an opening bracket
    /// (or `:` in Python and YAML), and puts a closing bracket right after the cursor
    /// on its own line.
    fn newline(&mut self, cx: &mut Context<Self>) {
        let selection = self.selection();
        let line = self.line_of(selection.start);
        let line_start = self.line_start(line);
        let before = self.slice(line_start..selection.start);
        let indent = before[..text::indent_len(&before)].to_string();
        let opener = before.trim_end_matches([' ', '\t']).chars().last();
        let next = self.slice(selection.end..self.line_range(self.line_of(selection.end)).end).chars().next();

        let opens_block = match opener {
            Some('{' | '[' | '(') => true,
            Some(':') => self.language.is_some_and(|l| l.opens_block_with_colon()),
            _ => false,
        };
        let unit = self.indentation(cx).unit();
        let mut insertion = format!("{}{}", self.line_ending, indent);
        if opens_block {
            insertion.push_str(&unit);
        }
        let closes = opens_block && opener.and_then(closer) == next && next.is_some();
        let caret = selection.start + insertion.len();
        if closes {
            insertion.push_str(self.line_ending);
            insertion.push_str(&indent);
        }
        self.transact(EditKind::Other, cx, |this, cx| {
            this.edit(selection.clone(), &insertion, cx);
            this.select(caret..caret, false);
            this.marked = None;
        });
    }

    /// Indents the selected lines when the selection spans lines; otherwise inserts a
    /// tab or, in space-indented files, spaces up to the next indent stop.
    fn tab(&mut self, cx: &mut Context<Self>) {
        let selection = self.selection();
        if !selection.is_empty() && self.selected_text().contains(['\n', '\r']) {
            self.indent_lines(cx);
            return;
        }
        let indentation = self.indentation(cx);
        if indentation.uses_tabs {
            self.insert("\t", cx);
            return;
        }
        let line_start = self.line_start(self.line_of(selection.start));
        let before = self.slice(line_start..selection.start);
        let column = text::visual_column(&before, before.len(), Self::tab_width(cx));
        let count = indentation.width - column % indentation.width;
        self.insert(&" ".repeat(count), cx);
    }

    // MARK: Line commands

    /// Whole lines the selection touches. A selection that ends right after a line
    /// break leaves the next line out, as in other editors.
    fn selected_lines_range(&self) -> Range<usize> {
        let mut selection = self.selection();
        if !selection.is_empty() {
            let last = self.rope.char(self.rope.byte_to_char(selection.end) - 1);
            if text::is_line_break(last) {
                selection.end -= 1;
            }
        }
        let first = self.line_of(selection.start);
        let last = self.line_of(selection.end);
        self.line_start(first)..self.line_start(last + 1)
    }

    fn indent_lines(&mut self, cx: &mut Context<Self>) {
        let unit = self.indentation(cx).unit();
        self.edit_selected_lines(cx, |lines| {
            lines
                .iter()
                .map(|line| (!line.is_empty()).then(|| LineEdit { column: 0, remove: 0, insert: unit.clone() }))
                .collect()
        });
    }

    fn outdent_lines(&mut self, cx: &mut Context<Self>) {
        let width = self.indentation(cx).width;
        self.edit_selected_lines(cx, |lines| {
            lines
                .iter()
                .map(|line| {
                    let remove = if line.starts_with('\t') {
                        1
                    } else {
                        line.bytes().take(width).take_while(|b| *b == b' ').count()
                    };
                    (remove > 0).then(|| LineEdit { column: 0, remove, insert: String::new() })
                })
                .collect()
        });
    }

    /// Comments out the selected lines, or uncomments them if they all are already.
    fn toggle_comment(&mut self, cx: &mut Context<Self>) {
        let Some(language) = self.language else { return };
        if let Some(token) = language.line_comment() {
            self.edit_selected_lines(cx, |lines| {
                let non_blank: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
                if non_blank.is_empty() {
                    return lines.iter().map(|_| None).collect();
                }
                let commented =
                    non_blank.iter().all(|l| l[text::indent_len(l)..].starts_with(token));
                let column = non_blank.iter().map(|l| text::indent_len(l)).min().unwrap_or(0);
                lines
                    .iter()
                    .map(|line| {
                        if line.trim().is_empty() {
                            return None;
                        }
                        if commented {
                            let start = text::indent_len(line);
                            let rest = &line[start + token.len()..];
                            Some(LineEdit {
                                column: start,
                                remove: token.len() + usize::from(rest.starts_with(' ')),
                                insert: String::new(),
                            })
                        } else {
                            Some(LineEdit { column, remove: 0, insert: format!("{token} ") })
                        }
                    })
                    .collect()
            });
        } else if let Some((open, close)) = language.block_comment() {
            self.edit_selected_lines(cx, |lines| {
                let non_blank: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
                if non_blank.is_empty() {
                    return lines.iter().map(|_| None).collect();
                }
                let commented = non_blank.iter().all(|l| {
                    let body = l.trim();
                    body.starts_with(open) && body.ends_with(close)
                });
                lines
                    .iter()
                    .map(|line| {
                        if line.trim().is_empty() {
                            return None;
                        }
                        let start = text::indent_len(line);
                        let body = &line[start..];
                        let replacement = if commented {
                            let inner = body.trim();
                            inner[open.len()..inner.len() - close.len()].trim().to_string()
                        } else {
                            format!("{open} {body} {close}")
                        };
                        Some(LineEdit { column: start, remove: body.len(), insert: replacement })
                    })
                    .collect()
            });
        }
    }

    /// Replaces the selected lines in one undoable step. `transform` gets each line
    /// (without its break) and returns one change per line, or `None` to leave it.
    /// The selection follows the changes.
    fn edit_selected_lines(
        &mut self,
        cx: &mut Context<Self>,
        transform: impl FnOnce(&[String]) -> Vec<Option<LineEdit>>,
    ) {
        let block = self.selected_lines_range();
        let first = self.line_of(block.start);
        let mut lines = Vec::new();
        let mut line = first;
        loop {
            let start = self.line_start(line);
            if start >= block.end && line > first {
                break;
            }
            let content = self.line_range(line);
            let next = self.line_start(line + 1);
            lines.push((start, self.slice(content.clone()), self.slice(content.end..next)));
            if next >= block.end || line + 1 >= self.line_count() {
                break;
            }
            line += 1;
        }
        let contents: Vec<String> = lines.iter().map(|(_, c, _)| c.clone()).collect();
        let edits = transform(&contents);
        if edits.iter().all(Option::is_none) {
            return;
        }

        let mut replacement = String::new();
        let mut applied: Vec<(usize, usize, usize)> = Vec::new();
        for ((start, contents, brk), edit) in lines.iter().zip(edits) {
            let Some(edit) = edit else {
                replacement.push_str(contents);
                replacement.push_str(brk);
                continue;
            };
            let column = edit.column.min(contents.len());
            let remove = edit.remove.min(contents.len() - column);
            replacement.push_str(&contents[..column]);
            replacement.push_str(&edit.insert);
            replacement.push_str(&contents[column + remove..]);
            replacement.push_str(brk);
            applied.push((start + column, remove, edit.insert.len()));
        }

        let selection = self.selection();
        let has_selection = !selection.is_empty();
        let map = |position: usize, is_start: bool| -> usize {
            let mut shift: isize = 0;
            for &(at, remove, insert) in &applied {
                if position > at + remove
                    || (position == at + remove && !(is_start && has_selection && remove == 0))
                {
                    shift += insert as isize - remove as isize;
                } else if position > at {
                    return (at as isize + shift) as usize + (position - at).min(insert);
                }
            }
            (position as isize + shift) as usize
        };
        let start = map(selection.start, true);
        let end = if has_selection { map(selection.end, false) } else { start };
        let replaced = block.start..block.start + lines.iter().map(|(_, c, b)| c.len() + b.len()).sum::<usize>();
        self.transact(EditKind::Other, cx, |this, cx| {
            this.edit(replaced.clone(), &replacement, cx);
            this.select(start..end.max(start), false);
        });
    }

    fn move_lines(&mut self, down: bool, cx: &mut Context<Self>) {
        let block = self.selected_lines_range();
        let selection = self.selection();
        let block_text = self.slice(block.clone());
        let (block_body, own_break) = text::split_line_break(&block_text);
        if !down {
            if block.start == 0 {
                return;
            }
            let previous = self.full_line_range(block.start - 1);
            let previous_text = self.slice(previous.clone());
            let (previous_body, previous_break) = text::split_line_break(&previous_text);
            // The block is the last line, without a break: it takes the previous line's.
            let block_break = if own_break.is_empty() { previous_break } else { own_break };
            let after_break = if own_break.is_empty() { "" } else { previous_break };
            let replacement = format!("{block_body}{block_break}{previous_body}{after_break}");
            let shift = previous.len();
            self.transact(EditKind::Other, cx, |this, cx| {
                this.edit(previous.start..block.end, &replacement, cx);
                this.select(selection.start - shift..selection.end - shift, false);
            });
        } else {
            if block.end >= self.len() {
                return;
            }
            let next = self.full_line_range(block.end);
            let next_text = self.slice(next.clone());
            let (next_body, next_own_break) = text::split_line_break(&next_text);
            // The next line is the last, without a break: it takes the block's.
            let next_break = if next_own_break.is_empty() { own_break } else { next_own_break };
            let block_break = if next_own_break.is_empty() { "" } else { own_break };
            let moved = format!("{next_body}{next_break}");
            let replacement = format!("{moved}{block_body}{block_break}");
            let shift = moved.len();
            self.transact(EditKind::Other, cx, |this, cx| {
                this.edit(block.start..next.end, &replacement, cx);
                this.select(selection.start + shift..selection.end + shift, false);
            });
        }
        self.request_autoscroll(Autoscroll::Fit);
    }

    fn duplicate_lines(&mut self, down: bool, cx: &mut Context<Self>) {
        let block = self.selected_lines_range();
        let selection = self.selection();
        let block_text = self.slice(block.clone());
        let (body, line_break) = text::split_line_break(&block_text);
        let ending = self.line_ending;
        if down {
            // After the block; a last line without a break gets one first.
            let insertion =
                if line_break.is_empty() { format!("{ending}{body}") } else { format!("{body}{line_break}") };
            let shift = if line_break.is_empty() { ending.len() + body.len() } else { block.len() };
            self.transact(EditKind::Other, cx, |this, cx| {
                this.edit(block.end..block.end, &insertion, cx);
                this.select(selection.start + shift..selection.end + shift, false);
            });
        } else {
            let insertion = format!("{body}{}", if line_break.is_empty() { ending } else { line_break });
            self.transact(EditKind::Other, cx, |this, cx| {
                this.edit(block.start..block.start, &insertion, cx);
                // The selection stays on the upper copy.
                this.select(selection.clone(), false);
            });
        }
    }
}

fn closer(opener: char) -> Option<char> {
    match opener {
        '{' => Some('}'),
        '[' => Some(']'),
        '(' => Some(')'),
        _ => None,
    }
}
