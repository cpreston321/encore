//! The player bar's fill as a Core Animation layer while a song plays. Core Animation moves it
//! along the song by itself, so the window presents no frames during playback: every frame
//! presented makes macOS hold about 390 MB for the window for a second or so, and the bar
//! used to redraw a few times a second.

use std::time::Instant;

use gpui::{Bounds, Hsla, Pixels, Rgba, Window};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{NSNumber, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// What the fill should show: where the bar is, its color, and how far along the song is.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct FillState {
    pub bounds: Bounds<Pixels>,
    pub thickness: f32,
    pub color: Hsla,
    pub position: f32,
    pub duration: f32,
}

pub(crate) struct NativeFill {
    layer: Retained<AnyObject>,
    host: Retained<AnyObject>,
    shown: Option<(FillState, Instant)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

unsafe impl objc2::Encode for CGPoint {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl objc2::Encode for CGSize {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl objc2::Encode for CGRect {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

impl NativeFill {
    /// Adds a hidden fill layer above the window's GPUI content.
    pub(crate) fn new(window: &Window) -> Option<Self> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return None };
        let view: &AnyObject = unsafe { appkit.ns_view.cast::<AnyObject>().as_ref() };
        unsafe {
            let host: Option<Retained<AnyObject>> = msg_send![view, layer];
            let host = host?;
            let class = AnyClass::get(c"CALayer")?;
            let layer: Retained<AnyObject> = msg_send![class, layer];
            let _: () = msg_send![&*layer, setHidden: true];
            let _: () = msg_send![&*layer, setAnchorPoint: CGPoint { x: 0., y: 0.5 }];
            let _: () = msg_send![&*layer, setZPosition: 1000f64];
            let _: () = msg_send![&*host, addSublayer: &*layer];
            Some(Self { layer, host, shown: None })
        }
    }

    /// Hides the fill; GPUI draws the bar again.
    pub(crate) fn hide(&mut self) -> bool {
        if self.shown.take().is_none() {
            return false;
        }
        Self::without_actions(|| unsafe {
            let _: () = msg_send![&*self.layer, removeAllAnimations];
            let _: () = msg_send![&*self.layer, setHidden: true];
        });
        true
    }

    /// Where the shown fill expects the song to be now.
    pub(crate) fn expected_position(&self) -> Option<f32> {
        self.shown.map(|(state, at)| state.position + at.elapsed().as_secs_f32())
    }

    /// Shows the fill and animates it to the end of the song. Does nothing if what's shown
    /// already matches, so it can be called on every render.
    pub(crate) fn show(&mut self, state: FillState) {
        if let Some((shown, at)) = self.shown {
            let expected = shown.position + at.elapsed().as_secs_f32();
            let same_place = shown.bounds == state.bounds
                && shown.thickness == state.thickness
                && shown.color == state.color
                && shown.duration == state.duration;
            if same_place && (expected - state.position).abs() < 0.5 {
                return;
            }
        }
        let host_height = unsafe {
            let bounds: CGRect = msg_send![&*self.host, bounds];
            bounds.size.height
        };
        let width = f64::from(f32::from(state.bounds.size.width));
        let fraction = |p: f32| f64::from((p / state.duration.max(0.1)).clamp(0., 1.));
        let from = width * fraction(state.position);
        let x = f64::from(f32::from(state.bounds.origin.x));
        let center = f64::from(f32::from(state.bounds.origin.y + state.bounds.size.height / 2.));
        // The host layer's origin is at the bottom left.
        let y = host_height - center;
        let height = f64::from(state.thickness);
        let remaining = f64::from((state.duration - state.position).max(0.));
        let Rgba { r, g, b, a } = state.color.to_rgb();
        Self::without_actions(|| unsafe {
            let layer = &*self.layer;
            let _: () = msg_send![layer, removeAllAnimations];
            let _: () = msg_send![layer, setPosition: CGPoint { x, y }];
            let _: () = msg_send![layer, setBounds: CGRect {
                origin: CGPoint { x: 0., y: 0. },
                size: CGSize { width, height },
            }];
            let _: () = msg_send![layer, setCornerRadius: height / 2.];
            if let Some(color_class) = AnyClass::get(c"NSColor") {
                let color: Retained<AnyObject> = msg_send![
                    color_class,
                    colorWithSRGBRed: f64::from(r),
                    green: f64::from(g),
                    blue: f64::from(b),
                    alpha: f64::from(a)
                ];
                let cg: *const std::ffi::c_void = msg_send![&*color, CGColor];
                let _: () = msg_send![layer, setBackgroundColor: cg];
            }
            let _: () = msg_send![layer, setHidden: false];
            if let Some(animation_class) = AnyClass::get(c"CABasicAnimation") {
                let key = NSString::from_str("bounds.size.width");
                let animation: Retained<AnyObject> = msg_send![animation_class, animationWithKeyPath: &*key];
                let _: () = msg_send![&*animation, setFromValue: &*NSNumber::new_f64(from)];
                let _: () = msg_send![&*animation, setToValue: &*NSNumber::new_f64(width)];
                let _: () = msg_send![&*animation, setDuration: remaining];
                let fill_mode = NSString::from_str("forwards");
                let _: () = msg_send![&*animation, setFillMode: &*fill_mode];
                let _: () = msg_send![&*animation, setRemovedOnCompletion: false];
                let _: () = msg_send![layer, addAnimation: &*animation, forKey: &*key];
            }
        });
        self.shown = Some((state, Instant::now()));
    }

    fn without_actions(f: impl FnOnce()) {
        let Some(transaction) = AnyClass::get(c"CATransaction") else { return f() };
        unsafe {
            let _: () = msg_send![transaction, begin];
            let _: () = msg_send![transaction, setDisableActions: true];
        }
        f();
        unsafe {
            let _: () = msg_send![transaction, commit];
        }
    }
}

impl crate::app::MusicApp {
    /// Hands the bar's fill to Core Animation while a song plays undisturbed, and back to
    /// GPUI for hovering, scrubbing, pausing, or when something covers the bar.
    pub(crate) fn sync_native_fill(&self, window: &Window, cx: &mut gpui::Context<Self>) {
        let progress = self.progress.read(cx);
        let (hover, hover_moving) = progress.hover_amount();
        let bounds = self.progress_bounds.get();
        let covered = self.show_welcome
            || self.show_now_playing
            || self.fx.borrow().now_playing.visible()
            || self.menu.is_some()
            || self.palette_open
            || self.show_shortcuts
            || self.new_playlist.is_some();
        let undisturbed = hover == 0.
            && !hover_moving
            && progress.scrub().is_none()
            && self.drag.is_none()
            && !covered
            && bounds.size.width > gpui::px(1.);
        let playing = self
            .audio
            .borrow()
            .as_ref()
            .filter(|audio| audio.is_playing())
            .map(|audio| (audio.position(), audio.duration().unwrap_or(30.)));
        let color = progress.fill_color();
        let mut fill = self.native_fill.borrow_mut();
        if fill.is_none() && playing.is_some() {
            *fill = NativeFill::new(window);
        }
        let Some(fill) = fill.as_mut() else { return };
        match playing.filter(|_| undisturbed) {
            Some((position, duration)) => {
                fill.show(FillState { bounds, thickness: 2.5, color, position, duration });
                let expected = fill.expected_position().unwrap_or(position);
                progress.set_native(Some((expected, Instant::now())));
            }
            None => {
                progress.set_native(None);
                if fill.hide() {
                    // GPUI's own fill is where it was when the native one took over.
                    self.progress.update(cx, |_, cx| cx.notify());
                }
            }
        }
    }
}
