//! Plain-text helpers shared by the editor: line ranges, indentation and line breaks.

use std::ops::Range;

use ropey::Rope;

/// How a file indents, detected from the text when it opens or reloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Indentation {
    pub uses_tabs: bool,
    /// Columns per level: 2, 4 or 8, or the tab width for tabs.
    pub width: usize,
}

impl Indentation {
    /// The text one level of indentation inserts.
    pub fn unit(&self) -> String {
        if self.uses_tabs { "\t".into() } else { " ".repeat(self.width) }
    }
}

pub fn is_line_break(c: char) -> bool {
    c == '\n' || c == '\r'
}

/// The rope's line `line` without its line break, as an owned string.
pub fn line_text(rope: &Rope, line: usize) -> String {
    let mut text: String = rope.line(line).chars().collect();
    while text.ends_with(['\n', '\r']) {
        text.pop();
    }
    text
}

/// Byte range of line `line`, without its line break.
pub fn line_content_range(rope: &Rope, line: usize) -> Range<usize> {
    let start = rope.line_to_byte(line);
    let slice = rope.line(line);
    let mut len = slice.len_bytes();
    let mut chars = slice.chars_at(slice.len_chars());
    while let Some(c) = chars.prev() {
        if is_line_break(c) {
            len -= 1;
        } else {
            break;
        }
    }
    start..start + len
}

/// `\r\n` if the file's first line break is one, else `\n` (or `\r` for old Mac files).
pub fn detect_line_ending(text: &str) -> &'static str {
    if let Some(index) = text.find('\n') {
        if index > 0 && text.as_bytes()[index - 1] == b'\r' { "\r\n" } else { "\n" }
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

/// Indent of a line in columns, or `None` for a blank line.
pub fn indent_columns(line: &str, tab_width: usize) -> Option<usize> {
    let mut column = 0;
    for c in line.chars() {
        match c {
            ' ' => column += 1,
            '\t' => column += tab_width - column % tab_width,
            '\n' | '\r' => return None,
            _ => return Some(column),
        }
    }
    None
}

/// Byte length of the line's leading spaces and tabs.
pub fn indent_len(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ' || *b == b'\t').count()
}

/// Picks the indentation from the first lines: tabs if most indented lines start with a
/// tab, otherwise spaces with the most common step between lines (2, 4 or 8).
pub fn detect_indentation(rope: &Rope, tab_width: usize, default: Indentation) -> Indentation {
    let mut tab_lines = 0;
    let mut space_lines = 0;
    let mut steps = [0usize; 9];
    let mut previous = 0;
    for line in rope.lines().take(2000) {
        let text: String = line.chars().take(200).collect();
        if let Some(indent) = indent_columns(&text, tab_width) {
            match text.as_bytes().first() {
                Some(b'\t') => tab_lines += 1,
                Some(b' ') => space_lines += 1,
                _ => {}
            }
            let step = indent as isize - previous as isize;
            if step > 1 && step <= 8 {
                steps[step as usize] += 1;
            }
            previous = indent;
        }
    }
    // Nothing indented yet (a new file): the style from Settings.
    if tab_lines == 0 && space_lines == 0 {
        return default;
    }
    if tab_lines > space_lines {
        return Indentation { uses_tabs: true, width: tab_width };
    }
    let best = [2, 4, 8].into_iter().max_by_key(|w| (steps[*w], std::cmp::Reverse(*w))).unwrap();
    let width = if steps[best] > 0 { best } else { default.width };
    Indentation { uses_tabs: false, width }
}

/// One level deeper than the shallower neighbor, but no deeper than the deeper one:
/// a blank line after `{` joins the block it opens, one between two lines of a block
/// keeps its depth, and one after `}` before less-indented code drops out.
pub fn blank_line_indent(above: Option<usize>, below: Option<usize>, unit: usize) -> usize {
    let (Some(above), Some(below)) = (above, below) else { return 0 };
    if above == below {
        return below;
    }
    (above.min(below) + unit).min(above.max(below))
}

/// Splits off a trailing `\n`, `\r\n` or `\r`.
pub fn split_line_break(line: &str) -> (&str, &str) {
    for brk in ["\r\n", "\n", "\r"] {
        if let Some(body) = line.strip_suffix(brk) {
            return (body, brk);
        }
    }
    (line, "")
}

/// Columns from the start of `line` to byte `offset` in it, with tabs expanded.
pub fn visual_column(line: &str, offset: usize, tab_width: usize) -> usize {
    let mut column = 0;
    for c in line[..offset.min(line.len())].chars() {
        column += if c == '\t' { tab_width - column % tab_width } else { 1 };
    }
    column
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    Space,
    Word,
    Punct,
    LineBreak,
}

pub fn char_class(c: char) -> CharClass {
    if is_line_break(c) {
        CharClass::LineBreak
    } else if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_two_space_indent() {
        let rope = Rope::from_str("a {\n  b {\n    c\n  }\n}\n");
        let default = Indentation { uses_tabs: false, width: 4 };
        assert_eq!(detect_indentation(&rope, 4, default), Indentation { uses_tabs: false, width: 2 });
    }

    #[test]
    fn detects_tabs() {
        let rope = Rope::from_str("a {\n\tb\n\tc\n}\n");
        let default = Indentation { uses_tabs: false, width: 4 };
        assert!(detect_indentation(&rope, 4, default).uses_tabs);
    }

    #[test]
    fn content_range_strips_crlf() {
        let rope = Rope::from_str("ab\r\ncd");
        assert_eq!(line_content_range(&rope, 0), 0..2);
        assert_eq!(line_content_range(&rope, 1), 4..6);
        assert_eq!(rope.len_lines(), 2);
    }
}
