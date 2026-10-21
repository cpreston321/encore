//! The icon set, embedded in the binary. Register [`Assets`] with
//! `Application::new().with_assets(tahoe::Assets)` (or chain it from your own
//! `AssetSource`), then draw icons with [`icon`].

use std::borrow::Cow;

use gpui::{AssetSource, Hsla, SharedString, Styled, Svg, px, svg};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*]
    };
}

/// Every icon as (`icons/<name>.svg`, SVG bytes).
pub const ICONS: &[(&str, &[u8])] = icons![
    "airplay",
    "album",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "clock",
    "ellipsis",
    "heart-fill",
    "heart",
    "home",
    "lyrics",
    "mic",
    "new",
    "next",
    "note",
    "open",
    "pause",
    "person",
    "play-next",
    "play",
    "playlist",
    "playlists",
    "plus",
    "prev",
    "queue",
    "radio",
    "repeat-one",
    "repeat",
    "search",
    "shuffle",
    "speaker-loud",
    "speaker",
    "star-square",
    "xmark",
];

/// An `AssetSource` serving [`ICONS`].
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS.iter().find(|(p, _)| *p == path).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)).collect())
    }
}

/// The icon `name` (an `icons/<name>.svg` in [`ICONS`]) at `size` points.
pub fn icon(name: &'static str, size: f32, color: Hsla) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size)).flex_none().text_color(color)
}
