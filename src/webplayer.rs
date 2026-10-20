//! Full-length playback for signed-in subscribers. Apple only streams complete songs to its
//! own players, so this runs MusicKit JS (Apple's web player) in a hidden WebKit view and
//! drives it over a small localhost bridge: the page polls for commands and posts back its
//! playback state.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest};
use objc2_web_kit::{WKAudiovisualMediaTypes, WKWebView, WKWebViewConfiguration};
use serde::Deserialize;
use serde_json::{Value, json};

// MusicKit.PlaybackStates
const PLAYING: i64 = 2;
const STOPPED: i64 = 4;
const ENDED: i64 = 5;
const COMPLETED: i64 = 10;

#[derive(Clone, Debug, Default, Deserialize)]
struct Snapshot {
    #[serde(default)]
    state: i64,
    #[serde(default)]
    time: f64,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Default)]
struct Track {
    /// Song we last asked the page to play.
    expected: Option<String>,
    /// Whether that song has been seen playing (so an "ended" state is really its end).
    started: bool,
    paused: bool,
}

struct Shared {
    snapshot: Mutex<(Snapshot, Instant)>,
    /// The position last shown and when, for a smooth, monotonic progress bar.
    shown: Mutex<Option<(f32, Instant)>>,
    /// Reports older than this predate the last seek and are ignored.
    seeked_at: Mutex<Option<Instant>>,
    commands: Mutex<Vec<Value>>,
    track: Mutex<Track>,
}

pub struct WebPlayer {
    shared: Arc<Shared>,
    // Kept alive for the lifetime of the player; the page dies with them.
    _window: Retained<NSWindow>,
    _webview: Retained<WKWebView>,
}

impl WebPlayer {
    /// Must run on the main thread (it creates AppKit/WebKit objects).
    pub fn new(developer_token: &str, user_token: &str) -> Result<Self> {
        let shared = Arc::new(Shared {
            snapshot: Mutex::new((Snapshot::default(), Instant::now())),
            shown: Mutex::new(None),
            seeked_at: Mutex::new(None),
            commands: Mutex::new(Vec::new()),
            track: Mutex::new(Track::default()),
        });
        let key = random_key()?;
        let page = PAGE
            .replace("__DEVELOPER_TOKEN__", &serde_json::to_string(developer_token)?)
            .replace("__USER_TOKEN__", &serde_json::to_string(user_token)?)
            .replace("__KEY__", &key);
        let listener = TcpListener::bind("127.0.0.1:0").context("starting player bridge")?;
        let port = listener.local_addr()?.port();
        let server_shared = shared.clone();
        let server_key = key.clone();
        std::thread::Builder::new()
            .name("musickit-bridge".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = handle(stream, &server_shared, &server_key, &page);
                }
            })?;

        let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("the web player must be created on the main thread"))?;
        let frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(64., 64.));
        let (window, webview) = unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            // Playback is started by the app, not a click inside the page.
            config.setMediaTypesRequiringUserActionForPlayback(WKAudiovisualMediaTypes::None);
            let webview = WKWebView::initWithFrame_configuration(mtm.alloc(), frame, &config);
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setReleasedWhenClosed(false);
            window.setContentView(Some(&webview));
            // Invisible, but kept in the window list so WebKit doesn't treat the page as
            // hidden and throttle it.
            window.setAlphaValue(0.0);
            window.setIgnoresMouseEvents(true);
            window.orderBack(None);
            let url = NSURL::URLWithString(&NSString::from_str(&format!("http://localhost:{port}/player?k={key}")))
                .ok_or_else(|| anyhow!("bad player URL"))?;
            webview.loadRequest(&NSURLRequest::requestWithURL(&url));
            (window, webview)
        };
        Ok(Self { shared, _window: window, _webview: webview })
    }

    fn send(&self, command: Value) {
        self.shared.commands.lock().unwrap().push(command);
    }

    fn snapshot(&self) -> (Snapshot, Instant) {
        self.shared.snapshot.lock().unwrap().clone()
    }

    pub fn load(&self, song_id: &str, volume: f32) {
        *self.shared.track.lock().unwrap() = Track { expected: Some(song_id.to_string()), started: false, paused: false };
        *self.shared.shown.lock().unwrap() = None;
        self.send(json!({ "op": "load", "id": song_id, "volume": volume }));
    }

    pub fn play(&self) {
        self.shared.track.lock().unwrap().paused = false;
        self.send(json!({ "op": "play" }));
    }

    pub fn pause(&self) {
        self.shared.track.lock().unwrap().paused = true;
        self.send(json!({ "op": "pause" }));
    }

    pub fn stop(&self) {
        *self.shared.track.lock().unwrap() = Track::default();
        self.send(json!({ "op": "stop" }));
    }

    pub fn seek(&self, secs: f32) {
        // Jump the shown position straight to the target instead of easing toward it.
        *self.shared.shown.lock().unwrap() = Some((secs, Instant::now()));
        *self.shared.seeked_at.lock().unwrap() = Some(Instant::now());
        self.send(json!({ "op": "seek", "t": secs }));
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(json!({ "op": "volume", "v": volume }));
    }

    pub fn is_playing(&self) -> bool {
        let track = self.shared.track.lock().unwrap();
        !track.paused && track.started && self.snapshot().0.state == PLAYING
    }

    /// Seconds into the current song. The page reports coarse times a few times a second,
    /// so the shown position runs on its own clock and eases toward each report; it never
    /// steps backwards (which made the bar jitter) unless the song really jumped.
    pub fn position(&self) -> f32 {
        let (snap, at) = self.snapshot();
        if !self.shared.track.lock().unwrap().started {
            return 0.;
        }
        let playing = snap.state == PLAYING;
        // Right after a seek the page still reports the old time; trust our own clock
        // until a report arrives from after the seek.
        let stale = self.shared.seeked_at.lock().unwrap().is_some_and(|s| at < s + Duration::from_millis(150));
        if stale {
            let mut shown = self.shared.shown.lock().unwrap();
            let next = shown.map_or(0., |(v, since)| v + if playing { since.elapsed().as_secs_f32() } else { 0. });
            *shown = Some((next, Instant::now()));
            return next;
        }
        let reported = (snap.time + if playing { at.elapsed().as_secs_f64().min(1.) } else { 0. }) as f32;
        let mut shown = self.shared.shown.lock().unwrap();
        let next = match *shown {
            Some((value, since)) => {
                let predicted = value + if playing { since.elapsed().as_secs_f32() } else { 0. };
                let drift = reported - predicted;
                if drift.abs() > 2. {
                    reported // a seek or track change
                } else {
                    // Correct a tenth of the drift per frame, but only ever forward.
                    (predicted + drift * 0.1).max(value)
                }
            }
            None => reported,
        };
        *shown = Some((next, Instant::now()));
        next
    }

    pub fn duration(&self) -> Option<f32> {
        let snap = self.snapshot().0;
        (snap.duration > 0. && self.shared.track.lock().unwrap().started).then_some(snap.duration as f32)
    }

    /// The current song played to the end.
    pub fn finished(&self) -> bool {
        let snap = self.snapshot().0;
        let track = self.shared.track.lock().unwrap();
        track.started && !track.paused && matches!(snap.state, ENDED | COMPLETED | STOPPED)
    }

    /// An error reported for the current song before it started playing.
    pub fn startup_error(&self) -> Option<String> {
        let snap = self.snapshot().0;
        let track = self.shared.track.lock().unwrap();
        (!track.started && track.expected.is_some()).then_some(snap.error).flatten()
    }

    fn observe(shared: &Shared, snap: Snapshot) {
        {
            let mut track = shared.track.lock().unwrap();
            if snap.state == PLAYING && snap.id.is_some() && snap.id == track.expected {
                track.started = true;
            }
        }
        *shared.snapshot.lock().unwrap() = (snap, Instant::now());
    }
}

fn handle(mut stream: TcpStream, shared: &Shared, key: &str, page: &str) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    // Only the page we served knows the key, so other local pages can't drive playback.
    if query != format!("k={key}") {
        return respond(&mut stream, "403 Forbidden", "text/plain", "forbidden");
    }
    match (method, path) {
        ("GET", "/player") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", page),
        ("GET", "/cmd") => {
            let commands = std::mem::take(&mut *shared.commands.lock().unwrap());
            respond(&mut stream, "200 OK", "application/json", &serde_json::to_string(&commands)?)
        }
        ("POST", "/state") if content_length <= 64 * 1024 => {
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body)?;
            if let Ok(snap) = serde_json::from_slice::<Snapshot>(&body) {
                if let Some(err) = &snap.error {
                    eprintln!("musickit: {err}");
                }
                WebPlayer::observe(shared, snap);
            }
            respond(&mut stream, "204 No Content", "text/plain", "")
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}

fn random_key() -> Result<String> {
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Encore player</title>
<script src="https://js-cdn.music.apple.com/musickit/v3/musickit.js" async></script>
</head><body>
<script>
const KEY = "__KEY__";
let music = null;
let lastError = null;

function snapshot() {
  if (!music) return { ready: false, error: lastError };
  const item = music.nowPlayingItem;
  return {
    ready: true,
    state: music.playbackState,
    time: music.currentPlaybackTime || 0,
    duration: music.currentPlaybackDuration || 0,
    id: item ? String(item.id) : null,
    error: lastError,
  };
}

function report() {
  fetch('/state?k=' + KEY, { method: 'POST', body: JSON.stringify(snapshot()) }).catch(() => {});
}

function describe(e) {
  return String((e && (e.errorCode || e.message || e.name)) || e);
}

async function run(c) {
  try {
    switch (c.op) {
      case 'load':
        lastError = null;
        if (c.volume != null) music.volume = c.volume;
        await music.setQueue({ song: c.id, startPlaying: true });
        break;
      case 'play': await music.play(); break;
      case 'pause': music.pause(); break;
      case 'stop': await music.stop(); break;
      case 'seek': await music.seekToTime(c.t); break;
      case 'volume': music.volume = c.v; break;
    }
  } catch (e) {
    lastError = c.op + ': ' + describe(e);
  }
  report();
}

async function poll() {
  for (;;) {
    try {
      const commands = await (await fetch('/cmd?k=' + KEY)).json();
      for (const c of commands) await run(c);
    } catch (e) {}
    await new Promise(r => setTimeout(r, 80));
  }
}

document.addEventListener('musickitloaded', async () => {
  try {
    music = await MusicKit.configure({
      developerToken: __DEVELOPER_TOKEN__,
      app: { name: 'Encore', build: '1.0.0' },
    });
    music.musicUserToken = __USER_TOKEN__;
    music.addEventListener('playbackStateDidChange', report);
    music.addEventListener('mediaPlaybackError', e => { lastError = 'playback: ' + describe(e); report(); });
  } catch (e) {
    lastError = 'configure: ' + describe(e);
  }
  report();
  setInterval(report, 250);
  poll();
});
</script>
</body></html>
"#;
