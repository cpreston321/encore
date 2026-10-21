//! The keyboard shortcuts sheet (⌘/).

use gpui::{AnyElement, Context, FontWeight, MouseButton, div, prelude::*, px};

use crate::{app::MusicApp, theme::Theme};

impl MusicApp {
    pub(crate) fn render_shortcuts(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().shortcuts;
        if !presence.visible() {
            return None;
        }
        let p = presence.value();
        let groups: [(&str, &[(&str, &str)]); 4] = [
            (
                "Playback",
                &[
                    ("Space", "Play / Pause"),
                    ("⌘→", "Next song"),
                    ("⌘←", "Previous song"),
                    ("⌘↑ / ⌘↓", "Volume"),
                    ("⌘L", "Go to current song"),
                    ("⌘⌥U", "Playing Next"),
                    ("⌃⌘F", "Now Playing"),
                    ("⇧⌘L", "Lyrics"),
                    ("⇧⌘M", "Mini Player"),
                ],
            ),
            (
                "Navigation",
                &[
                    ("⌘1 – ⌘9", "Home … Favorite Songs"),
                    ("⌘[ / ⌘]", "Back / Forward"),
                    ("↑ / ↓", "Move through songs"),
                    ("Return", "Play selected song"),
                    ("Esc", "Close"),
                ],
            ),
            (
                "Search",
                &[
                    ("⌘K", "Quick search"),
                    ("⌘F", "Search page"),
                    ("⌥⌫ / ⌘⌫", "Delete word / all"),
                    ("↓", "Into results"),
                ],
            ),
            ("Library", &[("⌘R", "Refresh library"), ("⌘/", "This list")]),
        ];
        let mut columns = div().flex().flex_wrap().gap_x_8().gap_y_6();
        for (title, rows) in groups {
            let mut col = div().w(px(250.)).flex().flex_col().gap_2().child(
                div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text_tertiary).child(title),
            );
            for (keys, what) in rows {
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().text_color(t.text_secondary).child(what.to_string()))
                        .child(tahoe::primitives::kbd(t, *keys).text_size(px(11.5)).text_color(t.text)),
                );
            }
            columns = columns.child(col);
        }
        Some(
            div()
                .id("shortcuts-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.35 * p))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.show_shortcuts = false;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .id("shortcuts")
                        .w(px(600.))
                        .p_7()
                        .rounded(px(18.))
                        .glass_style(crate::ui::panel_glass())
                        .bg(t.glass)
                        .border_1()
                        .border_color(t.panel_border)
                        .shadow_lg()
                        .opacity(p)
                        .mt(px((1. - p) * 10.))
                        .flex()
                        .flex_col()
                        .gap_5()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child("Keyboard Shortcuts"))
                        .child(columns),
                )
                .into_any_element(),
        )
    }
}
