use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Artwork URL template containing `{w}` and `{h}` placeholders.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Artwork(pub String);

impl Artwork {
    /// Converts a sized mzstatic URL (`…/100x100bb.jpg`) into a template.
    pub fn from_sized(url: &str) -> Self {
        let Some(slash) = url.rfind('/') else {
            return Self(url.to_string());
        };
        let (head, tail) = url.split_at(slash + 1);
        let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
        let w = digits(tail);
        if w > 0 && tail[w..].starts_with('x') {
            let h = digits(&tail[w + 1..]);
            if h > 0 {
                return Self(format!("{head}{{w}}x{{h}}{}", &tail[w + 1 + h..]));
            }
        }
        Self(url.to_string())
    }

    pub fn url(&self, px: u32) -> String {
        let px = px.to_string();
        self.0.replace("{w}", &px).replace("{h}", &px)
    }

    /// Exactly `w`×`h`, cropped by Apple's image server ("sr" keeps the subject in frame).
    /// Templates without the usual `bb` suffix fall back to a square image.
    pub fn url_cropped(&self, w: u32, h: u32) -> String {
        if !self.0.contains("{w}x{h}bb") {
            return self.url(w.max(h));
        }
        self.0
            .replace("{w}x{h}bb", "{w}x{h}sr")
            .replace("{w}", &w.to_string())
            .replace("{h}", &h.to_string())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: String,
    pub album_id: Option<String>,
    pub artwork: Artwork,
    pub preview_url: Option<String>,
    pub duration_ms: u32,
    pub explicit: bool,
    pub release_date: String,
    pub track_number: u32,
    pub url: String,
}

/// Colors Apple picks for a piece of artwork: a background and four text colors that read on it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtColors {
    pub bg: u32,
    pub text: [u32; 4],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub artwork: Artwork,
    pub genre: String,
    pub release_date: String,
    pub explicit: bool,
    pub url: String,
    #[serde(default)]
    pub colors: Option<ArtColors>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub track_count: Option<u32>,
}

impl Album {
    pub fn year(&self) -> &str {
        self.release_date.get(..4).unwrap_or("")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub curator: String,
    pub description: String,
    pub artwork: Artwork,
    pub url: String,
    #[serde(default)]
    pub colors: Option<ArtColors>,
    /// The user can add songs to it (their own library playlists).
    #[serde(default)]
    pub editable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub genre: String,
    pub artwork: Artwork,
    pub url: String,
    #[serde(default)]
    pub colors: Option<ArtColors>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Browse {
    pub top_songs: Vec<Song>,
    pub top_albums: Vec<Album>,
    pub top_playlists: Vec<Playlist>,
}

impl Browse {
    /// Chart songs released in the last ~6 weeks, in chart order.
    pub fn best_new_songs(&self) -> Vec<Song> {
        let cutoff = recent_cutoff(&self.top_songs.iter().map(|s| s.release_date.as_str()).collect::<Vec<_>>());
        self.top_songs
            .iter()
            .filter(|s| s.release_date.as_str() >= cutoff.as_str())
            .cloned()
            .collect()
    }

    /// Chart albums sorted newest first.
    pub fn new_releases(&self) -> Vec<Album> {
        let mut albums = self.top_albums.clone();
        albums.sort_by(|a, b| b.release_date.cmp(&a.release_date));
        albums
    }
}

/// Six weeks before the newest date in the list, as `YYYY-MM-DD`.
fn recent_cutoff(dates: &[&str]) -> String {
    let newest = dates.iter().max().copied().unwrap_or("");
    let (Some(y), Some(m)) = (
        newest.get(..4).and_then(|s| s.parse::<i32>().ok()),
        newest.get(5..7).and_then(|s| s.parse::<i32>().ok()),
    ) else {
        return String::new();
    };
    let day = newest.get(8..10).unwrap_or("01");
    let (y, m) = if m > 1 { (y, m - 1) } else { (y - 1, 12) };
    // A calendar month back, then two more weeks: close enough for "new".
    let d: i32 = day.parse().unwrap_or(1);
    let (y, m, d) = if d > 14 { (y, m, d - 14) } else if m > 1 { (y, m - 1, d + 14) } else { (y - 1, 12, d + 14) };
    format!("{y:04}-{m:02}-{:02}", d.min(28))
}

/// The signed-in user's Apple Music library.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RemoteLibrary {
    /// Shared so large song lists aren't copied on every render.
    pub songs: Arc<Vec<Song>>,
    #[serde(default)]
    pub favorites: Arc<Vec<Song>>,
    pub albums: Vec<Album>,
    pub playlists: Vec<Playlist>,
    pub recently_added: Vec<Album>,
    #[serde(default)]
    pub artists: Vec<Artist>,
}

/// A live Apple Music radio station.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub description: String,
    pub artwork: Artwork,
    pub url: String,
}

/// `2019-05-31` → `May 31, 2019`; leaves anything else as-is.
pub fn format_date(date: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    match parts.as_slice() {
        [y, m, d] => match (m.parse::<usize>(), d.get(..2).unwrap_or(d).parse::<u32>()) {
            (Ok(m @ 1..=12), Ok(d)) => format!("{} {d}, {y}", MONTHS[m - 1]),
            _ => date.to_string(),
        },
        [y, m] => match m.parse::<usize>() {
            Ok(m @ 1..=12) => format!("{} {y}", MONTHS[m - 1]),
            _ => date.to_string(),
        },
        _ => date.to_string(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    /// Apple's own relevance ranking across types (MusicKit only).
    pub top: Vec<TopResult>,
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

/// Signed-in Home: Apple's personal shelves.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PersonalHome {
    pub recent_tracks: Vec<Song>,
    pub shelves: Vec<(String, Vec<TopResult>)>,
    pub heavy_rotation: Vec<TopResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TopResult {
    Song(Song),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
}

#[derive(Clone, Debug)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
    pub top_songs: Vec<Song>,
    pub similar: Vec<Artist>,
    pub about: String,
}

impl ArtistDetail {
    /// Newest release of any kind.
    pub fn latest(&self) -> Option<&Album> {
        self.albums.iter().max_by(|a, b| a.release_date.cmp(&b.release_date))
    }

    /// Splits releases the way the Music app does: full albums vs. singles and EPs.
    pub fn split_releases(&self) -> (Vec<Album>, Vec<Album>) {
        self.albums
            .iter()
            .cloned()
            .partition(|a| !(a.title.ends_with(" - Single") || a.title.ends_with(" - EP")))
    }
}

pub fn format_ms(ms: u32) -> String {
    let secs = ms / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropped_artwork_urls() {
        let a = Artwork("https://x/a.jpg/{w}x{h}bb.jpg".into());
        assert_eq!(a.url_cropped(1000, 562), "https://x/a.jpg/1000x562sr.jpg");
        let p = Artwork("https://x/f.png/{w}x{h}SC.DN02.jpg".into());
        assert_eq!(p.url_cropped(1000, 562), "https://x/f.png/1000x1000SC.DN02.jpg");
    }

    #[test]
    fn artwork_template_from_sized_url() {
        let a = Artwork::from_sized("https://x/Music211/v4/a.jpg/100x100bb.jpg");
        assert_eq!(a.url(600), "https://x/Music211/v4/a.jpg/600x600bb.jpg");
        let p = Artwork::from_sized("https://x/f.png/100x100SC.DN02.jpg?l=en-US");
        assert_eq!(p.url(300), "https://x/f.png/300x300SC.DN02.jpg?l=en-US");
    }

    #[test]
    fn formats_release_dates() {
        assert_eq!(format_date("2019-05-31"), "May 31, 2019");
        assert_eq!(format_date("2026-10-16T07:00:00Z"), "Oct 16, 2026");
        assert_eq!(format_date("2001-02"), "Feb 2001");
        assert_eq!(format_date("1999"), "1999");
    }

    #[test]
    fn recent_cutoff_goes_back_about_six_weeks() {
        assert_eq!(recent_cutoff(&["2026-10-16", "2025-01-01"]), "2026-09-02");
        assert_eq!(recent_cutoff(&["2026-01-05"]), "2025-11-19");
    }
}
