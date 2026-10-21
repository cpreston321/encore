//! Keyboard: shortcut bindings and actions, list navigation, search-field editing, and
//! library matching for search. Quick search and the shortcuts sheet are drawn in
//! `ui::palette` and `ui::shortcuts`.

use std::sync::Arc;

use gpui::{App, Context, KeyBinding, Keystroke, ScrollStrategy, Window, actions, prelude::*};

use crate::{
    app::{Account, Load, MusicApp, Route},
    model::{Album, Artwork, Playlist, Song},
};

actions!(
    music_keys,
    [
        GoForward,
        ToggleAnimatedArtwork,
        ShowNowPlaying,
        RefreshLibrary,
        ShowShortcuts,
        QuickSearch,
        GoHome,
        GoNew,
        GoRadio,
        GoRecentlyAdded,
        GoArtists,
        GoAlbums,
        GoSongs,
        GoPlaylists,
        GoFavorites,
        ToggleMiniPlayer,
        ToggleNowPlaying,
        ToggleLyrics,
    ]
);

/// Bindings that would otherwise steal editing keys from the search field (⌘←, ⌘→…) are
/// limited to when it isn't focused.
const OUTSIDE_FIELD: Option<&str> = Some("MusicApp && !SearchField");

pub fn bind(cx: &mut App) {
    use crate::app::{FocusSearch, GoBack, NextTrack, PreviousTrack, Quit, ToggleQueue, VolumeDown, VolumeUp};
    cx.bind_keys([
        KeyBinding::new("cmd-right", NextTrack, OUTSIDE_FIELD),
        KeyBinding::new("cmd-left", PreviousTrack, OUTSIDE_FIELD),
        KeyBinding::new("cmd-up", VolumeUp, None),
        KeyBinding::new("cmd-down", VolumeDown, None),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("cmd-k", QuickSearch, None),
        KeyBinding::new("cmd-[", GoBack, None),
        KeyBinding::new("cmd-]", GoForward, None),
        KeyBinding::new("cmd-l", ShowNowPlaying, None),
        KeyBinding::new("cmd-r", RefreshLibrary, None),
        KeyBinding::new("cmd-/", ShowShortcuts, None),
        KeyBinding::new("cmd-alt-u", ToggleQueue, None),
        KeyBinding::new("cmd-1", GoHome, None),
        KeyBinding::new("cmd-2", GoNew, None),
        KeyBinding::new("cmd-3", GoRadio, None),
        KeyBinding::new("cmd-4", GoRecentlyAdded, None),
        KeyBinding::new("cmd-5", GoArtists, None),
        KeyBinding::new("cmd-6", GoAlbums, None),
        KeyBinding::new("cmd-7", GoSongs, None),
        KeyBinding::new("cmd-8", GoPlaylists, None),
        KeyBinding::new("cmd-9", GoFavorites, None),
        KeyBinding::new("cmd-ctrl-f", ToggleNowPlaying, None),
        KeyBinding::new("cmd-shift-l", ToggleLyrics, None),
        KeyBinding::new("cmd-shift-m", ToggleMiniPlayer, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}

/// The text fields `MusicApp` owns, for [`tahoe::TextInput`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextFieldId {
    Search,
    /// A library list's filter, by list key.
    Filter(&'static str),
    NewPlaylistName,
    SetupTeamId,
    SetupKeyId,
}

impl tahoe::TextFieldHost for MusicApp {
    type Field = TextFieldId;

    fn text_field<R>(&mut self, field: TextFieldId, edit: impl FnOnce(&mut tahoe::TextField) -> R) -> Option<R> {
        match field {
            TextFieldId::Search => Some(edit(&mut self.search)),
            TextFieldId::Filter(key) => Some(edit(&mut self.list_view(key).filter)),
            TextFieldId::NewPlaylistName => self.new_playlist.as_mut().map(|sheet| edit(&mut sheet.name)),
            TextFieldId::SetupTeamId => Some(edit(&mut self.key_setup.team_id)),
            TextFieldId::SetupKeyId => Some(edit(&mut self.key_setup.key_id)),
        }
    }

    fn text_field_changed(&mut self, field: TextFieldId, _: &mut Window, cx: &mut Context<Self>) {
        if field == TextFieldId::Search {
            self.search_edited(cx);
        }
    }
}

/// What ⌘K results and the Search page's "In Your Library" section show.
#[derive(Default)]
pub struct LocalMatches {
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<(String, Option<String>, Artwork)>,
    pub playlists: Vec<Playlist>,
}

impl LocalMatches {
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty() && self.albums.is_empty() && self.artists.is_empty() && self.playlists.is_empty()
    }
}

#[derive(Clone)]
pub enum PaletteItem {
    Song(Song, Arc<Vec<Song>>),
    Album(Album),
    Artist(String, Option<String>, Artwork),
    Playlist(Playlist),
    Recent(String),
}

/// Loose name match: letters and digits only, case-insensitive, within a couple of edits
/// ("big x the plug" ≈ "BigXthaPlug").
pub fn similar(name: &str, query: &str) -> bool {
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<Vec<_>>();
    let (a, b) = (norm(name), norm(query));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b || (b.len() >= 4 && a.starts_with(&b[..])) {
        return true;
    }
    // Levenshtein distance, early-out when lengths differ a lot.
    if a.len().abs_diff(b.len()) > 2 {
        return false;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()] <= 2.max(b.len() / 6)
}

/// Every word of the query appears somewhere in the haystack (case-insensitive).
pub fn matches(query_words: &[String], haystack: &str) -> bool {
    let hay = haystack.to_lowercase();
    query_words.iter().all(|w| hay.contains(w.as_str()))
}

impl MusicApp {
    // Navigation shortcuts

    pub(crate) fn register_key_actions<E: InteractiveElement>(&self, el: E, cx: &mut Context<Self>) -> E {
        el.on_action(cx.listener(|this, _: &GoHome, w, cx| this.go_root(Route::Home, w, cx)))
            .on_action(cx.listener(|this, _: &GoNew, w, cx| this.go_root(Route::New, w, cx)))
            .on_action(cx.listener(|this, _: &GoRadio, w, cx| this.go_root(Route::Radio, w, cx)))
            .on_action(cx.listener(|this, _: &GoRecentlyAdded, w, cx| this.go_root(Route::RecentlyAdded, w, cx)))
            .on_action(cx.listener(|this, _: &GoArtists, w, cx| this.go_root(Route::Artists, w, cx)))
            .on_action(cx.listener(|this, _: &GoAlbums, w, cx| this.go_root(Route::Albums, w, cx)))
            .on_action(cx.listener(|this, _: &GoSongs, w, cx| this.go_root(Route::Songs, w, cx)))
            .on_action(cx.listener(|this, _: &GoPlaylists, w, cx| this.go_root(Route::AllPlaylists, w, cx)))
            .on_action(cx.listener(|this, _: &GoFavorites, w, cx| {
                let route = this.favorites_route();
                this.go_root(route, w, cx)
            }))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.forward(cx)))
            .on_action(cx.listener(|this, _: &ShowNowPlaying, _, cx| {
                if let Some(album) = this.queue.current().and_then(|s| s.album_id.clone()) {
                    this.push(Route::Album(album), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &RefreshLibrary, _, cx| {
                if this.account == Account::SignedIn {
                    this.load_remote_library(cx);
                    this.show_notice("Refreshing your library…".into(), cx);
                } else {
                    this.load_browse(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowShortcuts, _, cx| {
                this.show_shortcuts = !this.show_shortcuts;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &QuickSearch, window, cx| this.open_palette(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleNowPlaying, _, cx| {
                if this.show_now_playing {
                    this.show_now_playing = false;
                    cx.notify();
                } else {
                    this.open_now_playing(None, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleLyrics, _, cx| {
                use crate::ui::now_playing::NowPlayingTab;
                if this.show_now_playing && this.now_playing_tab == NowPlayingTab::Lyrics {
                    this.show_now_playing = false;
                    cx.notify();
                } else {
                    this.open_now_playing(Some(NowPlayingTab::Lyrics), cx);
                }
            }))
    }

    /// Esc closes the topmost thing that's open. Returns false if nothing was.
    pub(crate) fn dismiss_top(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let closed = if self.show_welcome {
            self.finish_welcome(cx);
            true
        } else if self.palette_open {
            self.palette_open = false;
            window.focus(&self.focus);
            true
        } else if self.show_now_playing {
            self.show_now_playing = false;
            true
        } else if self.show_shortcuts {
            self.show_shortcuts = false;
            true
        } else if self.menu.take().is_some() {
            true
        } else if self.show_account || self.show_volume {
            self.show_account = false;
            self.show_volume = false;
            true
        } else if self.show_queue {
            self.show_queue = false;
            true
        } else {
            false
        };
        if closed {
            cx.notify();
        }
        closed
    }

    // Search field

    /// Edits the search query. Returns true if the key was used.
    pub(crate) fn search_key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (cmd, alt) = (k.modifiers.platform, k.modifiers.alt);
        let before = self.search.text().to_string();
        match (k.key.as_str(), cmd, alt) {
            ("escape", ..) => {
                if self.palette_open || self.search.is_empty() {
                    self.palette_open = false;
                    window.focus(&self.focus);
                } else {
                    self.search.clear();
                }
            }
            ("up", ..) if self.palette_open => self.palette_index = self.palette_index.saturating_sub(1),
            ("down", ..) if self.palette_open => self.palette_index += 1,
            ("enter", true, _) if self.palette_open => {
                self.palette_go_to_artist(window, cx);
                return true;
            }
            ("enter", ..) if self.palette_open => {
                self.activate_palette(window, cx);
                return true;
            }
            ("enter", ..) => self.remember_search(),
            // ↓ leaves the field and walks the results.
            ("down", ..) => {
                window.focus(&self.focus);
                self.move_selection(1, cx);
            }
            _ => {
                if !self.search.handle_key(k, window, cx) {
                    return false;
                }
            }
        }
        if self.search.text() != before {
            self.search_edited(cx);
        }
        cx.notify();
        true
    }

    /// After the query changes, from a key or the input handler.
    pub(crate) fn search_edited(&mut self, cx: &mut Context<Self>) {
        if self.search.is_composing() {
            return;
        }
        self.palette_index = 0;
        if !self.palette_open && self.route != Route::Search {
            self.history.clear();
            self.go_search_page(cx);
        }
        self.schedule_search(cx);
    }

    pub(crate) fn remember_search(&mut self) {
        let term = self.search.text().trim().to_string();
        if term.chars().count() < 2 {
            return;
        }
        self.recent_searches.retain(|t| !t.eq_ignore_ascii_case(&term));
        self.recent_searches.insert(0, term);
        self.recent_searches.truncate(8);
        self.save_settings();
    }

    /// Instant matches from the user's own library, cached per query.
    pub(crate) fn local_matches(&self) -> Arc<LocalMatches> {
        let query = self.search.text().trim().to_lowercase();
        let key = (query.clone(), self.library_key());
        if let Some((k, v)) = &*self.local_matches_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let words: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        let found = if words.is_empty() {
            LocalMatches::default()
        } else {
            LocalMatches {
                songs: self
                    .lib_songs()
                    .iter()
                    .filter(|s| matches(&words, &format!("{} {} {}", s.title, s.artist, s.album)))
                    .take(12)
                    .cloned()
                    .collect(),
                albums: self
                    .lib_albums()
                    .iter()
                    .filter(|a| matches(&words, &format!("{} {}", a.title, a.artist)))
                    .take(12)
                    .cloned()
                    .collect(),
                artists: self
                    .library_artist_rows()
                    .iter()
                    .filter(|(n, _, _)| matches(&words, n))
                    .take(8)
                    .cloned()
                    .collect(),
                playlists: self.lib_playlists().into_iter().filter(|p| matches(&words, &p.name)).take(8).collect(),
            }
        };
        let found = Arc::new(found);
        *self.local_matches_cache.borrow_mut() = Some((key, found.clone()));
        found
    }

    // List navigation

    /// The songs ↑/↓ walk on the current page.
    pub(crate) fn page_songs(&self) -> Option<Arc<Vec<Song>>> {
        match &self.route {
            Route::Songs => Some(self.lib_songs()),
            Route::Favorites => Some(self.lib_favorites()),
            Route::Album(id) => self.albums.get(id)?.ready().map(|(_, t)| Arc::new(t.clone())),
            Route::Playlist(p) => match self.playlist_tracks.get(&p.id) {
                Some(Load::Ready(Some(t))) => Some(Arc::new(t.clone())),
                _ => None,
            },
            Route::Artist(id) => self.artists.get(id)?.ready().map(|d| Arc::new(d.top_songs.clone())),
            Route::Search => {
                let local = self.local_matches();
                let mut songs = local.songs.clone();
                if let Some((_, Load::Ready(r))) = &self.search_results {
                    songs.extend(r.songs.iter().filter(|s| !local.songs.iter().any(|l| l.id == s.id)).cloned());
                }
                Some(Arc::new(songs))
            }
            _ => None,
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(songs) = self.page_songs().filter(|s| !s.is_empty()) else {
            return;
        };
        let current = self.selected_song.as_ref().and_then(|id| songs.iter().position(|s| &s.id == id));
        let next = match current {
            Some(i) => (i as isize + delta).clamp(0, songs.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected_song = Some(songs[next].id.clone());
        if matches!(self.route, Route::Songs | Route::Favorites) {
            self.song_list_scroll.scroll_to_item(next, ScrollStrategy::Center);
        }
        cx.notify();
    }

    pub(crate) fn play_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(songs) = self.page_songs() else {
            return false;
        };
        let Some(ix) = self.selected_song.as_ref().and_then(|id| songs.iter().position(|s| &s.id == id)) else {
            return false;
        };
        self.play(songs.as_ref().clone(), ix, false, cx);
        true
    }

    // ⌘K quick search

    // ⌘/ shortcuts sheet
}

#[cfg(test)]
mod tests {
    use super::similar;

    #[test]
    fn loose_artist_name_matching() {
        assert!(similar("BigXthaPlug", "big x the plug"));
        assert!(similar("Zach Bryan", "zach bryan"));
        assert!(similar("Zach Bryan", "zach"));
        assert!(!similar("Zach Top", "zach bryan"));
        assert!(!similar("Drake", "big x the plug"));
    }
}
