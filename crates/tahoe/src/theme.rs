//! Color tokens for Tahoe-style interfaces: a light and a dark theme, palettes for surfaces
//! tinted by artwork, and WCAG contrast helpers that keep text and icons readable on them.

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{Hsla, Rgba, WindowAppearance, rgb, rgba};

/// Semantic colors. `window` is the page background; `pill` floating bars and toolbars;
/// `glass` the fill of glass menus, popovers and sheets; `button` a tinted control's fill;
/// `accent` filled controls and selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub window: Hsla,
    pub panel: Hsla,
    pub panel_border: Hsla,
    pub pill: Hsla,
    pub menu: Hsla,
    pub glass: Hsla,
    pub placeholder: Hsla,
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub icon: Hsla,
    pub separator: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub row_alt: Hsla,
    pub field: Hsla,
    pub button: Hsla,
    pub accent: Hsla,
    pub track_bg: Hsla,
    pub track_fill: Hsla,
}

/// Forces every `Theme::for_appearance` to one appearance, regardless of the system setting,
/// for checking both without changing it. Call before the first window opens.
pub fn force_appearance(appearance: Option<Appearance>) {
    FORCED.store(
        match appearance {
            None => 0,
            Some(Appearance::Light) => 1,
            Some(Appearance::Dark) => 2,
        },
        Ordering::Relaxed,
    );
}

static FORCED: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Theme {
    /// The light or dark theme for a window's appearance (`window.appearance()`).
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match FORCED.load(Ordering::Relaxed) {
            1 => return Self::light(),
            2 => return Self::dark(),
            _ => {}
        }
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    /// Text and icon color for anything drawn on the accent, such as a selected row: white,
    /// or near-black when the accent is too light for white (artwork-tinted pages).
    pub fn on_accent(&self) -> Hsla {
        readable_on(self.accent, rgb(0xffffff).into())
    }

    /// An overlay scrollbar's thumb: the text color, translucent, so it reads on light, dark
    /// and artwork-tinted pages alike.
    pub fn scrollbar_thumb(&self) -> Hsla {
        Hsla { a: 0.42, ..self.text }
    }

    pub fn dark() -> Self {
        Self {
            window: rgb(0x1e1e1e).into(),
            panel: rgb(0x282828).into(),
            panel_border: rgba(0xffffff1a).into(),
            pill: rgba(0x2c2c2ef2).into(),
            menu: rgb(0x2c2c2e).into(),
            glass: rgb(0x2c2c2e).into(),
            placeholder: rgba(0xffffff12).into(),
            text: rgba(0xffffffeb).into(),
            text_secondary: rgba(0xffffff8c).into(),
            text_tertiary: rgba(0xffffff4d).into(),
            icon: rgba(0xffffffd9).into(),
            separator: rgba(0xffffff14).into(),
            hover: rgba(0xffffff0f).into(),
            selected: rgba(0xffffff1a).into(),
            row_alt: rgba(0xffffff08).into(),
            field: rgba(0xffffff12).into(),
            button: rgba(0xffffff17).into(),
            accent: rgb(0xfa2d48).into(),
            track_bg: rgba(0xffffff26).into(),
            track_fill: rgba(0xffffffb3).into(),
        }
        .with_glass()
    }

    pub fn light() -> Self {
        Self {
            window: rgb(0xffffff).into(),
            panel: rgb(0xf3f3f5).into(),
            panel_border: rgba(0x0000001a).into(),
            pill: rgba(0xf8f8faf2).into(),
            menu: rgb(0xf6f6f6).into(),
            glass: rgb(0xf6f6f6).into(),
            placeholder: rgba(0x0000000d).into(),
            text: rgba(0x000000e0).into(),
            text_secondary: rgba(0x0000008c).into(),
            text_tertiary: rgba(0x00000047).into(),
            icon: rgba(0x000000c7).into(),
            separator: rgba(0x00000014).into(),
            hover: rgba(0x0000000a).into(),
            selected: rgba(0x00000012).into(),
            row_alt: rgba(0x00000006).into(),
            field: rgba(0x0000000d).into(),
            button: rgba(0x0000000f).into(),
            accent: rgb(0xfa233b).into(),
            track_bg: rgba(0x0000001f).into(),
            track_fill: rgba(0x00000099).into(),
        }
        .with_glass()
    }

    fn with_glass(self) -> Self {
        Self { glass: glass_fill(self.menu, self.text, self.text_secondary, GLASS_MIN_ALPHA), ..self }
    }
}

/// Colors for a page tinted by its artwork, like the iOS Music detail pages.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Hsla,
    pub text: Hsla,
    pub text2: Hsla,
    /// Fill for the main play button.
    pub accent: Hsla,
}

impl Palette {
    /// From Apple's own artwork colors, which are picked to read on the background.
    ///
    /// `bg` and `text` are 0xRRGGBB colors, such as an Apple Music artwork's `bgColor` and
    /// `textColor1`–`4`.
    pub fn from_art_colors(bg: u32, text: [u32; 4]) -> Self {
        Self::from_art_colors_raw(bg, text).readable()
    }

    fn from_art_colors_raw(bg: u32, text: [u32; 4]) -> Self {
        let bg: Hsla = rgb(bg).into();
        let texts: Vec<Hsla> = text.iter().map(|&t| rgb(t).into()).collect();
        let accent = texts
            .iter()
            .copied()
            .max_by(|a, b| a.s.total_cmp(&b.s))
            .filter(|a| a.s > 0.3 && (a.l - bg.l).abs() > 0.25)
            .unwrap_or(texts[0]);
        // Apple's backgrounds can be very light; keep page backgrounds dark enough for the
        // app's white chrome (player, menus) while keeping the hue.
        if bg.l > 0.42 {
            // Apple's text colors are dark ones meant for its light background; use light text.
            let bg = Hsla { l: 0.24, s: bg.s.min(0.5), ..bg };
            Self { bg, text: white_ish(0.95), text2: white_ish(0.68), accent }
        } else {
            Self { bg, text: texts[0], text2: texts[1], accent }
        }
    }

    /// Fallback from the artwork's average color.
    pub fn from_average(c: Hsla) -> Self {
        Self::from_average_raw(c).readable()
    }

    fn from_average_raw(c: Hsla) -> Self {
        Self {
            bg: Hsla { s: c.s.min(0.45), l: (c.l * 0.55).clamp(0.11, 0.22), ..c },
            text: white_ish(0.95),
            text2: white_ish(0.68),
            accent: Hsla { s: c.s.clamp(0.45, 0.75), l: 0.56, ..c },
        }
    }

    /// Icon color that reads on the accent fill.
    pub fn on_accent(&self) -> Hsla {
        readable_on(self.accent, white_ish(1.))
    }

    /// Makes sure the colors read on the background: body text at 4.5:1, secondary text at
    /// 3:1, and the accent at 3:1 too, since it's also used for text and selected items.
    fn readable(mut self) -> Self {
        if contrast(self.text, self.bg) < 4.5 {
            self.text = best_of_black_white(self.bg, 0.95);
        }
        if contrast(self.text2, self.bg) < 3. {
            self.text2 = Hsla { a: 0.7, ..self.text };
        }
        if contrast(self.accent, self.bg) < 3. {
            self.accent = self.text;
        }
        self
    }
}

/// WCAG relative luminance: 0 for black, 1 for white. Alpha is ignored.
pub fn luminance(c: Hsla) -> f32 {
    let c: gpui::Rgba = Hsla { a: 1., ..c }.into();
    let linear = |v: f32| if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// WCAG contrast ratio, from 1 (none) to 21 (black on white).
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn best_of_black_white(bg: Hsla, alpha: f32) -> Hsla {
    let black = Hsla { h: 0., s: 0., l: 0.07, a: alpha };
    if contrast(black, bg) > contrast(white_ish(1.), bg) { black } else { white_ish(alpha) }
}

/// `preferred` if it reads on `bg` (3:1, enough for icons and bold labels), otherwise near-black
/// or white, whichever reads better. Artwork-tinted pages can make the accent almost white, so
/// anything drawn on it goes through this.
pub fn readable_on(bg: Hsla, preferred: Hsla) -> Hsla {
    if contrast(preferred, bg) >= 3. { preferred } else { best_of_black_white(bg, preferred.a) }
}

/// `top` drawn over `bottom`, blended in sRGB as the renderer does.
pub fn over(top: Hsla, bottom: Hsla) -> Hsla {
    let (t, b): (Rgba, Rgba) = (top.into(), bottom.into());
    let a = t.a + b.a * (1. - t.a);
    if a <= 0. {
        return Hsla::transparent_black();
    }
    let mix = |x: f32, y: f32| (x * t.a + y * b.a * (1. - t.a)) / a;
    Rgba { r: mix(t.r, b.r), g: mix(t.g, b.g), b: mix(t.b, b.b), a }.into()
}

/// The most translucent a glass fill gets, enough for the blur to show through.
pub const GLASS_MIN_ALPHA: f32 = 0.7;

/// `fill` as translucent as it can be, down to `min_alpha`, while `text` keeps 4.5:1 and
/// `secondary` 3:1 over it whatever the glass blurs. A blur averages its backdrop and the
/// glass lifts it only slightly, so a white or a black backdrop is the worst case.
pub fn glass_fill(fill: Hsla, text: Hsla, secondary: Hsla, min_alpha: f32) -> Hsla {
    let reads = |fill: Hsla| {
        [gpui::white(), gpui::black()].into_iter().all(|backdrop| {
            let bg = over(fill, backdrop);
            contrast(over(text, bg), bg) >= 4.5 && contrast(over(secondary, bg), bg) >= 3.
        })
    };
    let mut alpha = min_alpha.clamp(0., 1.);
    while alpha < 1. && !reads(Hsla { a: alpha, ..fill }) {
        alpha = (alpha + 0.02).min(1.);
    }
    Hsla { a: alpha, ..fill }
}

/// A 135° gradient between two RGB hex colors, the fill for avatars and placeholder art.
pub fn gradient(from: u32, to: u32) -> gpui::Background {
    gpui::linear_gradient(135., gpui::linear_color_stop(rgb(from), 0.), gpui::linear_color_stop(rgb(to), 1.))
}

fn white_ish(alpha: f32) -> Hsla {
    Hsla { h: 0., s: 0., l: 1., a: alpha }
}

impl Theme {
    /// This theme recolored for a page whose background is the palette's tint.
    pub fn tinted(&self, p: Palette) -> Theme {
        let ink = |a: f32| Hsla { a, ..p.text };
        let panel = Hsla { l: (p.bg.l + 0.05).min(1.), ..p.bg };
        Theme {
            window: p.bg,
            panel,
            glass: glass_fill(panel, p.text, p.text2, GLASS_MIN_ALPHA),
            placeholder: ink(0.08),
            text: p.text,
            text_secondary: p.text2,
            text_tertiary: ink(0.45),
            icon: p.text,
            separator: ink(0.13),
            hover: ink(0.07),
            selected: ink(0.12),
            row_alt: ink(0.),
            field: ink(0.10),
            button: ink(0.14),
            accent: p.accent,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_matches_wcag() {
        let (black, white): (Hsla, Hsla) = (rgb(0x000000).into(), rgb(0xffffff).into());
        assert!((contrast(black, white) - 21.).abs() < 0.01);
        assert!((contrast(white, white) - 1.).abs() < 0.01);
    }

    #[test]
    fn readable_on_keeps_good_colors_and_swaps_bad_ones() {
        let white: Hsla = rgb(0xffffff).into();
        // The app's red accent keeps its white play icons.
        for appearance in [WindowAppearance::Dark, WindowAppearance::Light] {
            let accent = Theme::for_appearance(appearance).accent;
            assert_eq!(readable_on(accent, white), white, "{accent:?}");
        }
        // A near-white accent from light artwork gets a dark icon instead.
        let light: Hsla = rgb(0xe8e8e8).into();
        assert!(luminance(readable_on(light, white)) < 0.1);
    }

    #[test]
    fn scrollbar_thumbs_stand_out_from_the_page() {
        let tinted = [0x101010, 0xf4e9d0, 0x2b4d7a, 0xc0392b]
            .map(|bg| Theme::light().tinted(Palette::from_art_colors(bg, [0xffffff, 0xeeeeee, 0x111111, 0x222222])));
        for t in [Theme::light(), Theme::dark()].into_iter().chain(tinted) {
            let thumb = t.scrollbar_thumb();
            assert!(thumb.a > 0.3 && thumb.a < 0.6);
            let solid = Hsla { a: 1., ..thumb };
            assert!(contrast(solid, Hsla { a: 1., ..t.window }) >= 4.5, "{:?} on {:?}", thumb, t.window);
        }
    }

    #[test]
    fn glass_reads_over_white_and_black_art() {
        let tinted = [0x101010, 0xf4e9d0, 0x2b4d7a, 0xc0392b]
            .map(|bg| Theme::dark().tinted(Palette::from_art_colors(bg, [0xffffff, 0xeeeeee, 0x111111, 0x222222])));
        for t in [Theme::light(), Theme::dark()].into_iter().chain(tinted) {
            for backdrop in [rgb(0xffffff).into(), rgb(0x000000).into(), rgb(0x808080).into()] {
                let bg = over(t.glass, backdrop);
                assert!(contrast(over(t.text, bg), bg) >= 4.5, "{:?} on {:?} over {backdrop:?}", t.text, t.glass);
                assert!(contrast(over(t.text_secondary, bg), bg) >= 3., "{:?} on {:?}", t.text_secondary, t.glass);
            }
        }
        assert!(Theme::light().glass.a <= 0.8 && Theme::dark().glass.a <= 0.85);
    }

    #[test]
    fn over_blends_like_the_renderer() {
        let half_white = Hsla { a: 0.5, ..gpui::white() };
        let c: Rgba = over(half_white, gpui::black()).into();
        assert!((c.r - 0.5).abs() < 0.01 && (c.a - 1.).abs() < 0.001);
    }

    #[test]
    fn tinted_palettes_keep_text_readable() {
        // Apple's colors for a dark cover whose text colors are all low-contrast grays.
        let p = Palette::from_art_colors(0x101010, [0x303030, 0x2a2a2a, 0x282828, 0x252525]);
        assert!(contrast(p.text, p.bg) >= 4.5);
        assert!(contrast(p.accent, p.bg) >= 3.);
        assert!(contrast(p.on_accent(), p.accent) >= 3.);
    }
}
