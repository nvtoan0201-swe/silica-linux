//! The Markdown preview: CommonMark with GitHub's extensions (tables, task lists,
//! strikethrough, footnotes and `> [!NOTE]` alerts), laid out like a README on GitHub.

use std::cell::Cell;
use std::iter::Peekable;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use gpui::{
    AnyElement, App, FontStyle, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, StrikethroughStyle, Styled,
    TextRun, TextSystem, UnderlineStyle, WeakEntity, Window, div, font, img, prelude::FluentBuilder, px, relative,
};
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use regex::Regex;

use crate::settings::Settings;
use crate::syntax::{Language, SyntaxTheme};
use crate::theme::{self, Colors, alpha, hex};
use super::text::WrappedText;
use crate::ui::icon_view;
use crate::workspace::Workspace;

// ─── Document model ─────────────────────────────────────────────────────────────

pub(super) enum Block {
    Paragraph(Inline),
    Heading(HeadingLevel, Inline),
    Code { language: Option<Language>, text: String },
    Quote { kind: Option<BlockQuoteKind>, blocks: Vec<Block> },
    List { start: Option<u64>, items: Vec<Item> },
    Table { alignments: Vec<Alignment>, head: Vec<Inline>, rows: Vec<Vec<Inline>> },
    Footnote { label: String, blocks: Vec<Block> },
    Rule,
}

pub(super) struct Item {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
    pub superscript: bool,
}

/// A run of styled text, with the links and images in it.
#[derive(Default)]
pub(super) struct Inline {
    pub text: String,
    runs: Vec<(usize, Marks)>,
    pub links: Vec<(Range<usize>, String)>,
    pub images: Vec<ImageRef>,
}

pub(super) struct ImageRef {
    pub url: String,
    pub alt: String,
}

impl Inline {
    pub fn push(&mut self, text: &str, marks: Marks) {
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
        match self.runs.last_mut() {
            Some((len, last)) if *last == marks => *len += text.len(),
            _ => self.runs.push((text.len(), marks)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.images.is_empty()
    }

    /// Drops trailing whitespace, as a hard break at the end of a paragraph leaves.
    pub fn trim_end(&mut self) {
        let keep = self.text.trim_end().len();
        let mut cut = self.text.len() - keep;
        self.text.truncate(keep);
        while cut > 0 {
            let Some((len, _)) = self.runs.last_mut() else { break };
            if *len > cut {
                *len -= cut;
                break;
            }
            cut -= *len;
            self.runs.pop();
        }
        for (range, _) in &mut self.links {
            range.end = range.end.min(keep);
        }
    }
}

// ─── Parsing ────────────────────────────────────────────────────────────────────

static IMG_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["'][^>]*>"#).unwrap());
static IMG_ALT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\balt\s*=\s*["']([^"']*)["']"#).unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->|<[^>]+>").unwrap());

fn parse(source: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_SUPERSCRIPT;
    let mut parser = Builder { events: Parser::new_ext(source, options).peekable(), task: None };
    parser.blocks()
}

struct Builder<'a> {
    events: Peekable<Parser<'a>>,
    /// The checkbox of the list item being read, seen before its text.
    task: Option<bool>,
}

fn is_inline(event: &Event) -> bool {
    match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::InlineHtml(_)
        | Event::InlineMath(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::FootnoteReference(_)
        | Event::TaskListMarker(_) => true,
        Event::Start(tag) => matches!(
            tag,
            Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Superscript
                | Tag::Subscript
                | Tag::Link { .. }
                | Tag::Image { .. }
        ),
        Event::End(tag) => matches!(
            tag,
            TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
                | TagEnd::Link
                | TagEnd::Image
        ),
        _ => false,
    }
}

impl Builder<'_> {
    /// Blocks up to the end of the enclosing container, which is consumed.
    fn blocks(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        while let Some(event) = self.events.peek() {
            if is_inline(event) {
                // A tight list item's text comes without a paragraph around it.
                let inline = self.inline();
                if !inline.is_empty() {
                    blocks.push(Block::Paragraph(inline));
                }
                continue;
            }
            let Some(event) = self.events.next() else { break };
            match event {
                Event::End(_) => break,
                Event::Start(Tag::Paragraph) => {
                    let inline = self.inline_until_end();
                    if !inline.is_empty() {
                        blocks.push(Block::Paragraph(inline));
                    }
                }
                Event::Start(Tag::Heading { level, .. }) => blocks.push(Block::Heading(level, self.inline_until_end())),
                Event::Start(Tag::BlockQuote(kind)) => blocks.push(Block::Quote { kind, blocks: self.blocks() }),
                Event::Start(Tag::CodeBlock(kind)) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => {
                            Language::for_fence(info.split([' ', ',', '{']).next().unwrap_or(""))
                        }
                        CodeBlockKind::Indented => None,
                    };
                    let mut text = String::new();
                    for event in self.events.by_ref() {
                        match event {
                            Event::Text(t) => text.push_str(&t),
                            Event::End(_) => break,
                            _ => {}
                        }
                    }
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    blocks.push(Block::Code { language, text });
                }
                Event::Start(Tag::List(start)) => {
                    let mut items = Vec::new();
                    while let Some(event) = self.events.next() {
                        match event {
                            Event::Start(Tag::Item) => {
                                self.task = None;
                                let blocks = self.blocks();
                                items.push(Item { task: self.task.take(), blocks });
                            }
                            Event::End(_) => break,
                            _ => {}
                        }
                    }
                    blocks.push(Block::List { start, items });
                }
                Event::Start(Tag::Table(alignments)) => blocks.push(self.table(alignments)),
                Event::Start(Tag::FootnoteDefinition(label)) => {
                    blocks.push(Block::Footnote { label: label.to_string(), blocks: self.blocks() })
                }
                Event::Start(Tag::HtmlBlock) => {
                    let mut html = String::new();
                    for event in self.events.by_ref() {
                        match event {
                            Event::Html(t) | Event::Text(t) => html.push_str(&t),
                            Event::End(_) => break,
                            _ => {}
                        }
                    }
                    let inline = html_inline(&html);
                    if !inline.is_empty() {
                        blocks.push(Block::Paragraph(inline));
                    }
                }
                Event::Rule => blocks.push(Block::Rule),
                Event::DisplayMath(math) => {
                    blocks.push(Block::Code { language: None, text: math.trim().to_string() })
                }
                Event::Start(Tag::MetadataBlock(_)) => self.skip_to_end(),
                // Definition lists and anything newer: read their contents as blocks.
                Event::Start(_) => blocks.extend(self.blocks()),
                _ => {}
            }
        }
        blocks
    }

    fn skip_to_end(&mut self) {
        let mut depth = 0;
        for event in self.events.by_ref() {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) if depth == 0 => break,
                Event::End(_) => depth -= 1,
                _ => {}
            }
        }
    }

    /// Inline content, then the end of its container (paragraph, heading, cell).
    fn inline_until_end(&mut self) -> Inline {
        let inline = self.inline();
        if matches!(self.events.peek(), Some(Event::End(_))) {
            self.events.next();
        }
        inline
    }

    fn inline(&mut self) -> Inline {
        let mut inline = Inline::default();
        let mut marks = Marks::default();
        let mut links: Vec<(usize, String)> = Vec::new();
        let mut image: Option<ImageRef> = None;
        while self.events.peek().is_some_and(is_inline) {
            let Some(event) = self.events.next() else { break };
            if let Some(current) = image.as_mut() {
                match event {
                    Event::Text(t) | Event::Code(t) => current.alt.push_str(&t),
                    Event::End(TagEnd::Image) => inline.images.extend(image.take()),
                    _ => {}
                }
                continue;
            }
            match event {
                Event::Text(t) => inline.push(&t, marks),
                Event::Code(t) | Event::InlineMath(t) => inline.push(&t, Marks { code: true, ..marks }),
                Event::SoftBreak => inline.push(" ", marks),
                Event::HardBreak => inline.push("\n", marks),
                Event::FootnoteReference(label) => {
                    inline.push(&format!("[{label}]"), Marks { superscript: true, ..marks })
                }
                Event::TaskListMarker(checked) => self.task = Some(checked),
                Event::InlineHtml(html) => {
                    let lower = html.to_lowercase();
                    if lower.starts_with("<br") {
                        inline.push("\n", marks);
                    } else if let Some(found) = IMG_SRC.captures(&html) {
                        let alt = IMG_ALT.captures(&html).map(|c| c[1].to_string()).unwrap_or_default();
                        inline.images.push(ImageRef { url: found[1].to_string(), alt });
                    } else if lower.starts_with("<b>") || lower.starts_with("<strong>") {
                        marks.bold = true;
                    } else if lower.starts_with("</b>") || lower.starts_with("</strong>") {
                        marks.bold = false;
                    } else if lower.starts_with("<i>") || lower.starts_with("<em>") {
                        marks.italic = true;
                    } else if lower.starts_with("</i>") || lower.starts_with("</em>") {
                        marks.italic = false;
                    } else if lower.starts_with("<code>") || lower.starts_with("<kbd>") {
                        marks.code = true;
                    } else if lower.starts_with("</code>") || lower.starts_with("</kbd>") {
                        marks.code = false;
                    }
                }
                Event::Start(Tag::Emphasis) => marks.italic = true,
                Event::End(TagEnd::Emphasis) => marks.italic = false,
                Event::Start(Tag::Strong) => marks.bold = true,
                Event::End(TagEnd::Strong) => marks.bold = false,
                Event::Start(Tag::Strikethrough) => marks.strike = true,
                Event::End(TagEnd::Strikethrough) => marks.strike = false,
                Event::Start(Tag::Superscript) => marks.superscript = true,
                Event::End(TagEnd::Superscript) => marks.superscript = false,
                Event::Start(Tag::Link { dest_url, .. }) => {
                    links.push((inline.text.len(), dest_url.to_string()));
                    marks.link = true;
                }
                Event::End(TagEnd::Link) => {
                    if let Some((start, url)) = links.pop() {
                        if start < inline.text.len() {
                            inline.links.push((start..inline.text.len(), url));
                        }
                    }
                    marks.link = !links.is_empty();
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    image = Some(ImageRef { url: dest_url.to_string(), alt: String::new() })
                }
                _ => {}
            }
        }
        inline.trim_end();
        inline
    }

    fn table(&mut self, alignments: Vec<Alignment>) -> Block {
        let mut head = Vec::new();
        let mut rows: Vec<Vec<Inline>> = Vec::new();
        let mut in_head = false;
        while let Some(event) = self.events.next() {
            match event {
                Event::Start(Tag::TableHead) => in_head = true,
                Event::End(TagEnd::TableHead) => in_head = false,
                Event::Start(Tag::TableRow) => rows.push(Vec::new()),
                Event::Start(Tag::TableCell) => {
                    let cell = self.inline_until_end();
                    if in_head {
                        head.push(cell);
                    } else if let Some(row) = rows.last_mut() {
                        row.push(cell);
                    }
                }
                Event::End(TagEnd::Table) => break,
                _ => {}
            }
        }
        Block::Table { alignments, head, rows }
    }
}

/// An HTML block shown as its text and images, without the markup.
fn html_inline(html: &str) -> Inline {
    let mut inline = Inline::default();
    for found in IMG_SRC.captures_iter(html) {
        let alt = IMG_ALT.captures(&found[0]).map(|c| c[1].to_string()).unwrap_or_default();
        inline.images.push(ImageRef { url: found[1].to_string(), alt });
    }
    let with_breaks = Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</h\d>").unwrap().replace_all(html, "\n");
    let text = TAG.replace_all(&with_breaks, "");
    let text = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(decode_entities)
        .collect::<Vec<_>>()
        .join("\n");
    inline.push(&text, Marks::default());
    inline
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

// ─── Rendering ──────────────────────────────────────────────────────────────────

const BODY_SIZE: f32 = 15.;
const CODE_SIZE: f32 = 13.;
const TABLE_SIZE: f32 = 14.;
/// The padding either side of a table cell's text.
const CELL_PADDING: f32 = 12.;
/// A column is never narrower than its longest word, unless that word is longer than this.
const MAX_WORD_WIDTH: f32 = 220.;

struct Renderer<'a> {
    colors: &'a Colors,
    syntax: SyntaxTheme,
    ui_font: SharedString,
    mono_font: SharedString,
    link: Hsla,
    /// The Markdown file's folder, for relative links and images.
    folder: PathBuf,
    workspace: WeakEntity<Workspace>,
    next_id: Cell<usize>,
    text_system: Arc<TextSystem>,
}

/// Renders `source`, the text of the Markdown file at `path`.
pub fn render(source: &str, path: &Path, workspace: WeakEntity<Workspace>, cx: &App) -> AnyElement {
    document(&parse(source), path, workspace, cx)
}

/// Lays out `blocks` like a README, with links and images relative to the file at `path`.
pub(super) fn document(blocks: &[Block], path: &Path, workspace: WeakEntity<Workspace>, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let settings = Settings::get(cx);
    let renderer = Renderer {
        colors,
        syntax: settings.syntax_theme,
        ui_font: crate::desktop::fonts().ui.clone().into(),
        mono_font: settings.editor_font_family().into(),
        link: hex(settings.syntax_theme.link(colors.dark)),
        folder: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        workspace,
        next_id: Cell::new(0),
        text_system: cx.text_system().clone(),
    };
    div()
        .id("markdown-preview")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .max_w(px(860.))
                .mx_auto()
                .px(px(44.))
                .pt(px(32.))
                .pb(px(64.))
                .flex()
                .flex_col()
                .gap(px(16.))
                .font_family(renderer.ui_font.clone())
                .text_size(px(BODY_SIZE))
                .line_height(relative(1.55))
                .text_color(colors.label)
                .children(blocks.iter().map(|block| renderer.block(block, 0))),
        )
        .into_any_element()
}

impl Renderer<'_> {
    fn id(&self, prefix: &'static str) -> gpui::ElementId {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        gpui::ElementId::NamedInteger(prefix.into(), id as u64)
    }

    fn blocks(&self, blocks: &[Block], depth: usize) -> gpui::Div {
        div().flex().flex_col().gap(px(12.)).children(blocks.iter().map(|b| self.block(b, depth)))
    }

    /// A block, held to the width of its column. A flex column rather than a plain div: taffy's
    /// block layout gave a list or quote inside it its unwrapped width, past the column's edge.
    fn block(&self, block: &Block, depth: usize) -> AnyElement {
        div().flex().flex_col().min_w_0().child(self.block_content(block, depth)).into_any_element()
    }

    fn block_content(&self, block: &Block, depth: usize) -> AnyElement {
        let colors = self.colors;
        match block {
            Block::Paragraph(inline) => self.inline(inline, colors.label),
            Block::Heading(level, inline) => {
                let (size, ruled) = match level {
                    HeadingLevel::H1 => (30., true),
                    HeadingLevel::H2 => (23., true),
                    HeadingLevel::H3 => (19., false),
                    HeadingLevel::H4 => (16., false),
                    HeadingLevel::H5 => (14.5, false),
                    HeadingLevel::H6 => (13.5, false),
                };
                let color = if *level == HeadingLevel::H6 { colors.secondary } else { colors.label };
                div()
                    .mt(px(if ruled { 10. } else { 6. }))
                    .text_size(px(size))
                    .line_height(relative(1.25))
                    .font_weight(FontWeight::BOLD)
                    .when(ruled, |d| d.pb(px(8.)).border_b_1().border_color(colors.separator))
                    .child(self.inline(inline, color))
                    .into_any_element()
            }
            Block::Code { language, text } => self.code_block(*language, text),
            Block::Quote { kind, blocks } => self.quote(*kind, blocks, depth),
            Block::List { start, items } => self.list(*start, items, depth),
            Block::Table { alignments, head, rows } => self.table(alignments, head, rows),
            Block::Footnote { label, blocks } => div()
                .flex()
                .gap(px(6.))
                .text_size(px(13.))
                .text_color(colors.secondary)
                .child(div().flex_none().child(format!("{label}.")))
                .child(self.blocks(blocks, depth).flex_1().min_w_0())
                .into_any_element(),
            Block::Rule => div().my(px(8.)).h(px(2.)).w_full().bg(colors.separator).into_any_element(),
        }
    }

    /// Styled text with clickable links, followed by its images.
    fn inline(&self, inline: &Inline, color: Hsla) -> AnyElement {
        let has_text = !inline.text.trim().is_empty();
        let images = (!inline.images.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .children(inline.images.iter().map(|image| self.image(image)))
        });
        if !has_text {
            return images.map(IntoElement::into_any_element).unwrap_or_else(|| div().into_any_element());
        }
        let runs = inline.runs.iter().map(|(len, marks)| self.run(*len, *marks, color)).collect();
        let mut text = WrappedText::new(self.id("md-text"), inline.text.clone()).with_runs(runs);
        if !inline.links.is_empty() {
            let ranges = inline.links.iter().map(|(range, _)| range.clone()).collect();
            let urls: Vec<String> = inline.links.iter().map(|(_, url)| url.clone()).collect();
            let (folder, workspace) = (self.folder.clone(), self.workspace.clone());
            text = text.on_click(ranges, move |index, window, cx| open_link(&urls[index], &folder, &workspace, window, cx));
        }
        let text = text.into_any_element();
        match images {
            Some(images) => div().flex().flex_col().gap(px(8.)).child(text).child(images).into_any_element(),
            None => text,
        }
    }

    fn run(&self, len: usize, marks: Marks, color: Hsla) -> TextRun {
        let mut run_font = font(if marks.code { self.mono_font.clone() } else { self.ui_font.clone() });
        if marks.bold {
            run_font.weight = FontWeight::BOLD;
        }
        if marks.italic {
            run_font.style = FontStyle::Italic;
        }
        let color = if marks.link {
            self.link
        } else if marks.superscript {
            self.colors.secondary
        } else {
            color
        };
        TextRun {
            len,
            font: run_font,
            color,
            background_color: marks.code.then(|| alpha(self.colors.primary, if self.colors.dark { 0.12 } else { 0.06 })),
            underline: marks.link.then(|| UnderlineStyle { thickness: px(1.), color: Some(alpha(self.link, 0.5)), wavy: false }),
            strikethrough: marks.strike.then(|| StrikethroughStyle { thickness: px(1.), color: Some(color) }),
        }
    }

    fn image(&self, image: &ImageRef) -> AnyElement {
        if let Some(path) = local_path(&image.url, &self.folder).filter(|p| p.is_file()) {
            return img(path).max_w_full().into_any_element();
        }
        // Images from the web aren't fetched: show a chip that opens them in the browser.
        let url = image.url.clone();
        let label = if image.alt.trim().is_empty() { "Image".to_string() } else { image.alt.clone() };
        div()
            .id(self.id("md-image"))
            .flex()
            .items_center()
            .gap(px(5.))
            .px(px(8.))
            .py(px(2.))
            .rounded(px(6.))
            .bg(alpha(self.colors.primary, 0.06))
            .border_1()
            .border_color(self.colors.separator)
            .text_size(px(12.))
            .text_color(self.colors.secondary)
            .cursor_pointer()
            .hover(|s| s.bg(alpha(self.colors.primary, 0.1)))
            .child(icon_view("image", 12., self.colors.secondary))
            .child(label)
            .on_click(move |_, _, cx| cx.open_url(&url))
            .into_any_element()
    }

    fn code_block(&self, language: Option<Language>, text: &str) -> AnyElement {
        let colors = self.colors;
        let mut runs = Vec::new();
        let base = |len: usize, color: Hsla| TextRun {
            len,
            font: font(self.mono_font.clone()),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut at = 0;
        if let Some(language) = language {
            for span in language.grammar().highlight(text) {
                let range = span.range.start.max(at)..span.range.end.min(text.len());
                if range.is_empty() {
                    continue;
                }
                if range.start > at {
                    runs.push(base(range.start - at, colors.label));
                }
                runs.push(base(range.len(), hex(self.syntax.color(span.kind, colors.dark))));
                at = range.end;
            }
        }
        if at < text.len() || runs.is_empty() {
            runs.push(base(text.len() - at, colors.label));
        }
        // Long lines wrap, so nothing in the preview needs scrolling sideways.
        div()
            .w_full()
            .px(px(16.))
            .py(px(12.))
            .rounded(px(8.))
            .bg(alpha(colors.primary, if colors.dark { 0.08 } else { 0.04 }))
            .border_1()
            .border_color(colors.separator)
            .text_size(px(CODE_SIZE))
            .line_height(relative(1.5))
            .child(WrappedText::new(self.id("md-code"), text.to_string()).with_runs(runs))
            .into_any_element()
    }

    fn quote(&self, kind: Option<BlockQuoteKind>, blocks: &[Block], depth: usize) -> AnyElement {
        let colors = self.colors;
        let alert = kind.map(|kind| match kind {
            BlockQuoteKind::Note => ("info", "Note", 0x0969DA, 0x4493F8),
            BlockQuoteKind::Tip => ("badge-check", "Tip", 0x1A7F37, 0x3FB950),
            BlockQuoteKind::Important => ("info", "Important", 0x8250DF, 0xAB7DF8),
            BlockQuoteKind::Warning => ("triangle-alert", "Warning", 0x9A6700, 0xD29922),
            BlockQuoteKind::Caution => ("circle-x", "Caution", 0xCF222E, 0xF85149),
        });
        let bar = match alert {
            Some((_, _, light, dark)) => hex(if colors.dark { dark } else { light }),
            None => alpha(colors.primary, 0.15),
        };
        div()
            .flex()
            .gap(px(14.))
            .child(div().w(px(4.)).flex_none().rounded_full().bg(bar))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_color(colors.secondary)
                    .when_some(alert, |d, (icon, title, _, _)| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(bar)
                                .child(icon_view(icon, 15., bar))
                                .child(title),
                        )
                    })
                    .child(self.blocks(blocks, depth)),
            )
            .into_any_element()
    }

    fn list(&self, start: Option<u64>, items: &[Item], depth: usize) -> AnyElement {
        let colors = self.colors;
        let bullet = ["•", "◦", "▪"][depth % 3];
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = match (item.task, start) {
                    (Some(checked), _) => self.checkbox(checked),
                    (None, Some(first)) => div()
                        .text_color(colors.secondary)
                        .child(format!("{}.", first + index as u64))
                        .into_any_element(),
                    (None, None) => div().text_color(colors.secondary).child(bullet).into_any_element(),
                };
                div()
                    .flex()
                    .gap(px(6.))
                    .child(div().w(px(22.)).flex_none().flex().justify_end().child(marker))
                    // Without `min_w_0` a flex item is as wide as its unwrapped text, so long
                    // lines ran off the card instead of wrapping (here, in quotes and footnotes).
                    .child(self.blocks(&item.blocks, depth + 1).flex_1().min_w_0().gap(px(6.)))
            }))
            .into_any_element()
    }

    fn checkbox(&self, checked: bool) -> AnyElement {
        let colors = self.colors;
        div()
            .mt(px(4.))
            .size(px(15.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .when(checked, |d| d.bg(colors.accent).child(icon_view("check", 11., gpui::white())))
            .when(!checked, |d| d.border_1().border_color(colors.tertiary))
            .into_any_element()
    }

    fn table(&self, alignments: &[Alignment], head: &[Inline], rows: &[Vec<Inline>]) -> AnyElement {
        let colors = self.colors;
        let columns = rows.iter().map(Vec::len).fold(alignments.len().max(head.len()), usize::max).max(1);
        let lengths: Vec<Vec<usize>> = std::iter::once(head)
            .chain(rows.iter().map(Vec::as_slice))
            .map(|cells| cells.iter().map(|cell| cell.text.chars().count()).collect())
            .collect();
        let weights = super::table::column_weights(columns, lengths.iter().map(Vec::as_slice));
        // Columns share the width by weight, but none gets so narrow that its words break
        // apart; when they can't all fit, the table scrolls sideways instead.
        let mut min_widths = vec![0f32; columns];
        for (cells, header) in std::iter::once((head, true)).chain(rows.iter().map(|r| (r.as_slice(), false))) {
            for (column, cell) in cells.iter().enumerate().take(columns) {
                let word = self.longest_word(cell, header).min(MAX_WORD_WIDTH);
                min_widths[column] = min_widths[column].max(word + CELL_PADDING * 2. + 1.);
            }
        }
        let table_min_width = min_widths.iter().sum::<f32>() + 2.;
        let row = |cells: &[Inline], header: bool, striped: bool| {
            div()
                .flex()
                .w_full()
                .when(header, |d| d.bg(alpha(colors.primary, 0.05)).font_weight(FontWeight::SEMIBOLD))
                .when(striped, |d| d.bg(alpha(colors.primary, 0.025)))
                .children((0..columns).map(|column| {
                    let cell = super::table::cell(weights[column])
                        .min_w(px(min_widths[column]))
                        .px(px(CELL_PADDING))
                        .py(px(6.))
                        .when(column > 0, |d| d.border_l_1().border_color(colors.separator));
                    let cell = match alignments.get(column) {
                        Some(Alignment::Center) => cell.text_center(),
                        Some(Alignment::Right) => cell.text_right(),
                        _ => cell,
                    };
                    match cells.get(column) {
                        Some(inline) => cell.child(self.inline(inline, colors.label)),
                        None => cell,
                    }
                }))
        };
        // A table without a header row (as HTML allows) starts straight with its rows.
        let has_head = !head.is_empty();
        let table = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(table_min_width))
            .rounded(px(8.))
            .border_1()
            .border_color(colors.separator)
            .overflow_hidden()
            .text_size(px(TABLE_SIZE))
            .when(has_head, |d| d.child(row(head, true, false)))
            .children(rows.iter().enumerate().map(|(index, cells)| {
                row(cells, false, index % 2 == 1)
                    .when(has_head || index > 0, |d| d.border_t_1().border_color(colors.separator))
            }));
        // A flex row rather than a `w_full` table: in a scroll container taffy sized the rows
        // at the full width, then shrank the table to its minimum and clipped what wrapped.
        div().id(self.id("md-table")).flex().overflow_x_scroll().child(table).into_any_element()
    }

    /// How wide the widest run of text without a space in `inline` is, in a table cell.
    fn longest_word(&self, inline: &Inline, header: bool) -> f32 {
        let size = px(TABLE_SIZE);
        let (mut longest, mut word) = (0f32, 0f32);
        let mut runs = inline.runs.iter();
        let (mut run_end, mut font_id) = (0, None);
        for (at, c) in inline.text.char_indices() {
            while at >= run_end {
                let Some((len, marks)) = runs.next() else { break };
                run_end += len;
                let mut run_font = font(if marks.code { self.mono_font.clone() } else { self.ui_font.clone() });
                if marks.bold || header {
                    run_font.weight = FontWeight::BOLD;
                }
                font_id = Some(self.text_system.resolve_font(&run_font));
            }
            if c.is_whitespace() {
                word = 0.;
                continue;
            }
            let font_id = font_id.unwrap_or_else(|| self.text_system.resolve_font(&font(self.ui_font.clone())));
            word += self.text_system.advance(font_id, size, c).map_or(TABLE_SIZE * 0.6, |a| f32::from(a.width));
            longest = longest.max(word);
        }
        longest
    }
}

/// A link or image target as a file next to the Markdown file, if it isn't a web address.
fn local_path(url: &str, folder: &Path) -> Option<PathBuf> {
    if url.contains("://") || url.starts_with("mailto:") || url.starts_with("data:") || url.starts_with('#') {
        return None;
    }
    let url = url.split(['#', '?']).next().unwrap_or(url);
    let url = url.replace("%20", " ");
    let path = Path::new(&url);
    Some(if path.is_absolute() { path.to_path_buf() } else { folder.join(path) })
}

/// Web links open in the browser; links to other files open them in the workspace.
fn open_link(url: &str, folder: &Path, workspace: &WeakEntity<Workspace>, window: &mut Window, cx: &mut App) {
    match local_path(url, folder) {
        Some(path) if path.exists() => {
            workspace.update(cx, |workspace, cx| workspace.open(&path, window, cx)).ok();
        }
        Some(_) => {}
        None if url.starts_with('#') => {}
        None => cx.open_url(url),
    }
}
