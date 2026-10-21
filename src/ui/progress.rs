//! The player's progress bar as its own view, which decides when playback needs a redraw.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    Bounds, Context, MouseButton, MouseDownEvent, Pixels, SharedString, WeakEntity, Window, div, prelude::*, px,
    relative,
};

use crate::{
    app::{Drag, MusicApp},
    audio::Audio,
    ui::capture_bounds,
};

pub type SharedAudio = Rc<RefCell<Option<Audio>>>;

/// How long the bar takes to thicken (or thin) when the song area is hovered.
const HOVER_ANIM: Duration = Duration::from_millis(170);

/// Hover progress (0 = resting, 1 = hovered), eased from wherever it was when the
/// hover changed so quick in-and-out movements don't jump.
#[derive(Clone, Copy)]
struct HoverAnim {
    hovered: bool,
    from: f32,
    at: Instant,
}

impl HoverAnim {
    fn amount(&self) -> (f32, bool) {
        let t = (self.at.elapsed().as_secs_f32() / HOVER_ANIM.as_secs_f32()).min(1.);
        let eased = 1. - (1. - t).powi(3);
        let target = if self.hovered { 1. } else { 0. };
        (self.from + (target - self.from) * eased, t < 1.)
    }
}

/// How often to check whether the bar has visibly moved.
const FRAME: Duration = Duration::from_millis(33);

pub struct Progress {
    audio: SharedAudio,
    /// Track, fill and label colors, set by the player to match its tinted background.
    colors: Cell<(gpui::Hsla, gpui::Hsla, gpui::Hsla)>,
    /// Where the user is dragging to (0–1); shown instead of the playback position.
    scrub: Cell<Option<f32>>,
    /// Position and length shown before anything has loaded: a restored session's song.
    resume: Cell<Option<(f32, f32)>>,
    /// "Artist — Album" line, which the times replace while hovering.
    subtitle: RefCell<SharedString>,
    hover: Cell<HoverAnim>,
    /// Space kept clear left and right of the subtitle line, for the art and menu beside it.
    insets: Cell<(f32, f32)>,
    /// Where the native fill layer expects the song to be, while it's drawing the fill
    /// instead of this view (see `native_fill`).
    native: Cell<Option<(f32, Instant)>>,
    /// Where the pointer is along the bar (0–1) while hovering it, previewing a click.
    pointer: Cell<Option<f32>>,
    /// The current song's spectrum (set by the app once analyzed).
    pub spectrum: RefCell<Option<std::sync::Arc<crate::spectrum::Spectrum>>>,
    shown_levels: RefCell<[f32; crate::spectrum::BANDS]>,
    live: RefCell<crate::spectrum::Live>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Fill length last drawn, in half points.
    drawn_step: Cell<i64>,
    app: WeakEntity<MusicApp>,
    ticking: bool,
}

impl Progress {
    pub fn new(audio: SharedAudio, bounds: Rc<Cell<Bounds<Pixels>>>, app: WeakEntity<MusicApp>) -> Self {
        let white = |a: f32| gpui::Hsla { h: 0., s: 0., l: 1., a };
        Self {
            audio,
            colors: Cell::new((white(0.2), white(0.9), white(0.6))),
            scrub: Cell::new(None),
            resume: Cell::new(None),
            subtitle: RefCell::default(),
            hover: Cell::new(HoverAnim { hovered: false, from: 0., at: Instant::now() }),
            insets: Cell::new((0., 0.)),
            native: Cell::new(None),
            pointer: Cell::new(None),
            spectrum: RefCell::new(None),
            shown_levels: RefCell::new([0.; crate::spectrum::BANDS]),
            live: RefCell::new(crate::spectrum::Live::new()),
            bounds,
            drawn_step: Cell::new(-1),
            app,
            ticking: false,
        }
    }

    /// Watches the bar while a song plays; stops by itself when playback does, so a paused
    /// app has no timer waking it. The app's playback tick restarts it.
    pub fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !self.playing() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        if this.needs_redraw() {
                            cx.notify();
                        }
                        // The native fill runs on its own clock; a seek or a stall puts it
                        // out of step, and the player then re-times it.
                        if this.native_drifted() {
                            this.app.update(cx, |_, cx| cx.notify()).ok();
                        }
                        this.ticking = this.playing();
                        this.ticking
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn playing(&self) -> bool {
        self.audio.borrow().as_ref().is_some_and(Audio::is_playing)
    }

    /// A notify here re-renders the whole window (GPUI marks every ancestor view dirty), so
    /// playback only redraws when the fill moves by half a point: a few times a second.
    fn needs_redraw(&self) -> bool {
        let audio = self.audio.borrow();
        let Some(audio) = audio.as_ref().filter(|a| a.is_playing()) else {
            return false;
        };
        if self.native.get().is_some() {
            return false;
        }
        let duration = audio.duration().unwrap_or(30.).max(1.);
        let width = f32::from(self.bounds.get().size.width);
        let step = (audio.position() / duration * width * 2.) as i64;
        self.drawn_step.replace(step) != step
    }

    fn native_drifted(&self) -> bool {
        let Some((position, at)) = self.native.get() else { return false };
        let audio = self.audio.borrow();
        audio.as_ref().is_some_and(|a| (a.position() - (position + at.elapsed().as_secs_f32())).abs() > 0.75)
    }

    pub fn set_native(&self, native: Option<(f32, Instant)>) {
        self.native.set(native);
    }

    pub fn fill_color(&self) -> gpui::Hsla {
        self.colors.get().1
    }

    pub fn set_resume(&self, resume: Option<(f32, f32)>) {
        self.resume.set(resume);
    }

    pub fn set_scrub(&self, fraction: Option<f32>) {
        self.scrub.set(fraction);
    }

    pub fn scrub(&self) -> Option<f32> {
        self.scrub.get()
    }

    /// How far the hover has come (0 resting, 1 hovered), and whether it's still moving.
    pub fn hover_amount(&self) -> (f32, bool) {
        self.hover.get().amount()
    }

    pub fn set_insets(&self, left: f32, right: f32) {
        self.insets.set((left, right));
    }

    pub fn set_subtitle(&self, text: SharedString) {
        *self.subtitle.borrow_mut() = text;
    }

    /// Returns true if the hover state changed (and the view needs a redraw).
    pub fn set_hovered(&self, hovered: bool) -> bool {
        let anim = self.hover.get();
        if anim.hovered == hovered {
            return false;
        }
        self.hover.set(HoverAnim { hovered, from: anim.amount().0, at: Instant::now() });
        true
    }

    /// Level bars from the song's own audio: the preview clip's frequency analysis, in sync
    /// for previews and looped for full songs (whose DRM-protected audio can't be read).
    fn equalizer(&self, color: gpui::Hsla, playing: bool, position: f32) -> Vec<gpui::Div> {
        use crate::spectrum::BANDS;
        let live = match self.audio.borrow().as_ref() {
            Some(Audio::Full(crate::audio::FullPlayer::Native(p))) if playing => {
                self.live.borrow_mut().bands(|buf| p.samples(buf))
            }
            _ => None,
        };
        let levels = match (live, &*self.spectrum.borrow(), playing) {
            (Some(levels), _, _) => levels,
            (None, Some(s), true) => s.at(position),
            _ => [0.; BANDS],
        };
        // Ease toward the target each frame so pausing settles instead of snapping.
        let mut shown = self.shown_levels.borrow_mut();
        (0..BANDS)
            .map(|i| {
                shown[i] += (levels[i] - shown[i]) * 0.35;
                let level = shown[i].clamp(0.06, 1.);
                // Bars share the width, so the visualizer spans the space between the times.
                div()
                    .flex_1()
                    .min_w(px(1.))
                    .h(px(2. + 11. * level))
                    .rounded_full()
                    .bg(color.opacity(0.35 + 0.55 * level))
            })
            .collect()
    }

    pub fn set_colors(&self, track: gpui::Hsla, fill: gpui::Hsla, label: gpui::Hsla) {
        self.colors.set((track, fill, label));
    }
}

impl Render for Progress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, animating) = self.hover.get().amount();
        let playing = self.audio.borrow().as_ref().is_some_and(Audio::is_playing);
        // Hovered, the times and visualizer show and update at the display's refresh rate.
        if animating || (playing && self.hover.get().hovered) {
            window.request_animation_frame();
        }
        let (position, duration) = self
            .audio
            .borrow()
            .as_ref()
            .map_or(self.resume.get().unwrap_or((0., 30.)), |a| (a.position(), a.duration().unwrap_or(30.)));
        let (position, fraction) = match self.scrub.get() {
            Some(f) => (f * duration, f),
            None => {
                let position = position.min(duration);
                (position, if duration > 0. { position / duration } else { 0. })
            }
        };
        let (track, fill, label) = self.colors.get();
        let app = self.app.clone();
        let time = |s: String| div().text_size(px(10.5)).text_color(label).child(s);
        let thickness = 2.5 + 5.5 * hover;
        let pointer = self.pointer.get().filter(|_| hover > 0. && self.scrub.get().is_none());
        // Hovered, the art and menu beside the subtitle fade out and the times spread to the
        // bar's ends.
        let (inset_left, inset_right) = self.insets.get();
        let (inset_left, inset_right) = (inset_left * (1. - hover), inset_right * (1. - hover));

        div()
            .flex()
            .flex_col()
            // Subtitle and times share a line, cross-fading with the hover.
            .child(
                div()
                    .relative()
                    .ml(px(inset_left))
                    .mr(px(inset_right))
                    .h(px(15.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(15.))
                            .text_color(label)
                            .opacity(1. - hover)
                            .child(self.subtitle.borrow().clone()),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .opacity(hover)
                            .child(time(format_secs(position)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .items_end()
                                    .overflow_hidden()
                                    .gap(px(1.5))
                                    .when(hover > 0., |d| d.children(self.equalizer(fill, playing, position))),
                            )
                            .child(time(format!("-{}", format_secs(duration - position)))),
                    ),
            )
            .child(
                div()
                    .id("progress")
                    .relative()
                    .w_full()
                    .h(px(12.))
                    // The player fades its art, title and menu with the hover, so it redraws too.
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.pointer.set(None);
                        }
                        if this.set_hovered(*hovered) {
                            cx.notify();
                            this.app.update(cx, |_, cx| cx.notify()).ok();
                        }
                    }))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        app.update(cx, |app, cx| {
                            app.drag = Some(Drag::Progress);
                            app.drag_to(Drag::Progress, e.position, cx);
                        })
                        .ok();
                    })
                    .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                        let b = this.bounds.get();
                        if b.size.width <= px(0.) {
                            return;
                        }
                        let at = ((e.position.x - b.origin.x) / b.size.width).clamp(0., 1.);
                        if this.pointer.replace(Some(at)).is_none_or(|was| (was - at).abs() > 0.001) {
                            cx.notify();
                        }
                    }))
                    .child(capture_bounds(self.bounds.clone()))
                    .child({
                        // A click jumps to the pointer: ahead of the playhead, the stretch it
                        // would skip shows lighter; behind, the part it would undo dims.
                        let (solid, preview) = match pointer {
                            Some(at) if at > fraction => (fraction, Some((fraction, at - fraction, 0.45))),
                            Some(at) => (at, Some((at, fraction - at, 0.5))),
                            None => (fraction, None),
                        };
                        div()
                            .relative()
                            .w_full()
                            .h(px(thickness))
                            .rounded_full()
                            .bg(track)
                            .overflow_hidden()
                            .when_some(preview, |d, (start, width, alpha)| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left(relative(start))
                                        .w(relative(width))
                                        .bg(fill.opacity(alpha)),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left_0()
                                    .w(relative(solid))
                                    // Against the preview, a rounded end would leave a notch.
                                    .when(preview.is_none(), |d| d.rounded_full())
                                    .bg(fill),
                            )
                    }),
            )
    }
}

fn format_secs(secs: f32) -> String {
    let s = secs.max(0.) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}
