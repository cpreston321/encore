//! Rubber-band overscroll: scrolling past an edge with a trackpad stretches the view, and it
//! springs back once the gesture or its momentum ends.

use std::time::Instant;

use crate::{App, Global, ScrollWheelEvent, TouchPhase};

/// AppKit's stiffness for the stretch past an edge.
pub const RUBBER_BAND_COEFFICIENT: f32 = 0.55;

/// Angular frequency of the critically damped spring that pulls a stretched view back, in
/// radians per second. It settles in about 0.3 s, as quick as AppKit's.
pub const SPRING_BACK_OMEGA: f32 = 26.;

/// How far momentum may carry a view past its edge, as a fraction of the view's length.
const MAX_BOUNCE: f32 = 0.1;

/// Wheels with precise deltas but no gesture phases never say when they stop, so the stretch
/// is released this long after their last event.
const UNPHASED_RELEASE_SECS: f32 = 0.1;

/// How far content is drawn past an edge when the finger has moved `distance` past it, in a
/// view `dimension` long: `(1 - 1 / (|distance| * 0.55 / dimension + 1)) * dimension`, with the
/// sign of `distance`. It approaches `dimension` but never reaches it.
pub fn rubber_band(distance: f32, dimension: f32) -> f32 {
    if dimension <= 0. || distance == 0. {
        return 0.;
    }
    let x = distance.abs();
    ((1. - 1. / (x * RUBBER_BAND_COEFFICIENT / dimension + 1.)) * dimension).copysign(distance)
}

/// The finger distance past the edge that [`rubber_band`] draws `offset` away from it.
pub fn rubber_band_inverse(offset: f32, dimension: f32) -> f32 {
    if dimension <= 0. || offset == 0. {
        return 0.;
    }
    let v = offset.abs().min(dimension * 0.999);
    (v * dimension / (RUBBER_BAND_COEFFICIENT * (dimension - v))).copysign(offset)
}

/// A critically damped spring pulling `position` to zero, stepped exactly rather than
/// integrated, so it is stable at any frame rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    /// Displacement from rest, in pixels.
    pub position: f32,
    /// Pixels per second.
    pub velocity: f32,
}

impl Spring {
    /// Advances the spring by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let omega = SPRING_BACK_OMEGA;
        let decay = (-omega * dt).exp();
        let c = self.velocity + omega * self.position;
        self.position = (self.position + c * dt) * decay;
        self.velocity = (self.velocity - omega * c * dt) * decay;
    }

    /// Whether the spring is close enough to rest to stop drawing it.
    pub fn is_settled(&self) -> bool {
        self.position.abs() < 0.25 && self.velocity.abs() < 8.
    }
}

/// Turns elastic overscroll on or off for every scroll view in the app; it's on by default.
/// Apps can turn it off when the user has asked for reduced motion.
#[derive(Clone, Copy, Debug)]
pub struct ElasticScrolling(pub bool);

impl Global for ElasticScrolling {}

pub(crate) fn elastic_scrolling_enabled(cx: &App) -> bool {
    cx.try_global::<ElasticScrolling>().is_none_or(|e| e.0)
}

/// What produced a scroll event, which decides how it may move past an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollInput {
    /// A wheel with line deltas: clamps at the edges.
    Wheel,
    /// Fingers on the trackpad: stretches past the edges.
    Touch,
    /// Momentum after the fingers lift: bounces off the edges.
    Momentum,
}

impl ScrollInput {
    pub(crate) fn of(event: &ScrollWheelEvent) -> Self {
        if !event.delta.precise() {
            Self::Wheel
        } else if event.momentum_phase.is_some() {
            Self::Momentum
        } else {
            Self::Touch
        }
    }
}

/// Overscroll along one axis. Offsets follow gpui's convention: 0 at the start and `-max` at
/// the end, so a positive overscroll is past the start (top or left).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ElasticAxis {
    /// How far the finger has moved past the edge.
    stretch: f32,
    /// How far the content is drawn past the edge.
    pub(crate) overscroll: f32,
    spring: Option<Spring>,
    /// The side the spring is returning from, so it stops rather than crossing the edge.
    edge: f32,
    /// Momentum velocity, pixels per second.
    velocity: f32,
    /// Momentum already bounced off the edge in this fling.
    bounced: bool,
}

impl ElasticAxis {
    /// Applies one event's `delta` to `offset`, stretching past the edges of `0..=-max`.
    pub(crate) fn scroll(
        &mut self,
        input: ScrollInput,
        offset: &mut f32,
        delta: f32,
        max: f32,
        dimension: f32,
        dt: f32,
    ) {
        match input {
            ScrollInput::Wheel => *offset = (*offset + delta).clamp(-max, 0.),
            ScrollInput::Touch => self.drag(offset, delta, max, dimension),
            ScrollInput::Momentum => self.fling(offset, delta, max, dimension, dt),
        }
    }

    fn drag(&mut self, offset: &mut f32, mut delta: f32, max: f32, dimension: f32) {
        if delta == 0. {
            return;
        }
        self.spring = None;
        if self.stretch != 0. {
            let stretched = self.stretch + delta;
            if stretched * self.stretch > 0. {
                self.stretch = stretched;
                delta = 0.;
            } else {
                self.stretch = 0.;
                delta = stretched;
            }
        }
        let target = *offset + delta;
        if target > 0. {
            *offset = 0.;
            self.stretch = target;
        } else if target < -max {
            *offset = -max;
            self.stretch = target + max;
        } else {
            *offset = target;
        }
        self.overscroll = rubber_band(self.stretch, dimension);
    }

    fn fling(&mut self, offset: &mut f32, delta: f32, max: f32, dimension: f32, dt: f32) {
        if delta == 0. {
            return;
        }
        let velocity = delta / dt;
        self.velocity = if self.velocity == 0. {
            velocity
        } else {
            (self.velocity + velocity) / 2.
        };
        if self.bounced || self.spring.is_some() || self.overscroll != 0. {
            return;
        }
        let target = *offset + delta;
        if target > 0. || target < -max {
            *offset = target.clamp(-max, 0.);
            self.bounced = true;
            let cap = MAX_BOUNCE * dimension * SPRING_BACK_OMEGA * std::f32::consts::E;
            self.edge = delta.signum();
            self.spring = Some(Spring {
                position: 0.,
                velocity: self.velocity.abs().min(cap).copysign(delta),
            });
        } else {
            *offset = target;
        }
    }

    /// The fingers touched down: catch a view that's springing back where it is.
    fn grab(&mut self, dimension: f32) {
        if self.spring.take().is_some() {
            self.stretch = rubber_band_inverse(self.overscroll, dimension);
        }
        self.bounced = false;
        self.velocity = 0.;
    }

    /// The fingers lifted: spring back from wherever the view was stretched to.
    fn release(&mut self) {
        self.stretch = 0.;
        if self.overscroll != 0. && self.spring.is_none() {
            // The momentum that follows this gesture has had its bounce: without this,
            // once the spring settled, the momentum still pushing at the edge would
            // bounce the view a second time.
            self.bounced = true;
            self.edge = self.overscroll.signum();
            self.spring = Some(Spring {
                position: self.overscroll,
                velocity: 0.,
            });
        }
    }

    fn tick(&mut self, dt: f32) -> bool {
        let Some(spring) = self.spring.as_mut() else {
            return false;
        };
        spring.step(dt);
        if spring.is_settled() || spring.position * self.edge < 0. {
            self.spring = None;
            self.overscroll = 0.;
            false
        } else {
            self.overscroll = spring.position;
            true
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Overscroll for a scroll view on both axes, and the gesture it's part of.
#[derive(Clone, Debug, Default)]
pub(crate) struct Elastic {
    pub(crate) x: ElasticAxis,
    pub(crate) y: ElasticAxis,
    touching: bool,
    /// The current touch began with a phase, so it will also end with one.
    phased: bool,
    last_event: Option<Instant>,
    last_tick: Option<Instant>,
}

impl Elastic {
    /// Call before applying an event's deltas. Returns how to apply them, and the time since
    /// the previous event in seconds, for momentum velocity.
    pub(crate) fn begin(
        &mut self,
        event: &ScrollWheelEvent,
        dimensions: (f32, f32),
        now: Instant,
    ) -> (ScrollInput, f32) {
        let dt = self
            .last_event
            .map_or(1. / 120., |last| now.duration_since(last).as_secs_f32())
            .clamp(1. / 240., 1. / 30.);
        self.last_event = Some(now);
        // A spring keeps its own clock. Momentum events go on arriving while it pulls the
        // view back, each just before a frame, and restarting the clock at each one would
        // step the spring by only the time since that event: it would barely move until
        // the momentum ran out.
        if !self.springing() {
            self.last_tick = Some(now);
        }
        let input = ScrollInput::of(event);
        match input {
            ScrollInput::Touch => match event.touch_phase {
                TouchPhase::Started => {
                    self.touching = true;
                    self.phased = true;
                    self.x.grab(dimensions.0);
                    self.y.grab(dimensions.1);
                }
                TouchPhase::Moved if !self.touching => {
                    self.touching = true;
                    self.phased = false;
                    self.x.grab(dimensions.0);
                    self.y.grab(dimensions.1);
                }
                _ => {}
            },
            ScrollInput::Momentum => self.touching = false,
            ScrollInput::Wheel => {}
        }
        (input, dt)
    }

    /// Call after applying an event's deltas.
    pub(crate) fn end(&mut self, event: &ScrollWheelEvent) {
        match ScrollInput::of(event) {
            ScrollInput::Touch if matches!(event.touch_phase, TouchPhase::Ended) => self.release(),
            ScrollInput::Momentum if matches!(event.momentum_phase, Some(TouchPhase::Ended)) => {
                self.x.bounced = false;
                self.y.bounced = false;
            }
            _ => {}
        }
    }

    /// Lets go of a stretch, for when the gesture's end went to another view.
    pub(crate) fn release(&mut self) {
        self.touching = false;
        self.phased = false;
        self.x.release();
        self.y.release();
    }

    fn springing(&self) -> bool {
        self.x.spring.is_some() || self.y.spring.is_some()
    }

    pub(crate) fn is_stretched(&self) -> bool {
        self.x.overscroll != 0. || self.y.overscroll != 0.
    }

    /// Advances the springs to `now`. Returns whether another frame is needed.
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        if self.touching
            && !self.phased
            && self
                .last_event
                .is_some_and(|last| now.duration_since(last).as_secs_f32() > UNPHASED_RELEASE_SECS)
        {
            self.release();
        }
        let dt = self
            .last_tick
            .map_or(0., |last| now.duration_since(last).as_secs_f32())
            .min(0.05);
        self.last_tick = Some(now);
        let moving_x = self.x.tick(dt);
        let moving_y = self.y.tick(dt);
        moving_x || moving_y || (self.touching && !self.phased && self.is_stretched())
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ScrollDelta, point, px};
    use std::time::Duration;

    fn touch(phase: TouchPhase, dy: f32) -> ScrollWheelEvent {
        ScrollWheelEvent {
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            touch_phase: phase,
            ..Default::default()
        }
    }

    fn momentum(phase: TouchPhase, dy: f32) -> ScrollWheelEvent {
        ScrollWheelEvent {
            momentum_phase: Some(phase),
            ..touch(TouchPhase::Moved, dy)
        }
    }

    /// Feeds `event` through `elastic` the way the scroll listeners do, on the y axis.
    fn feed(
        elastic: &mut Elastic,
        offset: &mut f32,
        event: ScrollWheelEvent,
        max: f32,
        now: Instant,
    ) {
        let (input, dt) = elastic.begin(&event, (500., 500.), now);
        let delta = event.delta.pixel_delta(px(20.)).y.0;
        elastic.y.scroll(input, offset, delta, max, 500., dt);
        elastic.end(&event);
    }

    fn settle(elastic: &mut Elastic, mut now: Instant) -> (Instant, f32) {
        let mut peak = 0f32;
        for _ in 0..240 {
            now += Duration::from_millis(8);
            peak = peak.max(elastic.y.overscroll.abs());
            if !elastic.tick(now) {
                break;
            }
        }
        (now, peak)
    }

    #[test]
    fn momentum_events_dont_stall_the_spring_back() {
        let mut elastic = Elastic::default();
        let mut offset = 0.;
        let mut now = Instant::now();
        feed(&mut elastic, &mut offset, touch(TouchPhase::Started, 0.), 1000., now);
        for _ in 0..20 {
            now += Duration::from_millis(8);
            feed(&mut elastic, &mut offset, touch(TouchPhase::Moved, 30.), 1000., now);
        }
        now += Duration::from_millis(8);
        feed(&mut elastic, &mut offset, touch(TouchPhase::Ended, 0.), 1000., now);
        let stretched = elastic.y.overscroll;
        assert!(stretched > 50.);
        // Momentum arrives 1 ms before every frame for half a second.
        for _ in 0..60 {
            now += Duration::from_millis(7);
            feed(&mut elastic, &mut offset, momentum(TouchPhase::Moved, 4.), 1000., now);
            now += Duration::from_millis(1);
            elastic.tick(now);
        }
        assert!(
            elastic.y.overscroll.abs() < stretched * 0.05,
            "still {} of {stretched} past the edge",
            elastic.y.overscroll
        );
    }

    #[test]
    fn momentum_after_a_released_stretch_doesnt_bounce_again() {
        let mut elastic = Elastic::default();
        let mut offset = 0.;
        let mut now = Instant::now();
        feed(&mut elastic, &mut offset, touch(TouchPhase::Started, 0.), 1000., now);
        for _ in 0..10 {
            now += Duration::from_millis(8);
            feed(&mut elastic, &mut offset, touch(TouchPhase::Moved, 30.), 1000., now);
        }
        now += Duration::from_millis(8);
        feed(&mut elastic, &mut offset, touch(TouchPhase::Ended, 0.), 1000., now);
        // Momentum keeps pushing at the edge for two seconds, long after the spring settles.
        let mut settled = false;
        for frame in 0..250 {
            now += Duration::from_millis(8);
            feed(&mut elastic, &mut offset, momentum(TouchPhase::Moved, 6.), 1000., now);
            elastic.tick(now);
            if frame > 60 {
                settled = true;
                assert_eq!(elastic.y.overscroll, 0., "bounced again at frame {frame}");
            }
        }
        assert!(settled);
        // The next gesture's momentum can bounce again.
        now += Duration::from_millis(8);
        feed(&mut elastic, &mut offset, momentum(TouchPhase::Ended, 0.), 1000., now);
        feed(&mut elastic, &mut offset, touch(TouchPhase::Started, 0.), 1000., now);
        feed(&mut elastic, &mut offset, touch(TouchPhase::Ended, 0.), 1000., now);
        now += Duration::from_millis(8);
        feed(&mut elastic, &mut offset, momentum(TouchPhase::Moved, 40.), 1000., now);
        elastic.tick(now + Duration::from_millis(8));
        assert!(elastic.y.overscroll > 0.);
    }

    #[test]
    fn rubber_band_matches_appkit() {
        assert_eq!(rubber_band(0., 500.), 0.);
        let d = 500.;
        let x = 200.;
        let expected = (1. - 1. / (x * 0.55 / d + 1.)) * d;
        assert!((rubber_band(x, d) - expected).abs() < 1e-4);
        assert!((rubber_band(-x, d) + expected).abs() < 1e-4);
        assert!(rubber_band(1e7, d) < d);
        assert!(rubber_band(10., d) < 10. * RUBBER_BAND_COEFFICIENT + 1e-3);
        assert_eq!(rubber_band(100., 0.), 0.);
    }

    #[test]
    fn rubber_band_grows_ever_more_slowly() {
        let d = 400.;
        let mut last = 0.;
        let mut last_gain = f32::MAX;
        for step in 1..50 {
            let value = rubber_band(step as f32 * 20., d);
            let gain = value - last;
            assert!(gain > 0. && gain < last_gain);
            last = value;
            last_gain = gain;
        }
    }

    #[test]
    fn rubber_band_inverse_round_trips() {
        for &x in &[-900., -120., -1., 0., 0.5, 33., 400., 2500.] {
            let back = rubber_band_inverse(rubber_band(x, 600.), 600.);
            assert!((back - x).abs() < 0.05 * x.abs().max(1.), "{x} -> {back}");
        }
    }

    #[test]
    fn spring_settles_without_crossing_rest() {
        let mut spring = Spring {
            position: 120.,
            velocity: 0.,
        };
        let mut t = 0.;
        while !spring.is_settled() {
            spring.step(1. / 120.);
            assert!(spring.position >= 0.);
            t += 1. / 120.;
            assert!(t < 2., "spring didn't settle");
        }
        assert!(t > 0.15 && t < 0.45, "settled in {t}s");
    }

    #[test]
    fn spring_is_independent_of_frame_rate() {
        let mut fast = Spring {
            position: 80.,
            velocity: -200.,
        };
        let mut slow = fast;
        for _ in 0..120 {
            fast.step(1. / 120.);
        }
        for _ in 0..30 {
            slow.step(1. / 30.);
        }
        assert!((fast.position - slow.position).abs() < 1e-3);
        assert!((fast.velocity - slow.velocity).abs() < 1e-2);
    }

    #[test]
    fn spring_from_rest_with_velocity_peaks_and_returns() {
        let mut spring = Spring {
            position: 0.,
            velocity: 1000.,
        };
        let mut peak = 0f32;
        for _ in 0..240 {
            spring.step(1. / 120.);
            peak = peak.max(spring.position);
        }
        let expected = 1000. / (SPRING_BACK_OMEGA * std::f32::consts::E);
        assert!(
            (peak - expected).abs() < 1.5,
            "peak {peak}, expected {expected}"
        );
        assert!(spring.is_settled());
    }

    #[test]
    fn dragging_past_the_top_stretches_and_springs_back() {
        let mut elastic = Elastic::default();
        let mut offset = 0.;
        let now = Instant::now();
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Started, 0.),
            1000.,
            now,
        );
        for i in 1..=10 {
            feed(
                &mut elastic,
                &mut offset,
                touch(TouchPhase::Moved, 30.),
                1000.,
                now + Duration::from_millis(i * 8),
            );
        }
        assert_eq!(offset, 0.);
        let stretched = elastic.y.overscroll;
        assert!((stretched - rubber_band(300., 500.)).abs() < 1e-3);
        assert!(stretched > 0. && stretched < 300.);
        assert!(
            !elastic.tick(now + Duration::from_millis(200)),
            "holds while touching"
        );
        assert_eq!(elastic.y.overscroll, stretched);

        let now = now + Duration::from_millis(200);
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Ended, 0.),
            1000.,
            now,
        );
        let (_, peak) = settle(&mut elastic, now);
        assert!(peak <= stretched + 1e-3);
        assert_eq!(elastic.y.overscroll, 0.);
        assert!(!elastic.tick(now + Duration::from_secs(5)));
    }

    #[test]
    fn dragging_back_unstretches_before_scrolling() {
        let mut elastic = Elastic::default();
        let mut offset = -1000.;
        let now = Instant::now();
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Started, 0.),
            1000.,
            now,
        );
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Moved, -50.),
            1000.,
            now,
        );
        assert_eq!(offset, -1000.);
        assert!(elastic.y.overscroll < 0.);
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Moved, 80.),
            1000.,
            now,
        );
        assert_eq!(elastic.y.overscroll, 0.);
        assert_eq!(offset, -970.);
    }

    #[test]
    fn momentum_bounces_off_the_edge() {
        let mut elastic = Elastic::default();
        let mut offset = -950.;
        let mut now = Instant::now();
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Started, 0.),
            1000.,
            now,
        );
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Ended, 0.),
            1000.,
            now,
        );
        feed(
            &mut elastic,
            &mut offset,
            momentum(TouchPhase::Started, -20.),
            1000.,
            now,
        );
        for _ in 0..5 {
            now += Duration::from_millis(8);
            feed(
                &mut elastic,
                &mut offset,
                momentum(TouchPhase::Moved, -20.),
                1000.,
                now,
            );
        }
        assert_eq!(offset, -1000.);
        let (now, peak) = settle(&mut elastic, now);
        assert!(peak > 1. && peak <= MAX_BOUNCE * 500. + 1e-3, "peak {peak}");
        assert_eq!(elastic.y.overscroll, 0.);
        feed(
            &mut elastic,
            &mut offset,
            momentum(TouchPhase::Moved, -20.),
            1000.,
            now,
        );
        assert_eq!(elastic.y.overscroll, 0., "one bounce per fling");
        assert_eq!(offset, -1000.);
    }

    #[test]
    fn touching_down_catches_the_spring() {
        let mut elastic = Elastic::default();
        let mut offset = 0.;
        let now = Instant::now();
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Started, 0.),
            1000.,
            now,
        );
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Moved, 200.),
            1000.,
            now,
        );
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Ended, 0.),
            1000.,
            now,
        );
        let now = now + Duration::from_millis(50);
        assert!(elastic.tick(now));
        let caught = elastic.y.overscroll;
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Started, 0.),
            1000.,
            now,
        );
        assert!(!elastic.tick(now + Duration::from_millis(100)));
        assert!((elastic.y.overscroll - caught).abs() < 1e-3);
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Moved, 1.),
            1000.,
            now,
        );
        assert!(elastic.y.overscroll > caught);
    }

    #[test]
    fn unphased_precise_scrolling_releases_after_a_pause() {
        let mut elastic = Elastic::default();
        let mut offset = 0.;
        let now = Instant::now();
        feed(
            &mut elastic,
            &mut offset,
            touch(TouchPhase::Moved, 60.),
            1000.,
            now,
        );
        assert!(elastic.y.overscroll > 0.);
        assert!(elastic.tick(now + Duration::from_millis(50)));
        settle(&mut elastic, now + Duration::from_millis(50));
        assert_eq!(elastic.y.overscroll, 0.);
    }

    #[test]
    fn wheels_clamp() {
        let mut elastic = Elastic::default();
        let mut offset = -10.;
        let event = ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0., 3.)),
            ..Default::default()
        };
        feed(&mut elastic, &mut offset, event, 1000., Instant::now());
        assert_eq!(offset, 0.);
        assert_eq!(elastic.y.overscroll, 0.);
    }
}
