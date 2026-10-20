//! Mini Player: a small floating window with the cover, and controls on hover.

use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, FontWeight, ObjectFit, Subscription, TitlebarOptions,
    Window, WindowBounds, WindowKind, WindowOptions, div, img, linear_color_stop, linear_gradient, point,
    prelude::*, px, size, white,
};

use crate::{
    app::{MusicApp, PlayState},
    theme::Theme,
    views::icon,
};

pub struct MiniPlayer {
    app: Entity<MusicApp>,
    _observe: Subscription,
}

thread_local! {
    static OPEN: std::cell::Cell<Option<AnyWindowHandle>> = const { std::cell::Cell::new(None) };
}

/// Opens the mini player, or closes it if it's open.
pub fn toggle(app: Entity<MusicApp>, cx: &mut App) {
    if let Some(handle) = OPEN.with(|o| o.take()) {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return;
    }
    let bounds = Bounds::new(point(px(80.), px(80.)), size(px(300.), px(300.)));
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Encore Mini Player".into()), appears_transparent: true, traffic_light_position: Some(point(px(10.), px(10.))) }),
            kind: WindowKind::Floating,
            is_resizable: false,
            window_min_size: Some(size(px(300.), px(300.))),
            ..Default::default()
        },
        |window, cx| {
            window.on_window_should_close(cx, |_, _| {
                OPEN.with(|o| o.set(None));
                true
            });
            cx.new(|cx| MiniPlayer { _observe: cx.observe(&app, |_, _, cx| cx.notify()), app })
        },
    );
    if let Ok(handle) = handle {
        OPEN.with(|o| o.set(Some(handle.into())));
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::for_appearance(window.appearance());
        let app = self.app.read(cx);
        let song = app.queue.current().cloned();
        let playing = matches!(app.state, PlayState::Playing | PlayState::Loading);
        let (position, duration) = (app.position(), app.duration());
        let art = song.as_ref().and_then(|s| app.cached_image(&s.artwork.url(600)));
        // Keep the progress line moving while playing.
        if playing {
            window.request_animation_frame();
        }

        let control = |id: &'static str, name: &'static str, size: f32| {
            div()
                .id(id)
                .size(px(size + 18.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white().opacity(0.15)))
                .child(icon(name, size, white()))
        };
        let app_entity = self.app.clone();
        let (a1, a2, a3) = (app_entity.clone(), app_entity.clone(), app_entity);
        let fraction = if duration > 0. { (position / duration).clamp(0., 1.) } else { 0. };

        div()
            .id("mini")
            .group("mini")
            .size_full()
            .relative()
            .bg(t.window)
            .font_family(".SystemUIFont")
            .text_color(white())
            .children(art.map(|path| img(path).size_full().object_fit(ObjectFit::Cover)))
            .when(song.is_none(), |d| {
                d.flex().items_center().justify_center().child(icon("note", 48., t.text_tertiary))
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .invisible()
                    .group_hover("mini", |s| s.visible())
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(gpui::black().opacity(0.25), 0.),
                        linear_color_stop(gpui::black().opacity(0.8), 1.),
                    ))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .p_4()
                    .gap_2()
                    .when_some(song, |d, s| {
                        d.child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(s.title))
                            .child(div().truncate().text_size(px(12.)).text_color(white().opacity(0.75)).child(s.artist))
                    })
                    .child(
                        div().h(px(3.)).rounded_full().bg(white().opacity(0.25)).child(
                            div().h_full().rounded_full().w(gpui::relative(fraction)).bg(white()),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_3()
                            .child(control("mini-prev", "prev", 18.).on_click(move |_, _, cx| {
                                a1.update(cx, |a, cx| a.previous(cx));
                            }))
                            .child(control("mini-play", if playing { "pause" } else { "play" }, 24.).on_click(
                                move |_, _, cx| {
                                    a2.update(cx, |a, cx| a.toggle_play(cx));
                                },
                            ))
                            .child(control("mini-next", "next", 18.).on_click(move |_, _, cx| {
                                a3.update(cx, |a, cx| a.next(cx));
                            })),
                    ),
            )
    }
}
