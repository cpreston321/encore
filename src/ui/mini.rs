//! Mini Player: a small floating window with the cover, and on hover the player bar's glass,
//! scrubber and controls floating over it.

use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, FocusHandle, FontWeight, GlassStyle, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, ScrollWheelEvent, Subscription, Task,
    TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions, div, img, linear_color_stop, linear_gradient,
    point, prelude::*, px, relative, size,
};
use tahoe::primitives::icon_button;

use crate::{
    api,
    app::{MusicApp, PlayState},
    theme::{Palette, Theme},
    ui::{icon, player::player_theme},
};

const DEFAULT: f32 = 300.;
const MIN: f32 = 220.;
/// The glass card's gap from the window's edges, and its padding.
const CARD_INSET: f32 = 8.;
const CARD_PAD: f32 = 14.;
/// The times beside the scrubber, and their gap from it.
const TIME_W: f32 = 30.;
const TIME_GAP: f32 = 6.;
/// Where the scrubber starts and stops, from either side of the window.
const SCRUB_INSET: f32 = CARD_INSET + CARD_PAD + TIME_W + TIME_GAP;
const FADE: Duration = Duration::from_millis(180);

pub struct MiniPlayer {
    app: Entity<MusicApp>,
    focus: FocusHandle,
    hovered: bool,
    hover_changed: Instant,
    /// Where a drag on the scrubber would seek to, as a fraction of the song.
    scrub: Option<f32>,
    /// Where the pointer is over the scrubber, which thickens and previews the jump.
    pointer: Option<f32>,
    /// The cover's color, which tints the glass as it does the player bar.
    tint: Option<Palette>,
    /// What was last drawn, so changes elsewhere in the app don't redraw the window.
    drawn: Option<Drawn>,
    _observe: Subscription,
    _ticker: Task<()>,
}

#[derive(PartialEq)]
struct Drawn {
    song: Option<String>,
    playing: bool,
    art: bool,
    tint: bool,
}

thread_local! {
    static OPEN: std::cell::Cell<Option<AnyWindowHandle>> = const { std::cell::Cell::new(None) };
}

fn bounds_path() -> std::path::PathBuf {
    api::data_dir().join("mini-player.json")
}

fn saved_bounds() -> Option<Bounds<Pixels>> {
    let [x, y, w, h]: [f32; 4] = serde_json::from_slice(&std::fs::read(bounds_path()).ok()?).ok()?;
    (w >= MIN && h >= MIN).then(|| Bounds::new(point(px(x), px(y)), size(px(w), px(h))))
}

fn save_bounds(window: &Window) {
    let b = window.bounds();
    let values = [b.origin.x, b.origin.y, b.size.width, b.size.height].map(f32::from);
    if let Ok(json) = serde_json::to_vec(&values) {
        let _ = std::fs::create_dir_all(api::data_dir());
        let _ = std::fs::write(bounds_path(), json);
    }
}

/// Whether `window` is the mini player.
pub fn is_mini(window: AnyWindowHandle) -> bool {
    OPEN.with(|o| o.get()) == Some(window)
}

/// The app's other windows: the main window, which the mini player stands in for.
fn main_windows(cx: &App) -> Vec<AnyWindowHandle> {
    let mini = OPEN.with(|o| o.get());
    cx.windows().into_iter().filter(|w| Some(*w) != mini).collect()
}

fn show_main_windows(cx: &mut App) {
    for handle in main_windows(cx) {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
    }
    cx.activate(true);
}

/// Closes the mini player and brings the main window back.
fn close(cx: &mut App) {
    if let Some(mini) = OPEN.with(|o| o.take()) {
        let _ = mini.update(cx, |_, window, _| {
            save_bounds(window);
            window.remove_window();
        });
    }
    show_main_windows(cx);
}

/// Opens the mini player in place of the main window, or closes it and brings the main
/// window back.
pub fn toggle(app: Entity<MusicApp>, cx: &mut App) {
    // The menu or shortcut arrives while its window is mid-update, and a window can't be
    // hidden or closed from inside its own update; those calls would be dropped.
    cx.defer(move |cx| toggle_now(app, cx));
}

fn toggle_now(app: Entity<MusicApp>, cx: &mut App) {
    if OPEN.with(|o| o.get()).is_some() {
        return close(cx);
    }
    let bounds = saved_bounds().unwrap_or_else(|| Bounds::new(point(px(80.), px(80.)), size(px(DEFAULT), px(DEFAULT))));
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Encore Mini Player".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(10.), px(10.))),
            }),
            kind: WindowKind::Floating,
            is_resizable: true,
            window_min_size: Some(size(px(MIN), px(MIN))),
            ..Default::default()
        },
        |window, cx| {
            window.on_window_should_close(cx, |window, cx| {
                save_bounds(window);
                OPEN.with(|o| o.set(None));
                show_main_windows(cx);
                true
            });
            let view = cx.new(|cx| {
                // Only the times and scrubber move, and they only show on hover.
                let ticker = cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(Duration::from_millis(250)).await;
                        let alive = this.update(cx, |this: &mut MiniPlayer, cx| {
                            if this.hovered && this.app.read(cx).state == PlayState::Playing {
                                cx.notify();
                            }
                        });
                        if alive.is_err() {
                            break;
                        }
                    }
                });
                let observe = cx.observe(&app, |this: &mut MiniPlayer, _, cx| {
                    this.load_art(cx);
                    if this.hovered || this.drawn.as_ref() != Some(&this.snapshot(cx)) {
                        cx.notify();
                    }
                });
                let mut mini = MiniPlayer {
                    app,
                    focus: cx.focus_handle(),
                    // A tour shows the controls, which otherwise need the pointer.
                    hovered: cfg!(any(debug_assertions, feature = "tour"))
                        && std::env::var_os("ENCORE_TOUR_MINI").is_some(),
                    hover_changed: Instant::now() - FADE,
                    scrub: None,
                    pointer: None,
                    tint: None,
                    drawn: None,
                    _observe: observe,
                    _ticker: ticker,
                };
                mini.load_art(cx);
                mini
            });
            window.focus(&view.read(cx).focus);
            view
        },
    );
    if let Ok(handle) = handle {
        OPEN.with(|o| o.set(Some(handle.into())));
        for main in main_windows(cx) {
            let _ = main.update(cx, |_, window, _| window.hide_window());
        }
    }
}

fn art_url(app: &MusicApp) -> Option<String> {
    app.queue.current().map(|s| s.artwork.url(600)).filter(|u| !u.is_empty())
}

impl MiniPlayer {
    fn snapshot(&self, cx: &App) -> Drawn {
        let app = self.app.read(cx);
        Drawn {
            song: app.queue.current().map(|s| s.id.clone()),
            playing: matches!(app.state, PlayState::Playing | PlayState::Loading),
            art: art_url(app).is_some_and(|u| app.cached_image(&u).is_some()),
            tint: self.tint.is_some(),
        }
    }

    fn load_art(&mut self, cx: &mut Context<Self>) {
        let Some(artwork) = self.app.read(cx).queue.current().map(|s| s.artwork.clone()) else {
            self.tint = None;
            return;
        };
        let url = artwork.url(600);
        let color = self.app.update(cx, |app, cx| {
            app.image(&url, cx);
            app.art_color(&artwork, cx)
        });
        self.tint = color.map(Palette::from_average);
    }

    /// 0 (hidden) to 1 (shown) for the controls, which fade with the pointer.
    fn overlay(&self, window: &mut Window) -> f32 {
        let t = (self.hover_changed.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.);
        if t < 1. {
            window.request_animation_frame();
        }
        let eased = 1. - (1. - t).powi(3);
        if self.hovered || self.scrub.is_some() { eased } else { 1. - eased }
    }

    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered != self.hovered {
            self.hovered = hovered;
            self.hover_changed = Instant::now();
            cx.notify();
        }
    }

    fn scrub_fraction(position: gpui::Point<Pixels>, window: &Window) -> f32 {
        let width = f32::from(window.viewport_size().width) - SCRUB_INSET * 2.;
        ((f32::from(position.x) - SCRUB_INSET) / width.max(1.)).clamp(0., 1.)
    }

    fn show_main_window(&mut self, cx: &mut Context<Self>) {
        // Closing the window from inside its own view's update would update it reentrantly.
        cx.defer(close);
    }

    fn key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let app = self.app.clone();
        match e.keystroke.key.as_str() {
            "space" => app.update(cx, |a, cx| a.toggle_play(cx)),
            "left" => app.update(cx, |a, cx| a.previous(cx)),
            "right" => app.update(cx, |a, cx| a.next(cx)),
            _ => return,
        }
        cx.stop_propagation();
    }
}

fn format_secs(secs: f32) -> String {
    let s = secs.max(0.) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drawn = Some(self.snapshot(cx));
        let shown = self.overlay(window);
        let base = Theme::for_appearance(window.appearance());
        // The player bar's colors: glass in a deep shade of the cover, with light ink.
        let t = player_theme(&base, self.tint);
        let app = self.app.read(cx);
        let song = app.queue.current().cloned();
        let playing = matches!(app.state, PlayState::Playing | PlayState::Loading);
        let (position, duration) = (app.position(), app.duration());
        let art = art_url(app).and_then(|url| app.cached_image(&url));
        let has_art = art.is_some();
        let fraction = self.scrub.unwrap_or(if duration > 0. { (position / duration).clamp(0., 1.) } else { 0. });
        let shown_position = if self.scrub.is_some() { fraction * duration } else { position };

        let control = |id: &'static str, label: &'static str, name: &'static str, size: f32, color: Hsla| {
            icon_button(&t, id, name, size, color)
                .accessibility_label(label)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let act = |f: fn(&mut MusicApp, &mut Context<MusicApp>)| {
            let app = self.app.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| app.update(cx, f)
        };

        // The player bar's scrubber: thin, thickening under the pointer, with the stretch a
        // click would skip shown lighter and the part it would undo dimmed.
        let pointer = self.pointer.filter(|_| self.scrub.is_none());
        let thickness = if pointer.is_some() || self.scrub.is_some() { 7. } else { 3. };
        let (solid, preview) = match pointer {
            Some(at) if at > fraction => (fraction, Some((fraction, at - fraction, 0.45))),
            Some(at) => (at, Some((at, fraction - at, 0.5))),
            None => (fraction, None),
        };
        let scrubber = div()
            .id("mini-scrubber")
            .h(px(12.))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.pointer.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.scrub = Some(Self::scrub_fraction(e.position, window));
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                if e.pressed_button.is_none() {
                    this.pointer = Some(Self::scrub_fraction(e.position, window));
                    cx.notify();
                }
            }))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(thickness))
                    .rounded_full()
                    .bg(t.track_bg)
                    .overflow_hidden()
                    .when_some(preview, |d, (start, width, alpha)| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(relative(start))
                                .w(relative(width))
                                .bg(t.track_fill.opacity(alpha)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .w(relative(solid))
                            .when(preview.is_none(), |d| d.rounded_full())
                            .bg(t.track_fill),
                    ),
            );

        let time =
            |text: String| div().flex_none().w(px(TIME_W)).text_size(px(10.)).text_color(t.text_secondary).child(text);
        let card = div()
            .id("mini-card")
            .absolute()
            .left(px(CARD_INSET))
            .right(px(CARD_INSET))
            // Rises into place as it fades in.
            .bottom(px(CARD_INSET - (1. - shown) * 8.))
            .pl(px(CARD_PAD))
            .pr_1()
            .pt_1()
            .pb_1p5()
            .rounded(px(18.))
            .glass_style(GlassStyle { blur: px(22.), ..GlassStyle::default() })
            .bg(t.pill)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .opacity(shown)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().flex_1().min_w_0().flex().flex_col().when_some(song.clone(), |d, s| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .line_height(px(17.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.text)
                                .child(s.title),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.5))
                                .line_height(px(15.))
                                .text_color(t.text_secondary)
                                .child(s.artist),
                        )
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .child(
                                control("mini-prev", "Previous", "prev", 15., t.icon)
                                    .on_click(act(|a, cx| a.previous(cx))),
                            )
                            .child(
                                control(
                                    "mini-play",
                                    if playing { "Pause" } else { "Play" },
                                    if playing { "pause" } else { "play" },
                                    19.,
                                    t.icon,
                                )
                                .size(px(34.))
                                .on_click(act(|a, cx| a.toggle_play(cx))),
                            )
                            .child(control("mini-next", "Next", "next", 15., t.icon).on_click(act(|a, cx| a.next(cx)))),
                    ),
            )
            // The times sit at the scrubber's ends, on its line.
            .child(
                div()
                    .mr(px(CARD_PAD - 4.))
                    .flex()
                    .items_center()
                    .gap(px(TIME_GAP))
                    .child(time(format_secs(shown_position)))
                    .child(div().flex_1().min_w_0().child(scrubber))
                    .child(div().text_right().child(time(format!("-{}", format_secs(duration - shown_position))))),
            );

        // A soft shade under the window buttons and the expand button, so they read on bright
        // covers.
        let top = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(56.))
            .opacity(shown)
            .bg(linear_gradient(
                180.,
                linear_color_stop(gpui::black().opacity(0.35), 0.),
                linear_color_stop(gpui::black().opacity(0.), 1.),
            ))
            .child(
                div().absolute().top(px(7.)).right(px(7.)).child(
                    icon_button(&t, "mini-expand", "open", 13., t.icon)
                        .accessibility_label("Show Main Window")
                        .bg(t.pill)
                        .glass_style(GlassStyle { blur: px(16.), ..GlassStyle::default() })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| this.show_main_window(cx))),
                ),
            );

        div()
            .id("mini")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(base.window)
            .font_family(".SystemUIFont")
            .text_color(t.text)
            .on_key_down(cx.listener(Self::key_down))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(*hovered, cx)))
            // The cover is the window's handle: drag anywhere that isn't a control to move it.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                if this.scrub.is_some() && e.pressed_button == Some(MouseButton::Left) {
                    this.scrub = Some(Self::scrub_fraction(e.position, window));
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(fraction) = this.scrub.take() {
                        this.app.update(cx, |a, cx| {
                            let secs = fraction * a.duration();
                            a.seek_to(secs, cx);
                        });
                        cx.notify();
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                this.app.update(cx, |a, cx| a.volume_scrolled(e, cx));
            }))
            .children(art.map(|path| img(path).absolute().inset_0().size_full().object_fit(ObjectFit::Cover)))
            .when(song.is_none(), |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(base.text_tertiary)
                        .child(icon("note", 40., base.text_tertiary))
                        .child(div().text_size(px(12.)).child("Not Playing")),
                )
            })
            .when(song.is_some() && !has_art, |d| {
                d.child(div().absolute().inset_0().flex().items_center().justify_center().child(icon(
                    "note",
                    48.,
                    base.text_tertiary,
                )))
            })
            .when(shown > 0., |d| d.child(top))
            .when(shown > 0. && song.is_some(), |d| d.child(card))
    }
}
