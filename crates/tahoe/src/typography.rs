//! Text treatments for Tahoe surfaces. The system font needs no extra tracking: CoreText already
//! tightens it with size, as AppKit does.

use gpui::{TextShadow, black, point, px};

/// A soft shadow that keeps white text legible on artwork without looking embossed.
pub fn text_shadow_on_art() -> TextShadow {
    TextShadow { color: black().opacity(0.35), offset: point(px(0.), px(1.)), blur_radius: px(4.) }
}
