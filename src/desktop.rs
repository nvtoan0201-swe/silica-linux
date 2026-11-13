//! What the desktop says about fonts and light or dark mode, read once at launch.

use std::process::Command;
use std::sync::OnceLock;

pub struct Fonts {
    /// The interface font family.
    pub ui: String,
    /// The monospaced font family, for the editor.
    pub monospace: String,
}

pub fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| Fonts {
        ui: gsetting("org.gnome.desktop.interface", "font-name")
            .map(|name| strip_size(&name))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Cantarell".into()),
        monospace: gsetting("org.gnome.desktop.interface", "monospace-font-name")
            .map(|name| strip_size(&name))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "DejaVu Sans Mono".into()),
    })
}

/// Whether the desktop prefers dark mode.
pub fn prefers_dark() -> bool {
    static DARK: OnceLock<bool> = OnceLock::new();
    *DARK.get_or_init(read_prefers_dark)
}

fn read_prefers_dark() -> bool {
    gsetting("org.gnome.desktop.interface", "color-scheme")
        .map(|scheme| scheme.contains("dark"))
        .or_else(|| {
            gsetting("org.gnome.desktop.interface", "gtk-theme")
                .map(|theme| theme.to_lowercase().contains("dark"))
        })
        .unwrap_or(false)
}

fn gsetting(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings").args(["get", schema, key]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(value.trim_matches('\'').to_string())
}

/// "Ubuntu Sans Mono 11" → "Ubuntu Sans Mono".
fn strip_size(name: &str) -> String {
    let name = name.trim();
    match name.rsplit_once(' ') {
        Some((family, size)) if size.parse::<f32>().is_ok() => family.trim().to_string(),
        _ => name.to_string(),
    }
}
