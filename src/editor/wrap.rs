//! Which visual rows each logical line takes when long lines wrap.

use smallvec::SmallVec;

/// Byte offsets (in the line's source) where a line's second and later rows start.
pub type RowStarts = SmallVec<[u32; 1]>;

#[derive(Clone, PartialEq)]
pub struct WrapKey {
    pub width: f32,
    pub font_size: f32,
    pub family: String,
    pub tab_width: usize,
}

#[derive(Default)]
pub struct WrapMap {
    /// `None` until the line has been measured since it last changed.
    lines: Vec<Option<RowStarts>>,
    /// The first row of each line, plus the total at the end.
    prefix: Vec<u32>,
    prefix_is_stale: bool,
    key: Option<WrapKey>,
}

impl WrapMap {
    pub fn new(line_count: usize) -> WrapMap {
        let mut map = WrapMap::default();
        map.reset(line_count);
        map
    }

    /// Forgets every line's rows (the width or font changed).
    pub fn reset(&mut self, line_count: usize) {
        self.lines = vec![None; line_count];
        self.prefix_is_stale = true;
    }

    /// Lines `start..start + old_count` became `new_count` lines.
    pub fn splice(&mut self, start: usize, old_count: usize, new_count: usize) {
        let end = (start + old_count).min(self.lines.len());
        self.lines.splice(start..end, std::iter::repeat_n(None, new_count));
        self.prefix_is_stale = true;
    }

    /// Takes a new wrapping width or font; returns `true` if every line must be measured again.
    pub fn set_key(&mut self, key: Option<WrapKey>) -> bool {
        if self.key == key {
            return false;
        }
        self.key = key;
        let count = self.lines.len();
        self.reset(count);
        true
    }

    pub fn key(&self) -> Option<&WrapKey> {
        self.key.as_ref()
    }

    pub fn is_wrapping(&self) -> bool {
        self.key.is_some()
    }

    /// Lines still to be measured.
    pub fn missing(&self) -> impl Iterator<Item = usize> + '_ {
        self.lines.iter().enumerate().filter(|(_, rows)| rows.is_none()).map(|(i, _)| i)
    }

    pub fn has_missing(&self) -> bool {
        self.lines.iter().any(Option::is_none)
    }

    pub fn set(&mut self, line: usize, rows: RowStarts) {
        if let Some(slot) = self.lines.get_mut(line) {
            *slot = Some(rows);
            self.prefix_is_stale = true;
        }
    }

    fn update_prefix(&mut self) {
        if !self.prefix_is_stale {
            return;
        }
        self.prefix.clear();
        self.prefix.reserve(self.lines.len() + 1);
        let mut total = 0u32;
        for rows in &self.lines {
            self.prefix.push(total);
            total += 1 + rows.as_ref().map_or(0, |r| r.len() as u32);
        }
        self.prefix.push(total);
        self.prefix_is_stale = false;
    }

    pub fn row_count(&mut self) -> usize {
        self.update_prefix();
        *self.prefix.last().unwrap_or(&0) as usize
    }

    /// The first visual row of a line.
    pub fn first_row(&mut self, line: usize) -> usize {
        self.update_prefix();
        self.prefix[line.min(self.prefix.len() - 1)] as usize
    }

    /// The line a visual row belongs to, and which of its rows it is.
    pub fn line_for_row(&mut self, row: usize) -> (usize, usize) {
        self.update_prefix();
        let lines = self.lines.len();
        if lines == 0 {
            return (0, 0);
        }
        let row = row as u32;
        // The last line whose first row is at or before `row`.
        let index = self.prefix[..lines].partition_point(|start| *start <= row).max(1) - 1;
        let within = (row - self.prefix[index]) as usize;
        (index, within.min(self.row_starts(index).len()))
    }

    pub fn row_starts(&self, line: usize) -> &[u32] {
        self.lines.get(line).and_then(|r| r.as_deref()).unwrap_or(&[])
    }

    /// The row within its line where a source byte offset sits; an offset right at a
    /// wrap goes to the start of the next row.
    pub fn row_in_line(&self, line: usize, offset_in_line: usize) -> usize {
        self.row_starts(line).partition_point(|start| *start as usize <= offset_in_line)
    }

    /// The byte range (within the line) of one of its rows.
    pub fn row_range(&self, line: usize, row: usize, line_len: usize) -> std::ops::Range<usize> {
        let starts = self.row_starts(line);
        let start = if row == 0 { 0 } else { starts.get(row - 1).map_or(line_len, |s| *s as usize) };
        let end = starts.get(row).map_or(line_len, |s| *s as usize);
        start.min(line_len)..end.min(line_len)
    }

    pub fn rows_in_line(&self, line: usize) -> usize {
        1 + self.row_starts(line).len()
    }
}

impl WrapMap {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Marks one line to be measured again.
    pub fn invalidate(&mut self, line: usize) {
        if let Some(slot) = self.lines.get_mut(line) {
            *slot = None;
            self.prefix_is_stale = true;
        }
    }
}
