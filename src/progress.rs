//! The player's progress bar as its own view, so it can refresh smoothly while a song
//! plays without redrawing the rest of the app.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    Bounds, Context, MouseButton, MouseDownEvent, Pixels, SharedString, Task, WeakEntity, Window, div,
    prelude::*, px, relative,
};

use crate::{
    app::{Drag, MusicApp},
    audio::Audio,
    views::capture_bounds,
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

/// ~30fps keeps the bar visibly smooth; the 30s previews make each pixel ~50ms.
const FRAME: Duration = Duration::from_millis(33);

pub struct Progress {
    audio: SharedAudio,
    /// Track, fill and label colors, set by the player to match its tinted background.
    colors: Cell<(gpui::Hsla, gpui::Hsla, gpui::Hsla)>,
    /// Where the user is dragging to (0–1); shown instead of the playback position.
    scrub: Cell<Option<f32>>,
    /// "Artist — Album" line, which the times replace while hovering.
    subtitle: RefCell<SharedString>,
    hover: Cell<HoverAnim>,
    /// The current song's spectrum (set by the app once analyzed).
    pub spectrum: RefCell<Option<std::sync::Arc<crate::spectrum::Spectrum>>>,
    shown_levels: RefCell<[f32; crate::spectrum::BANDS]>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    app: WeakEntity<MusicApp>,
    _ticker: Task<()>,
}

impl Progress {
    pub fn new(audio: SharedAudio, bounds: Rc<Cell<Bounds<Pixels>>>, app: WeakEntity<MusicApp>, cx: &mut Context<Self>) -> Self {
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let alive = this.update(cx, |this, cx| {
                    if this.audio.borrow().as_ref().is_some_and(Audio::is_playing) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let white = |a: f32| gpui::Hsla { h: 0., s: 0., l: 1., a };
        Self {
            audio,
            colors: Cell::new((white(0.2), white(0.9), white(0.6))),
            scrub: Cell::new(None),
            subtitle: RefCell::default(),
            hover: Cell::new(HoverAnim { hovered: false, from: 0., at: Instant::now() }),
            spectrum: RefCell::new(None),
            shown_levels: RefCell::new([0.; crate::spectrum::BANDS]),
            bounds,
            app,
            _ticker: ticker,
        }
    }

    pub fn set_scrub(&self, fraction: Option<f32>) {
        self.scrub.set(fraction);
    }

    pub fn scrub(&self) -> Option<f32> {
        self.scrub.get()
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

    /// Animated level bars. DRM-protected streams can't be analysed, so this is a
    /// procedural visualizer: layered waves per bar, settling flat while paused.
    /// Level bars from the song's own audio: the preview clip's frequency analysis, in sync
    /// for previews and looped for full songs (whose DRM-protected audio can't be read).
    fn equalizer(&self, color: gpui::Hsla, playing: bool, position: f32) -> Vec<gpui::Div> {
        use crate::spectrum::BANDS;
        let levels = match (&*self.spectrum.borrow(), playing) {
            (Some(s), true) => s.at(position),
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
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, animating) = self.hover.get().amount();
        if animating {
            window.request_animation_frame();
        }
        let (position, duration) = self
            .audio
            .borrow()
            .as_ref()
            .map_or((0., 30.), |a| (a.position(), a.duration().unwrap_or(30.)));
        let (position, fraction) = match self.scrub.get() {
            Some(f) => (f * duration, f),
            None => {
                let position = position.min(duration);
                (position, if duration > 0. { position / duration } else { 0. })
            }
        };
        let (track, fill, label) = self.colors.get();
        let playing = self.audio.borrow().as_ref().is_some_and(Audio::is_playing);
        let app = self.app.clone();
        let time = |s: String| div().text_size(px(10.5)).text_color(label).child(s);
        let thickness = 2.5 + 3. * hover;

        div()
            .flex()
            .flex_col()
            // Subtitle and times share a line, cross-fading with the hover.
            .child(
                div()
                    .relative()
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
                    .h(px(9.))
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
                    .child(capture_bounds(self.bounds.clone()))
                    .child(
                        div()
                            .w_full()
                            .h(px(thickness))
                            .rounded_full()
                            .bg(track)
                            .child(div().h_full().rounded_full().w(relative(fraction)).bg(fill)),
                    ),
            )
    }
}

fn format_secs(secs: f32) -> String {
    let s = secs.max(0.) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}
