//! Trackpad gestures: swiping between pages, swiping the player to skip songs, scrolling over
//! the volume control, and shelves that settle on a tile.

use std::time::{Duration, Instant};

use gpui::{Axis, Point, ScrollHandle, ScrollWheelEvent, TouchPhase, Window, gesture_axis, point};
use tahoe::haptics::{Haptic, perform};

use super::*;

/// How far a swipe has to go before letting go acts on it.
const PAGE_SWIPE: f32 = 110.;
const SKIP_SWIPE: f32 = 70.;
/// How long a shelf takes to settle on a tile.
const SNAP: Duration = Duration::from_millis(260);
/// How long after the fingers lift to wait for momentum before settling a shelf.
const MOMENTUM_GRACE: Duration = Duration::from_millis(90);
/// A swipe that gets no events for this long is abandoned.
const SWIPE_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(crate) struct Shelf {
    handle: ScrollHandle,
    /// When the last gesture's fingers lifted, while waiting to see if momentum follows.
    lifted: Option<Instant>,
    snap: Option<(f32, f32, Instant)>,
}

/// A two-finger swipe in progress: how far it has gone and whether it has passed the point
/// where letting go acts on it.
#[derive(Clone, Copy, Default)]
pub(crate) struct Swipe {
    pub offset: f32,
    armed: bool,
}

impl Swipe {
    /// Adds a gesture's movement; returns true if it just crossed `threshold` either way.
    fn add(&mut self, delta: f32, threshold: f32) -> bool {
        self.offset += delta;
        let armed = self.offset.abs() >= threshold;
        let crossed = armed != self.armed;
        self.armed = armed;
        crossed && armed
    }
}

fn horizontal_gesture(e: &ScrollWheelEvent) -> bool {
    e.delta.precise() && gesture_axis(e) == Some(Axis::Horizontal)
}

impl MusicApp {
    /// A horizontal carousel that settles with a tile at its edge, as the Music app's do.
    pub(crate) fn shelf(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let handle = self.shelves.borrow_mut().entry(id).or_default().handle.clone();
        div()
            .id(id)
            .px(px(PAD))
            .pb_2()
            .flex()
            .gap(px(GAP))
            .overflow_x_scroll()
            .track_scroll(&handle)
            .map(lock_scroll_axis)
            .on_scroll_wheel(cx.listener(move |this, e: &ScrollWheelEvent, window, cx| {
                this.shelf_scrolled(id, e, window, cx);
            }))
    }

    fn shelf_scrolled(&mut self, id: &'static str, e: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !e.delta.precise() {
            return;
        }
        let mut shelves = self.shelves.borrow_mut();
        let Some(shelf) = shelves.get_mut(id) else { return };
        let gesture_end = matches!(e.touch_phase, TouchPhase::Ended) && e.momentum_phase.is_none();
        match e.momentum_phase {
            Some(TouchPhase::Ended) => {
                shelf.lifted = None;
                drop(shelves);
                self.settle_shelf(id, window);
            }
            Some(_) => shelf.lifted = None,
            None if gesture_end => {
                shelf.lifted = Some(Instant::now());
                drop(shelves);
                // Settle only if no momentum follows the lift.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(MOMENTUM_GRACE).await;
                    this.update(cx, |this, cx| {
                        let waiting = this.shelves.borrow().get(id).and_then(|s| s.lifted).is_some();
                        if waiting {
                            if let Some(shelf) = this.shelves.borrow_mut().get_mut(id) {
                                shelf.lifted = None;
                            }
                            if let Some(window) = cx.windows().first().copied() {
                                window.update(cx, |_, window, _| this.settle_shelf(id, window)).ok();
                            }
                        }
                    })
                    .ok();
                })
                .detach();
            }
            None => {
                shelf.snap = None;
                shelf.lifted = None;
            }
        }
    }

    /// Starts moving a shelf to the tile nearest its leading edge.
    fn settle_shelf(&self, id: &'static str, window: &mut Window) {
        let mut shelves = self.shelves.borrow_mut();
        let Some(shelf) = shelves.get_mut(id) else { return };
        let handle = &shelf.handle;
        let viewport = handle.bounds();
        let max = f32::from(handle.max_offset().width);
        if max <= 0. {
            return;
        }
        let current = f32::from(handle.offset().x);
        let edge = viewport.origin.x + px(PAD);
        let nearest = (0..handle.children_count())
            .filter_map(|ix| handle.bounds_for_item(ix))
            .map(|b| f32::from(b.origin.x - edge))
            .min_by(|a, b| a.abs().total_cmp(&b.abs()));
        let Some(distance) = nearest else { return };
        let target = (current - distance).clamp(-max, 0.);
        if (target - current).abs() < 0.5 {
            return;
        }
        shelf.snap = Some((current, target, Instant::now()));
        window.refresh();
    }

    /// Moves shelves that are settling; call each frame from the page's render.
    pub(crate) fn step_shelf_snaps(&self, window: &mut Window) {
        let mut moving = false;
        for shelf in self.shelves.borrow_mut().values_mut() {
            let Some((from, to, start)) = shelf.snap else { continue };
            let t = (start.elapsed().as_secs_f32() / SNAP.as_secs_f32()).min(1.);
            let eased = 1. - (1. - t).powi(3);
            shelf.handle.set_offset(point(px(from + (to - from) * eased), shelf.handle.offset().y));
            if t < 1. {
                moving = true;
            } else {
                shelf.snap = None;
            }
        }
        if moving {
            window.request_animation_frame();
        }
    }

    /// Whether `position` is over a shelf, whose sideways swipes scroll it rather than the page.
    fn over_shelf(&self, position: Point<Pixels>) -> bool {
        self.shelves.borrow().values().any(|s| s.handle.bounds().contains(&position))
    }

    /// Two-finger swipes sideways go back and forward, like Safari's: the page follows the
    /// fingers, and letting go past the threshold navigates.
    pub(crate) fn page_swiped(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if !horizontal_gesture(e) || e.momentum_phase.is_some() {
            return;
        }
        match e.touch_phase {
            TouchPhase::Started => {
                self.nav_swipe = if self.over_shelf(e.position) { None } else { Some(Swipe::default()) };
            }
            TouchPhase::Moved => {
                let Some(swipe) = self.nav_swipe.as_mut() else { return };
                let dx = f32::from(e.delta.pixel_delta(px(20.)).x);
                // Only toward a page there is: back with history, forward with a forward stack.
                let (can_back, can_forward) = (!self.history.is_empty(), !self.forward_history.is_empty());
                let next = swipe.offset + dx;
                let allowed = (next > 0. && can_back) || (next < 0. && can_forward);
                if !allowed {
                    swipe.offset = 0.;
                    return;
                }
                if swipe.add(dx, PAGE_SWIPE) {
                    perform(Haptic::Alignment);
                }
                cx.notify();
            }
            // Finished by `swipe_ended`, wherever the fingers lift.
            TouchPhase::Ended => {}
        }
        self.watch_swipe(cx);
    }

    /// How far the page is pulled by a swipe in progress, with resistance.
    pub(crate) fn page_swipe_offset(&self) -> f32 {
        self.nav_swipe.map_or(0., |s| s.offset * 0.5)
    }

    /// Swiping the song in the player sideways skips to the next or previous one.
    pub(crate) fn player_swiped(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if !horizontal_gesture(e) || e.momentum_phase.is_some() || self.queue.current().is_none() {
            return;
        }
        match e.touch_phase {
            TouchPhase::Started => self.player_swipe = Swipe::default(),
            TouchPhase::Moved => {
                let dx = f32::from(e.delta.pixel_delta(px(20.)).x);
                if self.player_swipe.add(dx, SKIP_SWIPE) {
                    perform(Haptic::Alignment);
                }
                cx.notify();
            }
            TouchPhase::Ended => {}
        }
        self.watch_swipe(cx);
    }

    /// Ends any swipe in progress when its gesture ends, whatever is under the pointer by
    /// then; with `act`, a swipe past its threshold navigates or skips.
    pub(crate) fn finish_swipes(&mut self, act: bool, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(swipe) = self.nav_swipe.take() {
            changed = true;
            if act && swipe.offset >= PAGE_SWIPE {
                self.back(cx);
            } else if act && swipe.offset <= -PAGE_SWIPE {
                self.forward(cx);
            }
        }
        let offset = std::mem::take(&mut self.player_swipe).offset;
        if offset != 0. {
            changed = true;
            if act && offset <= -SKIP_SWIPE {
                self.next(cx);
            } else if act && offset >= SKIP_SWIPE {
                self.previous(cx);
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// Called for every scroll event in the window: the end of a gesture, or the start of its
    /// momentum, finishes a swipe.
    pub(crate) fn swipe_ended(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let ended = e.momentum_phase.is_some() || matches!(e.touch_phase, TouchPhase::Ended);
        if ended && (self.nav_swipe.is_some() || self.player_swipe.offset != 0.) {
            self.finish_swipes(true, cx);
        }
    }

    /// A swipe whose gesture stops sending events without ending springs back.
    fn watch_swipe(&mut self, cx: &mut Context<Self>) {
        self.swipe_seen = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SWIPE_TIMEOUT).await;
            this.update(cx, |this, cx| {
                if this.swipe_seen.is_some_and(|at| at.elapsed() >= SWIPE_TIMEOUT) {
                    this.swipe_seen = None;
                    this.finish_swipes(false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Scrolling over the volume control changes the volume, with a tap at silent and full:
    /// sideways like the slider (right is louder), or up for louder.
    pub(crate) fn volume_scrolled(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if e.momentum_phase.is_some() {
            return;
        }
        let delta = e.delta.pixel_delta(px(20.));
        let change = match gesture_axis(e) {
            Some(Axis::Horizontal) => f32::from(delta.x),
            Some(Axis::Vertical) => -f32::from(delta.y),
            // Wheels have no gesture: whichever way it turned.
            None => f32::from(delta.x) - f32::from(delta.y),
        };
        let before = self.volume;
        let after = (before + change / 300.).clamp(0., 1.);
        if (after - before).abs() < f32::EPSILON {
            return;
        }
        if (after == 0. || after == 1.) && before != after {
            perform(Haptic::LevelChange);
        }
        self.change_volume(after, cx);
    }
}
