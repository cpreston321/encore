//! Browse pages: routing plus Home, New, Radio and Search.

use super::*;

impl MusicApp {
    pub(crate) fn render_route(
        &self,
        t: &Theme,
        content_w: f32,
        search_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.route.clone() {
            Route::New => self.with_browse(t, cx, |this, cx| this.render_new(t, content_w, cx)),
            Route::Home => self.with_browse(t, cx, |this, cx| this.render_home(t, content_w, cx)),
            Route::Radio => self.render_radio(t, content_w, cx).into_any_element(),
            Route::Search => self.render_search(t, content_w, search_focused, cx).into_any_element(),
            Route::Album(id) => self.render_album(t, &id, content_w, cx),
            Route::Artist(id) => self.render_artist(t, &id, content_w, cx),
            Route::Playlist(p) => self.render_playlist(t, &p, content_w, cx),
            Route::RecentlyAdded
            | Route::Albums
            | Route::Artists
            | Route::Songs
            | Route::AllPlaylists
            | Route::Favorites
                if self.remote_pending().is_some() =>
            {
                self.remote_pending().unwrap()(self, t, cx)
            }
            Route::RecentlyAdded => {
                self.render_library_albums(t, "Recently Added", Arc::new(self.lib_recently_added()), content_w, cx)
            }
            Route::Albums => self.render_library_albums(t, "Albums", self.shaped_albums(), content_w, cx),
            Route::Artists => self.render_library_artists(t, content_w, cx),
            Route::Songs => self.render_song_list_page(
                t,
                "Songs",
                self.lib_songs(),
                "Songs you add from Apple Music will show up here.",
                cx,
            ),
            Route::Favorites => self.render_song_list_page(
                t,
                "Favorite Songs",
                self.lib_favorites(),
                "Favorite songs from the ⋯ menu to see them here.",
                cx,
            ),
            Route::AllPlaylists => self.render_all_playlists(t, content_w, cx).into_any_element(),
        }
    }

    /// While signed in, library pages wait on (or report errors from) the Apple Music library.
    #[allow(clippy::type_complexity)]
    pub(crate) fn remote_pending(&self) -> Option<fn(&Self, &Theme, &mut Context<Self>) -> AnyElement> {
        if self.account != Account::SignedIn {
            return None;
        }
        match &self.remote {
            Load::Ready(_) => None,
            Load::Loading => Some(|_, t, _| loading(t).into_any_element()),
            Load::Failed(_) => Some(|this, t, cx| {
                let Load::Failed(err) = &this.remote else { unreachable!() };
                this.error_state(t, err, cx).into_any_element()
            }),
        }
    }

    /// Pages built on a virtualized list, which brings its own scrolling.
    pub(crate) fn route_scrolls_itself(&self) -> bool {
        self.remote_pending().is_none()
            && match self.route {
                Route::Songs => !self.lib_songs().is_empty(),
                Route::Favorites => !self.lib_favorites().is_empty(),
                Route::Artists => !self.library_artist_rows().is_empty(),
                Route::Albums => self.lib_albums().len() > VIRTUAL_GRID_MIN,
                Route::RecentlyAdded => self.lib_recently_added().len() > VIRTUAL_GRID_MIN,
                _ => false,
            }
    }

    pub(crate) fn with_browse(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
        f: impl FnOnce(&Self, &mut Context<Self>) -> Div,
    ) -> AnyElement {
        match &self.browse {
            Load::Ready(_) => f(self, cx).into_any_element(),
            Load::Loading => skeleton(t).into_any_element(),
            Load::Failed(err) => self.error_state(t, err, cx).into_any_element(),
        }
    }

    pub(crate) fn render_new(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> Div {
        let browse = self.browse.ready().unwrap();
        let hero_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();
        let releases = browse.new_releases();

        let mut heroes = scroll_row("heroes");
        for (i, album) in releases.iter().take(6).enumerate() {
            heroes = heroes.child(self.hero_card(t, i, album, hero_w, cx));
        }

        let mut new_albums = scroll_row("new-releases");
        for (i, album) in releases.iter().skip(6).take(20).enumerate() {
            new_albums = new_albums.child(self.album_tile(t, "new-album", i, album, 180., cx));
        }

        let mut playlists = scroll_row("new-playlists");
        for (i, playlist) in browse.top_playlists.iter().take(20).enumerate() {
            playlists = playlists.child(self.playlist_tile(t, "new-playlist", i, playlist, 180., cx));
        }

        div()
            .child(page_title("New"))
            .child(heroes)
            .child(section_header(t, "Best New Songs"))
            .child(self.song_columns(
                t,
                "best-new",
                browse.best_new_songs().into_iter().take(24).collect(),
                hero_w,
                false,
                cx,
            ))
            .child(section_header(t, "New Releases"))
            .child(new_albums)
            .child(section_header(t, "Top Playlists"))
            .child(playlists)
    }

    /// A row of mixed cards (songs, albums, artists, playlists). Songs play from `songs`.
    pub(crate) fn mixed_row(
        &self,
        t: &Theme,
        id: &'static str,
        items: &[crate::model::TopResult],
        songs: &Arc<Vec<Song>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use crate::model::TopResult;
        let mut row = scroll_row(id).pt_1();
        for (i, item) in items.iter().enumerate() {
            let (art, title, subtitle, kind): (AnyElement, String, String, &str) = match item {
                TopResult::Song(s) => {
                    (self.art(&s.artwork, 160., 8., t, cx), s.title.clone(), s.artist.clone(), "Song")
                }
                TopResult::Album(a) => {
                    (self.art(&a.artwork, 160., 8., t, cx), a.title.clone(), a.artist.clone(), "Album")
                }
                TopResult::Artist(a) => {
                    (self.avatar(&a.artwork, &a.name, 160., t, cx), a.name.clone(), String::new(), "Artist")
                }
                TopResult::Playlist(p) => {
                    (self.playlist_art(p, 160., 8., t, cx), p.name.clone(), p.curator.clone(), "Playlist")
                }
            };
            let item = item.clone();
            let songs = songs.clone();
            row =
                row.child(
                    div()
                        .id(ElementId::from((SharedString::from(id), i)))
                        .w(px(160.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .cursor_pointer()
                        .child(art)
                        .child(div().pt_2().truncate().child(title))
                        .child(div().truncate().text_size(px(12.)).text_color(t.text_secondary).child(
                            if subtitle.is_empty() { kind.to_string() } else { format!("{kind} · {subtitle}") },
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.route == Route::Search {
                                this.remember_search();
                            }
                            match &item {
                                TopResult::Song(s) => {
                                    let ix = songs.iter().position(|x| x.id == s.id).unwrap_or(0);
                                    let list = if songs.is_empty() { vec![s.clone()] } else { songs.as_ref().clone() };
                                    this.play(list, ix, false, cx);
                                }
                                TopResult::Album(a) => this.push(Route::Album(a.id.clone()), cx),
                                TopResult::Artist(a) => this.push(Route::Artist(a.id.clone()), cx),
                                TopResult::Playlist(p) => this.push(Route::Playlist(p.clone()), cx),
                            }
                        })),
                );
        }
        row
    }

    pub(crate) fn render_home(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> Div {
        let browse = self.browse.ready().unwrap();
        let col_w_personal = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();
        // Signed in: Apple's personal shelves first, the charts below.
        let mut personal = div();
        if let Some(Load::Ready(Some(home))) = &self.personal {
            if !home.recent_tracks.is_empty() {
                personal = personal.child(section_header(t, "Recently Played")).child(self.song_columns(
                    t,
                    "home-recent",
                    home.recent_tracks.clone(),
                    col_w_personal,
                    false,
                    cx,
                ));
            }
            for (n, (title, items)) in home.shelves.iter().enumerate() {
                if title == "Recently Played" && !home.recent_tracks.is_empty() {
                    continue;
                }
                let ids = [
                    "shelf-0", "shelf-1", "shelf-2", "shelf-3", "shelf-4", "shelf-5", "shelf-6", "shelf-7", "shelf-8",
                    "shelf-9",
                ];
                personal = personal.child(section_header_owned(t, title.clone())).child(self.mixed_row(
                    t,
                    ids[n.min(9)],
                    items,
                    &Arc::new(Vec::new()),
                    cx,
                ));
            }
            if !home.heavy_rotation.is_empty() {
                personal = personal.child(section_header(t, "Heavy Rotation")).child(self.mixed_row(
                    t,
                    "heavy-rotation",
                    &home.heavy_rotation,
                    &Arc::new(Vec::new()),
                    cx,
                ));
            }
        } else if matches!(self.personal, Some(Load::Loading))
            || (self.personal.is_none() && self.account == Account::SignedIn)
        {
            // Same footprint as the real first shelves, so nothing jumps when they arrive.
            personal = personal.child(skeleton_home_shelves(t));
        }
        let (cols, card_w) = grid_layout(content_w, 200., 5);
        let col_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();

        let mut picks = div().px(px(PAD)).flex().gap(px(GAP));
        for (i, playlist) in browse.top_playlists.iter().take(cols).enumerate() {
            picks = picks.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(div().text_size(px(12.)).text_color(t.text_secondary).child("Top Playlist"))
                    .child(self.playlist_tile(t, "pick", i, playlist, card_w, cx)),
            );
        }

        let mut albums = scroll_row("top-albums");
        for (i, album) in browse.top_albums.iter().take(24).enumerate() {
            albums = albums.child(self.album_tile(t, "top-album", i, album, 180., cx));
        }

        let mut page = div()
            .child(page_title("Home"))
            .child(personal)
            .child(section_header(t, if self.personal.is_some() { "Top Picks" } else { "Top Picks for You" }))
            .child(picks)
            .child(section_header(t, "Top Songs"))
            .child(self.song_columns(
                t,
                "top-songs",
                browse.top_songs.iter().take(40).cloned().collect(),
                col_w,
                true,
                cx,
            ))
            .child(section_header(t, "Top Albums"))
            .child(albums);

        let recently_added = self.lib_recently_added();
        if !recently_added.is_empty() {
            let mut recent = scroll_row("home-library");
            for (i, album) in recently_added.iter().take(20).enumerate() {
                recent = recent.child(self.album_tile(t, "home-library-album", i, album, 180., cx));
            }
            page = page.child(section_header(t, "Recently Added")).child(recent);
        }
        page
    }

    pub(crate) fn render_radio(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let stations: &[Station] = match &self.stations {
            Some(Load::Ready(stations)) => stations,
            _ => &[],
        };
        let featured = stations.first();
        let hero_url = featured.map_or("https://music.apple.com/us/radio".to_string(), |s| s.url.clone());

        let hero = div()
            .h(px(240.))
            .rounded(px(12.))
            .bg(gradient(0xfa2d48, 0x7a0b1d))
            .p_6()
            .flex()
            .items_center()
            .gap_6()
            .when_some(featured, |d, s| d.child(self.art(&s.artwork, 192., 10., t, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div().flex().child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(white())
                                .text_color(t.accent)
                                .text_size(px(11.))
                                .font_weight(FontWeight::BOLD)
                                .child("LIVE"),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(32.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(white())
                            .truncate()
                            .child(featured.map_or("Apple Music Radio".to_string(), |s| s.name.clone())),
                    )
                    .child(
                        div().max_w(px(560.)).text_size(px(14.)).text_color(white().opacity(0.85)).line_clamp(2).child(
                            match featured {
                                Some(s) if !s.description.is_empty() => s.description.clone(),
                                _ => "Live radio from Apple Music.".to_string(),
                            },
                        ),
                    )
                    .child(
                        div().text_size(px(12.)).text_color(white().opacity(0.7)).child(
                            "Apple only streams live radio inside its own apps, so listening opens Apple Music.",
                        ),
                    )
                    .child(
                        div().pt_1().flex().child(
                            button(t, "open-radio", "open", "Listen in Apple Music", white().into(), t.accent)
                                .on_click(cx.listener(move |this, _, _, cx| this.open_url(&hero_url, cx))),
                        ),
                    ),
            );

        let mut page = div().child(page_title("Radio")).child(div().px(px(PAD)).child(hero));

        if stations.len() > 1 {
            let mut row = scroll_row("live-stations");
            for (i, station) in stations.iter().enumerate().skip(1) {
                let url = station.url.clone();
                row = row.child(
                    div()
                        .id(("station", i))
                        .w(px(200.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.open_url(&url, cx)))
                        .child(self.art(&station.artwork, 200., 8., t, cx))
                        .child(div().pt_2().truncate().child(station.name.clone()))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .line_clamp(2)
                                .child(station.description.clone()),
                        ),
                );
            }
            page = page.child(section_header(t, "Live Stations")).child(row);
        }

        if let Some(browse) = self.browse.ready() {
            let (_, tile) = grid_layout(content_w, 170., usize::MAX);
            let mut grid = grid_container();
            for (i, p) in browse.top_playlists.iter().enumerate() {
                grid = grid.child(self.playlist_tile(t, "radio-playlist", i, p, tile, cx));
            }
            page = page.child(section_header(t, "Playlists to Tune Into")).child(grid);
        }
        page
    }

    pub(crate) fn render_search(
        &self,
        t: &Theme,
        content_w: f32,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = focused && !self.palette_open;
        let text = self.search_text(t, focused, "Artists, Songs, Albums, and More", 17.);

        let field = div()
            .id("search-field")
            .key_context("SearchField")
            .when(!self.palette_open, |d| d.track_focus(&self.search_focus))
            .mx(px(PAD))
            .h(px(38.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(10.))
            .bg(t.field)
            .border_2()
            .border_color(if focused { t.accent.opacity(0.55) } else { gpui::transparent_black() })
            .text_size(px(14.))
            .child(icon("search", 15., t.text_secondary))
            .child(div().flex_1().min_w_0().child(text))
            .when(!self.search.is_empty(), |d| {
                d.child(
                    div()
                        .id("search-clear")
                        .size(px(18.))
                        .rounded_full()
                        .bg(t.text_tertiary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("xmark", 10., t.window))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search.clear();
                            this.search_results = None;
                            window.focus(&this.search_focus);
                            cx.notify();
                        })),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.search_focus);
                    this.menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            );

        let mut page = div().child(page_title("Search")).child(field);
        let col_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();

        // Recent searches while the field is empty.
        if self.search.is_empty() && !self.recent_searches.is_empty() {
            let mut chips = div().px(px(PAD)).pt_4().flex().flex_wrap().gap_2();
            for (i, term) in self.recent_searches.iter().enumerate() {
                let term = term.clone();
                chips = chips.child(
                    div()
                        .id(("recent-search", i))
                        .px_3()
                        .h(px(30.))
                        .rounded_full()
                        .bg(t.button)
                        .hover(|s| s.bg(t.selected))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .cursor_pointer()
                        .child(icon("clock", 12., t.text_secondary))
                        .child(term.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.search.set(term.clone());
                            this.schedule_search(cx);
                            cx.notify();
                        })),
                );
            }
            page = page.child(chips);
        }

        // Apple's own relevance ranking first: the artist, their latest release, the hits.
        if let Some((term, Load::Ready(r))) = &self.search_results
            && term.trim() == self.search.text().trim()
            && !r.top.is_empty()
        {
            use crate::model::TopResult;
            let top_songs: Arc<Vec<Song>> = Arc::new(
                r.top.iter().filter_map(|t| if let TopResult::Song(s) = t { Some(s.clone()) } else { None }).collect(),
            );
            let row = self.mixed_row(t, "top-results", &r.top, &top_songs, cx);
            page = page.child(section_header(t, "Top Results")).child(row);
        }

        // Instant matches from the library, before Apple's catalog answers.
        let local = self.local_matches();
        if !local.is_empty() {
            page = page.child(section_header(t, "In Your Library"));
            if !local.songs.is_empty() {
                page = page.child(self.song_columns(t, "local-songs", local.songs.clone(), col_w, false, cx));
            }
            if !local.albums.is_empty() || !local.playlists.is_empty() {
                let mut row = scroll_row("local-albums").pt_2();
                for (i, album) in local.albums.iter().enumerate() {
                    row = row.child(self.album_tile(t, "local-album", i, album, 150., cx));
                }
                for (i, playlist) in local.playlists.iter().enumerate() {
                    row = row.child(self.playlist_tile(t, "local-playlist", i, playlist, 150., cx));
                }
                page = page.child(row);
            }
        }

        let Some((term, results)) = &self.search_results else {
            if let Load::Failed(err) = &self.browse {
                return page.child(self.error_state(t, err, cx));
            }
            return match self.browse.ready() {
                Some(browse) => page.child(section_header(t, "Top Songs Right Now")).child(self.song_columns(
                    t,
                    "search-top",
                    browse.top_songs.iter().take(20).cloned().collect(),
                    col_w,
                    true,
                    cx,
                )),
                None => page,
            };
        };
        let results = match results {
            Load::Loading => return page.child(div().pt_4().child(skeleton_rows(t, 8))),
            Load::Failed(err) => return page.child(self.error_state(t, err, cx)),
            Load::Ready(r) => r,
        };
        if results.songs.is_empty() && results.albums.is_empty() && results.artists.is_empty() {
            return page.child(
                div()
                    .pt(px(80.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child("No Results"))
                    .child(
                        div()
                            .text_color(t.text_secondary)
                            .child(format!("Nothing matched “{term}”. Try a new search.")),
                    ),
            );
        }

        // Results fade in as a group whenever a new search lands.
        let mut found = div();
        if !results.artists.is_empty() {
            let mut row = scroll_row("artist-results");
            for (i, artist) in results.artists.iter().enumerate() {
                let id = artist.id.clone();
                row = row.child(
                    div()
                        .id(("artist-result", i))
                        .w(px(130.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .child(self.avatar(&artist.artwork, &artist.name, 120., t, cx))
                        .child(div().max_w_full().truncate().child(artist.name.clone()))
                        .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Artist(id.clone()), cx))),
                );
            }
            found = found.child(section_header(t, "Artists")).child(row);
        }
        if !results.songs.is_empty() {
            found = found.child(section_header(t, "Songs")).child(self.song_columns(
                t,
                "song-results",
                results.songs.clone(),
                col_w,
                false,
                cx,
            ));
        }
        if !results.albums.is_empty() {
            let mut row = scroll_row("album-results");
            for (i, album) in results.albums.iter().enumerate() {
                row = row.child(self.album_tile(t, "album-result", i, album, 180., cx));
            }
            found = found.child(section_header(t, "Albums")).child(row);
        }
        page.child(found.with_animation(
            ElementId::Name(format!("results-{term}").into()),
            Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
            |el, p| el.opacity(p).mt(px((1. - p) * 8.)),
        ))
    }
}
