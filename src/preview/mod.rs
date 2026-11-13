//! Markdown, CSV and HTML files can switch between their source and a rendered preview.

use std::path::Path;

use crate::syntax::Language;

/// The kinds of file that can be shown rendered as well as in source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewFormat {
    Markdown,
    /// Comma-separated values, or semicolon-separated as some spreadsheets export them.
    Csv,
    /// Tab-separated values.
    Tsv,
    /// A web page, shown as a browser would show it.
    Html,
}

impl PreviewFormat {
    pub fn for_path(path: &Path) -> Option<PreviewFormat> {
        if Language::for_path(path) == Some(Language::Markdown) {
            return Some(PreviewFormat::Markdown);
        }
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        match ext.as_str() {
            "csv" => Some(PreviewFormat::Csv),
            "tsv" => Some(PreviewFormat::Tsv),
            // Not every file highlighted as HTML (Vue, Svelte) is a page.
            "html" | "htm" | "xhtml" => Some(PreviewFormat::Html),
            _ => None,
        }
    }

    /// What the source side of the preview is called, as in "Show Markdown".
    pub fn source_name(self) -> &'static str {
        match self {
            PreviewFormat::Markdown => "Markdown",
            PreviewFormat::Csv => "CSV",
            PreviewFormat::Tsv => "TSV",
            PreviewFormat::Html => "HTML",
        }
    }
}

use std::path::Path as StdPath;

use gpui::{AnyElement, Context, Entity, IntoElement, Window};

use crate::editor::Editor;
use crate::workspace::Workspace;

/// The rendered preview of the active file, in the editor card.
pub fn render(
    format: PreviewFormat,
    path: &StdPath,
    editor: Entity<Editor>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let _ = (path, editor, window);
    crate::ui::unavailable("eye", format!("{} Preview", format.source_name()), "Coming soon.", None, cx).into_any_element()
}
