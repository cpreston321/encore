//! Shared pieces built from app state: artwork, song rows and tables, tiles, menus buttons.

use super::*;

impl MusicApp {
    pub(crate) fn album_track_row(
        &self,
        t: &Theme,
        list: &Arc<Vec<Song>>,
        ix: usize,
        song: &Song,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_current = self.queue.current().is_some_and(|c| c.id == song.id);
        let selected = self.is_selected(&song.id);
        let fg2 = if selected { white().opacity(0.85) } else { t.text_secondary };
        // The number (or now-playing speaker) sits in-flow in a centered box; the hover
        // play icon overlays the same box, so swapping between them doesn't shift anything.
        let number: AnyElement = if is_current {
            icon(
                if self.state == PlayState::Playing { "speaker-loud" } else { "speaker" },
                14.,
                if selected { white() } else { t.accent },
            )
            .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(16.))
                        .group_hover("track", |s| s.invisible())
                        .child(format!("{}", song.track_number.max(ix as u32 + 1))),
                )
                .into_any_element()
        };
        let hover_play = (!is_current).then(|| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .invisible()
                .group_hover("track", |s| s.visible())
                // The triangle's visual center sits left of its box; nudge it.
                .child(div().ml(px(1.5)).child(icon("play", 12., if selected { white() } else { t.text })))
        });
        let list = list.clone();
        let menu_list = list.clone();

        div()
            .id(("track", ix))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    let song = menu_list[ix].clone();
                    if !this.is_selected(&song.id) {
                        this.click_song(&menu_list, ix, false, false);
                    }
                    this.open_song_menu(&song, &menu_list, e.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_drag(self.drag_payload(&list[ix], &list), |d, _, _, cx| crate::ui::list_tools::drag_preview(d, cx))
            .group("track")
            .h(px(44.))
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(7.))
            .when(ix.is_multiple_of(2) && !selected, |d| d.bg(t.row_alt))
            // Tinted (artwork-colored) pages use thin dividers instead of zebra stripes.
            .when(t.row_alt.a == 0., |d| d.rounded_none().border_b_1().border_color(t.separator))
            .when(selected, |d| d.bg(t.accent).text_color(white()))
            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
            .child(
                div()
                    .relative()
                    .w(px(24.))
                    .h(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(fg2)
                    .child(number)
                    .children(hover_play),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .truncate()
                            .when(is_current && !selected, |d| d.text_color(t.accent))
                            .child(song.title.clone()),
                    )
                    .when(song.explicit, |d| d.child(explicit_badge(t))),
            )
            .child(div().text_color(fg2).child(format_ms(song.duration_ms)))
            .child(self.more_button(t, ("track-more", ix).into(), song, "track", cx))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                let m = e.modifiers();
                if e.click_count() >= 2 && !m.platform && !m.shift {
                    this.multi.clear();
                    this.play(list.as_ref().clone(), ix, false, cx);
                }
                this.click_song(&list, ix, m.platform, m.shift);
                cx.notify();
            }))
    }

    pub(crate) fn error_state(&self, t: &Theme, err: &str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pt(px(120.))
            .px(px(PAD))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child("Couldn't connect to Apple Music"))
            .child(
                div()
                    .max_w(px(520.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(t.text_secondary)
                    .child(err.to_string()),
            )
            .child(
                div().pt_2().child(
                    pill_button(t, "retry", "repeat", "Try Again", true)
                        .on_click(cx.listener(|this, _, _, cx| this.retry_route(cx))),
                ),
            )
    }

    /// Cover art at `size`×`size`, downloaded at a nearby fixed resolution so sizes share cache.
    pub(crate) fn art(
        &self,
        artwork: &Artwork,
        size: f32,
        radius: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.art_box(artwork, size, size, radius, t, cx)
    }

    pub(crate) fn art_box(
        &self,
        artwork: &Artwork,
        w: f32,
        h: f32,
        radius: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.art_box_moving(artwork, w, h, radius, t, None, cx)
    }

    /// Artwork that slowly drifts while Animate Artwork is on, for hero cards and headers.
    pub(crate) fn living_art_box(
        &self,
        artwork: &Artwork,
        w: f32,
        h: f32,
        radius: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let motion = self.artwork_motion(&artwork.url(1), 0.1);
        self.art_box_moving(artwork, w, h, radius, t, motion, cx)
    }

    pub(crate) fn living_art(
        &self,
        artwork: &Artwork,
        size: f32,
        radius: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.living_art_box(artwork, size, size, radius, t, cx)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn art_box_moving(
        &self,
        artwork: &Artwork,
        w: f32,
        h: f32,
        radius: f32,
        t: &Theme,
        motion: Option<(f32, f32)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Zoomed-in artwork needs more pixels to stay sharp.
        let zoom = motion.map_or(1., |(_, strength)| 1. + strength);
        let want = (w.max(h) * 2. * zoom) as u32;
        let px_size = [120, 240, 400, 600, 1000, 1600, 2400].into_iter().find(|&s| s >= want).unwrap_or(2400);
        // Non-square frames get a server-side crop: "cover" scaling would draw past the
        // frame, and the image's rounded corners would land outside the visible area.
        let placeholder = || div().w(px(w)).h(px(h)).flex_none().rounded(px(radius)).bg(t.placeholder);
        if artwork.is_empty() {
            return placeholder().into_any_element();
        }
        let url = if (w - h).abs() > 1. {
            let (pw, ph) = if w >= h {
                (px_size, (px_size as f32 * h / w).round() as u32)
            } else {
                ((px_size as f32 * w / h).round() as u32, px_size)
            };
            artwork.url_cropped(pw, ph)
        } else {
            artwork.url(px_size)
        };
        match self.image(&url, cx) {
            // gpui forces an image's intrinsic aspect ratio onto its layout box, so min/max
            // bounds pin the size; `Cover` then paints past those bounds, so the wrapper clips it.
            Some(path) => {
                let frame =
                    div().w(px(w)).h(px(h)).flex_none().rounded(px(radius)).overflow_hidden().bg(t.placeholder).child(
                        img(path)
                            .min_w(px(w))
                            .max_w(px(w))
                            .min_h(px(h))
                            .max_h(px(h))
                            .rounded(px(radius))
                            .object_fit(ObjectFit::Cover)
                            .when_some(motion, |img, (phase, strength)| img.motion(phase, strength)),
                    );
                if self.image_is_fresh(&url) {
                    frame
                        .with_animation(
                            ElementId::Name(url.into()),
                            Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
                            |el, p| el.opacity(p),
                        )
                        .into_any_element()
                } else {
                    frame.into_any_element()
                }
            }
            None => placeholder().into_any_element(),
        }
    }

    pub(crate) fn avatar(
        &self,
        artwork: &Artwork,
        name: &str,
        size: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !artwork.is_empty() {
            return self.art(artwork, size, size / 2., t, cx);
        }
        let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .bg(gradient(0x8e8e93, 0x3a3a3c))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size * 0.36))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(white())
            .child(initials.to_uppercase())
            .into_any_element()
    }

    pub(crate) fn hero_card(
        &self,
        t: &Theme,
        i: usize,
        album: &Album,
        w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let h = (w * 0.56).floor();
        let id = album.id.clone();
        div()
            .id(("hero", i))
            .w(px(w))
            .h(px(h))
            .flex_none()
            .relative()
            .rounded(px(12.))
            .cursor_pointer()
            .shadow_md()
            .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Album(id.clone()), cx)))
            .child(self.living_art_box(&album.artwork, w, h, 12., t, cx))
            .child(div().absolute().left_0().right_0().bottom_0().h(relative(0.65)).rounded_b(px(12.)).bg(
                linear_gradient(
                    180.,
                    linear_color_stop(gpui::black().opacity(0.), 0.),
                    linear_color_stop(gpui::black().opacity(0.88), 1.),
                ),
            ))
            .child(
                div()
                    .absolute()
                    .left(px(20.))
                    .right(px(20.))
                    .bottom(px(16.))
                    .flex()
                    .flex_col()
                    .text_color(white())
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(white().opacity(0.75))
                            .child(if i == 0 { "NEW ALBUM · FEATURED" } else { "NEW ALBUM" }),
                    )
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).truncate().child(album.title.clone()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(white().opacity(0.85))
                            .truncate()
                            .child(album.artist.clone()),
                    ),
            )
    }

    /// Horizontally scrolling columns of four song rows, like "Best New Songs".
    pub(crate) fn song_columns(
        &self,
        t: &Theme,
        id: &'static str,
        songs: Vec<Song>,
        col_w: f32,
        ranked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.song_columns_impl(t, id, songs, col_w, ranked, false, cx)
    }

    /// Song columns whose second line is "ALBUM · YEAR" (for artist pages, where the
    /// artist is already known).
    pub(crate) fn song_columns_by_album(
        &self,
        t: &Theme,
        id: &'static str,
        songs: Vec<Song>,
        col_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.song_columns_impl(t, id, songs, col_w, false, true, cx)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn song_columns_impl(
        &self,
        t: &Theme,
        id: &'static str,
        songs: Vec<Song>,
        col_w: f32,
        ranked: bool,
        by_album: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list = Arc::new(songs);
        let mut row = scroll_row(id);
        for (c, chunk) in list.chunks(4).enumerate() {
            let mut col = div().w(px(col_w)).flex_none().flex().flex_col();
            for r in 0..chunk.len() {
                col = col.child(self.song_cell(t, id, &list, c * 4 + r, ranked, by_album, cx));
            }
            row = row.child(col);
        }
        row
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn song_cell(
        &self,
        t: &Theme,
        prefix: &'static str,
        list: &Arc<Vec<Song>>,
        ix: usize,
        ranked: bool,
        by_album: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let song = &list[ix];
        let subtitle = if by_album {
            let year = song.release_date.get(..4).unwrap_or("");
            [song.album.to_uppercase(), year.to_string()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        } else {
            song.artist.clone()
        };
        let is_current = self.queue.current().is_some_and(|c| c.id == song.id);
        let play_list = list.clone();
        let dbl_list = list.clone();
        let group: SharedString = format!("{prefix}-cell-{ix}").into();
        let hover = self.hover_value(&group);
        let hover_key = group.clone();
        let menu_list = list.clone();

        div()
            .id(ElementId::from((SharedString::from(prefix), ix)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    let song = menu_list[ix].clone();
                    this.open_song_menu(&song, &menu_list, e.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_drag(self.drag_payload(&list[ix], list), |d, _, _, cx| crate::ui::list_tools::drag_preview(d, cx))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if this.set_hover(hover_key.clone(), *hovered) {
                    cx.notify();
                }
            }))
            .h(px(60.))
            .flex()
            .items_center()
            .gap_3()
            .border_t_1()
            .border_color(t.separator)
            .child(
                div()
                    .id(ElementId::from((SharedString::from(format!("{prefix}-art")), ix)))
                    .relative()
                    .flex_none()
                    .cursor_pointer()
                    .child(self.art(&song.artwork, 44., 5., t, cx))
                    .child(
                        art_overlay(44., icon(if is_current { "speaker-loud" } else { "play" }, 15., white()))
                            .when(!is_current, |d| d.opacity(hover).when(hover == 0., |d| d.invisible())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.play(play_list.as_ref().clone(), ix, false, cx);
                    })),
            )
            .when(ranked, |d| {
                d.child(
                    div()
                        .w(px(20.))
                        .flex_none()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text_secondary)
                        .child(format!("{}", ix + 1)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(is_current, |d| d.text_color(t.accent))
                                    .child(song.title.clone()),
                            )
                            .when(song.explicit, |d| d.child(explicit_badge(t))),
                    )
                    .child(div().truncate().text_size(px(12.5)).text_color(t.text_secondary).child(subtitle)),
            )
            .child(self.more_button(
                t,
                ElementId::from((SharedString::from(format!("{prefix}-more")), ix)),
                song,
                "",
                cx,
            ))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if e.click_count() >= 2 {
                    this.play(dbl_list.as_ref().clone(), ix, false, cx);
                }
            }))
    }

    /// Library-style table: Title / Artist / Album / Time.
    pub(crate) fn song_table(
        &self,
        t: &Theme,
        id: &'static str,
        songs: Vec<Song>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list = Arc::new(songs);
        let mut rows = div().flex().flex_col();
        for ix in 0..list.len() {
            rows = rows.child(self.table_row(t, id, &list, ix, cx));
        }
        div().px(px(PAD - 8.)).pt_2().child(table_header(t)).child(rows)
    }

    pub(crate) fn table_row(
        &self,
        t: &Theme,
        id: &'static str,
        list: &Arc<Vec<Song>>,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let song = &list[ix];
        let is_current = self.queue.current().is_some_and(|c| c.id == song.id);
        let selected = self.is_selected(&song.id);
        let fg2 = if selected { white().opacity(0.85) } else { t.text_secondary };
        let list = list.clone();
        let menu_list = list.clone();

        div()
            .id(ElementId::from((SharedString::from(format!("{id}-row")), ix)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    let song = menu_list[ix].clone();
                    if !this.is_selected(&song.id) {
                        this.click_song(&menu_list, ix, false, false);
                    }
                    this.open_song_menu(&song, &menu_list, e.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_drag(self.drag_payload(&list[ix], &list), |d, _, _, cx| crate::ui::list_tools::drag_preview(d, cx))
            .group("row")
            // Rows inside a uniform_list don't stretch to its width on their own.
            .w_full()
            .h(px(44.))
            .px_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(6.))
            .when(ix % 2 == 1 && !selected, |d| d.bg(t.row_alt))
            .when(selected, |d| d.bg(t.accent).text_color(white()))
            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .child(self.art(&song.artwork, 32., 4., t, cx))
                            .when(is_current, |d| d.child(art_overlay(32., icon("speaker-loud", 13., white())))),
                    )
                    .child(
                        div()
                            .truncate()
                            .when(is_current && !selected, |d| d.text_color(t.accent))
                            .child(song.title.clone()),
                    )
                    .when(song.explicit, |d| d.child(explicit_badge(t))),
            )
            .child(div().w(relative(0.22)).truncate().text_color(fg2).child(song.artist.clone()))
            .child(div().w(relative(0.22)).truncate().text_color(fg2).child(song.album.clone()))
            .child(div().w(px(44.)).flex().justify_end().text_color(fg2).child(format_ms(song.duration_ms)))
            .child(self.more_button(
                t,
                ElementId::from((SharedString::from(format!("{id}-more")), ix)),
                song,
                "row",
                cx,
            ))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                let m = e.modifiers();
                if e.click_count() >= 2 && !m.platform && !m.shift {
                    this.multi.clear();
                    this.play(list.as_ref().clone(), ix, false, cx);
                }
                this.click_song(&list, ix, m.platform, m.shift);
                cx.notify();
            }))
    }

    /// "⋯" button that opens the song menu. With a `hover_group`, it only shows on row hover.
    pub(crate) fn more_button(
        &self,
        t: &Theme,
        id: ElementId,
        song: &Song,
        hover_group: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let song = song.clone();
        div()
            .id(id)
            .size(px(26.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(t.selected))
            .when(!hover_group.is_empty(), |d| d.invisible().group_hover(hover_group, |s| s.visible()))
            .child(icon("ellipsis", 14., t.text_secondary))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    let list = this.page_songs().unwrap_or_default();
                    this.open_song_menu(&song, &list, e.position);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_click(|_, _, cx| cx.stop_propagation())
    }

    pub(crate) fn album_tile(
        &self,
        t: &Theme,
        prefix: &'static str,
        i: usize,
        album: &Album,
        size: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = album.id.clone();
        let play_id = album.id.clone();
        let group: SharedString = format!("{prefix}-tile-{i}").into();
        let hover = self.hover_value(&group);
        let hover_key = group.clone();
        let menu_album = album.clone();
        div()
            .id(ElementId::from((SharedString::from(prefix), i)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.menu = Some(this.album_menu(&menu_album, e.position));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .w(px(size))
            .flex_none()
            .flex()
            .flex_col()
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if this.set_hover(hover_key.clone(), *hovered) {
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Album(id.clone()), cx)))
            .child(
                div()
                    .relative()
                    .child(self.art(&album.artwork, size, 8., t, cx))
                    // Hover dims the cover slightly and fades the play button in.
                    .child(div().absolute().inset_0().rounded(px(8.)).bg(gpui::black().opacity(0.16 * hover)))
                    .child(
                        div()
                            .id(ElementId::from((SharedString::from(format!("{prefix}-play")), i)))
                            .absolute()
                            .left_2()
                            .bottom_2()
                            .size(px(32.))
                            .rounded_full()
                            .bg(gpui::black().opacity(0.55))
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(hover)
                            .mb(px((1. - hover) * -4.))
                            .when(hover == 0., |d| d.invisible())
                            .hover(|s| s.bg(t.accent))
                            .child(icon("play", 13., white()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.play_album(play_id.clone(), cx);
                            })),
                    ),
            )
            .child(
                div()
                    .pt_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .child(div().truncate().child(album.title.clone()))
                    .when(album.explicit, |d| d.child(explicit_badge(t))),
            )
            .child(div().truncate().text_size(px(12.)).text_color(t.text_secondary).child(album.artist.clone()))
    }

    pub(crate) fn playlist_tile(
        &self,
        t: &Theme,
        prefix: &'static str,
        i: usize,
        playlist: &Playlist,
        size: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = playlist.clone();
        div()
            .id(ElementId::from((SharedString::from(prefix), i)))
            .w(px(size))
            .flex_none()
            .flex()
            .flex_col()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Playlist(p.clone()), cx)))
            .child(self.playlist_art(playlist, size, 8., t, cx))
            .child(div().pt_2().truncate().child(playlist.name.clone()))
            .child(
                div()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(t.text_secondary)
                    .child(if playlist.curator.is_empty() { "Playlist".to_string() } else { playlist.curator.clone() }),
            )
    }

    /// One quarter of a 2×2 mosaic, rounded only on its outer corner (gpui clips to
    /// rectangles, so the parent can't round the grid for us).
    pub(crate) fn mosaic_cell(
        &self,
        artwork: &Artwork,
        size: f32,
        ix: usize,
        radius: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let r = px(radius);
        let round = |d: Div| match ix {
            0 => d.rounded_tl(r),
            1 => d.rounded_tr(r),
            2 => d.rounded_bl(r),
            _ => d.rounded_br(r),
        };
        let px_size = if size * 2. <= 240. { 240 } else { 600 };
        match self.image(&artwork.url(px_size), cx) {
            Some(path) => {
                let mut image = img(path).size(px(size)).flex_none().object_fit(ObjectFit::Cover);
                image = match ix {
                    0 => image.rounded_tl(r),
                    1 => image.rounded_tr(r),
                    2 => image.rounded_bl(r),
                    _ => image.rounded_br(r),
                };
                image.into_any_element()
            }
            None => round(div().size(px(size)).flex_none().bg(t.placeholder)).into_any_element(),
        }
    }

    /// Playlist artwork, or — like the Music app — a 2×2 grid of its songs' covers when
    /// Apple has none (common for playlists you made yourself).
    pub(crate) fn playlist_art(
        &self,
        playlist: &Playlist,
        size: f32,
        radius: f32,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !playlist.artwork.is_empty() {
            return self.art(&playlist.artwork, size, radius, t, cx);
        }
        self.ensure_playlist_tracks(playlist, cx);
        let mut covers: Vec<Artwork> = Vec::new();
        if let Some(Load::Ready(Some(tracks))) = self.playlist_tracks.get(&playlist.id) {
            for song in tracks {
                if !song.artwork.is_empty() && !covers.contains(&song.artwork) {
                    covers.push(song.artwork.clone());
                    if covers.len() == 4 {
                        break;
                    }
                }
            }
        }
        match covers.len() {
            0 => div()
                .size(px(size))
                .flex_none()
                .rounded(px(radius))
                .bg(t.placeholder)
                .flex()
                .items_center()
                .justify_center()
                .child(icon("playlist", size * 0.3, t.text_tertiary))
                .into_any_element(),
            1..=3 => self.art(&covers[0], size, radius, t, cx),
            _ => {
                let half = (size / 2.).floor();
                div()
                    .size(px(half * 2.))
                    .flex_none()
                    .rounded(px(radius))
                    .overflow_hidden()
                    .flex()
                    .flex_wrap()
                    .children(covers.iter().enumerate().map(|(i, c)| self.mosaic_cell(c, half, i, radius, t, cx)))
                    .into_any_element()
            }
        }
    }

    pub(crate) fn play_shuffle(&self, t: &Theme, list: Arc<Vec<Song>>, cx: &mut Context<Self>) -> impl IntoElement {
        let shuffled = list.clone();
        div()
            .flex()
            .gap_3()
            .child(
                pill_button(t, "play-all", "play", "Play", true)
                    .on_click(cx.listener(move |this, _, _, cx| this.play(list.as_ref().clone(), 0, false, cx))),
            )
            .child(pill_button(t, "shuffle-all", "shuffle", "Shuffle", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    let start = (shuffled.len() / 2).min(shuffled.len().saturating_sub(1));
                    this.play(shuffled.as_ref().clone(), start, true, cx);
                },
            )))
    }

    /// Plays an album from a tile, fetching its tracks first if needed.
    pub(crate) fn play_album(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(Load::Ready((_, tracks))) = self.albums.get(&id) {
            let tracks = tracks.clone();
            return self.play(tracks, 0, false, cx);
        }
        let catalog = self.catalog.clone();
        cx.spawn(async move |this, cx| {
            let key = id.clone();
            let result = cx.background_executor().spawn(async move { catalog.album(&key) }).await;
            this.update(cx, |this, cx| match result {
                Ok((album, tracks)) => {
                    this.albums.insert(id, Load::Ready((album, tracks.clone())));
                    this.play(tracks, 0, false, cx);
                }
                Err(e) => {
                    this.playback_error = Some(format!("Couldn't load album: {e:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The query with a caret at the cursor, or the placeholder.
    pub(crate) fn search_text(&self, t: &Theme, focused: bool, placeholder: &'static str, caret_h: f32) -> AnyElement {
        let text = self.search.text();
        if text.is_empty() && !focused {
            return div().text_color(t.text_tertiary).child(placeholder).into_any_element();
        }
        let row = div().flex().items_center().min_w_0().overflow_hidden().whitespace_nowrap();
        let piece = |s: &str| SharedString::from(s.to_string());
        if let Some(sel) = self.search.selection() {
            // Like macOS: no caret while text is selected, just the highlight.
            let highlight = if focused { t.accent.opacity(0.38) } else { t.selected };
            return row
                .when(sel.start > 0, |d| d.child(piece(&text[..sel.start])))
                .child(div().rounded(px(3.)).bg(highlight).child(piece(&text[sel.clone()])))
                .when(sel.end < text.len(), |d| d.child(piece(&text[sel.end..])))
                .into_any_element();
        }
        let (before, after) = text.split_at(self.search.cursor());
        row.when(!before.is_empty(), |d| d.child(piece(before)))
            .when(focused, |d| d.child(div().flex_none().w(px(1.5)).h(px(caret_h)).bg(t.accent)))
            .when(!after.is_empty(), |d| d.child(piece(after)))
            .when(text.is_empty(), |d| d.child(div().pl_0p5().text_color(t.text_tertiary).child(placeholder)))
            .into_any_element()
    }
}
