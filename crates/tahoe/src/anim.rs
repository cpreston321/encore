//! Motion for state that changes over time rather than per element: open/close transitions
//! that can reverse mid-way, color blends, and the Reduce Motion setting.

use std::time::{Duration, Instant};

use gpui::{Hsla, Rgba};

/// The system "Reduce motion" accessibility setting (read once), or the override from
/// `force_reduce_motion`. Components and transitions skip their motion when it's on.
pub fn reduce_motion() -> bool {
    static REDUCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REDUCE.get_or_init(|| {
        FORCE_REDUCE.load(std::sync::atomic::Ordering::Relaxed)
            || objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    })
}

/// Behaves as if Reduce Motion is on, for testing. Call before anything animates.
pub fn force_reduce_motion() {
    FORCE_REDUCE.store(true, std::sync::atomic::Ordering::Relaxed);
}

static FORCE_REDUCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Decelerating curve used for nearly every transition in the app.
pub fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Tracks an open/closed flag as a 0–1 value that eases between states, so things can
/// animate out as well as in. Flipping it mid-animation reverses from where it is.
#[derive(Clone, Copy, Debug)]
pub struct Presence {
    open: bool,
    at: Instant,
    duration: Duration,
}

impl Presence {
    pub fn new(duration: Duration) -> Self {
        Self { open: false, at: Instant::now() - duration * 2, duration }
    }

    /// Starts fully open, without animating in.
    pub fn new_open(duration: Duration) -> Self {
        Self { open: true, at: Instant::now() - duration * 2, duration }
    }

    pub fn set(&mut self, open: bool) {
        if open == self.open {
            return;
        }
        let p = self.linear();
        self.open = open;
        // Rewind the clock so the new direction starts from the current value.
        let done = if open { p } else { 1. - p };
        self.at = Instant::now() - self.duration.mul_f32(done);
    }

    fn linear(&self) -> f32 {
        // With Reduce Motion, things appear and disappear without animating.
        let t =
            if reduce_motion() { 1. } else { (self.at.elapsed().as_secs_f32() / self.duration.as_secs_f32()).min(1.) };
        if self.open { t } else { 1. - t }
    }

    /// Eased 0 (closed) – 1 (open).
    pub fn value(&self) -> f32 {
        ease_out(self.linear())
    }

    /// Worth rendering: open, or still animating closed.
    pub fn visible(&self) -> bool {
        self.open || self.linear() > 0.
    }

    pub fn animating(&self) -> bool {
        !reduce_motion() && self.at.elapsed() < self.duration
    }
}

/// A color that blends to new targets instead of jumping.
#[derive(Clone, Copy, Debug)]
pub struct ColorBlend {
    from: Hsla,
    to: Hsla,
    at: Instant,
    duration: Duration,
}

impl ColorBlend {
    pub fn new(color: Hsla, duration: Duration) -> Self {
        Self { from: color, to: color, at: Instant::now() - duration, duration }
    }

    /// Retargets (if `target` changed) and returns the current color.
    pub fn update(&mut self, target: Hsla) -> Hsla {
        if target != self.to {
            self.from = self.current();
            self.to = target;
            self.at = Instant::now();
        }
        self.current()
    }

    pub fn current(&self) -> Hsla {
        if reduce_motion() {
            return self.to;
        }
        let t = ease_out(self.at.elapsed().as_secs_f32() / self.duration.as_secs_f32());
        lerp(self.from, self.to, t)
    }

    pub fn animating(&self) -> bool {
        !reduce_motion() && self.at.elapsed() < self.duration
    }
}

/// Blends in RGB space, which avoids hue sweeps through unrelated colors.
pub fn lerp(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b): (Rgba, Rgba) = (a.into(), b.into());
    let mix = |x: f32, y: f32| x + (y - x) * t;
    Rgba { r: mix(a.r, b.r), g: mix(a.g, b.g), b: mix(a.b, b.b), a: mix(a.a, b.a) }.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_reverses_from_current_value() {
        let mut p = Presence::new(Duration::from_millis(200));
        assert!(!p.visible());
        p.set(true);
        // CI's macOS runners have Reduce Motion on, where presences snap instead.
        if reduce_motion() {
            assert_eq!(p.value(), 1.);
            p.set(false);
            assert!(!p.visible());
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        let mid = p.linear();
        p.set(false);
        assert!((p.linear() - mid).abs() < 0.05, "{} vs {mid}", p.linear());
        std::thread::sleep(Duration::from_millis(250));
        assert!(!p.visible());
    }

    #[test]
    fn lerp_endpoints() {
        let a: Hsla = gpui::rgb(0x000000).into();
        let b: Hsla = gpui::rgb(0xffffff).into();
        let mid: Rgba = lerp(a, b, 0.5).into();
        assert!((mid.r - 0.5).abs() < 0.01);
    }
}
