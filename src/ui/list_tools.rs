//! Library list tools: sort and filter for Songs/Favorites/Albums, and multi-select with
//! ⌘-click / ⇧-click plus bulk actions.

use std::{rc::Rc, sync::Arc};

use gpui::{
    Context, FontWeight, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window, div, prelude::*,
    px,
};

use crate::{
    app::{Account, ContextMenu, MenuEntry, MusicApp},
    keys::matches,
    model::{Album, Song},
    textfield::TextField,
    theme::Theme,
    ui::{PAD, icon},
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum SortKey {
    /// The library's own order (recently added first).
    #[default]
    Added,
    Title,
    Artist,
    Album,
    Duration,
    Year,
}

impl SortKey {
    fn label(self) -> &'static str {
        match self {
            SortKey::Added => "Recently Added",
            SortKey::Title => "Title",
            SortKey::Artist => "Artist",
            SortKey::Album => "Album",
            SortKey::Duration => "Time",
            SortKey::Year => "Year",
        }
    }
}

/// Sort and filter state for a library list page.
#[derive(Default)]
pub struct ListView {
    pub sort: SortKey,
    pub filter: TextField,
}

pub fn sort_songs(songs: &[Song], key: SortKey, filter: &str) -> Vec<Song> {
    let words: Vec<String> = filter.to_lowercase().split_whitespace().map(str::to_string).collect();
    let mut out: Vec<Song> = songs
        .iter()
        .filter(|s| words.is_empty() || matches(&words, &format!("{} {} {}", s.title, s.artist, s.album)))
        .cloned()
        .collect();
    let lower = |s: &str| s.to_lowercase();
    match key {
        SortKey::Added => {}
        SortKey::Title => out.sort_by_cached_key(|s| lower(&s.title)),
        SortKey::Artist => out.sort_by_cached_key(|s| (lower(&s.artist), lower(&s.album), s.track_number)),
        SortKey::Album => out.sort_by_cached_key(|s| (lower(&s.album), s.track_number)),
        SortKey::Duration => out.sort_by_key(|s| std::cmp::Reverse(s.duration_ms)),
        SortKey::Year => out.sort_by(|a, b| b.release_date.cmp(&a.release_date)),
    }
    out
}

pub fn sort_albums(albums: &[Album], key: SortKey, filter: &str) -> Vec<Album> {
    let words: Vec<String> = filter.to_lowercase().split_whitespace().map(str::to_string).collect();
    let mut out: Vec<Album> = albums
        .iter()
        .filter(|a| words.is_empty() || matches(&words, &format!("{} {}", a.title, a.artist)))
        .cloned()
        .collect();
    match key {
        SortKey::Artist => out.sort_by_cached_key(|a| (a.artist.to_lowercase(), a.release_date.clone())),
        SortKey::Year => out.sort_by(|a, b| b.release_date.cmp(&a.release_date)),
        SortKey::Added => {}
        _ => out.sort_by_cached_key(|a| a.title.to_lowercase()),
    }
    out
}

impl MusicApp {
    pub(crate) fn list_view(&self, key: &'static str) -> std::cell::RefMut<'_, ListView> {
        std::cell::RefMut::map(self.list_views.borrow_mut(), |m| {
            m.entry(key).or_insert_with(|| ListView {
                // Albums default to A–Z, like the Music app; song lists to recently added.
                sort: if key == "albums" { SortKey::Title } else { SortKey::Added },
                filter: TextField::default(),
            })
        })
    }

    /// Songs for a list page after its sort and filter, cached per state.
    pub(crate) fn shaped_songs(&self, key: &'static str, songs: Arc<Vec<Song>>) -> Arc<Vec<Song>> {
        let (sort, filter) = {
            let v = self.list_view(key);
            (v.sort, v.filter.text().trim().to_string())
        };
        if sort == SortKey::Added && filter.is_empty() {
            return songs;
        }
        let cache_key = (key, sort, filter.clone(), Arc::as_ptr(&songs) as usize, songs.len());
        if let Some((k, v)) = &*self.shaped_cache.borrow()
            && *k == cache_key
        {
            return v.clone();
        }
        let shaped = Arc::new(sort_songs(&songs, sort, &filter));
        *self.shaped_cache.borrow_mut() = Some((cache_key, shaped.clone()));
        shaped
    }

    /// Library albums after the Albums page's sort and filter.
    pub(crate) fn shaped_albums(&self) -> Arc<Vec<Album>> {
        let (sort, filter) = {
            let v = self.list_view("albums");
            (v.sort, v.filter.text().trim().to_string())
        };
        if sort == SortKey::Title && filter.is_empty() {
            return self.sorted_albums();
        }
        Arc::new(sort_albums(self.lib_albums(), sort, &filter))
    }

    // Multi-select

    /// Click on a song row: plain selects one, ⌘ toggles, ⇧ extends from the anchor.
    pub(crate) fn click_song(&mut self, list: &[Song], ix: usize, cmd: bool, shift: bool) {
        let id = list[ix].id.clone();
        if shift && let Some(anchor) = self.select_anchor.filter(|a| *a < list.len()) {
            let (a, b) = (anchor.min(ix), anchor.max(ix));
            self.multi = list[a..=b].iter().map(|s| s.id.clone()).collect();
        } else if cmd {
            if let Some(pos) = self.multi.iter().position(|m| *m == id) {
                self.multi.remove(pos);
            } else {
                if self.multi.is_empty()
                    && let Some(single) = self.selected_song.clone()
                {
                    self.multi.push(single);
                }
                self.multi.push(id.clone());
            }
            self.select_anchor = Some(ix);
        } else {
            self.multi.clear();
            self.select_anchor = Some(ix);
        }
        self.selected_song = Some(id);
    }

    pub(crate) fn is_selected(&self, id: &str) -> bool {
        if self.multi.is_empty() {
            self.selected_song.as_deref() == Some(id)
        } else {
            self.multi.iter().any(|m| m == id)
        }
    }

    /// Songs a menu opened on `song` should act on: the whole selection if it's in it.
    pub(crate) fn menu_targets(&self, song: &Song, list: &[Song]) -> Vec<Song> {
        if self.multi.len() > 1 && self.multi.contains(&song.id) {
            list.iter().filter(|s| self.multi.contains(&s.id)).cloned().collect()
        } else {
            vec![song.clone()]
        }
    }

    pub(crate) fn bulk_menu(&self, songs: Vec<Song>, position: Point<Pixels>) -> ContextMenu {
        let n = songs.len();
        let entry = |label: String, icon: &'static str, action: crate::app::MenuAction| {
            Some(MenuEntry { label: SharedString::from(label), icon, action })
        };
        let (s1, s2, s3, s4) = (songs.clone(), songs.clone(), songs.clone(), songs);
        let mut entries = vec![
            entry(format!("Play {n} Songs"), "play", Rc::new(move |this, _, cx| this.play(s1.clone(), 0, false, cx))),
            entry(
                "Play Next".into(),
                "play-next",
                Rc::new(move |this, _, cx| {
                    for s in s2.iter().rev() {
                        this.play_next(s.clone(), cx);
                    }
                }),
            ),
        ];
        if self.account == Account::SignedIn {
            entries.push(entry(
                "Add to Playlist…".into(),
                "playlist",
                Rc::new(move |this, _, _| this.menu = Some(this.playlist_picker(&s3, position))),
            ));
            entries.push(entry(
                "Add to Library".into(),
                "plus",
                Rc::new(move |this, _, cx| {
                    let missing: Vec<Song> = s4.iter().filter(|s| !this.song_in_library(s)).cloned().collect();
                    for s in missing {
                        this.add_to_library("songs", s.id.clone(), format!("“{}”", s.title), cx);
                    }
                }),
            ));
        }
        entries.push(None);
        entries.push(entry(
            "Deselect".into(),
            "xmark",
            Rc::new(|this, _, cx| {
                this.multi.clear();
                cx.notify();
            }),
        ));
        ContextMenu { position, entries }
    }

    // Toolbar

    /// Filter field + sort menu shown above library lists.
    pub(crate) fn list_toolbar(
        &self,
        t: &Theme,
        key: &'static str,
        options: &[SortKey],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (sort, focused) = (self.list_view(key).sort, self.filter_focused && self.filter_target == key);
        let filter_text = {
            let v = self.list_view(key);
            v.filter.text().to_string()
        };
        let options = options.to_vec();
        let n_selected = self.multi.len();
        div()
            .px(px(PAD))
            .pb_3()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .id(SharedString::from(format!("filter-{key}")))
                    .key_context("SearchField")
                    .track_focus(&self.filter_focus)
                    .w(px(240.))
                    .h(px(30.))
                    .px_2p5()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(8.))
                    .bg(t.field)
                    .border_1()
                    .border_color(if focused { t.accent.opacity(0.55) } else { gpui::transparent_black() })
                    .child(icon("search", 12., t.text_secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .when(filter_text.is_empty(), |d| d.text_color(t.text_tertiary).child("Filter"))
                            .when(!filter_text.is_empty(), |d| d.child(filter_text.clone()))
                            .when(focused, |d| d.child(div().w(px(1.5)).h(px(14.)).bg(t.accent))),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.filter_target = key;
                            window.focus(&this.filter_focus);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("sort-{key}")))
                    .h(px(30.))
                    .px_3()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .bg(t.button)
                    .hover(|s| s.bg(t.selected))
                    .text_size(px(12.5))
                    .child(div().text_color(t.text_secondary).child("Sort:"))
                    .child(div().font_weight(FontWeight::MEDIUM).child(sort.label()))
                    .child(icon("chevron-down", 10., t.text_secondary))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            let entries = options
                                .iter()
                                .map(|&o| {
                                    Some(MenuEntry {
                                        label: o.label().into(),
                                        icon: if o == sort { "check" } else { "note" },
                                        action: Rc::new(
                                            move |this: &mut MusicApp, _: &mut Window, cx: &mut Context<MusicApp>| {
                                                this.list_view(key).sort = o;
                                                cx.notify();
                                            },
                                        ),
                                    })
                                })
                                .collect();
                            this.menu = Some(ContextMenu { position: e.position, entries });
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
            .when(n_selected > 1, |d| {
                d.child(
                    div()
                        .ml_2()
                        .text_size(px(12.5))
                        .text_color(t.text_secondary)
                        .child(format!("{n_selected} selected · Right-click or ⋯ for actions")),
                )
            })
            .into_any_element()
    }

    /// Keys for the list filter field. Returns true if used.
    pub(crate) fn filter_key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = self.filter_target;
        if k.key == "escape" || k.key == "enter" {
            if k.key == "escape" {
                self.list_view(key).filter.clear();
            }
            window.focus(&self.focus);
            cx.notify();
            return true;
        }
        let used = {
            let mut v = self.list_view(key);
            crate::keys::edit_text(&mut v.filter, k, cx)
        };
        if used {
            cx.notify();
        }
        used
    }

    pub(crate) fn album_menu(&self, album: &Album, position: Point<Pixels>) -> ContextMenu {
        let entry = |label: &str, icon: &'static str, action: crate::app::MenuAction| {
            Some(MenuEntry { label: SharedString::from(label.to_string()), icon, action })
        };
        let (a1, a2, a3, a4) = (album.id.clone(), album.id.clone(), album.id.clone(), album.clone());
        let mut entries = vec![
            entry("Play", "play", Rc::new(move |this, _, cx| this.play_album_by_id(a1.clone(), false, cx))),
            entry("Shuffle", "shuffle", Rc::new(move |this, _, cx| this.play_album_by_id(a2.clone(), true, cx))),
            entry("Play Next", "play-next", Rc::new(move |this, _, cx| this.queue_album_next(a3.clone(), cx))),
        ];
        if !self.album_in_library(album) {
            entries.push(entry(
                "Add to Library",
                "plus",
                Rc::new(move |this, _, cx| {
                    if this.account == Account::SignedIn {
                        let label = format!("“{}”", a4.title);
                        this.add_to_library("albums", a4.id.clone(), label, cx);
                    } else {
                        this.library.toggle_album(&a4, &[]);
                    }
                }),
            ));
        }
        entries.push(None);
        let (name, id) = (album.artist.clone(), album.artist_id.clone());
        entries.push(entry(
            "Go to Artist",
            "mic",
            Rc::new(move |this, _, cx| this.open_artist(name.clone(), id.clone(), cx)),
        ));
        let url = album.url.clone();
        entries.push(entry("Open in Apple Music", "open", Rc::new(move |this, _, cx| this.open_url(&url, cx))));
        ContextMenu { position, entries }
    }

    /// Fetches an album's tracks (if needed), then runs `then` with them.
    fn with_album_tracks(
        &mut self,
        id: String,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Vec<Song>, &mut Context<Self>) + 'static,
    ) {
        if let Some(crate::app::Load::Ready((_, tracks))) = self.albums.get(&id) {
            let tracks = tracks.clone();
            return then(self, tracks, cx);
        }
        let key = id.clone();
        self.fetch(
            cx,
            move |c| c.album(&id),
            move |this, load, cx| match load {
                crate::app::Load::Ready((album, tracks)) => {
                    this.albums.insert(key, crate::app::Load::Ready((album, tracks.clone())));
                    then(this, tracks, cx);
                }
                crate::app::Load::Failed(e) => this.show_notice(format!("Couldn't load the album: {e}"), cx),
                crate::app::Load::Loading => {}
            },
        );
    }

    pub(crate) fn play_album_by_id(&mut self, id: String, shuffle: bool, cx: &mut Context<Self>) {
        self.with_album_tracks(id, cx, move |this, tracks, cx| {
            let start = if shuffle { tracks.len() / 2 } else { 0 };
            this.play(tracks, start, shuffle, cx);
        });
    }

    pub(crate) fn queue_album_next(&mut self, id: String, cx: &mut Context<Self>) {
        self.with_album_tracks(id, cx, |this, tracks, cx| {
            for s in tracks.into_iter().rev() {
                this.play_next(s, cx);
            }
            this.show_notice("Album will play next".into(), cx);
        });
    }

    /// Song menu for one song, or the bulk menu when it's part of a multi-selection.
    pub(crate) fn open_song_menu(&mut self, song: &Song, list: &[Song], position: Point<Pixels>) {
        let targets = self.menu_targets(song, list);
        self.menu =
            Some(if targets.len() > 1 { self.bulk_menu(targets, position) } else { self.song_menu(song, position) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, artist: &str, ms: u32) -> Song {
        Song {
            id: title.into(),
            title: title.into(),
            artist: artist.into(),
            artist_id: None,
            artist_ids: Vec::new(),
            album: "A".into(),
            album_id: None,
            artwork: Default::default(),
            preview_url: None,
            duration_ms: ms,
            explicit: false,
            release_date: String::new(),
            track_number: 0,
            url: String::new(),
        }
    }

    #[test]
    fn sorts_and_filters() {
        let songs = vec![song("b", "Zed", 3), song("a", "Amy", 9), song("c", "amy", 1)];
        let titles = |v: Vec<Song>| v.into_iter().map(|s| s.title).collect::<Vec<_>>();
        assert_eq!(titles(sort_songs(&songs, SortKey::Title, "")), ["a", "b", "c"]);
        assert_eq!(titles(sort_songs(&songs, SortKey::Duration, "")), ["a", "b", "c"]);
        assert_eq!(titles(sort_songs(&songs, SortKey::Added, "amy")), ["a", "c"]);
    }
}

/// Songs being dragged (from a song row) toward a playlist or the queue.
#[derive(Clone)]
pub struct DraggedSongs(pub Vec<Song>);

/// The pill that follows the cursor while dragging songs.
pub struct DragPreview {
    label: SharedString,
}

impl gpui::Render for DragPreview {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::for_appearance(window.appearance());
        div()
            .px_3()
            .py_1p5()
            .rounded_full()
            .bg(t.accent)
            .text_color(gpui::white())
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .font_family(".SystemUIFont")
            .shadow_lg()
            .child(self.label.clone())
    }
}

impl MusicApp {
    /// What a drag starting on `song` carries: the whole selection if it's part of it.
    pub(crate) fn drag_payload(&self, song: &Song, list: &[Song]) -> DraggedSongs {
        DraggedSongs(self.menu_targets(song, list))
    }
}

pub fn drag_preview(d: &DraggedSongs, cx: &mut gpui::App) -> gpui::Entity<DragPreview> {
    let label = match d.0.as_slice() {
        [one] => one.title.clone(),
        many => format!("{} songs", many.len()),
    };
    cx.new(|_| DragPreview { label: label.into() })
}
