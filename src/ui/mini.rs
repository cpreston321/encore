//! Mini Player: a small floating window with the cover, and controls on hover.

use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, Role, Bounds, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ObjectFit, Pixels, ScrollWheelEvent, Subscription, Task, TitlebarOptions, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, linear_color_stop, linear_gradient, point, prelude::*, px, relative, size,
    white,
};

use crate::{
    api,
    app::{MusicApp, PlayState},
    queue::Repeat,
    theme::Theme,
    ui::icon,
};

const DEFAULT: f32 = 300.;
const MIN: f32 = 220.;
const PAD: f32 = 16.;
const FADE: Duration = Duration::from_millis(180);

pub struct MiniPlayer {
    app: Entity<MusicApp>,
    focus: FocusHandle,
    hovered: bool,
    hover_changed: Instant,
    /// Where a drag on the scrubber would seek to, as a fraction of the song.
    scrub: Option<f32>,
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
    shuffle: bool,
    repeat: Repeat,
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

/// Opens the mini player, or closes it if it's open.
pub fn toggle(app: Entity<MusicApp>, cx: &mut App) {
    if let Some(handle) = OPEN.with(|o| o.take()) {
        let _ = handle.update(cx, |_, window, _| {
            save_bounds(window);
            window.remove_window();
        });
        return;
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
            window.on_window_should_close(cx, |window, _| {
                save_bounds(window);
                OPEN.with(|o| o.set(None));
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
                let mini = MiniPlayer {
                    app,
                    focus: cx.focus_handle(),
                    hovered: false,
                    hover_changed: Instant::now() - FADE,
                    scrub: None,
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
            shuffle: app.queue.shuffle,
            repeat: app.queue.repeat,
        }
    }

    fn load_art(&self, cx: &mut Context<Self>) {
        if let Some(url) = art_url(self.app.read(cx)) {
            self.app.update(cx, |app, cx| {
                app.image(&url, cx);
            });
        }
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
        let width = f32::from(window.viewport_size().width) - PAD * 2.;
        ((f32::from(position.x) - PAD) / width.max(1.)).clamp(0., 1.)
    }

    fn show_main_window(&mut self, cx: &mut Context<Self>) {
        let mini = OPEN.with(|o| o.get());
        for handle in cx.windows() {
            if Some(handle) != mini {
                let _ = handle.update(cx, |_, window, _| window.activate_window());
            }
        }
        cx.activate(true);
        if let Some(mini) = OPEN.with(|o| o.take()) {
            let _ = mini.update(cx, |_, window, _| {
                save_bounds(window);
                window.remove_window();
            });
        }
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
        let t = Theme::for_appearance(window.appearance());
        let app = self.app.read(cx);
        let song = app.queue.current().cloned();
        let playing = matches!(app.state, PlayState::Playing | PlayState::Loading);
        let (position, duration) = (app.position(), app.duration());
        let (shuffle, repeat) = (app.queue.shuffle, app.queue.repeat);
        let art = art_url(app).and_then(|url| app.cached_image(&url));
        let has_art = art.is_some();
        let fraction = self.scrub.unwrap_or(if duration > 0. { (position / duration).clamp(0., 1.) } else { 0. });
        let shown_position = if self.scrub.is_some() { fraction * duration } else { position };

        let control = |id: &'static str, label: &'static str, name: &'static str, size: f32, on: bool| {
            div()
                .id(id)
                .accessibility_role(Role::Button)
                .accessibility_label(label)
                .size(px(size + 16.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white().opacity(0.15)))
                .active(|s| s.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(icon(name, size, if on { white() } else { white().opacity(0.6) }))
        };
        let act = |f: fn(&mut MusicApp, &mut Context<MusicApp>)| {
            let app = self.app.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| app.update(cx, f)
        };

        let scrubber = div()
            .id("mini-scrubber")
            .h(px(14.))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.scrub = Some(Self::scrub_fraction(e.position, window));
                    cx.notify();
                }),
            )
            .child(
                div()
                    .w_full()
                    .h(px(if self.scrub.is_some() { 6. } else { 4. }))
                    .rounded_full()
                    .bg(white().opacity(0.25))
                    .child(div().h_full().rounded_full().w(relative(fraction)).bg(white())),
            );

        let overlay = div()
            .absolute()
            .inset_0()
            .opacity(shown)
            .bg(linear_gradient(
                180.,
                linear_color_stop(gpui::black().opacity(0.3), 0.),
                linear_color_stop(gpui::black().opacity(0.82), 1.),
            ))
            .flex()
            .flex_col()
            .justify_end()
            .px(px(PAD))
            .pb_3()
            .gap_1()
            .child(
                div().absolute().top(px(6.)).right(px(6.)).child(
                    control("mini-expand", "Show Main Window", "open", 14., true)
                        .on_click(cx.listener(|this, _, _, cx| this.show_main_window(cx))),
                ),
            )
            .when_some(song.clone(), |d, s| {
                d.child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(s.title)).child(
                    div().truncate().text_size(px(12.)).text_color(white().opacity(0.75)).child(s.artist),
                )
            })
            .child(div().pt_1().child(scrubber))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(10.5))
                    .text_color(white().opacity(0.7))
                    .child(format_secs(shown_position))
                    .child(format!("-{}", format_secs(duration - shown_position))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(control("mini-shuffle", "Shuffle", "shuffle", 13., shuffle).on_click(act(|a, cx| {
                        a.queue.toggle_shuffle();
                        cx.notify();
                    })))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(control("mini-prev", "Previous", "prev", 18., true).on_click(act(|a, cx| a.previous(cx))))
                            .child(
                                control("mini-play", if playing { "Pause" } else { "Play" }, if playing { "pause" } else { "play" }, 24., true)
                                    .on_click(act(|a, cx| a.toggle_play(cx))),
                            )
                            .child(control("mini-next", "Next", "next", 18., true).on_click(act(|a, cx| a.next(cx)))),
                    )
                    .child(
                        control(
                            "mini-repeat",
                            "Repeat",
                            if repeat == Repeat::One { "repeat-one" } else { "repeat" },
                            13.,
                            repeat != Repeat::Off,
                        )
                        .on_click(act(|a, cx| {
                            a.queue.cycle_repeat();
                            cx.notify();
                        })),
                    ),
            );

        div()
            .id("mini")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(t.window)
            .font_family(".SystemUIFont")
            .text_color(white())
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
                        .text_color(t.text_tertiary)
                        .child(icon("note", 40., t.text_tertiary))
                        .child(div().text_size(px(12.)).child("Not Playing")),
                )
            })
            .when(song.is_some() && !has_art, |d| {
                d.child(div().absolute().inset_0().flex().items_center().justify_center().child(icon(
                    "note",
                    48.,
                    t.text_tertiary,
                )))
            })
            .when(shown > 0. && song.is_some(), |d| d.child(overlay))
    }
}
