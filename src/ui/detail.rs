//! iOS Music–style detail pages: the whole page takes its color from the artwork, the
//! header art moves with parallax, and a compact title bar fades in once it scrolls away.

use std::sync::Arc;

use gpui::{
    AnyElement, Context, Div, FontWeight, Hsla, ObjectFit, ScrollHandle, SharedString, Stateful, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, white,
};

use crate::{
    app::{Account, Load, MusicApp, Route, SIDEBAR_W, SeeAllItems},
    model::{Album, ArtColors, Artwork, Playlist, Song, format_date},
    theme::{Palette, Theme},
    ui::{GAP, PAD, explicit_badge, icon, icon_button, pill_button, scroll_row, skeleton_detail, skeleton_rows},
};

/// A click handler stored for later (header subtitle, add button).
type AppAction = Box<dyn Fn(&mut MusicApp, &mut Context<MusicApp>)>;
/// How far the page has scrolled down, in pixels.
fn scrolled(handle: &ScrollHandle) -> f32 {
    (-f32::from(handle.offset().y)).max(0.)
}

pub(crate) fn artist_hero_height(content_w: f32) -> f32 {
    (content_w * 0.56).clamp(420., 660.).floor()
}

/// Approximate height of the album/playlist header, for the compact bar threshold.
const COVER_HEADER_H: f32 = 520.;

/// Frosted circular button floating over artwork.
fn glass_button(id: &'static str, icon_name: &'static str, size: f32) -> Stateful<Div> {
    tahoe::primitives::glass_button(id, icon_name, size)
}

impl MusicApp {
    /// The tint for the current page, if it's one of the artwork-colored detail pages.
    pub(crate) fn page_palette(&self, cx: &mut Context<Self>) -> Option<Palette> {
        match &self.route {
            Route::Artist(id) => {
                let detail = self.artists.get(id)?.ready()?;
                let photo = self.artist_photo(detail);
                self.palette_for(detail.artist.colors, &photo, cx)
            }
            Route::Album(id) => {
                let (album, _) = self.albums.get(id)?.ready()?;
                self.palette_for(album.colors, &album.artwork, cx)
            }
            Route::Playlist(p) => {
                let art = self.playlist_tint_art(p);
                self.palette_for(p.colors, &art, cx)
            }
            _ => None,
        }
    }

    /// Apple's colors when the API provided them, otherwise the artwork's average color.
    fn palette_for(&self, colors: Option<ArtColors>, artwork: &Artwork, cx: &mut Context<Self>) -> Option<Palette> {
        let average = self.art_color(artwork, cx).map(Palette::from_average);
        match colors {
            Some(c) => {
                let mut palette = Palette::from_art_colors(c.bg, c.text);
                // Apple's text colors are often greys; borrow a hue from the image instead.
                if palette.accent.s < 0.3
                    && let Some(avg) = average
                {
                    palette.accent = avg.accent;
                }
                Some(palette)
            }
            None => average,
        }
    }

    fn artist_photo(&self, detail: &crate::model::ArtistDetail) -> Artwork {
        // Public-API artists have no photo; their newest cover stands in.
        if detail.artist.artwork.is_empty() {
            detail.latest().map(|a| a.artwork.clone()).unwrap_or_default()
        } else {
            detail.artist.artwork.clone()
        }
    }

    fn playlist_tint_art(&self, playlist: &Playlist) -> Artwork {
        if !playlist.artwork.is_empty() {
            return playlist.artwork.clone();
        }
        match self.playlist_tracks.get(&playlist.id) {
            Some(Load::Ready(Some(tracks))) => tracks.first().map(|s| s.artwork.clone()).unwrap_or_default(),
            _ => Artwork::default(),
        }
    }

    /// Full-bleed image with a parallax offset: it moves at `rate` of the scroll speed,
    /// so with 0.5 it drifts down half as fast as the page scrolls up.
    fn parallax_image(
        &self,
        artwork: &Artwork,
        w: f32,
        h: f32,
        scroll: f32,
        rate: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let want = (w.max(h) * 2.) as u32;
        let size = [600, 1000, 1600, 2400].into_iter().find(|&s| s >= want).unwrap_or(2400);
        let ph = (size as f32 * h / w).round() as u32;
        let url = if (w - h).abs() > 1. { artwork.url_cropped(size, ph.max(1)) } else { artwork.url(size) };
        let motion = self.artwork_motion(&artwork.url(1), 0.08);
        let image = self.image(&url, cx).map(|path| {
            img(path)
                .min_w(px(w))
                .max_w(px(w))
                .min_h(px(h))
                .max_h(px(h))
                .object_fit(ObjectFit::Cover)
                .when_some(motion, |img, (phase, strength)| img.motion(phase, strength))
        });
        div().absolute().left_0().top(px(scroll * rate)).w(px(w)).h(px(h)).children(image).into_any_element()
    }

    /// A tiny copy of the artwork stretched to fill an area: gpui can't blur, but upscaling
    /// a 24px image gives the same soft wash of color.
    fn blurred_backdrop(
        &self,
        artwork: &Artwork,
        w: f32,
        h: f32,
        scroll: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let path = self.image(&artwork.url(24), cx)?;
        // The wash is blurry, so it can move further than a cover without looking busy.
        let motion = self.artwork_motion(&artwork.url(24), 0.35);
        Some(
            div()
                .absolute()
                .left_0()
                .top(px(scroll * 0.5))
                .w(px(w))
                .h(px(h))
                .opacity(0.45)
                .child(
                    img(path)
                        .min_w(px(w))
                        .max_w(px(w))
                        .min_h(px(h))
                        .max_h(px(h))
                        .object_fit(ObjectFit::Cover)
                        .when_some(motion, |img, (phase, strength)| img.motion(phase, strength)),
                )
                .into_any_element(),
        )
    }

    /// Back (left) and open-in-Apple-Music (right) buttons floating over the header.
    /// `inset` is how far the header extends left under the sidebar.
    fn floating_controls(&self, url: String, inset: f32, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top(px(14.))
            .left(px(inset + 18.))
            .right(px(18.))
            .flex()
            .justify_between()
            .child(div().when(!self.history.is_empty(), |d| {
                d.child(
                    glass_button("hero-back", "chevron-left", 38.)
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                )
            }))
            .when(!url.is_empty(), |d| {
                d.child(
                    glass_button("hero-open", "open", 38.)
                        .on_click(cx.listener(move |this, _, _, cx| this.open_url(&url, cx))),
                )
            })
    }

    /// Editorial text clamped to two lines with a MORE / LESS toggle.
    fn notes(&self, t: &Theme, key: &str, text: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = strip_html(text);
        if text.trim().is_empty() {
            return None;
        }
        let expanded = self.expanded_notes.contains(key);
        let key = key.to_string();
        Some(
            div()
                .px(px(PAD))
                .pt_4()
                .child(
                    tahoe::ExpandableText::new(SharedString::from(format!("notes-{key}")), text)
                        .expanded(expanded)
                        .theme(*t)
                        .on_toggle(cx.listener(move |this, _, _, cx| {
                            if !this.expanded_notes.remove(&key) {
                                this.expanded_notes.insert(key.clone());
                            }
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    // Artist

    pub(crate) fn render_artist(&self, t: &Theme, id: &str, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let hero_h = artist_hero_height(content_w);
        let detail = match self.artists.get(id) {
            Some(Load::Ready(d)) => d,
            Some(Load::Failed(err)) => return self.error_state(t, err, cx).into_any_element(),
            // Hold the hero's space so the page doesn't jump when the artist arrives.
            _ => {
                return div()
                    .child(
                        div()
                            .relative()
                            .ml(px(-SIDEBAR_W))
                            .w(px(content_w + SIDEBAR_W))
                            .h(px(hero_h))
                            .bg(t.placeholder)
                            .child(self.floating_controls(String::new(), SIDEBAR_W, cx)),
                    )
                    .child(skeleton_rows(t, 8))
                    .into_any_element();
            }
        };
        let scroll = scrolled(&self.scroll);
        let photo = self.artist_photo(detail);
        let palette = self.palette_for(detail.artist.colors, &photo, cx);
        let on_accent = palette.map_or(white(), |p| p.on_accent());
        let fade = (1. - scroll / (hero_h * 0.55)).clamp(0., 1.);
        let top = Arc::new(detail.top_songs.clone());
        let (play_top, shuffle_top) = (top.clone(), top.clone());
        let about_key = format!("artist-{id}");

        let full_w = content_w + SIDEBAR_W;
        let hero = div()
            .relative()
            .ml(px(-SIDEBAR_W))
            .w(px(full_w))
            .h(px(hero_h))
            .overflow_hidden()
            .bg(t.window)
            .child(self.parallax_image(&photo, full_w, hero_h, scroll, 0.5, cx))
            // Darken the top a touch so the floating buttons read on bright photos.
            .child(div().absolute().top_0().left_0().right_0().h(px(110.)).bg(linear_gradient(
                180.,
                linear_color_stop(gpui::black().opacity(0.3), 0.),
                linear_color_stop(gpui::black().opacity(0.), 1.),
            )))
            // The photo dissolves into the page's tint, as on iOS.
            .child(div().absolute().left_0().right_0().bottom_0().h(relative(0.62)).bg(linear_gradient(
                180.,
                linear_color_stop(t.window.opacity(0.), 0.),
                linear_color_stop(t.window, 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(SIDEBAR_W + PAD))
                    .right(px(PAD))
                    .bottom(px(24. - scroll * 0.2))
                    .opacity(fade)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_5()
                    .child(
                        div()
                            .max_w(px(820.))
                            .text_center()
                            .text_size(px(64.))
                            .line_height(px(68.))
                            .font_weight(FontWeight::BLACK)
                            .text_color(white())
                            .line_clamp(2)
                            .child(detail.artist.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_6()
                            .child(glass_button("artist-about", "lyrics", 46.).on_click(cx.listener({
                                let key = about_key.clone();
                                move |this, _, _, cx| {
                                    if !this.expanded_notes.remove(&key) {
                                        this.expanded_notes.insert(key.clone());
                                    }
                                    cx.notify();
                                }
                            })))
                            .child(
                                circle(t, "artist-hero-play", "play", 66., t.accent, on_accent).shadow_lg().on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.play(play_top.as_ref().clone(), 0, false, cx);
                                    }),
                                ),
                            )
                            .child(glass_button("artist-shuffle", "shuffle", 46.).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let start = shuffle_top.len() / 2;
                                    this.play(shuffle_top.as_ref().clone(), start, true, cx);
                                },
                            ))),
                    ),
            )
            .child(self.floating_controls(detail.artist.url.clone(), SIDEBAR_W, cx));

        let col_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();
        let (albums, singles) = detail.split_releases();
        let mut page = div().child(hero);

        if self.expanded_notes.contains(&about_key) {
            let about = if detail.about.is_empty() {
                format!("{} · {}", detail.artist.name, detail.artist.genre)
            } else {
                strip_html(&detail.about)
            };
            page = page.child(
                div()
                    .mx(px(PAD))
                    .mt_2()
                    .p_5()
                    .max_w(px(860.))
                    .rounded(px(18.))
                    .bg(t.hover)
                    .border_1()
                    .border_color(t.separator)
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(t.text_secondary)
                    .child(about),
            );
        }
        if let Some(latest) = detail.latest() {
            page = page.child(div().pt_6().child(self.latest_release(t, latest, content_w - 2. * PAD, cx)));
        }
        if !detail.top_songs.is_empty() {
            let artist_id = id.to_string();
            page = page
                .child(self.section_link(
                    t,
                    "Top Songs",
                    move |this| {
                        (match this.artists.get(&artist_id) {
                            Some(Load::Ready(d)) => Some(d),
                            _ => None,
                        })
                        .map(|d| SeeAllItems::Songs(Arc::new(d.top_songs.clone())))
                    },
                    cx,
                ))
                .child(self.song_columns_by_album(t, "artist-top", detail.top_songs.clone(), col_w, cx));
        }
        for (title, prefix, list) in
            [("Albums", "artist-albums", &albums), ("Singles & EPs", "artist-singles", &singles)]
        {
            if list.is_empty() {
                continue;
            }
            let mut row = scroll_row(prefix);
            for (i, album) in list.iter().enumerate() {
                row = row.child(self.album_tile(t, prefix, i, album, 180., cx));
            }
            let (artist_id, singles) = (id.to_string(), prefix == "artist-singles");
            page = page
                .child(self.section_link(
                    t,
                    title,
                    move |this| {
                        (match this.artists.get(&artist_id) {
                            Some(Load::Ready(d)) => Some(d),
                            _ => None,
                        })
                        .map(|d| {
                            let (albums, single_list) = d.split_releases();
                            SeeAllItems::Albums(Arc::new(if singles { single_list } else { albums }))
                        })
                    },
                    cx,
                ))
                .child(row);
        }
        if !detail.similar.is_empty() {
            let mut row = scroll_row("similar-artists");
            for (i, artist) in detail.similar.iter().enumerate() {
                let id = artist.id.clone();
                row = row.child(
                    div()
                        .id(("similar", i))
                        .w(px(130.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .child(self.avatar(&artist.artwork, &artist.name, 120., t, cx))
                        .child(div().max_w_full().truncate().child(artist.name.clone()))
                        .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Artist(id.clone()), cx))),
                );
            }
            let artist_id = id.to_string();
            page = page
                .child(self.section_link(
                    t,
                    "Similar Artists",
                    move |this| {
                        (match this.artists.get(&artist_id) {
                            Some(Load::Ready(d)) => Some(d),
                            _ => None,
                        })
                        .map(|d| SeeAllItems::Artists(Arc::new(d.similar.clone())))
                    },
                    cx,
                ))
                .child(row);
        }
        page.into_any_element()
    }

    /// The artist's newest release as a full-width strip in the album's own colors, as in
    /// Apple Music: a wash of its artwork behind, the cover with a play button on hover, and
    /// Play / Add on the right.
    fn latest_release(&self, t: &Theme, album: &Album, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        const ART: f32 = 84.;
        const H: f32 = ART + 28.;
        const RADIUS: f32 = 16.;
        let in_library = self.album_in_library(album);
        let kind = match album.track_count {
            Some(1..=3) => "Single",
            Some(4..=6) => "EP",
            _ => "Album",
        };
        let meta = match album.track_count {
            Some(1) => format!("{kind} · 1 song · {}", format_date(&album.release_date)),
            Some(n) => format!("{kind} · {n} songs · {}", format_date(&album.release_date)),
            None => format!("{kind} · {}", format_date(&album.release_date)),
        };
        // Apple's own background color for the artwork when it has one, darkened so white text
        // reads on it; otherwise the artwork's average color.
        let base = album
            .colors
            .map(|c| Palette::from_art_colors(c.bg, c.text).bg)
            .or_else(|| self.art_color(&album.artwork, cx))
            .unwrap_or(t.hover);
        let bg = Hsla { s: base.s.min(0.55), l: base.l.clamp(0.1, 0.2), a: 1., ..base };
        let wash = self.image(&album.artwork.url(24), cx);
        let (open, play, play_art, add) = (album.id.clone(), album.id.clone(), album.id.clone(), album.clone());

        div()
            .id("latest-release")
            .group("latest-release")
            .mx(px(PAD))
            .w(px(width))
            .h(px(H))
            .relative()
            .overflow_hidden()
            .rounded(px(RADIUS))
            .bg(bg)
            .border_1()
            .border_color(white().opacity(0.08))
            .shadow_md()
            .cursor_pointer()
            .hover(|s| s.border_color(white().opacity(0.16)))
            .on_click(cx.listener(move |this, _, _, cx| this.push(Route::Album(open.clone()), cx)))
            // A soft, stretched copy of the cover, strongest behind the art and fading out.
            .children(wash.map(|path| {
                div().absolute().left_0().top_0().opacity(0.6).child(
                    img(path).min_w(px(width)).max_w(px(width)).min_h(px(H)).max_h(px(H)).object_fit(ObjectFit::Cover),
                )
            }))
            .child(div().absolute().inset_0().bg(linear_gradient(
                90.,
                linear_color_stop(bg.opacity(0.2), 0.),
                linear_color_stop(bg.opacity(0.9), 0.6),
            )))
            .child(
                div()
                    .relative()
                    .size_full()
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .rounded(px(8.))
                            .shadow_md()
                            .child(self.art(&album.artwork, ART, 8., t, cx))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(8.))
                                    .bg(gpui::black().opacity(0.3))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(0.)
                                    .group_hover("latest-release", |s| s.opacity(1.))
                                    .child(
                                        circle(t, "latest-play-art", "play", 36., white().opacity(0.92), gpui::black())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.play_album(play_art.clone(), cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .text_color(white())
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(white().opacity(0.6))
                                    .child("LATEST RELEASE"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_shrink()
                                            .truncate()
                                            .text_size(px(17.))
                                            .font_weight(FontWeight::BOLD)
                                            .child(album.title.clone()),
                                    )
                                    .when(album.explicit, |d| d.child(div().flex_none().child(explicit_badge(t)))),
                            )
                            .child(div().truncate().text_size(px(13.)).text_color(white().opacity(0.7)).child(meta)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                crate::ui::button(t, "latest-play", "play", "Play", white().into(), gpui::black())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.play_album(play.clone(), cx);
                                    })),
                            )
                            .child(
                                circle(
                                    t,
                                    "latest-add",
                                    if in_library { "check" } else { "plus" },
                                    32.,
                                    white().opacity(0.14),
                                    white(),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.add_album(&add, cx);
                                })),
                            ),
                    ),
            )
    }

    /// Adds an album to the library (Apple Music when signed in), fetching tracks if needed.
    fn add_album(&mut self, album: &Album, cx: &mut Context<Self>) {
        if self.account == Account::SignedIn {
            if self.album_in_library(album) {
                return self.show_notice("Apple's API can't remove albums. Remove it in the Music app.".into(), cx);
            }
            let label = format!("“{}”", album.title);
            return self.add_to_library("albums", album.id.clone(), label, cx);
        }
        let tracks = match self.albums.get(&album.id) {
            Some(Load::Ready((_, tracks))) => tracks.clone(),
            _ => Vec::new(),
        };
        self.library.toggle_album(album, &tracks);
        cx.notify();
    }

    // Album & playlist

    /// Shared iOS-style header: soft wash of the cover's colors, the cover centered and
    /// drifting with the scroll, then title, subtitle, details and the action row.
    #[allow(clippy::too_many_arguments)]
    fn cover_header(
        &self,
        t: &Theme,
        artwork: &Artwork,
        cover: impl FnOnce(f32, &mut Context<Self>) -> AnyElement,
        title: String,
        subtitle: Option<(String, AppAction)>,
        meta: String,
        actions: AnyElement,
        url: String,
        content_w: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scroll = scrolled(&self.scroll);
        let cover_size = (content_w * 0.32).clamp(240., 340.).floor();
        let fade = (1. - scroll / (cover_size + 120.)).clamp(0., 1.);
        let glow = Hsla { l: (t.window.l + 0.1).min(0.55), ..t.window };
        let wash_h = cover_size + 220.;

        let subtitle = subtitle.map(|(label, on_click)| {
            div()
                .id("cover-subtitle")
                .max_w(px(640.))
                .text_center()
                .text_size(px(21.))
                .text_color(t.text)
                .line_clamp(1)
                .cursor_pointer()
                .hover(|s| s.underline())
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
        });

        let full_w = content_w + SIDEBAR_W;
        div()
            .relative()
            .ml(px(-SIDEBAR_W))
            .w(px(full_w))
            .pl(px(SIDEBAR_W))
            .overflow_hidden()
            .bg(t.window)
            .child(div().absolute().top_0().left_0().right_0().h(px(wash_h)).bg(linear_gradient(
                180.,
                linear_color_stop(glow, 0.),
                linear_color_stop(glow.opacity(0.), 1.),
            )))
            .children(self.blurred_backdrop(artwork, full_w, wash_h, scroll, cx))
            // Fade everything above into the page color well before the header ends.
            .child(div().absolute().left_0().right_0().top(px(wash_h * 0.4)).bottom_0().bg(linear_gradient(
                180.,
                linear_color_stop(t.window.opacity(0.), 0.),
                linear_color_stop(t.window, 0.55),
            )))
            .child(
                div()
                    .relative()
                    .pt(px(72.))
                    .px(px(PAD))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .relative()
                            .top(px(scroll * 0.35))
                            .opacity(fade)
                            .rounded(px(10.))
                            .shadow_lg()
                            .child(cover(cover_size, cx)),
                    )
                    .child(
                        div()
                            .pt_6()
                            .max_w(px(720.))
                            .text_center()
                            .text_size(px(28.))
                            .line_height(px(34.))
                            .font_weight(FontWeight::BOLD)
                            .line_clamp(2)
                            .child(title),
                    )
                    .children(subtitle)
                    .child(
                        div()
                            .pt_1()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text_secondary)
                            .child(meta),
                    )
                    .child(div().pt_5().child(actions)),
            )
            .child(self.floating_controls(url, SIDEBAR_W, cx))
    }

    /// Shuffle · wide white Play · add, as on iOS.
    fn cover_actions(
        &self,
        t: &Theme,
        songs: Option<Arc<Vec<Song>>>,
        added: bool,
        on_add: AppAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = songs.as_ref().is_some_and(|s| !s.is_empty());
        let (play, shuffle) = (songs.clone(), songs);
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(
                circle(t, "cover-shuffle", "shuffle", 48., t.button, t.text)
                    .when(!enabled, |d| d.opacity(0.4))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = &shuffle {
                            this.play(s.as_ref().clone(), s.len() / 2, true, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("cover-play")
                    .w(px(220.))
                    .h(px(48.))
                    .rounded_full()
                    .bg(white())
                    .when(!enabled, |d| d.opacity(0.4))
                    .hover(|s| s.opacity(0.9))
                    .active(|s| s.opacity(0.75))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_size(px(17.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui::black())
                    .child(icon("play", 15., gpui::black()))
                    .child("Play")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(s) = &play {
                            this.play(s.as_ref().clone(), 0, false, cx);
                        }
                    })),
            )
            .child(
                circle(t, "cover-add", if added { "check" } else { "plus" }, 48., t.button, t.text)
                    .on_click(cx.listener(move |this, _, _, cx| on_add(this, cx))),
            )
            .into_any_element()
    }

    pub(crate) fn render_album(&self, t: &Theme, id: &str, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let (album, tracks) = match self.albums.get(id) {
            Some(Load::Ready((album, tracks))) => (album, tracks),
            Some(Load::Failed(err)) => return self.error_state(t, err, cx).into_any_element(),
            _ => {
                return div()
                    .relative()
                    .child(skeleton_detail(t))
                    .child(self.floating_controls(String::new(), 0., cx))
                    .into_any_element();
            }
        };
        let list = Arc::new(tracks.clone());
        let total_min = tracks.iter().map(|s| s.duration_ms).sum::<u32>().div_ceil(60_000);

        let mut rows = div().px(px(PAD)).pt_4().flex().flex_col();
        for (ix, song) in tracks.iter().enumerate() {
            rows = rows.child(self.album_track_row(t, &list, ix, song, cx));
        }

        let add = album.clone();
        let actions = self.cover_actions(
            t,
            Some(list.clone()),
            self.album_in_library(album),
            Box::new(move |this, cx| this.add_album(&add, cx)),
            cx,
        );
        let (artist, artist_id) = (album.artist.clone(), album.artist_id.clone());
        let subtitle: AppAction = Box::new(move |this, cx| this.open_artist(artist.clone(), artist_id.clone(), cx));
        let meta = [album.genre.clone(), album.year().to_string()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let artwork = album.artwork.clone();

        div()
            .child(self.cover_header(
                t,
                &album.artwork,
                move |size, cx| self.living_art(&artwork, size, 10., t, cx),
                album.title.clone(),
                Some((album.artist.clone(), subtitle)),
                meta,
                actions,
                album.url.clone(),
                content_w,
                cx,
            ))
            .children(self.notes(t, &format!("album-{}", album.id), &album.description, cx))
            .child(rows)
            .child(div().px(px(PAD)).pt_4().text_size(px(12.5)).text_color(t.text_secondary).child(format!(
                "{} · {} songs, {} minutes",
                format_date(&album.release_date),
                tracks.len(),
                total_min
            )))
            .children(self.more_by_artist(t, album, cx))
            .children(self.you_might_also_like(t, album, cx))
            .into_any_element()
    }

    /// Apple's related albums, as in the Music app.
    fn you_might_also_like(&self, t: &Theme, album: &Album, cx: &mut Context<Self>) -> Option<AnyElement> {
        if album.id.starts_with("l.") {
            return None;
        }
        self.ensure_related_albums(&album.id, cx);
        let Some(Load::Ready(related)) = self.related_albums.get(&album.id) else {
            return None;
        };
        if related.is_empty() {
            return None;
        }
        let mut row = scroll_row("also-like");
        for (i, a) in related.iter().enumerate() {
            row = row.child(self.album_tile(t, "also-like", i, a, 170., cx));
        }
        let id = album.id.clone();
        Some(
            div()
                .child(self.section_link(
                    t,
                    "You Might Also Like",
                    move |this| match this.related_albums.get(&id) {
                        Some(Load::Ready(albums)) => Some(SeeAllItems::Albums(Arc::new(albums.clone()))),
                        _ => None,
                    },
                    cx,
                ))
                .child(row)
                .into_any_element(),
        )
    }

    /// "More by <artist>" row under an album; loads the artist in the background.
    fn more_by_artist(&self, t: &Theme, album: &Album, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = album.artist_id.as_ref()?;
        self.ensure_artist(id, cx);
        let Some(Load::Ready(detail)) = self.artists.get(id) else {
            return None;
        };
        let others: Vec<&Album> = detail.albums.iter().filter(|a| a.id != album.id).take(15).collect();
        if others.is_empty() {
            return None;
        }
        let mut row = scroll_row("more-by-artist");
        for (i, a) in others.into_iter().enumerate() {
            row = row.child(self.album_tile(t, "more-by", i, a, 170., cx));
        }
        Some(
            div()
                .child(
                    div()
                        .px(px(PAD))
                        .pt_7()
                        .pb_2p5()
                        .text_size(px(19.))
                        .font_weight(FontWeight::BOLD)
                        .child(format!("More by {}", album.artist)),
                )
                .child(row)
                .into_any_element(),
        )
    }

    pub(crate) fn render_playlist(
        &self,
        t: &Theme,
        playlist: &Playlist,
        content_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tracks: Option<Arc<Vec<Song>>> = match self.playlist_tracks.get(&playlist.id) {
            Some(Load::Ready(Some(tracks))) => Some(Arc::new(tracks.clone())),
            _ => None,
        };
        let for_save = playlist.clone();
        let actions = self.cover_actions(
            t,
            tracks.clone(),
            self.playlist_in_library(playlist),
            Box::new(move |this, cx| {
                if this.account != Account::SignedIn {
                    this.library.toggle_playlist(&for_save);
                } else if this.playlist_in_library(&for_save) {
                    this.show_notice("Apple's API can't remove playlists. Remove it in the Music app.".into(), cx);
                } else {
                    let label = format!("“{}”", for_save.name);
                    this.add_to_library("playlists", for_save.id.clone(), label, cx);
                }
                cx.notify();
            }),
            cx,
        );
        let meta = match &tracks {
            Some(tracks) => {
                let mins = tracks.iter().map(|s| s.duration_ms).sum::<u32>().div_ceil(60_000);
                format!("{} songs · {} min", tracks.len(), mins)
            }
            None => "Playlist".into(),
        };
        let curator = (!playlist.curator.is_empty()).then(|| {
            let noop: AppAction = Box::new(|_, _| {});
            (playlist.curator.clone(), noop)
        });

        let body: AnyElement = match self.playlist_tracks.get(&playlist.id) {
            Some(Load::Ready(Some(tracks))) => self.song_table(t, "playlist", tracks.clone(), cx).into_any_element(),
            Some(Load::Ready(None)) => div()
                .mx(px(PAD))
                .mt_4()
                .p_5()
                .rounded(px(16.))
                .bg(t.hover)
                .border_1()
                .border_color(t.separator)
                .flex()
                .flex_col()
                .gap_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Track list unavailable"))
                .child(div().text_color(t.text_secondary).child(
                    "Apple's public APIs don't list the songs in editorial playlists. Add a MusicKit key \
                     to load them here (see README), or open the playlist in Apple Music.",
                ))
                .into_any_element(),
            // Apple refuses some library playlists (e.g. ones shared with you) to apps.
            Some(Load::Failed(err)) if err.contains("http status: 403") => div()
                .mx(px(PAD))
                .mt_4()
                .p_5()
                .rounded(px(16.))
                .bg(t.hover)
                .border_1()
                .border_color(t.separator)
                .flex()
                .flex_col()
                .gap_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Apple doesn't share this playlist with apps"))
                .child(div().text_color(t.text_secondary).child("Open it in Apple Music to play it."))
                .into_any_element(),
            Some(Load::Failed(err)) => self.error_state(t, err, cx).into_any_element(),
            _ => skeleton_rows(t, 8).into_any_element(),
        };
        let tint_art = self.playlist_tint_art(playlist);
        let for_cover = playlist.clone();

        div()
            .child(self.cover_header(
                t,
                &tint_art,
                move |size, cx| {
                    if for_cover.artwork.is_empty() {
                        self.playlist_art(&for_cover, size, 10., t, cx)
                    } else {
                        self.living_art(&for_cover.artwork, size, 10., t, cx)
                    }
                },
                playlist.name.clone(),
                curator,
                meta,
                actions,
                playlist.url.clone(),
                content_w,
                cx,
            ))
            .children(self.notes(t, &format!("playlist-{}", playlist.id), &playlist.description, cx))
            .child(body)
            .into_any_element()
    }

    /// Title bar that fades in over the top of the page once its header has scrolled away.
    pub(crate) fn render_compact_bar(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (title, threshold, songs): (String, f32, Vec<Song>) = match &self.route {
            Route::Artist(id) => {
                let d = self.artists.get(id)?.ready()?;
                (d.artist.name.clone(), artist_hero_height(content_w) - 70., d.top_songs.clone())
            }
            Route::Album(id) => {
                let (album, tracks) = self.albums.get(id)?.ready()?;
                (album.title.clone(), COVER_HEADER_H - 90., tracks.clone())
            }
            Route::Playlist(p) => {
                let tracks = match self.playlist_tracks.get(&p.id) {
                    Some(Load::Ready(Some(tracks))) => tracks.clone(),
                    _ => Vec::new(),
                };
                (p.name.clone(), COVER_HEADER_H - 90., tracks)
            }
            _ => return None,
        };
        let opacity = ((scrolled(&self.scroll) - threshold) / 40.).clamp(0., 1.);
        if opacity <= 0. {
            return None;
        }
        Some(
            div()
                .id("compact-bar")
                .occlude()
                .absolute()
                .top_0()
                .left(px(SIDEBAR_W))
                .right_0()
                .h(px(52.))
                .opacity(opacity)
                .bg(t.window.opacity(0.94))
                .border_b_1()
                .border_color(t.separator)
                .flex()
                .items_center()
                .px_4()
                .gap_3()
                .child(div().w(px(32.)).when(!self.history.is_empty(), |d| {
                    d.child(
                        icon_button(t, "compact-back", "chevron-left", 16., t.icon)
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    )
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_center()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(15.))
                        .child(SharedString::from(title)),
                )
                .child(div().w(px(32.)).when(!songs.is_empty(), |d| {
                    d.child(
                        circle(t, "compact-play", "play", 30., t.accent, white())
                            .on_click(cx.listener(move |this, _, _, cx| this.play(songs.clone(), 0, false, cx))),
                    )
                }))
                .into_any_element(),
        )
    }

    /// The right-hand pane of the library Artists page, with the same hero treatment.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn library_artist_hero(
        &self,
        t: &Theme,
        name: &str,
        artwork: &Artwork,
        width: f32,
        play: Arc<Vec<Song>>,
        open: (String, Option<String>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scroll = scrolled(&self.artist_pane_scroll);
        let h = (width * 0.5).clamp(280., 420.).floor();
        let fade = (1. - scroll / (h * 0.6)).clamp(0., 1.);
        div()
            .relative()
            .w_full()
            .h(px(h))
            .overflow_hidden()
            .bg(t.placeholder)
            .when(!artwork.is_empty(), |d| d.child(self.parallax_image(artwork, width, h, scroll, 0.5, cx)))
            .when(artwork.is_empty(), |d| {
                d.child(div().absolute().inset_0().flex().items_center().justify_center().child(icon(
                    "mic",
                    h * 0.3,
                    white().opacity(0.25),
                )))
            })
            .child(div().absolute().left_0().right_0().bottom_0().h(relative(0.6)).bg(linear_gradient(
                180.,
                linear_color_stop(t.window.opacity(0.), 0.),
                linear_color_stop(t.window, 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(PAD))
                    .right(px(PAD))
                    .bottom(px(20. - scroll * 0.2))
                    .opacity(fade)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .text_center()
                            .text_size(px(42.))
                            .line_height(px(46.))
                            .font_weight(FontWeight::BLACK)
                            .line_clamp(2)
                            .child(name.to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(pill_button(t, "artist-page", "mic", "Artist Page", false).on_click(
                                cx.listener(move |this, _, _, cx| this.open_artist(open.0.clone(), open.1.clone(), cx)),
                            ))
                            .child(
                                circle(t, "library-artist-play", "play", 50., t.accent, white()).shadow_lg().on_click(
                                    cx.listener(move |this, _, _, cx| this.play(play.as_ref().clone(), 0, false, cx)),
                                ),
                            ),
                    ),
            )
    }
}

/// Round icon button.
fn circle(_t: &Theme, id: &'static str, icon_name: &'static str, size: f32, bg: Hsla, fg: Hsla) -> Stateful<Div> {
    tahoe::primitives::circle_button(id, icon_name, size, bg, fg)
}

fn relative(f: f32) -> gpui::DefiniteLength {
    gpui::relative(f)
}

/// Apple's editorial notes can contain light HTML (`<i>`, `<b>`, `<br>`).
fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_editorial_html() {
        assert_eq!(super::strip_html("<i>Lodi</i> &amp; more<br/>"), "Lodi & more");
    }
}
