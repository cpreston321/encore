//! Overlay scrollbars, like macOS with "Show scroll bars: When scrolling": a thin thumb over
//! the content's edge that shows while scrolling, fades out a second after, widens under the
//! mouse, and can be dragged.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::{
    App, Axis, BorderStyle, Bounds, CursorStyle, DispatchPhase, Edges, Hitbox, HitboxBehavior,
    Hsla, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Overflow,
    Pixels, Point, ScrollPhysics, Size, Task, Window, WindowAppearance, hsla, point, px, quad,
};

const HOLD_SECS: f32 = 1.0;
const FADE_SECS: f32 = 0.25;
const EXPAND_SECS: f32 = 0.12;
const THICKNESS: f32 = 6.;
const EXPANDED_THICKNESS: f32 = 10.;
/// Between the thumb and the view's edge.
const EDGE_GAP: f32 = 2.;
/// Between the ends of the track and the view's ends.
const END_GAP: f32 = 3.;
/// The strip along the edge that the mouse hovers and clicks.
const HIT_THICKNESS: f32 = 16.;
const MIN_THUMB: f32 = 28.;

/// Colors and placement for an overlay scrollbar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayScrollbar {
    /// The thumb's color. By default, translucent black in light windows and white in dark ones.
    pub color: Option<Hsla>,
    /// Keeps the bar clear of content floating over the view's edges, such as a toolbar.
    pub insets: Edges<Pixels>,
}

impl OverlayScrollbar {
    fn thumb_color(&self, window: &Window) -> Hsla {
        self.color.unwrap_or_else(|| match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => hsla(0., 0., 1., 0.5),
            WindowAppearance::Light | WindowAppearance::VibrantLight => hsla(0., 0., 0., 0.45),
        })
    }
}

/// Where the thumb sits along a track `track_len` long, as `(start, length)` from the track's
/// start. `offset` runs from 0 to `-max`; overscroll shrinks the thumb against its end.
pub(crate) fn thumb_span(
    track_len: f32,
    viewport: f32,
    max: f32,
    offset: f32,
    overscroll: f32,
) -> (f32, f32) {
    if track_len <= 0. || viewport <= 0. {
        return (0., 0.);
    }
    let full = (track_len * viewport / (viewport + max.max(0.)))
        .max(MIN_THUMB)
        .min(track_len);
    let len = (full - overscroll.abs()).max(full.min(MIN_THUMB) / 2.);
    let progress = if max > 0. {
        (-offset / max).clamp(0., 1.)
    } else {
        0.
    };
    (progress * (track_len - len), len)
}

/// The scroll offset, from 0 to `-max`, that puts the thumb's start `thumb_start` along the
/// track.
pub(crate) fn offset_for_thumb(thumb_start: f32, track_len: f32, thumb_len: f32, max: f32) -> f32 {
    let room = track_len - thumb_len;
    if room <= 0. {
        return 0.;
    }
    -(thumb_start / room).clamp(0., 1.) * max
}

#[derive(Clone, Copy, Debug)]
struct Track {
    axis: Axis,
    /// The hover and click area.
    hit: Bounds<Pixels>,
    start: f32,
    len: f32,
    /// Where the thumb starts, relative to `start`.
    thumb_start: f32,
    thumb_len: f32,
    max: f32,
}

impl Track {
    fn along(&self, position: Point<Pixels>) -> f32 {
        match self.axis {
            Axis::Vertical => position.y.0,
            Axis::Horizontal => position.x.0,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ScrollbarState {
    last_activity: Option<Instant>,
    hovered: Option<Axis>,
    /// The axis being dragged and where on the thumb it was grabbed.
    drag: Option<(Axis, f32)>,
    expand: f32,
    expand_tick: Option<Instant>,
    wake: Option<(Instant, Task<()>)>,
    tracks: Vec<Track>,
    hitboxes: Vec<Hitbox>,
}

impl ScrollbarState {
    /// Shows the scrollbar, as the user just scrolled.
    pub(crate) fn mark_activity(&mut self, now: Instant) {
        self.last_activity = Some(now);
    }

    fn opacity(&self, now: Instant) -> f32 {
        if self.drag.is_some() || self.hovered.is_some() {
            return 1.;
        }
        let Some(last) = self.last_activity else {
            return 0.;
        };
        let idle = now.saturating_duration_since(last).as_secs_f32();
        if idle < HOLD_SECS {
            1.
        } else {
            (1. - (idle - HOLD_SECS) / FADE_SECS).max(0.)
        }
    }

    fn track(&self, axis: Axis) -> Option<Track> {
        self.tracks.iter().find(|track| track.axis == axis).copied()
    }

    /// Lays out the tracks for this frame and, while the bar is showing, inserts hitboxes over
    /// them so clicks there don't reach the content.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepaint(
        &mut self,
        style: &OverlayScrollbar,
        bounds: Bounds<Pixels>,
        overflow: Point<Overflow>,
        offset: Point<Pixels>,
        overscroll: Point<f32>,
        max: Size<Pixels>,
        window: &mut Window,
    ) {
        self.tracks.clear();
        self.hitboxes.clear();
        let vertical = overflow.y == Overflow::Scroll && max.height > px(0.);
        let horizontal = overflow.x == Overflow::Scroll && max.width > px(0.);
        let corner = if vertical && horizontal {
            HIT_THICKNESS
        } else {
            0.
        };
        let insets = style.insets;
        let left = (bounds.left() + insets.left).0;
        let right = (bounds.right() - insets.right).0;
        let top = (bounds.top() + insets.top).0;
        let bottom = (bounds.bottom() - insets.bottom).0;

        if vertical {
            let start = top + END_GAP;
            let len = bottom - END_GAP - corner - start;
            if len > MIN_THUMB {
                let (thumb_start, thumb_len) = thumb_span(
                    len,
                    bounds.size.height.0,
                    max.height.0,
                    offset.y.0,
                    overscroll.y,
                );
                self.tracks.push(Track {
                    axis: Axis::Vertical,
                    hit: Bounds::from_corners(
                        point(px(right - HIT_THICKNESS), px(start)),
                        point(px(right), px(start + len)),
                    ),
                    start,
                    len,
                    thumb_start,
                    thumb_len,
                    max: max.height.0,
                });
            }
        }
        if horizontal {
            let start = left + END_GAP;
            let len = right - END_GAP - corner - start;
            if len > MIN_THUMB {
                let (thumb_start, thumb_len) = thumb_span(
                    len,
                    bounds.size.width.0,
                    max.width.0,
                    offset.x.0,
                    overscroll.x,
                );
                self.tracks.push(Track {
                    axis: Axis::Horizontal,
                    hit: Bounds::from_corners(
                        point(px(start), px(bottom - HIT_THICKNESS)),
                        point(px(start + len), px(bottom)),
                    ),
                    start,
                    len,
                    thumb_start,
                    thumb_len,
                    max: max.width.0,
                });
            }
        }

        if self.opacity(Instant::now()) > 0. {
            for track in &self.tracks {
                self.hitboxes
                    .push(window.insert_hitbox(track.hit, HitboxBehavior::BlockMouseExceptScroll));
            }
        }
    }

    fn paint_tracks(&self, style: &OverlayScrollbar, opacity: f32, window: &mut Window) {
        let thickness = THICKNESS + (EXPANDED_THICKNESS - THICKNESS) * self.expand;
        let color = style.thumb_color(window);
        let thumb = Hsla {
            a: (color.a * (1. + 0.2 * self.expand)).min(1.) * opacity,
            ..color
        };
        let border = if color.l < 0.5 {
            hsla(0., 0., 1., 0.25 * opacity)
        } else {
            hsla(0., 0., 0., 0.15 * opacity)
        };
        let groove = Hsla {
            a: color.a * 0.2 * self.expand * opacity,
            ..color
        };
        for track in &self.tracks {
            let cross_end = match track.axis {
                Axis::Vertical => track.hit.right().0,
                Axis::Horizontal => track.hit.bottom().0,
            } - EDGE_GAP;
            let span = |start: f32, len: f32, grow: f32| {
                let cross = cross_end - thickness - grow;
                let cross_len = thickness + 2. * grow;
                match track.axis {
                    Axis::Vertical => Bounds::new(
                        point(px(cross), px(start - grow)),
                        crate::size(px(cross_len), px(len + 2. * grow)),
                    ),
                    Axis::Horizontal => Bounds::new(
                        point(px(start - grow), px(cross)),
                        crate::size(px(len + 2. * grow), px(cross_len)),
                    ),
                }
            };
            if groove.a > 0. {
                window.paint_quad(quad(
                    span(track.start, track.len, 1.),
                    px(thickness / 2. + 1.),
                    groove,
                    Edges::default(),
                    Hsla::transparent_black(),
                    BorderStyle::Solid,
                ));
            }
            window.paint_quad(quad(
                span(track.start + track.thumb_start, track.thumb_len, 0.),
                px(thickness / 2.),
                thumb,
                px(0.5),
                border,
                BorderStyle::Solid,
            ));
        }
    }
}

/// Draws the scrollbars of a scroll view and handles hovering and dragging them. Call after
/// painting the view's children so the bars draw and receive the mouse first.
pub(crate) fn paint_overlay_scrollbars(
    physics: &Rc<RefCell<ScrollPhysics>>,
    scroll_offset: &Rc<RefCell<Point<Pixels>>>,
    style: &OverlayScrollbar,
    container: &Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    let now = Instant::now();
    let view = window.current_view();
    {
        let mut physics = physics.borrow_mut();
        let state = &mut physics.scrollbar;
        if state.tracks.is_empty() {
            state.drag = None;
            state.hovered = None;
            return;
        }

        let target = if state.drag.is_some() || state.hovered.is_some() {
            1.
        } else {
            0.
        };
        if state.expand == target {
            state.expand_tick = None;
        } else {
            let dt = state
                .expand_tick
                .map_or(0., |last| now.saturating_duration_since(last).as_secs_f32());
            let step = dt / EXPAND_SECS;
            state.expand = if target > state.expand {
                (state.expand + step).min(target)
            } else {
                (state.expand - step).max(target)
            };
            state.expand_tick = Some(now);
            window.request_animation_frame();
        }

        let opacity = state.opacity(now);
        if opacity > 0. {
            state.paint_tracks(style, opacity, window);
            for hitbox in &state.hitboxes {
                window.set_cursor_style(CursorStyle::Arrow, hitbox);
            }
        }

        if state.drag.is_none()
            && state.hovered.is_none()
            && let Some(last) = state.last_activity
        {
            let idle = now.saturating_duration_since(last).as_secs_f32();
            let fade_at = last + Duration::from_secs_f32(HOLD_SECS);
            if idle < HOLD_SECS {
                if fade_at.saturating_duration_since(now) < Duration::from_millis(20) {
                    window.request_animation_frame();
                } else if state
                    .wake
                    .as_ref()
                    .is_none_or(|(at, _)| *at <= now + Duration::from_millis(20))
                {
                    // A wake set for an earlier fade repaints in time to schedule this one.
                    let task = window.spawn(cx, async move |cx| {
                        cx.background_executor()
                            .timer(fade_at.saturating_duration_since(Instant::now()))
                            .await;
                        cx.update(|_, cx| cx.notify(view)).ok();
                    });
                    state.wake = Some((fade_at, task));
                }
            } else if idle < HOLD_SECS + FADE_SECS {
                window.request_animation_frame();
            } else {
                state.wake = None;
            }
        }
    }

    window.on_mouse_event({
        let physics = physics.clone();
        let scroll_offset = scroll_offset.clone();
        let container = container.clone();
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            let mut physics = physics.borrow_mut();
            let now = Instant::now();
            if let Some((axis, grab)) = physics.scrollbar.drag {
                let Some(track) = physics.scrollbar.track(axis) else {
                    return;
                };
                let thumb_start = track.along(event.position) - grab - track.start;
                let offset = px(offset_for_thumb(
                    thumb_start,
                    track.len,
                    track.thumb_len,
                    track.max,
                ));
                let mut scroll_offset = scroll_offset.borrow_mut();
                match axis {
                    Axis::Vertical => {
                        scroll_offset.y = offset;
                        physics.elastic.y.reset();
                    }
                    Axis::Horizontal => {
                        scroll_offset.x = offset;
                        physics.elastic.x.reset();
                    }
                }
                physics.scrollbar.mark_activity(now);
                cx.notify(view);
                return;
            }
            let hovered = if container.should_handle_scroll(window) {
                physics
                    .scrollbar
                    .tracks
                    .iter()
                    .find(|track| track.hit.contains(&event.position))
                    .map(|track| track.axis)
            } else {
                None
            };
            if hovered != physics.scrollbar.hovered {
                physics.scrollbar.hovered = hovered;
                physics.scrollbar.mark_activity(now);
                cx.notify(view);
            }
        }
    });

    window.on_mouse_event({
        let physics = physics.clone();
        move |_: &MouseExitEvent, phase, _window, cx| {
            let mut physics = physics.borrow_mut();
            if phase == DispatchPhase::Capture
                && physics.scrollbar.drag.is_none()
                && physics.scrollbar.hovered.take().is_some()
            {
                physics.scrollbar.mark_activity(Instant::now());
                cx.notify(view);
            }
        }
    });

    window.on_mouse_event({
        let physics = physics.clone();
        let scroll_offset = scroll_offset.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let mut physics = physics.borrow_mut();
            let Some(track) = physics
                .scrollbar
                .hitboxes
                .iter()
                .zip(physics.scrollbar.tracks.iter())
                .find(|(hitbox, _)| hitbox.is_hovered(window))
                .map(|(_, track)| *track)
            else {
                return;
            };
            let along = track.along(event.position) - track.start;
            let on_thumb =
                (track.thumb_start..=track.thumb_start + track.thumb_len).contains(&along);
            let grab = if on_thumb {
                along - track.thumb_start
            } else {
                track.thumb_len / 2.
            };
            if !on_thumb {
                let offset = px(offset_for_thumb(
                    along - grab,
                    track.len,
                    track.thumb_len,
                    track.max,
                ));
                let mut scroll_offset = scroll_offset.borrow_mut();
                match track.axis {
                    Axis::Vertical => {
                        scroll_offset.y = offset;
                        physics.elastic.y.reset();
                    }
                    Axis::Horizontal => {
                        scroll_offset.x = offset;
                        physics.elastic.x.reset();
                    }
                }
            }
            physics.scrollbar.drag = Some((track.axis, grab));
            physics.scrollbar.mark_activity(Instant::now());
            window.prevent_default();
            cx.stop_propagation();
            cx.notify(view);
        }
    });

    window.on_mouse_event({
        let physics = physics.clone();
        move |event: &MouseUpEvent, phase, _window, cx| {
            if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                return;
            }
            let mut physics = physics.borrow_mut();
            if physics.scrollbar.drag.take().is_some() {
                physics.scrollbar.mark_activity(Instant::now());
                cx.notify(view);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_spans_the_visible_fraction() {
        let (start, len) = thumb_span(400., 500., 1500., 0., 0.);
        assert_eq!(start, 0.);
        assert!((len - 100.).abs() < 1e-3);
        let (start, len) = thumb_span(400., 500., 1500., -1500., 0.);
        assert!((start + len - 400.).abs() < 1e-3);
        let (start, _) = thumb_span(400., 500., 1500., -750., 0.);
        assert!((start - 150.).abs() < 1e-3);
    }

    #[test]
    fn thumb_has_a_minimum_length() {
        let (_, len) = thumb_span(400., 500., 1_000_000., -10., 0.);
        assert_eq!(len, MIN_THUMB);
    }

    #[test]
    fn overscroll_shrinks_the_thumb_against_its_end() {
        let (start, len) = thumb_span(400., 500., 1500., 0., 40.);
        assert_eq!(start, 0.);
        assert!((len - 60.).abs() < 1e-3);
        let (start, len) = thumb_span(400., 500., 1500., -1500., -40.);
        assert!((start + len - 400.).abs() < 1e-3);
        let (_, len) = thumb_span(400., 500., 1500., 0., 5000.);
        assert_eq!(len, MIN_THUMB / 2.);
    }

    #[test]
    fn dragging_the_thumb_maps_back_to_the_offset() {
        for &offset in &[0., -200., -750., -1500.] {
            let (start, len) = thumb_span(400., 500., 1500., offset, 0.);
            let back = offset_for_thumb(start, 400., len, 1500.);
            assert!((back - offset).abs() < 1e-2, "{offset} -> {back}");
        }
        assert_eq!(offset_for_thumb(-50., 400., 100., 1500.), 0.);
        assert_eq!(offset_for_thumb(900., 400., 100., 1500.), -1500.);
    }

    #[test]
    fn fades_a_second_after_scrolling() {
        let now = Instant::now();
        let mut state = ScrollbarState::default();
        assert_eq!(state.opacity(now), 0.);
        state.mark_activity(now);
        assert_eq!(state.opacity(now + Duration::from_millis(900)), 1.);
        let fading = state.opacity(now + Duration::from_millis(1100));
        assert!(fading > 0. && fading < 1.);
        assert_eq!(state.opacity(now + Duration::from_millis(1300)), 0.);
        state.hovered = Some(Axis::Vertical);
        assert_eq!(state.opacity(now + Duration::from_secs(10)), 1.);
    }
}
