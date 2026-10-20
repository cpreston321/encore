use std::{path::Path, time::Duration};

use anyhow::{Context as _, Result};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

/// Playback engine: 30-second previews decoded locally, or full songs through Apple's
/// web player when signed in.
pub enum Audio {
    Preview(Preview),
    Full(std::rc::Rc<crate::webplayer::WebPlayer>),
}

impl Audio {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full(_))
    }

    pub fn play(&self) {
        match self {
            Self::Preview(p) => p.play(),
            Self::Full(w) => w.play(),
        }
    }

    pub fn pause(&self) {
        match self {
            Self::Preview(p) => p.pause(),
            Self::Full(w) => w.pause(),
        }
    }

    pub fn stop(&mut self) {
        match self {
            Self::Preview(p) => p.stop(),
            Self::Full(w) => w.stop(),
        }
    }

    pub fn is_playing(&self) -> bool {
        match self {
            Self::Preview(p) => p.is_playing(),
            Self::Full(w) => w.is_playing(),
        }
    }

    pub fn finished(&self) -> bool {
        match self {
            Self::Preview(p) => p.finished(),
            Self::Full(w) => w.finished(),
        }
    }

    pub fn position(&self) -> f32 {
        match self {
            Self::Preview(p) => p.position(),
            Self::Full(w) => w.position(),
        }
    }

    pub fn duration(&self) -> Option<f32> {
        match self {
            Self::Preview(p) => p.duration(),
            Self::Full(w) => w.duration(),
        }
    }

    pub fn seek(&self, secs: f32) {
        match self {
            Self::Preview(p) => p.seek(secs),
            Self::Full(w) => w.seek(secs),
        }
    }

    pub fn set_volume(&self, volume: f32) {
        match self {
            Self::Preview(p) => p.set_volume(volume),
            Self::Full(w) => w.set_volume(volume),
        }
    }
}

/// One output stream with a single-track player on it.
pub struct Preview {
    _sink: MixerDeviceSink,
    player: Player,
    duration: Option<Duration>,
}

impl Preview {
    pub fn new() -> Result<Self> {
        let mut sink = open_output()?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        Ok(Self { _sink: sink, player, duration: None })
    }

    pub fn load(&mut self, path: &Path, volume: f32) -> Result<()> {
        let file = std::fs::File::open(path)?;
        let decoder = Decoder::try_from(file).context("decoding preview")?;
        self.duration = decoder.total_duration();
        self.player.clear();
        self.player.set_volume(volume);
        self.player.append(decoder);
        self.player.play();
        Ok(())
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn stop(&mut self) {
        self.player.clear();
        self.duration = None;
    }

    pub fn is_playing(&self) -> bool {
        !self.player.is_paused() && !self.player.empty()
    }

    pub fn finished(&self) -> bool {
        self.player.empty()
    }

    pub fn position(&self) -> f32 {
        self.player.get_pos().as_secs_f32()
    }

    pub fn duration(&self) -> Option<f32> {
        self.duration.map(|d| d.as_secs_f32())
    }

    pub fn seek(&self, secs: f32) {
        let _ = self.player.try_seek(Duration::from_secs_f32(secs.max(0.)));
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }
}

/// rodio asks for a fixed buffer of ~50ms (2048 frames at 48kHz), which some outputs
/// (e.g. Bluetooth/USB devices capped at 960) reject. Let the OS pick first, then try a
/// small fixed size, then rodio's own fallbacks.
fn open_output() -> Result<MixerDeviceSink> {
    use rodio::cpal::BufferSize;
    let mut last_err = None;
    for buffer in [BufferSize::Default, BufferSize::Fixed(512)] {
        match DeviceSinkBuilder::from_default_device().and_then(|b| b.with_buffer_size(buffer).open_stream()) {
            Ok(sink) => return Ok(sink),
            Err(e) => last_err = Some(e),
        }
    }
    DeviceSinkBuilder::open_default_sink().map_err(|e| last_err.unwrap_or(e)).context("opening audio output")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens the real output device and plays a downloaded preview silently.
    /// Run with `cargo test plays_preview -- --ignored`.
    #[test]
    #[ignore]
    fn plays_preview_on_default_output() {
        let (_, tracks) = crate::api::Catalog::Public.album("6822944288").unwrap();
        let url = tracks.iter().find_map(|t| t.preview_url.clone()).unwrap();
        let path = crate::api::download_cached(&url, "previews", "m4a").unwrap();
        let mut audio = Preview::new().unwrap();
        audio.load(&path, 0.0).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        assert!(audio.position() > 0.5, "position {}", audio.position());
        assert!(audio.duration().is_some_and(|d| d > 20.));
    }
}
