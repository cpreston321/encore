//! The screen a new install opens to: the week's covers as a wall behind a glass card that
//! says what Encore is and offers to sign in.

use std::sync::{Arc, OnceLock};

use gpui::{GlassStyle, Image, ImageFormat};

use super::*;

static ICON: OnceLock<Arc<Image>> = OnceLock::new();

fn app_icon() -> Arc<Image> {
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../docs/icon.png").to_vec())))
        .clone()
}

const TILE: f32 = 150.;
const TILE_GAP: f32 = 14.;

impl MusicApp {
    pub(crate) fn render_welcome(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().welcome;
        if !presence.visible() {
            return None;
        }
        let p = presence.value();
        let signed_in = self.account == Account::SignedIn;

        let covers: Vec<Artwork> = self
            .browse
            .ready()
            .map(|b| b.top_albums.iter().map(|a| a.artwork.clone()).filter(|a| !a.is_empty()).take(48).collect())
            .unwrap_or_default();
        // Enough rows and columns to cover a large window; covers repeat if there are too few.
        const ROWS: usize = 9;
        const COLUMNS: usize = 14;
        let wall = div().absolute().top(px(-70.)).left(px(-60.)).flex().flex_col().gap(px(TILE_GAP)).when(
            !covers.is_empty(),
            |wall| {
                wall.children((0..ROWS).map(|row| {
                    // Alternate rows shift by half a tile, so the wall reads as a mosaic.
                    div().flex().gap(px(TILE_GAP)).when(row % 2 == 1, |d| d.ml(px(-(TILE + TILE_GAP) / 2.))).children(
                        (0..COLUMNS).map(|column| {
                            // Stepping by a large prime spreads repeats far apart.
                            let artwork = &covers[(row * COLUMNS + column) * 29 % covers.len()];
                            self.art(artwork, TILE, 14., t, cx)
                        }),
                    )
                }))
            },
        );

        let feature = |icon_name: &'static str, title: &'static str, detail: &'static str| {
            div()
                .flex()
                .items_start()
                .gap_3()
                .child(
                    div()
                        .size(px(34.))
                        .flex_none()
                        .rounded(px(10.))
                        .bg(t.accent.opacity(0.16))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(icon_name, 15., t.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(
                            div().text_size(px(12.5)).line_height(px(17.)).text_color(t.text_secondary).child(detail),
                        ),
                )
        };

        let primary_label = if signed_in { "Get Started" } else { "Sign In to Apple Music" };
        let card = div()
            .id("welcome-card")
            .occlude()
            .w(px(440.))
            .px(px(36.))
            .pt(px(34.))
            .pb(px(28.))
            .rounded(px(30.))
            .glass_style(GlassStyle { blur: px(30.), ..GlassStyle::default() })
            .bg(t.glass)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .flex()
            .flex_col()
            .items_center()
            .opacity(p)
            .mt(px((1. - p) * 18.))
            .child(div().size(px(92.)).rounded(px(22.)).shadow_lg().child(img(app_icon()).size(px(92.))))
            .child(
                div()
                    .pt_5()
                    .text_size(px(30.))
                    .line_height(px(36.))
                    .font_weight(FontWeight::BOLD)
                    .child("Welcome to Encore"),
            )
            .child(
                div()
                    .pt_1p5()
                    .text_center()
                    .text_color(t.text_secondary)
                    .child("Apple Music, made into a fast, native Mac app."),
            )
            .child(
                div()
                    .w_full()
                    .pt_7()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(feature(
                        "playlists",
                        "All of your music",
                        "Your library, playlists and favorites from Apple Music, with new music every week.",
                    ))
                    .child(feature(
                        "speaker-loud",
                        "At home on your Mac",
                        "Media keys, Control Center, VoiceOver and trackpad gestures, all working as you'd expect.",
                    ))
                    .child(feature(
                        "new",
                        "Light and quick",
                        "Opens in a quarter of a second and barely touches the processor while it plays.",
                    )),
            )
            .child(
                div()
                    .w_full()
                    .pt_8()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        pill_button(
                            t,
                            "welcome-primary",
                            if signed_in { "play" } else { "person" },
                            primary_label,
                            true,
                        )
                        .w_full()
                        .h(px(40.))
                        .text_size(px(14.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.finish_welcome(cx);
                            if !signed_in {
                                this.sign_in(cx);
                            }
                        })),
                    )
                    .when(!signed_in, |d| {
                        d.child(
                            div()
                                .id("welcome-later")
                                .h(px(32.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .text_color(t.text_secondary)
                                .cursor_pointer()
                                .hover(|s| s.text_color(t.text))
                                .child("Not Now")
                                .on_click(cx.listener(|this, _, _, cx| this.finish_welcome(cx))),
                        )
                    }),
            );

        Some(
            div()
                .id("welcome")
                .occlude()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .bg(t.window)
                .opacity(p)
                .child(wall)
                // Darken the wall toward the edges and bottom so the card stands out.
                .child(div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(t.window.opacity(0.55), 0.),
                    linear_color_stop(t.window.opacity(0.9), 1.),
                )))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    90.,
                    linear_color_stop(t.window.opacity(0.6), 0.),
                    linear_color_stop(t.window.opacity(0.), 0.5),
                )))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    270.,
                    linear_color_stop(t.window.opacity(0.6), 0.),
                    linear_color_stop(t.window.opacity(0.), 0.5),
                )))
                .child(div().absolute().inset_0().flex().items_center().justify_center().child(card))
                .into_any_element(),
        )
    }

    /// Closes the welcome screen for good.
    pub(crate) fn finish_welcome(&mut self, cx: &mut Context<Self>) {
        self.show_welcome = false;
        self.welcomed = true;
        self.save_settings();
        cx.notify();
    }
}
