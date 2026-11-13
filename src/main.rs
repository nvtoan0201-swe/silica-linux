mod app;
mod assets;
mod buffer;
mod desktop;
mod editor;
mod keymap;
mod preview;
mod project;
mod settings;
mod settings_window;
mod syntax;
mod text;
mod theme;
mod ui;
mod welcome;
mod workspace;

use std::path::PathBuf;

use gpui::{App, Application};

use settings::Settings;

fn main() {
    let paths: Vec<PathBuf> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();
    Application::new().with_assets(assets::Assets).run(move |cx: &mut App| {
        cx.set_global(Settings::load());
        keymap::bind(cx);
        ui::text_field::bind_keys(cx);
        ui::menu::bind_keys(cx);
        workspace::bind_keys(cx);
        workspace::quick_open::bind_keys(cx);
        app::bind_keys(cx);
        app::watch_windows(cx);
        if paths.is_empty() {
            app::show_welcome(cx);
        } else {
            app::open_paths(&paths, cx);
        }
        cx.activate(true);
    });
}
