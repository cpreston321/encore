//! The floating player bar, volume popover and Playing Next panel.

use super::*;

const PILL_ALPHA: f32 = 0.62;
const PILL_RADIUS: f32 = 20.;

/// The player's colors: a deep shade of the cover, or the theme's pill before anything plays,
/// as glass over the page like the Music app's bar. The pill is as translucent as it can be
/// while its text reads over any page.
pub(crate) fn player_theme(t: &Theme, tint: Option<crate::theme::Palette>) -> Theme {
    let pt = match tint {
        Some(p) => {
            let ink = |a: f32| Hsla { h: 0., s: 0., l: 1., a };
            Theme {
                pill: Hsla { l: (p.bg.l + 0.05).min(0.3), a: 1., ..p.bg },
                text: ink(0.96),
                text_secondary: ink(0.62),
                text_tertiary: ink(0.35),
                icon: ink(0.92),
                hover: ink(0.1),
                placeholder: ink(0.1),
                panel_border: ink(0.12),
                track_bg: ink(0.2),
                track_fill: ink(0.9),
                accent: t.accent,
                ..*t
            }
        }
        None => *t,
    };
    Theme { pill: crate::theme::glass_fill(pt.pill, pt.text, pt.text_secondary, PILL_ALPHA), ..pt }
}

impl MusicApp {
    /// Floating player laid out like the macOS Music app (transport · now playing · tools),
    /// tinted by the current cover.
    pub(crate) fn render_player(&self, t: &Theme, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.queue.current().cloned();
        let has_track = current.is_some();
        let playing = matches!(self.state, PlayState::Playing | PlayState::Loading);

        let tint =
            current.as_ref().and_then(|s| self.art_color(&s.artwork, cx)).map(crate::theme::Palette::from_average);
        let pt = player_theme(t, tint);
        self.progress.read(cx).set_colors(pt.track_bg, pt.track_fill, pt.text_secondary);
        let t = &pt;
        let dim = if has_track { t.icon } else { t.text_tertiary };
        let toggle_color = |on: bool| if on { t.accent } else { t.text_secondary };

        let transport = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .child(
                icon_button(t, "shuffle", "shuffle", 14., toggle_color(self.queue.shuffle))
                    .accessibility_role(Role::ToggleButton)
                    .accessibility_label("Shuffle")
                    .accessibility_toggled(self.queue.shuffle)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.queue.toggle_shuffle();
                        cx.notify();
                    })),
            )
            .child(
                icon_button(t, "prev", "prev", 19., dim)
                    .accessibility_label("Previous")
                    .on_click(cx.listener(|this, _, _, cx| this.previous(cx))),
            )
            .child(
                icon_button(t, "play", if playing { "pause" } else { "play" }, 24., t.icon)
                    .size(px(40.))
                    .accessibility_label(if playing { "Pause" } else { "Play" })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
            )
            .child(
                icon_button(t, "next", "next", 19., dim)
                    .accessibility_label("Next")
                    .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
            )
            .child(
                icon_button(
                    t,
                    "repeat",
                    if self.queue.repeat == Repeat::One { "repeat-one" } else { "repeat" },
                    14.,
                    toggle_color(self.queue.repeat != Repeat::Off),
                )
                .accessibility_role(Role::ToggleButton)
                .accessibility_label(if self.queue.repeat == Repeat::One { "Repeat One" } else { "Repeat" })
                .accessibility_toggled(self.queue.repeat != Repeat::Off)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.queue.cycle_repeat();
                    cx.notify();
                })),
            );

        let now_playing: AnyElement = match &current {
            None => div().flex_1().flex().justify_center().child(icon("note", 18., t.text_tertiary)).into_any_element(),
            Some(song) => {
                let album = song.album_id.clone();
                let subtitle = if self.state == PlayState::Loading {
                    "Loading…".to_string()
                } else {
                    format!("{} — {}", song.artist, song.album)
                };
                self.progress.read(cx).set_subtitle(subtitle.into());
                // Like the Music app: art and text side by side with the menu at their right,
                // and the bar running under all three.
                const ART: f32 = 31.;
                const MORE: f32 = 26.;
                self.progress.read(cx).set_insets(ART + 10., MORE + 6.);
                let (hover, _) = self.progress.read(cx).hover_amount();
                let more = song.clone();
                let more_keyboard = song.clone();
                let swipe = self.player_swipe.offset;
                div()
                    .id("now-playing")
                    .on_scroll_wheel(cx.listener(|this, e: &gpui::ScrollWheelEvent, _, cx| this.player_swiped(e, cx)))
                    .left(px(swipe * 0.4))
                    .opacity((1. - swipe.abs() / 260.).max(0.3))
                    .flex_1()
                    .min_w_0()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("np-art")
                            .accessibility_label("Show Now Playing")
                            .opacity(1. - hover)
                            .absolute()
                            .top_0()
                            .left_0()
                            .cursor_pointer()
                            .rounded(px(5.))
                            .shadow_sm()
                            .child(self.art(&song.artwork, ART, 5., t, cx))
                            .on_click(cx.listener(|this, _, _, cx| this.open_now_playing(None, cx))),
                    )
                    .child(
                        div().absolute().top_0().right_0().h(px(ART)).flex().items_center().opacity(1. - hover).child(
                            icon_button(t, "player-more", "ellipsis", 15., dim)
                                .size(px(MORE))
                                .accessibility_label("More")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                        this.menu = Some(this.song_menu(&more, e.position));
                                        cx.stop_propagation();
                                        cx.notify();
                                    }),
                                )
                                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                                    if let ClickEvent::Keyboard(k) = e {
                                        this.menu = Some(this.song_menu(&more_keyboard, k.bounds.origin));
                                        cx.notify();
                                    }
                                })),
                        ),
                    )
                    .child(
                        div()
                            .id("np-title")
                            .opacity(1. - hover)
                            .accessibility_role(Role::Link)
                            .accessibility_label(format!("{}, Go to Album", song.title))
                            .pl(px(ART + 10.))
                            .pr(px(MORE + 6.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .cursor_pointer()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_height(px(16.))
                                    .text_color(t.text)
                                    .child(song.title.clone()),
                            )
                            .when(song.explicit, |d| d.child(explicit_badge(t)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(id) = &album {
                                    this.push(Route::Album(id.clone()), cx);
                                }
                            })),
                    )
                    // Subtitle, times and bar animate together inside Progress.
                    .child(self.progress.clone())
                    .with_animation(
                        ElementId::Name(format!("np-{}", song.id).into()),
                        Animation::new(Duration::from_millis(320)).with_easing(ease_out_quint()),
                        |el, p| el.opacity(p),
                    )
                    .into_any_element()
            }
        };

        let volume_icon = if self.volume < 0.01 { "speaker" } else { "speaker-loud" };
        let tools = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .child(
                icon_button(t, "lyrics", "lyrics", 16., if self.show_now_playing { t.accent } else { t.icon })
                    .accessibility_role(Role::ToggleButton)
                    .accessibility_label("Lyrics")
                    .accessibility_toggled(self.show_now_playing)
                    .on_click(cx.listener(|this, _, _, cx| {
                        use crate::ui::now_playing::NowPlayingTab;
                        if this.show_now_playing {
                            this.show_now_playing = false;
                            cx.notify();
                        } else {
                            this.open_now_playing(Some(NowPlayingTab::Lyrics), cx);
                        }
                    })),
            )
            .child(
                icon_button(t, "queue", "queue", 16., if self.show_queue { t.accent } else { t.icon })
                    .accessibility_role(Role::ToggleButton)
                    .accessibility_label("Queue")
                    .accessibility_toggled(self.show_queue)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_queue = !this.show_queue;
                        cx.notify();
                    })),
            )
            .child(
                icon_button(t, "volume", volume_icon, 17., t.icon)
                    .on_scroll_wheel(cx.listener(|this, e: &gpui::ScrollWheelEvent, _, cx| this.volume_scrolled(e, cx)))
                    .accessibility_label("Volume")
                    .accessibility_value(format!("{}%", (self.volume * 100.).round()))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_volume = true;
                        this.menu = None;
                        cx.notify();
                    })),
            );
        // The volume control opens over the tools, as in the Music app, reaching left over the
        // end of the song's row. The tools keep their place underneath, so nothing else in the
        // bar moves. Its speaker mutes and unmutes.
        let tools = div().flex_none().relative().child(tools.when(self.show_volume, |d| d.invisible())).when(
            self.show_volume,
            |d| {
                d.child(
                    div()
                        .id("volume-control")
                        .occlude()
                        .absolute()
                        .right_0()
                        .top(px(-3.))
                        .h(px(36.))
                        .pl_3()
                        .pr_0p5()
                        .rounded_full()
                        .glass_style(GlassStyle { blur: px(18.), ..GlassStyle::default() })
                        .bg(t.pill)
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_scroll_wheel(
                            cx.listener(|this, e: &gpui::ScrollWheelEvent, _, cx| this.volume_scrolled(e, cx)),
                        )
                        .child(div().absolute().inset_0().rounded_full().bg(t.text.opacity(0.08)))
                        .child(self.volume_slider(self.volume, px(118.), t, cx))
                        .child(
                            icon_button(t, "mute", volume_icon, 17., t.icon)
                                .accessibility_label(if self.volume < 0.01 { "Unmute" } else { "Mute" })
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                        ),
                )
            },
        );

        div()
            .id("player")
            .occlude()
            .w(px(width))
            .h(px(58.))
            .pl_2()
            .pr_2p5()
            .rounded(px(PILL_RADIUS))
            .glass_style(GlassStyle { blur: px(22.), ..GlassStyle::default() })
            .bg(self.fx.borrow_mut().pill_bg.update(t.pill))
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .flex()
            .items_center()
            .gap_4()
            .child(transport)
            .child(now_playing)
            .child(tools)
    }

    pub(crate) fn render_queue(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let accent = t.accent;
        let mut panel = div()
            .id("queue")
            .drag_over::<crate::ui::list_tools::DraggedSongs>(move |s, _, _, _| s.border_color(accent).border_2())
            .on_drop(cx.listener(|this, dragged: &crate::ui::list_tools::DraggedSongs, _, cx| {
                for s in dragged.0.iter().rev() {
                    this.play_next(s.clone(), cx);
                }
                let n = dragged.0.len();
                this.show_notice(if n == 1 { "Playing next".into() } else { format!("{n} songs playing next") }, cx);
            }))
            .occlude()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .bottom(px(82.))
            .w(px(QUEUE_W))
            .rounded(px(PANEL_RADIUS))
            .glass_style(panel_glass())
            .bg(crate::theme::glass_fill(t.panel, t.text, t.text_secondary, crate::theme::GLASS_MIN_ALPHA))
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .overflow_y_scroll()
            .map(lock_scroll_axis)
            .p_3()
            .flex()
            .flex_col()
            .gap_0p5();

        let heading =
            |s: &'static str| div().px_1().pt_2().pb_1().text_size(px(15.)).font_weight(FontWeight::BOLD).child(s);

        let Some(current) = self.queue.current().cloned() else {
            return panel
                .child(heading("Playing Next"))
                .child(div().pt_8().text_color(t.text_secondary).text_center().child("Nothing is playing."));
        };

        let repeat_icon = if self.queue.repeat == Repeat::One { "repeat-one" } else { "repeat" };
        let crossfade =
            crate::app::CROSSFADE_OPTIONS.into_iter().fold(tahoe::SegmentedControl::new().theme(*t), |c, secs| {
                c.segment(
                    ("crossfade", secs as usize),
                    if secs == 0 { "Off".to_string() } else { format!("{secs}s") },
                    self.crossfade_secs == secs,
                    cx.listener(move |this, _, _, cx| this.set_crossfade(secs, cx)),
                )
            });
        panel = panel
            .child(heading("Now Playing"))
            .child(self.queue_row(t, &current, cx).bg(t.hover))
            .child(
                div()
                    .mt_2()
                    .px_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(12.5)).text_color(t.text_secondary).child("Crossfade"))
                    .child(crossfade),
            )
            .child(
                div().flex().items_center().justify_between().child(heading("Playing Next")).child(
                    div()
                        .flex()
                        .gap_1()
                        .child(toggle_button(t, "queue-shuffle", "shuffle", self.queue.shuffle).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.queue.toggle_shuffle();
                                cx.notify();
                            },
                        )))
                        .child(
                            toggle_button(t, "queue-repeat", repeat_icon, self.queue.repeat != Repeat::Off).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.queue.cycle_repeat();
                                    cx.notify();
                                }),
                            ),
                        ),
                ),
            );

        let up_next = self.queue.up_next();
        if up_next.is_empty() {
            panel = panel.child(div().px_1().text_color(t.text_secondary).child("No upcoming songs."));
        }
        for (n, song) in up_next.iter().enumerate().take(100) {
            panel = panel.child(
                self.queue_row(t, song, cx)
                    .id(("queue-item", n))
                    .hover(|s| s.bg(t.hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_ahead(n, cx))),
            );
        }
        panel
    }

    pub(crate) fn queue_row(&self, t: &Theme, song: &Song, cx: &mut Context<Self>) -> Div {
        div()
            .p_1p5()
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap_2p5()
            .min_w_0()
            .child(self.art(&song.artwork, 40., 5., t, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().truncate().child(song.title.clone()))
                    .child(div().truncate().text_size(px(12.)).text_color(t.text_secondary).child(song.artist.clone())),
            )
    }

    /// Horizontal volume slider: fills from the left; drag or click anywhere on it.
    pub(crate) fn volume_slider(
        &self,
        value: f32,
        width: Pixels,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let knob = px(14.);
        div()
            .id("volume")
            .accessibility_role(Role::Slider)
            .accessibility_label("Volume")
            .accessibility_value(format!("{}%", (value * 100.).round()))
            .relative()
            .w(width)
            .h(px(22.))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.drag = Some(Drag::Volume);
                    this.drag_to(Drag::Volume, e.position, cx);
                    cx.stop_propagation();
                }),
            )
            .child(capture_bounds(self.volume_bounds.clone()))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(5.))
                    .rounded_full()
                    .bg(t.track_bg)
                    .child(div().h_full().w(relative(value)).rounded_full().bg(t.track_fill)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left((width - knob) * value)
                    .size(knob)
                    .rounded_full()
                    .bg(rgb(0xf5f5f5))
                    .shadow_sm(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Palette, contrast, over};

    #[test]
    fn player_text_reads_over_white_and_black_pages() {
        let covers =
            [0xffffff, 0x000000, 0xf4d03f, 0x1f3a93, 0xc0392b].map(|c| Some(Palette::from_average(rgb(c).into())));
        for base in [Theme::light(), Theme::dark()] {
            for tint in covers.into_iter().chain([None]) {
                let t = player_theme(&base, tint);
                assert!(t.pill.a < 1., "{:?}", t.pill);
                for page in [rgb(0xffffff).into(), rgb(0x000000).into()] {
                    let bg = over(t.pill, page);
                    assert!(contrast(over(t.text, bg), bg) >= 4.5, "{:?} on {:?}", t.text, t.pill);
                    assert!(contrast(over(t.text_secondary, bg), bg) >= 3., "{:?} on {:?}", t.text_secondary, t.pill);
                }
            }
        }
    }
}
