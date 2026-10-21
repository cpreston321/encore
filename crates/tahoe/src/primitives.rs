//! Styled building blocks that return plain gpui elements, so callers can keep chaining
//! (`.on_click(...)`, `.shadow_lg()`, sizing). The components in [`crate::components`] are
//! built on these; use whichever reads better at the call site.

use gpui::{
    Div, ElementId, Fill, FontWeight, Hsla, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, px, white,
};

use crate::{
    icons::icon,
    theme::{Theme, readable_on},
};

/// A capsule button with an icon and a label. The icon follows the label color.
pub fn button(
    _t: &Theme,
    id: impl Into<ElementId>,
    icon_name: &'static str,
    label: impl Into<SharedString>,
    bg: Fill,
    fg: Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_w(px(112.))
        .px_4()
        .h(px(32.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .gap_1p5()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .bg(bg)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.7))
        .child(icon(icon_name, 12., fg))
        .child(label.into())
}

/// A capsule button in the accent (`filled`) or the quieter tinted fill. The filled label
/// switches to dark when the accent is too light for white.
pub fn pill_button(
    t: &Theme,
    id: impl Into<ElementId>,
    icon_name: &'static str,
    label: impl Into<SharedString>,
    filled: bool,
) -> Stateful<Div> {
    if filled {
        button(t, id, icon_name, label, t.accent.into(), readable_on(t.accent, white()))
    } else {
        button(t, id, icon_name, label, t.button.into(), t.accent)
    }
}

/// A borderless round button that shows a fill on hover, for toolbars and rows.
pub fn icon_button(t: &Theme, id: impl Into<ElementId>, name: &'static str, size: f32, color: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(30.))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(t.hover))
        .active(|s| s.opacity(0.6))
        .child(icon(name, size, color))
}

/// A filled circle with an icon, like a play button. `fg` switches to dark or white when it
/// doesn't contrast with `bg`.
pub fn circle_button(
    id: impl Into<ElementId>,
    icon_name: &'static str,
    size: f32,
    bg: Hsla,
    fg: Hsla,
) -> Stateful<Div> {
    let fg = readable_on(bg, fg);
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(bg)
        .hover(|s| s.opacity(0.88))
        .active(|s| s.opacity(0.72))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(icon_name, size * 0.38, fg))
}

/// A frosted round button for floating over artwork or photos.
pub fn glass_button(id: impl Into<ElementId>, icon_name: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(white().opacity(0.16))
        .border_1()
        .border_color(white().opacity(0.22))
        .hover(|s| s.bg(white().opacity(0.26)))
        .active(|s| s.opacity(0.75))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(icon_name, size * 0.42, white()))
}

/// The round, translucent toolbar button of macOS Tahoe: a soft fill and hairline border
/// in the text color, so it works on plain and tinted backgrounds alike.
pub fn toolbar_button(t: &Theme, id: impl Into<ElementId>, icon_name: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(t.text.opacity(0.07))
        .border_1()
        .border_color(t.text.opacity(0.1))
        .shadow_sm()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(t.text.opacity(0.13)))
        .active(|s| s.bg(t.text.opacity(0.18)))
        .child(icon(icon_name, size * 0.4, t.text))
}

/// A small square letter badge, like the "E" for explicit content.
pub fn badge(t: &Theme, letter: impl Into<SharedString>) -> Div {
    div()
        .size(px(13.))
        .flex_none()
        .rounded(px(2.5))
        .bg(t.text_tertiary)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(9.))
        .font_weight(FontWeight::BOLD)
        .text_color(t.window)
        .child(letter.into())
}

/// A rounded placeholder block for content that's still loading.
pub fn skeleton_block(t: &Theme, w: f32, h: f32, radius: f32) -> Div {
    div().w(px(w)).h(px(h)).flex_none().rounded(px(radius)).bg(t.placeholder)
}
