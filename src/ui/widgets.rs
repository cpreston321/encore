//! Small stateless building blocks: icons, buttons, headers, placeholders, layout helpers.

use super::*;

pub(crate) use tahoe::{
    icon,
    primitives::{button, icon_button, pill_button},
};

/// Small rounded toggle, filled with the accent when on (shuffle / repeat).
pub(crate) fn toggle_chip(t: &Theme, id: &'static str, icon_name: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(34.))
        .h(px(26.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .bg(if on { t.accent } else { t.button })
        .hover(|s| s.opacity(0.85))
        .child(icon(icon_name, 13., if on { white() } else { t.icon }))
}

/// `1395` → `1,395`.
pub(crate) fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn explicit_badge(t: &Theme) -> impl IntoElement {
    tahoe::primitives::badge(t, "E")
}

pub(crate) fn table_header(t: &Theme) -> impl IntoElement {
    let header_cell = |label: &'static str| {
        div().text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(t.text_secondary).child(label)
    };
    div()
        .px_2()
        .h(px(28.))
        .flex()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(t.separator)
        .child(header_cell("Title").flex_1().pl(px(44.)))
        .child(header_cell("Artist").w(relative(0.22)))
        .child(header_cell("Album").w(relative(0.22)))
        .child(div().w(px(44.)).flex().justify_end().child(icon("clock", 11., t.text_secondary)))
        .child(div().w(px(26.)))
}

pub(crate) fn page_title(title: impl Into<SharedString>) -> impl IntoElement {
    let title = title.into();
    div().px(px(PAD)).pt(px(46.)).pb_4().text_size(px(28.)).font_weight(FontWeight::BOLD).child(title)
}

/// A section title with nothing more to show. Sections that open a fuller page use
/// `MusicApp::section_link`, which adds the chevron.
pub(crate) fn section_header(t: &Theme, title: &'static str) -> impl IntoElement {
    tahoe::SectionHeader::new(title, title).inset(PAD).theme(*t)
}

pub(crate) fn loading(t: &Theme) -> Div {
    div().pt(px(160.)).flex().justify_center().text_color(t.text_secondary).child(
        div().child("Loading…").with_animation(
            "loading-pulse",
            Animation::new(Duration::from_millis(1200)).repeat().with_easing(pulsating_between(0.35, 1.)),
            |el, p| el.opacity(p),
        ),
    )
}

/// Placeholder for Home's personal shelves: a section of 4 song rows in two columns, then a
/// row of 160pt cards, matching the loaded layout's height.
pub(crate) fn skeleton_home_shelves(t: &Theme) -> impl IntoElement {
    let block = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(r)).bg(t.placeholder);
    let header = || div().px(px(PAD)).pt_7().pb_2p5().h(px(28. + 10. + 28.)).child(block(170., 20., 5.));
    let row = || {
        div()
            .h(px(60.))
            .flex()
            .items_center()
            .gap_3()
            .border_t_1()
            .border_color(t.separator)
            .child(block(44., 44., 5.))
            .child(div().flex().flex_col().gap_1p5().child(block(150., 11., 4.)).child(block(90., 9., 4.)))
    };
    let column = || div().flex_1().flex().flex_col().children((0..4).map(|_| row()));
    let card = || {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(block(160., 160., 8.))
            .child(block(120., 11., 4.))
            .child(block(80., 9., 4.))
    };
    div()
        .child(header())
        .child(div().px(px(PAD)).flex().gap(px(GAP)).child(column()).child(column()))
        .child(header())
        .child(div().px(px(PAD)).pt_1().flex().gap(px(GAP)).children((0..6).map(|_| card())))
        .with_animation(
            "home-skeleton-pulse",
            Animation::new(Duration::from_millis(1300)).repeat().with_easing(pulsating_between(0.5, 1.)),
            |el, p| el.opacity(p),
        )
}

/// Placeholder layout shown while a browse page loads, gently pulsing.
pub(crate) fn skeleton(t: &Theme) -> impl IntoElement {
    let block = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(r)).bg(t.placeholder);
    let tiles = |n: usize, size: f32| {
        div().px(px(PAD)).flex().gap(px(GAP)).children((0..n).map(move |_| {
            div().flex().flex_col().gap_2().child(block(size, size, 8.)).child(block(size * 0.7, 12., 4.)).child(block(
                size * 0.45,
                10.,
                4.,
            ))
        }))
    };
    div()
        .child(div().px(px(PAD)).pt(px(52.)).pb_5().child(block(140., 28., 6.)))
        .child(div().px(px(PAD)).flex().gap(px(GAP)).child(block(480., 270., 12.)).child(block(480., 270., 12.)))
        .child(div().px(px(PAD)).pt_8().pb_3().child(block(180., 20., 5.)))
        .child(tiles(6, 170.))
        .with_animation(
            "skeleton-pulse",
            Animation::new(Duration::from_millis(1300)).repeat().with_easing(pulsating_between(0.5, 1.)),
            |el, p| el.opacity(p),
        )
}

fn pulse<E: IntoElement + Styled + 'static>(id: &'static str, el: E) -> impl IntoElement {
    el.with_animation(
        id,
        Animation::new(Duration::from_millis(1300)).repeat().with_easing(pulsating_between(0.5, 1.)),
        |el, p| el.opacity(p),
    )
}

/// Placeholder song rows: art square, two text lines, duration.
pub(crate) fn skeleton_rows(t: &Theme, n: usize) -> impl IntoElement {
    let block = |w: f32, h: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(4.)).bg(t.placeholder);
    pulse(
        "rows-pulse",
        div().px(px(PAD)).pt_3().flex().flex_col().children((0..n).map(move |i| {
            div()
                .h(px(52.))
                .flex()
                .items_center()
                .gap_3()
                .border_t_1()
                .border_color(t.separator)
                .child(block(36., 36.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(block(160. + (i % 3) as f32 * 40., 11.))
                        .child(block(90. + (i % 2) as f32 * 30., 9.)),
                )
                .child(block(32., 10.))
        })),
    )
}

/// Placeholder album/playlist page: centered cover, title lines, buttons, rows.
pub(crate) fn skeleton_detail(t: &Theme) -> impl IntoElement {
    let block = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(r)).bg(t.placeholder);
    div()
        .child(pulse(
            "detail-pulse",
            div()
                .pt(px(72.))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(block(300., 300., 10.))
                .child(div().pt_3().child(block(260., 24., 6.)))
                .child(block(150., 18., 5.))
                .child(block(110., 11., 4.))
                .child(
                    div()
                        .pt_3()
                        .flex()
                        .gap_4()
                        .child(block(48., 48., 24.))
                        .child(block(220., 48., 24.))
                        .child(block(48., 48., 24.)),
                ),
        ))
        .child(skeleton_rows(t, 6))
}

/// Overlay entrance: short and decelerating, so it feels responsive rather than slow.
pub(crate) fn pop_in() -> Animation {
    Animation::new(Duration::from_millis(170)).with_easing(ease_out_quint())
}

/// Fades and lifts each new page into place. Keyed by navigation count so it replays.
pub(crate) fn page_transition(
    generation: usize,
    direction: crate::app::NavDirection,
    fill: bool,
    page: AnyElement,
) -> impl IntoElement {
    use crate::app::NavDirection;
    let ms = if crate::anim::reduce_motion() { 1 } else { 280 };
    div().when(fill, |d| d.size_full()).pl(px(SIDEBAR_W)).child(page).with_animation(
        ("page-transition", generation),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quint()),
        move |el, p| {
            let el = el.opacity(p);
            match direction {
                NavDirection::Root => el.mt(px((1. - p) * 14.)),
                // Detail pages arrive from the right; going back, the previous page from the left.
                NavDirection::Forward => el.ml(px((1. - p) * 28.)),
                NavDirection::Back => el.ml(px((1. - p) * -28.)),
            }
        },
    )
}

pub(crate) fn empty_state(t: &Theme, title: &'static str, hint: &'static str) -> impl IntoElement {
    div()
        .pt(px(100.))
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child(title))
        .child(div().text_color(t.text_secondary).child(hint))
}

pub(crate) fn gradient(from: u32, to: u32) -> gpui::Background {
    linear_gradient(135., linear_color_stop(rgb(from), 0.), linear_color_stop(rgb(to), 1.))
}

pub(crate) fn art_overlay(size: f32, child: Svg) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size(px(size))
        .rounded(px(5.))
        .bg(gpui::black().opacity(0.45))
        .flex()
        .items_center()
        .justify_center()
        .child(child)
}

pub(crate) fn scroll_row(id: &'static str) -> Stateful<Div> {
    div().id(id).px(px(PAD)).pb_2().flex().gap(px(GAP)).overflow_x_scroll().map(lock_scroll_axis)
}

pub(crate) fn grid_container() -> Div {
    div().px(px(PAD)).flex().flex_wrap().gap_x(px(GAP)).gap_y(px(26.))
}

/// Without this, gpui maps a horizontal swipe onto a vertical-only container (and vice versa),
/// so swiping a carousel sideways would also scroll the page.
pub(crate) fn lock_scroll_axis<E: Styled>(mut element: E) -> E {
    element.style().restrict_scroll_to_axis = Some(true);
    element
}

/// Records an element's laid-out bounds so mouse positions can be mapped onto it.
pub(crate) fn capture_bounds(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {}).absolute().size_full()
}

/// Columns and tile width that fill `width` with tiles at least `min_tile` wide.
pub(crate) fn grid_layout(width: f32, min_tile: f32, max_cols: usize) -> (usize, f32) {
    let avail = (width - 2. * PAD - 2.).max(min_tile);
    let cols = (((avail + GAP) / (min_tile + GAP)).floor() as usize).clamp(1, max_cols.max(1));
    let tile = (avail - GAP * (cols as f32 - 1.)) / cols as f32;
    (cols, tile.floor())
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_digits() {
        assert_eq!(super::group_digits(7), "7");
        assert_eq!(super::group_digits(1395), "1,395");
        assert_eq!(super::group_digits(1_234_567), "1,234,567");
    }
}
