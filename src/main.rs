mod anim;
mod api;
mod browse;
mod app;
mod auth;
mod chrome;
mod hero;
mod keys;
mod audio;
mod library;
mod localhost;
mod lyrics;
mod media;
mod mini;
mod nowplaying;
mod playlists;
mod model;
mod perf;
mod progress;
mod queue;
mod spectrum;
mod textfield;
mod theme;
mod views;
mod webplayer;

use std::borrow::Cow;

use gpui::{
    App, Application, AssetSource, Bounds, Menu, MenuItem, SharedString, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, point, prelude::*, px, size,
};

use app::{MusicApp, NextTrack, PlayPause, PreviousTrack, Quit, ToggleQueue, VolumeDown, VolumeUp};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*]
    };
}

const ICONS: &[(&str, &[u8])] = icons![
    "airplay", "album", "check", "chevron-down", "chevron-left", "chevron-right", "clock", "ellipsis", "heart",
    "heart-fill", "home", "lyrics", "mic", "new", "next", "note", "open",
    "pause", "person", "play", "play-next", "playlist", "playlists", "plus", "prev",
    "queue", "radio", "repeat", "repeat-one", "search", "shuffle", "speaker", "speaker-loud",
    "star-square", "xmark",
];

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

fn main() {
    perf::init();
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        keys::bind(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![
            Menu {
                name: "Encore".into(),
                items: vec![MenuItem::action("Quit Encore", Quit)],
            },
            Menu {
                name: "Controls".into(),
                items: vec![
                    MenuItem::action("Play / Pause", PlayPause),
                    MenuItem::action("Next", NextTrack),
                    MenuItem::action("Previous", PreviousTrack),
                    MenuItem::action("Go to Current Song", keys::ShowNowPlaying),
                    MenuItem::separator(),
                    MenuItem::action("Increase Volume", VolumeUp),
                    MenuItem::action("Decrease Volume", VolumeDown),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Home", keys::GoHome),
                    MenuItem::action("New", keys::GoNew),
                    MenuItem::action("Radio", keys::GoRadio),
                    MenuItem::separator(),
                    MenuItem::action("Recently Added", keys::GoRecentlyAdded),
                    MenuItem::action("Artists", keys::GoArtists),
                    MenuItem::action("Albums", keys::GoAlbums),
                    MenuItem::action("Songs", keys::GoSongs),
                    MenuItem::action("All Playlists", keys::GoPlaylists),
                    MenuItem::action("Favorite Songs", keys::GoFavorites),
                    MenuItem::separator(),
                    MenuItem::action("Quick Search", keys::QuickSearch),
                    MenuItem::action("Refresh Library", keys::RefreshLibrary),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![
                    MenuItem::action("Now Playing", keys::ToggleNowPlaying),
                    MenuItem::action("Lyrics", keys::ToggleLyrics),
                    MenuItem::action("Mini Player", keys::ToggleMiniPlayer),
                    MenuItem::separator(),
                    MenuItem::action("Playing Next", ToggleQueue),
                    MenuItem::action("Keyboard Shortcuts", keys::ShowShortcuts),
                ],
            },
        ]);

        cx.set_dock_menu(vec![
            MenuItem::action("Play / Pause", PlayPause),
            MenuItem::action("Next", NextTrack),
            MenuItem::action("Previous", PreviousTrack),
            MenuItem::separator(),
            MenuItem::action("Quick Search", keys::QuickSearch),
            MenuItem::action("Mini Player", keys::ToggleMiniPlayer),
        ]);
        perf::milestone("gpui ready");
        let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Encore".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(22.), px(22.))),
                }),
                window_background: WindowBackgroundAppearance::Opaque,
                window_min_size: Some(size(px(960.), px(600.))),
                app_id: Some("dev.gpui.music".into()),
                ..Default::default()
            },
            |window, cx| {
                chrome::use_tahoe_corners(window);
                let view = cx.new(|cx| MusicApp::new(window, cx));
                let mini_app = view.clone();
                cx.on_action(move |_: &keys::ToggleMiniPlayer, cx| mini::toggle(mini_app.clone(), cx));
                window.focus(&view.read(cx).focus);
                view
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
