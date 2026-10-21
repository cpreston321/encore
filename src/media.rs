//! macOS media integration: the keyboard's media keys, headphone buttons and Control
//! Center's Now Playing widget, via MediaPlayer's remote command and now-playing APIs.

use block2::RcBlock;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSImage;
use objc2_foundation::{NSDictionary, NSNumber, NSSize, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
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
    /// Taken by the app, which awaits commands instead of polling for them.
    commands: Option<UnboundedReceiver<Command>>,
    // The command center keeps targets alive as long as these tokens exist.
    _targets: Vec<Retained<AnyObject>>,
    last: Option<Published>,
    /// The artwork published for the current song, kept so republishing reuses it: each new
    /// MPMediaItemArtwork is rendered again by the system, a few MB at a time.
    artwork: Option<(String, Retained<MPMediaItemArtwork>)>,
}

/// What was last published, to tell whether the system needs telling again.
struct Published {
    id: String,
    playing: bool,
    position: f64,
    at: std::time::Instant,
}

impl MediaControls {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let mut targets = Vec::new();
        unsafe {
            let center = MPRemoteCommandCenter::sharedCommandCenter();
            let simple = |tx: &UnboundedSender<Command>, cmd: Command| {
                let tx = tx.clone();
                RcBlock::new(move |_: std::ptr::NonNull<MPRemoteCommandEvent>| {
                    let _ = tx.unbounded_send(cmd);
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
                    let _ = seek_tx.unbounded_send(Command::Seek(event.as_ref().positionTime()));
                    MPRemoteCommandHandlerStatus::Success
                },
            )));
        }
        Self { commands: Some(rx), _targets: targets, last: None, artwork: None }
    }

    pub fn take_commands(&mut self) -> Option<UnboundedReceiver<Command>> {
        self.commands.take()
    }

    /// Publishes what's playing. Cheap to call every tick: it only touches the system when
    /// the song or play state changes, or the position jumps (a seek). In between, the system
    /// advances the elapsed time itself from the playback rate.
    pub fn update(&mut self, now: Option<NowPlaying>) {
        let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
        let Some(now) = now else {
            self.artwork = None;
            if self.last.take().is_some() {
                unsafe {
                    center.setNowPlayingInfo(None);
                    center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
                }
            }
            return;
        };
        let unchanged = self.last.as_ref().is_some_and(|last| {
            let expected = last.position + if last.playing { last.at.elapsed().as_secs_f64() } else { 0. };
            last.id == now.id && last.playing == now.playing && (now.position - expected).abs() < 2.
        });
        if unchanged {
            return;
        }
        self.last = Some(Published {
            id: now.id.clone(),
            playing: now.playing,
            position: now.position,
            at: std::time::Instant::now(),
        });
        if self.artwork.as_ref().is_some_and(|(id, _)| *id != now.id) {
            self.artwork = None;
        }
        if self.artwork.is_none()
            && let Some(path) = &now.artwork_path
            && let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path))
        {
            let size = image.size();
            let artwork = unsafe {
                MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                    MPMediaItemArtwork::alloc(),
                    size,
                    &RcBlock::new(move |_: NSSize| std::ptr::NonNull::from(&*image)),
                )
            };
            self.artwork = Some((now.id.clone(), artwork));
        }
        unsafe {
            let mut keys: Vec<&NSString> = vec![
                MPMediaItemPropertyTitle,
                MPMediaItemPropertyArtist,
                MPMediaItemPropertyAlbumTitle,
                MPMediaItemPropertyPlaybackDuration,
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                MPNowPlayingInfoPropertyPlaybackRate,
            ];
            let mut values: Vec<Retained<AnyObject>> =
                vec![
                    Retained::into_super(Retained::into_super(NSString::from_str(&now.title))),
                    Retained::into_super(Retained::into_super(NSString::from_str(&now.artist))),
                    Retained::into_super(Retained::into_super(NSString::from_str(&now.album))),
                    Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(now.duration)))),
                    Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(now.position)))),
                    Retained::into_super(Retained::into_super(Retained::into_super(NSNumber::new_f64(
                        if now.playing { 1. } else { 0. },
                    )))),
                ];
            if let Some((_, artwork)) = &self.artwork {
                keys.push(MPMediaItemPropertyArtwork);
                values.push(Retained::into_super(Retained::into_super(artwork.clone())));
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
