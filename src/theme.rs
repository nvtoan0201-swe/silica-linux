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
    window_top: hex(0xEDEDF0),
    window_bottom: hex(0xE6E6EA),
    card: hex(0xFFFFFF),
    card_border: rgba(0x0000001A),
    card_shadow: rgba(0x00000014),
    capsule: rgba(0xFFFFFFF2),
    capsule_border: rgba(0x00000014),
    popover: rgba(0xFBFBFDFA),
    popover_border: rgba(0x0000001F),
    label: rgba(0x1D1D1FFF),
    secondary: rgba(0x3C3C4399),
    tertiary: rgba(0x3C3C4359),
    primary: hex(0x000000),
    accent: hex(0x007AFF),
    separator: rgba(0x00000017),
    text_selection: rgba(0x007AFF40),
    text_selection_unfocused: rgba(0x0000001F),
    cursor: hex(0x007AFF),
    line_number: hex(0xA1A1A6),
    current_line_number: hex(0x1D1D1F),
    indent_guide: rgba(0x00000014),
    find_match: rgba(0xFFD60A59),
    find_current: rgba(0xFFD60AD9),
    traffic_close: hex(0xFF5F57),
    traffic_minimize: hex(0xFEBC2E),
    traffic_zoom: hex(0x28C840),
    traffic_inactive: rgba(0x0000001F),
});

pub static DARK: std::sync::LazyLock<Colors> = std::sync::LazyLock::new(|| Colors {
    dark: true,
    window_top: hex(0x2A2A2D),
    window_bottom: hex(0x242427),
    card: hex(0x1B1B1D),
    card_border: rgba(0xFFFFFF14),
    card_shadow: rgba(0x00000059),
    capsule: rgba(0x323235F2),
    capsule_border: rgba(0xFFFFFF14),
    popover: rgba(0x2C2C2FFA),
    popover_border: rgba(0xFFFFFF1F),
    label: rgba(0xE8E8EDFF),
    secondary: rgba(0xEBEBF58C),
    tertiary: rgba(0xEBEBF552),
    primary: hex(0xFFFFFF),
    accent: hex(0x0A84FF),
    separator: rgba(0xFFFFFF14),
    text_selection: rgba(0x3A6EA580),
    text_selection_unfocused: rgba(0xFFFFFF1F),
    cursor: hex(0x5AA9FF),
    line_number: hex(0x5F5F66),
    current_line_number: hex(0xC8C8CE),
    indent_guide: rgba(0xFFFFFF14),
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
