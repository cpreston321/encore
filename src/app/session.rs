//! The queue and playback position, saved while playing and restored on the next launch,
//! paused, so a song closed midway picks up where it left off.

use super::*;

/// How often the position is saved while playing. A crash or force quit loses at most this much.
const SAVE_EVERY: Duration = Duration::from_secs(5);

#[derive(serde::Serialize, serde::Deserialize)]
struct Session {
    queue: crate::queue::Saved<Song>,
    position: f32,
}

fn path() -> PathBuf {
    api::data_dir().join("session.json")
}

impl MusicApp {
    pub(crate) fn save_session(&mut self) {
        self.session_saved_at = Some(std::time::Instant::now());
        if self.queue.current().is_none() {
            self.clear_session();
            return;
        }
        let session = Session { queue: self.queue.save(), position: self.position() };
        let Ok(json) = serde_json::to_vec(&session) else { return };
        let _ = std::fs::create_dir_all(api::data_dir());
        // Write-then-rename so a crash mid-write can't leave half a file.
        let tmp = path().with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }

    pub(crate) fn save_session_periodically(&mut self) {
        if self.session_saved_at.is_none_or(|t| t.elapsed() >= SAVE_EVERY) {
            self.save_session();
        }
    }

    pub(crate) fn clear_session(&mut self) {
        self.resume_at = None;
        let _ = std::fs::remove_file(path());
    }

    /// Puts the last session's queue back, paused at the saved position. Pressing play starts
    /// the song and seeks there (`apply_resume`).
    pub(crate) fn restore_session(&mut self, cx: &mut Context<Self>) {
        let Some(session) = std::fs::read(path()).ok().and_then(|bytes| serde_json::from_slice::<Session>(&bytes).ok())
        else {
            return;
        };
        if !self.queue.restore(session.queue) {
            return;
        }
        let Some(song) = self.queue.current().cloned() else { return };
        self.state = PlayState::Paused;
        self.resume_at = Some((song.id.clone(), session.position));
        self.progress.read(cx).set_resume(Some((session.position, song.duration_ms as f32 / 1000.)));
    }

    /// Seeks a song that's just started to where the last session left it, if it's that song.
    pub(crate) fn apply_resume(&mut self) {
        let Some((id, position)) = self.resume_at.take() else { return };
        if self.queue.current().is_none_or(|s| s.id != id) || position < 1. {
            return;
        }
        if let Some(audio) = self.audio.borrow().as_ref()
            && audio.duration().is_none_or(|d| position < d - 2.)
        {
            audio.seek(position);
        }
    }
}
