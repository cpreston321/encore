//! Styled building blocks that return plain gpui elements, so callers can keep chaining
//! (`.on_click(...)`, `.shadow_lg()`, sizing). The components in [`crate::components`] are
//! built on these; use whichever reads better at the call site.

use gpui::{
    Div, ElementId, Fill, FontWeight, GlassStyle, Hsla, InteractiveElement, ParentElement, Role, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div, px, rgb, white,
};

use crate::{
    icons::{icon, icon_label},
    theme::{Theme, over, readable_on},
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
    let label = label.into();
    div()
        .id(id)
        .accessibility_role(Role::Button)
        .accessibility_label(label.clone())
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
        .child(label)
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

/// A borderless round button that shows a fill on hover, for toolbars and rows. VoiceOver
/// reads it by its icon (see [`icon_label`]) unless `.accessibility_label(..)` says more.
pub fn icon_button(t: &Theme, id: impl Into<ElementId>, name: &'static str, size: f32, color: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .accessibility_role(Role::Button)
        .accessibility_label(icon_label(name))
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
        .accessibility_role(Role::Button)
        .accessibility_label(icon_label(icon_name))
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

/// Glass for a small control: a lighter blur, and refraction scaled to its size.
pub fn control_glass(size: f32) -> GlassStyle {
    GlassStyle { blur: px(10.), refraction: px((size * 0.22).clamp(4., 10.)), ..GlassStyle::default() }
}

/// A round Liquid Glass button for floating over artwork or photos, with a white icon.
pub fn glass_button(id: impl Into<ElementId>, icon_name: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .accessibility_role(Role::Button)
        .accessibility_label(icon_label(icon_name))
        .size(px(size))
        .flex_none()
        .rounded_full()
        // With Reduce Transparency, a dark gray that the white icon reads on.
        .glass_style(GlassStyle { fallback: Some(rgb(0x48484a).into()), ..control_glass(size) })
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

/// [`toolbar_button`] in Liquid Glass, for toolbars that float over scrolling content.
pub fn floating_toolbar_button(
    t: &Theme,
    id: impl Into<ElementId>,
    icon_name: &'static str,
    size: f32,
) -> Stateful<Div> {
    toolbar_button(t, id, icon_name, size)
        .glass_style(GlassStyle { fallback: Some(over(t.button, t.window)), ..control_glass(size) })
        .bg(Hsla { a: 0.5, ..t.glass })
        .hover(|s| s.bg(Hsla { a: 0.7, ..t.glass }))
}

/// The round, translucent toolbar button of macOS Tahoe: a soft fill and hairline border
/// in the text color, so it works on plain and tinted backgrounds alike.
pub fn toolbar_button(t: &Theme, id: impl Into<ElementId>, icon_name: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .accessibility_role(Role::Button)
        .accessibility_label(icon_label(icon_name))
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

/// A small rounded toggle, filled with the accent when on, like shuffle and repeat.
pub fn toggle_button(t: &Theme, id: impl Into<ElementId>, icon_name: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .accessibility_role(Role::ToggleButton)
        .accessibility_label(icon_label(icon_name))
        .accessibility_toggled(on)
        .w(px(34.))
        .h(px(26.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .bg(if on { t.accent } else { t.button })
        .hover(|s| s.opacity(0.85))
        .child(icon(icon_name, 13., if on { t.on_accent() } else { t.icon }))
}

/// A keyboard key or shortcut drawn as a key cap, like `⌘K`.
pub fn kbd(t: &Theme, keys: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .py_0p5()
        .rounded(px(5.))
        .bg(t.button)
        .border_1()
        .border_color(t.separator)
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_secondary)
        .child(keys.into())
}

/// A centered title and hint for a list or page with nothing in it.
pub fn empty_state(t: &Theme, title: impl Into<SharedString>, hint: impl Into<SharedString>) -> Div {
    div()
        .pt(px(100.))
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(
            div()
                .accessibility_role(Role::Heading)
                .text_size(px(18.))
                .font_weight(FontWeight::BOLD)
                .child(title.into()),
        )
        .child(div().text_color(t.text_secondary).child(hint.into()))
}

/// Up to two initials on a grey circle, the fallback for a missing profile photo.
pub fn initials_avatar(name: &str, size: f32) -> Div {
    let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(crate::theme::gradient(0x8e8e93, 0x3a3a3c))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.36))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(white())
        .child(initials.to_uppercase())
}
