//! Full-window Now Playing: large artwork over a wash of its colors, big controls, and a
//! side panel with synced lyrics or Up Next.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, FontWeight, Hsla, MouseButton, MouseDownEvent, ObjectFit, Window, div, img,
    linear_color_stop, linear_gradient, point, prelude::*, px, relative, white,
};

use crate::{
    app::{Drag, Load, MusicApp, PlayState},
    lyrics::Lyrics,
    model::Song,
    queue::Repeat,
    theme::{Palette, Theme},
    views::{capture_bounds, icon},
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NowPlayingTab {
    Lyrics,
    UpNext,
}

/// Auto-scroll state for the lyrics panel.
#[derive(Default)]
pub struct LyricsScroll {
    /// The user scrolled by hand; auto-follow pauses for a moment.
    pub(crate) user_until: Option<Instant>,
}

impl MusicApp {
    pub(crate) fn open_now_playing(&mut self, tab: Option<NowPlayingTab>, cx: &mut Context<Self>) {
        if self.queue.current().is_none() {
            return;
        }
        if let Some(tab) = tab {
            self.now_playing_tab = tab;
        }
        self.show_now_playing = true;
        self.menu = None;
        self.show_queue = false;
        if let Some(song) = self.queue.current().cloned() {
            self.ensure_lyrics(&song, cx);
        }
        cx.notify();
    }

    pub(crate) fn ensure_lyrics(&mut self, song: &Song, cx: &mut Context<Self>) {
        if self.lyrics.contains_key(&song.id) {
            return;
        }
        self.lyrics.insert(song.id.clone(), Load::Loading);
        let (id, title, artist, album, secs) =
            (song.id.clone(), song.title.clone(), song.artist.clone(), song.album.clone(), song.duration_ms as f32 / 1000.);
        self.fetch(
            cx,
            move |_| crate::lyrics::fetch(&title, &artist, &album, secs),
            move |this, load, _| {
                this.lyrics.insert(id, load);
            },
        );
    }

    pub(crate) fn render_now_playing(&self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().now_playing;
        if !presence.visible() {
            return None;
        }
        let p = presence.value();
        let song = self.queue.current().cloned()?;
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let palette = self.art_color(&song.artwork, cx).map(Palette::from_average);
        let base: Hsla = palette.map_or(t.window, |p| Hsla { l: p.bg.l.min(0.2), ..p.bg });
        let accent = palette.map_or(t.accent, |p| p.accent);
        let playing = matches!(self.state, PlayState::Playing | PlayState::Loading);
        if playing {
            window.request_animation_frame();
        }

        let panel_w = (vw * 0.36).clamp(340., 500.);
        let art_size = (vh - 330.).min(vw - panel_w - 200.).clamp(200., 520.);
        let duration = self.duration();
        let position = match self.np_scrub {
            Some(f) => f * duration,
            None => self.position().min(duration),
        };
        let fraction = if duration > 0. { position / duration } else { 0. };
        let fmt = |s: f32| {
            let s = s.max(0.) as u32;
            format!("{}:{:02}", s / 60, s % 60)
        };
        let white_a = |a: f32| white().opacity(a);
        let big = |id: &'static str, name: &'static str, size: f32, color: Hsla| {
            div()
                .id(id)
                .size(px(size + 20.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white_a(0.1)))
                .active(|s| s.opacity(0.6))
                .child(icon(name, size, color))
        };
        let toggle = |on: bool| if on { accent } else { white_a(0.7) };

        let backdrop = self.cached_image(&song.artwork.url(24)).or_else(|| self.image(&song.artwork.url(24), cx));
        let artwork = self.art(&song.artwork, art_size, 14., t, cx);
        let (artist, artist_id) = (song.artist.clone(), song.artist_id.clone());

        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_5()
            .child(div().rounded(px(14.)).shadow_lg().child(artwork))
            .child(
                div()
                    .w(px(art_size.max(360.)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(white())
                            .truncate()
                            .child(song.title.clone()),
                    )
                    .child(
                        div()
                            .id("np-big-artist")
                            .text_size(px(17.))
                            .text_color(white_a(0.7))
                            .truncate()
                            .cursor_pointer()
                            .hover(|s| s.text_color(white()))
                            .child(format!("{} — {}", song.artist, song.album))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.show_now_playing = false;
                                this.open_artist(artist.clone(), artist_id.clone(), cx);
                            })),
                    )
                    .child(
                        div()
                            .id("np-big-progress")
                            .relative()
                            .mt_4()
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                    this.drag = Some(Drag::BigProgress);
                                    this.drag_to(Drag::BigProgress, e.position, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(capture_bounds(self.np_progress_bounds.clone()))
                            .child(
                                div()
                                    .w_full()
                                    .h(px(6.))
                                    .rounded_full()
                                    .bg(white_a(0.22))
                                    .child(div().h_full().rounded_full().w(relative(fraction)).bg(white_a(0.92))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(12.))
                            .text_color(white_a(0.6))
                            .child(fmt(position))
                            .child(format!("-{}", fmt(duration - position))),
                    )
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_5()
                            .child(big("np-shuffle", "shuffle", 18., toggle(self.queue.shuffle)).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.queue.toggle_shuffle();
                                    cx.notify();
                                },
                            )))
                            .child(big("np-prev", "prev", 28., white()).on_click(cx.listener(|this, _, _, cx| this.previous(cx))))
                            .child(
                                div()
                                    .id("np-play")
                                    .size(px(72.))
                                    .rounded_full()
                                    .bg(white())
                                    .shadow_lg()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|s| s.opacity(0.9))
                                    .active(|s| s.opacity(0.75))
                                    .child(div().when(!playing, |d| d.ml(px(3.))).child(icon(
                                        if playing { "pause" } else { "play" },
                                        30.,
                                        base,
                                    )))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
                            )
                            .child(big("np-next", "next", 28., white()).on_click(cx.listener(|this, _, _, cx| this.next(cx))))
                            .child(
                                big(
                                    "np-repeat",
                                    if self.queue.repeat == Repeat::One { "repeat-one" } else { "repeat" },
                                    18.,
                                    toggle(self.queue.repeat != Repeat::Off),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.queue.cycle_repeat();
                                    cx.notify();
                                })),
                            ),
                    ),
            );

        let panel_body: AnyElement = match self.now_playing_tab {
            NowPlayingTab::Lyrics => self.render_lyrics_panel(&song, window, cx),
            NowPlayingTab::UpNext => self.render_up_next_panel(t, cx),
        };
        let tab = |id: &'static str, label: &'static str, which: NowPlayingTab| {
            let on = self.now_playing_tab == which;
            div()
                .id(id)
                .px_3()
                .h(px(28.))
                .rounded_full()
                .flex()
                .items_center()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .when(on, |d| d.bg(white_a(0.18)).text_color(white()))
                .when(!on, |d| d.text_color(white_a(0.6)).hover(|s| s.text_color(white())))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.now_playing_tab = which;
                    cx.notify();
                }))
        };
        let right = div()
            .w(px(panel_w))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(tab("np-tab-lyrics", "Lyrics", NowPlayingTab::Lyrics))
                    .child(tab("np-tab-queue", "Up Next", NowPlayingTab::UpNext)),
            )
            .child(div().flex_1().min_h_0().child(panel_body));

        Some(
            div()
                .id("now-playing-view")
                .occlude()
                .absolute()
                .inset_0()
                .opacity(p)
                .top(px((1. - p) * 60.))
                .bg(base)
                .font_family(".SystemUIFont")
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .children(backdrop.map(|path| {
                    div().absolute().inset_0().opacity(0.55).child(
                        img(path)
                            .min_w(px(vw))
                            .min_h(px(vh))
                            .max_w(px(vw))
                            .max_h(px(vh))
                            .object_fit(ObjectFit::Cover),
                    )
                }))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    180.,
                    linear_color_stop(base.opacity(0.35), 0.),
                    linear_color_stop(base.opacity(0.85), 1.),
                )))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .pt(px(56.))
                        .pb(px(40.))
                        .px(px(56.))
                        .flex()
                        .gap(px(48.))
                        .child(left)
                        .child(right),
                )
                .child(
                    div()
                        .id("np-close")
                        .absolute()
                        .top(px(14.))
                        .left(px(90.))
                        .size(px(32.))
                        .rounded_full()
                        .bg(white_a(0.12))
                        .hover(|s| s.bg(white_a(0.22)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("chevron-down", 16., white()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_now_playing = false;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    fn render_lyrics_panel(&self, song: &Song, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let message = |text: &str| {
            div()
                .pt(px(60.))
                .text_size(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(white().opacity(0.55))
                .child(text.to_string())
                .into_any_element()
        };
        let lyrics: &Lyrics = match self.lyrics.get(&song.id) {
            Some(Load::Ready(Some(l))) => l,
            Some(Load::Ready(None)) => return message("No lyrics available for this song."),
            Some(Load::Failed(_)) => return message("Couldn't load lyrics."),
            _ => return message("Loading lyrics…"),
        };
        // Previews are a clip from mid-song, so synced timings don't apply to them.
        let full = self.is_full_playback();
        let current = if full { lyrics.current(self.position()) } else { None };
        if full {
            self.follow_lyrics(current, window);
        }

        let mut list = div()
            .id("lyrics")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.lyrics_scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, _| {
                this.lyrics_follow.borrow_mut().user_until = Some(Instant::now() + Duration::from_secs(4));
            }))
            .flex()
            .flex_col()
            .gap_4()
            .pt(px(40.))
            .pb(px(300.));
        for (i, line) in lyrics.lines.iter().enumerate() {
            let is_current = current == Some(i);
            let past = current.is_some_and(|c| i < c);
            let at = line.at;
            list = list.child(
                div()
                    .id(("lyric", i))
                    .rounded(px(10.))
                    .px_2()
                    .py_1()
                    .text_size(px(26.))
                    .line_height(px(33.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(if !lyrics.synced || !full {
                        white().opacity(0.85)
                    } else if is_current {
                        white()
                    } else if past {
                        white().opacity(0.4)
                    } else {
                        white().opacity(0.3)
                    })
                    .when(at.is_some() && full, |d| d.cursor_pointer().hover(|s| s.bg(white().opacity(0.08))))
                    .child(if line.text.is_empty() { "♪".to_string() } else { line.text.clone() })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(at) = at.filter(|_| this.is_full_playback()) {
                            this.seek_to(at, cx);
                        }
                    })),
            );
        }
        list.child(
            div()
                .pt_6()
                .px_2()
                .text_size(px(11.))
                .text_color(white().opacity(0.35))
                .child("Lyrics from LRCLIB"),
        )
        .into_any_element()
    }

    /// Eases the lyrics so the current line sits about a third of the way down, unless
    /// the user has just scrolled them by hand.
    fn follow_lyrics(&self, current: Option<usize>, window: &mut Window) {
        let state = self.lyrics_follow.borrow();
        if state.user_until.is_some_and(|u| Instant::now() < u) {
            return;
        }
        let Some(item) = current.and_then(|i| self.lyrics_scroll.bounds_for_item(i)) else {
            return;
        };
        // Child bounds are layout positions, before the scroll offset is applied, so the
        // offset that puts the line at the target is independent of the current offset.
        let view = self.lyrics_scroll.bounds();
        let target_top = view.origin.y + view.size.height * 0.32;
        let max = f32::from(self.lyrics_scroll.max_offset().height);
        let target = (-f32::from(item.origin.y - target_top)).clamp(-max, 0.);
        let offset = self.lyrics_scroll.offset();
        let gap = target - f32::from(offset.y);
        if gap.abs() < 0.5 {
            return;
        }
        let step = if gap.abs() > f32::from(view.size.height) * 2. { gap } else { gap * 0.18 };
        self.lyrics_scroll.set_offset(point(offset.x, px(f32::from(offset.y) + step)));
        window.request_animation_frame();
    }

    fn render_up_next_panel(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let up_next = self.queue.up_next();
        let mut list = div().id("np-queue").size_full().overflow_y_scroll().flex().flex_col().gap_0p5();
        if up_next.is_empty() {
            list = list.child(div().pt_8().text_color(white().opacity(0.55)).child("Nothing up next."));
        }
        for (n, song) in up_next.iter().enumerate().take(100) {
            list = list.child(
                div()
                    .id(("np-queue-item", n))
                    .p_2()
                    .rounded(px(10.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .hover(|s| s.bg(white().opacity(0.08)))
                    .child(self.art(&song.artwork, 44., 6., t, cx))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_color(white()).font_weight(FontWeight::MEDIUM).child(song.title.clone()))
                            .child(div().truncate().text_size(px(12.)).text_color(white().opacity(0.6)).child(song.artist.clone())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_ahead(n, cx))),
            );
        }
        list.into_any_element()
    }
}
