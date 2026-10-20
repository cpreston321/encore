//! Opt-in performance logging (`ENCORE_PERF=1`): frame timings, slow frames, request
//! durations and startup milestones, printed to stderr.

use std::{
    cell::RefCell,
    sync::OnceLock,
    time::{Duration, Instant},
};

/// A frame slower than this misses a 120Hz (ProMotion) refresh.
const SLOW_FRAME: Duration = Duration::from_micros(8_333);
/// Frames closer together than this are part of one animation or scroll; their spacing is
/// the frame rate actually achieved.
const CONTINUOUS: Duration = Duration::from_millis(40);
const REPORT_EVERY: Duration = Duration::from_secs(5);

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ENCORE_PERF").is_some())
}

fn process_start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Call early in `main` so milestones are measured from launch.
pub fn init() {
    process_start();
}

pub fn milestone(name: &str) {
    if enabled() {
        eprintln!("[perf] {name} at {:.0}ms", process_start().elapsed().as_secs_f64() * 1000.);
    }
}

pub fn request(label: &str, took: Duration) {
    if enabled() {
        eprintln!("[perf] request {label}: {:.0}ms", took.as_secs_f64() * 1000.);
    }
}

#[derive(Default)]
struct Frames {
    started: Option<Instant>,
    route: String,
    samples: Vec<Duration>,
    window_start: Option<Instant>,
    last_start: Option<Instant>,
    intervals: Vec<Duration>,
}

thread_local! {
    static FRAMES: RefCell<Frames> = RefCell::default();
}

/// Marks the start of a frame (top of the root view's render).
pub fn frame_start(route: &str) {
    if !enabled() {
        return;
    }
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let now = Instant::now();
        if let Some(gap) = f.last_start.map(|t| now - t).filter(|gap| *gap < CONTINUOUS) {
            f.intervals.push(gap);
        }
        f.last_start = Some(now);
        f.started = Some(now);
        if f.route != route {
            // Each report covers one page.
            f.route = route.to_string();
            f.samples.clear();
            f.intervals.clear();
            f.window_start = None;
        }
    });
}

/// Marks the end of a frame (after the last element painted).
pub fn frame_end() {
    if !enabled() {
        return;
    }
    FRAMES.with(|f| {
        let mut f = f.borrow_mut();
        let Some(started) = f.started.take() else {
            return;
        };
        let took = started.elapsed();
        if took > SLOW_FRAME * 2 {
            eprintln!("[perf] slow frame on {}: {:.1}ms", f.route, took.as_secs_f64() * 1000.);
        }
        f.samples.push(took);
        let window_start = *f.window_start.get_or_insert_with(Instant::now);
        if window_start.elapsed() >= REPORT_EVERY {
            let mut samples = std::mem::take(&mut f.samples);
            samples.sort();
            let n = samples.len();
            let avg = samples.iter().sum::<Duration>() / n as u32;
            let p95 = samples[(n * 95 / 100).min(n - 1)];
            let max = samples[n - 1];
            let slow = samples.iter().filter(|d| **d > SLOW_FRAME).count();
            let ms = |d: Duration| d.as_secs_f64() * 1000.;
            let mut intervals = std::mem::take(&mut f.intervals);
            intervals.sort();
            let fps = intervals
                .get(intervals.len() / 2)
                .map(|d| format!(", animating at {:.0}fps", 1. / d.as_secs_f64()))
                .unwrap_or_default();
            eprintln!(
                "[perf] {}: {n} frames in {:.0}s, avg {:.1}ms, p95 {:.1}ms, max {:.1}ms, {slow} over 8.3ms{fps}",
                f.route,
                window_start.elapsed().as_secs_f64(),
                ms(avg),
                ms(p95),
                ms(max),
            );
            f.window_start = Some(Instant::now());
        }
    });
}
