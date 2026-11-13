//! App-wide preferences, saved as JSON in the config folder.

use std::path::PathBuf;

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

use crate::syntax::SyntaxTheme;
use crate::text::Indentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Font size of the editor text; the line-number gutter scales with it.
    pub font_size: f32,
    /// A monospaced font family, or `None` for the system's.
    pub font_family: Option<String>,
    /// Font size of the sidebar's file tree and search results.
    pub sidebar_font_size: f32,
    /// Long lines wrap to the editor's width; off, they scroll sideways.
    pub wraps_lines: bool,
    /// Columns a tab character advances, and the indent step for new files.
    pub tab_width: usize,
    /// How files with no indentation yet are indented.
    pub indents_with_tabs: bool,
    pub shows_line_numbers: bool,
    pub shows_indent_guides: bool,
    /// An overview of the file down the editor's right edge.
    pub shows_minimap: bool,
    pub syntax_theme: SyntaxTheme,
    pub appearance: Appearance,
    /// Reopen a project's tabs where they were left.
    pub restores_tabs: bool,
    /// The file tree keeps the folders around its first visible row at the top.
    pub pins_parent_folders: bool,
    /// See-through window background (blurred where the compositor supports it).
    pub translucent_window: bool,
    pub sidebar_width: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            font_size: Self::DEFAULT_FONT_SIZE,
            font_family: None,
            sidebar_font_size: Self::DEFAULT_SIDEBAR_FONT_SIZE,
            wraps_lines: true,
            tab_width: 4,
            indents_with_tabs: false,
            shows_line_numbers: true,
            shows_indent_guides: true,
            shows_minimap: false,
            syntax_theme: SyntaxTheme::default(),
            appearance: Appearance::System,
            restores_tabs: true,
            pins_parent_folders: true,
            translucent_window: false,
            sidebar_width: 260.,
        }
    }
}

impl Global for Settings {}

impl Settings {
    pub const DEFAULT_FONT_SIZE: f32 = 13.;
    pub const FONT_SIZE_RANGE: (f32, f32) = (9., 36.);
    pub const DEFAULT_SIDEBAR_FONT_SIZE: f32 = 13.;
    pub const SIDEBAR_FONT_SIZE_RANGE: (f32, f32) = (9., 20.);
    pub const TAB_WIDTHS: [usize; 3] = [2, 4, 8];
    pub const SIDEBAR_WIDTH_RANGE: (f32, f32) = (220., 480.);

    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("silica").join("settings.json"))
    }

    pub fn load() -> Settings {
        let mut settings: Settings = Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        settings.font_size = clamp(settings.font_size.round(), Self::FONT_SIZE_RANGE);
        settings.sidebar_font_size =
            clamp(settings.sidebar_font_size.round(), Self::SIDEBAR_FONT_SIZE_RANGE);
        if !Self::TAB_WIDTHS.contains(&settings.tab_width) {
            settings.tab_width = 4;
        }
        settings
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, data);
        }
    }

    pub fn get(cx: &App) -> &Settings {
        cx.global::<Settings>()
    }

    /// Changes the settings, saves them and lets every window redraw.
    pub fn update(cx: &mut App, change: impl FnOnce(&mut Settings)) {
        let mut settings = cx.global::<Settings>().clone();
        change(&mut settings);
        settings.font_size = clamp(settings.font_size.round(), Self::FONT_SIZE_RANGE);
        settings.sidebar_font_size =
            clamp(settings.sidebar_font_size.round(), Self::SIDEBAR_FONT_SIZE_RANGE);
        settings.sidebar_width = clamp(settings.sidebar_width, Self::SIDEBAR_WIDTH_RANGE);
        if &settings != cx.global::<Settings>() {
            settings.save();
            cx.set_global(settings);
            cx.refresh_windows();
        }
    }

    /// The indentation for files that have none yet.
    pub fn default_indentation(&self) -> Indentation {
        Indentation { uses_tabs: self.indents_with_tabs, width: self.tab_width }
    }

    /// The editor font family: the chosen one, or the desktop's monospace font.
    pub fn editor_font_family(&self) -> String {
        self.font_family.clone().unwrap_or_else(|| crate::desktop::fonts().monospace.clone())
    }

    /// The width of a file icon beside sidebar text, growing with the text.
    pub fn sidebar_icon_width(&self) -> f32 {
        (self.sidebar_font_size * 16. / Self::DEFAULT_SIDEBAR_FONT_SIZE).round()
    }
}

fn clamp(value: f32, range: (f32, f32)) -> f32 {
    value.max(range.0).min(range.1)
}
