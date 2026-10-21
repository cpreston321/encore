use gpui::{Hsla, WindowAppearance, rgb, rgba};

#[derive(Clone, Copy)]
pub struct Theme {
    pub window: Hsla,
    pub panel: Hsla,
    pub panel_border: Hsla,
    pub pill: Hsla,
    pub menu: Hsla,
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

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        // ENCORE_APPEARANCE=light|dark forces a theme (for checking both without changing
        // the system setting).
        match std::env::var("ENCORE_APPEARANCE").as_deref() {
            Ok("light") => return Self::light(),
            Ok("dark") => return Self::dark(),
            _ => {}
        }
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    fn dark() -> Self {
        Self {
            window: rgb(0x1e1e1e).into(),
            panel: rgb(0x282828).into(),
            panel_border: rgba(0xffffff1a).into(),
            pill: rgba(0x2c2c2ef2).into(),
            menu: rgb(0x2c2c2e).into(),
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
    }

    fn light() -> Self {
        Self {
            window: rgb(0xffffff).into(),
            panel: rgb(0xf3f3f5).into(),
            panel_border: rgba(0x0000001a).into(),
            pill: rgba(0xf8f8faf2).into(),
            menu: rgb(0xf6f6f6).into(),
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
    pub fn from_art_colors(c: crate::model::ArtColors) -> Self {
        Self::from_art_colors_raw(c).readable()
    }

    fn from_art_colors_raw(c: crate::model::ArtColors) -> Self {
        let bg: Hsla = rgb(c.bg).into();
        let texts: Vec<Hsla> = c.text.iter().map(|&t| rgb(t).into()).collect();
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

fn white_ish(alpha: f32) -> Hsla {
    Hsla { h: 0., s: 0., l: 1., a: alpha }
}

impl Theme {
    /// This theme recolored for a page whose background is the palette's tint.
    pub fn tinted(&self, p: Palette) -> Theme {
        let ink = |a: f32| Hsla { a, ..p.text };
        Theme {
            window: p.bg,
            panel: Hsla { l: (p.bg.l + 0.05).min(1.), ..p.bg },
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
    fn tinted_palettes_keep_text_readable() {
        // Apple's colors for a dark cover whose text colors are all low-contrast grays.
        let colors = crate::model::ArtColors { bg: 0x101010, text: [0x303030, 0x2a2a2a, 0x282828, 0x252525] };
        let p = Palette::from_art_colors(colors);
        assert!(contrast(p.text, p.bg) >= 4.5);
        assert!(contrast(p.accent, p.bg) >= 3.);
        assert!(contrast(p.on_accent(), p.accent) >= 3.);
    }
}
