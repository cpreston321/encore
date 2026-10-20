//! macOS media integration: the keyboard's media keys, headphone buttons and Control
//! Center's Now Playing widget, via MediaPlayer's remote command and now-playing APIs.

use std::sync::mpsc::{Receiver, Sender, channel};

use block2::RcBlock;
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSImage;
use objc2_foundation::{NSDictionary, NSNumber, NSSize, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommandCenter,
    MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek(f64),
}

pub struct MediaControls {
    rx: Receiver<Command>,
    // The command center keeps targets alive as long as these tokens exist.
    _targets: Vec<Retained<AnyObject>>,
    last_key: Option<(String, bool, i64)>,
}

impl MediaControls {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let mut targets = Vec::new();
        unsafe {
            let center = MPRemoteCommandCenter::sharedCommandCenter();
            let simple = |tx: &Sender<Command>, cmd: Command| {
                let tx = tx.clone();
                RcBlock::new(move |_: std::ptr::NonNull<MPRemoteCommandEvent>| {
                    let _ = tx.send(cmd);
                    MPRemoteCommandHandlerStatus::Success
                })
            };
            for (command, cmd) in [
                (center.playCommand(), Command::Play),
                (center.pauseCommand(), Command::Pause),
                (center.togglePlayPauseCommand(), Command::Toggle),
                (center.nextTrackCommand(), Command::Next),
                (center.previousTrackCommand(), Command::Previous),
            ] {
                command.setEnabled(true);
                targets.push(command.addTargetWithHandler(&simple(&tx, cmd)));
            }
            let seek_tx = tx.clone();
            let seek = center.changePlaybackPositionCommand();
            seek.setEnabled(true);
            targets.push(seek.addTargetWithHandler(&RcBlock::new(
                move |event: std::ptr::NonNull<MPRemoteCommandEvent>| {
                    let event = event.cast::<MPChangePlaybackPositionCommandEvent>();
                    let _ = seek_tx.send(Command::Seek(event.as_ref().positionTime()));
                    MPRemoteCommandHandlerStatus::Success
                },
            )));
        }
        Self { rx, _targets: targets, last_key: None }
    }

    pub fn poll(&self) -> Vec<Command> {
        self.rx.try_iter().collect()
    }

    /// Publishes what's playing. Cheap to call every tick: it only touches the system when
    /// the song, play state or position (to the second) changes.
    pub fn update(&mut self, now: Option<NowPlaying>) {
        let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
        let Some(now) = now else {
            if self.last_key.take().is_some() {
                unsafe {
                    center.setNowPlayingInfo(None);
                    center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
                }
            }
            return;
        };
        let key = (now.id.clone(), now.playing, now.position as i64);
        if self.last_key.as_ref().is_some_and(|k| k.0 == key.0 && k.1 == key.1 && (k.2 - key.2).abs() <= 1) {
            return;
        }
        self.last_key = Some(key);
        unsafe {
            let mut keys: Vec<&NSString> = vec![
                MPMediaItemPropertyTitle,
                MPMediaItemPropertyArtist,
                MPMediaItemPropertyAlbumTitle,
                MPMediaItemPropertyPlaybackDuration,
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                MPNowPlayingInfoPropertyPlaybackRate,
            ];
            let mut values: Vec<Retained<AnyObject>> = vec![
                Retained::into_super(Retained::into_super(NSString::from_str(&now.title))),
                Retained::into_super(Retained::into_super(NSString::from_str(&now.artist))),
                Retained::into_super(Retained::into_super(NSString::from_str(&now.album))),
                Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(now.duration)))),
                Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(now.position)))),
                Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(
                    if now.playing { 1. } else { 0. },
                )))),
            ];
            if let Some(path) = &now.artwork_path
                && let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path))
            {
                let size = image.size();
                let artwork = MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                    MPMediaItemArtwork::alloc(),
                    size,
                    &RcBlock::new(move |_: NSSize| std::ptr::NonNull::from(&*image)),
                );
                keys.push(MPMediaItemPropertyArtwork);
                values.push(Retained::into_super(Retained::into_super(artwork)));
            }
            let refs: Vec<&AnyObject> = values.iter().map(|v| &**v).collect();
            let info = NSDictionary::from_slices(&keys, &refs);
            center.setNowPlayingInfo(Some(&info));
            center.setPlaybackState(if now.playing {
                MPNowPlayingPlaybackState::Playing
            } else {
                MPNowPlayingPlaybackState::Paused
            });
        }
    }
}

pub struct NowPlaying {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
    pub artwork_path: Option<String>,
}
