//! Full songs through MusicKit's native player (macOS 14+), via native/EncoreNative.swift.
//! No web view: MusicKit streams through MediaPlayer's RemotePlayerService, which macOS runs
//! for the app. MusicKit has no volume control on macOS, so the Swift side routes the audio
//! through a Core Audio process tap when the volume isn't at full.
//!
//! The Swift code lives in libencore_native.dylib, loaded at runtime. Where it's missing or
//! can't load (macOS 13, a binary outside an app bundle) Encore uses the web player.

use std::{
    cell::Cell,
    ffi::{CString, c_char, c_int, c_void},
    sync::OnceLock,
};

#[allow(clippy::type_complexity)]
struct Lib {
    init: unsafe extern "C" fn(*const c_char) -> bool,
    authorization: unsafe extern "C" fn() -> i32,
    request_authorization: unsafe extern "C" fn(),
    load: unsafe extern "C" fn(*const c_char, u64),
    play: unsafe extern "C" fn(),
    pause: unsafe extern "C" fn(),
    stop: unsafe extern "C" fn(),
    seek: unsafe extern "C" fn(f64),
    set_volume: unsafe extern "C" fn(f32),
    enable_volume: unsafe extern "C" fn(),
    set_system_volume: unsafe extern "C" fn(f32),
    status: unsafe extern "C" fn() -> i32,
    time: unsafe extern "C" fn() -> f64,
    duration: unsafe extern "C" fn() -> f64,
    loaded_seq: unsafe extern "C" fn() -> u64,
    error_seq: unsafe extern "C" fn() -> u64,
    volume_control: unsafe extern "C" fn() -> i32,
    error: unsafe extern "C" fn(*mut c_char, usize) -> usize,
    samples: unsafe extern "C" fn(*mut f32, usize) -> f64,
}

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

fn library() -> Option<&'static Lib> {
    static LIB: OnceLock<Option<Lib>> = OnceLock::new();
    LIB.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let path = [dir.join("../Frameworks/libencore_native.dylib"), dir.join("libencore_native.dylib")]
            .into_iter()
            .find(|p| p.exists())?;
        let path = CString::new(path.to_str()?).ok()?;
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return None;
        }
        macro_rules! sym {
            ($name:literal) => {{
                let f = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
                if f.is_null() {
                    return None;
                }
                #[allow(clippy::missing_transmute_annotations)]
                unsafe {
                    std::mem::transmute::<*mut c_void, _>(f)
                }
            }};
        }
        Some(Lib {
            init: sym!("encore_native_init"),
            authorization: sym!("encore_native_authorization"),
            request_authorization: sym!("encore_native_request_authorization"),
            load: sym!("encore_native_load"),
            play: sym!("encore_native_play"),
            pause: sym!("encore_native_pause"),
            stop: sym!("encore_native_stop"),
            seek: sym!("encore_native_seek"),
            set_volume: sym!("encore_native_set_volume"),
            enable_volume: sym!("encore_native_enable_volume"),
            set_system_volume: sym!("encore_native_set_system_volume"),
            status: sym!("encore_native_status"),
            time: sym!("encore_native_time"),
            duration: sym!("encore_native_duration"),
            loaded_seq: sym!("encore_native_loaded_seq"),
            error_seq: sym!("encore_native_error_seq"),
            volume_control: sym!("encore_native_volume_control"),
            error: sym!("encore_native_error"),
            samples: sym!("encore_native_samples"),
        })
    })
    .as_ref()
}

pub enum Open {
    Ready(NativePlayer),
    /// The user hasn't answered MusicKit's permission prompt yet (it's been asked).
    Waiting,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeControl {
    None,
    Requesting,
    Active,
    Unavailable,
}

#[derive(Clone, Copy, Default)]
struct Track {
    /// Seen playing since the last load, so a stopped state afterwards is the song's end.
    started: bool,
    /// Seen playing within the last seconds of the song. At its end MusicKit reports
    /// "paused" with the time back at 0, so that's the only way to tell an end from a pause.
    near_end: bool,
    paused: bool,
}

/// MusicKit has one player per app, so there's at most one of these.
pub struct NativePlayer {
    lib: &'static Lib,
    seq: Cell<u64>,
    track: Cell<Track>,
}

impl NativePlayer {
    /// Must run on the main thread.
    pub fn open(developer_token: &str) -> Open {
        let Some(lib) = library() else { return Open::Unavailable };
        let Ok(token) = CString::new(developer_token) else { return Open::Unavailable };
        if !unsafe { (lib.init)(token.as_ptr()) } {
            return Open::Unavailable;
        }
        match unsafe { (lib.authorization)() } {
            3 => Open::Ready(Self { lib, seq: Cell::new(0), track: Cell::new(Track::default()) }),
            0 => {
                unsafe { (lib.request_authorization)() };
                Open::Waiting
            }
            _ => Open::Unavailable,
        }
    }

    fn loaded(&self) -> bool {
        self.seq.get() != 0 && unsafe { (self.lib.loaded_seq)() } == self.seq.get()
    }

    fn status(&self) -> i32 {
        if self.loaded() { unsafe { (self.lib.status)() } } else { 0 }
    }

    pub fn load(&self, song_id: &str, volume: f32) {
        let Ok(id) = CString::new(song_id) else { return };
        let seq = self.seq.get() + 1;
        self.seq.set(seq);
        self.track.set(Track::default());
        self.set_volume(volume);
        unsafe { (self.lib.load)(id.as_ptr(), seq) };
    }

    pub fn play(&self) {
        self.track.set(Track { paused: false, ..self.track.get() });
        unsafe { (self.lib.play)() };
    }

    pub fn pause(&self) {
        self.track.set(Track { paused: true, ..self.track.get() });
        unsafe { (self.lib.pause)() };
    }

    pub fn stop(&self) {
        self.track.set(Track { paused: true, ..self.track.get() });
        unsafe { (self.lib.stop)() };
    }

    pub fn seek(&self, secs: f32) {
        // Seeking back from the end means the song isn't about to finish anymore.
        self.track.set(Track { near_end: false, ..self.track.get() });
        unsafe { (self.lib.seek)(secs as f64) };
    }

    /// Has no effect until volume control is active (`enable_volume`).
    pub fn set_volume(&self, volume: f32) {
        unsafe { (self.lib.set_volume)(volume) };
    }

    /// Routes playback through a process tap so `set_volume` works. macOS asks for System
    /// Audio Recording permission the first time.
    pub fn enable_volume(&self) {
        unsafe { (self.lib.enable_volume)() };
    }

    /// The fallback when volume control is unavailable.
    pub fn set_system_volume(&self, volume: f32) {
        unsafe { (self.lib.set_system_volume)(volume) };
    }

    pub fn is_playing(&self) -> bool {
        self.observe();
        self.status() == 1
    }

    pub fn position(&self) -> f32 {
        if self.loaded() { unsafe { (self.lib.time)() as f32 } } else { 0. }
    }

    pub fn duration(&self) -> Option<f32> {
        let duration = unsafe { (self.lib.duration)() } as f32;
        (self.loaded() && duration > 0.).then_some(duration)
    }

    pub fn finished(&self) -> bool {
        self.observe();
        let track = self.track.get();
        if !track.started || track.paused {
            return false;
        }
        match self.status() {
            0 => true,
            2 => track.near_end,
            _ => false,
        }
    }

    fn observe(&self) {
        if self.status() != 1 {
            return;
        }
        let near_end = self.duration().is_some_and(|d| self.position() >= d - 3.);
        let track = self.track.get();
        self.track.set(Track { started: true, near_end: near_end || track.near_end, ..track });
    }

    pub fn startup_error(&self) -> Option<String> {
        if self.seq.get() == 0 || unsafe { (self.lib.error_seq)() } != self.seq.get() {
            return None;
        }
        let mut buf = vec![0 as c_char; 512];
        let len = unsafe { (self.lib.error)(buf.as_mut_ptr(), buf.len()) };
        let bytes: Vec<u8> = buf[..len.min(buf.len() - 1)].iter().map(|&c| c as u8).collect();
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Fills `out` with the latest mono samples of what's playing and returns the sample rate,
    /// or 0 when Encore isn't routing the audio (volume control off).
    pub fn samples(&self, out: &mut [f32]) -> f32 {
        unsafe { (self.lib.samples)(out.as_mut_ptr(), out.len()) as f32 }
    }

    pub fn volume_control(&self) -> VolumeControl {
        match unsafe { (self.lib.volume_control)() } {
            1 => VolumeControl::Requesting,
            2 => VolumeControl::Active,
            3 => VolumeControl::Unavailable,
            _ => VolumeControl::None,
        }
    }
}

impl Drop for NativePlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
