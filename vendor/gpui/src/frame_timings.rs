use smallvec::SmallVec;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// Where the last drawn frame spent its time. Each phase excludes the phases nested in it:
/// a cached view that renders during prepaint counts toward `render` and `layout`, not
/// `prepaint`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimings {
    /// Views' `render` methods.
    pub render: Duration,
    /// Requesting and computing layout.
    pub layout: Duration,
    /// Prepaint, including replaying cached views' prepaint state.
    pub prepaint: Duration,
    /// Paint, including replaying cached views' scenes.
    pub paint: Duration,
    /// The whole of `Window::draw`, including finishing the frame.
    pub draw: Duration,
    /// Encoding and submitting the scene to the GPU.
    pub present: Duration,
    /// GPU time of the most recently completed frame (Metal only; zero elsewhere).
    pub gpu: Duration,
}

#[derive(Clone, Copy)]
pub(crate) enum FramePhase {
    Render,
    Layout,
    Prepaint,
    Paint,
}

#[derive(Default)]
pub(crate) struct PhaseClock {
    stack: SmallVec<[(FramePhase, Instant); 8]>,
    pub(crate) totals: FrameTimings,
}

impl PhaseClock {
    pub(crate) fn enter(&mut self, phase: FramePhase) {
        let now = Instant::now();
        if let Some((outer, since)) = self.stack.last().copied() {
            self.charge(outer, now - since);
        }
        self.stack.push((phase, now));
    }

    pub(crate) fn exit(&mut self) {
        let now = Instant::now();
        if let Some((phase, since)) = self.stack.pop() {
            self.charge(phase, now - since);
        }
        if let Some(outer) = self.stack.last_mut() {
            outer.1 = now;
        }
    }

    fn charge(&mut self, phase: FramePhase, took: Duration) {
        let t = &mut self.totals;
        match phase {
            FramePhase::Render => t.render += took,
            FramePhase::Layout => t.layout += took,
            FramePhase::Prepaint => t.prepaint += took,
            FramePhase::Paint => t.paint += took,
        }
    }
}

static LAST_GPU_NANOS: AtomicU64 = AtomicU64::new(0);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn record_gpu_time(took: Duration) {
    LAST_GPU_NANOS.store(took.as_nanos() as u64, Ordering::Relaxed);
}

pub(crate) fn last_gpu_time() -> Duration {
    Duration::from_nanos(LAST_GPU_NANOS.load(Ordering::Relaxed))
}
