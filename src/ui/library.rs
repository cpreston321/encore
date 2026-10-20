//! Library pages: Recently Added, Albums, Artists, Songs, playlists.

use super::*;

impl MusicApp {
    pub(crate) fn render_library_albums(
        &self,
        t: &Theme,
        title: &'static str,
        albums: Arc<Vec<Album>>,
        content_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if albums.is_empty() {
            return div()
                .child(page_title(title))
                .child(empty_state(t, "Your library is empty", "Open an album and press + to add it."))
                .into_any_element();
        }
        let (cols, tile) = grid_layout(content_w, 170., usize::MAX);
        let toolbar = (title == "Albums").then(|| {
            use crate::ui::list_tools::SortKey;
            self.list_toolbar(t, "albums", &[SortKey::Title, SortKey::Artist, SortKey::Year, SortKey::Added], cx)
        });
        if albums.len() <= VIRTUAL_GRID_MIN {
            let mut grid = grid_container();
            for (i, album) in albums.iter().enumerate() {
                grid = grid.child(self.album_tile(t, "library-album", i, album, tile, cx));
            }
            return div().child(page_title(title)).children(toolbar).child(grid).into_any_element();
        }
        // Large libraries: only the rows on screen are built each frame.
        let rows = albums.len().div_ceil(cols);
        let row_h = tile + 54. + 26.;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_none().child(page_title(title)).children(toolbar))
            .child(
                uniform_list(
                    "album-grid",
                    rows + 1, // trailing spacer keeps the last row clear of the player
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        let t = Theme::for_appearance(window.appearance());
                        range
                            .map(|row| {
                                let mut line = div().w_full().h(px(row_h)).px(px(PAD)).flex().gap(px(GAP));
                                for i in row * cols..((row + 1) * cols).min(albums.len()) {
                                    line = line.child(this.album_tile(&t, "library-album", i, &albums[i], tile, cx));
                                }
                                line.into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .map(lock_scroll_axis),
            )
            .into_any_element()
    }

    /// (name, catalog id, artwork) for the Artists page: Apple's library artist list when
    /// signed in, otherwise derived from the local library's songs.
    pub(crate) fn library_artist_rows_uncached(&self) -> Vec<(String, Option<String>, Artwork)> {
        match self.remote() {
            Some(r) if !r.artists.is_empty() => r
                .artists
                .iter()
                .map(|a| {
                    let id = (!a.id.starts_with("r.")).then(|| a.id.clone());
                    (a.name.clone(), id, a.artwork.clone())
                })
                .collect(),
            _ => crate::library::artists(&self.lib_songs(), self.lib_albums())
                .into_iter()
                .map(|(name, id, album)| (name, id, album.map(|a| a.artwork).unwrap_or_default()))
                .collect(),
        }
    }

    /// Two panes like the Music app: artist list on the left, the selected artist's
    /// albums and songs from the library on the right.
    pub(crate) fn render_library_artists(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        const LIST_W: f32 = 260.;
        let artists = self.library_artist_rows();
        if artists.is_empty() {
            return div()
                .child(page_title("Artists"))
                .child(empty_state(t, "No artists yet", "Artists of songs you add appear here."))
                .into_any_element();
        }
        let selected_ix = self
            .selected_library_artist
            .as_ref()
            .and_then(|name| artists.iter().position(|(n, _, _)| n == name))
            .unwrap_or(0);
        let (name, id, artwork) = artists[selected_ix].clone();

        let rows = artists.clone();
        let list = uniform_list(
            "library-artists",
            artists.len() + 2,
            cx.processor(move |this, range: Range<usize>, window, cx| {
                let t = Theme::for_appearance(window.appearance());
                range
                    .map(|ix| {
                        let Some((name, _, artwork)) = rows.get(ix) else {
                            return div().h(px(48.)).into_any_element();
                        };
                        let selected = ix == selected_ix;
                        let select = name.clone();
                        div()
                            .id(("library-artist", ix))
                            .w_full()
                            .h(px(48.))
                            .px_2()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap_3()
                            .when(selected, |d| d.bg(t.selected))
                            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
                            .child(this.avatar(artwork, name, 34., &t, cx))
                            .child(div().truncate().child(name.clone()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_library_artist = Some(select.clone());
                                this.artist_pane_scroll.set_offset(gpui::point(px(0.), px(0.)));
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .w(px(LIST_W))
        .h_full()
        .px_2()
        .map(lock_scroll_axis);

        let lower = name.to_lowercase();
        let by_artist = |artist: &str| {
            let a = artist.to_lowercase();
            a == lower || a.split([',', '&']).any(|part| part.trim() == lower)
        };
        let albums: Vec<Album> = self.lib_albums().iter().filter(|a| by_artist(&a.artist)).cloned().collect();
        let songs: Vec<Song> = self.lib_songs().iter().filter(|s| by_artist(&s.artist)).cloned().collect();
        let detail_w = content_w - LIST_W;
        let (_, tile) = grid_layout(detail_w, 150., usize::MAX);
        let mut grid = grid_container();
        for (i, album) in albums.iter().enumerate() {
            grid = grid.child(self.album_tile(t, "artist-library-album", i, album, tile, cx));
        }
        let play_songs = songs.clone();
        let (open_name, open_id) = (name.clone(), id.clone());

        let detail = div()
            .id("library-artist-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .map(lock_scroll_axis)
            .track_scroll(&self.artist_pane_scroll)
            .child(self.library_artist_hero(
                t,
                &name,
                &artwork,
                detail_w,
                Arc::new(play_songs),
                (open_name, open_id),
                cx,
            ))
            .when(!albums.is_empty(), |d| d.child(section_header(t, "Albums")).child(grid))
            .when(!songs.is_empty(), |d| {
                d.child(section_header(t, "Songs")).child(self.song_table(t, "artist-library-songs", songs, cx))
            })
            .child(div().h(px(110.)));

        div()
            .size_full()
            .flex()
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(t.separator)
                    .child(
                        div()
                            .px(px(PAD))
                            .pt(px(46.))
                            .pb_3()
                            .text_size(px(28.))
                            .font_weight(FontWeight::BOLD)
                            .child("Artists"),
                    )
                    .child(list.flex_1()),
            )
            .child(detail)
            .into_any_element()
    }

    pub(crate) fn render_all_playlists(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let (_, tile) = grid_layout(content_w, 170., usize::MAX);
        let mut grid = grid_container().child(
            div()
                .id("favorites-tile")
                .w(px(tile))
                .flex_none()
                .flex()
                .flex_col()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    let route = this.favorites_route();
                    this.go_root(route, window, cx)
                }))
                .child(
                    div()
                        .size(px(tile))
                        .rounded(px(8.))
                        .bg(gradient(0xfa2d48, 0xa1122c))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("star-square", tile * 0.35, white())),
                )
                .child(div().pt_2().child("Favorite Songs"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text_secondary)
                        .child(format!("{} songs", self.lib_favorites().len())),
                ),
        );
        let playlists: Vec<Playlist> =
            self.lib_playlists().into_iter().filter(|p| !self.is_apple_favorites(p)).collect();
        for (i, p) in playlists.iter().enumerate() {
            grid = grid.child(self.playlist_tile(t, "library-playlist", i, p, tile, cx));
        }
        div().child(page_title("All Playlists")).child(grid).when(playlists.is_empty(), |d| {
            d.child(
                div()
                    .px(px(PAD))
                    .pt_6()
                    .text_color(t.text_secondary)
                    .child("Open any playlist and press + to add it here."),
            )
        })
    }

    pub(crate) fn render_song_list_page(
        &self,
        t: &Theme,
        title: &'static str,
        songs: Arc<Vec<Song>>,
        empty_hint: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if songs.is_empty() {
            return div().child(page_title(title)).child(empty_state(t, "No songs yet", empty_hint)).into_any_element();
        }
        self.render_virtual_song_page(t, title, songs, cx)
    }

    /// Library-sized song lists: only the visible rows are built each frame.
    pub(crate) fn render_virtual_song_page(
        &self,
        t: &Theme,
        title: &'static str,
        list: Arc<Vec<Song>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const BOTTOM_SPACERS: usize = 2; // keep the last rows clear of the floating player
        use crate::ui::list_tools::SortKey;
        let key: &'static str = if title == "Songs" { "songs" } else { "favorites" };
        let list = self.shaped_songs(key, list);
        let rows = list.clone();
        let toolbar = self.list_toolbar(
            t,
            key,
            &[SortKey::Added, SortKey::Title, SortKey::Artist, SortKey::Album, SortKey::Duration],
            cx,
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .child(page_title(title))
                    .child(div().px(px(PAD)).pb_4().child(self.play_shuffle(t, list.clone(), cx)))
                    .child(toolbar)
                    .child(div().px(px(PAD - 8.)).child(table_header(t))),
            )
            .child(
                uniform_list(
                    ElementId::from((SharedString::from("virtual-songs"), title.len())),
                    list.len() + BOTTOM_SPACERS,
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        let t = Theme::for_appearance(window.appearance());
                        range
                            .map(|ix| {
                                if ix < rows.len() {
                                    this.table_row(&t, title, &rows, ix, cx).into_any_element()
                                } else {
                                    div().h(px(44.)).into_any_element()
                                }
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .px(px(PAD - 8.))
                .track_scroll(self.song_list_scroll.clone())
                .map(lock_scroll_axis),
            )
            .into_any_element()
    }
}
