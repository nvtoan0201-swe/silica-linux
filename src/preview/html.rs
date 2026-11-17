//! The HTML preview: the page read into the Markdown preview's blocks (headings,
//! paragraphs, lists, tables, code, quotes, images and links) and laid out the same way,
//! fitted to the editor's width. Scripts and stylesheets aren't run.

use std::path::Path;
use std::sync::LazyLock;

use gpui::{AnyElement, App, IntoElement, WeakEntity};
use pulldown_cmark::Alignment;
use regex::Regex;

use super::markdown::{self, Block, ImageRef, Inline, Item, Marks};
use crate::syntax::Language;
use crate::workspace::Workspace;

/// Renders `source`, the text of the HTML file at `path`.
pub fn render(source: &str, path: &Path, workspace: WeakEntity<Workspace>, cx: &App) -> AnyElement {
    let root = parse(source);
    let blocks = Converter.blocks(&root.children);
    if blocks.is_empty() {
        return crate::ui::unavailable("globe", "Nothing to Show", "This page has no visible text.", None, cx)
            .into_any_element();
    }
    markdown::document(&blocks, path, workspace, cx)
}

// ─── Parsing ────────────────────────────────────────────────────────────────────

enum Node {
    Element(Element),
    Text(String),
}

#[derive(Default)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// Hidden by its `hidden` attribute or an inline `display: none`.
    fn is_hidden(&self) -> bool {
        self.attr("hidden").is_some()
            || self.attr("style").is_some_and(|style| style.replace(' ', "").to_lowercase().contains("display:none"))
    }
}

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<!--.*?-->|<[!?][^>]*>|<(/?)([a-zA-Z][\w:-]*)((?:[^>"']|"[^"]*"|'[^']*')*)>"#).unwrap()
});
static ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([^\s=/>"']+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?"#).unwrap()
});

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
/// Elements whose contents are never shown, skipped without reading them as markup.
const RAW: &[&str] = &["script", "style", "template", "textarea", "title", "noscript", "svg", "math"];
/// Elements that end an open paragraph, as browsers do.
const CLOSES_P: &[&str] = &[
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "dl", "table", "pre", "blockquote", "hr",
    "section", "article", "header", "footer", "nav", "aside", "main", "figure", "form", "details", "address",
];

/// A forgiving tree builder: unclosed tags close when the browser would close them,
/// and stray closing tags are dropped.
fn parse(source: &str) -> Element {
    let lower = source.to_ascii_lowercase();
    let mut stack = vec![Element { name: "#root".into(), ..Default::default() }];
    let mut at = 0;
    while let Some(found) = TOKEN.captures_at(source, at) {
        let whole = found.get(0).unwrap();
        push_text(&mut stack, &source[at..whole.start()]);
        at = whole.end();
        let Some(name) = found.get(2) else { continue };
        let name = name.as_str().to_ascii_lowercase();
        if found[1].is_empty() {
            let attrs_text = &found[3];
            if RAW.contains(&name.as_str()) {
                // Skip to the matching close tag.
                let close = format!("</{name}");
                at = lower[at..].find(&close).map_or(source.len(), |i| at + i);
                continue;
            }
            open(&mut stack, &name);
            let attrs = ATTR
                .captures_iter(attrs_text)
                .map(|a| {
                    let value = a.get(2).or(a.get(3)).or(a.get(4)).map_or("", |v| v.as_str());
                    (a[1].to_ascii_lowercase(), decode_entities(value))
                })
                .collect();
            let element = Element { name, attrs, children: Vec::new() };
            if VOID.contains(&element.name.as_str()) || attrs_text.trim_end().ends_with('/') {
                stack.last_mut().unwrap().children.push(Node::Element(element));
            } else {
                stack.push(element);
            }
        } else if let Some(index) = stack.iter().rposition(|e| e.name == name) {
            if index > 0 {
                close_to(&mut stack, index);
            }
        }
    }
    push_text(&mut stack, &source[at..]);
    close_to(&mut stack, 1);
    stack.pop().unwrap()
}

fn push_text(stack: &mut [Element], text: &str) {
    if !text.is_empty() {
        stack.last_mut().unwrap().children.push(Node::Text(decode_entities(text)));
    }
}

/// Pops the elements down to (and including) `stack[index]` into their parents.
fn close_to(stack: &mut Vec<Element>, index: usize) {
    while stack.len() > index {
        let element = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::Element(element));
    }
}

/// Closes what opening `name` implicitly ends: an open `<p>`, the previous list item,
/// cell or row.
fn open(stack: &mut Vec<Element>, name: &str) {
    let close_open = |stack: &mut Vec<Element>, targets: &[&str], boundary: &[&str]| {
        for index in (1..stack.len()).rev() {
            let open = stack[index].name.as_str();
            if targets.contains(&open) {
                close_to(stack, index);
                return;
            }
            if boundary.contains(&open) {
                return;
            }
        }
    };
    if CLOSES_P.contains(&name) {
        close_open(stack, &["p"], &["div", "li", "td", "th", "blockquote", "section", "article", "body"]);
    }
    match name {
        "li" => close_open(stack, &["li"], &["ul", "ol", "menu"]),
        "dt" | "dd" => close_open(stack, &["dt", "dd"], &["dl"]),
        "td" | "th" => close_open(stack, &["td", "th"], &["tr", "table"]),
        "tr" => close_open(stack, &["tr"], &["table"]),
        "thead" | "tbody" | "tfoot" => close_open(stack, &["thead", "tbody", "tfoot"], &["table"]),
        "option" => close_open(stack, &["option"], &["select"]),
        _ => {}
    }
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[a-zA-Z]+);").unwrap());
    ENTITY
        .replace_all(text, |c: &regex::Captures| {
            let name = &c[1];
            let decoded = if let Some(hex) = name.strip_prefix("#x").or(name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(decimal) = name.strip_prefix('#') {
                decimal.parse().ok().and_then(char::from_u32)
            } else {
                Some(match name {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => ' ',
                    "copy" => '©',
                    "reg" => '®',
                    "trade" => '™',
                    "hellip" => '…',
                    "mdash" => '—',
                    "ndash" => '–',
                    "laquo" => '«',
                    "raquo" => '»',
                    "lsquo" => '‘',
                    "rsquo" => '’',
                    "ldquo" => '“',
                    "rdquo" => '”',
                    "bull" => '•',
                    "middot" => '·',
                    "times" => '×',
                    "divide" => '÷',
                    "deg" => '°',
                    "euro" => '€',
                    "pound" => '£',
                    "larr" => '←',
                    "rarr" => '→',
                    "uarr" => '↑',
                    "darr" => '↓',
                    "check" => '✓',
                    _ => return c[0].to_string(),
                })
            };
            decoded.map(String::from).unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

// ─── Conversion to blocks ───────────────────────────────────────────────────────

/// Elements laid out as blocks of their own; everything else flows inline.
const BLOCK: &[&str] = &[
    "html", "body", "p", "div", "section", "article", "main", "header", "footer", "nav", "aside", "figure",
    "figcaption", "form", "fieldset", "address", "details", "summary", "center", "dl", "dt", "dd", "h1", "h2",
    "h3", "h4", "h5", "h6", "pre", "blockquote", "ul", "ol", "menu", "li", "table", "hr", "legend", "caption",
];
/// Elements with nothing to show.
const SKIP: &[&str] = &["head", "iframe", "canvas", "object", "embed", "video", "audio", "select", "button", "map"];

struct Converter;

impl Converter {
    fn blocks(&self, nodes: &[Node]) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut inline = Inline::default();
        for node in nodes {
            match node {
                Node::Element(e) if e.is_hidden() || SKIP.contains(&e.name.as_str()) => {}
                Node::Element(e) if BLOCK.contains(&e.name.as_str()) => {
                    flush(&mut inline, &mut blocks);
                    self.block(e, &mut blocks);
                }
                _ => self.inline(node, Marks::default(), &mut inline),
            }
        }
        flush(&mut inline, &mut blocks);
        blocks
    }

    fn block(&self, e: &Element, blocks: &mut Vec<Block>) {
        let heading = |level| Block::Heading(level, self.inline_of(&e.children));
        use pulldown_cmark::HeadingLevel::*;
        match e.name.as_str() {
            "h1" => blocks.push(heading(H1)),
            "h2" => blocks.push(heading(H2)),
            "h3" => blocks.push(heading(H3)),
            "h4" => blocks.push(heading(H4)),
            "h5" => blocks.push(heading(H5)),
            "h6" => blocks.push(heading(H6)),
            "hr" => blocks.push(Block::Rule),
            "pre" => {
                let mut text = String::new();
                raw_text(&e.children, &mut text);
                let text = text.strip_prefix('\n').unwrap_or(&text).trim_end().to_string();
                blocks.push(Block::Code { language: code_language(e), text });
            }
            "blockquote" => blocks.push(Block::Quote { kind: None, blocks: self.blocks(&e.children) }),
            "ul" | "ol" | "menu" => {
                let start = (e.name == "ol").then(|| e.attr("start").and_then(|s| s.trim().parse().ok()).unwrap_or(1));
                let items = e
                    .children
                    .iter()
                    .filter_map(|node| match node {
                        Node::Element(li) if li.name == "li" && !li.is_hidden() => Some(self.item(li)),
                        _ => None,
                    })
                    .collect();
                blocks.push(Block::List { start, items });
            }
            "li" => blocks.push(Block::List { start: None, items: vec![self.item(e)] }),
            "table" => blocks.push(self.table(e)),
            "dt" | "summary" | "legend" | "caption" => {
                let mut inline = Inline::default();
                for child in &e.children {
                    self.inline(child, Marks { bold: true, ..Marks::default() }, &mut inline);
                }
                flush(&mut inline, blocks);
            }
            "dd" => blocks.push(Block::Quote { kind: None, blocks: self.blocks(&e.children) }),
            _ => blocks.extend(self.blocks(&e.children)),
        }
    }

    /// A list item, with its checkbox if it starts with one.
    fn item(&self, li: &Element) -> Item {
        let first = li.children.iter().position(|node| !matches!(node, Node::Text(text) if text.trim().is_empty()));
        let task = first.and_then(|index| match &li.children[index] {
            Node::Element(input) if input.name == "input" && input.attr("type") == Some("checkbox") => {
                Some((index, input.attr("checked").is_some()))
            }
            _ => None,
        });
        // The checkbox is drawn as the item's marker, so it's left out of the text.
        let children = match task {
            Some((index, _)) => &li.children[index + 1..],
            None => &li.children[..],
        };
        Item { task: task.map(|(_, checked)| checked), blocks: self.blocks(children) }
    }

    fn table(&self, table: &Element) -> Block {
        let mut rows: Vec<&Element> = Vec::new();
        collect_rows(&table.children, &mut rows);
        let header = rows.first().is_some_and(|row| {
            let cells = cells(row);
            !cells.is_empty() && cells.iter().all(|cell| cell.name == "th")
        });
        let alignment = |cell: &Element| {
            let style = cell.attr("style").unwrap_or("").replace(' ', "").to_lowercase();
            match cell.attr("align").map(str::to_lowercase).as_deref() {
                Some("center") => Alignment::Center,
                Some("right") => Alignment::Right,
                _ if style.contains("text-align:center") => Alignment::Center,
                _ if style.contains("text-align:right") => Alignment::Right,
                _ => Alignment::None,
            }
        };
        // Each column takes the first alignment set in its first two rows.
        let mut alignments: Vec<Alignment> = Vec::new();
        for row in rows.iter().take(2) {
            for (column, cell) in cells(row).into_iter().enumerate() {
                match alignments.get_mut(column) {
                    Some(set @ Alignment::None) => *set = alignment(cell),
                    Some(_) => {}
                    None => alignments.push(alignment(cell)),
                }
            }
        }
        let mut converted = rows.iter().map(|row| cells(row).into_iter().map(|cell| self.inline_of(&cell.children)).collect::<Vec<_>>());
        let head = if header { converted.next().unwrap_or_default() } else { Vec::new() };
        Block::Table { alignments, head, rows: converted.collect() }
    }

    /// Everything in `nodes` as one run of text, block elements becoming line breaks.
    fn inline_of(&self, nodes: &[Node]) -> Inline {
        let mut inline = Inline::default();
        for node in nodes {
            self.inline(node, Marks::default(), &mut inline);
        }
        inline.trim_end();
        inline
    }

    fn inline(&self, node: &Node, marks: Marks, inline: &mut Inline) {
        let e = match node {
            Node::Text(text) => return push_collapsed(inline, text, marks),
            Node::Element(e) => e,
        };
        if e.is_hidden() || SKIP.contains(&e.name.as_str()) {
            return;
        }
        let mut marks = marks;
        let mut link = None;
        match e.name.as_str() {
            "br" => return inline.push("\n", marks),
            "img" => {
                if let Some(url) = e.attr("src") {
                    let alt = e.attr("alt").unwrap_or("").to_string();
                    inline.images.push(ImageRef { url: url.to_string(), alt });
                }
                return;
            }
            "input" if e.attr("type") == Some("checkbox") => {
                return inline.push(if e.attr("checked").is_some() { "☑ " } else { "☐ " }, marks);
            }
            "b" | "strong" | "th" => marks.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" => marks.italic = true,
            "s" | "del" | "strike" => marks.strike = true,
            "code" | "kbd" | "samp" | "tt" => marks.code = true,
            "sup" => marks.superscript = true,
            "a" => {
                if let Some(href) = e.attr("href").filter(|href| !href.starts_with("javascript:")) {
                    marks.link = true;
                    link = Some((inline.text.len(), href.to_string()));
                }
            }
            "pre" => {
                let mut text = String::new();
                raw_text(&e.children, &mut text);
                return inline.push(text.trim_matches('\n'), Marks { code: true, ..marks });
            }
            _ => {}
        }
        let block = BLOCK.contains(&e.name.as_str());
        if block {
            line_break(inline);
        }
        for child in &e.children {
            self.inline(child, marks, inline);
        }
        if block {
            line_break(inline);
        }
        if let Some((start, url)) = link {
            if start < inline.text.len() {
                inline.links.push((start..inline.text.len(), url));
            }
        }
    }
}

fn cells(row: &Element) -> Vec<&Element> {
    row.children
        .iter()
        .filter_map(|node| match node {
            Node::Element(cell) if cell.name == "td" || cell.name == "th" => Some(cell),
            _ => None,
        })
        .collect()
}

fn collect_rows<'a>(nodes: &'a [Node], rows: &mut Vec<&'a Element>) {
    for node in nodes {
        if let Node::Element(e) = node {
            match e.name.as_str() {
                "tr" => rows.push(e),
                "thead" | "tbody" | "tfoot" => collect_rows(&e.children, rows),
                _ => {}
            }
        }
    }
}

/// Text as written, for `<pre>`: no whitespace collapsing, `<br>` as a line break.
fn raw_text(nodes: &[Node], text: &mut String) {
    for node in nodes {
        match node {
            Node::Text(t) => text.push_str(t),
            Node::Element(e) if e.name == "br" => text.push('\n'),
            Node::Element(e) => raw_text(&e.children, text),
        }
    }
}

/// The language of a `<pre>`, from a `language-…` or `lang-…` class on it or its `<code>`.
fn code_language(pre: &Element) -> Option<Language> {
    let code = pre.children.iter().find_map(|node| match node {
        Node::Element(e) if e.name == "code" => Some(e),
        _ => None,
    });
    [Some(pre), code].into_iter().flatten().find_map(|e| {
        e.attr("class")?.split_whitespace().find_map(|class| {
            let name = class.strip_prefix("language-").or(class.strip_prefix("lang-"))?;
            Language::for_fence(name)
        })
    })
}

/// Pushes `text` with runs of whitespace collapsed to one space, as browsers lay it out.
fn push_collapsed(inline: &mut Inline, text: &str, marks: Marks) {
    let mut collapsed = String::with_capacity(text.len());
    let mut space = inline.text.is_empty() || inline.text.ends_with([' ', '\n']);
    for c in text.chars() {
        if c.is_whitespace() {
            if !space {
                collapsed.push(' ');
                space = true;
            }
        } else {
            collapsed.push(c);
            space = false;
        }
    }
    inline.push(&collapsed, marks);
}

fn line_break(inline: &mut Inline) {
    if !inline.text.is_empty() && !inline.text.ends_with('\n') {
        inline.trim_end();
        inline.push("\n", Marks::default());
    }
}

/// Ends the paragraph being gathered, if it has anything in it.
fn flush(inline: &mut Inline, blocks: &mut Vec<Block>) {
    let mut paragraph = std::mem::take(inline);
    paragraph.trim_end();
    if !paragraph.is_empty() {
        blocks.push(Block::Paragraph(paragraph));
    }
}

