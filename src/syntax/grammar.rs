use std::ops::Range;

use fancy_regex::Regex;

/// What a highlighted range represents; mapped to a color by `SyntaxTheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TokenKind {
    Keyword,
    Type,
    String,
    Number,
    Comment,
    Attribute,
    Preprocessor,
    Function,
    Property,
    Tag,
    Variable,
    Heading,
    Link,
    /// Added and removed lines in a diff.
    Inserted,
    Deleted,
}

impl TokenKind {
    pub const ALL: [TokenKind; 15] = [
        TokenKind::Keyword,
        TokenKind::Type,
        TokenKind::String,
        TokenKind::Number,
        TokenKind::Comment,
        TokenKind::Attribute,
        TokenKind::Preprocessor,
        TokenKind::Function,
        TokenKind::Property,
        TokenKind::Tag,
        TokenKind::Variable,
        TokenKind::Heading,
        TokenKind::Link,
        TokenKind::Inserted,
        TokenKind::Deleted,
    ];

    fn from_u8(value: u8) -> Option<TokenKind> {
        Self::ALL.get(value as usize).copied()
    }
}

/// A colored stretch of text, as byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

pub struct Pattern {
    pub kind: TokenKind,
    pub pattern: String,
    /// Capture group to color; 0 colors the whole match.
    pub group: usize,
}

pub fn pat(kind: TokenKind, pattern: impl Into<String>) -> Pattern {
    Pattern { kind, pattern: pattern.into(), group: 0 }
}

pub fn pat_group(kind: TokenKind, pattern: impl Into<String>, group: usize) -> Pattern {
    Pattern { kind, pattern: pattern.into(), group }
}

struct Rule {
    kind: TokenKind,
    regex: Regex,
    group: usize,
}

/// A small regex-based grammar, the same as Silica's on macOS.
///
/// Tokenizing runs in two phases:
/// 1. `rules` are matched independently, in order; later rules win where they overlap.
/// 2. `spans` (comments and strings) are matched as one combined, left-to-right scan,
///    so whichever starts first wins (`"//"` inside a string stays a string), and they
///    override everything from phase 1.
///
/// Span patterns must only use non-capturing groups `(?:…)`.
pub struct Grammar {
    rules: Vec<Rule>,
    span_regex: Option<Regex>,
    span_kinds: Vec<TokenKind>,
}

impl Grammar {
    pub fn new(rules: Vec<Pattern>, spans: Vec<Pattern>) -> Grammar {
        // Grammars are static and covered by tests; a bad pattern is a programmer error.
        let rules = rules
            .into_iter()
            .map(|p| Rule {
                kind: p.kind,
                regex: Regex::new(&format!("(?m){}", p.pattern))
                    .unwrap_or_else(|e| panic!("bad pattern {}: {e}", p.pattern)),
                group: p.group,
            })
            .collect();
        let span_kinds = spans.iter().map(|p| p.kind).collect();
        let span_regex = if spans.is_empty() {
            None
        } else {
            let combined = spans
                .iter()
                .map(|p| format!("({})", p.pattern))
                .collect::<Vec<_>>()
                .join("|");
            Some(
                Regex::new(&format!("(?m){combined}"))
                    .unwrap_or_else(|e| panic!("bad span pattern {combined}: {e}")),
            )
        };
        Grammar { rules, span_regex, span_kinds }
    }

    /// The text's colored spans, sorted and non-overlapping. Thread-safe.
    pub fn highlight(&self, text: &str) -> Vec<Span> {
        const PLAIN: u8 = u8::MAX;
        let mut kinds = vec![PLAIN; text.len()];
        let mut paint = |range: Range<usize>, kind: TokenKind| {
            kinds[range].fill(kind as u8);
        };

        for rule in &self.rules {
            for caps in rule.regex.captures_iter(text) {
                let Ok(caps) = caps else { break };
                if let Some(m) = caps.get(rule.group) {
                    if !m.range().is_empty() {
                        paint(m.range(), rule.kind);
                    }
                }
            }
        }

        if let Some(regex) = &self.span_regex {
            for caps in regex.captures_iter(text) {
                let Ok(caps) = caps else { break };
                for (index, kind) in self.span_kinds.iter().enumerate() {
                    if let Some(m) = caps.get(index + 1) {
                        if !m.range().is_empty() {
                            paint(m.range(), *kind);
                        }
                        break;
                    }
                }
            }
        }

        let mut spans = Vec::new();
        let mut start = 0;
        while start < kinds.len() {
            let value = kinds[start];
            let mut end = start + 1;
            while end < kinds.len() && kinds[end] == value {
                end += 1;
            }
            if let Some(kind) = TokenKind::from_u8(value) {
                spans.push(Span { range: start..end, kind });
            }
            start = end;
        }
        spans
    }
}
