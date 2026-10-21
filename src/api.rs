//! Catalog access. Two backends:
//! - `Public`: Apple's no-auth RSS charts + iTunes Search/Lookup APIs.
//! - `MusicKit`: the official Apple Music API, used when a developer token is configured.
//!   With a Music User Token it can also read and add to the user's library.
//!
//! All calls block; run them on a background executor.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context as _, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    Album, ArtColors, Artist, ArtistDetail, Artwork, Browse, PersonalHome, Playlist, RemoteLibrary, SearchResults,
    Song, Station, TopResult,
};

pub const STOREFRONT: &str = "us";

/// Developer token, plus the signed-in user's Music User Token when there is one.
#[derive(Clone)]
pub struct Auth {
    pub developer: String,
    pub user: Option<String>,
}

#[derive(Clone)]
pub enum Catalog {
    Public,
    MusicKit(Auth),
}

impl Catalog {
    /// Uses MusicKit if credentials are configured, otherwise the public APIs.
    pub fn from_env() -> (Self, Option<String>) {
        match musickit_token() {
            Ok(Some(developer)) => (Self::MusicKit(Auth { developer, user: None }), None),
            Ok(None) => (Self::Public, None),
            Err(err) => (Self::Public, Some(format!("MusicKit disabled: {err:#}"))),
        }
    }

    pub fn is_musickit(&self) -> bool {
        matches!(self, Self::MusicKit(_))
    }

    pub fn developer_token(&self) -> Option<&str> {
        match self {
            Self::MusicKit(auth) => Some(&auth.developer),
            Self::Public => None,
        }
    }

    pub fn user_token(&self) -> Option<&str> {
        match self {
            Self::MusicKit(auth) => auth.user.as_deref(),
            Self::Public => None,
        }
    }

    pub fn set_user_token(&mut self, token: Option<String>) {
        if let Self::MusicKit(auth) = self {
            auth.user = token;
        }
    }

    pub fn browse(&self) -> Result<Browse> {
        match self {
            Self::Public => public::browse(),
            Self::MusicKit(auth) => musickit::browse(auth),
        }
    }

    pub fn album(&self, id: &str) -> Result<(Album, Vec<Song>)> {
        match self {
            Self::Public => public::album(id),
            Self::MusicKit(auth) => musickit::album(auth, id),
        }
    }

    /// Albums Apple suggests for fans of this one ("You Might Also Like"); none without MusicKit.
    pub fn related_albums(&self, id: &str) -> Result<Vec<Album>> {
        match self {
            Self::Public => Ok(Vec::new()),
            Self::MusicKit(auth) => musickit::related_albums(auth, id),
        }
    }

    /// `None` when the backend can't list playlist tracks (the public APIs can't).
    pub fn playlist_tracks(&self, id: &str) -> Result<Option<Vec<Song>>> {
        match self {
            Self::Public => Ok(None),
            Self::MusicKit(auth) => musickit::playlist_tracks(auth, id).map(Some),
        }
    }

    pub fn artist(&self, id: &str) -> Result<ArtistDetail> {
        match self {
            Self::Public => public::artist(id),
            Self::MusicKit(auth) => musickit::artist(auth, id),
        }
    }

    pub fn search(&self, term: &str) -> Result<SearchResults> {
        match self {
            Self::Public => public::search(term),
            Self::MusicKit(auth) => musickit::search(auth, term),
        }
    }

    /// `Ok(false)` only when Apple explicitly rejects the user token (expired/revoked).
    pub fn user_token_valid(&self) -> Result<bool> {
        let Self::MusicKit(auth @ Auth { user: Some(_), .. }) = self else {
            return Ok(false);
        };
        match request("GET", "https://api.music.apple.com/v1/me/storefront", Some(auth), None) {
            Ok(_) => Ok(true),
            Err(e) if matches!(e.downcast_ref::<ureq::Error>(), Some(ureq::Error::StatusCode(401 | 403))) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub fn library(&self) -> Result<RemoteLibrary> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::library(auth),
            _ => Err(anyhow!("not signed in to Apple Music")),
        }
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::set_favorite(auth, id, favorite),
            _ => Err(anyhow!("not signed in to Apple Music")),
        }
    }

    /// Apple Music's live stations (Apple Music 1, Hits, Country, …). MusicKit only.
    pub fn live_stations(&self) -> Result<Vec<Station>> {
        match self {
            Self::MusicKit(auth) => musickit::live_stations(auth),
            Self::Public => Ok(Vec::new()),
        }
    }

    /// Resolves an artist name to a catalog artist id via search.
    pub fn find_artist(&self, name: &str) -> Result<Option<String>> {
        let results = self.search(name)?;
        let exact = results.artists.iter().find(|a| a.name.eq_ignore_ascii_case(name));
        Ok(exact.or(results.artists.first()).map(|a| a.id.clone()))
    }

    pub fn library_favorites(&self, songs: &[Song]) -> Result<Vec<Song>> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::library_favorites(auth, songs),
            _ => Ok(Vec::new()),
        }
    }

    pub fn personal_home(&self) -> Result<Option<PersonalHome>> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::personal_home(auth).map(Some),
            _ => Ok(None),
        }
    }

    pub fn add_to_playlist(&self, playlist_id: &str, song_ids: &[String]) -> Result<()> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::add_to_playlist(auth, playlist_id, song_ids),
            _ => Err(anyhow!("not signed in to Apple Music")),
        }
    }

    pub fn create_playlist(&self, name: &str, song_id: Option<&str>) -> Result<Playlist> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::create_playlist(auth, name, song_id),
            _ => Err(anyhow!("not signed in to Apple Music")),
        }
    }

    /// Adds catalog items to the signed-in user's library. `kind` is `songs`, `albums` or `playlists`.
    pub fn add_to_library(&self, kind: &str, id: &str) -> Result<()> {
        match self {
            Self::MusicKit(auth @ Auth { user: Some(_), .. }) => musickit::add_to_library(auth, kind, id),
            _ => Err(anyhow!("not signed in to Apple Music")),
        }
    }
}

/// One shared agent, so requests reuse connections instead of a new TLS handshake each.
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::LazyLock<ureq::Agent> = std::sync::LazyLock::new(|| {
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(8))).build().into()
    });
    &AGENT
}

fn with_auth<B>(mut req: ureq::RequestBuilder<B>, auth: Option<&Auth>) -> ureq::RequestBuilder<B> {
    if let Some(auth) = auth {
        req = req.header("Authorization", &format!("Bearer {}", auth.developer));
        if let Some(user) = &auth.user {
            req = req.header("Music-User-Token", user);
        }
    }
    req
}

fn request(method: &str, url: &str, auth: Option<&Auth>, body: Option<&Value>) -> Result<Value> {
    let started = std::time::Instant::now();
    let result = request_inner(method, url, auth, body);
    if crate::perf::enabled() {
        let path = url.split('?').next().unwrap_or(url).trim_start_matches("https://");
        crate::perf::request(
            &format!("{method} {path}{}", if result.is_err() { " (failed)" } else { "" }),
            started.elapsed(),
        );
    }
    result
}

fn request_inner(method: &str, url: &str, auth: Option<&Auth>, body: Option<&Value>) -> Result<Value> {
    let agent = agent();
    let mut resp = match method {
        "POST" => with_auth(agent.post(url), auth).send_empty()?,
        "POST_JSON" => with_auth(agent.post(url), auth).send_json(body.unwrap_or(&Value::Null))?,
        "DELETE" => with_auth(agent.delete(url), auth).call()?,
        "PUT" => with_auth(agent.put(url), auth).send_json(body.unwrap_or(&Value::Null))?,
        _ => with_auth(agent.get(url), auth).call()?,
    };
    if matches!(resp.status().as_u16(), 202 | 204) {
        return Ok(Value::Null);
    }
    resp.body_mut().read_json::<Value>().with_context(|| format!("decoding {url}"))
}

/// GET with retries: Apple's feeds intermittently answer 5xx, and the Apple Music API
/// answers 429 when a burst of library pages exceeds its rate limit.
fn get_json(url: &str, auth: Option<&Auth>) -> Result<Value> {
    let mut attempt: u32 = 0;
    loop {
        let err = match request("GET", url, auth, None) {
            Ok(v) => return Ok(v),
            Err(err) => err,
        };
        attempt += 1;
        let (retryable, max_attempts, base_ms) = match err.downcast_ref::<ureq::Error>() {
            Some(ureq::Error::StatusCode(429)) => (true, 6, 1000),
            Some(ureq::Error::StatusCode(code)) if *code < 500 => (false, 0, 0),
            _ => (true, 3, 400),
        };
        if !retryable || attempt >= max_attempts {
            return Err(err).with_context(|| format!("GET {url}"));
        }
        // Exponential backoff with jitter so parallel page requests don't retry in lockstep.
        let jitter = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_millis() as u64 % 250);
        std::thread::sleep(Duration::from_millis(base_ms * 2u64.pow(attempt - 1) + jitter));
    }
}

/// The part of `url` that identifies the file. Photos people upload for their playlists are
/// served from signed URLs (`X-Amz-*`) that are re-signed on every library fetch and expire
/// after a day, so the signature is left out: the same photo keeps its cached copy.
fn cache_key(url: &str) -> std::borrow::Cow<'_, str> {
    let Some((base, query)) = url.split_once('?') else { return url.into() };
    if !query.contains("X-Amz-") {
        return url.into();
    }
    let kept: Vec<&str> = query.split('&').filter(|p| !p.starts_with("X-Amz-")).collect();
    if kept.is_empty() { base.to_string().into() } else { format!("{base}?{}", kept.join("&")).into() }
}

/// Downloads `url` into the cache directory (once) and returns the file path.
pub fn download_cached(url: &str, kind: &str, ext: &str) -> Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cache_key(url).hash(&mut hasher);
    let dir = cache_dir().join(kind);
    let path = dir.join(format!("{:016x}.{ext}", hasher.finish()));
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)?;
    let bytes = agent()
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?
        .body_mut()
        .with_config()
        .limit(50 * 1024 * 1024)
        .read_to_vec()?;
    // Write-then-rename so a concurrent reader never sees a partial file. The temp name is
    // unique per call: two threads can fetch the same URL at once (playback and analysis).
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{ext}.part{}-{n}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        // Another download of the same URL finished first; its file is just as good.
        if !path.exists() {
            return Err(e.into());
        }
    }
    Ok(path)
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Encore")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Encore")
}

/// Moves data from the app's pre-release folder names ("GPUI Music", "gpui-music") so an
/// existing library, settings and MusicKit key config carry over. Call once at launch.
pub fn migrate_legacy_dirs() {
    for (base, old) in [(dirs::data_dir(), "GPUI Music"), (dirs::cache_dir(), "gpui-music")] {
        let Some(base) = base else { continue };
        let (old, new) = (base.join(old), base.join("Encore"));
        if old.exists() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

fn s(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn opt(v: &Value, key: &str) -> Option<String> {
    Some(s(v, key)).filter(|s| !s.is_empty())
}

pub fn query_escape(term: &str) -> String {
    let mut out = String::new();
    for b in term.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

mod public {
    use super::*;

    const RSS: &str = "https://rss.marketingtools.apple.com/api/v2";
    const ITUNES: &str = "https://itunes.apple.com";

    /// Falls back to smaller feeds, which Apple serves from a separate (often healthier) cache.
    fn rss(kind: &str, limit: u32) -> Result<Vec<Value>> {
        let mut last_err = None;
        for limit in [limit, 50, 25] {
            let url = format!("{RSS}/{STOREFRONT}/music/most-played/{limit}/{kind}.json");
            match get_json(&url, None) {
                Ok(json) => return Ok(json["feed"]["results"].as_array().cloned().unwrap_or_default()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap())
    }

    fn lookup(query: &str) -> Result<Vec<Value>> {
        let url = format!("{ITUNES}/lookup?{query}&country={STOREFRONT}");
        Ok(get_json(&url, None)?["results"].as_array().cloned().unwrap_or_default())
    }

    fn itunes_search(term: &str, entity: &str, limit: u32) -> Result<Vec<Value>> {
        let url = format!(
            "{ITUNES}/search?term={}&media=music&entity={entity}&limit={limit}&country={STOREFRONT}",
            query_escape(term)
        );
        Ok(get_json(&url, None)?["results"].as_array().cloned().unwrap_or_default())
    }

    pub(super) fn song(v: &Value) -> Song {
        Song {
            id: s(v, "trackId"),
            title: s(v, "trackName"),
            artist: s(v, "artistName"),
            artist_id: opt(v, "artistId"),
            artist_ids: Vec::new(),
            album: s(v, "collectionName"),
            album_id: opt(v, "collectionId"),
            artwork: Artwork::from_sized(&s(v, "artworkUrl100")),
            preview_url: opt(v, "previewUrl"),
            duration_ms: v["trackTimeMillis"].as_u64().unwrap_or(0) as u32,
            explicit: s(v, "trackExplicitness") == "explicit",
            release_date: s(v, "releaseDate").chars().take(10).collect(),
            track_number: v["trackNumber"].as_u64().unwrap_or(0) as u32,
            url: s(v, "trackViewUrl"),
        }
    }

    pub(super) fn album_from_lookup(v: &Value) -> Album {
        Album {
            id: s(v, "collectionId"),
            title: s(v, "collectionName"),
            artist: s(v, "artistName"),
            artist_id: opt(v, "artistId"),
            artwork: Artwork::from_sized(&s(v, "artworkUrl100")),
            genre: s(v, "primaryGenreName"),
            release_date: s(v, "releaseDate").chars().take(10).collect(),
            explicit: s(v, "collectionExplicitness") == "explicit",
            url: s(v, "collectionViewUrl"),
            track_count: v["trackCount"].as_u64().map(|n| n as u32),
            ..Default::default()
        }
    }

    fn album_from_rss(v: &Value) -> Album {
        Album {
            id: s(v, "id"),
            title: s(v, "name"),
            artist: s(v, "artistName"),
            artist_id: opt(v, "artistId"),
            artwork: Artwork::from_sized(&s(v, "artworkUrl100")),
            genre: v["genres"][0]["name"].as_str().unwrap_or("").to_string(),
            release_date: s(v, "releaseDate"),
            explicit: v["contentAdvisoryRating"].as_str().is_some_and(|r| r.starts_with("Expl")),
            url: s(v, "url"),
            ..Default::default()
        }
    }

    pub fn browse() -> Result<Browse> {
        let (songs, albums, playlists) = std::thread::scope(|s| {
            let songs = s.spawn(|| rss("songs", 100));
            let albums = s.spawn(|| rss("albums", 100));
            let playlists = s.spawn(|| rss("playlists", 25));
            (songs.join(), albums.join(), playlists.join())
        });
        let songs = songs.map_err(|_| anyhow!("songs feed panicked"))??;
        let albums = albums.map_err(|_| anyhow!("albums feed panicked"))??;
        let playlists = playlists.ok().and_then(Result::ok).unwrap_or_default();

        // The RSS feed has no preview URLs or durations; fill them in from iTunes Lookup.
        let ids: Vec<String> = songs.iter().map(|v| s(v, "id")).collect();
        let mut details = std::collections::HashMap::new();
        for chunk in ids.chunks(100) {
            for v in lookup(&format!("id={}", chunk.join(",")))? {
                if v["wrapperType"] == "track" {
                    details.insert(s(&v, "trackId"), song(&v));
                }
            }
        }
        let top_songs = songs
            .iter()
            .filter_map(|v| {
                let mut song = details.remove(&s(v, "id"))?;
                song.release_date = s(v, "releaseDate");
                Some(song)
            })
            .collect();

        Ok(Browse {
            top_songs,
            top_albums: albums.iter().map(album_from_rss).collect(),
            top_playlists: playlists
                .iter()
                .map(|v| Playlist {
                    id: s(v, "id"),
                    name: s(v, "name"),
                    curator: "Apple Music".into(),
                    description: String::new(),
                    artwork: Artwork::from_sized(&s(v, "artworkUrl100")),
                    url: s(v, "url"),
                    ..Default::default()
                })
                .collect(),
        })
    }

    pub fn album(id: &str) -> Result<(Album, Vec<Song>)> {
        let results = lookup(&format!("id={id}&entity=song&limit=200"))?;
        let album = results
            .iter()
            .find(|v| v["wrapperType"] == "collection")
            .map(album_from_lookup)
            .ok_or_else(|| anyhow!("album {id} not found"))?;
        let mut songs: Vec<Song> = results.iter().filter(|v| v["wrapperType"] == "track").map(song).collect();
        songs.sort_by_key(|s| s.track_number);
        Ok((album, songs))
    }

    pub fn artist(id: &str) -> Result<ArtistDetail> {
        let albums = lookup(&format!("id={id}&entity=album&limit=60"))?;
        let artist_v =
            albums.iter().find(|v| v["wrapperType"] == "artist").ok_or_else(|| anyhow!("artist {id} not found"))?;
        let mut album_list: Vec<Album> =
            albums.iter().filter(|v| v["wrapperType"] == "collection").map(album_from_lookup).collect();
        album_list.sort_by(|a, b| b.release_date.cmp(&a.release_date));
        let top_songs = lookup(&format!("id={id}&entity=song&limit=10"))?
            .iter()
            .filter(|v| v["wrapperType"] == "track")
            .map(song)
            .collect();
        Ok(ArtistDetail {
            similar: Vec::new(),
            about: String::new(),
            artist: Artist {
                id: id.to_string(),
                name: s(artist_v, "artistName"),
                genre: s(artist_v, "primaryGenreName"),
                artwork: album_list.first().map(|a| a.artwork.clone()).unwrap_or_default(),
                url: s(artist_v, "artistLinkUrl"),
                ..Default::default()
            },
            albums: album_list,
            top_songs,
        })
    }

    pub fn search(term: &str) -> Result<SearchResults> {
        let (songs, albums, artists) = std::thread::scope(|s| {
            let songs = s.spawn(|| itunes_search(term, "song", 25));
            let albums = s.spawn(|| itunes_search(term, "album", 20));
            let artists = s.spawn(|| itunes_search(term, "musicArtist", 8));
            (songs.join(), albums.join(), artists.join())
        });
        let join = |r: std::thread::Result<Result<Vec<Value>>>| r.map_err(|_| anyhow!("search request panicked"))?;
        let (songs, albums, artists) = (join(songs)?, join(albums)?, join(artists)?);
        Ok(SearchResults {
            top: Vec::new(),
            songs: songs.iter().map(song).collect(),
            albums: albums.iter().map(album_from_lookup).collect(),
            artists: artists
                .iter()
                .map(|v| Artist {
                    id: s(v, "artistId"),
                    name: s(v, "artistName"),
                    genre: s(v, "primaryGenreName"),
                    artwork: Artwork::default(),
                    url: s(v, "artistLinkUrl"),
                    ..Default::default()
                })
                .collect(),
        })
    }
}

mod musickit {
    use super::*;

    const API: &str = "https://api.music.apple.com/v1/catalog";
    const ME: &str = "https://api.music.apple.com/v1/me";

    fn get(auth: &Auth, path: &str) -> Result<Value> {
        get_json(&format!("{API}/{STOREFRONT}/{path}"), Some(auth))
    }

    fn get_me(auth: &Auth, path: &str) -> Result<Value> {
        get_json(&format!("{ME}/{path}"), Some(auth))
    }

    /// Pages through a library collection. When the first page reports `meta.total`, the
    /// remaining pages are fetched several at a time instead of following `next` serially.
    fn get_me_pages(auth: &Auth, path: &str, max: usize) -> Result<Vec<Value>> {
        const PAGE: usize = 100;
        const PARALLEL: usize = 4;
        let sep = if path.contains('?') { '&' } else { '?' };
        let first = get_me(auth, &format!("{path}{sep}limit={PAGE}&meta=total"))?;
        let mut out = first["data"].as_array().cloned().unwrap_or_default();
        let Some(total) = first["meta"]["total"].as_u64().map(|t| (t as usize).min(max)) else {
            return match first["next"].as_str() {
                Some(next) if out.len() < max => {
                    let rest = get_me_all(auth, next.trim_start_matches("/v1/me/"), max - out.len())?;
                    out.extend(rest);
                    Ok(out)
                }
                _ => Ok(out),
            };
        };
        let offsets: Vec<usize> = (PAGE..total).step_by(PAGE).collect();
        for batch in offsets.chunks(PARALLEL) {
            let pages = std::thread::scope(|s| {
                let handles: Vec<_> = batch
                    .iter()
                    .map(|offset| {
                        let url = format!("{path}{sep}limit={PAGE}&offset={offset}");
                        s.spawn(move || get_me(auth, &url))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join()).collect::<Vec<_>>()
            });
            for page in pages {
                let page = page.map_err(|_| anyhow!("library page request panicked"))??;
                out.extend(page["data"].as_array().cloned().unwrap_or_default());
            }
        }
        Ok(out)
    }

    fn get_me_all(auth: &Auth, path: &str, max: usize) -> Result<Vec<Value>> {
        get_all(auth, &format!("{ME}/{path}"), max)
    }

    /// Follows `next` links from `url` until `max` items have been collected.
    fn get_all(auth: &Auth, url: &str, max: usize) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let mut url = url.to_string();
        loop {
            let page = get_json(&url, Some(auth))?;
            out.extend(page["data"].as_array().cloned().unwrap_or_default());
            match page["next"].as_str() {
                Some(next) if out.len() < max => url = format!("https://api.music.apple.com{next}"),
                _ => return Ok(out),
            }
        }
    }

    fn artwork(attrs: &Value) -> Artwork {
        Artwork(attrs["artwork"]["url"].as_str().unwrap_or("").to_string())
    }

    fn art_colors(attrs: &Value) -> Option<ArtColors> {
        let a = &attrs["artwork"];
        let hex = |k: &str| a[k].as_str().and_then(|h| u32::from_str_radix(h, 16).ok());
        Some(ArtColors {
            bg: hex("bgColor")?,
            text: [hex("textColor1")?, hex("textColor2")?, hex("textColor3")?, hex("textColor4")?],
        })
    }

    /// Parses `…/album/<slug>/<id>?i=…` into the album id.
    fn album_id_from_url(url: &str) -> Option<String> {
        let path = url.split('?').next()?;
        let mut parts = path.rsplit('/');
        let id = parts.next()?;
        (parts.nth(1) == Some("album")).then(|| id.to_string())
    }

    fn rel_id(v: &Value, rel: &str) -> Option<String> {
        v["relationships"][rel]["data"][0]["id"].as_str().map(str::to_string)
    }

    fn rel_ids(v: &Value, rel: &str) -> Vec<String> {
        v["relationships"][rel]["data"]
            .as_array()
            .map(|items| items.iter().filter_map(|i| i["id"].as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn song(v: &Value) -> Song {
        let a = &v["attributes"];
        let url = s(a, "url");
        Song {
            id: s(v, "id"),
            title: s(a, "name"),
            artist: s(a, "artistName"),
            artist_id: rel_id(v, "artists"),
            artist_ids: rel_ids(v, "artists"),
            album: s(a, "albumName"),
            album_id: rel_id(v, "albums").or_else(|| album_id_from_url(&url)),
            artwork: artwork(a),
            preview_url: a["previews"][0]["url"].as_str().map(str::to_string),
            duration_ms: a["durationInMillis"].as_u64().unwrap_or(0) as u32,
            explicit: s(a, "contentRating") == "explicit",
            release_date: s(a, "releaseDate"),
            track_number: a["trackNumber"].as_u64().unwrap_or(0) as u32,
            url,
        }
    }

    fn parse_album(v: &Value) -> Album {
        let a = &v["attributes"];
        Album {
            id: s(v, "id"),
            title: s(a, "name"),
            artist: s(a, "artistName"),
            artist_id: rel_id(v, "artists"),
            artwork: artwork(a),
            genre: a["genreNames"][0].as_str().unwrap_or("").to_string(),
            release_date: s(a, "releaseDate"),
            explicit: s(a, "contentRating") == "explicit",
            url: s(a, "url"),
            colors: art_colors(a),
            description: a["editorialNotes"]["standard"]
                .as_str()
                .or(a["editorialNotes"]["short"].as_str())
                .unwrap_or("")
                .to_string(),
            track_count: a["trackCount"].as_u64().map(|n| n as u32),
        }
    }

    fn playlist(v: &Value) -> Playlist {
        let a = &v["attributes"];
        Playlist {
            id: s(v, "id"),
            name: s(a, "name"),
            curator: s(a, "curatorName"),
            description: a["description"]["short"].as_str().unwrap_or("").to_string(),
            artwork: artwork(a),
            url: s(a, "url"),
            colors: art_colors(a),
            ..Default::default()
        }
    }

    fn parse_artist(v: &Value) -> Artist {
        let a = &v["attributes"];
        Artist {
            id: s(v, "id"),
            name: s(a, "name"),
            genre: a["genreNames"][0].as_str().unwrap_or("").to_string(),
            artwork: artwork(a),
            url: s(a, "url"),
            colors: art_colors(a),
        }
    }

    fn list<T>(v: &Value, f: fn(&Value) -> T) -> Vec<T> {
        v.as_array().map(|a| a.iter().map(f).collect()).unwrap_or_default()
    }

    pub fn browse(auth: &Auth) -> Result<Browse> {
        let charts = get(auth, "charts?types=songs,albums,playlists&limit=50")?;
        let r = &charts["results"];
        Ok(Browse {
            top_songs: list(&r["songs"][0]["data"], song),
            top_albums: list(&r["albums"][0]["data"], parse_album),
            top_playlists: list(&r["playlists"][0]["data"], playlist),
        })
    }

    pub fn album(auth: &Auth, id: &str) -> Result<(Album, Vec<Song>)> {
        if id.starts_with("l.") {
            let json = get_me(auth, &format!("library/albums/{id}?include=catalog"))?;
            let data = &json["data"][0];
            if let Some(catalog_id) = catalog_of(data).and_then(|c| c["id"].as_str()) {
                return album(auth, catalog_id);
            }
            // Uploaded or otherwise catalog-less album: use the library copy.
            let tracks = get_me_all(auth, &format!("library/albums/{id}/tracks?include=catalog&limit=100"), 300)?;
            return Ok((library_album(data), tracks.iter().map(library_song).collect()));
        }
        let json = get(auth, &format!("albums/{id}?include=tracks,artists"))?;
        let data = &json["data"][0];
        let mut tracks: Vec<Song> = list(&data["relationships"]["tracks"]["data"], song);
        let album = parse_album(data);
        for t in &mut tracks {
            t.album_id.get_or_insert_with(|| album.id.clone());
            t.artist_id = t.artist_id.take().or_else(|| album.artist_id.clone());
        }
        Ok((album, tracks))
    }

    pub fn related_albums(auth: &Auth, id: &str) -> Result<Vec<Album>> {
        let json = get(auth, &format!("albums/{id}/view/related-albums?limit=10"))?;
        Ok(list(&json["data"], parse_album))
    }

    pub fn playlist_tracks(auth: &Auth, id: &str) -> Result<Vec<Song>> {
        if id.starts_with("p.") {
            let tracks = get_me_all(auth, &format!("library/playlists/{id}/tracks?include=catalog&limit=100"), 1000)?;
            return Ok(tracks.iter().map(library_song).collect());
        }
        let tracks = get_all(auth, &format!("{API}/{STOREFRONT}/playlists/{id}/tracks?limit=100"), 1000)?;
        Ok(tracks.iter().map(song).collect())
    }

    pub fn artist(auth: &Auth, id: &str) -> Result<ArtistDetail> {
        let (json, albums, top, similar) = std::thread::scope(|s| {
            let json = s.spawn(|| get(auth, &format!("artists/{id}")));
            let albums = s.spawn(|| get(auth, &format!("artists/{id}/albums?limit=50")));
            let top = s.spawn(|| get(auth, &format!("artists/{id}/view/top-songs?limit=20")));
            let similar = s.spawn(|| get(auth, &format!("artists/{id}/view/similar-artists?limit=15")));
            (json.join(), albums.join(), top.join(), similar.join())
        });
        let join = |r: std::thread::Result<Result<Value>>| r.map_err(|_| anyhow!("artist request panicked"))?;
        let json = join(json)?;
        let mut albums = list(&join(albums)?["data"], parse_album);
        albums.sort_by(|a, b| b.release_date.cmp(&a.release_date));
        let data = &json["data"][0];
        let notes = &data["attributes"]["editorialNotes"];
        Ok(ArtistDetail {
            artist: parse_artist(data),
            albums,
            top_songs: list(&join(top)?["data"], song),
            similar: join(similar).map(|v| list(&v["data"], parse_artist)).unwrap_or_default(),
            about: notes["standard"].as_str().or(notes["short"].as_str()).unwrap_or("").to_string(),
        })
    }

    /// Library items carry their catalog twin under `relationships.catalog` when
    /// requested with `include=catalog`; the catalog version has previews and links.
    fn catalog_of(v: &Value) -> Option<&Value> {
        let c = &v["relationships"]["catalog"]["data"][0];
        c["attributes"].is_object().then_some(c)
    }

    fn library_song(v: &Value) -> Song {
        catalog_of(v).map_or_else(|| song(v), song)
    }

    fn library_album(v: &Value) -> Album {
        let mut album = parse_album(v);
        if let Some(c) = catalog_of(v) {
            let catalog = parse_album(c);
            album.url = catalog.url;
            if album.artwork.is_empty() {
                album.artwork = catalog.artwork;
            }
        }
        album
    }

    fn library_playlist(v: &Value) -> Playlist {
        let mut p = playlist(v);
        p.editable = v["attributes"]["canEdit"].as_bool().unwrap_or(false);
        if p.description.is_empty() {
            p.description = v["attributes"]["description"]["standard"].as_str().unwrap_or("").to_string();
        }
        if p.url.is_empty() {
            p.url = format!("https://music.apple.com/library/playlist/{}", p.id);
        }
        p
    }

    /// Everything except favorites, which need one ratings lookup per 100 songs and are
    /// fetched separately by `library_favorites`.
    pub fn library(auth: &Auth) -> Result<RemoteLibrary> {
        let (songs, albums, playlists, recent, artists) = std::thread::scope(|s| {
            let songs = s.spawn(|| get_me_pages(auth, "library/songs?include=catalog", 20_000));
            let albums = s.spawn(|| get_me_pages(auth, "library/albums?include=catalog", 5_000));
            let playlists = s.spawn(|| get_me_pages(auth, "library/playlists", 1_000));
            // recently-added caps `limit` at 10.
            let recent = s.spawn(|| get_me_all(auth, "library/recently-added?limit=10", 50));
            let artists = s.spawn(|| get_me_pages(auth, "library/artists?include=catalog", 5_000));
            (songs.join(), albums.join(), playlists.join(), recent.join(), artists.join())
        });
        let join = |r: std::thread::Result<Result<Vec<Value>>>| r.map_err(|_| anyhow!("library request panicked"))?;
        let recent = join(recent).unwrap_or_else(|e| {
            eprintln!("recently added failed: {e:#}");
            Vec::new()
        });
        let mut artists: Vec<Artist> = join(artists)
            .unwrap_or_else(|e| {
                eprintln!("library artists failed: {e:#}");
                Vec::new()
            })
            .iter()
            .map(library_artist)
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        Ok(RemoteLibrary {
            songs: std::sync::Arc::new(join(songs)?.iter().map(library_song).collect()),
            favorites: Default::default(),
            albums: join(albums)?.iter().map(library_album).collect(),
            playlists: join(playlists)?.iter().map(library_playlist).collect(),
            recently_added: recent
                .iter()
                .filter(|v| v["type"] == "library-albums" || v["type"] == "albums")
                .map(library_album)
                .collect(),
            artists,
        })
    }

    pub fn live_stations(auth: &Auth) -> Result<Vec<Station>> {
        let json = get(auth, "stations?filter%5Bfeatured%5D=apple-music-live-radio")?;
        Ok(json["data"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| {
                let a = &v["attributes"];
                Station {
                    id: s(v, "id"),
                    name: s(a, "name"),
                    description: a["editorialNotes"]["short"].as_str().unwrap_or("").to_string(),
                    artwork: artwork(a),
                    url: s(a, "url"),
                }
            })
            .collect())
    }

    fn library_artist(v: &Value) -> Artist {
        let name = s(&v["attributes"], "name");
        match catalog_of(v) {
            Some(c) => Artist { name, ..parse_artist(c) },
            None => Artist { id: s(v, "id"), name, ..Default::default() },
        }
    }

    pub fn library_favorites(auth: &Auth, songs: &[Song]) -> Result<Vec<Song>> {
        favorites(auth, songs)
    }

    /// Library-only songs (`i.` ids) are rated through `library-songs`; catalog songs through `songs`.
    fn rating_kind(id: &str) -> &'static str {
        if id.starts_with("i.") { "library-songs" } else { "songs" }
    }

    pub fn set_favorite(auth: &Auth, id: &str, favorite: bool) -> Result<()> {
        let url = format!("{ME}/ratings/{}/{}", rating_kind(id), query_escape(id));
        if favorite {
            let body = serde_json::json!({ "type": "rating", "attributes": { "value": 1 } });
            request("PUT", &url, Some(auth), Some(&body)).map(|_| ())
        } else {
            match request("DELETE", &url, Some(auth), None) {
                Err(e) if matches!(e.downcast_ref::<ureq::Error>(), Some(ureq::Error::StatusCode(404))) => Ok(()),
                other => other.map(|_| ()),
            }
        }
    }

    /// Songs rated "favorite" among `songs`, checked in batches.
    fn favorites(auth: &Auth, songs: &[Song]) -> Result<Vec<Song>> {
        let mut liked = std::collections::HashSet::new();
        for kind in ["songs", "library-songs"] {
            let ids: Vec<&str> = songs.iter().map(|s| s.id.as_str()).filter(|id| rating_kind(id) == kind).collect();
            let chunks: Vec<String> = ids.chunks(100).map(|c| c.join(",")).collect();
            for batch in chunks.chunks(4) {
                let pages = std::thread::scope(|scope| {
                    let handles: Vec<_> = batch
                        .iter()
                        .map(|ids| scope.spawn(move || get_me(auth, &format!("ratings/{kind}?ids={ids}"))))
                        .collect();
                    handles.into_iter().map(|h| h.join()).collect::<Vec<_>>()
                });
                for page in pages {
                    let json = page.map_err(|_| anyhow!("ratings request panicked"))??;
                    for v in json["data"].as_array().into_iter().flatten() {
                        if v["attributes"]["value"].as_i64() == Some(1) {
                            liked.insert(s(v, "id"));
                        }
                    }
                }
            }
        }
        Ok(songs.iter().filter(|s| liked.contains(&s.id)).cloned().collect())
    }

    fn song_ref(id: &str) -> Value {
        serde_json::json!({ "id": id, "type": if id.starts_with("i.") { "library-songs" } else { "songs" } })
    }

    /// Albums, playlists and songs in a mixed list (stations and others are skipped).
    fn mixed(v: &Value) -> Vec<TopResult> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| match v["type"].as_str()? {
                "songs" => Some(TopResult::Song(song(v))),
                "albums" => Some(TopResult::Album(parse_album(v))),
                "library-albums" => Some(TopResult::Album(library_album(v))),
                "playlists" => Some(TopResult::Playlist(playlist(v))),
                "library-playlists" => Some(TopResult::Playlist(library_playlist(v))),
                "artists" => Some(TopResult::Artist(parse_artist(v))),
                _ => None,
            })
            .collect()
    }

    pub fn personal_home(auth: &Auth) -> Result<PersonalHome> {
        let (recs, heavy, tracks) = std::thread::scope(|s| {
            let recs = s.spawn(|| get_me(auth, "recommendations?limit=10"));
            let heavy = s.spawn(|| get_me(auth, "history/heavy-rotation?limit=20"));
            let tracks = s.spawn(|| get_me(auth, "recent/played/tracks?limit=30"));
            (recs.join(), heavy.join(), tracks.join())
        });
        let ok = |r: std::thread::Result<Result<Value>>| r.ok().and_then(Result::ok);
        let recs = ok(recs).ok_or_else(|| anyhow!("recommendations unavailable"))?;
        let shelves = recs["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                let a = &v["attributes"];
                let title = a["title"]["stringForDisplay"].as_str()?.to_string();
                let items = mixed(&v["relationships"]["contents"]["data"]);
                (!items.is_empty()).then_some((title, items))
            })
            .collect();
        Ok(PersonalHome {
            recent_tracks: ok(tracks).map(|v| list(&v["data"], song)).unwrap_or_default(),
            heavy_rotation: ok(heavy).map(|v| mixed(&v["data"])).unwrap_or_default(),
            shelves,
        })
    }

    pub fn add_to_playlist(auth: &Auth, playlist_id: &str, song_ids: &[String]) -> Result<()> {
        let body = serde_json::json!({ "data": song_ids.iter().map(|id| song_ref(id)).collect::<Vec<_>>() });
        request("POST_JSON", &format!("{ME}/library/playlists/{playlist_id}/tracks"), Some(auth), Some(&body))
            .map(|_| ())
    }

    pub fn create_playlist(auth: &Auth, name: &str, song_id: Option<&str>) -> Result<Playlist> {
        let mut body = serde_json::json!({ "attributes": { "name": name } });
        if let Some(id) = song_id {
            body["relationships"] = serde_json::json!({ "tracks": { "data": [song_ref(id)] } });
        }
        let json = request("POST_JSON", &format!("{ME}/library/playlists"), Some(auth), Some(&body))?;
        let mut p = library_playlist(&json["data"][0]);
        p.editable = true;
        Ok(p)
    }

    pub fn add_to_library(auth: &Auth, kind: &str, id: &str) -> Result<()> {
        request("POST", &format!("{ME}/library?ids%5B{kind}%5D={}", query_escape(id)), Some(auth), None).map(|_| ())
    }

    pub fn search(auth: &Auth, term: &str) -> Result<SearchResults> {
        let json = get(
            auth,
            &format!(
                "search?term={}&types=songs,albums,artists,playlists&with=topResults&limit=25",
                query_escape(term)
            ),
        )?;
        let r = &json["results"];
        let top = r["topResults"]["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| match v["type"].as_str()? {
                "songs" => Some(TopResult::Song(song(v))),
                "albums" => Some(TopResult::Album(parse_album(v))),
                "artists" => Some(TopResult::Artist(parse_artist(v))),
                "playlists" => Some(TopResult::Playlist(playlist(v))),
                _ => None,
            })
            .collect();
        Ok(SearchResults {
            top,
            songs: list(&r["songs"]["data"], song),
            albums: list(&r["albums"]["data"], parse_album),
            artists: list(&r["artists"]["data"], parse_artist),
        })
    }

    #[cfg(test)]
    #[test]
    fn parses_album_id_from_song_url() {
        assert_eq!(
            album_id_from_url("https://music.apple.com/us/album/housewife/6822497770?i=6822497774").as_deref(),
            Some("6822497770")
        );
        assert_eq!(album_id_from_url("https://music.apple.com/us/song/x/1"), None);
    }
}

#[derive(Deserialize)]
struct MusicKitConfig {
    team_id: String,
    key_id: String,
    private_key_path: PathBuf,
}

/// Looks for, in order: `APPLE_MUSIC_DEVELOPER_TOKEN`; the `APPLE_MUSIC_TEAM_ID` /
/// `APPLE_MUSIC_KEY_ID` / `APPLE_MUSIC_PRIVATE_KEY_PATH` trio; `<data dir>/musickit.json`.
fn musickit_token() -> Result<Option<String>> {
    if let Ok(token) = std::env::var("APPLE_MUSIC_DEVELOPER_TOKEN")
        && !token.trim().is_empty()
    {
        return Ok(Some(token.trim().to_string()));
    }
    let config = match (
        std::env::var("APPLE_MUSIC_TEAM_ID"),
        std::env::var("APPLE_MUSIC_KEY_ID"),
        std::env::var("APPLE_MUSIC_PRIVATE_KEY_PATH"),
    ) {
        (Ok(team_id), Ok(key_id), Ok(path)) => MusicKitConfig { team_id, key_id, private_key_path: path.into() },
        _ => {
            let path = data_dir().join("musickit.json");
            if !path.exists() {
                return Ok(None);
            }
            serde_json::from_slice(&std::fs::read(&path)?).with_context(|| format!("reading {}", path.display()))?
        }
    };
    let pem = std::fs::read(&config.private_key_path)
        .with_context(|| format!("reading {}", config.private_key_path.display()))?;
    sign_developer_token(&config.team_id, &config.key_id, &pem).map(Some)
}

/// ES256 JWT as described in "Generating Developer Tokens" (Apple Music API docs).
pub fn sign_developer_token(team_id: &str, key_id: &str, p8_pem: &[u8]) -> Result<String> {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        iat: u64,
        exp: u64,
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(key_id.to_string());
    let claims = Claims { iss: team_id, iat: now, exp: now + 60 * 60 * 24 * 30 };
    let key = EncodingKey::from_ec_pem(p8_pem).context("invalid MusicKit .p8 key")?;
    Ok(encode(&header, &claims, &key)?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn signed_urls_share_a_cache_key() {
        let a = "https://store-032.blobstore.apple.com/x/image?X-Amz-Date=1&X-Amz-Signature=aa";
        let b = "https://store-032.blobstore.apple.com/x/image?X-Amz-Date=2&X-Amz-Signature=bb";
        assert_eq!(super::cache_key(a), super::cache_key(b));
        assert_eq!(super::cache_key(a), "https://store-032.blobstore.apple.com/x/image");
        let plain = "https://is1-ssl.mzstatic.com/image/gen/1200x1200AM.jpg?c1=B3B3B3&signature=x";
        assert_eq!(super::cache_key(plain), plain);
    }

    use super::*;

    #[test]
    fn escapes_search_terms() {
        assert_eq!(query_escape("Troye Sivan & co"), "Troye+Sivan+%26+co");
    }

    #[test]
    fn signs_verifiable_developer_token() {
        use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
        let dir = std::env::temp_dir().join(format!("musickit-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("key.p8");
        let public = dir.join("pub.pem");
        let ok = std::process::Command::new("openssl")
            .args(["genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out"])
            .arg(&key)
            .status()
            .is_ok_and(|s| s.success())
            && std::process::Command::new("openssl")
                .args(["pkey", "-pubout", "-in"])
                .arg(&key)
                .arg("-out")
                .arg(&public)
                .status()
                .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("openssl unavailable; skipping");
            return;
        }
        let token = sign_developer_token("TEAM123456", "KEY1234567", &std::fs::read(&key).unwrap()).unwrap();
        let header = decode_header(&token).unwrap();
        assert_eq!(header.alg, Algorithm::ES256);
        assert_eq!(header.kid.as_deref(), Some("KEY1234567"));
        let mut validation = Validation::new(Algorithm::ES256);
        validation.required_spec_claims = ["exp".to_string(), "iss".to_string()].into();
        validation.set_issuer(&["TEAM123456"]);
        let decoding = DecodingKey::from_ec_pem(&std::fs::read(&public).unwrap()).unwrap();
        decode::<serde_json::Value>(&token, &decoding, &validation).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Uses the configured MusicKit key against the live API. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_musickit_catalog() {
        let (catalog, note) = Catalog::from_env();
        assert!(note.is_none(), "{note:?}");
        assert!(catalog.is_musickit(), "no MusicKit key configured");
        let browse = catalog.browse().unwrap();
        assert!(!browse.top_songs.is_empty() && !browse.top_albums.is_empty());
        let song = &browse.top_songs[0];
        assert!(song.preview_url.is_some(), "{song:?}");
        let (album, tracks) = catalog.album(song.album_id.as_deref().unwrap()).unwrap();
        assert!(!tracks.is_empty(), "{album:?}");
        let related = catalog.related_albums("1645278559").unwrap();
        assert!(!related.is_empty() && related.iter().all(|a| !a.id.is_empty() && !a.title.is_empty()), "{related:?}");
        let playlist = &browse.top_playlists[0];
        assert!(!catalog.playlist_tracks(&playlist.id).unwrap().unwrap().is_empty());
        let results = catalog.search("taylor swift").unwrap();
        assert!(!results.songs.is_empty() && !results.artists.is_empty());
        let artist = catalog.artist(&results.artists[0].id).unwrap();
        assert!(!artist.albums.is_empty());
        let stations = catalog.live_stations().unwrap();
        assert!(!stations.is_empty() && !stations[0].artwork.is_empty(), "{stations:?}");
        assert!(catalog.find_artist("Zach Bryan").unwrap().is_some());
    }

    /// Hits the live public endpoints. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_public_catalog() {
        let catalog = Catalog::Public;
        let browse = catalog.browse().unwrap();
        assert!(browse.top_songs.len() > 50, "{}", browse.top_songs.len());
        assert!(browse.top_songs.iter().filter(|s| s.preview_url.is_some()).count() > 40);
        assert!(!browse.top_albums.is_empty() && !browse.top_playlists.is_empty());
        assert!(!browse.best_new_songs().is_empty());
        let album = &browse.top_albums[0];
        let (detail, tracks) = catalog.album(&album.id).unwrap();
        assert_eq!(detail.id, album.id);
        assert!(!tracks.is_empty());
        let artist = catalog.artist(album.artist_id.as_deref().unwrap()).unwrap();
        assert!(!artist.albums.is_empty());
        let results = catalog.search("taylor swift").unwrap();
        assert!(!results.songs.is_empty() && !results.albums.is_empty() && !results.artists.is_empty());
    }
}
