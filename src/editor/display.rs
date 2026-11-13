//! A line as drawn: tabs expanded to spaces up to the next tab stop, so tab
//! indentation lines up with space indentation and the indent guides.

pub struct DisplayLine {
    pub text: String,
    /// Each tab's byte offset in the source and in `text`, and how many spaces it became.
    tabs: Vec<(usize, usize, usize)>,
}

impl DisplayLine {
    pub fn new(source: &str, tab_width: usize) -> DisplayLine {
        if !source.contains('\t') {
            return DisplayLine { text: source.to_string(), tabs: Vec::new() };
        }
        let mut text = String::with_capacity(source.len() + 16);
        let mut tabs = Vec::new();
        let mut column = 0;
        for (offset, c) in source.char_indices() {
            if c == '\t' {
                let width = tab_width - column % tab_width;
                tabs.push((offset, text.len(), width));
                for _ in 0..width {
                    text.push(' ');
                }
                column += width;
            } else {
                text.push(c);
                column += 1;
            }
        }
        DisplayLine { text, tabs }
    }

    /// Where a source byte offset is drawn.
    pub fn to_display(&self, source: usize) -> usize {
        let mut shift = 0;
        for &(at, _, width) in &self.tabs {
            if at < source {
                shift += width - 1;
            } else {
                break;
            }
        }
        source + shift
    }

    /// The source byte offset drawn at a display offset; inside a tab, the side nearer.
    pub fn to_source(&self, display: usize) -> usize {
        let mut shift = 0;
        for &(at, start, width) in &self.tabs {
            if display <= start {
                break;
            }
            if display < start + width {
                return if display - start > width / 2 { at + 1 } else { at };
            }
            shift += width - 1;
        }
        display - shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tabs_to_stops() {
        let line = DisplayLine::new("a\tb", 4);
        assert_eq!(line.text, "a   b");
        assert_eq!(line.to_display(2), 4);
        assert_eq!(line.to_source(4), 2);
        assert_eq!(line.to_source(1), 1);
    }
}
