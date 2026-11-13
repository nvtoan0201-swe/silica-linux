//! Icons and images built into the binary.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const FILES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/assets.rs"));

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES.iter().find(|(name, _)| *name == path).map(|(_, data)| Cow::Borrowed(*data)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

/// An icon's asset path, e.g. `icon("search")` → `icons/search.svg`.
pub fn icon(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

/// The app icon, from the built-in assets.
pub fn app_icon() -> gpui::ImageSource {
    gpui::ImageSource::Resource(gpui::Resource::Embedded("app-icon-256.png".into()))
}
