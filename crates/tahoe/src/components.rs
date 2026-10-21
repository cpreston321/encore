//! Tahoe-style components as builders, in the style of Zed's `ui` crate:
//!
//! ```ignore
//! Button::new("play", "Play").icon("play").on_click(cx.listener(|this, _, _, cx| this.play(cx)))
//! ```
//!
//! Each takes the light or dark theme for its window unless `.theme(..)` sets one, such as a
//! page tinted by artwork (`Theme::tinted`).

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    Refineable, RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder, px, white,
};

use crate::{
    icons::icon,
    primitives,
    theme::{Theme, readable_on},
};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn theme_for(theme: Option<Theme>, window: &Window) -> Theme {
    theme.unwrap_or_else(|| Theme::for_appearance(window.appearance()))
}

/// How a [`Button`] is filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    /// The accent color, for the main action.
    #[default]
    Filled,
    /// A soft fill with an accent label, for secondary actions.
    Tinted,
    /// Frosted white, over artwork and photos.
    Glass,
    /// No fill until hovered.
    Plain,
}

/// A capsule button with a label and an optional leading icon.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<&'static str>,
    style: ButtonStyle,
    theme: Option<Theme>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            style: ButtonStyle::default(),
            theme: None,
            on_click: None,
        }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let t = theme_for(self.theme, window);
        let (bg, fg): (Hsla, Hsla) = match self.style {
            ButtonStyle::Filled => (t.accent, readable_on(t.accent, white())),
            ButtonStyle::Tinted => (t.button, t.accent),
            ButtonStyle::Glass => (white().opacity(0.16), white()),
            ButtonStyle::Plain => (gpui::transparent_black(), t.accent),
        };
        let el = div()
            .id(self.id)
            .min_w(px(96.))
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
            .cursor_pointer()
            .when(self.style == ButtonStyle::Glass, |d| d.border_1().border_color(white().opacity(0.22)))
            .when(self.style == ButtonStyle::Plain, move |d| d.hover(move |s| s.bg(t.hover)))
            .when(self.style != ButtonStyle::Plain, |d| d.hover(|s| s.opacity(0.85)))
            .active(|s| s.opacity(0.7))
            .when_some(self.icon, |d, name| d.child(icon(name, 12., fg)))
            .child(self.label);
        match self.on_click {
            Some(f) => el.on_click(f),
            None => el,
        }
    }
}

/// How an [`IconButton`] is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum IconButtonStyle {
    /// The translucent round toolbar button of macOS Tahoe.
    #[default]
    Toolbar,
    /// Frosted white, over artwork and photos.
    Glass,
    /// A solid circle, like a play button. The icon picks dark or white to contrast.
    Filled(Hsla),
    /// No fill until hovered.
    Plain,
}

/// A round button holding a single icon.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: &'static str,
    size: f32,
    style: IconButtonStyle,
    theme: Option<Theme>,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: &'static str) -> Self {
        Self { id: id.into(), icon, size: 32., style: IconButtonStyle::default(), theme: None, on_click: None }
    }

    /// The diameter in points (32 by default).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn style(mut self, style: IconButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let t = theme_for(self.theme, window);
        let el = match self.style {
            IconButtonStyle::Toolbar => primitives::toolbar_button(&t, self.id, self.icon, self.size),
            IconButtonStyle::Glass => primitives::glass_button(self.id, self.icon, self.size),
            IconButtonStyle::Filled(bg) => primitives::circle_button(self.id, self.icon, self.size, bg, white()),
            IconButtonStyle::Plain => {
                primitives::icon_button(&t, self.id, self.icon, self.size * 0.45, t.icon).size(px(self.size))
            }
        }
        .cursor_pointer();
        match self.on_click {
            Some(f) => el.on_click(f),
            None => el,
        }
    }
}

/// Inline text that underlines and brightens on hover, and does something when clicked.
///
/// The hover look is applied when rendering, from state kept for the element: gpui applies
/// `.hover()` styles at paint, after text is laid out, so an underline or color set there
/// wouldn't reach the glyphs.
#[derive(IntoElement)]
pub struct Link {
    id: ElementId,
    text: SharedString,
    color: Option<Hsla>,
    hover_color: Option<Hsla>,
    theme: Option<Theme>,
    on_click: Option<ClickHandler>,
}

impl Link {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self { id: id.into(), text: text.into(), color: None, hover_color: None, theme: None, on_click: None }
    }

    /// The resting color (the theme's secondary text by default).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// The hovered color (the theme's text by default).
    pub fn hover_color(mut self, color: Hsla) -> Self {
        self.hover_color = Some(color);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Link {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme_for(self.theme, window);
        let hovered = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let is_hovered = *hovered.read(cx);
        let el = div()
            .id(self.id)
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .text_color(if is_hovered {
                self.hover_color.unwrap_or(t.text)
            } else {
                self.color.unwrap_or(t.text_secondary)
            })
            .when(is_hovered, |d| d.underline())
            .on_hover(move |over, _, cx| {
                hovered.update(cx, |state, cx| {
                    if *state != *over {
                        *state = *over;
                        cx.notify();
                    }
                })
            })
            .child(self.text);
        match self.on_click {
            Some(f) => el.on_click(move |event, window, cx| {
                cx.stop_propagation();
                f(event, window, cx)
            }),
            None => el,
        }
    }
}

/// A small square letter badge, like the "E" for explicit content.
#[derive(IntoElement)]
pub struct Badge {
    letter: SharedString,
    theme: Option<Theme>,
}

impl Badge {
    pub fn new(letter: impl Into<SharedString>) -> Self {
        Self { letter: letter.into(), theme: None }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        primitives::badge(&theme_for(self.theme, window), self.letter)
    }
}

/// A section title. Given `on_click`, it shows a chevron and opens on click, so a chevron
/// always means there's more to see.
#[derive(IntoElement)]
pub struct SectionHeader {
    id: ElementId,
    title: SharedString,
    inset: f32,
    theme: Option<Theme>,
    on_click: Option<ClickHandler>,
}

impl SectionHeader {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self { id: id.into(), title: title.into(), inset: 32., theme: None, on_click: None }
    }

    /// Horizontal padding in points (32 by default).
    pub fn inset(mut self, inset: f32) -> Self {
        self.inset = inset;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let t = theme_for(self.theme, window);
        let title = div().text_size(px(19.)).font_weight(FontWeight::BOLD).child(self.title);
        let row = div().px(px(self.inset)).pt_7().pb_2p5().flex();
        match self.on_click {
            None => row.child(title),
            Some(f) => row.child(
                div()
                    .id(self.id)
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.7))
                    .active(|s| s.opacity(0.5))
                    .child(title)
                    .child(icon("chevron-right", 14., t.text_secondary))
                    .on_click(f),
            ),
        }
    }
}

/// A page's large title, with a Tahoe toolbar back button beside it when `on_back` is set.
#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    inset: f32,
    theme: Option<Theme>,
    on_back: Option<ClickHandler>,
}

impl PageHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), inset: 32., theme: None, on_back: None }
    }

    /// Horizontal padding in points (32 by default).
    pub fn inset(mut self, inset: f32) -> Self {
        self.inset = inset;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn on_back(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_back = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for PageHeader {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let t = theme_for(self.theme, window);
        div()
            .px(px(self.inset))
            .pt(px(42.))
            .pb_4()
            .flex()
            .items_center()
            .gap_3()
            .when_some(self.on_back, |d, f| {
                d.child(primitives::toolbar_button(&t, "page-back", "chevron-left", 32.).on_click(f))
            })
            .child(div().min_w_0().truncate().text_size(px(28.)).font_weight(FontWeight::BOLD).child(self.title))
    }
}

/// A rounded surface with a hairline border and a soft shadow. Children and styles work as
/// on a `div`.
#[derive(IntoElement)]
pub struct Card {
    base: Div,
    theme: Option<Theme>,
}

impl Card {
    pub fn new() -> Self {
        Self { base: div(), theme: None }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements)
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Card {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let t = theme_for(self.theme, window);
        let mut defaults =
            div().rounded(px(16.)).overflow_hidden().bg(t.panel).border_1().border_color(t.panel_border).shadow_md();
        // The card's look goes underneath, so styles the caller set on it win.
        let mut base = self.base;
        let caller = base.style().clone();
        *base.style() = defaults.style().clone();
        base.style().refine(&caller);
        base
    }
}
