//! Quick search (⌘K): Spotlight-style sections over the library and Apple Music.

use std::sync::Arc;

use gpui::{AnyElement, Context, FontWeight, MouseButton, SharedString, Window, div, prelude::*, px, white};

use crate::{
    app::{Load, MusicApp, Route},
    model::{Song, TopResult},
    theme::Theme,
    ui::{explicit_badge, icon},
};

use crate::keys::*;

impl MusicApp {
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_index = 0;
        self.menu = None;
        window.focus(&self.search_focus);
        cx.notify();
    }

    /// Quick-search results grouped like Spotlight: Top Result, Artists, Songs, Albums,
    /// Playlists. Apple's relevance ranking leads once it arrives; library matches show
    /// instantly before that.
    pub(crate) fn palette_sections(&self) -> Vec<(&'static str, Vec<PaletteItem>)> {
        if self.search.text().trim().is_empty() {
            let recent: Vec<PaletteItem> =
                self.recent_searches.iter().map(|t| PaletteItem::Recent(t.clone())).collect();
            return if recent.is_empty() { Vec::new() } else { vec![("Recent Searches", recent)] };
        }
        let local = self.local_matches();
        let local_songs = Arc::new(local.songs.clone());
        let catalog = match &self.search_results {
            Some((term, Load::Ready(r))) if term.trim() == self.search.text().trim() => Some(r),
            _ => None,
        };
        let mut top: Vec<PaletteItem> = Vec::new();
        let mut artists: Vec<PaletteItem> = Vec::new();
        let mut songs: Vec<PaletteItem> = Vec::new();
        let mut albums: Vec<PaletteItem> = Vec::new();
        let mut playlists: Vec<PaletteItem> = Vec::new();
        let artist_item =
            |a: &crate::model::Artist| PaletteItem::Artist(a.name.clone(), Some(a.id.clone()), a.artwork.clone());

        if let Some(r) = catalog {
            let catalog_songs: Arc<Vec<Song>> = Arc::new({
                let mut v: Vec<Song> = r
                    .top
                    .iter()
                    .filter_map(|t| if let TopResult::Song(s) = t { Some(s.clone()) } else { None })
                    .collect();
                v.extend(r.songs.iter().filter(|s| !v.iter().any(|x| x.id == s.id)).cloned().collect::<Vec<_>>());
                v
            });
            // The artist actually credited on most of the matching songs is almost always the
            // one being searched for (a different artist can share the typed spelling).
            let query = self.search.text().trim();
            let primary = |artist: &str| artist.split([',', '&']).next().unwrap_or(artist).trim().to_string();
            let mut credits: Vec<(String, usize, Option<String>)> = Vec::new();
            for s in catalog_songs.iter().take(12) {
                let name = primary(&s.artist);
                match credits.iter_mut().find(|(n, _, _)| n.eq_ignore_ascii_case(&name)) {
                    Some(entry) => {
                        entry.1 += 1;
                        if entry.2.is_none() {
                            entry.2 = s.artist_id.clone();
                        }
                    }
                    None => credits.push((name, 1, s.artist_id.clone())),
                }
            }
            credits.sort_by_key(|(_, n, _)| std::cmp::Reverse(*n));
            let dominant = credits.first().filter(|(_, n, _)| *n >= 3).cloned();
            let candidates: Vec<&crate::model::Artist> = r
                .top
                .iter()
                .filter_map(|t| if let TopResult::Artist(a) = t { Some(a) } else { None })
                .chain(r.artists.iter())
                .collect();
            let credited = dominant.as_ref().map(|(name, _, id)| {
                match candidates.iter().find(|a| a.name.eq_ignore_ascii_case(name) || id.as_ref() == Some(&a.id)) {
                    Some(a) => artist_item(a),
                    // Not in Apple's artist results: open it by name (resolved on click).
                    None => PaletteItem::Artist(name.clone(), id.clone(), Default::default()),
                }
            });
            let named = candidates.iter().find(|a| similar(&a.name, query)).map(|a| artist_item(a));
            match (credited.or(named), r.top.first()) {
                (Some(a), _) => top.push(a),
                (None, Some(TopResult::Artist(a))) => top.push(artist_item(a)),
                (None, Some(TopResult::Album(a))) => top.push(PaletteItem::Album(a.clone())),
                (None, Some(TopResult::Song(s))) => top.push(PaletteItem::Song(s.clone(), catalog_songs.clone())),
                (None, Some(TopResult::Playlist(p))) => top.push(PaletteItem::Playlist(p.clone())),
                (None, None) => {}
            }
            let top_id = |i: &PaletteItem| match i {
                PaletteItem::Artist(_, Some(id), _) => Some(id.clone()),
                PaletteItem::Album(a) => Some(a.id.clone()),
                PaletteItem::Song(s, _) => Some(s.id.clone()),
                PaletteItem::Playlist(p) => Some(p.id.clone()),
                _ => None,
            };
            let top_key = top.first().and_then(top_id);
            let top_name = match top.first() {
                Some(PaletteItem::Artist(n, _, _)) => Some(n.to_lowercase()),
                _ => None,
            };
            let not_top = |id: &str| top_key.as_deref() != Some(id);
            let mut seen_artists: Vec<String> = Vec::new();
            for a in r
                .top
                .iter()
                .filter_map(|t| if let TopResult::Artist(a) = t { Some(a) } else { None })
                .chain(r.artists.iter())
            {
                if not_top(&a.id)
                    && top_name.as_deref() != Some(&a.name.to_lowercase())
                    && !seen_artists.contains(&a.id)
                    && artists.len() < 3
                {
                    seen_artists.push(a.id.clone());
                    artists.push(artist_item(a));
                }
            }
            for s in catalog_songs.iter().filter(|s| not_top(&s.id)).take(6) {
                songs.push(PaletteItem::Song(s.clone(), catalog_songs.clone()));
            }
            // The user's own matching songs that Apple didn't list.
            for s in local.songs.iter() {
                let dup = songs.iter().any(|i| matches!(i, PaletteItem::Song(x, _) if x.title.eq_ignore_ascii_case(&s.title) && x.artist.eq_ignore_ascii_case(&s.artist)));
                if !dup && songs.len() < 8 {
                    songs.push(PaletteItem::Song(s.clone(), local_songs.clone()));
                }
            }
            let mut seen_albums: Vec<String> = Vec::new();
            for a in r
                .top
                .iter()
                .filter_map(|t| if let TopResult::Album(a) = t { Some(a) } else { None })
                .chain(r.albums.iter())
            {
                if not_top(&a.id) && !seen_albums.contains(&a.id) && albums.len() < 4 {
                    seen_albums.push(a.id.clone());
                    albums.push(PaletteItem::Album(a.clone()));
                }
            }
            for p in r
                .top
                .iter()
                .filter_map(|t| if let TopResult::Playlist(p) = t { Some(p) } else { None })
                .filter(|p| not_top(&p.id))
                .take(2)
            {
                playlists.push(PaletteItem::Playlist(p.clone()));
            }
        } else {
            artists.extend(
                local.artists.iter().take(3).map(|(n, id, a)| PaletteItem::Artist(n.clone(), id.clone(), a.clone())),
            );
            songs.extend(local.songs.iter().take(6).map(|s| PaletteItem::Song(s.clone(), local_songs.clone())));
            albums.extend(local.albums.iter().take(4).cloned().map(PaletteItem::Album));
        }
        playlists.extend(local.playlists.iter().take(3 - playlists.len().min(3)).cloned().map(PaletteItem::Playlist));

        [("Top Result", top), ("Artists", artists), ("Songs", songs), ("Albums", albums), ("Playlists", playlists)]
            .into_iter()
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }

    pub(crate) fn palette_items(&self) -> Vec<PaletteItem> {
        self.palette_sections().into_iter().flat_map(|(_, items)| items).collect()
    }

    /// ⌘↩: the selected song's or album's artist.
    pub(crate) fn palette_go_to_artist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.palette_items();
        let Some(item) = items.get(self.palette_index.min(items.len().saturating_sub(1))).cloned() else { return };
        let target = match item {
            PaletteItem::Song(s, _) => Some((s.artist, s.artist_id)),
            PaletteItem::Album(a) => Some((a.artist, a.artist_id)),
            PaletteItem::Artist(n, id, _) => Some((n, id)),
            _ => None,
        };
        if let Some((name, id)) = target {
            self.remember_search();
            self.palette_open = false;
            window.focus(&self.focus);
            self.open_artist(name, id, cx);
        }
    }

    pub(crate) fn activate_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.palette_items();
        let Some(item) = items.get(self.palette_index.min(items.len().saturating_sub(1))).cloned() else {
            return;
        };
        self.activate_item(item, window, cx);
    }

    pub(crate) fn activate_item(&mut self, item: PaletteItem, window: &mut Window, cx: &mut Context<Self>) {
        if let PaletteItem::Recent(term) = item {
            self.search.set(term);
            self.palette_index = 0;
            self.schedule_search(cx);
            cx.notify();
            return;
        }
        self.remember_search();
        self.palette_open = false;
        window.focus(&self.focus);
        match item {
            PaletteItem::Song(song, list) => {
                let ix = list.iter().position(|s| s.id == song.id).unwrap_or(0);
                self.play(list.as_ref().clone(), ix, false, cx);
            }
            PaletteItem::Album(album) => self.push(Route::Album(album.id), cx),
            PaletteItem::Artist(name, id, _) => self.open_artist(name, id, cx),
            PaletteItem::Playlist(p) => self.push(Route::Playlist(p), cx),
            PaletteItem::Recent(_) => {}
        }
        cx.notify();
    }

    pub(crate) fn render_palette(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().palette;
        if !presence.visible() {
            return None;
        }
        let p = presence.value();
        let sections = self.palette_sections();
        let total: usize = sections.iter().map(|(_, i)| i.len()).sum();
        let selected = self.palette_index.min(total.saturating_sub(1));
        let searching = matches!(&self.search_results, Some((_, Load::Loading)));
        let empty_query = self.search.text().trim().is_empty();

        let mut list = div().id("palette-results").max_h(px(460.)).overflow_y_scroll().pb_1p5().flex().flex_col();
        if total == 0 {
            list = list.child(div().px_4().py_6().text_center().text_color(t.text_secondary).child(if empty_query {
                "Search your library and Apple Music"
            } else if searching {
                "Searching…"
            } else {
                "No results"
            }));
        }
        let mut i = 0;
        for (title, items) in sections {
            list = list.child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_1()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text_tertiary)
                    .child(title),
            );
            for item in items {
                list = list.child(self.palette_row(t, i, i == selected, title == "Top Result", item, cx));
                i += 1;
            }
        }
        let hint = |keys: &'static str, what: &'static str| {
            div().flex().items_center().gap_1().child(tahoe::primitives::kbd(t, keys)).child(what)
        };
        let footer = div()
            .h(px(32.))
            .px_4()
            .flex()
            .items_center()
            .gap_4()
            .border_t_1()
            .border_color(t.separator)
            .text_size(px(11.))
            .text_color(t.text_tertiary)
            .child(hint("↑↓", "Move"))
            .child(hint("↩", "Play / Open"))
            .child(hint("⌘↩", "Go to Artist"));
        let list = div().flex().flex_col().child(list).when(total > 0, |d| d.child(footer));

        let focused = self.palette_open;
        let field = div()
            .id("palette-field")
            .key_context("SearchField")
            .track_focus(&self.search_focus)
            .h(px(52.))
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(t.separator)
            .text_size(px(17.))
            .child(icon("search", 17., t.text_secondary))
            .child(
                div().flex_1().min_w_0().child(
                    tahoe::TextInput::new(cx.entity(), TextFieldId::Search, &self.search_focus, t)
                        .placeholder("Search songs, albums, artists…")
                        .focused(focused),
                ),
            )
            .child(tahoe::primitives::kbd(t, "esc"));

        Some(
            div()
                .id("palette-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.28 * p))
                .flex()
                .justify_center()
                // Top-aligned so the panel hugs its results instead of stretching down.
                .items_start()
                .pt(px(110.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.palette_open = false;
                        window.focus(&this.focus);
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .id("palette")
                        .w(px(620.))
                        .h_auto()
                        .rounded(px(16.))
                        .glass_style(crate::ui::panel_glass())
                        .bg(t.glass)
                        .border_1()
                        .border_color(t.panel_border)
                        .shadow_lg()
                        .opacity(p)
                        .mt(px((1. - p) * -10.))
                        .flex()
                        .flex_col()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(field)
                        .child(list),
                )
                .into_any_element(),
        )
    }

    /// One quick-search row. Each kind looks different: artists get round photos and an
    /// arrow, albums a cover and arrow, songs a small cover with a hover "artist" button;
    /// the Top Result row is larger.
    pub(crate) fn palette_row(
        &self,
        t: &Theme,
        i: usize,
        is_sel: bool,
        big: bool,
        item: PaletteItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let art_size = if big { 44. } else { 36. };
        let sub = if is_sel { t.on_accent().opacity(0.8) } else { t.text_secondary };
        let (art, title, subtitle, trailing): (AnyElement, String, String, AnyElement) = match &item {
            PaletteItem::Artist(n, id, a) => (
                {
                    // Artists picked from song credits arrive without a photo; fetch it.
                    let resolved =
                        if id.is_none() && a.is_empty() { self.resolve_artist_id(n, cx) } else { id.clone() };
                    let photo = match (a.is_empty(), &resolved) {
                        (true, Some(id)) => {
                            self.ensure_artist(id, cx);
                            match self.artists.get(id) {
                                Some(Load::Ready(d)) => d.artist.artwork.clone(),
                                _ => a.clone(),
                            }
                        }
                        _ => a.clone(),
                    };
                    self.avatar(&photo, n, art_size, t, cx)
                },
                n.clone(),
                "Artist".into(),
                icon("chevron-right", 13., sub).into_any_element(),
            ),
            PaletteItem::Album(a) => (
                self.art(&a.artwork, art_size, 6., t, cx),
                a.title.clone(),
                [
                    Some("Album".to_string()),
                    Some(a.artist.clone()),
                    Some(a.year().to_string()).filter(|y| !y.is_empty()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(self.album_in_library(a), |d| {
                        d.child(div().text_size(px(11.)).text_color(sub).child("In Library"))
                    })
                    .child(icon("chevron-right", 13., sub))
                    .into_any_element(),
            ),
            PaletteItem::Song(s, _) => {
                let in_lib = self.song_in_library(s)
                    || self.lib_songs().iter().any(|l| l.title == s.title && l.artist == s.artist);
                let (artist, artist_id) = (s.artist.clone(), s.artist_id.clone());
                let group: SharedString = format!("palette-row-{i}").into();
                (
                    self.art(&s.artwork, art_size, 4., t, cx),
                    s.title.clone(),
                    format!("{} · {}", s.artist, s.album),
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(in_lib, |d| d.child(div().text_size(px(11.)).text_color(sub).child("In Library")))
                        .child(
                            div()
                                .id(("palette-artist", i))
                                .size(px(24.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .invisible()
                                .group_hover(group, |s| s.visible())
                                .hover(|s| s.bg(white().opacity(0.15)))
                                .child(icon("mic", 13., sub))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.remember_search();
                                    this.palette_open = false;
                                    window.focus(&this.focus);
                                    this.open_artist(artist.clone(), artist_id.clone(), cx);
                                })),
                        )
                        .when(big, |d| {
                            d.child(
                                div()
                                    .px_3()
                                    .h(px(26.))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .bg(if is_sel { t.on_accent() } else { t.accent })
                                    .text_color(if is_sel { t.accent } else { t.on_accent() })
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(icon("play", 10., if is_sel { t.accent } else { t.on_accent() }))
                                    .child("Play"),
                            )
                        })
                        .into_any_element(),
                )
            }
            PaletteItem::Playlist(pl) => (
                self.playlist_art(pl, art_size, 6., t, cx),
                pl.name.clone(),
                if pl.curator.is_empty() { "Playlist".into() } else { format!("Playlist · {}", pl.curator) },
                icon("chevron-right", 13., sub).into_any_element(),
            ),
            PaletteItem::Recent(term) => (
                div()
                    .size(px(art_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("clock", 15., t.text_secondary))
                    .into_any_element(),
                term.clone(),
                String::new(),
                div().into_any_element(),
            ),
        };
        let explicit = matches!(&item, PaletteItem::Song(s, _) if s.explicit);
        div()
            .id(("palette-item", i))
            .group(SharedString::from(format!("palette-row-{i}")))
            .mx_1p5()
            .px_2p5()
            .h(px(if big { 60. } else { 48. }))
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap_3()
            .when(is_sel, |d| d.bg(t.accent).text_color(t.on_accent()))
            .when(!is_sel, |d| d.hover(|s| s.bg(t.hover)))
            .child(art)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(if big { FontWeight::BOLD } else { FontWeight::MEDIUM })
                                    .text_size(px(if big { 16. } else { 13. }))
                                    .child(title),
                            )
                            .when(explicit, |d| d.child(explicit_badge(t))),
                    )
                    .when(!subtitle.is_empty(), |d| {
                        d.child(div().truncate().text_size(px(12.)).text_color(sub).child(subtitle))
                    }),
            )
            .child(trailing)
            .on_click(cx.listener(move |this, _, window, cx| this.activate_item(item.clone(), window, cx)))
            .into_any_element()
    }
}
