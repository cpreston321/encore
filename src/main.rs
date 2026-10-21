mod anim;
mod api;
mod app;
mod audio;
mod auth;
mod keys;
mod library;
mod localhost;
mod lyrics;
mod media;
mod model;
mod native;
mod perf;
mod queue;
mod spectrum;
mod theme;
mod ui;
mod webplayer;

use gpui::{
    App, Application, Bounds, Menu, MenuItem, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions,
    point, prelude::*, px, size,
};

use app::{MusicApp, NextTrack, PlayPause, PreviousTrack, Quit, ToggleQueue, VolumeDown, VolumeUp};

fn main() {
    perf::init();
    // ENCORE_APPEARANCE=light|dark and ENCORE_REDUCE_MOTION=1, for checking both without
    // changing the system settings.
    match std::env::var("ENCORE_APPEARANCE").as_deref() {
        Ok("light") => theme::force_appearance(Some(theme::Appearance::Light)),
        Ok("dark") => theme::force_appearance(Some(theme::Appearance::Dark)),
        _ => {}
    }
    if std::env::var_os("ENCORE_REDUCE_MOTION").is_some() {
        anim::force_reduce_motion();
    }
    api::migrate_legacy_dirs();
    Application::new().with_assets(tahoe::Assets).run(|cx: &mut App| {
        keys::bind(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![
            Menu { name: "Encore".into(), items: vec![MenuItem::action("Quit Encore", Quit)] },
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
                    MenuItem::separator(),
                    MenuItem::action("Animate Artwork", keys::ToggleAnimatedArtwork),
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
                app_id: Some("io.github.cpreston321.encore".into()),
                ..Default::default()
            },
            |window, cx| {
                tahoe::chrome::use_tahoe_corners_with_identifier(window, "io.github.cpreston321.encore.main");
                let view = cx.new(|cx| MusicApp::new(window, cx));
                let mini_app = view.clone();
                cx.on_action(move |_: &keys::ToggleMiniPlayer, cx| ui::mini::toggle(mini_app.clone(), cx));
                window.focus(&view.read(cx).focus);
                view
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
