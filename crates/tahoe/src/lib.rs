//! # Tahoe
//!
//! Components for [GPUI](https://www.gpui.rs) apps that look at home on macOS 26 Tahoe, built
//! for [Encore](https://github.com/cpreston321/encore) and usable in any GPUI app, the way
//! Zed keeps its own `ui` crate.
//!
//! - [`theme`]: light and dark color tokens, palettes for surfaces tinted by artwork, and
//!   WCAG contrast helpers ([`readable_on`]) that keep text and icons legible on them.
//! - [`components`]: builders such as [`Button`], [`IconButton`], [`Link`], [`SectionHeader`],
//!   [`PageHeader`], [`Card`] and [`Badge`].
//! - [`primitives`]: the same looks as plain, chainable gpui elements.
//! - [`anim`]: reversible open/close transitions, color blends and Reduce Motion.
//! - [`icons`]: the embedded icon set; register [`Assets`] with your `Application`.
//! - [`chrome`]: native window tweaks, such as Tahoe's larger window corners.
//!
//! ```ignore
//! use tahoe::prelude::*;
//!
//! Application::new().with_assets(tahoe::Assets).run(|cx| {
//!     cx.open_window(Default::default(), |window, cx| {
//!         tahoe::chrome::use_tahoe_corners(window);
//!         cx.new(|_| MyView)
//!     });
//! });
//!
//! // In a render function:
//! PageHeader::new("Albums").on_back(cx.listener(|this, _, _, cx| this.back(cx)))
//! ```

pub mod anim;
pub mod chrome;
pub mod components;
pub mod icons;
pub mod primitives;
pub mod theme;

pub use components::{Badge, Button, ButtonStyle, Card, IconButton, IconButtonStyle, Link, PageHeader, SectionHeader};
pub use icons::{Assets, icon};
pub use theme::{Palette, Theme, contrast, luminance, readable_on};

/// Everything for building Tahoe-style views.
pub mod prelude {
    pub use crate::{
        Badge, Button, ButtonStyle, Card, IconButton, IconButtonStyle, Link, PageHeader, Palette, SectionHeader, Theme,
        icon, readable_on,
    };
}
