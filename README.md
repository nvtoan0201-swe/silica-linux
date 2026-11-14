# Silica

A lightweight, native code editor for Linux, written in Rust on top of [GPUI](https://www.gpui.rs/) — the GPU-accelerated UI framework behind Zed.

No Electron, no web view, no language-server zoo: one small binary that opens instantly, renders every frame on the GPU, and stays out of your way.

![Silica on Linux](docs/screenshot.png)

## Features

- **Native and fast** — GPUI draws the whole UI on the GPU, on both Wayland and X11. Text lives in a rope (`ropey`), so large files stay responsive.
- **Editor** — syntax highlighting, line-number gutter, indent guides, word wrap, minimap, undo/redo, move/duplicate lines, toggle comments, indent/outdent, go to line.
- **Workspace** — file explorer with create/rename/move-to-trash, tabs, and a live file watcher that keeps the tree in sync with the disk.
- **Quick Open** — fuzzy file finder that respects `.gitignore`.
- **Find in project** — regex search across the whole folder.
- **Previews** — Markdown, CSV and HTML files can switch between source and a rendered view.
- **Sessions** — reopens your last folders and tabs; recent projects on the welcome screen.
- **Settings window** — font, size, theme and editor options, stored as plain JSON in `~/.config/silica/settings.json`.

Syntax highlighting covers Rust, C/C++, Go, Python, JavaScript/TypeScript, Java, Kotlin, C#, Swift, Objective-C, Ruby, PHP, Shell, SQL, HTML, XML, CSS, JSON, YAML, TOML, Markdown, Dockerfile, Makefile and diffs.

## Build

You need a recent stable Rust toolchain (edition 2024) and the usual GPUI system libraries. On Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libvulkan-dev libfontconfig-dev libssl-dev libzstd-dev
```

Then:

```sh
git clone https://github.com/nvtoan0201-swe/silica-linux.git
cd silica-linux
cargo run --release              # welcome screen
cargo run --release -- ~/code/my-project   # open a folder (or files)
```

## Keyboard shortcuts

| Action | Shortcut |
| --- | --- |
| Quick Open | `Ctrl+P` |
| Find in project | `Ctrl+Shift+F` |
| Go to line | `Ctrl+G` |
| Toggle sidebar | `Ctrl+B` |
| Toggle preview | `Ctrl+Shift+V` |
| Move line up / down | `Alt+↑` / `Alt+↓` |
| Duplicate line up / down | `Alt+Shift+↑` / `Alt+Shift+↓` |
| Toggle word wrap | `Alt+Z` |
| Next / previous tab | `Ctrl+PgDn` / `Ctrl+PgUp` |
| Save all | `Ctrl+Alt+S` |
| Zoom in / out / reset | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |
| Settings | `Ctrl+,` |

## Project layout

```
src/
  editor/     text view, input, wrapping, undo history
  syntax/     languages, grammars, highlight theme
  workspace/  project window, tabs, explorer, Quick Open
  project/    file tree, index, fuzzy matching, watcher, sessions
  preview/    Markdown / CSV / HTML rendering
  ui/         shared widgets: window chrome, menus, text fields, icons
```

## Status

Early and evolving — Silica started as a macOS editor and this is its Rust/GPUI port for Linux. Bug reports and pull requests are welcome.
