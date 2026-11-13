//! Icon + tint for a file, colored like Xcode's project navigator: blue folders and
//! orange Swift files, plus a hue per file family.

use std::path::Path;

use gpui::Hsla;

use crate::theme::hex;

/// A light-mode and a dark-mode shade.
#[derive(Clone, Copy)]
struct Pair(u32, u32);

impl Pair {
    fn color(self, dark: bool) -> Hsla {
        hex(if dark { self.1 } else { self.0 })
    }
}

const FOLDER: Pair = Pair(0x3B8EEA, 0x5AA5F5);
const SWIFT: Pair = Pair(0xF05138, 0xF4693F);
const BLUE: Pair = Pair(0x2F74C0, 0x5E9BEA);
const CYAN: Pair = Pair(0x0089AD, 0x3EC3E6);
const TEAL: Pair = Pair(0x1F8A8A, 0x4CC2C2);
const MINT: Pair = Pair(0x2E9C78, 0x5CC9A2);
const GREEN: Pair = Pair(0x3A9150, 0x63C27A);
const YELLOW: Pair = Pair(0xB88A00, 0xEAC04A);
const AMBER: Pair = Pair(0xC26F0F, 0xF09A45);
const RED: Pair = Pair(0xCC3D3D, 0xF06C66);
const PINK: Pair = Pair(0xC2357F, 0xF06AAE);
const PURPLE: Pair = Pair(0x8A45C4, 0xB582EE);
const INDIGO: Pair = Pair(0x5653C9, 0x8784F5);
const BROWN: Pair = Pair(0x976236, 0xC9976A);
const STEEL: Pair = Pair(0x5F7C9B, 0x92ADC9);
const NEUTRAL: Pair = Pair(0x8A8D94, 0xA2A5AC);

/// The icon name and its color.
pub fn style(path: &Path, is_dir: bool, expanded: bool, dark: bool) -> (&'static str, Hsla) {
    if is_dir {
        return (if expanded { "folder-open-fill" } else { "folder-fill" }, FOLDER.color(dark));
    }
    let code = "code-xml";
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (icon, pair) = match name.as_str() {
        "package.swift" => ("swift", SWIFT),
        "dockerfile" => ("package", BLUE),
        "makefile" => ("hammer", BROWN),
        "license" | "license.md" | "license.txt" => ("badge-check", YELLOW),
        ".gitignore" | ".gitattributes" => ("git-branch", RED),
        _ => {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            match ext.as_str() {
                "swift" => ("swift", SWIFT),
                "js" | "mjs" | "cjs" | "jsx" => ("braces", YELLOW),
                "ts" | "tsx" => ("braces", BLUE),
                "json" | "jsonc" => ("braces", AMBER),
                "md" | "markdown" | "mdx" => ("align-left", INDIGO),
                "html" | "htm" | "xml" | "svg" | "vue" => (code, PINK),
                "css" | "scss" | "sass" | "less" => ("paintbrush", PURPLE),
                "py" => (code, TEAL),
                "go" => (code, CYAN),
                "rb" | "java" => (code, RED),
                "rs" => (code, BROWN),
                "kt" | "cs" => (code, PURPLE),
                "php" => (code, INDIGO),
                "c" | "h" | "m" | "mm" | "cpp" | "hpp" => (code, BLUE),
                "sh" | "zsh" | "bash" | "fish" => ("terminal", GREEN),
                "yml" | "yaml" | "toml" | "ini" | "plist" | "xcconfig" | "env" => ("settings", STEEL),
                "png" | "jpg" | "jpeg" | "gif" | "heic" | "webp" | "ico" | "icns" => ("image", MINT),
                "pdf" => ("file-text", RED),
                "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "3gp" => ("film", PURPLE),
                "zip" | "gz" | "tar" | "rar" | "7z" => ("archive", BROWN),
                "diff" | "patch" | "rej" => ("diff", GREEN),
                "lock" | "resolved" => ("lock", NEUTRAL),
                "txt" | "log" => ("file-text", NEUTRAL),
                _ => ("file", NEUTRAL),
            }
        }
    };
    (icon, pair.color(dark))
}
