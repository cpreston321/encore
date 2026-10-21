//! Every Tahoe component in one window: `cargo run -p tahoe --example gallery`.
//! TAHOE_APPEARANCE=light|dark forces an appearance.

use gpui::{
    App, Application, Bounds, Context, FocusHandle, FontWeight, Hsla, IntoElement, KeyDownEvent, ParentElement, Render,
    Styled, TextShadow, TitlebarOptions, Window, WindowBounds, WindowOptions, black, div, point, prelude::*, px, size,
    white,
};
use tahoe::{
    prelude::*,
    primitives::skeleton_block,
    textfield::text_input_box,
    theme::{Appearance, force_appearance, gradient},
};

const NOTES: &str = "For a razor-toothed mixtape, the record is, to a surprising extent, a family affair. \
It was announced before its release by the artist's mother, and the only feature across its 15 songs is \
from a hometown friend.\n\nOne of the writer's great gifts is a keen understanding of how to manipulate \
the stakes. Frequently in his records, a boast turns into a confession halfway through a verse, and the \
beat drops out just long enough for you to notice.";

struct Gallery {
    clicks: usize,
    last: &'static str,
    field: TextField,
    field_focus: FocusHandle,
    segment: usize,
    shuffle: bool,
    notes_open: bool,
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

impl TextFieldHost for Gallery {
    type Field = ();

    fn text_field<R>(&mut self, _: (), edit: impl FnOnce(&mut TextField) -> R) -> Option<R> {
        Some(edit(&mut self.field))
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::for_appearance(window.appearance());
        div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .map(|d| tahoe::layout::overlay_scrollbar(d, &t, gpui::Edges::default()))
            .bg(t.window)
            .text_color(t.text)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .child(self.contents(&t, window, cx))
    }
}

impl Gallery {
    fn contents(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *t;
        // A page tinted by artwork, with a pale accent from light cover art.
        let tinted = t.tinted(Palette::from_art_colors(0x2b1d14, [0xf3e4d6, 0xd9c2ad, 0xe7d3c0, 0xc8b29c]));
        let row = || div().px(px(32.)).flex().flex_wrap().items_center().gap_3();
        let focused = self.field_focus.is_focused(window);
        div()
            .flex()
            .flex_col()
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
            .child(SectionHeader::new("controls", "Controls"))
            .child(
                row()
                    .child((0..3).fold(SegmentedControl::new(), |c, i| {
                        c.segment(
                            ("seg", i),
                            ["Off", "6s", "12s"][i],
                            self.segment == i,
                            cx.listener(move |this, _, _, cx| {
                                this.segment = i;
                                cx.notify();
                            }),
                        )
                    }))
                    .child(ToggleButton::new("shuffle", "shuffle", self.shuffle).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.shuffle = !this.shuffle;
                            cx.notify();
                        },
                    )))
                    .child(Kbd::new("⌘K"))
                    .child(Kbd::new("esc")),
            )
            .child(
                row()
                    .pt_3()
                    .child(
                        text_input_box(&t, "field", Some("search"), focused)
                            .w(px(260.))
                            .track_focus(&self.field_focus)
                            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                                if this.field.handle_key(&e.keystroke, window, cx) {
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(|this, _, window, _| window.focus(&this.field_focus)))
                            .child(
                                div().flex_1().min_w_0().child(
                                    TextInput::new(cx.entity(), (), &self.field_focus, &t)
                                        .placeholder("Type, compose or ⌃⌘Space"),
                                ),
                            ),
                    )
                    .child(div().text_color(t.text_secondary).child(format!("“{}”", self.field.text()))),
            )
            .child(SectionHeader::new("people", "Avatars and empty states"))
            .child(row().child(Avatar::new("Christian Preston").size(40.)).child(Avatar::new("Lana Del Rey").size(32.)))
            .child(
                row().pt_4().child(
                    Card::new().w(px(360.)).h(px(300.)).child(
                        EmptyState::new("No Songs", "Songs you add appear here.")
                            .action(Button::new("browse", "Browse").style(ButtonStyle::Tinted)),
                    ),
                ),
            )
            .child(SectionHeader::new("text", "Expandable text"))
            .child(div().px(px(32.)).child(ExpandableText::new("notes", NOTES).expanded(self.notes_open).on_toggle(
                cx.listener(|this, _, _, cx| {
                    this.notes_open = !this.notes_open;
                    cx.notify();
                }),
            )))
            .child(SectionHeader::new("typography", "Typography"))
            .child(
                div()
                    .px(px(32.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children([13., 19., 28.].map(|size| {
                        div()
                            .flex()
                            .items_baseline()
                            .gap_6()
                            .text_size(px(size))
                            .font_weight(FontWeight::BOLD)
                            .child(div().w(px(300.)).child("Recently Added Albums"))
                            .child(div().w(px(300.)).tracking(px(-0.02 * size)).child("Recently Added Albums"))
                    }))
                    .child(
                        div()
                            .text_color(t.text_secondary)
                            .child("13, 19 and 28 pt: as the system shapes it, then with .tracking(-2% of the size)"),
                    ),
            )
            .child(row().pt_4().items_start().children([1, 2, 3].map(|lines| {
                div()
                    .w(px(230.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(px(14.)).line_height(px(20.)).line_clamp(lines).text_ellipsis().child(NOTES))
                    .child(div().text_size(px(11.)).text_color(t.text_secondary).child(format!("line_clamp({lines})")))
            })))
            .child(
                row().pt_4().child(
                    div()
                        .w(px(560.))
                        .h(px(120.))
                        .rounded(px(12.))
                        .bg(gradient(0xfde68a, 0x93c5fd))
                        .p_5()
                        .flex()
                        .items_end()
                        .gap_8()
                        .text_color(white())
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(20.))
                        .child(div().child("No shadow"))
                        .child(div().text_shadow(text_shadow_on_art()).child("text_shadow_on_art"))
                        .child(
                            div()
                                .text_shadow(TextShadow {
                                    color: black().opacity(0.6),
                                    offset: point(px(2.), px(2.)),
                                    blur_radius: px(0.),
                                })
                                .child("Hard"),
                        ),
                ),
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
        tahoe::layout::init_scrolling(cx);
        let bounds = Bounds::centered(None, size(px(820.), px(1000.)), cx);
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
                cx.new(|cx| Gallery {
                    clicks: 0,
                    last: "nothing yet",
                    field: TextField::default(),
                    field_focus: cx.focus_handle(),
                    segment: 0,
                    shuffle: false,
                    notes_open: false,
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
