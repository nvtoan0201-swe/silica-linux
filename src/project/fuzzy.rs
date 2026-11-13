//! Subsequence matching in the style of editor "Quick Open" pickers: the query's
//! characters must appear in order, and matches score higher when they are
//! consecutive, start words, or fall in the file name rather than its folders.

use std::path::PathBuf;

/// A file in a project, prepared for fuzzy matching.
#[derive(Clone, Debug)]
pub struct FileItem {
    pub path: PathBuf,
    /// Path relative to the project root, e.g. `src/app/main.rs`.
    pub relative: String,
    /// Char index where the file name starts within `relative`.
    pub name_start: usize,
    chars: Vec<char>,
    lowered: Vec<char>,
}

impl FileItem {
    pub fn new(path: PathBuf, relative: String) -> FileItem {
        let chars: Vec<char> = relative.chars().collect();
        let lowered = chars.iter().map(|c| lower(*c)).collect();
        let name_start = chars.iter().rposition(|c| *c == '/').map_or(0, |i| i + 1);
        FileItem { path, relative, name_start, chars, lowered }
    }

    pub fn name(&self) -> &str {
        let byte = self.relative.char_indices().nth(self.name_start).map_or(0, |(i, _)| i);
        &self.relative[byte..]
    }
}

pub struct FuzzyMatch {
    pub score: i64,
    /// Char indices into the item's relative path that matched the query.
    pub positions: Vec<usize>,
}

/// Lowercased query chars, with spaces dropped so "app main" matches `app/main.rs`.
pub fn prepare(query: &str) -> Vec<char> {
    query.chars().filter(|c| !c.is_whitespace()).map(lower).collect()
}

pub fn fuzzy_match(query: &[char], item: &FileItem) -> Option<FuzzyMatch> {
    if query.is_empty() || query.len() > item.lowered.len() || !is_subsequence(query, &item.lowered) {
        return None;
    }
    let mut best: Option<FuzzyMatch> = None;
    let mut consider = |positions: Vec<usize>| {
        let score = score(&positions, item);
        if best.as_ref().is_none_or(|b| score > b.score) {
            best = Some(FuzzyMatch { score, positions });
        }
    };
    // Prefer a match inside the file name; try a few starting points there.
    if is_subsequence(query, &item.lowered[item.name_start..]) {
        let mut attempts = 0;
        for start in item.name_start..item.lowered.len() {
            if item.lowered[start] != query[0] {
                continue;
            }
            if let Some(positions) = forward(query, &item.lowered, start) {
                consider(positions);
            }
            attempts += 1;
            if attempts == 6 {
                break;
            }
        }
    }
    // Otherwise (or as a competitor) match right-to-left over the whole path, which
    // keeps the tail of the query as close to the file name as possible.
    if let Some(positions) = backward(query, &item.lowered) {
        consider(positions);
    }
    best
}

fn is_subsequence(query: &[char], text: &[char]) -> bool {
    let mut q = 0;
    for c in text {
        if *c == query[q] {
            q += 1;
            if q == query.len() {
                return true;
            }
        }
    }
    false
}

fn forward(query: &[char], text: &[char], start: usize) -> Option<Vec<usize>> {
    let mut positions = Vec::with_capacity(query.len());
    let mut q = 0;
    let mut i = start;
    while i < text.len() && q < query.len() {
        if text[i] == query[q] {
            positions.push(i);
            q += 1;
        }
        i += 1;
    }
    (q == query.len()).then_some(positions)
}

fn backward(query: &[char], text: &[char]) -> Option<Vec<usize>> {
    let mut positions = vec![0; query.len()];
    let mut q = query.len() as isize - 1;
    let mut i = text.len() as isize - 1;
    while i >= 0 && q >= 0 {
        if text[i as usize] == query[q as usize] {
            positions[q as usize] = i as usize;
            q -= 1;
        }
        i -= 1;
    }
    (q < 0).then_some(positions)
}

fn score(positions: &[usize], item: &FileItem) -> i64 {
    let chars = &item.chars;
    let mut score: i64 = 0;
    let mut previous: isize = -2;
    for &position in positions {
        score += 10;
        if position as isize == previous + 1 {
            score += 15;
        } else if previous >= 0 {
            score -= (position as isize - previous - 1).min(10) as i64;
        }
        if position == item.name_start {
            score += 25;
        } else if position == 0 || is_separator(chars[position - 1]) {
            score += 12;
        } else if chars[position].is_uppercase() && !chars[position - 1].is_uppercase() {
            score += 10;
        }
        if position >= item.name_start {
            score += 6;
        }
        previous = position as isize;
    }
    if let (Some(&first), Some(&last)) = (positions.first(), positions.last()) {
        if first >= item.name_start {
            score += 30;
            // The query is a prefix of the file name, e.g. "main" for main.rs.
            if first == item.name_start && last - first == positions.len() - 1 {
                score += 40;
            }
        }
    }
    score - item.lowered.len() as i64 / 6
}

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '_' | '-' | '.' | ' ')
}

fn lower(c: char) -> char {
    if c.is_ascii() { c.to_ascii_lowercase() } else { c.to_lowercase().next().unwrap_or(c) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str) -> FileItem {
        FileItem::new(PathBuf::from(path), path.to_string())
    }

    #[test]
    fn prefers_file_name_prefix() {
        let query = prepare("main");
        let a = fuzzy_match(&query, &item("src/main.rs")).unwrap();
        let b = fuzzy_match(&query, &item("main/app/lib.rs")).unwrap();
        assert!(a.score > b.score);
    }

    #[test]
    fn requires_order() {
        assert!(fuzzy_match(&prepare("nm"), &item("main.rs")).is_none());
    }
}
