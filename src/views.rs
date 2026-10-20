use std::{cell::Cell, ops::Range, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    Animation, AnimationExt, ease_out_quint, pulsating_between,
    AnyElement, Bounds, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, MouseButton,
    MouseDownEvent, ObjectFit, Pixels, SharedString, Stateful, Svg, anchored, canvas, deferred,
    div, img, linear_color_stop, uniform_list, linear_gradient, prelude::*, px, relative, rgb, svg, white,
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

// Shell: sidebar, main area, player, menus

impl MusicApp {
    pub(crate) fn render_sidebar(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let top = div()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .child(self.nav_item(t, 0, None, "search", "Search".into(), Route::Search, cx))
            .child(self.nav_item(t, 1, None, "home", "Home".into(), Route::Home, cx))
            .child(self.nav_item(t, 2, None, "new", "New".into(), Route::New, cx))
            .child(self.nav_item(t, 3, None, "radio", "Radio".into(), Route::Radio, cx));

        let library: Vec<AnyElement> = vec![
            self.nav_item(t, 4, None, "clock", "Recently Added".into(), Route::RecentlyAdded, cx).into_any_element(),
            self.nav_item(t, 5, None, "mic", "Artists".into(), Route::Artists, cx).into_any_element(),
            self.nav_item(t, 6, None, "album", "Albums".into(), Route::Albums, cx).into_any_element(),
            self.nav_item(t, 7, None, "note", "Songs".into(), Route::Songs, cx).into_any_element(),
        ];
        let mut playlists: Vec<AnyElement> = vec![
            self.nav_item(t, 8, None, "playlists", "All Playlists".into(), Route::AllPlaylists, cx).into_any_element(),
            self.nav_item(t, 9, None, "star-square", "Favorite Songs".into(), self.favorites_route(), cx)
                .into_any_element(),
        ];
        for (i, playlist) in self.lib_playlists().iter().filter(|p| !self.is_apple_favorites(p)).enumerate() {
            let art = self.playlist_thumb(playlist, t, cx);
            playlists.push(
                self.nav_item(t, 20 + i, Some(art), "playlist", playlist.name.clone().into(), Route::Playlist(playlist.clone()), cx)
                    .into_any_element(),
            );
        }

        let nav = div()
            .id("sidebar-nav")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .map(lock_scroll_axis)
            .px_2()
            .pb_3()
            .child(top)
            .child(self.sidebar_section(t, "Library", library, cx))
            .child(self.sidebar_section(t, "Playlists", playlists, cx));

        // Floats over the page, which runs underneath it to the window edge. gpui can't
        // blur, so the panel stays mostly opaque to keep its text legible over art.
        div().absolute().top_0().left_0().bottom_0().p(px(8.)).pr_0().child(
            div()
                .id("sidebar-panel")
                .occlude()
                .text_color(t.text)
                .w(px(SIDEBAR_W - 8.))
                .h_full()
                .relative()
                // Concentric with the window's corners: Tahoe's ~26pt radius minus the 8pt inset.
                .rounded(px(PANEL_RADIUS))
                // Frosted glass: a lighter, see-through shade of whatever is behind (the
                // page's tint on artwork pages), with a soft highlight along the top.
                .bg(self.fx.borrow_mut().sidebar_bg.update(Hsla { l: (t.window.l + 0.07).min(0.92), a: 0.74, ..t.window }))
                .border_1()
                .border_color(t.text.opacity(0.12))
                .shadow_lg()
                .flex()
                .flex_col()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(140.))
                        .rounded_t(px(PANEL_RADIUS))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(white().opacity(0.06), 0.),
                            linear_color_stop(white().opacity(0.), 1.),
                        )),
                )
                .child(div().h(px(44.)).flex_none())
                .child(nav)
                .child(self.render_account(t, cx)),
        )
    }

    pub(crate) fn render_account(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let initials: String = self
            .user_name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect();
        // Signed in, the row is just photo and name, like the Music app.
        let status: Option<AnyElement> = match self.account {
            Account::SignedIn => None,
            Account::SigningIn => Some(div().child("Waiting for browser…").into_any_element()),
            Account::SignedOut => div()
                .id("sign-in")
                .text_color(t.accent)
                .font_weight(FontWeight::MEDIUM)
                .hover(|s| s.underline())
                .child("Sign In to Apple Music")
                .on_click(cx.listener(|this, _, _, cx| this.sign_in(cx)))
                .into_any_element()
                .into(),
        };
        let avatar: AnyElement = match &self.avatar {
            Some(path) => img(path.clone())
                .size(px(30.))
                .flex_none()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .size(px(30.))
                .flex_none()
                .rounded_full()
                .bg(gradient(0x8e8e93, 0x48484a))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(white())
                .child(initials.to_uppercase())
                .into_any_element(),
        };
        // Red badge, as in the Music app, but only when there's something to act on.
        let needs_attention = self.account == Account::SignedOut;

        let row = div()
            .id("account")
            .relative()
            .mx_2()
            .mb_2()
            .px_2()
            .py_2()
            .rounded(px(10.))
            .flex()
            .flex_none()
            .items_center()
            .gap_2p5()
            .child(
                div().relative().flex_none().child(avatar).when(needs_attention, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(-1.))
                            .left(px(-1.))
                            .size(px(10.))
                            .rounded_full()
                            .bg(t.accent)
                            .border_2()
                            .border_color(t.panel),
                    )
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.user_name.clone()),
                    )
                    .children(status.map(|s| div().truncate().text_size(px(11.5)).text_color(t.text_tertiary).child(s))),
            );

        match self.account {
            Account::SignedIn => row
                .when(self.show_account, |d| d.bg(t.selected))
                .when(!self.show_account, |d| d.hover(|s| s.bg(t.hover)))
                .cursor_pointer()
                .child(capture_bounds(self.account_bounds.clone()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.show_account = !this.show_account;
                        this.menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                ),
            Account::SigningIn => row.child(
                div()
                    .id("cancel-sign-in")
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(t.button)
                    .text_size(px(11.5))
                    .hover(|s| s.bg(t.selected))
                    .child("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_sign_in(cx))),
            ),
            Account::SignedOut => row,
        }
    }

    /// Account card that opens above the sidebar's account row.
    pub(crate) fn render_account_popover(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().account;
        if !presence.visible() || self.account != Account::SignedIn {
            return None;
        }
        let row = self.account_bounds.get();
        let initials: String = self.user_name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        let stats = match self.remote() {
            Some(r) => format!(
                "{} songs · {} albums · {} playlists",
                group_digits(r.songs.len()),
                group_digits(r.albums.len()),
                r.playlists.len()
            ),
            None => "Loading your library…".into(),
        };
        let updated = match self.library_updated {
            Some(at) if at.elapsed().as_secs() < 60 => "Updated just now".to_string(),
            Some(at) => format!("Updated {} min ago", at.elapsed().as_secs() / 60),
            None => "Not updated yet".into(),
        };
        let action = |id: &'static str, icon_name: &'static str, label: &'static str, detail: Option<String>, color: Hsla| {
            div()
                .id(id)
                .mx_1p5()
                .px_2p5()
                .py_2()
                .rounded(px(9.))
                .flex()
                .items_center()
                .gap_3()
                .hover(|s| s.bg(t.selected))
                .child(
                    div()
                        .size(px(28.))
                        .flex_none()
                        .rounded(px(8.))
                        .bg(t.button)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(icon_name, 14., color)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_color(color).child(label))
                        .children(detail.map(|d| div().text_size(px(11.5)).text_color(t.text_tertiary).child(d))),
                )
        };

        let card = div()
            .id("account-popover")
            .occlude()
            .w(px(280.))
            .pb_1p5()
            .rounded(px(16.))
            .bg(t.menu)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .p_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(match &self.avatar {
                        Some(path) => img(path.clone())
                            .size(px(48.))
                            .flex_none()
                            .rounded_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div()
                            .size(px(48.))
                            .flex_none()
                            .rounded_full()
                            .bg(gradient(0x8e8e93, 0x48484a))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(white())
                            .child(initials.to_uppercase())
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.user_name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_size(px(12.))
                                    .text_color(t.text_secondary)
                                    .child(div().size(px(6.)).rounded_full().bg(gpui::rgb(0x34c759)))
                                    .child("Signed in to Apple Music"),
                            ),
                    ),
            )
            .child(
                div()
                    .mx_4()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .rounded(px(10.))
                    .bg(t.button)
                    .text_size(px(12.))
                    .text_color(t.text_secondary)
                    .child(stats),
            )
            .child(div().mx_4().my_1().h(px(1.)).bg(t.separator))
            .child(
                action(
                    "account-photo",
                    "person",
                    if self.avatar.is_some() { "Change Photo…" } else { "Choose Photo…" },
                    Some("Apple doesn't share your Apple Music photo".into()),
                    t.text,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_account = false;
                    this.choose_photo(cx);
                })),
            )
            .when(self.avatar.is_some(), |d| {
                d.child(action("account-photo-remove", "xmark", "Remove Photo", None, t.text).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.show_account = false;
                        this.remove_photo(cx);
                    },
                )))
            })
            .child(
                action("account-refresh", "repeat", "Refresh Library", Some(updated), t.text).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.show_account = false;
                        this.load_remote_library(cx);
                        this.show_notice("Refreshing your library…".into(), cx);
                    },
                )),
            )
            .child(
                action("account-settings", "open", "Apple Music Account", Some("Subscription and settings".into()), t.text)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_account = false;
                        this.open_url("https://music.apple.com/account/settings", cx);
                    })),
            )
            .child(div().mx_4().my_1().h(px(1.)).bg(t.separator))
            .child(
                action("account-sign-out", "person", "Sign Out", None, t.accent).on_click(cx.listener(|this, _, _, cx| {
                    this.show_account = false;
                    this.sign_out(cx);
                })),
            );

        Some(
            deferred(
                anchored()
                    .anchor(gpui::Corner::BottomLeft)
                    .position(gpui::point(row.origin.x, row.origin.y - px(8.)))
                    .snap_to_window_with_margin(px(8.))
                    .child({
                        let p = presence.value();
                        card.opacity(p).mt(px((1. - p) * 8.))
                    }),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// A foldable sidebar group. The rows' height animates when it opens or closes.
    pub(crate) fn sidebar_section(&self, t: &Theme, name: &'static str, rows: Vec<AnyElement>, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.section_open(name);
        let full = rows.len() as f32 * (ROW_H + ROW_GAP);
        let group: SharedString = format!("section-{name}").into();
        div()
            .child(
                div()
                    .id(SharedString::from(format!("section-header-{name}")))
                    .group(group.clone())
                    .mt_4()
                    .mb_1()
                    .px_2p5()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text_tertiary)
                    .hover(|s| s.text_color(t.text_secondary))
                    .child(name)
                    // Disclosure chevron (and, for playlists, a + button) appear on hover.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .when(name == "Playlists" && self.account == Account::SignedIn, |d| {
                                d.child(
                                    div()
                                        .id("new-playlist-button")
                                        .size(px(18.))
                                        .rounded(px(5.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .hover(|s| s.bg(t.hover))
                                        .child(icon("plus", 11., t.text_secondary))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.open_new_playlist(None, window, cx);
                                        })),
                                )
                            })
                            .child(icon(if open > 0.5 { "chevron-down" } else { "chevron-right" }, 11., t.text_tertiary)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_section(name, cx))),
            )
            .child(
                div()
                    .overflow_hidden()
                    .h(px(full * open))
                    .opacity(open)
                    .when(open < 0.01, |d| d.invisible())
                    .child(div().flex().flex_col().gap(px(ROW_GAP)).children(rows)),
            )
    }

    /// Tiny cover for a playlist row: its artwork, or its first song's when it has none.
    pub(crate) fn playlist_thumb(&self, playlist: &Playlist, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let art = if playlist.artwork.is_empty() {
            self.ensure_playlist_tracks(playlist, cx);
            match self.playlist_tracks.get(&playlist.id) {
                Some(Load::Ready(Some(tracks))) => tracks.iter().find(|s| !s.artwork.is_empty()).map(|s| s.artwork.clone()),
                _ => None,
            }
        } else {
            Some(playlist.artwork.clone())
        };
        match art {
            Some(art) => self.art(&art, 20., 5., t, cx),
            None => div()
                .size(px(20.))
                .flex_none()
                .rounded(px(5.))
                .bg(t.placeholder)
                .flex()
                .items_center()
                .justify_center()
                .child(icon("playlist", 11., t.text_tertiary))
                .into_any_element(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn nav_item(
        &self,
        t: &Theme,
        ix: usize,
        leading: Option<AnyElement>,
        icon_name: &'static str,
        label: SharedString,
        route: Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = *self.root_route() == route;
        let drop_target = match &route {
            Route::Playlist(p) if p.editable && self.account == Account::SignedIn => Some(p.clone()),
            _ => None,
        };
        // The highlight fades between items when the selection moves.
        let key: SharedString = format!("nav-selected-{ix}").into();
        self.set_hover(key.clone(), selected);
        let amount = self.hover_value(&key);
        let color = crate::anim::lerp(t.icon, t.accent, amount);
        div()
            .id(("nav", ix))
            .px_2()
            .h(px(ROW_H))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .rounded(px(7.))
            .text_size(px(13.))
            .when(amount > 0., |d| d.bg(t.selected.opacity(amount)))
            .text_color(crate::anim::lerp(t.text, t.accent, amount))
            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
            .when_some(drop_target, |d, playlist| {
                let accent = t.accent;
                d.drag_over::<crate::browse::DraggedSongs>(move |s, _, _, _| s.bg(accent.opacity(0.25)))
                    .on_drop(cx.listener(move |this, dragged: &crate::browse::DraggedSongs, _, cx| {
                        this.add_songs_to_playlist(&dragged.0, &playlist, cx);
                    }))
            })
            .child(match leading {
                Some(art) => art,
                None => div().size(px(20.)).flex_none().flex().items_center().justify_center().child(icon(icon_name, 16., color)).into_any_element(),
            })
            .child(div().truncate().child(label))
            .on_click(cx.listener(move |this, _, window, cx| this.go_root(route.clone(), window, cx)))
    }

    pub(crate) fn render_main(
        &self,
        t: &Theme,
        content_w: f32,
        search_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pill_w = (content_w - 48.).clamp(440., 780.);
        let page_theme = self.page_palette(cx).map(|p| t.tinted(p));
        let page_t = page_theme.as_ref().unwrap_or(t);
        // Background color blends between pages instead of snapping.
        let page_bg = self.fx.borrow_mut().page_bg.update(page_t.window);
        let (queue, notice, key_help) = {
            let fx = self.fx.borrow();
            (fx.queue, fx.notice, fx.key_help)
        };
        let direction = self.nav_direction;

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .child(if self.route_scrolls_itself() {
                div()
                    .id("content")
                    .size_full()
                    .bg(page_bg)
                    // Pages inherit text color from here, so tinted pages read in light mode too.
                    .text_color(page_t.text)
                    .child(page_transition(self.route_generation, direction, true, self.render_route(page_t, content_w, search_focused, cx)))
            } else {
                div()
                    .id("content")
                    .size_full()
                    .bg(page_bg)
                    // Pages inherit text color from here, so tinted pages read in light mode too.
                    .text_color(page_t.text)
                    .overflow_y_scroll()
                    .map(lock_scroll_axis)
                    .track_scroll(&self.scroll)
                    .child(page_transition(self.route_generation, direction, false, self.render_route(page_t, content_w, search_focused, cx)))
                    .child(div().h(px(110.)))
            })
            .children(self.render_compact_bar(page_t, content_w, cx))
            .when(queue.visible(), |d| {
                let p = queue.value();
                d.child(self.render_queue(t, cx).opacity(p).right(px(8. - (1. - p) * 28.)))
            })
            .when(notice.visible(), |d| {
                let p = notice.value();
                let err = self.fx.borrow().last_notice.clone();
                d.child(
                    div()
                        .absolute()
                        .left(px(SIDEBAR_W))
                        .right_0()
                        .bottom(px(80.))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("toast")
                                .px_3()
                                .py_1p5()
                                .rounded_full()
                                .bg(t.menu)
                                .border_1()
                                .border_color(t.panel_border)
                                .shadow_md()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .opacity(p)
                                .mb(px((1. - p) * -10.))
                                .child(err),
                        ),
                )
            })
            .child(
                div()
                    .absolute()
                    .left(px(SIDEBAR_W))
                    .right_0()
                    .bottom(px(14.))
                    .flex()
                    .justify_center()
                    .child(self.render_player(t, pill_w, cx)),
            )
            .when(key_help.visible(), |d| d.child(self.render_key_help(t, cx).opacity(key_help.value())))
    }

    pub(crate) fn render_key_help(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let config = crate::api::data_dir().join("musickit.json");
        let detail = match &self.catalog_note {
            Some(note) => note.clone(),
            None => "Signing in needs a MusicKit key from your Apple Developer account.".into(),
        };
        div()
            .id("key-help-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.35))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.show_key_help = false;
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                div()
                    .id("key-help")
                    .w(px(460.))
                    .p_6()
                    .rounded(px(16.))
                    .bg(t.menu)
                    .border_1()
                    .border_color(t.panel_border)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child("Connect a MusicKit key"))
                    .child(div().text_color(t.text_secondary).child(detail))
                    .child(div().text_color(t.text_secondary).child(
                        "Create one at developer.apple.com → Certificates, IDs & Profiles → Keys (enable MusicKit). \
                         Then save your Team ID, Key ID and the .p8 file's path in:",
                    ))
                    .child(
                        div()
                            .p_2()
                            .rounded(px(6.))
                            .bg(t.field)
                            .font_family("Menlo")
                            .text_size(px(11.))
                            .child(config.display().to_string()),
                    )
                    .child(
                        div()
                            .font_family("Menlo")
                            .text_size(px(11.))
                            .text_color(t.text_secondary)
                            .child("{ \"team_id\": \"…\", \"key_id\": \"…\", \"private_key_path\": \"/path/AuthKey_….p8\" }"),
                    )
                    .child(div().text_color(t.text_secondary).child("Then quit and reopen Encore."))
                    .child(
                        div().pt_2().flex().justify_end().gap_2().child(
                            pill_button(t, "key-help-done", "check", "OK", true).on_click(cx.listener(|this, _, _, cx| {
                                this.show_key_help = false;
                                cx.notify();
                            })),
                        ),
                    ),
            )
    }

    /// Floating player laid out like the macOS Music app (transport · now playing · tools),
    /// tinted by the current cover.
    pub(crate) fn render_player(&self, t: &Theme, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.queue.current().cloned();
        let has_track = current.is_some();
        let playing = matches!(self.state, PlayState::Playing | PlayState::Loading);

        // Pill colors: a deep shade of the cover, or the theme's pill before anything plays.
        let tint = current
            .as_ref()
            .and_then(|s| self.art_color(&s.artwork, cx))
            .map(crate::theme::Palette::from_average);
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
            None => Theme { pill: Hsla { a: 1., ..t.pill }, ..*t },
        };
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
                icon_button(t, "shuffle", "shuffle", 14., toggle_color(self.queue.shuffle)).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.queue.toggle_shuffle();
                        cx.notify();
                    },
                )),
            )
            .child(icon_button(t, "prev", "prev", 19., dim).on_click(cx.listener(|this, _, _, cx| this.previous(cx))))
            .child(
                icon_button(t, "play", if playing { "pause" } else { "play" }, 24., t.icon)
                    .size(px(40.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx))),
            )
            .child(icon_button(t, "next", "next", 19., dim).on_click(cx.listener(|this, _, _, cx| this.next(cx))))
            .child(
                icon_button(
                    t,
                    "repeat",
                    if self.queue.repeat == Repeat::One { "repeat-one" } else { "repeat" },
                    14.,
                    toggle_color(self.queue.repeat != Repeat::Off),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.queue.cycle_repeat();
                    cx.notify();
                })),
            );

        let now_playing: AnyElement = match &current {
            None => div()
                .flex_1()
                .flex()
                .justify_center()
                .child(icon("note", 18., t.text_tertiary))
                .into_any_element(),
            Some(song) => {
                let album = song.album_id.clone();
                let subtitle = if self.state == PlayState::Loading {
                    "Loading…".to_string()
                } else {
                    format!("{} — {}", song.artist, song.album)
                };
                self.progress.read(cx).set_subtitle(subtitle.into());
                div()
                    .id("now-playing")
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.progress.update(cx, |p, cx| {
                            if p.set_hovered(*hovered) {
                                cx.notify();
                            }
                        });
                    }))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .child(
                        div()
                            .id("np-art")
                            .flex_none()
                            .cursor_pointer()
                            .rounded(px(6.))
                            .shadow_sm()
                            .child(self.art(&song.artwork, 38., 6., t, cx))
                            .on_click(cx.listener(|this, _, _, cx| this.open_now_playing(None, cx))),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id("np-title")
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
                            .child(self.progress.clone()),
                    )
                    .with_animation(
                        ElementId::Name(format!("np-{}", song.id).into()),
                        Animation::new(Duration::from_millis(320)).with_easing(ease_out_quint()),
                        |el, p| el.opacity(p),
                    )
                    .into_any_element()
            }
        };

        let more = current.clone();
        let volume_icon = if self.volume < 0.01 { "speaker" } else { "speaker-loud" };
        let tools = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .child(
                icon_button(t, "player-more", "ellipsis", 15., dim).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        if let Some(song) = &more {
                            this.menu = Some(this.song_menu(song, e.position));
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }),
                ),
            )
            .child(
                icon_button(t, "lyrics", "lyrics", 16., if self.show_now_playing { t.accent } else { t.icon }).on_click(
                    cx.listener(|this, _, _, cx| {
                        use crate::nowplaying::NowPlayingTab;
                        if this.show_now_playing {
                            this.show_now_playing = false;
                            cx.notify();
                        } else {
                            this.open_now_playing(Some(NowPlayingTab::Lyrics), cx);
                        }
                    }),
                ),
            )
            .child(
                icon_button(t, "queue", "queue", 16., if self.show_queue { t.accent } else { t.icon }).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.show_queue = !this.show_queue;
                        cx.notify();
                    }),
                ),
            )
            .child(
                div()
                    .relative()
                    .child(capture_bounds(self.volume_button_bounds.clone()))
                    .child(
                        icon_button(t, "volume", volume_icon, 17., if self.show_volume { t.accent } else { t.icon })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_volume = !this.show_volume;
                                this.menu = None;
                                cx.notify();
                            })),
                    ),
            );

        div()
            .id("player")
            .occlude()
            .w(px(width))
            .h(px(58.))
            .pl_2()
            .pr_2p5()
            .rounded_full()
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

    /// Small volume slider that opens above the player's speaker button.
    pub(crate) fn render_volume_popover(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let presence = self.fx.borrow().volume;
        if !presence.visible() {
            return None;
        }
        let button = self.volume_button_bounds.get();
        const W: f32 = 46.;
        let card = div()
            .id("volume-popover")
            .occlude()
            .w(px(W))
            .py_3()
            .rounded(px(23.))
            .bg(t.menu)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .flex()
            .flex_col()
            .items_center()
            .gap_2p5()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(icon("speaker-loud", 15., t.text_secondary))
            .child(self.vertical_slider(self.volume, px(130.), t, cx))
            .child(icon("speaker", 12., t.text_secondary));
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Corner::BottomLeft)
                    .position(gpui::point(
                        button.origin.x + button.size.width / 2. - px(W / 2.),
                        button.top() - px(14.),
                    ))
                    .snap_to_window_with_margin(px(8.))
                    .child({
                        let p = presence.value();
                        card.opacity(p).mt(px((1. - p) * 6.))
                    }),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    pub(crate) fn render_menu(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let mut list = div()
            .id("context-menu")
            .occlude()
            .w(px(230.))
            .py_1()
            .rounded(px(10.))
            .bg(t.menu)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .flex()
            .flex_col();
        for (i, entry) in menu.entries.iter().enumerate() {
            list = match entry {
                None => list.child(div().mx_3().my_1().h(px(1.)).bg(t.separator)),
                Some(entry) => {
                    let action = entry.action.clone();
                    list.child(
                        div()
                            .id(("menu-item", i))
                            .mx_1()
                            .px_2()
                            .h(px(28.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .hover(|s| s.bg(t.selected))
                            .child(icon(entry.icon, 14., t.icon))
                            .child(entry.label.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.menu = None;
                                action(this, window, cx);
                                cx.notify();
                            })),
                    )
                }
            };
        }
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(list.with_animation("menu-in", pop_in(), |el, p| el.opacity(p).mt(px((1. - p) * -6.)))),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    pub(crate) fn render_queue(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let accent = t.accent;
        let mut panel = div()
            .id("queue")
            .drag_over::<crate::browse::DraggedSongs>(move |s, _, _, _| s.border_color(accent).border_2())
            .on_drop(cx.listener(|this, dragged: &crate::browse::DraggedSongs, _, cx| {
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
            .bg(t.panel)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .overflow_y_scroll()
            .map(lock_scroll_axis)
            .p_3()
            .flex()
            .flex_col()
            .gap_0p5();

        let heading = |s: &'static str| {
            div()
                .px_1()
                .pt_2()
                .pb_1()
                .text_size(px(15.))
                .font_weight(FontWeight::BOLD)
                .child(s)
        };

        let Some(current) = self.queue.current().cloned() else {
            return panel.child(heading("Playing Next")).child(
                div()
                    .pt_8()
                    .text_color(t.text_secondary)
                    .text_center()
                    .child("Nothing is playing."),
            );
        };

        let repeat_icon = if self.queue.repeat == Repeat::One { "repeat-one" } else { "repeat" };
        let mut crossfade = div().flex().p(px(2.)).gap(px(2.)).rounded(px(9.)).bg(t.button);
        for secs in crate::app::CROSSFADE_OPTIONS {
            let on = self.crossfade_secs == secs;
            crossfade = crossfade.child(
                div()
                    .id(("crossfade", secs as usize))
                    .px_2()
                    .h(px(22.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .when(on, |d| d.bg(t.menu).shadow_sm().text_color(t.text))
                    .when(!on, |d| d.text_color(t.text_secondary).hover(|s| s.text_color(t.text)))
                    .child(if secs == 0 { "Off".to_string() } else { format!("{secs}s") })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_crossfade(secs, cx))),
            );
        }
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
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(heading("Playing Next"))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                toggle_chip(t, "queue-shuffle", "shuffle", self.queue.shuffle).on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.queue.toggle_shuffle();
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                toggle_chip(t, "queue-repeat", repeat_icon, self.queue.repeat != Repeat::Off).on_click(
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
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(t.text_secondary)
                            .child(song.artist.clone()),
                    ),
            )
    }

    /// Vertical volume slider: fills from the bottom; drag or click anywhere on it.
    pub(crate) fn vertical_slider(&self, value: f32, height: Pixels, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let knob = px(14.);
        div()
            .id("volume")
            .relative()
            .w(px(22.))
            .h(height)
            .flex()
            .justify_center()
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
                    .w(px(5.))
                    .h_full()
                    .rounded_full()
                    .bg(t.track_bg)
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(relative(value))
                            .rounded_full()
                            .bg(t.track_fill),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(4.))
                    .top((height - knob) * (1. - value))
                    .size(knob)
                    .rounded_full()
                    .bg(rgb(0xf5f5f5))
                    .shadow_sm(),
            )
    }

}

// Pages

impl MusicApp {
    pub(crate) fn render_route(&self, t: &Theme, content_w: f32, search_focused: bool, cx: &mut Context<Self>) -> AnyElement {
        match self.route.clone() {
            Route::New => self.with_browse(t, cx, |this, cx| this.render_new(t, content_w, cx)),
            Route::Home => self.with_browse(t, cx, |this, cx| this.render_home(t, content_w, cx)),
            Route::Radio => self.render_radio(t, content_w, cx).into_any_element(),
            Route::Search => self.render_search(t, content_w, search_focused, cx).into_any_element(),
            Route::Album(id) => self.render_album(t, &id, content_w, cx),
            Route::Artist(id) => self.render_artist(t, &id, content_w, cx),
            Route::Playlist(p) => self.render_playlist(t, &p, content_w, cx),
            Route::RecentlyAdded | Route::Albums | Route::Artists | Route::Songs | Route::AllPlaylists | Route::Favorites
                if self.remote_pending().is_some() =>
            {
                self.remote_pending().unwrap()(self, t, cx)
            }
            Route::RecentlyAdded => {
                self.render_library_albums(t, "Recently Added", Arc::new(self.lib_recently_added()), content_w, cx)
            }
            Route::Albums => self.render_library_albums(t, "Albums", self.shaped_albums(), content_w, cx),
            Route::Artists => self.render_library_artists(t, content_w, cx),
            Route::Songs => self.render_song_list_page(
                t,
                "Songs",
                self.lib_songs(),
                "Songs you add from Apple Music will show up here.",
                cx,
            ),
            Route::Favorites => self.render_song_list_page(
                t,
                "Favorite Songs",
                self.lib_favorites(),
                "Favorite songs from the ⋯ menu to see them here.",
                cx,
            ),
            Route::AllPlaylists => self.render_all_playlists(t, content_w, cx).into_any_element(),
        }
    }

    /// While signed in, library pages wait on (or report errors from) the Apple Music library.
    #[allow(clippy::type_complexity)]
    pub(crate) fn remote_pending(&self) -> Option<fn(&Self, &Theme, &mut Context<Self>) -> AnyElement> {
        if self.account != Account::SignedIn {
            return None;
        }
        match &self.remote {
            Load::Ready(_) => None,
            Load::Loading => Some(|_, t, _| loading(t).into_any_element()),
            Load::Failed(_) => Some(|this, t, cx| {
                let Load::Failed(err) = &this.remote else { unreachable!() };
                this.error_state(t, err, cx).into_any_element()
            }),
        }
    }

    /// Pages built on a virtualized list, which brings its own scrolling.
    pub(crate) fn route_scrolls_itself(&self) -> bool {
        self.remote_pending().is_none()
            && match self.route {
                Route::Songs => !self.lib_songs().is_empty(),
                Route::Favorites => !self.lib_favorites().is_empty(),
                Route::Artists => !self.library_artist_rows().is_empty(),
                Route::Albums => self.lib_albums().len() > VIRTUAL_GRID_MIN,
                Route::RecentlyAdded => self.lib_recently_added().len() > VIRTUAL_GRID_MIN,
                _ => false,
            }
    }

    pub(crate) fn with_browse(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
        f: impl FnOnce(&Self, &mut Context<Self>) -> Div,
    ) -> AnyElement {
        match &self.browse {
            Load::Ready(_) => f(self, cx).into_any_element(),
            Load::Loading => skeleton(t).into_any_element(),
            Load::Failed(err) => self.error_state(t, err, cx).into_any_element(),
        }
    }

    pub(crate) fn render_new(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> Div {
        let browse = self.browse.ready().unwrap();
        let hero_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();
        let releases = browse.new_releases();

        let mut heroes = scroll_row("heroes");
        for (i, album) in releases.iter().take(6).enumerate() {
            heroes = heroes.child(self.hero_card(t, i, album, hero_w, cx));
        }

        let mut new_albums = scroll_row("new-releases");
        for (i, album) in releases.iter().skip(6).take(20).enumerate() {
            new_albums = new_albums.child(self.album_tile(t, "new-album", i, album, 180., cx));
        }

        let mut playlists = scroll_row("new-playlists");
        for (i, playlist) in browse.top_playlists.iter().take(20).enumerate() {
            playlists = playlists.child(self.playlist_tile(t, "new-playlist", i, playlist, 180., cx));
        }

        div()
            .child(page_title("New"))
            .child(heroes)
            .child(section_header(t, "Best New Songs"))
            .child(self.song_columns(t, "best-new", browse.best_new_songs().into_iter().take(24).collect(), hero_w, false, cx))
            .child(section_header(t, "New Releases"))
            .child(new_albums)
            .child(section_header(t, "Top Playlists"))
            .child(playlists)
    }

    /// A row of mixed cards (songs, albums, artists, playlists). Songs play from `songs`.
    pub(crate) fn mixed_row(
        &self,
        t: &Theme,
        id: &'static str,
        items: &[crate::model::TopResult],
        songs: &Arc<Vec<Song>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use crate::model::TopResult;
        let mut row = scroll_row(id).pt_1();
        for (i, item) in items.iter().enumerate() {
            let (art, title, subtitle, kind): (AnyElement, String, String, &str) = match item {
                TopResult::Song(s) => (self.art(&s.artwork, 160., 8., t, cx), s.title.clone(), s.artist.clone(), "Song"),
                TopResult::Album(a) => (self.art(&a.artwork, 160., 8., t, cx), a.title.clone(), a.artist.clone(), "Album"),
                TopResult::Artist(a) => (self.avatar(&a.artwork, &a.name, 160., t, cx), a.name.clone(), String::new(), "Artist"),
                TopResult::Playlist(p) => (self.playlist_art(p, 160., 8., t, cx), p.name.clone(), p.curator.clone(), "Playlist"),
            };
            let item = item.clone();
            let songs = songs.clone();
            row = row.child(
                div()
                    .id(ElementId::from((SharedString::from(id), i)))
                    .w(px(160.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .child(art)
                    .child(div().pt_2().truncate().child(title))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(t.text_secondary)
                            .child(if subtitle.is_empty() { kind.to_string() } else { format!("{kind} · {subtitle}") }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.route == Route::Search {
                            this.remember_search();
                        }
                        match &item {
                            TopResult::Song(s) => {
                                let ix = songs.iter().position(|x| x.id == s.id).unwrap_or(0);
                                let list = if songs.is_empty() { vec![s.clone()] } else { songs.as_ref().clone() };
                                this.play(list, ix, false, cx);
                            }
                            TopResult::Album(a) => this.push(Route::Album(a.id.clone()), cx),
                            TopResult::Artist(a) => this.push(Route::Artist(a.id.clone()), cx),
                            TopResult::Playlist(p) => this.push(Route::Playlist(p.clone()), cx),
                        }
                    })),
            );
        }
        row
    }

    pub(crate) fn render_home(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> Div {
        let browse = self.browse.ready().unwrap();
        let col_w_personal = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();
        // Signed in: Apple's personal shelves first, the charts below.
        let mut personal = div();
        if let Some(Load::Ready(Some(home))) = &self.personal {
            if !home.recent_tracks.is_empty() {
                personal = personal
                    .child(section_header(t, "Recently Played"))
                    .child(self.song_columns(t, "home-recent", home.recent_tracks.clone(), col_w_personal, false, cx));
            }
            for (n, (title, items)) in home.shelves.iter().enumerate() {
                if title == "Recently Played" && !home.recent_tracks.is_empty() {
                    continue;
                }
                let ids = ["shelf-0", "shelf-1", "shelf-2", "shelf-3", "shelf-4", "shelf-5", "shelf-6", "shelf-7", "shelf-8", "shelf-9"];
                personal = personal
                    .child(section_header_owned(t, title.clone()))
                    .child(self.mixed_row(t, ids[n.min(9)], items, &Arc::new(Vec::new()), cx));
            }
            if !home.heavy_rotation.is_empty() {
                personal = personal
                    .child(section_header(t, "Heavy Rotation"))
                    .child(self.mixed_row(t, "heavy-rotation", &home.heavy_rotation, &Arc::new(Vec::new()), cx));
            }
        } else if matches!(self.personal, Some(Load::Loading)) || (self.personal.is_none() && self.account == Account::SignedIn) {
            // Same footprint as the real first shelves, so nothing jumps when they arrive.
            personal = personal.child(skeleton_home_shelves(t));
        }
        let (cols, card_w) = grid_layout(content_w, 200., 5);
        let col_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();

        let mut picks = div().px(px(PAD)).flex().gap(px(GAP));
        for (i, playlist) in browse.top_playlists.iter().take(cols).enumerate() {
            picks = picks.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(div().text_size(px(12.)).text_color(t.text_secondary).child("Top Playlist"))
                    .child(self.playlist_tile(t, "pick", i, playlist, card_w, cx)),
            );
        }

        let mut albums = scroll_row("top-albums");
        for (i, album) in browse.top_albums.iter().take(24).enumerate() {
            albums = albums.child(self.album_tile(t, "top-album", i, album, 180., cx));
        }

        let mut page = div()
            .child(page_title("Home"))
            .child(personal)
            .child(section_header(t, if self.personal.is_some() { "Top Picks" } else { "Top Picks for You" }))
            .child(picks)
            .child(section_header(t, "Top Songs"))
            .child(self.song_columns(t, "top-songs", browse.top_songs.iter().take(40).cloned().collect(), col_w, true, cx))
            .child(section_header(t, "Top Albums"))
            .child(albums);

        let recently_added = self.lib_recently_added();
        if !recently_added.is_empty() {
            let mut recent = scroll_row("home-library");
            for (i, album) in recently_added.iter().take(20).enumerate() {
                recent = recent.child(self.album_tile(t, "home-library-album", i, album, 180., cx));
            }
            page = page.child(section_header(t, "Recently Added")).child(recent);
        }
        page
    }

    pub(crate) fn render_radio(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let stations: &[Station] = match &self.stations {
            Some(Load::Ready(stations)) => stations,
            _ => &[],
        };
        let featured = stations.first();
        let hero_url = featured.map_or("https://music.apple.com/us/radio".to_string(), |s| s.url.clone());

        let hero = div()
            .h(px(240.))
            .rounded(px(12.))
            .bg(gradient(0xfa2d48, 0x7a0b1d))
            .p_6()
            .flex()
            .items_center()
            .gap_6()
            .when_some(featured, |d, s| d.child(self.art(&s.artwork, 192., 10., t, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div().flex().child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(white())
                                .text_color(t.accent)
                                .text_size(px(11.))
                                .font_weight(FontWeight::BOLD)
                                .child("LIVE"),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(32.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(white())
                            .truncate()
                            .child(featured.map_or("Apple Music Radio".to_string(), |s| s.name.clone())),
                    )
                    .child(
                        div()
                            .max_w(px(560.))
                            .text_size(px(14.))
                            .text_color(white().opacity(0.85))
                            .line_clamp(2)
                            .child(match featured {
                                Some(s) if !s.description.is_empty() => s.description.clone(),
                                _ => "Live radio from Apple Music.".to_string(),
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(white().opacity(0.7))
                            .child("Apple only streams live radio inside its own apps, so listening opens Apple Music."),
                    )
                    .child(
                        div().pt_1().flex().child(
                            button(t, "open-radio", "open", "Listen in Apple Music", white().into(), t.accent).on_click(
                                cx.listener(move |this, _, _, cx| this.open_url(&hero_url, cx)),
                            ),
                        ),
                    ),
            );

        let mut page = div().child(page_title("Radio")).child(div().px(px(PAD)).child(hero));

        if stations.len() > 1 {
            let mut row = scroll_row("live-stations");
            for (i, station) in stations.iter().enumerate().skip(1) {
                let url = station.url.clone();
                row = row.child(
                    div()
                        .id(("station", i))
                        .w(px(200.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.open_url(&url, cx)))
                        .child(self.art(&station.artwork, 200., 8., t, cx))
                        .child(div().pt_2().truncate().child(station.name.clone()))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .line_clamp(2)
                                .child(station.description.clone()),
                        ),
                );
            }
            page = page.child(section_header(t, "Live Stations")).child(row);
        }

        if let Some(browse) = self.browse.ready() {
            let (_, tile) = grid_layout(content_w, 170., usize::MAX);
            let mut grid = grid_container();
            for (i, p) in browse.top_playlists.iter().enumerate() {
                grid = grid.child(self.playlist_tile(t, "radio-playlist", i, p, tile, cx));
            }
            page = page.child(section_header(t, "Playlists to Tune Into")).child(grid);
        }
        page
    }

    pub(crate) fn render_search(&self, t: &Theme, content_w: f32, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = focused && !self.palette_open;
        let text = self.search_text(t, focused, "Artists, Songs, Albums, and More", 17.);

        let field = div()
            .id("search-field")
            .key_context("SearchField")
            .when(!self.palette_open, |d| d.track_focus(&self.search_focus))
            .mx(px(PAD))
            .h(px(38.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(10.))
            .bg(t.field)
            .border_2()
            .border_color(if focused { t.accent.opacity(0.55) } else { gpui::transparent_black() })
            .text_size(px(14.))
            .child(icon("search", 15., t.text_secondary))
            .child(div().flex_1().min_w_0().child(text))
            .when(!self.search.is_empty(), |d| {
                d.child(
                    div()
                        .id("search-clear")
                        .size(px(18.))
                        .rounded_full()
                        .bg(t.text_tertiary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("xmark", 10., t.window))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search.clear();
                            this.search_results = None;
                            window.focus(&this.search_focus);
                            cx.notify();
                        })),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.search_focus);
                    this.menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            );

        let mut page = div().child(page_title("Search")).child(field);
        let col_w = ((content_w - 2. * PAD - GAP) / 2.).clamp(320., 620.).floor();

        // Recent searches while the field is empty.
        if self.search.is_empty() && !self.recent_searches.is_empty() {
            let mut chips = div().px(px(PAD)).pt_4().flex().flex_wrap().gap_2();
            for (i, term) in self.recent_searches.iter().enumerate() {
                let term = term.clone();
                chips = chips.child(
                    div()
                        .id(("recent-search", i))
                        .px_3()
                        .h(px(30.))
                        .rounded_full()
                        .bg(t.button)
                        .hover(|s| s.bg(t.selected))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .cursor_pointer()
                        .child(icon("clock", 12., t.text_secondary))
                        .child(term.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.search.set(term.clone());
                            this.schedule_search(cx);
                            cx.notify();
                        })),
                );
            }
            page = page.child(chips);
        }

        // Apple's own relevance ranking first: the artist, their latest release, the hits.
        if let Some((term, Load::Ready(r))) = &self.search_results
            && term.trim() == self.search.text().trim()
            && !r.top.is_empty()
        {
            use crate::model::TopResult;
            let top_songs: Arc<Vec<Song>> =
                Arc::new(r.top.iter().filter_map(|t| if let TopResult::Song(s) = t { Some(s.clone()) } else { None }).collect());
            let row = self.mixed_row(t, "top-results", &r.top, &top_songs, cx);
            page = page.child(section_header(t, "Top Results")).child(row);
        }

        // Instant matches from the library, before Apple's catalog answers.
        let local = self.local_matches();
        if !local.is_empty() {
            page = page.child(section_header(t, "In Your Library"));
            if !local.songs.is_empty() {
                page = page.child(self.song_columns(t, "local-songs", local.songs.clone(), col_w, false, cx));
            }
            if !local.albums.is_empty() || !local.playlists.is_empty() {
                let mut row = scroll_row("local-albums").pt_2();
                for (i, album) in local.albums.iter().enumerate() {
                    row = row.child(self.album_tile(t, "local-album", i, album, 150., cx));
                }
                for (i, playlist) in local.playlists.iter().enumerate() {
                    row = row.child(self.playlist_tile(t, "local-playlist", i, playlist, 150., cx));
                }
                page = page.child(row);
            }
        }

        let Some((term, results)) = &self.search_results else {
            if let Load::Failed(err) = &self.browse {
                return page.child(self.error_state(t, err, cx));
            }
            return match self.browse.ready() {
                Some(browse) => page.child(section_header(t, "Top Songs Right Now")).child(self.song_columns(
                    t,
                    "search-top",
                    browse.top_songs.iter().take(20).cloned().collect(),
                    col_w,
                    true,
                    cx,
                )),
                None => page,
            };
        };
        let results = match results {
            Load::Loading => return page.child(div().pt_4().child(skeleton_rows(t, 8))),
            Load::Failed(err) => return page.child(self.error_state(t, err, cx)),
            Load::Ready(r) => r,
        };
        if results.songs.is_empty() && results.albums.is_empty() && results.artists.is_empty() {
            return page.child(
                div()
                    .pt(px(80.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child("No Results"))
                    .child(
                        div()
                            .text_color(t.text_secondary)
                            .child(format!("Nothing matched “{term}”. Try a new search.")),
                    ),
            );
        }

        // Results fade in as a group whenever a new search lands.
        let mut found = div();
        if !results.artists.is_empty() {
            let mut row = scroll_row("artist-results");
            for (i, artist) in results.artists.iter().enumerate() {
                let id = artist.id.clone();
                row = row.child(
                    div()
                        .id(("artist-result", i))
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
            found = found.child(section_header(t, "Artists")).child(row);
        }
        if !results.songs.is_empty() {
            found = found
                .child(section_header(t, "Songs"))
                .child(self.song_columns(t, "song-results", results.songs.clone(), col_w, false, cx));
        }
        if !results.albums.is_empty() {
            let mut row = scroll_row("album-results");
            for (i, album) in results.albums.iter().enumerate() {
                row = row.child(self.album_tile(t, "album-result", i, album, 180., cx));
            }
            found = found.child(section_header(t, "Albums")).child(row);
        }
        page.child(found.with_animation(
            ElementId::Name(format!("results-{term}").into()),
            Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
            |el, p| el.opacity(p).mt(px((1. - p) * 8.)),
        ))
    }

    pub(crate) fn album_track_row(&self, t: &Theme, list: &Arc<Vec<Song>>, ix: usize, song: &Song, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_drag(self.drag_payload(&list[ix], &list), |d, _, _, cx| crate::browse::drag_preview(d, cx))
            .group("track")
            .h(px(44.))
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(7.))
            .when(ix % 2 == 0 && !selected, |d| d.bg(t.row_alt))
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

    pub(crate) fn render_library_albums(&self, t: &Theme, title: &'static str, albums: Arc<Vec<Album>>, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        if albums.is_empty() {
            return div()
                .child(page_title(title))
                .child(empty_state(t, "Your library is empty", "Open an album and press + to add it."))
                .into_any_element();
        }
        let (cols, tile) = grid_layout(content_w, 170., usize::MAX);
        let toolbar = (title == "Albums").then(|| {
            use crate::browse::SortKey;
            self.list_toolbar(t, "albums", &[SortKey::Title, SortKey::Artist, SortKey::Year, SortKey::Added], cx)
        });
        if albums.len() <= VIRTUAL_GRID_MIN {
            let mut grid = grid_container();
            for (i, album) in albums.iter().enumerate() {
                grid = grid.child(self.album_tile(t, "library-album", i, album, tile, cx));
            }
            return div().child(page_title(title)).children(toolbar).child(grid).into_any_element();
        }
        // Large libraries: only the rows on screen are built each frame.
        let rows = albums.len().div_ceil(cols);
        let row_h = tile + 54. + 26.;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_none().child(page_title(title)).children(toolbar))
            .child(
                uniform_list(
                    "album-grid",
                    rows + 1, // trailing spacer keeps the last row clear of the player
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        let t = Theme::for_appearance(window.appearance());
                        range
                            .map(|row| {
                                let mut line = div().w_full().h(px(row_h)).px(px(PAD)).flex().gap(px(GAP));
                                for i in row * cols..((row + 1) * cols).min(albums.len()) {
                                    line = line.child(this.album_tile(&t, "library-album", i, &albums[i], tile, cx));
                                }
                                line.into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .map(lock_scroll_axis),
            )
            .into_any_element()
    }

    /// (name, catalog id, artwork) for the Artists page: Apple's library artist list when
    /// signed in, otherwise derived from the local library's songs.
    pub(crate) fn library_artist_rows_uncached(&self) -> Vec<(String, Option<String>, Artwork)> {
        match self.remote() {
            Some(r) if !r.artists.is_empty() => r
                .artists
                .iter()
                .map(|a| {
                    let id = (!a.id.starts_with("r.")).then(|| a.id.clone());
                    (a.name.clone(), id, a.artwork.clone())
                })
                .collect(),
            _ => crate::library::artists(&self.lib_songs(), self.lib_albums())
                .into_iter()
                .map(|(name, id, album)| (name, id, album.map(|a| a.artwork).unwrap_or_default()))
                .collect(),
        }
    }

    /// Two panes like the Music app: artist list on the left, the selected artist's
    /// albums and songs from the library on the right.
    pub(crate) fn render_library_artists(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        const LIST_W: f32 = 260.;
        let artists = self.library_artist_rows();
        if artists.is_empty() {
            return div()
                .child(page_title("Artists"))
                .child(empty_state(t, "No artists yet", "Artists of songs you add appear here."))
                .into_any_element();
        }
        let selected_ix = self
            .selected_library_artist
            .as_ref()
            .and_then(|name| artists.iter().position(|(n, _, _)| n == name))
            .unwrap_or(0);
        let (name, id, artwork) = artists[selected_ix].clone();

        let rows = artists.clone();
        let list = uniform_list(
            "library-artists",
            artists.len() + 2,
            cx.processor(move |this, range: Range<usize>, window, cx| {
                let t = Theme::for_appearance(window.appearance());
                range
                    .map(|ix| {
                        let Some((name, _, artwork)) = rows.get(ix) else {
                            return div().h(px(48.)).into_any_element();
                        };
                        let selected = ix == selected_ix;
                        let select = name.clone();
                        div()
                            .id(("library-artist", ix))
                            .w_full()
                            .h(px(48.))
                            .px_2()
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap_3()
                            .when(selected, |d| d.bg(t.selected))
                            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
                            .child(this.avatar(artwork, name, 34., &t, cx))
                            .child(div().truncate().child(name.clone()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_library_artist = Some(select.clone());
                                this.artist_pane_scroll.set_offset(gpui::point(px(0.), px(0.)));
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .w(px(LIST_W))
        .h_full()
        .px_2()
        .map(lock_scroll_axis);

        let lower = name.to_lowercase();
        let by_artist = |artist: &str| {
            let a = artist.to_lowercase();
            a == lower || a.split([',', '&']).any(|part| part.trim() == lower)
        };
        let albums: Vec<Album> = self.lib_albums().iter().filter(|a| by_artist(&a.artist)).cloned().collect();
        let songs: Vec<Song> = self.lib_songs().iter().filter(|s| by_artist(&s.artist)).cloned().collect();
        let detail_w = content_w - LIST_W;
        let (_, tile) = grid_layout(detail_w, 150., usize::MAX);
        let mut grid = grid_container();
        for (i, album) in albums.iter().enumerate() {
            grid = grid.child(self.album_tile(t, "artist-library-album", i, album, tile, cx));
        }
        let play_songs = songs.clone();
        let (open_name, open_id) = (name.clone(), id.clone());

        let detail = div()
            .id("library-artist-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .map(lock_scroll_axis)
            .track_scroll(&self.artist_pane_scroll)
            .child(self.library_artist_hero(t, &name, &artwork, detail_w, Arc::new(play_songs), (open_name, open_id), cx))
            .when(!albums.is_empty(), |d| d.child(section_header(t, "Albums")).child(grid))
            .when(!songs.is_empty(), |d| {
                d.child(section_header(t, "Songs")).child(self.song_table(t, "artist-library-songs", songs, cx))
            })
            .child(div().h(px(110.)));

        div()
            .size_full()
            .flex()
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(t.separator)
                    .child(div().px(px(PAD)).pt(px(46.)).pb_3().text_size(px(28.)).font_weight(FontWeight::BOLD).child("Artists"))
                    .child(list.flex_1()),
            )
            .child(detail)
            .into_any_element()
    }

    pub(crate) fn render_all_playlists(&self, t: &Theme, content_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let (_, tile) = grid_layout(content_w, 170., usize::MAX);
        let mut grid = grid_container().child(
            div()
                .id("favorites-tile")
                .w(px(tile))
                .flex_none()
                .flex()
                .flex_col()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    let route = this.favorites_route();
                    this.go_root(route, window, cx)
                }))
                .child(
                    div()
                        .size(px(tile))
                        .rounded(px(8.))
                        .bg(gradient(0xfa2d48, 0xa1122c))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("star-square", tile * 0.35, white())),
                )
                .child(div().pt_2().child("Favorite Songs"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text_secondary)
                        .child(format!("{} songs", self.lib_favorites().len())),
                ),
        );
        let playlists: Vec<Playlist> = self.lib_playlists().into_iter().filter(|p| !self.is_apple_favorites(p)).collect();
        for (i, p) in playlists.iter().enumerate() {
            grid = grid.child(self.playlist_tile(t, "library-playlist", i, p, tile, cx));
        }
        div().child(page_title("All Playlists")).child(grid).when(playlists.is_empty(), |d| {
            d.child(
                div()
                    .px(px(PAD))
                    .pt_6()
                    .text_color(t.text_secondary)
                    .child("Open any playlist and press + to add it here."),
            )
        })
    }

    pub(crate) fn render_song_list_page(
        &self,
        t: &Theme,
        title: &'static str,
        songs: Arc<Vec<Song>>,
        empty_hint: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if songs.is_empty() {
            return div()
                .child(page_title(title))
                .child(empty_state(t, "No songs yet", empty_hint))
                .into_any_element();
        }
        self.render_virtual_song_page(t, title, songs, cx)
    }
}

// Shared pieces

impl MusicApp {
    pub(crate) fn error_state(&self, t: &Theme, err: &str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pt(px(120.))
            .px(px(PAD))
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::BOLD)
                    .child("Couldn't connect to Apple Music"),
            )
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
    pub(crate) fn art(&self, artwork: &Artwork, size: f32, radius: f32, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        self.art_box(artwork, size, size, radius, t, cx)
    }

    pub(crate) fn art_box(&self, artwork: &Artwork, w: f32, h: f32, radius: f32, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let want = (w.max(h) * 2.) as u32;
        let px_size = [120, 240, 400, 600, 1000, 1600, 2400].into_iter().find(|&s| s >= want).unwrap_or(2400);
        // Non-square frames get a server-side crop: "cover" scaling would draw past the
        // frame, and the image's rounded corners would land outside the visible area.
        let placeholder = || {
            div()
                .w(px(w))
                .h(px(h))
                .flex_none()
                .rounded(px(radius))
                .bg(t.placeholder)
        };
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
                let frame = div()
                    .w(px(w))
                    .h(px(h))
                    .flex_none()
                    .rounded(px(radius))
                    .overflow_hidden()
                    .bg(t.placeholder)
                    .child(
                        img(path)
                            .min_w(px(w))
                            .max_w(px(w))
                            .min_h(px(h))
                            .max_h(px(h))
                            .rounded(px(radius))
                            .object_fit(ObjectFit::Cover),
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

    pub(crate) fn avatar(&self, artwork: &Artwork, name: &str, size: f32, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if !artwork.is_empty() {
            return self.art(artwork, size, size / 2., t, cx);
        }
        let initials: String = name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect();
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

    pub(crate) fn hero_card(&self, t: &Theme, i: usize, album: &Album, w: f32, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.art_box(&album.artwork, w, h, 12., t, cx))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(relative(0.65))
                    .rounded_b(px(12.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(gpui::black().opacity(0.), 0.),
                        linear_color_stop(gpui::black().opacity(0.88), 1.),
                    )),
            )
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
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .truncate()
                            .child(album.title.clone()),
                    )
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
    pub(crate) fn song_columns(&self, t: &Theme, id: &'static str, songs: Vec<Song>, col_w: f32, ranked: bool, cx: &mut Context<Self>) -> impl IntoElement {
        self.song_columns_impl(t, id, songs, col_w, ranked, false, cx)
    }

    /// Song columns whose second line is "ALBUM · YEAR" (for artist pages, where the
    /// artist is already known).
    pub(crate) fn song_columns_by_album(&self, t: &Theme, id: &'static str, songs: Vec<Song>, col_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        self.song_columns_impl(t, id, songs, col_w, false, true, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn song_columns_impl(&self, t: &Theme, id: &'static str, songs: Vec<Song>, col_w: f32, ranked: bool, by_album: bool, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(crate) fn song_cell(&self, t: &Theme, prefix: &'static str, list: &Arc<Vec<Song>>, ix: usize, ranked: bool, by_album: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let song = &list[ix];
        let subtitle = if by_album {
            let year = song.release_date.get(..4).unwrap_or("");
            [song.album.to_uppercase(), year.to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
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
            .on_drag(self.drag_payload(&list[ix], &list), |d, _, _, cx| crate::browse::drag_preview(d, cx))
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
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(t.text_secondary)
                            .child(subtitle),
                    ),
            )
            .child(self.more_button(t, ElementId::from((SharedString::from(format!("{prefix}-more")), ix)), song, "", cx))
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if e.click_count() >= 2 {
                    this.play(dbl_list.as_ref().clone(), ix, false, cx);
                }
            }))
    }

    /// Library-style table: Title / Artist / Album / Time.
    pub(crate) fn song_table(&self, t: &Theme, id: &'static str, songs: Vec<Song>, cx: &mut Context<Self>) -> impl IntoElement {
        let list = Arc::new(songs);
        let mut rows = div().flex().flex_col();
        for ix in 0..list.len() {
            rows = rows.child(self.table_row(t, id, &list, ix, cx));
        }
        div().px(px(PAD - 8.)).pt_2().child(table_header(t)).child(rows)
    }

    /// Library-sized song lists: only the visible rows are built each frame.
    pub(crate) fn render_virtual_song_page(&self, t: &Theme, title: &'static str, list: Arc<Vec<Song>>, cx: &mut Context<Self>) -> AnyElement {
        const BOTTOM_SPACERS: usize = 2; // keep the last rows clear of the floating player
        use crate::browse::SortKey;
        let key: &'static str = if title == "Songs" { "songs" } else { "favorites" };
        let list = self.shaped_songs(key, list);
        let rows = list.clone();
        let toolbar = self.list_toolbar(
            t,
            key,
            &[SortKey::Added, SortKey::Title, SortKey::Artist, SortKey::Album, SortKey::Duration],
            cx,
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .child(page_title(title))
                    .child(div().px(px(PAD)).pb_4().child(self.play_shuffle(t, list.clone(), cx)))
                    .child(toolbar)
                    .child(div().px(px(PAD - 8.)).child(table_header(t))),
            )
            .child(
                uniform_list(
                    ElementId::from((SharedString::from("virtual-songs"), title.len())),
                    list.len() + BOTTOM_SPACERS,
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        let t = Theme::for_appearance(window.appearance());
                        range
                            .map(|ix| {
                                if ix < rows.len() {
                                    this.table_row(&t, title, &rows, ix, cx).into_any_element()
                                } else {
                                    div().h(px(44.)).into_any_element()
                                }
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .px(px(PAD - 8.))
                .track_scroll(self.song_list_scroll.clone())
                .map(lock_scroll_axis),
            )
            .into_any_element()
    }

    pub(crate) fn table_row(&self, t: &Theme, id: &'static str, list: &Arc<Vec<Song>>, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_drag(self.drag_payload(&list[ix], &list), |d, _, _, cx| crate::browse::drag_preview(d, cx))
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
            .child(self.more_button(t, ElementId::from((SharedString::from(format!("{id}-more")), ix)), song, "row", cx))
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
    pub(crate) fn more_button(&self, t: &Theme, id: ElementId, song: &Song, hover_group: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(crate) fn album_tile(&self, t: &Theme, prefix: &'static str, i: usize, album: &Album, size: f32, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(8.))
                            .bg(gpui::black().opacity(0.16 * hover)),
                    )
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
            .child(
                div()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(t.text_secondary)
                    .child(album.artist.clone()),
            )
    }

    pub(crate) fn playlist_tile(&self, t: &Theme, prefix: &'static str, i: usize, playlist: &Playlist, size: f32, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(crate) fn mosaic_cell(&self, artwork: &Artwork, size: f32, ix: usize, radius: f32, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
    pub(crate) fn playlist_art(&self, playlist: &Playlist, size: f32, radius: f32, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
}

pub(crate) fn icon(name: &'static str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

pub(crate) fn icon_button(t: &Theme, id: &'static str, name: &'static str, size: f32, color: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(30.))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(t.hover))
        .active(|s| s.opacity(0.6))
        .child(icon(name, size, color))
}

pub(crate) fn pill_button(t: &Theme, id: &'static str, icon_name: &'static str, label: &'static str, filled: bool) -> Stateful<Div> {
    if filled {
        button(t, id, icon_name, label, t.accent.into(), white())
    } else {
        button(t, id, icon_name, label, t.button.into(), t.accent)
    }
}

/// Capsule button; the icon follows the label color.
pub(crate) fn button(_t: &Theme, id: &'static str, icon_name: &'static str, label: &'static str, bg: gpui::Fill, fg: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .min_w(px(112.))
        .px_4()
        .h(px(32.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .gap_1p5()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .bg(bg)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.7))
        .child(icon(icon_name, 12., fg))
        .child(label)
}

/// Small rounded toggle, filled with the accent when on (shuffle / repeat).
pub(crate) fn toggle_chip(t: &Theme, id: &'static str, icon_name: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(34.))
        .h(px(26.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .bg(if on { t.accent } else { t.button })
        .hover(|s| s.opacity(0.85))
        .child(icon(icon_name, 13., if on { white() } else { t.icon }))
}

/// `1395` → `1,395`.
pub(crate) fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn explicit_badge(t: &Theme) -> impl IntoElement {
    div()
        .size(px(13.))
        .flex_none()
        .rounded(px(2.5))
        .bg(t.text_tertiary)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(9.))
        .font_weight(FontWeight::BOLD)
        .text_color(t.window)
        .child("E")
}

pub(crate) fn table_header(t: &Theme) -> impl IntoElement {
    let header_cell = |label: &'static str| {
        div()
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(t.text_secondary)
            .child(label)
    };
    div()
        .px_2()
        .h(px(28.))
        .flex()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_color(t.separator)
        .child(header_cell("Title").flex_1().pl(px(44.)))
        .child(header_cell("Artist").w(relative(0.22)))
        .child(header_cell("Album").w(relative(0.22)))
        .child(div().w(px(44.)).flex().justify_end().child(icon("clock", 11., t.text_secondary)))
        .child(div().w(px(26.)))
}

pub(crate) fn page_title(title: &'static str) -> impl IntoElement {
    div()
        .px(px(PAD))
        .pt(px(46.))
        .pb_4()
        .text_size(px(28.))
        .font_weight(FontWeight::BOLD)
        .child(title)
}

pub(crate) fn section_header_owned(t: &Theme, title: String) -> impl IntoElement {
    div()
        .px(px(PAD))
        .pt_7()
        .pb_2p5()
        .flex()
        .items_center()
        .gap_1()
        .child(div().text_size(px(19.)).font_weight(FontWeight::BOLD).child(title))
        .child(icon("chevron-right", 14., t.text_secondary))
}

pub(crate) fn section_header(t: &Theme, title: &'static str) -> impl IntoElement {
    div()
        .px(px(PAD))
        .pt_7()
        .pb_2p5()
        .flex()
        .items_center()
        .gap_1()
        .child(div().text_size(px(19.)).font_weight(FontWeight::BOLD).child(title))
        .child(icon("chevron-right", 14., t.text_secondary))
}

pub(crate) fn loading(t: &Theme) -> Div {
    div().pt(px(160.)).flex().justify_center().text_color(t.text_secondary).child(
        div().child("Loading…").with_animation(
            "loading-pulse",
            Animation::new(Duration::from_millis(1200)).repeat().with_easing(pulsating_between(0.35, 1.)),
            |el, p| el.opacity(p),
        ),
    )
}

/// Placeholder for Home's personal shelves: a section of 4 song rows in two columns, then a
/// row of 160pt cards, matching the loaded layout's height.
pub(crate) fn skeleton_home_shelves(t: &Theme) -> impl IntoElement {
    let block = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(r)).bg(t.placeholder);
    let header = || div().px(px(PAD)).pt_7().pb_2p5().h(px(28. + 10. + 28.)).child(block(170., 20., 5.));
    let row = || {
        div()
            .h(px(60.))
            .flex()
            .items_center()
            .gap_3()
            .border_t_1()
            .border_color(t.separator)
            .child(block(44., 44., 5.))
            .child(div().flex().flex_col().gap_1p5().child(block(150., 11., 4.)).child(block(90., 9., 4.)))
    };
    let column = || div().flex_1().flex().flex_col().children((0..4).map(|_| row()));
    let card = || div().flex().flex_col().gap_2().child(block(160., 160., 8.)).child(block(120., 11., 4.)).child(block(80., 9., 4.));
    div()
        .child(header())
        .child(div().px(px(PAD)).flex().gap(px(GAP)).child(column()).child(column()))
        .child(header())
        .child(div().px(px(PAD)).pt_1().flex().gap(px(GAP)).children((0..6).map(|_| card())))
        .with_animation(
            "home-skeleton-pulse",
            Animation::new(Duration::from_millis(1300)).repeat().with_easing(pulsating_between(0.5, 1.)),
            |el, p| el.opacity(p),
        )
}

/// Placeholder layout shown while a browse page loads, gently pulsing.
pub(crate) fn skeleton(t: &Theme) -> impl IntoElement {
    let block = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(r)).bg(t.placeholder);
    let tiles = |n: usize, size: f32| {
        div().px(px(PAD)).flex().gap(px(GAP)).children((0..n).map(move |_| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(block(size, size, 8.))
                .child(block(size * 0.7, 12., 4.))
                .child(block(size * 0.45, 10., 4.))
        }))
    };
    div()
        .child(div().px(px(PAD)).pt(px(52.)).pb_5().child(block(140., 28., 6.)))
        .child(div().px(px(PAD)).flex().gap(px(GAP)).child(block(480., 270., 12.)).child(block(480., 270., 12.)))
        .child(div().px(px(PAD)).pt_8().pb_3().child(block(180., 20., 5.)))
        .child(tiles(6, 170.))
        .with_animation(
            "skeleton-pulse",
            Animation::new(Duration::from_millis(1300)).repeat().with_easing(pulsating_between(0.5, 1.)),
            |el, p| el.opacity(p),
        )
}

fn pulse<E: IntoElement + Styled + 'static>(id: &'static str, el: E) -> impl IntoElement {
    el.with_animation(
        id,
        Animation::new(Duration::from_millis(1300)).repeat().with_easing(pulsating_between(0.5, 1.)),
        |el, p| el.opacity(p),
    )
}

/// Placeholder song rows: art square, two text lines, duration.
pub(crate) fn skeleton_rows(t: &Theme, n: usize) -> impl IntoElement {
    let block = |w: f32, h: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(4.)).bg(t.placeholder);
    pulse(
        "rows-pulse",
        div().px(px(PAD)).pt_3().flex().flex_col().children((0..n).map(move |i| {
            div()
                .h(px(52.))
                .flex()
                .items_center()
                .gap_3()
                .border_t_1()
                .border_color(t.separator)
                .child(block(36., 36.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(block(160. + (i % 3) as f32 * 40., 11.))
                        .child(block(90. + (i % 2) as f32 * 30., 9.)),
                )
                .child(block(32., 10.))
        })),
    )
}

/// Placeholder album/playlist page: centered cover, title lines, buttons, rows.
pub(crate) fn skeleton_detail(t: &Theme) -> impl IntoElement {
    let block = |w: f32, h: f32, r: f32| div().w(px(w)).h(px(h)).flex_none().rounded(px(r)).bg(t.placeholder);
    div()
        .child(pulse(
            "detail-pulse",
            div()
                .pt(px(72.))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(block(300., 300., 10.))
                .child(div().pt_3().child(block(260., 24., 6.)))
                .child(block(150., 18., 5.))
                .child(block(110., 11., 4.))
                .child(
                    div()
                        .pt_3()
                        .flex()
                        .gap_4()
                        .child(block(48., 48., 24.))
                        .child(block(220., 48., 24.))
                        .child(block(48., 48., 24.)),
                ),
        ))
        .child(skeleton_rows(t, 6))
}

/// Overlay entrance: short and decelerating, so it feels responsive rather than slow.
pub(crate) fn pop_in() -> Animation {
    Animation::new(Duration::from_millis(170)).with_easing(ease_out_quint())
}

/// Fades and lifts each new page into place. Keyed by navigation count so it replays.
pub(crate) fn page_transition(
    generation: usize,
    direction: crate::app::NavDirection,
    fill: bool,
    page: AnyElement,
) -> impl IntoElement {
    use crate::app::NavDirection;
    let ms = if crate::anim::reduce_motion() { 1 } else { 280 };
    div().when(fill, |d| d.size_full()).pl(px(SIDEBAR_W)).child(page).with_animation(
        ("page-transition", generation),
        Animation::new(Duration::from_millis(ms)).with_easing(ease_out_quint()),
        move |el, p| {
            let el = el.opacity(p);
            match direction {
                NavDirection::Root => el.mt(px((1. - p) * 14.)),
                // Detail pages arrive from the right; going back, the previous page from the left.
                NavDirection::Forward => el.ml(px((1. - p) * 28.)),
                NavDirection::Back => el.ml(px((1. - p) * -28.)),
            }
        },
    )
}

pub(crate) fn empty_state(t: &Theme, title: &'static str, hint: &'static str) -> impl IntoElement {
    div()
        .pt(px(100.))
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).child(title))
        .child(div().text_color(t.text_secondary).child(hint))
}

pub(crate) fn gradient(from: u32, to: u32) -> gpui::Background {
    linear_gradient(135., linear_color_stop(rgb(from), 0.), linear_color_stop(rgb(to), 1.))
}

pub(crate) fn art_overlay(size: f32, child: Svg) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size(px(size))
        .rounded(px(5.))
        .bg(gpui::black().opacity(0.45))
        .flex()
        .items_center()
        .justify_center()
        .child(child)
}

pub(crate) fn scroll_row(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(PAD))
        .pb_2()
        .flex()
        .gap(px(GAP))
        .overflow_x_scroll()
        .map(lock_scroll_axis)
}

pub(crate) fn grid_container() -> Div {
    div().px(px(PAD)).flex().flex_wrap().gap_x(px(GAP)).gap_y(px(26.))
}

/// Without this, gpui maps a horizontal swipe onto a vertical-only container (and vice versa),
/// so swiping a carousel sideways would also scroll the page.
pub(crate) fn lock_scroll_axis<E: Styled>(mut element: E) -> E {
    element.style().restrict_scroll_to_axis = Some(true);
    element
}

/// Records an element's laid-out bounds so mouse positions can be mapped onto it.
pub(crate) fn capture_bounds(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {})
        .absolute()
        .size_full()
}

/// Columns and tile width that fill `width` with tiles at least `min_tile` wide.
pub(crate) fn grid_layout(width: f32, min_tile: f32, max_cols: usize) -> (usize, f32) {
    let avail = (width - 2. * PAD - 2.).max(min_tile);
    let cols = (((avail + GAP) / (min_tile + GAP)).floor() as usize).clamp(1, max_cols.max(1));
    let tile = (avail - GAP * (cols as f32 - 1.)) / cols as f32;
    (cols, tile.floor())
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_digits() {
        assert_eq!(super::group_digits(7), "7");
        assert_eq!(super::group_digits(1395), "1,395");
        assert_eq!(super::group_digits(1_234_567), "1,234,567");
    }
}
