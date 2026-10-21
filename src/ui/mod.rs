//! Everything drawn on screen. Each file adds `impl MusicApp` methods for one area; shared
//! imports and layout constants live here and reach the children through `use super::*`.

use std::{ops::Range, sync::Arc, time::Duration};

use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, Div, ElementId, FontWeight, GlassStyle, Hsla,
    MouseButton, MouseDownEvent, ObjectFit, Pixels, Role, SharedString, Stateful, Svg, anchored, deferred, div,
    ease_out_quint, img, linear_color_stop, linear_gradient, prelude::*, pulsating_between, px, relative, rgb,
    uniform_list, white,
};

use crate::{
    app::{Account, Drag, Load, MusicApp, PlayState, QUEUE_W, Route, SIDEBAR_W},
    model::{Album, Artwork, Playlist, Song, Station, format_ms},
    queue::Repeat,
    theme::Theme,
};

pub(crate) const PAD: f32 = 32.;
/// Sidebar row height and spacing (the section fold animation depends on them).
const ROW_H: f32 = 28.;
const ROW_GAP: f32 = 2.;

/// Album grids larger than this are virtualized.
pub(crate) const VIRTUAL_GRID_MIN: usize = 60;

/// Floating panels sit 8pt inside the window, so their corners are the window's minus 8.
pub(crate) const PANEL_RADIUS: f32 = 18.;
pub(crate) const GAP: f32 = 20.;

/// Liquid Glass for floating panels, menus, popovers and sheets. Fill them with `Theme::glass`,
/// or a color through `tahoe::glass_fill`, so text reads whatever is behind.
pub(crate) fn panel_glass() -> GlassStyle {
    GlassStyle { blur: px(24.), ..GlassStyle::default() }
}

pub(crate) mod components;
pub(crate) mod detail;
pub(crate) mod gestures;
pub(crate) mod library;
pub(crate) mod list_tools;
pub(crate) mod mini;
pub(crate) mod native_fill;
pub(crate) mod now_playing;
pub(crate) mod pages;
pub(crate) mod palette;
pub(crate) mod player;
pub(crate) mod playlists;
pub(crate) mod progress;
pub(crate) mod regions;
pub(crate) mod see_all;
pub(crate) mod shell;
pub(crate) mod shortcuts;
pub(crate) mod welcome;
pub(crate) mod widgets;

pub(crate) use widgets::*;
