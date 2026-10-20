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
        // MUSIC_APPEARANCE=light|dark forces a theme (for checking both without changing
        // the system setting).
        match std::env::var("MUSIC_APPEARANCE").as_deref() {
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
        Self {
            bg: Hsla { s: c.s.min(0.45), l: (c.l * 0.55).clamp(0.11, 0.22), ..c },
            text: white_ish(0.95),
            text2: white_ish(0.68),
            accent: Hsla { s: c.s.clamp(0.45, 0.75), l: 0.56, ..c },
        }
    }

    /// Icon color that reads on the accent fill.
    pub fn on_accent(&self) -> Hsla {
        if self.accent.l > 0.62 { self.bg } else { white_ish(1.) }
    }
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
