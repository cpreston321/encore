use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use gpui::{
    Bounds, Context, FocusHandle, KeyDownEvent, MouseButton, MouseMoveEvent, Pixels, Point,
    ScrollHandle, SharedString, Task, Window, actions, div, point, prelude::*, px,
};

use crate::{
    webplayer::WebPlayer,
    api::{self, Catalog},
    audio::{Audio, Preview},
    auth,
    library::Library,
    model::{Album, ArtistDetail, Browse, Playlist, RemoteLibrary, SearchResults, Song, Station},
    queue::{Advance, Queue},
    theme::Theme,
};

actions!(
    music,
    [
        PlayPause,
        NextTrack,
        PreviousTrack,
        VolumeUp,
        VolumeDown,
        FocusSearch,
        GoBack,
        ToggleQueue,
        Quit
    ]
);

pub const SIDEBAR_W: f32 = 212.;
pub const QUEUE_W: f32 = 320.;

pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(t) => Some(t),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Route {
    Home,
    New,
    Radio,
    Search,
    RecentlyAdded,
    Artists,
    Albums,
    Songs,
    AllPlaylists,
    Favorites,
    Album(String),
    Artist(String),
    Playlist(Playlist),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PlayState {
    Idle,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Account {
    SignedOut,
    SigningIn,
    SignedIn,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Drag {
    Progress,
    Volume,
    /// The Now Playing view's large scrub bar.
    BigProgress,
}

pub type MenuAction = Rc<dyn Fn(&mut MusicApp, &mut Window, &mut Context<MusicApp>)>;

pub struct MenuEntry {
    pub label: SharedString,
    pub icon: &'static str,
    pub action: MenuAction,
}

pub struct ContextMenu {
    pub position: Point<Pixels>,
    pub entries: Vec<Option<MenuEntry>>,
}

pub enum ImageState {
    Loading,
    /// The file, and when it finished downloading (fresh images fade in).
    Ready(Arc<std::path::Path>, std::time::Instant),
    Failed,
}

/// An in-progress crossfade.
struct Fade {
    outgoing: Audio,
    length: Duration,
    created: std::time::Instant,
    /// When the incoming song actually started (streams take a moment to begin).
    started: Option<std::time::Instant>,
    _task: Task<()>,
}

/// How early the next consecutive album track starts, to cover stream start-up time.
const GAPLESS_LEAD: f32 = 1.2;

/// Crossfade lengths offered in the Playing Next panel.
pub const CROSSFADE_OPTIONS: [u32; 4] = [0, 3, 6, 12];

#[derive(serde::Serialize, serde::Deserialize)]
struct Settings {
    #[serde(default = "default_crossfade")]
    crossfade_secs: u32,
    /// Sidebar sections the user folded away.
    #[serde(default)]
    collapsed: Vec<String>,
    #[serde(default)]
    recent_searches: Vec<String>,
}

fn default_crossfade() -> u32 {
    6
}

impl Settings {
    fn path() -> PathBuf {
        api::data_dir().join("settings.json")
    }

    fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Self { crossfade_secs: default_crossfade(), collapsed: Vec::new(), recent_searches: Vec::new() })
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(api::data_dir());
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(Self::path(), json);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NavDirection {
    /// Sidebar jump: fade up in place.
    Root,
    /// Into a detail page: slide in from the right.
    Forward,
    /// Back: slide in from the left.
    Back,
}

/// Transition state that outlives a single render.
pub struct Fx {
    pub queue: crate::anim::Presence,
    pub account: crate::anim::Presence,
    pub volume: crate::anim::Presence,
    pub notice: crate::anim::Presence,
    pub key_help: crate::anim::Presence,
    pub now_playing: crate::anim::Presence,
    pub palette: crate::anim::Presence,
    pub shortcuts: crate::anim::Presence,
    /// The last notice text, kept so the toast can fade out after it's cleared.
    pub last_notice: String,
    pub page_bg: crate::anim::ColorBlend,
    pub sidebar_bg: crate::anim::ColorBlend,
    pub pill_bg: crate::anim::ColorBlend,
    pub hovers: HashMap<SharedString, crate::anim::Presence>,
    pub sections: HashMap<&'static str, crate::anim::Presence>,
}

impl Fx {
    fn new(t: &Theme) -> Self {
        use crate::anim::{ColorBlend, Presence};
        let quick = Duration::from_millis(180);
        let colors = Duration::from_millis(420);
        Self {
            queue: Presence::new(Duration::from_millis(240)),
            account: Presence::new(quick),
            volume: Presence::new(quick),
            notice: Presence::new(Duration::from_millis(220)),
            key_help: Presence::new(quick),
            now_playing: Presence::new(Duration::from_millis(320)),
            palette: Presence::new(Duration::from_millis(160)),
            shortcuts: Presence::new(quick),
            last_notice: String::new(),
            page_bg: ColorBlend::new(t.window, colors),
            sidebar_bg: ColorBlend::new(t.panel, colors),
            pill_bg: ColorBlend::new(t.pill, Duration::from_millis(600)),
            hovers: HashMap::new(),
            sections: HashMap::new(),
        }
    }

    fn animating(&self) -> bool {
        self.queue.animating()
            || self.account.animating()
            || self.volume.animating()
            || self.notice.animating()
            || self.key_help.animating()
            || self.now_playing.animating()
            || self.palette.animating()
            || self.shortcuts.animating()
            || self.page_bg.animating()
            || self.sidebar_bg.animating()
            || self.pill_bg.animating()
            || self.hovers.values().any(|h| h.animating())
            || self.sections.values().any(|s| s.animating())
    }
}

pub struct MusicApp {
    pub(crate) catalog: Catalog,
    pub(crate) catalog_note: Option<String>,
    pub(crate) browse: Load<Browse>,
    pub(crate) albums: HashMap<String, Load<(Album, Vec<Song>)>>,
    pub(crate) artists: HashMap<String, Load<ArtistDetail>>,
    pub(crate) playlist_tracks: HashMap<String, Load<Option<Vec<Song>>>>,
    /// Playlists whose tracks were requested only to build a cover mosaic.
    requested_playlists: RefCell<std::collections::HashSet<String>>,
    /// Artists fetched in the background (e.g. for "More by" under an album).
    requested_artists: RefCell<std::collections::HashSet<String>>,
    pub(crate) stations: Option<Load<Vec<Station>>>,
    /// Name of the artist selected in the library's Artists page.
    pub(crate) selected_library_artist: Option<String>,
    /// Keys of descriptions expanded with MORE.
    pub(crate) expanded_notes: std::collections::HashSet<String>,
    pub(crate) search: crate::textfield::TextField,
    pub(crate) palette_open: bool,
    pub(crate) palette_index: usize,
    pub(crate) show_shortcuts: bool,
    pub(crate) recent_searches: Vec<String>,
    /// Lets ↑/↓ keep the selection in view in the virtualized song lists.
    pub(crate) song_list_scroll: gpui::UniformListScrollHandle,
    #[allow(clippy::type_complexity)]
    pub(crate) local_matches_cache: RefCell<Option<((String, (usize, usize, usize, usize, usize)), Arc<crate::keys::LocalMatches>)>>,
    pub(crate) forward_history: Vec<Route>,
    pub(crate) search_results: Option<(String, Load<SearchResults>)>,
    search_task: Option<Task<()>>,
    pub(crate) library: Library,
    pub(crate) account: Account,
    pub(crate) remote: Load<RemoteLibrary>,
    pub(crate) notice: Option<String>,
    notice_task: Option<Task<()>>,
    sign_in_task: Option<Task<()>>,
    sign_in_stop: Option<crate::localhost::StopFlag>,
    pub(crate) show_key_help: bool,
    pub(crate) route: Route,
    pub(crate) history: Vec<Route>,
    /// Scroll offset of each page in `history`, restored on back.
    scroll_history: Vec<Point<Pixels>>,
    /// Bumped on every navigation so the page transition replays.
    pub(crate) route_generation: usize,
    pub(crate) nav_direction: NavDirection,
    pub(crate) fx: RefCell<Fx>,
    pub(crate) show_now_playing: bool,
    artist_ids: RefCell<HashMap<String, Option<String>>>,
    spectra: HashMap<String, Arc<crate::spectrum::Spectrum>>,
    pub(crate) list_views: RefCell<HashMap<&'static str, crate::browse::ListView>>,
    #[allow(clippy::type_complexity)]
    pub(crate) shaped_cache: RefCell<Option<((&'static str, crate::browse::SortKey, String, usize, usize), Arc<Vec<Song>>)>>,
    /// Songs selected with ⌘/⇧-click (empty: just `selected_song`).
    pub(crate) multi: Vec<String>,
    pub(crate) select_anchor: Option<usize>,
    pub(crate) filter_focus: FocusHandle,
    pub(crate) filter_target: &'static str,
    pub(crate) filter_focused: bool,
    pub(crate) personal: Option<Load<Option<crate::model::PersonalHome>>>,
    pub(crate) new_playlist: Option<crate::playlists::NewPlaylist>,
    pub(crate) new_playlist_focus: FocusHandle,
    pub(crate) now_playing_tab: crate::nowplaying::NowPlayingTab,
    pub(crate) lyrics: HashMap<String, Load<Option<crate::lyrics::Lyrics>>>,
    pub(crate) lyrics_scroll: ScrollHandle,
    pub(crate) lyrics_follow: RefCell<crate::nowplaying::LyricsScroll>,
    pub(crate) np_progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) np_scrub: Option<f32>,
    media: RefCell<crate::media::MediaControls>,
    #[allow(clippy::type_complexity)]
    sorted_albums_cache: RefCell<Option<((usize, usize, usize, usize, usize), Arc<Vec<Album>>)>>,
    #[allow(clippy::type_complexity)]
    artist_rows_cache: RefCell<Option<((usize, usize, usize, usize, usize), Arc<Vec<(String, Option<String>, crate::model::Artwork)>>)>>,
    first_frame_logged: Cell<bool>,
    pub(crate) queue: Queue<Song>,
    pub(crate) state: PlayState,
    pub(crate) playback_error: Option<String>,
    /// Shared with the progress view, which redraws itself while playing.
    audio: crate::progress::SharedAudio,
    pub(crate) progress: gpui::Entity<crate::progress::Progress>,
    /// Full-song players (Apple's web player), created on demand. A crossfade needs two:
    /// one finishing the old song while the other starts the next.
    web_players: Vec<Rc<WebPlayer>>,
    /// The song fading out during a crossfade.
    fade: Option<Fade>,
    /// Crossfade length in seconds (0 = off); persisted in settings.json.
    pub(crate) crossfade_secs: u32,
    pub(crate) collapsed_sections: std::collections::HashSet<String>,
    /// Set when full playback failed repeatedly; previews are used for the session.
    full_unavailable: bool,
    full_failures: u32,
    /// When a full song was requested and hasn't started yet.
    loading_since: Option<std::time::Instant>,
    load_generation: u64,
    pub(crate) volume: f32,
    pub(crate) images: RefCell<HashMap<String, ImageState>>,
    /// Average color per artwork URL, for tinting page headers. `None` while computing.
    art_colors: RefCell<HashMap<String, Option<gpui::Hsla>>>,
    pub(crate) menu: Option<ContextMenu>,
    pub(crate) show_queue: bool,
    pub(crate) show_account: bool,
    /// Where the sidebar's account row is, so its popover can sit just above it.
    pub(crate) account_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// When the Apple Music library last finished loading.
    pub(crate) library_updated: Option<std::time::Instant>,
    pub(crate) selected_song: Option<String>,
    pub(crate) drag: Option<Drag>,
    pub(crate) progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) show_volume: bool,
    /// The player's speaker button, which the volume popover opens above.
    pub(crate) volume_button_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub focus: FocusHandle,
    pub(crate) search_focus: FocusHandle,
    pub(crate) scroll: ScrollHandle,
    pub(crate) artist_pane_scroll: ScrollHandle,
    pub(crate) user_name: String,
    /// Profile photo the user chose (Apple doesn't share Apple Music profile photos).
    pub(crate) avatar: Option<Arc<std::path::Path>>,
    _ticker: Task<()>,
}

impl MusicApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
            .detach();

        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.tick(cx);
                        this.media_tick(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        let audio: crate::progress::SharedAudio = Rc::default();
        let progress_bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let app = cx.weak_entity();
        let progress = cx.new(|cx| crate::progress::Progress::new(audio.clone(), progress_bounds.clone(), app, cx));
        crate::perf::milestone("app init start");
        let (catalog, catalog_note) = Catalog::from_env();
        crate::perf::milestone("catalog configured");
        if let Some(note) = &catalog_note {
            eprintln!("{note}");
        }
        let mut this = Self {
            catalog,
            catalog_note,
            browse: Load::Loading,
            albums: HashMap::new(),
            artists: HashMap::new(),
            playlist_tracks: HashMap::new(),
            requested_playlists: RefCell::default(),
            requested_artists: RefCell::default(),
            stations: None,
            selected_library_artist: None,
            expanded_notes: Default::default(),
            search: Default::default(),
            palette_open: false,
            palette_index: 0,
            show_shortcuts: false,
            recent_searches: Settings::load().recent_searches,
            song_list_scroll: gpui::UniformListScrollHandle::new(),
            local_matches_cache: RefCell::default(),
            forward_history: Vec::new(),
            search_results: None,
            search_task: None,
            library: Library::load(api::data_dir().join("library.json")),
            account: Account::SignedOut,
            remote: Load::Loading,
            notice: None,
            notice_task: None,
            sign_in_task: None,
            sign_in_stop: None,
            show_key_help: false,
            route: Route::New,
            history: Vec::new(),
            scroll_history: Vec::new(),
            route_generation: 0,
            nav_direction: NavDirection::Root,
            fx: RefCell::new(Fx::new(&Theme::for_appearance(window.appearance()))),
            first_frame_logged: Cell::new(false),
            show_now_playing: false,
            artist_ids: RefCell::default(),
            spectra: HashMap::new(),
            list_views: RefCell::default(),
            shaped_cache: RefCell::default(),
            multi: Vec::new(),
            select_anchor: None,
            filter_focus: cx.focus_handle(),
            filter_target: "songs",
            filter_focused: false,
            personal: None,
            new_playlist: None,
            new_playlist_focus: cx.focus_handle(),
            now_playing_tab: crate::nowplaying::NowPlayingTab::Lyrics,
            lyrics: HashMap::new(),
            lyrics_scroll: ScrollHandle::new(),
            lyrics_follow: RefCell::default(),
            np_progress_bounds: Rc::default(),
            np_scrub: None,
            media: RefCell::new(crate::media::MediaControls::new()),
            sorted_albums_cache: RefCell::default(),
            artist_rows_cache: RefCell::default(),
            queue: Queue::new(),
            state: PlayState::Idle,
            playback_error: None,
            audio,
            progress,
            web_players: Vec::new(),
            fade: None,
            crossfade_secs: Settings::load().crossfade_secs,
            collapsed_sections: Settings::load().collapsed.into_iter().collect(),
            full_unavailable: false,
            full_failures: 0,
            loading_since: None,
            load_generation: 0,
            volume: 0.8,
            images: RefCell::default(),
            art_colors: RefCell::default(),
            menu: None,
            show_queue: false,
            show_account: false,
            account_bounds: Rc::default(),
            library_updated: None,
            selected_song: None,
            drag: None,
            progress_bounds,
            volume_bounds: Rc::default(),
            show_volume: false,
            volume_button_bounds: Rc::default(),
            focus: cx.focus_handle(),
            search_focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            artist_pane_scroll: ScrollHandle::new(),
            user_name: user_full_name(),
            avatar: saved_photo().map(|p| Arc::from(p.as_path())),
            _ticker: ticker,
        };
        crate::perf::milestone("state built");
        this.load_browse(cx);
        crate::perf::milestone("charts cache read");
        #[cfg(any(debug_assertions, feature = "tour"))]
        if std::env::var_os("MUSIC_TOUR").is_some() {
            this.start_tour(cx);
        }
        if this.catalog.is_musickit() {
            this.restore_sign_in(cx);
        }
        this
    }

    /// Debug-only: steps through every page so screenshots can be taken without driving
    /// the mouse. Each step is announced on stderr as `TOUR <n> <name>`.
    #[cfg(any(debug_assertions, feature = "tour"))]
    fn start_tour(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let executor = cx.background_executor().clone();
            let pause = |secs| executor.timer(Duration::from_secs(secs));
            loop {
                pause(1).await;
                let ready = this
                    .update(cx, |this, _| {
                        this.browse.ready().is_some()
                            && (this.account != Account::SignedIn || this.remote.ready().is_some())
                    })
                    .unwrap_or(true);
                if ready {
                    break;
                }
            }
            let Ok(steps) = this.update(cx, |this, _| this.tour_steps()) else {
                return;
            };
            // MUSIC_TOUR_PLAY=1: play silently so the player and progress bar show up.
            if std::env::var_os("MUSIC_TOUR_PLAY").is_some() {
                this.update(cx, |this, cx| {
                    this.show_queue = std::env::var_os("MUSIC_TOUR_QUEUE").is_some();
                    this.show_account = std::env::var_os("MUSIC_TOUR_ACCOUNT").is_some();
                    this.set_volume(0., cx);
                    if let Some(browse) = this.browse.ready() {
                        let songs = browse.top_songs.clone();
                        this.play(songs, 1, false, cx);
                    }
                })
                .ok();
                // MUSIC_TOUR_SEEK_END=1: jump near the end once playing, to exercise crossfade.
                if std::env::var_os("MUSIC_TOUR_SEEK_END").is_some() {
                    for _ in 0..100 {
                        pause(1).await;
                        let seeked = this
                            .update(cx, |this, _| {
                                if this.state != PlayState::Playing {
                                    return false;
                                }
                                let Some(d) = this.audio.borrow().as_ref().and_then(Audio::duration) else {
                                    return false;
                                };
                                let target = d - this.crossfade_secs as f32 - 4.;
                                eprintln!("TOUR seeking to {target:.0}s of {d:.0}s");
                                if let Some(audio) = this.audio.borrow().as_ref() {
                                    audio.seek(target);
                                }
                                true
                            })
                            .unwrap_or(true);
                        if seeked {
                            break;
                        }
                    }
                }
            }
            // MUSIC_TOUR_OVERLAY=palette|shortcuts: show that overlay on top of the tour.
            match std::env::var("MUSIC_TOUR_OVERLAY").as_deref() {
                Ok(overlay @ ("palette" | "palette-selected")) => {
                    let select = overlay == "palette-selected";
                    this.update(cx, |this, cx| {
                        this.search.set(std::env::var("MUSIC_TOUR_QUERY").unwrap_or_else(|_| "zach bryan".into()));
                        if select {
                            this.search.select_all();
                        }
                        this.palette_open = true;
                        this.schedule_search(cx);
                        cx.notify();
                    })
                    .ok();
                }
                Ok(overlay @ ("nowplaying" | "nowplaying-queue")) => {
                    let queue = overlay == "nowplaying-queue";
                    for _ in 0..60 {
                        pause(1).await;
                        let ready = this.update(cx, |this, _| this.state == PlayState::Playing).unwrap_or(true);
                        if ready {
                            break;
                        }
                    }
                    this.update(cx, |this, cx| {
                        use crate::nowplaying::NowPlayingTab;
                        this.open_now_playing(Some(if queue { NowPlayingTab::UpNext } else { NowPlayingTab::Lyrics }), cx);
                        if let Some(audio) = this.audio.borrow().as_ref() {
                            audio.seek(75.);
                        }
                    })
                    .ok();
                }
                Ok("shortcuts") => {
                    this.update(cx, |this, cx| {
                        this.show_shortcuts = true;
                        cx.notify();
                    })
                    .ok();
                }
                _ => {}
            }
            for (n, (name, route, search)) in steps.into_iter().enumerate() {
                // `name@300` means: show the page scrolled down 300px.
                let (label, scroll) = match name.split_once('@') {
                    Some((label, y)) => (label.to_string(), y.parse::<f32>().ok()),
                    None => (name.clone(), None),
                };
                this.update(cx, |this, cx| {
                    eprintln!("TOUR {n} {label}{}", scroll.map(|y| format!("-{y}")).unwrap_or_default());
                    this.history.clear();
                    this.scroll_history.clear();
                    if let Some(term) = search {
                        this.search.set(term);
                        this.schedule_search(cx);
                    }
                    this.set_route(route, cx);
                })
                .ok();
                if let Some(y) = scroll {
                    // Let the page load and lay out before scrolling it.
                    pause(3).await;
                    this.update(cx, |this, cx| {
                        this.scroll.set_offset(point(px(0.), px(-y)));
                        cx.notify();
                    })
                    .ok();
                }
                pause(6).await;
            }
            this.update(cx, |this, _| {
                let full = this.audio.borrow().as_ref().is_some_and(Audio::is_full);
                eprintln!(
                    "TOUR playback: full={full} state={:?} position={:.1}s duration={:.1}s",
                    this.state,
                    this.position(),
                    this.duration()
                );
            })
            .ok();
            eprintln!("TOUR done");
        })
        .detach();
    }

    #[cfg(any(debug_assertions, feature = "tour"))]
    fn tour_steps(&self) -> Vec<(String, Route, Option<String>)> {
        let mut all = self.all_tour_steps();
        // MUSIC_TOUR_ONLY=songs,artists limits the tour to those steps.
        if let Ok(only) = std::env::var("MUSIC_TOUR_ONLY") {
            let names: Vec<&str> = only.split(',').map(str::trim).collect();
            all.retain(|(name, _, _)| names.contains(&name.as_str()));
        }
        if std::env::var("MUSIC_TOUR").as_deref() != Ok("hero") {
            return all;
        }
        // Detail pages only, each at the top, mid-scroll and past the header.
        let mut steps = Vec::new();
        for (name, route, _) in all {
            match name.as_str() {
                "artist" | "catalog-album" | "library-album" | "library-playlist" | "catalog-playlist" => {
                    for y in ["", "@260", "@640"] {
                        steps.push((format!("{name}{y}"), route.clone(), None));
                    }
                }
                "artists" => steps.push((name, route, None)),
                _ => {}
            }
        }
        steps
    }

    #[cfg(any(debug_assertions, feature = "tour"))]
    fn all_tour_steps(&self) -> Vec<(String, Route, Option<String>)> {
        let mut steps: Vec<(String, Route, Option<String>)> = [
            ("new", Route::New),
            ("home", Route::Home),
            ("radio", Route::Radio),
            ("recently-added", Route::RecentlyAdded),
            ("artists", Route::Artists),
            ("albums", Route::Albums),
            ("songs", Route::Songs),
            ("all-playlists", Route::AllPlaylists),
            ("favorites", Route::Favorites),
        ]
        .into_iter()
        .map(|(n, r)| (n.to_string(), r, None))
        .collect();
        steps.push(("search".into(), Route::Search, Some(std::env::var("MUSIC_TOUR_QUERY").unwrap_or_else(|_| "taylor swift".into()))));
        // MUSIC_TOUR_ARTIST=Name focuses the artist/album steps on one library artist.
        let focus = std::env::var("MUSIC_TOUR_ARTIST").ok().map(|n| n.to_lowercase());
        let focus_album = focus
            .as_ref()
            .and_then(|n| self.lib_albums().iter().find(|a| a.artist.to_lowercase() == *n).cloned());
        if let Some(album) = focus_album.or_else(|| self.lib_albums().first().cloned()) {
            steps.push(("library-album".into(), Route::Album(album.id.clone()), None));
        }
        if let Some(name) = &focus
            && let Some(artist) = self.remote().and_then(|r| r.artists.iter().find(|a| a.name.to_lowercase() == *name))
        {
            steps.push(("artist".into(), Route::Artist(artist.id.clone()), None));
        }
        if let Some(browse) = self.browse.ready() {
            if let Some(album) = browse.top_albums.first() {
                steps.push(("catalog-album".into(), Route::Album(album.id.clone()), None));
            }
            let artist = browse
                .top_songs
                .iter()
                .find_map(|s| s.artist_id.clone())
                .or_else(|| self.remote().and_then(|r| r.artists.iter().find(|a| !a.id.starts_with("r.")).map(|a| a.id.clone())));
            if let Some(id) = artist.filter(|_| focus.is_none()) {
                steps.push(("artist".into(), Route::Artist(id), None));
            }
            if let Some(p) = browse.top_playlists.first() {
                steps.push(("catalog-playlist".into(), Route::Playlist(p.clone()), None));
            }
        }
        if let Some(p) = self.lib_playlists().first() {
            steps.push(("library-playlist".into(), Route::Playlist(p.clone()), None));
        }
        steps
    }

    // Account

    /// Reads the saved Music User Token off the main thread: macOS can show a Keychain
    /// prompt (e.g. after an app update), and the window shouldn't wait on it. If a cached
    /// library exists we show it right away as signed in; the token follows.
    fn restore_sign_in(&mut self, cx: &mut Context<Self>) {
        if remote_cache_path().exists() {
            self.account = Account::SignedIn;
            self.load_remote_library_cache();
            if let Some(home) = std::fs::read(api::cache_dir().join("personal-home.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<crate::model::PersonalHome>(&b).ok())
            {
                self.personal = Some(Load::Ready(Some(home)));
            }
        }
        cx.spawn(async move |this, cx| {
            let token = cx.background_executor().spawn(async { auth::stored_user_token() }).await;
            crate::perf::milestone("keychain read");
            this.update(cx, |this, cx| {
                match token {
                    Some(token) => {
                        this.catalog.set_user_token(Some(token));
                        this.account = Account::SignedIn;
                        this.load_remote_library(cx);
                        this.load_personal_home(cx);
                        this.ensure_route_data(cx);
                    }
                    None => {
                        this.account = Account::SignedOut;
                        this.remote = Load::Loading;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Apple doesn't expose Apple Music profile photos to third-party apps, so the user
    /// picks one; a copy lives in the app's data folder.
    pub(crate) fn choose_photo(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Photo".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            let copied = cx
                .background_executor()
                .spawn(async move {
                    // Re-encode so any image format works and the copy stays small.
                    let image = image::open(&source)?.thumbnail(256, 256);
                    std::fs::create_dir_all(api::data_dir())?;
                    let path = api::data_dir().join("profile-photo.png");
                    image.save(&path)?;
                    anyhow::Ok(path)
                })
                .await;
            this.update(cx, |this, cx| match copied {
                Ok(path) => {
                    // A new path each time so gpui's image cache doesn't show the old one.
                    let stamped = path.with_file_name(format!(
                        "profile-photo-{}.png",
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
                    ));
                    let _ = std::fs::rename(&path, &stamped);
                    remove_old_photos(&stamped);
                    this.avatar = Some(Arc::from(stamped.as_path()));
                    cx.notify();
                }
                Err(e) => this.show_notice(format!("Couldn't use that image: {e:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn remove_photo(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.avatar.take() {
            let _ = std::fs::remove_file(&*path);
        }
        cx.notify();
    }

    /// Home's personal shelves: the cached copy shows at once (no layout shift on launch),
    /// then a fresh copy replaces it when Apple answers.
    pub(crate) fn load_personal_home(&mut self, cx: &mut Context<Self>) {
        let path = api::cache_dir().join("personal-home.json");
        if self.personal.is_none() {
            self.personal = Some(
                std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<crate::model::PersonalHome>(&b).ok())
                    .map_or(Load::Loading, |h| Load::Ready(Some(h))),
            );
        }
        self.fetch(cx, |c| c.personal_home(), move |this, load, _| match load {
            Load::Ready(Some(home)) => {
                if let Ok(json) = serde_json::to_vec(&home) {
                    let _ = std::fs::write(&path, json);
                }
                this.personal = Some(Load::Ready(Some(home)));
            }
            // Keep showing the cached shelves if a refresh fails.
            other if !matches!(this.personal, Some(Load::Ready(Some(_)))) => this.personal = Some(other),
            _ => {}
        });
    }

    fn load_remote_library_cache(&mut self) {
        if self.remote.ready().is_none()
            && let Some(library) = std::fs::read(remote_cache_path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<RemoteLibrary>(&bytes).ok())
        {
            self.remote = Load::Ready(library);
        }
    }

    pub(crate) fn sign_in(&mut self, cx: &mut Context<Self>) {
        let Some(developer_token) = self.catalog.developer_token().map(str::to_string) else {
            self.show_key_help = true;
            cx.notify();
            return;
        };
        let server = match auth::SignInServer::start(&developer_token) {
            Ok(server) => server,
            Err(e) => return self.show_notice(format!("Couldn't start sign-in: {e:#}"), cx),
        };
        cx.open_url(&server.url);
        // A new attempt replaces any earlier one still waiting.
        if let Some(stop) = self.sign_in_stop.replace(server.stop_flag()) {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.account = Account::SigningIn;
        self.sign_in_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { server.wait_for_token() })
                .await;
            this.update(cx, |this, cx| {
                match result.and_then(|token| auth::store_user_token(&token).map(|_| token)) {
                    Ok(token) => {
                        this.catalog.set_user_token(Some(token));
                        this.account = Account::SignedIn;
                        this.load_remote_library(cx);
                        this.load_personal_home(cx);
                        this.show_notice("Signed in to Apple Music".into(), cx);
                    }
                    Err(e) => {
                        this.account = Account::SignedOut;
                        this.show_notice(format!("Sign-in didn't complete: {e:#}"), cx);
                    }
                }
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        // Stops the local sign-in server too, not just our wait on it.
        if let Some(stop) = self.sign_in_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.sign_in_task = None;
        self.account = Account::SignedOut;
        cx.notify();
    }

    pub(crate) fn sign_out(&mut self, cx: &mut Context<Self>) {
        if self.audio.borrow().as_ref().is_some_and(Audio::is_full) {
            if let Some(audio) = self.audio.borrow_mut().as_mut() {
                audio.stop();
            }
            *self.audio.borrow_mut() = None;
            self.state = PlayState::Idle;
        }
        self.cancel_fade();
        self.web_players.clear();
        self.personal = None;
        let _ = std::fs::remove_file(api::cache_dir().join("personal-home.json"));
        self.full_unavailable = false;
        let _ = std::fs::remove_file(remote_cache_path());
        auth::clear_user_token();
        self.catalog.set_user_token(None);
        self.account = Account::SignedOut;
        self.remote = Load::Loading;
        self.playlist_tracks.retain(|id, _| !id.starts_with("p."));
        self.albums.retain(|id, _| !id.starts_with("l."));
        cx.notify();
    }

    /// Shows the cached copy of the library immediately, then refreshes it. Favorites come
    /// in a second pass since they need a ratings lookup per 100 songs.
    pub(crate) fn load_remote_library(&mut self, cx: &mut Context<Self>) {
        self.load_remote_library_cache();
        let started = std::time::Instant::now();
        self.fetch(cx, |c| c.library(), move |this, load, cx| {
            match load {
                // A rejected token could also be one rejected endpoint; confirm with a
                // second request before throwing the sign-in away.
                Load::Failed(err) if err.contains("http status: 401") || err.contains("http status: 403") => {
                    eprintln!("library request rejected: {err}");
                    this.fetch(cx, |c| c.user_token_valid(), |this, load, cx| match load {
                        Load::Ready(false) => this.sign_out_expired(),
                        _ => {
                            if this.remote.ready().is_none() {
                                this.remote = Load::Failed("Apple Music refused the library request. Try again later.".into());
                            }
                            cx.notify();
                        }
                    });
                }
                Load::Failed(err) => {
                    eprintln!("library failed after {:?}: {err}", started.elapsed());
                    // Keep showing the last good copy if a refresh fails.
                    if this.remote.ready().is_none() {
                        this.remote = Load::Failed(err);
                    }
                }
                Load::Ready(mut library) => {
                    eprintln!(
                        "library: {} songs, {} albums, {} playlists in {:?}",
                        library.songs.len(),
                        library.albums.len(),
                        library.playlists.len(),
                        started.elapsed()
                    );
                    if let Some(old) = this.remote.ready() {
                        library.favorites = old.favorites.clone();
                    }
                    crate::perf::milestone("library ready");
                    let songs = library.songs.clone();
                    this.remote = Load::Ready(library);
                    this.library_updated = Some(std::time::Instant::now());
                    this.save_remote_cache();
                    this.load_favorites(songs, cx);
                }
                Load::Loading => {}
            }
        });
        cx.notify();
    }

    fn load_favorites(&mut self, songs: Arc<Vec<Song>>, cx: &mut Context<Self>) {
        self.fetch(cx, move |c| c.library_favorites(&songs), |this, load, _| match load {
            Load::Ready(favorites) => {
                if let Load::Ready(library) = &mut this.remote {
                    library.favorites = Arc::new(favorites);
                }
                this.save_remote_cache();
            }
            Load::Failed(err) => eprintln!("favorites failed: {err}"),
            Load::Loading => {}
        });
    }

    fn save_remote_cache(&self) {
        if let Some(library) = self.remote.ready()
            && let Ok(json) = serde_json::to_vec(library)
        {
            let _ = std::fs::create_dir_all(api::cache_dir());
            let _ = std::fs::write(remote_cache_path(), json);
        }
    }

    fn sign_out_expired(&mut self) {
        let _ = std::fs::remove_file(remote_cache_path());
        auth::clear_user_token();
        self.catalog.set_user_token(None);
        self.account = Account::SignedOut;
        self.remote = Load::Loading;
        self.notice = Some("Your Apple Music session expired. Sign in again to see your library.".into());
    }

    pub(crate) fn show_notice(&mut self, message: String, cx: &mut Context<Self>) {
        self.notice = Some(message);
        self.notice_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            this.update(cx, |this, cx| {
                this.notice = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    // Library (Apple Music when signed in, local otherwise)

    pub(crate) fn remote(&self) -> Option<&RemoteLibrary> {
        if self.account == Account::SignedIn {
            self.remote.ready()
        } else {
            None
        }
    }

    pub(crate) fn lib_songs(&self) -> Arc<Vec<Song>> {
        self.remote().map_or_else(|| Arc::new(self.library.songs.clone()), |r| r.songs.clone())
    }

    pub(crate) fn lib_albums(&self) -> &[Album] {
        self.remote().map_or(&self.library.albums, |r| &r.albums)
    }

    /// Changes whenever the library contents might have: cheap key for derived caches.
    pub(crate) fn library_key(&self) -> (usize, usize, usize, usize, usize) {
        match self.remote() {
            Some(r) => (1, Arc::as_ptr(&r.songs) as usize, r.albums.len(), r.artists.len(), r.albums.as_ptr() as usize),
            None => (0, self.library.songs.len(), self.library.albums.len(), 0, self.library.albums.as_ptr() as usize),
        }
    }

    /// Library albums sorted by title, computed once per library change.
    pub(crate) fn sorted_albums(&self) -> Arc<Vec<Album>> {
        let key = self.library_key();
        if let Some((k, v)) = &*self.sorted_albums_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let mut albums = self.lib_albums().to_vec();
        albums.sort_by_cached_key(|a| a.title.to_lowercase());
        let albums = Arc::new(albums);
        *self.sorted_albums_cache.borrow_mut() = Some((key, albums.clone()));
        albums
    }

    /// Cached rows for the Artists page (see `views::library_artist_rows_uncached`).
    pub(crate) fn library_artist_rows(&self) -> Arc<Vec<(String, Option<String>, crate::model::Artwork)>> {
        let key = self.library_key();
        if let Some((k, v)) = &*self.artist_rows_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let rows = Arc::new(self.library_artist_rows_uncached());
        *self.artist_rows_cache.borrow_mut() = Some((key, rows.clone()));
        rows
    }

    pub(crate) fn lib_recently_added(&self) -> Vec<Album> {
        match self.remote() {
            Some(r) if !r.recently_added.is_empty() => r.recently_added.clone(),
            Some(r) => r.albums.clone(),
            None => self.library.albums.clone(),
        }
    }

    pub(crate) fn lib_playlists(&self) -> Vec<Playlist> {
        self.remote().map_or_else(|| self.library.playlists.clone(), |r| r.playlists.clone())
    }

    /// Apple's own auto-updating "Favorite Songs" playlist, when the library has one.
    pub(crate) fn is_apple_favorites(&self, playlist: &Playlist) -> bool {
        self.remote().is_some() && playlist.id.starts_with("p.") && playlist.name == "Favorite Songs"
    }

    /// Where the sidebar's Favorite Songs goes: Apple's playlist (its real order) if present.
    pub(crate) fn favorites_route(&self) -> Route {
        self.lib_playlists()
            .into_iter()
            .find(|p| self.is_apple_favorites(p))
            .map_or(Route::Favorites, Route::Playlist)
    }

    pub(crate) fn lib_favorites(&self) -> Arc<Vec<Song>> {
        self.remote().map_or_else(|| Arc::new(self.library.favorites.clone()), |r| r.favorites.clone())
    }

    pub(crate) fn is_favorite(&self, song: &Song) -> bool {
        match self.remote() {
            Some(r) => r.favorites.iter().any(|s| s.id == song.id),
            None => self.library.is_favorite(&song.id),
        }
    }

    /// Signed in, this sets the song's Apple Music rating and updates the list optimistically,
    /// rolling back if Apple rejects it.
    pub(crate) fn toggle_favorite(&mut self, song: &Song, cx: &mut Context<Self>) {
        let Load::Ready(remote) = &mut self.remote else {
            self.library.toggle_favorite(song);
            return cx.notify();
        };
        if self.account != Account::SignedIn {
            self.library.toggle_favorite(song);
            return cx.notify();
        }
        let favorite = !remote.favorites.iter().any(|s| s.id == song.id);
        if favorite {
            Arc::make_mut(&mut remote.favorites).insert(0, song.clone());
        } else {
            Arc::make_mut(&mut remote.favorites).retain(|s| s.id != song.id);
        }
        let id = song.id.clone();
        let rollback = song.clone();
        self.fetch(cx, move |c| c.set_favorite(&id, favorite), move |this, load, _| {
            if let Load::Failed(err) = load {
                if let Load::Ready(remote) = &mut this.remote {
                    if favorite {
                        Arc::make_mut(&mut remote.favorites).retain(|s| s.id != rollback.id);
                    } else {
                        Arc::make_mut(&mut remote.favorites).insert(0, rollback);
                    }
                }
                this.notice = Some(format!("Couldn't update favorite: {err}"));
            }
        });
        cx.notify();
    }

    pub(crate) fn song_in_library(&self, song: &Song) -> bool {
        match self.remote() {
            Some(r) => r.songs.iter().any(|s| s.id == song.id),
            None => self.library.has_song(&song.id),
        }
    }

    pub(crate) fn album_in_library(&self, album: &Album) -> bool {
        match self.remote() {
            Some(r) => r.albums.iter().any(|a| a.id == album.id || (!album.url.is_empty() && a.url == album.url)),
            None => self.library.has_album(&album.id),
        }
    }

    pub(crate) fn playlist_in_library(&self, playlist: &Playlist) -> bool {
        match self.remote() {
            Some(r) => playlist.id.starts_with("p.") || r.playlists.iter().any(|p| p.id == playlist.id),
            None => self.library.has_playlist(&playlist.id),
        }
    }

    /// Adds to the Apple Music library when signed in (Apple's API can add but not remove),
    /// otherwise toggles the item in the local library.
    pub(crate) fn add_to_library(&mut self, kind: &'static str, id: String, label: String, cx: &mut Context<Self>) {
        let request_id = id.clone();
        self.fetch(cx, move |c| c.add_to_library(kind, &request_id), move |this, load, _| match load {
            Load::Failed(err) => this.notice = Some(format!("Couldn't add {label}: {err}")),
            _ => {
                this.notice = Some(format!("Added {label} to your library"));
                this.notice_task = None;
            }
        });
        // Apple applies library additions asynchronously; refresh after a short delay.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            this.update(cx, |this, cx| {
                this.notice = None;
                this.load_remote_library(cx);
            })
            .ok();
        })
        .detach();
    }

    // Data loading

    pub(crate) fn fetch<T: Send + 'static>(
        &self,
        cx: &mut Context<Self>,
        job: impl FnOnce(Catalog) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, Load<T>, &mut Context<Self>) + 'static,
    ) {
        let catalog = self.catalog.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job(catalog) }).await;
            let load = match result {
                Ok(v) => Load::Ready(v),
                Err(e) => {
                    eprintln!("catalog request failed: {e:#}");
                    Load::Failed(format!("{e:#}"))
                }
            };
            this.update(cx, |this, cx| {
                done(this, load, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows the last cached charts immediately, then refreshes them. A failed refresh
    /// keeps the cached copy rather than replacing it with an error.
    pub(crate) fn load_browse(&mut self, cx: &mut Context<Self>) {
        let cache = api::cache_dir().join(if self.catalog.is_musickit() { "browse-musickit.json" } else { "browse.json" });
        if self.browse.ready().is_none() {
            self.browse = std::fs::read(&cache)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .map_or(Load::Loading, Load::Ready);
        }
        self.fetch(cx, |c| c.browse(), move |this, load, _| match load {
            Load::Ready(browse) => {
                if let Ok(json) = serde_json::to_vec(&browse) {
                    let _ = std::fs::create_dir_all(api::cache_dir());
                    let _ = std::fs::write(&cache, json);
                }
                crate::perf::milestone("charts ready");
                this.browse = Load::Ready(browse);
            }
            failed if this.browse.ready().is_none() => this.browse = failed,
            _ => {}
        });
        cx.notify();
    }

    fn ensure_route_data(&mut self, cx: &mut Context<Self>) {
        match self.route.clone() {
            Route::Album(id) if !matches!(self.albums.get(&id), Some(Load::Ready(_) | Load::Loading)) => {
                self.albums.insert(id.clone(), Load::Loading);
                let key = id.clone();
                self.fetch(cx, move |c| c.album(&id), move |this, load, _| {
                    this.albums.insert(key, load);
                });
            }
            Route::Artist(id) if !matches!(self.artists.get(&id), Some(Load::Ready(_) | Load::Loading)) => {
                self.artists.insert(id.clone(), Load::Loading);
                let key = id.clone();
                self.fetch(cx, move |c| c.artist(&id), move |this, load, _| {
                    this.artists.insert(key, load);
                });
            }
            Route::Radio if self.catalog.is_musickit() && self.stations.is_none() => {
                self.stations = Some(Load::Loading);
                self.fetch(cx, |c| c.live_stations(), |this, load, _| this.stations = Some(load));
            }
            Route::Playlist(p)
                if !matches!(self.playlist_tracks.get(&p.id), Some(Load::Ready(_) | Load::Loading)) =>
            {
                self.playlist_tracks.insert(p.id.clone(), Load::Loading);
                let id = p.id.clone();
                self.fetch(cx, move |c| c.playlist_tracks(&p.id), move |this, load, _| {
                    this.playlist_tracks.insert(id, load);
                });
            }
            _ => {}
        }
    }

    /// Loads a playlist's tracks in the background (e.g. to draw a cover mosaic for
    /// playlists Apple gives no artwork). Safe to call every frame.
    pub(crate) fn ensure_playlist_tracks(&self, playlist: &Playlist, cx: &mut Context<Self>) {
        if self.playlist_tracks.contains_key(&playlist.id)
            || !self.requested_playlists.borrow_mut().insert(playlist.id.clone())
        {
            return;
        }
        let id = playlist.id.clone();
        let key = id.clone();
        self.fetch(cx, move |c| c.playlist_tracks(&id), move |this, load, _| {
            this.playlist_tracks.insert(key, load);
        });
    }

    /// Catalog id for an artist known only by name (looked up once, then cached).
    pub(crate) fn resolve_artist_id(&self, name: &str, cx: &mut Context<Self>) -> Option<String> {
        let key = name.to_lowercase();
        if let Some(found) = self.artist_ids.borrow().get(&key) {
            return found.clone();
        }
        self.artist_ids.borrow_mut().insert(key.clone(), None);
        let name = name.to_string();
        self.fetch(cx, move |c| c.find_artist(&name), move |this, load, _| {
            if let Load::Ready(id) = load {
                this.artist_ids.borrow_mut().insert(key, id);
            }
        });
        None
    }

    pub(crate) fn ensure_artist(&self, id: &str, cx: &mut Context<Self>) {
        if self.artists.contains_key(id) || !self.requested_artists.borrow_mut().insert(id.to_string()) {
            return;
        }
        let (id, key) = (id.to_string(), id.to_string());
        self.fetch(cx, move |c| c.artist(&id), move |this, load, _| {
            this.artists.insert(key, load);
        });
    }

    /// Opens an artist page, looking the artist up by name when there's no id (chart and
    /// library songs from the Apple Music API don't carry one).
    pub(crate) fn open_artist(&mut self, name: String, id: Option<String>, cx: &mut Context<Self>) {
        if let Some(id) = id {
            return self.push(Route::Artist(id), cx);
        }
        // Collaborations are credited as "A & B" / "A, B"; the first name is the main artist.
        let primary = name
            .split([',', '&'])
            .next()
            .unwrap_or(&name)
            .trim()
            .to_string();
        let label = primary.clone();
        self.fetch(cx, move |c| c.find_artist(&primary), move |this, load, cx| match load {
            Load::Ready(Some(id)) => this.push(Route::Artist(id), cx),
            Load::Ready(None) => this.show_notice(format!("Couldn't find {label} on Apple Music"), cx),
            Load::Failed(err) => this.show_notice(format!("Couldn't open {label}: {err}"), cx),
            Load::Loading => {}
        });
    }

    pub(crate) fn retry_route(&mut self, cx: &mut Context<Self>) {
        match &self.route {
            Route::Album(id) => {
                self.albums.remove(id);
            }
            Route::Artist(id) => {
                self.artists.remove(id);
            }
            Route::Playlist(p) => {
                self.playlist_tracks.remove(&p.id);
            }
            Route::Radio => {
                self.stations = None;
            }
            Route::Search => {
                self.search_results = None;
                self.schedule_search(cx);
                return;
            }
            Route::RecentlyAdded | Route::Albums | Route::Artists | Route::Songs | Route::AllPlaylists
                if self.account == Account::SignedIn =>
            {
                return self.load_remote_library(cx);
            }
            _ => return self.load_browse(cx),
        }
        self.ensure_route_data(cx);
        cx.notify();
    }

    pub(crate) fn schedule_search(&mut self, cx: &mut Context<Self>) {
        let term = self.search.text().trim().to_string();
        if term.is_empty() {
            self.search_results = None;
            self.search_task = None;
            return;
        }
        if self.search_results.as_ref().is_some_and(|(t, _)| *t == term) {
            return;
        }
        let catalog = self.catalog.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            this.update(cx, |this, cx| {
                this.search_results = Some((term.clone(), Load::Loading));
                cx.notify();
            })
            .ok();
            let query = term.clone();
            let result = cx
                .background_executor()
                .spawn(async move { catalog.search(&query) })
                .await;
            this.update(cx, |this, cx| {
                if this.search.text().trim() == term {
                    this.search_results = Some((
                        term,
                        match result {
                            Ok(r) => Load::Ready(r),
                            Err(e) => Load::Failed(format!("{e:#}")),
                        },
                    ));
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Returns the cached file for `url`, starting a download if needed.
    pub(crate) fn image(&self, url: &str, cx: &mut Context<Self>) -> Option<Arc<std::path::Path>> {
        if url.is_empty() {
            return None;
        }
        if let Some(state) = self.images.borrow().get(url) {
            return match state {
                ImageState::Ready(path, _) => Some(path.clone()),
                _ => None,
            };
        }
        self.images
            .borrow_mut()
            .insert(url.to_string(), ImageState::Loading);
        let url = url.to_string();
        cx.spawn(async move |this, cx| {
            let fetch_url = url.clone();
            let result = cx
                .background_executor()
                .spawn(async move { api::download_cached(&fetch_url, "art", "jpg") })
                .await;
            this.update(cx, |this, cx| {
                let state = match result {
                    Ok(path) => ImageState::Ready(Arc::from(path.as_path()), std::time::Instant::now()),
                    Err(_) => ImageState::Failed,
                };
                this.images.borrow_mut().insert(url, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// The artwork's average color (computed once, in the background, from a small copy).
    pub(crate) fn art_color(&self, artwork: &crate::model::Artwork, cx: &mut Context<Self>) -> Option<gpui::Hsla> {
        if artwork.is_empty() {
            return None;
        }
        let url = artwork.url(60);
        if let Some(color) = self.art_colors.borrow().get(&url) {
            return *color;
        }
        self.art_colors.borrow_mut().insert(url.clone(), None);
        cx.spawn(async move |this, cx| {
            let fetch_url = url.clone();
            let color = cx
                .background_executor()
                .spawn(async move {
                    api::download_cached(&fetch_url, "art", "jpg")
                        .ok()
                        .and_then(|path| average_color(&path))
                })
                .await;
            this.update(cx, |this, cx| {
                this.art_colors.borrow_mut().insert(url, color);
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// An already-downloaded image (for other windows, which can't start downloads).
    pub(crate) fn cached_image(&self, url: &str) -> Option<Arc<std::path::Path>> {
        match self.images.borrow().get(url) {
            Some(ImageState::Ready(path, _)) => Some(path.clone()),
            _ => None,
        }
    }

    /// Eased 0–1 hover amount for an element registered with `set_hover`.
    pub(crate) fn hover_value(&self, key: &str) -> f32 {
        self.fx.borrow().hovers.get(key).map_or(0., |p| p.value())
    }

    /// Records a hover change; returns true if a redraw is needed.
    pub(crate) fn set_hover(&self, key: SharedString, hovered: bool) -> bool {
        let mut fx = self.fx.borrow_mut();
        let entry = fx
            .hovers
            .entry(key)
            .or_insert_with(|| crate::anim::Presence::new(Duration::from_millis(160)));
        let before = entry.value();
        entry.set(hovered);
        hovered || before > 0.
    }

    /// True for images that arrived in the last moment, so they fade in rather than pop;
    /// images already on disk when a page opens appear immediately.
    pub(crate) fn image_is_fresh(&self, url: &str) -> bool {
        matches!(self.images.borrow().get(url), Some(ImageState::Ready(_, at)) if at.elapsed() < Duration::from_millis(400))
    }

    // Navigation

    pub(crate) fn go_root(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_direction = NavDirection::Root;
        self.history.clear();
        self.scroll_history.clear();
        if route == Route::Search {
            window.focus(&self.search_focus);
        }
        self.set_route(route, cx);
    }

    pub(crate) fn push(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route == Route::Search {
            self.remember_search();
        }
        if route != self.route {
            self.history.push(self.route.clone());
            self.scroll_history.push(self.scroll.offset());
        }
        self.forward_history.clear();
        self.nav_direction = NavDirection::Forward;
        self.set_route(route, cx);
    }

    /// Goes back and restores where the previous page was scrolled to.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.history.pop() {
            let offset = self.scroll_history.pop();
            self.forward_history.push(self.route.clone());
            self.nav_direction = NavDirection::Back;
            self.set_route(route, cx);
            if let Some(offset) = offset {
                self.scroll.set_offset(offset);
            }
        }
    }

    /// ⌘]: re-open the page Back left.
    pub(crate) fn forward(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.forward_history.pop() {
            self.history.push(self.route.clone());
            self.scroll_history.push(self.scroll.offset());
            self.nav_direction = NavDirection::Forward;
            self.set_route(route, cx);
        }
    }

    pub(crate) fn go_search_page(&mut self, cx: &mut Context<Self>) {
        self.forward_history.clear();
        self.nav_direction = NavDirection::Root;
        self.set_route(Route::Search, cx);
    }

    fn set_route(&mut self, route: Route, cx: &mut Context<Self>) {
        self.multi.clear();
        self.route = route;
        self.route_generation += 1;
        self.menu = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.ensure_route_data(cx);
        cx.notify();
    }

    /// Short page name for perf logs.
    pub(crate) fn route_name(&self) -> &'static str {
        match self.route {
            Route::Home => "home",
            Route::New => "new",
            Route::Radio => "radio",
            Route::Search => "search",
            Route::RecentlyAdded => "recently-added",
            Route::Artists => "artists",
            Route::Albums => "albums",
            Route::Songs => "songs",
            Route::AllPlaylists => "all-playlists",
            Route::Favorites => "favorites",
            Route::Album(_) => "album",
            Route::Artist(_) => "artist",
            Route::Playlist(_) => "playlist",
        }
    }

    /// The top-level sidebar entry the current page belongs to.
    pub(crate) fn root_route(&self) -> &Route {
        self.history.first().unwrap_or(&self.route)
    }

    // Playback

    pub(crate) fn play(&mut self, songs: Vec<Song>, start: usize, shuffle: bool, cx: &mut Context<Self>) {
        let songs: Vec<Song> = songs.into_iter().collect();
        if songs.is_empty() {
            return;
        }
        self.queue.set(songs, start, shuffle);
        self.start_current(cx);
    }

    pub(crate) fn play_next(&mut self, song: Song, cx: &mut Context<Self>) {
        let was_empty = self.queue.current().is_none();
        self.queue.play_next(song);
        if was_empty {
            self.start_current(cx);
        }
        cx.notify();
    }

    /// Starts the queue's current song, cutting off anything playing (including a fade).
    fn start_current(&mut self, cx: &mut Context<Self>) {
        self.cancel_fade();
        if let Some(audio) = self.audio.borrow_mut().as_mut() {
            audio.stop();
        }
        self.start_track(cx);
    }

    /// Loads the current song into the (empty or reusable) current deck. During a
    /// crossfade it starts silent and the fade brings it up.
    fn start_track(&mut self, cx: &mut Context<Self>) {
        self.load_generation += 1;
        self.playback_error = None;
        self.loading_since = None;
        let Some(song) = self.queue.current().cloned() else {
            self.state = PlayState::Idle;
            return;
        };
        // Prefetch so lyrics are ready the moment Now Playing opens.
        self.ensure_lyrics(&song, cx);
        self.load_spectrum(&song, cx);
        if let Some(player) = self.full_player() {
            *self.audio.borrow_mut() = Some(Audio::Full(player.clone()));
            player.load(&song.id, self.start_volume());
            self.state = PlayState::Loading;
            self.loading_since = Some(std::time::Instant::now());
            cx.notify();
            return;
        }
        self.start_preview(song, cx);
    }

    /// The signed-in full-song player, created on first use. `None` when signed out or
    /// after full playback proved unavailable this session.
    fn full_player(&mut self) -> Option<Rc<WebPlayer>> {
        if self.account != Account::SignedIn || self.full_unavailable {
            return None;
        }
        // Any player except the one finishing a crossfade.
        let busy = match self.fade.as_ref().map(|f| &f.outgoing) {
            Some(Audio::Full(p)) => Some(p.clone()),
            _ => None,
        };
        if let Some(player) = self.web_players.iter().find(|p| busy.as_ref().is_none_or(|b| !Rc::ptr_eq(b, p))) {
            return Some(player.clone());
        }
        let (developer, user) = (self.catalog.developer_token()?, self.catalog.user_token()?);
        match WebPlayer::new(developer, user) {
            Ok(player) => {
                let player = Rc::new(player);
                self.web_players.push(player.clone());
                Some(player)
            }
            Err(e) => {
                eprintln!("full playback unavailable: {e:#}");
                self.full_unavailable = true;
                None
            }
        }
    }

    fn start_preview(&mut self, song: Song, cx: &mut Context<Self>) {
        let generation = self.load_generation;
        let Some(preview) = song.preview_url.clone() else {
            self.state = PlayState::Paused;
            self.playback_error = Some("No preview is available for this song.".into());
            cx.notify();
            return;
        };
        self.state = PlayState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { api::download_cached(&preview, "previews", "m4a") })
                .await;
            this.update(cx, |this, cx| {
                if this.load_generation != generation {
                    return;
                }
                if let Err(e) = result.and_then(|path| this.load_preview(&path)) {
                    this.state = PlayState::Paused;
                    this.playback_error = Some(format!("Couldn't play preview: {e:#}"));
                } else {
                    this.state = PlayState::Playing;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_preview(&mut self, path: &PathBuf) -> Result<()> {
        let mut slot = self.audio.borrow_mut();
        if !matches!(slot.as_ref(), Some(Audio::Preview(_))) {
            *slot = Some(Audio::Preview(Preview::new()?));
        }
        match slot.as_mut() {
            Some(Audio::Preview(preview)) => preview.load(path, self.start_volume()),
            _ => unreachable!(),
        }
    }

    /// Handles media keys / Control Center commands and keeps Now Playing current.
    fn media_tick(&mut self, cx: &mut Context<Self>) {
        let commands = self.media.borrow().poll();
        for command in commands {
            use crate::media::Command;
            match command {
                Command::Toggle => self.toggle_play(cx),
                Command::Play if self.state != PlayState::Playing => self.toggle_play(cx),
                Command::Pause if self.state == PlayState::Playing => self.toggle_play(cx),
                Command::Next => self.next(cx),
                Command::Previous => self.previous(cx),
                Command::Seek(t) => {
                    if let Some(audio) = self.audio.borrow().as_ref() {
                        audio.seek(t as f32);
                    }
                }
                _ => {}
            }
        }
        let now = self.queue.current().filter(|_| self.state != PlayState::Idle).map(|song| {
            let artwork_path = self.image(&song.artwork.url(600), cx).map(|p| p.to_string_lossy().into_owned());
            crate::media::NowPlaying {
                id: song.id.clone(),
                title: song.title.clone(),
                artist: song.artist.clone(),
                album: song.album.clone(),
                duration: self.duration() as f64,
                position: self.position() as f64,
                playing: matches!(self.state, PlayState::Playing | PlayState::Loading),
                artwork_path,
            }
        });
        self.media.borrow_mut().update(now);
    }

    /// Analyzes the song's preview clip for the player's visualizer.
    fn load_spectrum(&mut self, song: &Song, cx: &mut Context<Self>) {
        let cached = self.spectra.get(&song.id).cloned();
        self.progress.read(cx).spectrum.replace(cached.clone());
        let (Some(url), None) = (song.preview_url.clone(), cached) else { return };
        let id = song.id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { api::download_cached(&url, "previews", "m4a").and_then(|p| crate::spectrum::analyze(&p)) })
                .await;
            let Ok(spectrum) = result else { return };
            let spectrum = Arc::new(spectrum);
            this.update(cx, |this, cx| {
                this.spectra.insert(id.clone(), spectrum.clone());
                if this.queue.current().is_some_and(|s| s.id == id) {
                    this.progress.read(cx).spectrum.replace(Some(spectrum));
                }
            })
            .ok();
        })
        .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        match self.state {
            // A full song has started streaming, or failed to.
            PlayState::Loading if self.loading_since.is_some() => {
                let (playing, error) = match self.audio.borrow().as_ref() {
                    Some(Audio::Full(player)) => (player.is_playing(), player.startup_error()),
                    _ => (false, None),
                };
                let timed_out = self.loading_since.is_some_and(|t| t.elapsed() > Duration::from_secs(20));
                if playing {
                    eprintln!(
                        "full playback started ({:.0}s long)",
                        self.audio.borrow().as_ref().and_then(Audio::duration).unwrap_or(0.)
                    );
                    self.state = PlayState::Playing;
                    self.loading_since = None;
                    self.full_failures = 0;
                    cx.notify();
                } else if error.is_some() || timed_out {
                    let reason = error.unwrap_or_else(|| "timed out".into());
                    self.full_failures += 1;
                    // One failure can be a single unavailable song; repeated ones mean full
                    // playback doesn't work for this account, so stop trying.
                    if self.full_failures >= 2 {
                        self.full_unavailable = true;
                    }
                    self.loading_since = None;
                    // Stop the web player too, or a late-starting stream would play over the preview.
                    if let Some(audio) = self.audio.borrow_mut().as_mut() {
                        audio.stop();
                    }
                    if let Some(song) = self.queue.current().cloned() {
                        self.start_preview(song, cx);
                    }
                    self.playback_error = Some(format!("Couldn't stream the full song ({reason}) — playing a preview."));
                    cx.notify();
                }
            }
            PlayState::Playing if self.should_crossfade() => self.begin_crossfade(cx),
            // Only a finished track needs the whole app to redraw; the progress view
            // refreshes itself.
            PlayState::Playing if self.audio.borrow().as_ref().is_some_and(Audio::finished) => {
                match self.queue.track_ended() {
                    Advance::Play => self.start_current(cx),
                    Advance::Stop => {
                        self.state = PlayState::Idle;
                        if let Some(audio) = self.audio.borrow_mut().as_mut() {
                            audio.stop();
                        }
                    }
                }
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        match self.state {
            PlayState::Playing => {
                self.cancel_fade();
                if let Some(audio) = self.audio.borrow().as_ref() {
                    audio.pause();
                }
                self.state = PlayState::Paused;
            }
            PlayState::Paused if self.audio.borrow().as_ref().is_some_and(|a| !a.finished()) => {
                if let Some(audio) = self.audio.borrow().as_ref() {
                    audio.play();
                }
                self.state = PlayState::Playing;
            }
            PlayState::Paused | PlayState::Idle => {
                if self.queue.current().is_some() {
                    self.start_current(cx);
                } else if let Some(browse) = self.browse.ready() {
                    let songs = browse.best_new_songs();
                    self.play(songs, 0, false, cx);
                }
            }
            PlayState::Loading => {}
        }
        cx.notify();
    }

    pub(crate) fn next(&mut self, cx: &mut Context<Self>) {
        if self.queue.current().is_none() {
            return;
        }
        match self.queue.next() {
            Advance::Play => self.start_current(cx),
            Advance::Stop => {
                self.load_generation += 1;
                self.state = PlayState::Idle;
                if let Some(audio) = self.audio.borrow_mut().as_mut() {
                    audio.stop();
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn previous(&mut self, cx: &mut Context<Self>) {
        if self.queue.current().is_none() {
            return;
        }
        if self.queue.previous(self.position()) {
            self.start_current(cx);
        } else {
            self.cancel_fade();
            if let Some(audio) = self.audio.borrow().as_ref() {
                audio.seek(0.);
            }
        }
        cx.notify();
    }

    /// Volume a newly loaded song starts at: silent while crossfading in.
    fn start_volume(&self) -> f32 {
        if self.fade.is_some() { 0. } else { self.volume }
    }

    /// Seconds of overlap with the next song: the crossfade setting, or a near-instant
    /// handoff between consecutive album tracks, which should play gaplessly.
    fn transition_secs(&self) -> Option<f32> {
        let next = self.queue.peek_next()?;
        let current = self.queue.current()?;
        let consecutive = current.album_id.is_some()
            && current.album_id == next.album_id
            && next.track_number == current.track_number + 1;
        if consecutive {
            // Starts the next track just before this one ends; streams need a moment to
            // begin, so this lands close to seamless.
            Some(GAPLESS_LEAD)
        } else if self.crossfade_secs > 0 {
            Some(self.crossfade_secs as f32)
        } else {
            None
        }
    }

    fn should_crossfade(&self) -> bool {
        if self.fade.is_some() {
            return false;
        }
        let Some(secs) = self.transition_secs() else {
            return false;
        };
        let audio = self.audio.borrow();
        let Some(audio) = audio.as_ref() else {
            return false;
        };
        match audio.duration() {
            // Very short tracks just play out.
            Some(d) if d > secs * 2. => audio.position() >= d - secs,
            _ => false,
        }
    }

    /// Moves the playing song to the fade-out slot and starts the next one silently in a
    /// fresh deck; `step_fade` then trades their volumes.
    fn begin_crossfade(&mut self, cx: &mut Context<Self>) {
        let Some(outgoing) = self.audio.borrow_mut().take() else {
            return;
        };
        let remaining = outgoing.duration().unwrap_or(0.) - outgoing.position();
        #[cfg(any(debug_assertions, feature = "tour"))]
        eprintln!("crossfade begin: pos={:.1} dur={:?}", outgoing.position(), outgoing.duration());
        let secs = self.transition_secs().unwrap_or(self.crossfade_secs as f32);
        // Gapless handoffs are a quick swap, not an audible fade.
        let length = Duration::from_secs_f32(if secs <= GAPLESS_LEAD { 0.12 } else { remaining.clamp(0.5, secs) });
        if self.queue.next() != Advance::Play {
            *self.audio.borrow_mut() = Some(outgoing);
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(30)).await;
                match this.update(cx, |this, cx| this.step_fade(cx)) {
                    Ok(true) => {}
                    _ => break,
                }
            }
        });
        self.fade = Some(Fade { outgoing, length, created: std::time::Instant::now(), started: None, _task: task });
        self.start_track(cx);
        cx.notify();
    }

    /// One frame of an equal-power crossfade. Returns false once it's over.
    fn step_fade(&mut self, _cx: &mut Context<Self>) -> bool {
        let incoming_playing = self.audio.borrow().as_ref().is_some_and(Audio::is_playing);
        let volume = self.volume;
        let Some(fade) = self.fade.as_mut() else {
            return false;
        };
        // Hold the old song at full volume until the new one is actually audible.
        if fade.started.is_none() {
            if incoming_playing || fade.created.elapsed() > Duration::from_secs(4) {
                fade.started = Some(std::time::Instant::now());
            } else {
                return true;
            }
        }
        let t = (fade.started.unwrap().elapsed().as_secs_f32() / fade.length.as_secs_f32()).min(1.);
        #[cfg(any(debug_assertions, feature = "tour"))]
        if (0.44..0.5).contains(&t) || t >= 1. {
            eprintln!(
                "crossfade t={t:.2}: outgoing playing={} incoming playing={incoming_playing}",
                fade.outgoing.is_playing()
            );
        }
        let angle = t * std::f32::consts::FRAC_PI_2;
        fade.outgoing.set_volume(volume * angle.cos());
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.set_volume(volume * angle.sin());
        }
        if t >= 1. {
            if let Some(mut fade) = self.fade.take() {
                fade.outgoing.stop();
            }
            return false;
        }
        true
    }

    /// Ends any fade at once: the old song stops and the new one jumps to full volume.
    fn cancel_fade(&mut self) {
        if let Some(mut fade) = self.fade.take() {
            fade.outgoing.stop();
            if let Some(audio) = self.audio.borrow().as_ref() {
                audio.set_volume(self.volume);
            }
        }
    }

    pub(crate) fn save_settings(&self) {
        Settings {
            crossfade_secs: self.crossfade_secs,
            collapsed: self.collapsed_sections.iter().cloned().collect(),
            recent_searches: self.recent_searches.clone(),
        }
        .save();
    }

    pub(crate) fn toggle_section(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.collapsed_sections.remove(name) {
            self.collapsed_sections.insert(name.to_string());
        }
        self.save_settings();
        cx.notify();
    }

    /// Eased 0 (folded) – 1 (open) for a sidebar section.
    pub(crate) fn section_open(&self, name: &'static str) -> f32 {
        let open = !self.collapsed_sections.contains(name);
        let mut fx = self.fx.borrow_mut();
        let p = fx
            .sections
            .entry(name)
            .or_insert_with(|| {
                let d = Duration::from_millis(220);
                if open { crate::anim::Presence::new_open(d) } else { crate::anim::Presence::new(d) }
            });
        p.set(open);
        p.value()
    }

    pub(crate) fn set_crossfade(&mut self, secs: u32, cx: &mut Context<Self>) {
        self.crossfade_secs = secs;
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn jump_ahead(&mut self, n: usize, cx: &mut Context<Self>) {
        if self.queue.jump_ahead(n) {
            self.start_current(cx);
        }
    }

    pub(crate) fn position(&self) -> f32 {
        match self.state {
            PlayState::Playing | PlayState::Paused => self.audio.borrow().as_ref().map_or(0., Audio::position),
            _ => 0.,
        }
    }

    pub(crate) fn duration(&self) -> f32 {
        match self.state {
            // Until the stream reports a length, use the catalog's (full songs aren't 30s).
            PlayState::Playing | PlayState::Paused => self
                .audio
                .borrow()
                .as_ref()
                .and_then(Audio::duration)
                .unwrap_or_else(|| self.catalog_duration()),
            _ => self.catalog_duration(),
        }
    }

    pub(crate) fn is_full_playback(&self) -> bool {
        self.audio.borrow().as_ref().is_some_and(Audio::is_full)
    }

    fn catalog_duration(&self) -> f32 {
        let full = self.audio.borrow().as_ref().is_some_and(Audio::is_full);
        match self.queue.current() {
            Some(s) if full && s.duration_ms > 0 => s.duration_ms as f32 / 1000.,
            _ => 30.,
        }
    }

    pub(crate) fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.volume = volume.clamp(0., 1.);
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.set_volume(self.volume);
        }
        cx.notify();
    }

    /// Progress scrubs horizontally; volume is a vertical slider (louder toward the top).
    pub(crate) fn drag_to(&mut self, drag: Drag, at: Point<Pixels>, cx: &mut Context<Self>) {
        let fraction = match drag {
            Drag::BigProgress => {
                let b = self.np_progress_bounds.get();
                if b.size.width <= px(0.) {
                    return;
                }
                ((at.x - b.origin.x) / b.size.width).clamp(0., 1.)
            }
            Drag::Progress => {
                let b = self.progress_bounds.get();
                if b.size.width <= px(0.) {
                    return;
                }
                ((at.x - b.origin.x) / b.size.width).clamp(0., 1.)
            }
            Drag::Volume => {
                let b = self.volume_bounds.get();
                if b.size.height <= px(0.) {
                    return;
                }
                (1. - (at.y - b.origin.y) / b.size.height).clamp(0., 1.)
            }
        };
        match drag {
            // Scrubbing only moves the bar; the seek happens once on release (see end_drag),
            // since a stream of seeks makes the full-song player stutter.
            Drag::Progress => self.progress.update(cx, |p, cx| {
                p.set_scrub(Some(fraction));
                cx.notify();
            }),
            Drag::Volume => self.set_volume(fraction, cx),
            Drag::BigProgress => {
                self.np_scrub = Some(fraction);
                cx.notify();
            }
        }
    }

    pub(crate) fn seek_to(&mut self, secs: f32, cx: &mut Context<Self>) {
        if let Some(audio) = self.audio.borrow().as_ref() {
            audio.seek(secs);
        }
        // Let the lyrics jump to the new line right away.
        self.lyrics_follow.borrow_mut().user_until = None;
        cx.notify();
    }

    pub(crate) fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag == Some(Drag::BigProgress) {
            self.drag = None;
            if let Some(f) = self.np_scrub.take() {
                let secs = f * self.duration();
                self.seek_to(secs, cx);
            }
            return;
        }
        if self.drag.take() == Some(Drag::Progress) {
            let fraction = self.progress.read(cx).scrub();
            let duration = self.duration();
            if let (Some(f), Some(audio)) = (fraction, self.audio.borrow().as_ref()) {
                audio.seek(f * duration);
            }
            // Keep showing the target until the player reports the new position.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(600)).await;
                this.update(cx, |this, cx| {
                    if this.drag.is_none() {
                        this.progress.update(cx, |p, cx| {
                            p.set_scrub(None);
                            cx.notify();
                        });
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    // Library

    pub(crate) fn open_url(&self, url: &str, cx: &mut Context<Self>) {
        if !url.is_empty() {
            cx.open_url(url);
        }
    }

    pub(crate) fn song_menu(&self, song: &Song, position: Point<Pixels>) -> ContextMenu {
        let entry = |label: &str, icon: &'static str, action: MenuAction| {
            Some(MenuEntry {
                label: SharedString::from(label.to_string()),
                icon,
                action,
            })
        };
        let s = song.clone();
        let mut entries = vec![entry(
            "Play Next",
            "play-next",
            Rc::new(move |this, _, cx| this.play_next(s.clone(), cx)),
        )];
        if self.account == Account::SignedIn {
            let (s, at) = (song.clone(), position);
            entries.push(entry(
                "Add to Playlist…",
                "playlist",
                // The picker replaces this menu at the same spot (the menu is cleared just
                // before an entry's action runs).
                Rc::new(move |this, _, _| this.menu = Some(this.playlist_picker(std::slice::from_ref(&s), at))),
            ));
        }
        let s = song.clone();
        let in_library = self.song_in_library(song);
        if self.account == Account::SignedIn {
            entries.push(if in_library {
                entry(
                    "In Your Library",
                    "check",
                    Rc::new(|this, _, cx| {
                        this.show_notice("Apple's API can't remove songs. Remove it in the Music app.".into(), cx)
                    }),
                )
            } else {
                entry(
                    "Add to Library",
                    "plus",
                    Rc::new(move |this, _, cx| {
                        this.add_to_library("songs", s.id.clone(), format!("“{}”", s.title), cx)
                    }),
                )
            });
        } else {
            entries.push(entry(
                if in_library { "Remove from Library" } else { "Add to Library" },
                if in_library { "check" } else { "plus" },
                Rc::new(move |this, _, _| this.library.toggle_song(&s)),
            ));
        }
        let s = song.clone();
        let favorite = self.is_favorite(song);
        entries.push(entry(
            if favorite { "Undo Favorite" } else { "Favorite" },
            if favorite { "heart-fill" } else { "heart" },
            Rc::new(move |this, _, cx| this.toggle_favorite(&s, cx)),
        ));
        entries.push(None);
        if let Some(album_id) = song.album_id.clone() {
            entries.push(entry(
                "Go to Album",
                "album",
                Rc::new(move |this, _, cx| this.push(Route::Album(album_id.clone()), cx)),
            ));
        }
        let (artist, artist_id) = (song.artist.clone(), song.artist_id.clone());
        entries.push(entry(
            "Go to Artist",
            "mic",
            Rc::new(move |this, _, cx| this.open_artist(artist.clone(), artist_id.clone(), cx)),
        ));
        let url = song.url.clone();
        entries.push(None);
        entries.push(entry(
            "Open in Apple Music",
            "open",
            Rc::new(move |this, _, cx| this.open_url(&url, cx)),
        ));
        ContextMenu { position, entries }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if self.filter_focus.is_focused(window) {
            if self.filter_key(k, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if self.new_playlist_focus.is_focused(window) {
            if self.new_playlist_key(k, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        if self.search_focus.is_focused(window) {
            if self.search_key(k, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        let plain = !k.modifiers.modified();
        match k.key.as_str() {
            "escape" => {
                self.dismiss_top(window, cx);
            }
            "space" if plain => self.toggle_play(cx),
            "up" if plain => self.move_selection(-1, cx),
            "down" if plain => self.move_selection(1, cx),
            "enter" if plain => {
                self.play_selection(cx);
            }
            // Typing "/" anywhere jumps to search, like many web apps.
            "/" if plain => self.open_palette(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::frame_start(self.route_name());
        if !self.first_frame_logged.replace(true) {
            crate::perf::milestone("first frame");
        }
        let t = Theme::for_appearance(window.appearance());
        let viewport_w = f32::from(window.viewport_size().width);
        // Pages span the whole window (art and tints run under the floating sidebar);
        // this is the visible width beside it.
        let content_w = viewport_w - SIDEBAR_W;
        let search_focused = self.search_focus.is_focused(window);
        self.filter_focused = self.filter_focus.is_focused(window);
        {
            let mut fx = self.fx.borrow_mut();
            fx.queue.set(self.show_queue);
            fx.account.set(self.show_account && self.account == Account::SignedIn);
            fx.volume.set(self.show_volume);
            fx.key_help.set(self.show_key_help);
            fx.now_playing.set(self.show_now_playing && self.queue.current().is_some());
            fx.palette.set(self.palette_open);
            fx.shortcuts.set(self.show_shortcuts);
            let message = self.notice.clone().or_else(|| self.playback_error.clone());
            if let Some(m) = &message {
                fx.last_notice = m.clone();
            }
            fx.notice.set(message.is_some());
            // Forget finished hover animations so the map doesn't grow without bound.
            fx.hovers.retain(|_, h| h.visible() || h.animating());
        }
        // The sidebar floats over the page, so on artwork-tinted pages it takes the page's
        // colors (keeping the app's red for selection).
        let sidebar_t = self
            .page_palette(cx)
            .map(|p| Theme { accent: t.accent, ..t.tinted(p) })
            .unwrap_or(t);

        div()
            .id("root")
            .track_focus(&self.focus)
            .key_context("MusicApp")
            .on_action(cx.listener(|this, _: &PlayPause, _, cx| this.toggle_play(cx)))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| this.next(cx)))
            .on_action(cx.listener(|this, _: &PreviousTrack, _, cx| this.previous(cx)))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| this.set_volume(this.volume + 0.1, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| this.set_volume(this.volume - 0.1, cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.go_root(Route::Search, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.back(cx)))
            .on_action(cx.listener(|this, _: &ToggleQueue, _, cx| {
                this.show_queue = !this.show_queue;
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .map(|el| self.register_key_actions(el, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    let had_overlay = this.menu.take().is_some()
                        || std::mem::take(&mut this.show_account)
                        || std::mem::take(&mut this.show_volume);
                    if this.search_focus.is_focused(window) {
                        window.focus(&this.focus);
                    }
                    if had_overlay {
                        cx.notify();
                    }
                }),
            )
            .child(drag_tracker(cx.weak_entity()))
            .size_full()
            .flex()
            .bg(t.window)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .text_color(t.text)
            .child(self.render_main(&t, content_w, search_focused, cx))

            .child(self.render_sidebar(&sidebar_t, cx))
            .children(self.render_menu(&t, cx))
            .children(self.render_account_popover(&t, cx))
            .children(self.render_volume_popover(&t, cx))
            .children(self.render_now_playing(&t, window, cx))
            .children(self.render_palette(&t, cx))
            .children(self.render_new_playlist(&t, cx))
            .children(self.render_shortcuts(&t, cx))
            // Last, so it sees transitions started anywhere above.
            .child(gpui::canvas(
                |_, _, _| {},
                {
                    // Keep drawing frames only while something is mid-transition.
                    let animating = self.fx.borrow().animating();
                    move |_, _, window, _| {
                        crate::perf::frame_end();
                        if animating {
                            window.request_animation_frame();
                        }
                    }
                },
            ))
    }
}

/// Follows slider drags anywhere in the window. Element mouse handlers only fire while
/// the pointer is over that element, and the player and popovers occlude the rest of the
/// app, so a drag that strays off the slider would otherwise stop tracking.
fn drag_tracker(app: gpui::WeakEntity<MusicApp>) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let on_move = app.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase != gpui::DispatchPhase::Bubble {
                    return;
                }
                on_move
                    .update(cx, |this, cx| {
                        if let Some(drag) = this.drag {
                            if e.pressed_button == Some(MouseButton::Left) {
                                this.drag_to(drag, e.position, cx);
                            } else {
                                this.end_drag(cx);
                            }
                        }
                    })
                    .ok();
            });
            let on_up = app.clone();
            window.on_mouse_event(move |e: &gpui::MouseUpEvent, phase, _, cx| {
                if phase == gpui::DispatchPhase::Bubble && e.button == MouseButton::Left {
                    on_up.update(cx, |this, cx| this.end_drag(cx)).ok();
                }
            });
        },
    )
    .absolute()
    .size_0()
}

fn average_color(path: &std::path::Path) -> Option<gpui::Hsla> {
    let pixels = image::open(path).ok()?.thumbnail(16, 16).to_rgb8();
    let n = (pixels.width() * pixels.height()) as f32;
    let [mut r, mut g, mut b] = [0f32; 3];
    for p in pixels.pixels() {
        r += p[0] as f32;
        g += p[1] as f32;
        b += p[2] as f32;
    }
    Some(gpui::Rgba { r: r / n / 255., g: g / n / 255., b: b / n / 255., a: 1. }.into())
}

fn remote_cache_path() -> PathBuf {
    api::cache_dir().join("library-remote.json")
}

fn saved_photo() -> Option<PathBuf> {
    std::fs::read_dir(api::data_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("profile-photo-")))
        .max()
}

fn remove_old_photos(keep: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(api::data_dir()) {
        for path in entries.flatten().map(|e| e.path()) {
            let is_photo = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("profile-photo-"));
            if is_photo && path != keep {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn user_full_name() -> String {
    std::process::Command::new("id")
        .arg("-F")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "Listener".into())
}
