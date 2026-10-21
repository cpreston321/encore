//! Layout helpers for scrolling pages of tiles and for mapping the mouse onto an element.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Edges, ElasticScrolling, InteractiveElement, IntoElement, OverlayScrollbar, Pixels, Styled, canvas,
};

use crate::Theme;

/// Without this, gpui maps a horizontal swipe onto a vertical-only container (and vice versa),
/// so swiping a carousel sideways would also scroll the page.
pub fn lock_scroll_axis<E: Styled>(mut element: E) -> E {
    element.style().restrict_scroll_to_axis = Some(true);
    element
}

/// Gives a scroll view a macOS overlay scrollbar in the theme's colors, kept `insets` clear of
/// bars floating over its edges.
pub fn overlay_scrollbar<E: InteractiveElement>(element: E, theme: &Theme, insets: Edges<Pixels>) -> E {
    element.overlay_scrollbar_style(OverlayScrollbar { color: Some(theme.scrollbar_thumb()), insets })
}

/// Rubber-band overscroll, off when Reduce Motion is on. Call once at startup.
pub fn init_scrolling(cx: &mut App) {
    cx.set_global(ElasticScrolling(!crate::anim::reduce_motion()));
}

/// Records an element's laid-out bounds so mouse positions can be mapped onto it. Add it as
/// a child of a `relative()` element.
pub fn capture_bounds(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {}).absolute().size_full()
}

/// The column count and whole-point tile width that fill `width`, less `padding` on each side,
/// with tiles at least `min_tile` wide and `gap` apart.
pub fn grid_layout(width: f32, min_tile: f32, max_cols: usize, padding: f32, gap: f32) -> (usize, f32) {
    let avail = (width - 2. * padding - 2.).max(min_tile);
    let cols = (((avail + gap) / (min_tile + gap)).floor() as usize).clamp(1, max_cols.max(1));
    let tile = (avail - gap * (cols as f32 - 1.)) / cols as f32;
    (cols, tile.floor())
}

/// `1395` → `1,395`.
pub fn group_digits(n: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_digits() {
        assert_eq!(group_digits(7), "7");
        assert_eq!(group_digits(1395), "1,395");
        assert_eq!(group_digits(1_234_567), "1,234,567");
    }

    #[test]
    fn overscroll_stretches_like_appkit() {
        let d = 600.;
        for x in [1., 50., 300., 2000.] {
            let expected = (1. - 1. / (x * 0.55 / d + 1.)) * d;
            assert!((gpui::rubber_band(x, d) - expected).abs() < 1e-3);
            assert!((gpui::rubber_band(-x, d) + expected).abs() < 1e-3);
            assert!((gpui::rubber_band_inverse(expected, d) - x).abs() < 0.01 * x);
        }
        assert!(gpui::rubber_band(1e6, d) < d);
    }

    #[test]
    fn overscroll_springs_back_in_about_half_a_second() {
        let mut spring = gpui::Spring { position: 90., velocity: 0. };
        let mut frames = 0;
        while !spring.is_settled() {
            spring.step(1. / 120.);
            assert!(spring.position >= 0., "overshot the edge");
            frames += 1;
        }
        assert!((30..100).contains(&frames), "{frames} frames");
    }

    #[test]
    fn grid_fills_width() {
        let (cols, tile) = grid_layout(1000., 160., 8, 32., 20.);
        assert_eq!(cols, 5);
        assert!(cols as f32 * tile + (cols - 1) as f32 * 20. <= 1000. - 64.);
        assert_eq!(grid_layout(100., 160., 8, 32., 20.).0, 1);
    }
}
