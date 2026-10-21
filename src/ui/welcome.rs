//! The screen a new install opens to: the week's covers as a wall behind a glass card that
//! says what Encore is and offers to sign in.

use std::sync::{Arc, OnceLock};

use gpui::{GlassStyle, Image, ImageFormat, Transformation, Window, percentage};

use super::*;

static ICON: OnceLock<Arc<Image>> = OnceLock::new();

fn app_icon() -> Arc<Image> {
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../docs/icon.png").to_vec())))
        .clone()
}

/// The sign-in button while it waits: a tinted capsule with a spinning ring that breathes,
/// so it reads as busy rather than pressable.
pub(crate) fn connecting(t: &Theme, label: &'static str) -> impl IntoElement {
    let ring = icon("spinner", 13., t.accent).with_animation(
        "welcome-spin",
        Animation::new(Duration::from_millis(900)).repeat(),
        |ring, delta| ring.with_transformation(Transformation::rotate(percentage(delta))),
    );
    div()
        .id("welcome-connecting")
        .accessibility_role(Role::Button)
        .accessibility_label(label)
        .w_full()
        .h(px(40.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(t.accent.opacity(0.16))
        .border_1()
        .border_color(t.accent.opacity(0.35))
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.accent)
        .child(ring)
        .child(label)
        .with_animation("welcome-breathe", Animation::new(Duration::from_millis(1600)).repeat(), |d, delta| {
            d.opacity(0.75 + 0.25 * (delta * std::f32::consts::TAU).cos())
        })
}

enum WelcomeStep {
    /// No MusicKit key, so signing in can't work yet.
    Setup,
    Intro,
    Waiting,
    Loading,
    Failed(String),
    Ready,
}

const TILE: f32 = 150.;
const TILE_GAP: f32 = 14.;

impl MusicApp {
    pub(crate) fn render_welcome(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().welcome;
        if !presence.visible() {
            return None;
        }
        let p = presence.value();
        let step = self.welcome_step();
        let setup = matches!(step, WelcomeStep::Setup);

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
            .child(div().pt_5().text_size(px(30.)).line_height(px(36.)).font_weight(FontWeight::BOLD).child(if setup {
                "Connect to Apple Music"
            } else {
                "Welcome to Encore"
            }))
            .child(div().pt_1p5().text_center().text_color(t.text_secondary).child(if setup {
                "Encore signs in with a MusicKit key from your Apple Developer account."
            } else {
                "Apple Music, made into a fast, native Mac app."
            }))
            .when(setup, |d| d.child(self.render_key_setup(t, window, cx)).child(self.key_setup_actions(t, cx)))
            .when(!setup, |d| {
                d.child(
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
                .child(self.welcome_actions(t, step, cx))
            });

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

    fn welcome_step(&self) -> WelcomeStep {
        if !self.catalog.is_musickit() {
            return WelcomeStep::Setup;
        }
        match self.account {
            Account::SigningIn => WelcomeStep::Waiting,
            Account::SignedIn if self.welcome_signing => WelcomeStep::Loading,
            Account::SignedIn => WelcomeStep::Ready,
            Account::SignedOut => match &self.sign_in_error {
                Some(error) => WelcomeStep::Failed(error.clone()),
                None => WelcomeStep::Intro,
            },
        }
    }

    /// The buttons under the highlights. They follow sign-in through to a loaded library, so
    /// the app never appears signed out by accident.
    fn welcome_actions(&self, t: &Theme, step: WelcomeStep, cx: &mut Context<Self>) -> Div {
        let primary = |id: &'static str, icon_name: &'static str, label: &'static str| {
            pill_button(t, id, icon_name, label, true).w_full().h(px(40.)).text_size(px(14.))
        };
        let secondary = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .text_color(t.text_secondary)
                .cursor_pointer()
                .hover(|s| s.text_color(t.text))
                .child(label)
        };
        let status = |text: String| {
            div().pb_1().text_center().text_size(px(12.5)).line_height(px(17.)).text_color(t.text_secondary).child(text)
        };
        let column = div().w_full().pt_8().flex().flex_col().gap_2();
        match step {
            WelcomeStep::Setup => column,
            WelcomeStep::Intro => column
                .child(
                    primary("welcome-sign-in", "person", "Sign In to Apple Music")
                        .on_click(cx.listener(|this, _, _, cx| this.welcome_sign_in(cx))),
                )
                .child(
                    secondary("welcome-browse", "Browse Without Signing In")
                        .on_click(cx.listener(|this, _, _, cx| this.finish_welcome(cx))),
                ),
            WelcomeStep::Failed(error) => column
                .child(status(format!("Sign-in didn't finish: {error}")))
                .child(
                    primary("welcome-retry", "person", "Try Again")
                        .on_click(cx.listener(|this, _, _, cx| this.welcome_sign_in(cx))),
                )
                .child(
                    secondary("welcome-browse", "Browse Without Signing In")
                        .on_click(cx.listener(|this, _, _, cx| this.finish_welcome(cx))),
                ),
            WelcomeStep::Waiting => column.child(connecting(t, "Connecting to Apple Music…")).child(
                div()
                    .flex()
                    .justify_center()
                    .gap_4()
                    .child(
                        secondary("welcome-reopen", "Reopen Sign-In Page")
                            .on_click(cx.listener(|this, _, _, cx| this.welcome_sign_in(cx))),
                    )
                    .child(
                        secondary("welcome-cancel", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_sign_in(cx))),
                    ),
            ),
            // The empty row keeps the card the same height as the step before.
            WelcomeStep::Loading => column.child(connecting(t, "Loading Your Library…")).child(div().h(px(32.))),
            WelcomeStep::Ready => column.child(
                primary("welcome-start", "play", "Get Started")
                    .on_click(cx.listener(|this, _, _, cx| this.finish_welcome(cx))),
            ),
        }
    }

    fn welcome_sign_in(&mut self, cx: &mut Context<Self>) {
        self.welcome_signing = true;
        self.sign_in(cx);
    }

    /// The library from a welcome-screen sign-in has loaded, or failed to: time for the app.
    pub(crate) fn welcome_library_settled(&mut self, cx: &mut Context<Self>) {
        if self.welcome_signing {
            self.welcome_signing = false;
            self.finish_welcome(cx);
        }
    }

    /// Closes the welcome screen for good.
    pub(crate) fn finish_welcome(&mut self, cx: &mut Context<Self>) {
        if self.account == Account::SigningIn && self.welcome_signing {
            self.cancel_sign_in(cx);
        }
        self.show_welcome = false;
        self.welcomed = true;
        self.save_settings();
        cx.notify();
    }
}
