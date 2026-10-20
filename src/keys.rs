//! Keyboard: shortcuts, list navigation, search-field editing, quick search (⌘K) and
//! the shortcuts sheet (⌘/).

use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, FontWeight, KeyBinding, Keystroke, MouseButton, ScrollStrategy, SharedString,
    Window, actions, div, prelude::*, px, white,
};

use crate::{
    app::{Account, Load, MusicApp, Route},
    model::{Album, Artwork, Playlist, Song, TopResult},
    theme::Theme,
    views::{explicit_badge, icon},
};

actions!(
    music_keys,
    [
        GoForward,
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

/// Generic single-line editing (typing, deletion, cursor/selection movement, clipboard).
/// Returns false for keys it doesn't handle.
pub fn edit_text(f: &mut crate::textfield::TextField, k: &Keystroke, cx: &mut Context<MusicApp>) -> bool {
    use crate::textfield::TextField as F;
    let (cmd, alt, shift) = (k.modifiers.platform, k.modifiers.alt, k.modifiers.shift);
    match (k.key.as_str(), cmd, alt) {
        ("backspace", true, _) => f.delete_to_start(),
        ("backspace", _, true) => f.delete_word_back(),
        ("backspace", ..) => f.backspace(),
        ("delete", ..) => f.delete_forward(),
        ("left", true, _) | ("home", ..) => f.motion(shift, F::home),
        ("right", true, _) | ("end", ..) => f.motion(shift, F::end),
        ("left", _, true) => f.motion(shift, F::word_left),
        ("right", _, true) => f.motion(shift, F::word_right),
        ("left", ..) if shift => f.motion(true, F::left),
        ("right", ..) if shift => f.motion(true, F::right),
        ("left", ..) => f.collapse_or(true, F::left),
        ("right", ..) => f.collapse_or(false, F::right),
        ("a", true, _) => f.select_all(),
        ("c", true, _) | ("x", true, _) => {
            if let Some(text) = f.selected_text() {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                if k.key == "x" {
                    f.backspace();
                }
            }
        }
        ("v", true, _) => {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                f.insert(&text);
            }
        }
        _ if cmd || k.modifiers.control => return false,
        _ => match &k.key_char {
            Some(ch) if !ch.chars().any(char::is_control) => f.insert(ch),
            _ => return false,
        },
    }
    true
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
                use crate::nowplaying::NowPlayingTab;
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
        let closed = if self.palette_open {
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
        } else if self.show_account || self.show_volume || self.show_key_help {
            self.show_account = false;
            self.show_volume = false;
            self.show_key_help = false;
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
        use crate::textfield::TextField as F;
        let (cmd, alt, ctrl, shift) = (k.modifiers.platform, k.modifiers.alt, k.modifiers.control, k.modifiers.shift);
        let before = self.search.text().to_string();
        match (k.key.as_str(), cmd, alt) {
            ("backspace", true, _) => self.search.delete_to_start(),
            ("backspace", _, true) => self.search.delete_word_back(),
            ("backspace", ..) => self.search.backspace(),
            ("delete", ..) => self.search.delete_forward(),
            // Movement; with ⇧ it extends the selection.
            ("left", true, _) | ("home", ..) => self.search.motion(shift, F::home),
            ("right", true, _) | ("end", ..) => self.search.motion(shift, F::end),
            ("left", _, true) => self.search.motion(shift, F::word_left),
            ("right", _, true) => self.search.motion(shift, F::word_right),
            ("left", ..) if shift => self.search.motion(true, F::left),
            ("right", ..) if shift => self.search.motion(true, F::right),
            ("left", ..) => self.search.collapse_or(true, F::left),
            ("right", ..) => self.search.collapse_or(false, F::right),
            ("a", true, _) => self.search.select_all(),
            ("c", true, _) | ("x", true, _) => {
                if let Some(text) = self.search.selected_text() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                    if k.key == "x" {
                        self.search.backspace();
                    }
                }
            }
            ("v", true, _) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.search.insert(&text);
                }
            }
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
            _ if cmd || ctrl => return false,
            _ => match &k.key_char {
                Some(ch) if !ch.chars().any(char::is_control) => self.search.insert(ch),
                _ => return false,
            },
        }
        if self.search.text() != before {
            self.palette_index = 0;
            if !self.palette_open && self.route != Route::Search {
                self.history.clear();
                self.go_search_page(cx);
            }
            self.schedule_search(cx);
        }
        cx.notify();
        true
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
                artists: self.library_artist_rows().iter().filter(|(n, _, _)| matches(&words, n)).take(8).cloned().collect(),
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
            let recent: Vec<PaletteItem> = self.recent_searches.iter().map(|t| PaletteItem::Recent(t.clone())).collect();
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
        let artist_item = |a: &crate::model::Artist| PaletteItem::Artist(a.name.clone(), Some(a.id.clone()), a.artwork.clone());

        if let Some(r) = catalog {
            let catalog_songs: Arc<Vec<Song>> = Arc::new({
                let mut v: Vec<Song> = r.top.iter().filter_map(|t| if let TopResult::Song(s) = t { Some(s.clone()) } else { None }).collect();
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
            let candidates: Vec<&crate::model::Artist> =
                r.top.iter().filter_map(|t| if let TopResult::Artist(a) = t { Some(a) } else { None }).chain(r.artists.iter()).collect();
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
            for a in r.top.iter().filter_map(|t| if let TopResult::Artist(a) = t { Some(a) } else { None }).chain(r.artists.iter()) {
                if not_top(&a.id) && top_name.as_deref() != Some(&a.name.to_lowercase()) && !seen_artists.contains(&a.id) && artists.len() < 3 {
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
            for a in r.top.iter().filter_map(|t| if let TopResult::Album(a) = t { Some(a) } else { None }).chain(r.albums.iter()) {
                if not_top(&a.id) && !seen_albums.contains(&a.id) && albums.len() < 4 {
                    seen_albums.push(a.id.clone());
                    albums.push(PaletteItem::Album(a.clone()));
                }
            }
            for p in r.top.iter().filter_map(|t| if let TopResult::Playlist(p) = t { Some(p) } else { None }).filter(|p| not_top(&p.id)).take(2) {
                playlists.push(PaletteItem::Playlist(p.clone()));
            }
        } else {
            artists.extend(local.artists.iter().take(3).map(|(n, id, a)| PaletteItem::Artist(n.clone(), id.clone(), a.clone())));
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
    fn palette_go_to_artist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn activate_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.palette_items();
        let Some(item) = items.get(self.palette_index.min(items.len().saturating_sub(1))).cloned() else {
            return;
        };
        self.activate_item(item, window, cx);
    }

    fn activate_item(&mut self, item: PaletteItem, window: &mut Window, cx: &mut Context<Self>) {
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
            list = list.child(
                div()
                    .px_4()
                    .py_6()
                    .text_center()
                    .text_color(t.text_secondary)
                    .child(if empty_query {
                        "Search your library and Apple Music"
                    } else if searching {
                        "Searching…"
                    } else {
                        "No results"
                    }),
            );
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
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().px_1().rounded(px(4.)).bg(t.button).child(keys))
                .child(what)
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
            .child(div().flex_1().min_w_0().child(self.search_text(t, focused, "Search songs, albums, artists…", 20.)))
            .child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.))
                    .bg(t.button)
                    .text_size(px(11.))
                    .text_color(t.text_secondary)
                    .child("esc"),
            );

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
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.palette_open = false;
                    window.focus(&this.focus);
                    cx.notify();
                }))
                .child(
                    div()
                        .id("palette")
                        .w(px(620.))
                        .h_auto()
                        .rounded(px(16.))
                        .bg(t.menu)
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
    fn palette_row(&self, t: &Theme, i: usize, is_sel: bool, big: bool, item: PaletteItem, cx: &mut Context<Self>) -> AnyElement {
        let art_size = if big { 52. } else { 36. };
        let sub = if is_sel { white().opacity(0.8) } else { t.text_secondary };
        let (art, title, subtitle, trailing): (AnyElement, String, String, AnyElement) = match &item {
            PaletteItem::Artist(n, id, a) => (
                {
                    // Artists picked from song credits arrive without a photo; fetch it.
                    let resolved = if id.is_none() && a.is_empty() { self.resolve_artist_id(n, cx) } else { id.clone() };
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
                [Some("Album".to_string()), Some(a.artist.clone()), Some(a.year().to_string()).filter(|y| !y.is_empty())]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(self.album_in_library(a), |d| d.child(div().text_size(px(11.)).text_color(sub).child("In Library")))
                    .child(icon("chevron-right", 13., sub))
                    .into_any_element(),
            ),
            PaletteItem::Song(s, _) => {
                let in_lib = self.song_in_library(s) || self.lib_songs().iter().any(|l| l.title == s.title && l.artist == s.artist);
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
                                    .bg(if is_sel { white() } else { t.accent })
                                    .text_color(if is_sel { t.accent } else { white() })
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(icon("play", 10., if is_sel { t.accent } else { white() }))
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
            .h(px(if big { 68. } else { 48. }))
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap_3()
            .when(is_sel, |d| d.bg(t.accent).text_color(white()))
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
                    .when(!subtitle.is_empty(), |d| d.child(div().truncate().text_size(px(12.)).text_color(sub).child(subtitle))),
            )
            .child(trailing)
            .on_click(cx.listener(move |this, _, window, cx| this.activate_item(item.clone(), window, cx)))
            .into_any_element()
    }

    /// The query with a caret at the cursor, or the placeholder.
    pub(crate) fn search_text(&self, t: &Theme, focused: bool, placeholder: &'static str, caret_h: f32) -> AnyElement {
        let text = self.search.text();
        if text.is_empty() && !focused {
            return div().text_color(t.text_tertiary).child(placeholder).into_any_element();
        }
        let row = div().flex().items_center().min_w_0().overflow_hidden().whitespace_nowrap();
        let piece = |s: &str| SharedString::from(s.to_string());
        if let Some(sel) = self.search.selection() {
            // Like macOS: no caret while text is selected, just the highlight.
            let highlight = if focused { t.accent.opacity(0.38) } else { t.selected };
            return row
                .when(sel.start > 0, |d| d.child(piece(&text[..sel.start])))
                .child(div().rounded(px(3.)).bg(highlight).child(piece(&text[sel.clone()])))
                .when(sel.end < text.len(), |d| d.child(piece(&text[sel.end..])))
                .into_any_element();
        }
        let (before, after) = text.split_at(self.search.cursor());
        row.when(!before.is_empty(), |d| d.child(piece(before)))
            .when(focused, |d| d.child(div().flex_none().w(px(1.5)).h(px(caret_h)).bg(t.accent)))
            .when(!after.is_empty(), |d| d.child(piece(after)))
            .when(text.is_empty(), |d| d.child(div().pl_0p5().text_color(t.text_tertiary).child(placeholder)))
            .into_any_element()
    }

    // ⌘/ shortcuts sheet

    pub(crate) fn render_shortcuts(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().shortcuts;
        if !presence.visible() {
            return None;
        }
        let p = presence.value();
        let groups: [(&str, &[(&str, &str)]); 4] = [
            (
                "Playback",
                &[
                    ("Space", "Play / Pause"),
                    ("⌘→", "Next song"),
                    ("⌘←", "Previous song"),
                    ("⌘↑ / ⌘↓", "Volume"),
                    ("⌘L", "Go to current song"),
                    ("⌘⌥U", "Playing Next"),
                    ("⌃⌘F", "Now Playing"),
                    ("⇧⌘L", "Lyrics"),
                    ("⇧⌘M", "Mini Player"),
                ],
            ),
            (
                "Navigation",
                &[
                    ("⌘1 – ⌘9", "Home … Favorite Songs"),
                    ("⌘[ / ⌘]", "Back / Forward"),
                    ("↑ / ↓", "Move through songs"),
                    ("Return", "Play selected song"),
                    ("Esc", "Close"),
                ],
            ),
            (
                "Search",
                &[
                    ("⌘K", "Quick search"),
                    ("⌘F", "Search page"),
                    ("⌥⌫ / ⌘⌫", "Delete word / all"),
                    ("↓", "Into results"),
                ],
            ),
            ("Library", &[("⌘R", "Refresh library"), ("⌘/", "This list")]),
        ];
        let key_cap = |k: &str| {
            div()
                .px_1p5()
                .py_0p5()
                .rounded(px(5.))
                .bg(t.button)
                .border_1()
                .border_color(t.separator)
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .child(k.to_string())
        };
        let mut columns = div().flex().flex_wrap().gap_x_8().gap_y_6();
        for (title, rows) in groups {
            let mut col = div().w(px(250.)).flex().flex_col().gap_2().child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text_tertiary)
                    .child(title),
            );
            for (keys, what) in rows {
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().text_color(t.text_secondary).child(what.to_string()))
                        .child(key_cap(keys)),
                );
            }
            columns = columns.child(col);
        }
        Some(
            div()
                .id("shortcuts-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.35 * p))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.show_shortcuts = false;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("shortcuts")
                        .w(px(600.))
                        .p_7()
                        .rounded(px(18.))
                        .bg(t.menu)
                        .border_1()
                        .border_color(t.panel_border)
                        .shadow_lg()
                        .opacity(p)
                        .mt(px((1. - p) * 10.))
                        .flex()
                        .flex_col()
                        .gap_5()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child("Keyboard Shortcuts"))
                        .child(columns),
                )
                .into_any_element(),
        )
    }
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
