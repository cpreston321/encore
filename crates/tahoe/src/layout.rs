//! Layout helpers for scrolling pages of tiles and for mapping the mouse onto an element.

use std::{cell::Cell, rc::Rc};

use gpui::{Bounds, IntoElement, Pixels, Styled, canvas};

/// Without this, gpui maps a horizontal swipe onto a vertical-only container (and vice versa),
/// so swiping a carousel sideways would also scroll the page.
pub fn lock_scroll_axis<E: Styled>(mut element: E) -> E {
    element.style().restrict_scroll_to_axis = Some(true);
    element
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
    fn grid_fills_width() {
        let (cols, tile) = grid_layout(1000., 160., 8, 32., 20.);
        assert_eq!(cols, 5);
        assert!(cols as f32 * tile + (cols - 1) as f32 * 20. <= 1000. - 64.);
        assert_eq!(grid_layout(100., 160., 8, 32., 20.).0, 1);
    }
}
