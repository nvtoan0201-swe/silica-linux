//! One open file. Text files own an editor, so each tab keeps its own undo history,
//! selection and scroll position.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{App, AppContext, Entity};

use crate::editor::Editor;
use crate::preview::PreviewFormat;
use crate::syntax::Language;

const MAX_TEXT_SIZE: u64 = 20 * 1024 * 1024;
/// For images, which are shown rather than loaded into the editor.
const MAX_IMAGE_SIZE: u64 = 200 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// UTF-8 with a byte order mark, written back with one.
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Latin1,
}

impl Encoding {
    /// e.g. "UTF-8", for the status bar.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le | Encoding::Utf16Be => "UTF-16",
            Encoding::Latin1 => "ISO Latin 1",
        }
    }

    fn decode(data: &[u8]) -> Option<(String, Encoding)> {
        if let Some(rest) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            return String::from_utf8(rest.to_vec()).ok().map(|t| (t, Encoding::Utf8Bom));
        }
        if let Some(rest) = data.strip_prefix(&[0xFF, 0xFE]) {
            let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            return String::from_utf16(&units).ok().map(|t| (t, Encoding::Utf16Le));
        }
        if let Some(rest) = data.strip_prefix(&[0xFE, 0xFF]) {
            let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            return String::from_utf16(&units).ok().map(|t| (t, Encoding::Utf16Be));
        }
        if looks_binary(data) {
            return None;
        }
        match String::from_utf8(data.to_vec()) {
            Ok(text) => Some((text, Encoding::Utf8)),
            // Not UTF-8: every byte is a Latin-1 character.
            Err(_) => Some((data.iter().map(|b| *b as char).collect(), Encoding::Latin1)),
        }
    }

    fn encode(self, text: &str) -> Option<Vec<u8>> {
        Some(match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
            Encoding::Utf16Le => {
                let mut out = vec![0xFF, 0xFE];
                out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
                out
            }
            Encoding::Utf16Be => {
                let mut out = vec![0xFE, 0xFF];
                out.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
                out
            }
            Encoding::Latin1 => {
                let mut out = Vec::with_capacity(text.len());
                for c in text.chars() {
                    out.push(u8::try_from(u32::from(c)).ok()?);
                }
                out
            }
        })
    }
}

fn looks_binary(data: &[u8]) -> bool {
    data[..data.len().min(8192)].contains(&0)
}

/// A decoded image shown read-only in the editor card.
pub struct ImageFile {
    pub width: u32,
    pub height: u32,
    pub byte_count: u64,
    /// e.g. "PNG", "JPEG", "WebP".
    pub format: String,
}

pub enum Content {
    Text(Entity<Editor>),
    Image(ImageFile),
    /// A video or PDF: Silica on Linux doesn't play or show these.
    Unsupported(&'static str),
    Binary,
    TooLarge(u64),
    Unreadable(String),
}

/// The file on disk as Silica last read or wrote it, to tell when another app changes it.
#[derive(Clone, PartialEq)]
struct DiskSnapshot {
    modified: Option<SystemTime>,
    size: u64,
    /// A hash of the bytes, for text files.
    digest: Option<u64>,
}

pub enum DiskChange {
    Unchanged,
    Modified(Option<Vec<u8>>),
    Deleted,
}

pub struct Buffer {
    pub path: PathBuf,
    pub content: Content,
    /// A new file (Ctrl+N) that hasn't been saved anywhere yet; `path` is a placeholder.
    pub untitled: bool,
    pub encoding: Encoding,
    /// The editor's version when the file was last read or written.
    saved_version: u64,
    /// The file was deleted or moved away after it was opened. The tab keeps its
    /// text; saving writes the file again.
    pub missing_on_disk: bool,
    snapshot: Option<DiskSnapshot>,
    pub preview_format: Option<PreviewFormat>,
    pub showing_preview: bool,
}

impl Buffer {
    /// An empty, untitled file, e.g. "Untitled-1".
    pub fn untitled(name: &str, cx: &mut App) -> Buffer {
        let editor = cx.new(|cx| Editor::new("", None, cx));
        Buffer {
            path: PathBuf::from(format!("untitled:{name}")),
            content: Content::Text(editor),
            untitled: true,
            encoding: Encoding::Utf8,
            saved_version: 0,
            missing_on_disk: false,
            snapshot: None,
            preview_format: None,
            showing_preview: false,
        }
    }

    pub fn open(path: &Path, cx: &mut App) -> Buffer {
        let metadata = std::fs::metadata(path).ok();
        let size = metadata.as_ref().map_or(0, |m| m.len());
        let mut buffer = Buffer {
            path: path.to_path_buf(),
            content: Content::Binary,
            untitled: false,
            encoding: Encoding::Utf8,
            saved_version: 0,
            missing_on_disk: false,
            snapshot: None,
            preview_format: PreviewFormat::for_path(path),
            showing_preview: false,
        };
        let snapshot = |digest| {
            metadata.as_ref().map(|m| DiskSnapshot { modified: m.modified().ok(), size: m.len(), digest })
        };

        if let Some(kind) = unsupported_kind(path) {
            buffer.snapshot = snapshot(None);
            buffer.content = Content::Unsupported(kind);
            return buffer;
        }
        if is_image(path) {
            buffer.snapshot = snapshot(None);
            buffer.content = if size > MAX_IMAGE_SIZE {
                Content::TooLarge(size)
            } else {
                match load_image(path, size) {
                    Some(image) => Content::Image(image),
                    None => Content::Unreadable(format!("“{}” couldn’t be decoded as an image.", file_name(path))),
                }
            };
            return buffer;
        }
        if size > MAX_TEXT_SIZE {
            buffer.snapshot = snapshot(None);
            buffer.content = Content::TooLarge(size);
            return buffer;
        }
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) => {
                buffer.content = Content::Unreadable(error.to_string());
                return buffer;
            }
        };
        buffer.snapshot = snapshot(Some(digest(&data)));
        let Some((text, encoding)) = Encoding::decode(&data) else {
            buffer.content = Content::Binary;
            return buffer;
        };
        buffer.encoding = encoding;
        let language = Language::for_path(path);
        let editor = cx.new(|cx| Editor::new(&text, language, cx));
        buffer.content = Content::Text(editor);
        buffer
    }

    pub fn name(&self) -> String {
        if self.untitled {
            return self.path.to_string_lossy().trim_start_matches("untitled:").to_string();
        }
        file_name(&self.path)
    }

    pub fn editor(&self) -> Option<&Entity<Editor>> {
        match &self.content {
            Content::Text(editor) => Some(editor),
            _ => None,
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.editor().is_some_and(|e| e.read(cx).version() != self.saved_version)
    }

    /// Whether closing it should ask to save: edited files, and untitled ones with any text.
    pub fn needs_saving(&self, cx: &App) -> bool {
        if self.untitled {
            return self.editor().is_some_and(|e| !e.read(cx).is_empty());
        }
        self.is_dirty(cx)
    }

    pub fn language(&self) -> Option<Language> {
        if self.untitled { None } else { Language::for_path(&self.path) }
    }

    /// "LF", "CRLF" or "CR".
    pub fn line_ending_name(&self, cx: &App) -> &'static str {
        match self.editor().map(|e| e.read(cx).line_ending()) {
            Some("\r\n") => "CRLF",
            Some("\r") => "CR",
            _ => "LF",
        }
    }

    /// Follows a rename or move made in Silica: same text, edits and undo history,
    /// new location (and language, if the extension changed).
    pub fn move_to(&mut self, path: &Path, cx: &mut App) {
        self.path = path.to_path_buf();
        self.untitled = false;
        self.preview_format = PreviewFormat::for_path(path);
        self.showing_preview = self.showing_preview && self.preview_format.is_some();
        let language = Language::for_path(path);
        if let Some(editor) = self.editor() {
            editor.update(cx, |editor, cx| editor.set_language(language, cx));
        }
    }

    fn encoded(&self, cx: &App) -> std::io::Result<Vec<u8>> {
        let Some(editor) = self.editor() else { return Ok(Vec::new()) };
        self.encoding.encode(&editor.read(cx).text()).ok_or_else(|| {
            std::io::Error::other(format!("The text can’t be saved as {}.", self.encoding.name()))
        })
    }

    /// Writes the text to another file (Save As), leaving this buffer as it is.
    pub fn write_to(&self, destination: &Path, cx: &App) -> std::io::Result<()> {
        let data = self.encoded(cx)?;
        write_atomically(destination, &data)
    }

    pub fn save(&mut self, cx: &App) -> std::io::Result<()> {
        if self.untitled {
            return Ok(());
        }
        let Some(editor) = self.editor() else { return Ok(()) };
        let version = editor.read(cx).version();
        let data = self.encoded(cx)?;
        write_atomically(&self.path, &data)?;
        self.snapshot = std::fs::metadata(&self.path).ok().map(|m| DiskSnapshot {
            modified: m.modified().ok(),
            size: m.len(),
            digest: Some(digest(&data)),
        });
        self.saved_version = version;
        self.missing_on_disk = false;
        Ok(())
    }

    /// Compares the file on disk with what Silica last read or wrote. A new modification
    /// date with the same bytes (a `touch`, a checkout of the same content) counts as unchanged.
    pub fn disk_change(&mut self) -> DiskChange {
        let Ok(metadata) = std::fs::metadata(&self.path) else { return DiskChange::Deleted };
        let Some(snapshot) = &self.snapshot else { return DiskChange::Modified(None) };
        if snapshot.modified == metadata.modified().ok() && snapshot.size == metadata.len() {
            return DiskChange::Unchanged;
        }
        let Some(old_digest) = snapshot.digest else { return DiskChange::Modified(None) };
        let Ok(data) = std::fs::read(&self.path) else { return DiskChange::Deleted };
        if digest(&data) == old_digest {
            self.snapshot = Some(DiskSnapshot {
                modified: metadata.modified().ok(),
                size: metadata.len(),
                digest: Some(old_digest),
            });
            return DiskChange::Unchanged;
        }
        DiskChange::Modified(Some(data))
    }

    /// Notes whether the file still exists and, for unedited text, takes in changes
    /// made by other apps. Returns `true` if the buffer can't follow the change itself
    /// (it isn't text) and should be opened again.
    pub fn sync_with_disk(&mut self, cx: &mut App) -> bool {
        if self.untitled {
            return false;
        }
        match self.disk_change() {
            DiskChange::Unchanged => {
                self.missing_on_disk = false;
                false
            }
            DiskChange::Deleted => {
                self.missing_on_disk = true;
                false
            }
            DiskChange::Modified(data) => {
                self.missing_on_disk = false;
                if self.editor().is_none() {
                    return true;
                }
                // Edited here too: keep the edits; saving asks which version wins.
                if !self.is_dirty(cx) {
                    if let Some(data) = data {
                        self.revert_to(&data, cx);
                    }
                }
                false
            }
        }
    }

    /// Replaces the text with the file on disk, discarding unsaved edits. It's one
    /// undoable change, so undo brings the edits back.
    pub fn revert_to_disk(&mut self, cx: &mut App) {
        if let Ok(data) = std::fs::read(&self.path) {
            self.revert_to(&data, cx);
        }
    }

    fn revert_to(&mut self, data: &[u8], cx: &mut App) {
        let Some((text, encoding)) = Encoding::decode(data) else { return };
        let Some(editor) = self.editor().cloned() else { return };
        self.encoding = encoding;
        let version = editor.update(cx, |editor, cx| {
            editor.replace_all(&text, cx);
            editor.version()
        });
        self.snapshot = std::fs::metadata(&self.path).ok().map(|m| DiskSnapshot {
            modified: m.modified().ok(),
            size: m.len(),
            digest: Some(digest(data)),
        });
        self.saved_version = version;
        self.missing_on_disk = false;
    }
}

fn digest(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into())
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Raster images the editor shows. SVG stays text so it can be edited.
fn is_image(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "tga" | "avif"
    )
}

fn unsupported_kind(path: &Path) -> Option<&'static str> {
    match extension(path).as_str() {
        "pdf" => Some("PDF"),
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "3gp" | "wmv" | "flv" => Some("Video"),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" => Some("Audio"),
        _ => None,
    }
}

fn load_image(path: &Path, byte_count: u64) -> Option<ImageFile> {
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let format = reader.format().map_or_else(
        || extension(path).to_uppercase(),
        |f| match f {
            image::ImageFormat::Jpeg => "JPEG".into(),
            image::ImageFormat::WebP => "WebP".into(),
            other => format!("{other:?}").to_uppercase(),
        },
    );
    let (width, height) = reader.into_dimensions().ok()?;
    Some(ImageFile { width, height, byte_count, format })
}

/// Through a symlink to its target, then a temporary file renamed over the original,
/// so a crash mid-write never leaves a half-written file.
fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let folder = target.parent().unwrap_or(Path::new("."));
    let temp = folder.join(format!(".{}.silica-tmp", file_name(&target)));
    std::fs::write(&temp, data)?;
    if let Ok(metadata) = std::fs::metadata(&target) {
        let _ = std::fs::set_permissions(&temp, metadata.permissions());
    }
    std::fs::rename(&temp, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// "1.2 MB", as Finder writes sizes.
pub fn format_bytes(bytes: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000. && unit < units.len() - 1 {
        value /= 1000.;
        unit += 1;
    }
    if value < 10. { format!("{value:.1} {}", units[unit]) } else { format!("{value:.0} {}", units[unit]) }
}
