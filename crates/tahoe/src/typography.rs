//! Tracking and shadows for titles, so large text sits as tight as it does in Apple's apps.

use gpui::{Pixels, TextShadow, black, point, px};

/// Tracking in points by font size, interpolated between rows. CoreText already tightens the
/// system font with size, as AppKit does, so this is extra tightening for large titles on top
/// of it, and body sizes get none.
const TRACKING: [(f32, f32); 4] = [(13., 0.), (19., -0.45), (22., -0.45), (28., -0.4)];

/// The tracking for a title at `font_size` points, for `.tracking(..)`. Past the table it
/// scales with the size, keeping the same proportion of the em as at 28pt.
pub fn tracking(font_size: f32) -> Pixels {
    let (first, last) = (TRACKING[0], TRACKING[TRACKING.len() - 1]);
    if font_size <= first.0 {
        return px(0.);
    }
    if font_size >= last.0 {
        return px(last.1 * font_size / last.0);
    }
    let upper = TRACKING.iter().position(|&(size, _)| size >= font_size).unwrap_or(TRACKING.len() - 1);
    let ((size_a, tracking_a), (size_b, tracking_b)) = (TRACKING[upper - 1], TRACKING[upper]);
    px(tracking_a + (tracking_b - tracking_a) * (font_size - size_a) / (size_b - size_a))
}

/// A soft shadow that keeps white text legible on artwork without looking embossed.
pub fn text_shadow_on_art() -> TextShadow {
    TextShadow { color: black().opacity(0.35), offset: point(px(0.), px(1.)), blur_radius: px(4.) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pixels, b: f32) -> bool {
        (f32::from(a) - b).abs() < 1e-4
    }

    #[test]
    fn matches_the_table_at_its_rows() {
        assert!(close(tracking(13.), 0.));
        assert!(close(tracking(19.), -0.45));
        assert!(close(tracking(20.), -0.45));
        assert!(close(tracking(22.), -0.45));
        assert!(close(tracking(28.), -0.4));
    }

    #[test]
    fn interpolates_between_rows_and_scales_past_them() {
        assert!(close(tracking(11.), 0.));
        assert!(close(tracking(16.), -0.225));
        assert!(close(tracking(25.), -0.425));
        assert!(close(tracking(56.), -0.8));
    }
}
