//! The local library, used when not signed in to Apple Music. Signed-in users get their
//! real library from the Apple Music API instead (see `RemoteLibrary`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Album, Playlist, Song};

#[derive(Default, Serialize, Deserialize)]
pub struct Library {
    /// Newest first.
    pub albums: Vec<Album>,
    /// Newest first. Includes the tracks of added albums.
    pub songs: Vec<Song>,
    pub favorites: Vec<Song>,
    pub playlists: Vec<Playlist>,
    #[serde(skip)]
    path: PathBuf,
}

impl Library {
    pub fn load(path: PathBuf) -> Self {
        let mut lib: Self =
            std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        lib.path = path;
        lib
    }

    pub fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }

    pub fn has_song(&self, id: &str) -> bool {
        self.songs.iter().any(|s| s.id == id)
    }

    pub fn has_album(&self, id: &str) -> bool {
        self.albums.iter().any(|a| a.id == id)
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.favorites.iter().any(|s| s.id == id)
    }

    pub fn has_playlist(&self, id: &str) -> bool {
        self.playlists.iter().any(|p| p.id == id)
    }

    pub fn toggle_song(&mut self, song: &Song) {
        if self.has_song(&song.id) {
            self.songs.retain(|s| s.id != song.id);
        } else {
            self.songs.insert(0, song.clone());
        }
        self.save();
    }

    pub fn toggle_favorite(&mut self, song: &Song) {
        if self.is_favorite(&song.id) {
            self.favorites.retain(|s| s.id != song.id);
        } else {
            self.favorites.insert(0, song.clone());
        }
        self.save();
    }

    pub fn toggle_playlist(&mut self, playlist: &Playlist) {
        if self.has_playlist(&playlist.id) {
            self.playlists.retain(|p| p.id != playlist.id);
        } else {
            self.playlists.insert(0, playlist.clone());
        }
        self.save();
    }

    pub fn toggle_album(&mut self, album: &Album, tracks: &[Song]) {
        if self.has_album(&album.id) {
            self.albums.retain(|a| a.id != album.id);
            self.songs.retain(|s| s.album_id.as_deref() != Some(album.id.as_str()));
        } else {
            self.albums.insert(0, album.clone());
            for track in tracks.iter().rev() {
                if !self.has_song(&track.id) {
                    self.songs.insert(0, track.clone());
                }
            }
        }
        self.save();
    }
}

/// Unique artists with their first album (for artwork), sorted by name.
pub fn artists(songs: &[Song], albums: &[Album]) -> Vec<(String, Option<String>, Option<Album>)> {
    let mut out: Vec<(String, Option<String>, Option<Album>)> = Vec::new();
    for song in songs {
        if !out.iter().any(|(name, _, _)| *name == song.artist) {
            let album = albums.iter().find(|a| a.artist == song.artist).cloned();
            out.push((song.artist.clone(), song.artist_id.clone(), album));
        }
    }
    out.sort_by_key(|(name, _, _)| name.to_lowercase());
    out
}
