//! The window chrome's colors, in a light and a dark set, standing in for the macOS
//! system colors and Liquid Glass that Silica uses there.

use gpui::{App, Hsla, Rgba, WindowAppearance};

use crate::settings::{Appearance, Settings};

pub fn hex(value: u32) -> Hsla {
    rgba((value << 8) | 0xFF)
}

pub fn rgba(value: u32) -> Hsla {
    Rgba {
        r: ((value >> 24) & 0xFF) as f32 / 255.,
        g: ((value >> 16) & 0xFF) as f32 / 255.,
        b: ((value >> 8) & 0xFF) as f32 / 255.,
        a: (value & 0xFF) as f32 / 255.,
    }
    .into()
}

/// A color at a fraction of its opacity.
pub fn alpha(color: Hsla, opacity: f32) -> Hsla {
    Hsla { a: color.a * opacity, ..color }
}

pub struct Colors {
    pub dark: bool,
    /// The window's frosted background, top and bottom of its gradient.
    pub window_top: Hsla,
    pub window_bottom: Hsla,
    /// The editor card's glass.
    pub card: Hsla,
    pub card_border: Hsla,
    pub card_shadow: Hsla,
    /// Small glass capsules (status bar, filter field, shortcut keys).
    pub capsule: Hsla,
    pub capsule_border: Hsla,
    /// Popovers that float above everything (Quick Open, menus).
    pub popover: Hsla,
    pub popover_border: Hsla,
    pub label: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    /// `Color.primary` at a low opacity, for hover and selection fills.
    pub primary: Hsla,
    pub accent: Hsla,
    pub separator: Hsla,
    pub text_selection: Hsla,
    pub text_selection_unfocused: Hsla,
    pub cursor: Hsla,
    pub line_number: Hsla,
    pub current_line_number: Hsla,
    pub indent_guide: Hsla,
    pub find_match: Hsla,
    pub find_current: Hsla,
    pub traffic_close: Hsla,
    pub traffic_minimize: Hsla,
    pub traffic_zoom: Hsla,
    pub traffic_inactive: Hsla,
}

pub static LIGHT: std::sync::LazyLock<Colors> = std::sync::LazyLock::new(|| Colors {
    dark: false,
    window_top: hex(0xE8E6EC),
    window_bottom: hex(0xE1DFE6),
    card: rgba(0xFBFAFCE6),
    card_border: rgba(0xFFFFFFB3),
    card_shadow: rgba(0x0000001F),
    capsule: rgba(0xFFFFFFB8),
    capsule_border: rgba(0x0000000F),
    popover: rgba(0xF6F5F8F5),
    popover_border: rgba(0x0000001A),
    label: rgba(0x000000D9),
    secondary: rgba(0x3C3C4399),
    tertiary: rgba(0x3C3C434D),
    primary: hex(0x000000),
    accent: hex(0x007AFF),
    separator: rgba(0x0000001A),
    text_selection: rgba(0x007AFF40),
    text_selection_unfocused: rgba(0x0000001F),
    cursor: hex(0x1D1D1F),
    line_number: hex(0x9A9AA0),
    current_line_number: hex(0x3C3C43),
    indent_guide: rgba(0x0000001A),
    find_match: rgba(0xFFD60A59),
    find_current: rgba(0xFFD60AD9),
    traffic_close: hex(0xFF5F57),
    traffic_minimize: hex(0xFEBC2E),
    traffic_zoom: hex(0x28C840),
    traffic_inactive: rgba(0x0000001F),
});

pub static DARK: std::sync::LazyLock<Colors> = std::sync::LazyLock::new(|| Colors {
    dark: true,
    window_top: hex(0x26252A),
    window_bottom: hex(0x1E1D22),
    card: rgba(0x2E2D33E6),
    card_border: rgba(0xFFFFFF1F),
    card_shadow: rgba(0x00000066),
    capsule: rgba(0x3A393FCC),
    capsule_border: rgba(0xFFFFFF14),
    popover: rgba(0x2C2B31F5),
    popover_border: rgba(0xFFFFFF1F),
    label: rgba(0xFFFFFFD9),
    secondary: rgba(0xEBEBF599),
    tertiary: rgba(0xEBEBF54D),
    primary: hex(0xFFFFFF),
    accent: hex(0x0A84FF),
    separator: rgba(0xFFFFFF1A),
    text_selection: rgba(0x0A84FF59),
    text_selection_unfocused: rgba(0xFFFFFF26),
    cursor: hex(0xF2F2F7),
    line_number: hex(0x9C9CA2),
    current_line_number: hex(0xF2F2F7),
    indent_guide: rgba(0xFFFFFF1F),
    find_match: rgba(0xFFD60A4D),
    find_current: rgba(0xFFD60ABF),
    traffic_close: hex(0xFF5F57),
    traffic_minimize: hex(0xFEBC2E),
    traffic_zoom: hex(0x28C840),
    traffic_inactive: rgba(0xFFFFFF26),
});

/// Whether windows are drawn dark: the Settings choice, or the desktop's.
pub fn is_dark(cx: &App) -> bool {
    match Settings::get(cx).appearance {
        Appearance::Light => false,
        Appearance::Dark => true,
        Appearance::System => match cx.window_appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => true,
            WindowAppearance::Light | WindowAppearance::VibrantLight => {
                crate::desktop::prefers_dark()
            }
        },
    }
}

pub fn colors(cx: &App) -> &'static Colors {
    if is_dark(cx) { &DARK } else { &LIGHT }
}
