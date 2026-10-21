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
    Bounds, Context, FocusHandle, KeyDownEvent, MouseButton, MouseMoveEvent, Pixels, Point, ScrollHandle, SharedString,
    Task, Window, actions, div, point, prelude::*, px,
};

use crate::{
    api::{self, Catalog},
    audio::{Audio, FullPlayer, Preview},
    auth,
    library::Library,
    model::{Album, ArtistDetail, Browse, Playlist, RemoteLibrary, SearchResults, Song, Station},
    queue::{Advance, Queue},
    theme::Theme,
    webplayer::WebPlayer,
};

mod account;
mod data;
mod media;
mod navigation;
mod playback;
mod session;
mod settings;
#[cfg(any(debug_assertions, feature = "tour"))]
mod tour;

actions!(music, [PlayPause, NextTrack, PreviousTrack, VolumeUp, VolumeDown, FocusSearch, GoBack, ToggleQueue, Quit]);

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
    /// Everything in a shelf whose header was clicked.
    SeeAll(Arc<SeeAll>),
}

/// A shelf's full contents, for its See All page.
#[derive(Clone, Debug)]
pub struct SeeAll {
    pub title: SharedString,
    pub items: SeeAllItems,
}

#[derive(Clone, Debug)]
pub enum SeeAllItems {
    Songs(Arc<Vec<crate::model::Song>>),
    Albums(Arc<Vec<crate::model::Album>>),
    Playlists(Arc<Vec<Playlist>>),
    Artists(Arc<Vec<crate::model::Artist>>),
    Stations(Arc<Vec<crate::model::Station>>),
    Mixed(Arc<Vec<crate::model::TopResult>>),
}

impl SeeAllItems {
    fn len(&self) -> usize {
        match self {
            Self::Songs(v) => v.len(),
            Self::Albums(v) => v.len(),
            Self::Playlists(v) => v.len(),
            Self::Artists(v) => v.len(),
            Self::Stations(v) => v.len(),
            Self::Mixed(v) => v.len(),
        }
    }
}

/// Mixed search results have no equality, so pages compare by title and size, which is
/// what history needs to tell them apart.
impl PartialEq for SeeAll {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title && self.items.len() == other.items.len()
    }
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

#[derive(Clone, Copy, PartialEq, Debug)]
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
    /// The user has turned on volume control for the native player (and so been asked for
    /// System Audio Recording permission).
    #[serde(default)]
    native_volume: bool,
    /// View > Animate Artwork: covers and headers slowly drift and zoom.
    #[serde(default)]
    animate_artwork: bool,
}

fn default_crossfade() -> u32 {
    6
}

impl Settings {
    fn path() -> PathBuf {
        api::data_dir().join("settings.json")
    }

    fn load() -> Self {
        std::fs::read(Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Self {
            crossfade_secs: default_crossfade(),
            collapsed: Vec::new(),
            recent_searches: Vec::new(),
            native_volume: false,
            animate_artwork: false,
        })
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
    /// When the current page was navigated to, for its entrance.
    pub page_entered: std::time::Instant,
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
            page_entered: std::time::Instant::now().checked_sub(PAGE_ENTRANCE).unwrap_or_else(std::time::Instant::now),
        }
    }

    /// How far the current page's entrance has come, eased, from 0 to 1. It's timed from the
    /// navigation rather than held in element state, so a redraw can never restart it.
    pub(crate) fn page_progress(&self) -> f32 {
        if crate::anim::reduce_motion() {
            return 1.;
        }
        let t = (self.page_entered.elapsed().as_secs_f32() / PAGE_ENTRANCE.as_secs_f32()).min(1.);
        1. - (1. - t).powi(5)
    }

    /// Transitions drawn by the root view: overlays and panels.
    fn animating(&self) -> bool {
        self.queue.animating()
            || self.account.animating()
            || self.volume.animating()
            || self.notice.animating()
            || self.key_help.animating()
            || self.now_playing.animating()
            || self.palette.animating()
            || self.shortcuts.animating()
    }

    /// Transitions inside one cached region, which requests its own frames so the other
    /// regions stay cached while it animates.
    pub(crate) fn region_animating(&self, region: crate::ui::regions::Region) -> bool {
        use crate::ui::regions::Region;
        let hovers = |sidebar: bool| self.hovers.iter().any(|(k, h)| k.starts_with("nav-") == sidebar && h.animating());
        match region {
            Region::Page => self.page_bg.animating() || hovers(false) || self.page_progress() < 1.,
            Region::Content => hovers(false) || self.page_progress() < 1.,
            Region::Sidebar => {
                self.sidebar_bg.animating() || self.sections.values().any(|s| s.animating()) || hovers(true)
            }
            Region::Player => self.pill_bg.animating(),
        }
    }
}

const PAGE_ENTRANCE: Duration = Duration::from_millis(280);

pub(crate) type DeferredFetch = Box<dyn FnOnce(&mut MusicApp, &mut Context<MusicApp>)>;

pub struct MusicApp {
    pub(crate) catalog: Catalog,
    pub(crate) catalog_note: Option<String>,
    pub(crate) browse: Load<Browse>,
    pub(crate) albums: HashMap<String, Load<(Album, Vec<Song>)>>,
    pub(crate) artists: HashMap<String, Load<ArtistDetail>>,
    pub(crate) playlist_tracks: HashMap<String, Load<Option<Vec<Song>>>>,
    /// Up to four song covers per playlist without its own artwork, saved so the sidebar and
    /// mosaics show them at launch instead of waiting for every playlist's tracks.
    pub(crate) playlist_covers: RefCell<HashMap<String, Vec<crate::model::Artwork>>>,
    /// Requests made while the Keychain is still being read for the user token. They wait for
    /// it: sent without it, anything under `me/` fails with 403.
    pub(crate) awaiting_token: RefCell<Option<Vec<DeferredFetch>>>,
    /// Playlists whose tracks were requested only to build a cover mosaic.
    requested_playlists: RefCell<std::collections::HashSet<String>>,
    /// Artists fetched in the background (e.g. for "More by" under an album).
    requested_artists: RefCell<std::collections::HashSet<String>>,
    pub(crate) related_albums: HashMap<String, Load<Vec<Album>>>,
    requested_related: RefCell<std::collections::HashSet<String>>,
    pub(crate) stations: Option<Load<Vec<Station>>>,
    /// Name of the artist selected in the library's Artists page.
    pub(crate) selected_library_artist: Option<String>,
    /// Keys of descriptions expanded with MORE.
    pub(crate) expanded_notes: std::collections::HashSet<String>,
    pub(crate) search: tahoe::TextField,
    pub(crate) palette_open: bool,
    pub(crate) palette_index: usize,
    pub(crate) show_shortcuts: bool,
    pub(crate) recent_searches: Vec<String>,
    /// Lets ↑/↓ keep the selection in view in the virtualized song lists.
    pub(crate) song_list_scroll: gpui::UniformListScrollHandle,
    #[allow(clippy::type_complexity)]
    pub(crate) local_matches_cache:
        RefCell<Option<((String, (usize, usize, usize, usize, usize)), Arc<crate::keys::LocalMatches>)>>,
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
    pub(crate) list_views: RefCell<HashMap<&'static str, crate::ui::list_tools::ListView>>,
    #[allow(clippy::type_complexity)]
    pub(crate) shaped_cache:
        RefCell<Option<((&'static str, crate::ui::list_tools::SortKey, String, usize, usize), Arc<Vec<Song>>)>>,
    /// Songs selected with ⌘/⇧-click (empty: just `selected_song`).
    pub(crate) multi: Vec<String>,
    pub(crate) select_anchor: Option<usize>,
    pub(crate) filter_focus: FocusHandle,
    pub(crate) filter_target: &'static str,
    pub(crate) filter_focused: bool,
    pub(crate) personal: Option<Load<Option<crate::model::PersonalHome>>>,
    pub(crate) new_playlist: Option<crate::ui::playlists::NewPlaylist>,
    pub(crate) new_playlist_focus: FocusHandle,
    pub(crate) now_playing_tab: crate::ui::now_playing::NowPlayingTab,
    pub(crate) lyrics: HashMap<String, Load<Option<crate::lyrics::Lyrics>>>,
    pub(crate) lyrics_scroll: ScrollHandle,
    pub(crate) lyrics_follow: RefCell<crate::ui::now_playing::LyricsScroll>,
    pub(crate) np_progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) np_scrub: Option<f32>,
    media: RefCell<crate::media::MediaControls>,
    #[allow(clippy::type_complexity)]
    sorted_albums_cache: RefCell<Option<((usize, usize, usize, usize, usize), Arc<Vec<Album>>)>>,
    #[allow(clippy::type_complexity)]
    artist_rows_cache: RefCell<
        Option<((usize, usize, usize, usize, usize), Arc<Vec<(String, Option<String>, crate::model::Artwork)>>)>,
    >,
    first_frame_logged: Cell<bool>,
    #[cfg(any(debug_assertions, feature = "tour"))]
    pub(crate) tour_scroll: Cell<Option<std::time::Instant>>,
    /// How far down the page (in scrolled pixels) something drawn depends on the scroll
    /// offset, such as a parallax header. Set while the page content renders.
    pub(crate) scroll_dependent_until: std::rc::Rc<Cell<f32>>,
    pub(crate) tour_scroll_cpu: Cell<Duration>,
    pub(crate) queue: Queue<Song>,
    pub(crate) state: PlayState,
    pub(crate) playback_error: Option<String>,
    /// Shared with the progress view, which redraws itself while playing.
    pub(crate) audio: crate::ui::progress::SharedAudio,
    pub(crate) progress: gpui::Entity<crate::ui::progress::Progress>,
    /// Full-song players (Apple's web player), created on demand. A crossfade needs two:
    /// one finishing the old song while the other starts the next.
    web_players: Vec<Rc<WebPlayer>>,
    /// MusicKit's native player, once the user has allowed Apple Music access.
    pub(crate) native_player: Option<Rc<crate::native::NativePlayer>>,
    /// The native player can't work here (macOS 13, no app bundle, access denied).
    native_unavailable: bool,
    pub(crate) native_volume_allowed: bool,
    pub(crate) animate_artwork: bool,
    pub(crate) session_saved_at: Option<std::time::Instant>,
    /// A restored session's song and position, applied when that song starts playing.
    pub(crate) resume_at: Option<(String, f32)>,
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
    /// When each loaded image file was last drawn (see `start_image_eviction`).
    pub(crate) image_used: RefCell<HashMap<Arc<std::path::Path>, std::time::Instant>>,
    /// The song row, and which of its links (0 the album, then each credited artist), under the
    /// pointer; that row doesn't highlight.
    pub(crate) hovered_link: RefCell<Option<(SharedString, u8)>>,
    pub(crate) evict_after: Duration,
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
    pub(crate) native_fill: RefCell<Option<crate::ui::native_fill::NativeFill>>,
    pub(crate) volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) show_volume: bool,
    /// The player's speaker button, which the volume popover opens above.
    /// The volume before muting, which unmuting returns to.
    pub(crate) muted_volume: Option<f32>,
    pub focus: FocusHandle,
    pub(crate) search_focus: FocusHandle,
    pub(crate) scroll: ScrollHandle,
    pub(crate) artist_pane_scroll: ScrollHandle,
    pub(crate) user_name: String,
    /// Profile photo the user chose (Apple doesn't share Apple Music profile photos).
    pub(crate) avatar: Option<Arc<std::path::Path>>,
    pub(crate) regions: Option<crate::ui::regions::Regions>,
    /// The playback tick is running (see `ensure_ticker`).
    pub(crate) ticking: bool,
}

impl MusicApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, _, cx| cx.notify()).detach();

        let audio: crate::ui::progress::SharedAudio = Rc::default();
        let progress_bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let app = cx.weak_entity();
        let progress = cx.new(|_| crate::ui::progress::Progress::new(audio.clone(), progress_bounds.clone(), app));
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
            playlist_covers: RefCell::new(data::load_playlist_covers()),
            awaiting_token: RefCell::new(None),
            requested_playlists: RefCell::default(),
            requested_artists: RefCell::default(),
            related_albums: HashMap::new(),
            requested_related: RefCell::default(),
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
            #[cfg(any(debug_assertions, feature = "tour"))]
            tour_scroll: Cell::new(None),
            scroll_dependent_until: Default::default(),
            tour_scroll_cpu: Cell::new(Duration::ZERO),
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
            now_playing_tab: crate::ui::now_playing::NowPlayingTab::Lyrics,
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
            native_player: None,
            native_unavailable: false,
            native_volume_allowed: Settings::load().native_volume,
            animate_artwork: Settings::load().animate_artwork,
            session_saved_at: None,
            resume_at: None,
            fade: None,
            crossfade_secs: Settings::load().crossfade_secs,
            collapsed_sections: Settings::load().collapsed.into_iter().collect(),
            full_unavailable: false,
            full_failures: 0,
            loading_since: None,
            load_generation: 0,
            volume: 0.8,
            images: RefCell::default(),
            image_used: RefCell::default(),
            hovered_link: RefCell::default(),
            evict_after: Duration::from_secs(120),
            art_colors: RefCell::default(),
            menu: None,
            show_queue: false,
            show_account: false,
            account_bounds: Rc::default(),
            library_updated: None,
            selected_song: None,
            drag: None,
            progress_bounds,
            native_fill: RefCell::new(None),
            volume_bounds: Rc::default(),
            show_volume: false,
            muted_volume: None,
            focus: cx.focus_handle(),
            search_focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            artist_pane_scroll: ScrollHandle::new(),
            user_name: user_full_name(),
            avatar: saved_photo().map(|p| Arc::from(p.as_path())),
            regions: None,
            ticking: false,
        };
        crate::perf::milestone("state built");
        if let Some(mut commands) = this.media.borrow_mut().take_commands() {
            cx.spawn(async move |this, cx| {
                use futures::StreamExt as _;
                while let Some(command) = commands.next().await {
                    if this.update(cx, |this, cx| this.handle_media_command(command, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        this.load_browse(cx);
        this.restore_session(cx);
        this.start_image_eviction(cx);
        cx.on_app_quit(|this, _| {
            this.save_session();
            async {}
        })
        .detach();
        crate::perf::milestone("charts cache read");
        #[cfg(any(debug_assertions, feature = "tour"))]
        if std::env::var_os("ENCORE_TOUR").is_some() {
            this.start_tour(cx);
        }
        if this.catalog.is_musickit() {
            this.restore_sign_in(cx);
        }
        this
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

    pub(crate) fn open_url(&self, url: &str, cx: &mut Context<Self>) {
        if !url.is_empty() {
            cx.open_url(url);
        }
    }

    pub(crate) fn song_menu(&self, song: &Song, position: Point<Pixels>) -> ContextMenu {
        let entry = |label: &str, icon: &'static str, action: MenuAction| {
            Some(MenuEntry { label: SharedString::from(label.to_string()), icon, action })
        };
        let s = song.clone();
        let mut entries =
            vec![entry("Play Next", "play-next", Rc::new(move |this, _, cx| this.play_next(s.clone(), cx)))];
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
        entries.push(entry("Open in Apple Music", "open", Rc::new(move |this, _, cx| this.open_url(&url, cx))));
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
        crate::perf::frame_start(self.route_name(), window.frame_timings());
        if !self.first_frame_logged.replace(true) {
            crate::perf::milestone("first frame");
        }
        let t = Theme::for_appearance(window.appearance());
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
        let regions = self.regions.get_or_insert_with(|| crate::ui::regions::Regions::new(cx)).views();
        let key_help = self.fx.borrow().key_help;
        // Under a fully open Now Playing nothing else shows, so the rest isn't drawn at all.
        let now_playing = self.fx.borrow().now_playing;
        let covered = now_playing.value() >= 1. && !now_playing.animating();

        div()
            .id("root")
            .track_focus(&self.focus)
            .key_context("MusicApp")
            .on_action(cx.listener(|this, _: &PlayPause, _, cx| this.toggle_play(cx)))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| this.next(cx)))
            .on_action(cx.listener(|this, _: &PreviousTrack, _, cx| this.previous(cx)))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| this.change_volume(this.volume + 0.1, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| this.change_volume(this.volume - 0.1, cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.go_root(Route::Search, window, cx);
            }))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.back(cx)))
            // A mouse's back and forward buttons.
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.forward(cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::keys::ToggleAnimatedArtwork, _, cx| this.toggle_animate_artwork(cx)),
            )
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
            .when(!covered, |d| d.children(regions))
            .when(key_help.visible(), |d| d.child(self.render_key_help(&t, cx).opacity(key_help.value())))
            .children(self.render_menu(&t, cx))
            .children(self.render_account_popover(&t, cx))
            .children(self.render_now_playing(&t, window, cx))
            .children(self.render_palette(&t, cx))
            .children(self.render_new_playlist(&t, cx))
            .children(self.render_shortcuts(&t, cx))
            // Last, so it sees transitions started anywhere above.
            .child(gpui::canvas(|_, _, _| {}, {
                // Keep drawing frames only while something is mid-transition.
                let animating = self.fx.borrow().animating();
                move |_, _, window, _| {
                    crate::perf::frame_end();
                    if animating {
                        window.request_animation_frame();
                    }
                }
            }))
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
