//! Every Tahoe component in one window: `cargo run -p tahoe --example gallery`.
//! TAHOE_APPEARANCE=light|dark forces an appearance.

use gpui::{
    App, Application, Bounds, Context, Hsla, IntoElement, ParentElement, Render, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, point, prelude::*, px, size,
};
use tahoe::{
    prelude::*,
    primitives::skeleton_block,
    theme::{Appearance, force_appearance},
};

struct Gallery {
    clicks: usize,
    last: &'static str,
}

impl Gallery {
    fn clicked(&mut self, what: &'static str, cx: &mut Context<Self>) {
        self.clicks += 1;
        self.last = what;
        cx.notify();
    }
}

fn swatch(label: &'static str, color: Hsla, t: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().size(px(44.)).rounded(px(10.)).bg(color).border_1().border_color(t.panel_border))
        .child(div().text_size(px(11.)).text_color(t.text_secondary).child(label))
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::for_appearance(window.appearance());
        // A page tinted by artwork, with a pale accent from light cover art.
        let tinted = t.tinted(Palette::from_art_colors(0x2b1d14, [0xf3e4d6, 0xd9c2ad, 0xe7d3c0, 0xc8b29c]));
        let row = || div().px(px(32.)).flex().flex_wrap().items_center().gap_3();

        div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(t.window)
            .text_color(t.text)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .child(PageHeader::new("Tahoe").on_back(cx.listener(|this, _, _, cx| this.clicked("back", cx))))
            .child(
                div()
                    .px(px(32.))
                    .text_color(t.text_secondary)
                    .child(format!("{} clicks · last: {}", self.clicks, self.last)),
            )
            .child(
                SectionHeader::new("buttons", "Buttons")
                    .on_click(cx.listener(|this, _, _, cx| this.clicked("section", cx))),
            )
            .child(
                row()
                    .child(
                        Button::new("filled", "Play")
                            .icon("play")
                            .on_click(cx.listener(|this, _, _, cx| this.clicked("Play", cx))),
                    )
                    .child(
                        Button::new("tinted", "Shuffle")
                            .icon("shuffle")
                            .style(ButtonStyle::Tinted)
                            .on_click(cx.listener(|this, _, _, cx| this.clicked("Shuffle", cx))),
                    )
                    .child(Button::new("plain", "Add").icon("plus").style(ButtonStyle::Plain))
                    .child(IconButton::new("toolbar", "chevron-left"))
                    .child(IconButton::new("toolbar-more", "ellipsis"))
                    .child(IconButton::new("plain-icon", "heart").style(IconButtonStyle::Plain))
                    .child(IconButton::new("filled-icon", "play").size(44.).style(IconButtonStyle::Filled(t.accent))),
            )
            .child(SectionHeader::new("links", "Links and badges"))
            .child(
                row()
                    .child(
                        Link::new("artist", "Zach Bryan")
                            .on_click(cx.listener(|this, _, _, cx| this.clicked("Zach Bryan", cx))),
                    )
                    .child(div().text_color(t.text_secondary).child("&"))
                    .child(
                        Link::new("artist-2", "Lana Del Rey")
                            .on_click(cx.listener(|this, _, _, cx| this.clicked("Lana Del Rey", cx))),
                    )
                    .child(Badge::new("E")),
            )
            .child(SectionHeader::new("cards", "Cards"))
            .child(
                row().child(
                    Card::new()
                        .w(px(360.))
                        .p_4()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(skeleton_block(&t, 64., 64., 8.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().font_weight(gpui::FontWeight::BOLD).child("A card"))
                                .child(div().text_color(t.text_secondary).child("Rounded, with a hairline border")),
                        ),
                ),
            )
            .child(SectionHeader::new("tinted", "On a tinted surface"))
            .child(
                div()
                    .mx(px(32.))
                    .p_4()
                    .rounded(px(16.))
                    .bg(tinted.window)
                    .text_color(tinted.text)
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        IconButton::new("tinted-play", "play")
                            .size(44.)
                            .style(IconButtonStyle::Filled(tinted.accent))
                            .theme(tinted),
                    )
                    .child(Button::new("tinted-filled", "Play").icon("play").theme(tinted))
                    .child(IconButton::new("tinted-back", "chevron-left").theme(tinted))
                    .child(Link::new("tinted-link", "Linked artist").theme(tinted)),
            )
            .child(SectionHeader::new("colors", "Theme"))
            .child(
                row()
                    .pb(px(40.))
                    .child(swatch("window", t.window, &t))
                    .child(swatch("panel", t.panel, &t))
                    .child(swatch("pill", t.pill, &t))
                    .child(swatch("button", t.button, &t))
                    .child(swatch("accent", t.accent, &t))
                    .child(swatch("text", t.text, &t))
                    .child(swatch("secondary", t.text_secondary, &t)),
            )
    }
}

fn main() {
    match std::env::var("TAHOE_APPEARANCE").as_deref() {
        Ok("light") => force_appearance(Some(Appearance::Light)),
        Ok("dark") => force_appearance(Some(Appearance::Dark)),
        _ => {}
    }
    Application::new().with_assets(tahoe::Assets).run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(820.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tahoe Gallery".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(18.), px(18.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                tahoe::chrome::use_tahoe_corners(window);
                cx.new(|_| Gallery { clicks: 0, last: "nothing yet" })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
