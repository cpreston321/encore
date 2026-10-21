//! The window frame: floating sidebar, account popover, main area and context menus.

use super::*;

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
                self.nav_item(
                    t,
                    20 + i,
                    Some(art),
                    "playlist",
                    playlist.name.clone().into(),
                    Route::Playlist(playlist.clone()),
                    cx,
                )
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

        // Floats over the page, which runs underneath it to the window edge, as glass.
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
                // A lighter shade of the page (its tint on artwork pages), with a soft highlight
                // along the top.
                .glass_style(GlassStyle { blur: px(30.), ..GlassStyle::default() })
                .bg(self.fx.borrow_mut().sidebar_bg.update(crate::theme::glass_fill(
                    Hsla { l: (t.window.l + 0.07).min(0.92), ..t.window },
                    t.text,
                    t.text_secondary,
                    crate::theme::GLASS_MIN_ALPHA,
                )))
                .border_1()
                .border_color(t.text.opacity(0.12))
                .shadow_lg()
                .flex()
                .flex_col()
                .child(div().absolute().top_0().left_0().right_0().h(px(140.)).rounded_t(px(PANEL_RADIUS)).bg(
                    linear_gradient(
                        180.,
                        linear_color_stop(white().opacity(0.06), 0.),
                        linear_color_stop(white().opacity(0.), 1.),
                    ),
                ))
                .child(div().h(px(44.)).flex_none())
                .child(nav)
                .child(self.render_account(t, cx)),
        )
    }

    fn user_avatar(&self, size: f32) -> tahoe::Avatar {
        let avatar = tahoe::Avatar::new(self.user_name.clone()).size(size);
        match &self.avatar {
            Some(path) => avatar.image(img(path.clone()).size_full().object_fit(ObjectFit::Cover)),
            None => avatar,
        }
    }

    pub(crate) fn render_account(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
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
        let avatar = self.user_avatar(30.);
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
            .child(div().relative().flex_none().child(avatar).when(needs_attention, |d| {
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
            }))
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
                    .children(
                        status.map(|s| div().truncate().text_size(px(11.5)).text_color(t.text_tertiary).child(s)),
                    ),
            );

        match self.account {
            Account::SignedIn => row
                .when(self.show_account, |d| d.bg(t.selected))
                .when(!self.show_account, |d| d.hover(|s| s.bg(t.hover)))
                .cursor_pointer()
                .accessibility_role(Role::Button)
                .accessibility_label(format!("Account, {}", self.user_name))
                .child(capture_bounds(self.account_bounds.clone()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.show_account = !this.show_account;
                        this.menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                    if e.is_keyboard() {
                        this.show_account = !this.show_account;
                        this.menu = None;
                        cx.notify();
                    }
                })),
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
        let action =
            |id: &'static str, icon_name: &'static str, label: &'static str, detail: Option<String>, color: Hsla| {
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
            .glass_style(panel_glass())
            .bg(t.glass)
            .border_1()
            .border_color(t.panel_border)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div().p_4().flex().items_center().gap_3().child(self.user_avatar(48.)).child(
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
            .child(action("account-refresh", "repeat", "Refresh Library", Some(updated), t.text).on_click(cx.listener(
                |this, _, _, cx| {
                    this.show_account = false;
                    this.load_remote_library(cx);
                    this.show_notice("Refreshing your library…".into(), cx);
                },
            )))
            .child(
                action(
                    "account-animate",
                    if self.animate_artwork { "check" } else { "album" },
                    "Animate Artwork",
                    Some(if self.animate_artwork { "On: covers slowly drift" } else { "Off" }.into()),
                    t.text,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_animate_artwork(cx))),
            )
            .child(
                action(
                    "account-settings",
                    "open",
                    "Apple Music Account",
                    Some("Subscription and settings".into()),
                    t.text,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_account = false;
                    this.open_url("https://music.apple.com/account/settings", cx);
                })),
            )
            .child(div().mx_4().my_1().h(px(1.)).bg(t.separator))
            .child(action("account-sign-out", "person", "Sign Out", None, t.accent).on_click(cx.listener(
                |this, _, _, cx| {
                    this.show_account = false;
                    this.sign_out(cx);
                },
            )));

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
    pub(crate) fn sidebar_section(
        &self,
        t: &Theme,
        name: &'static str,
        rows: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.section_open(name);
        let full = rows.len() as f32 * (ROW_H + ROW_GAP);
        let group: SharedString = format!("section-{name}").into();
        div()
            .child(
                div()
                    .id(SharedString::from(format!("section-header-{name}")))
                    .accessibility_label(name)
                    .accessibility_value(if open > 0.5 { "expanded" } else { "collapsed" })
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
                            .child(icon(
                                if open > 0.5 { "chevron-down" } else { "chevron-right" },
                                11.,
                                t.text_tertiary,
                            )),
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
            self.playlist_covers(playlist, cx).into_iter().next()
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
            .accessibility_role(Role::Tab)
            .accessibility_label(label.clone())
            .accessibility_selected(selected)
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
                d.drag_over::<crate::ui::list_tools::DraggedSongs>(move |s, _, _, _| s.bg(accent.opacity(0.25)))
                    .on_drop(cx.listener(move |this, dragged: &crate::ui::list_tools::DraggedSongs, _, cx| {
                        this.add_songs_to_playlist(&dragged.0, &playlist, cx);
                    }))
            })
            .child(match leading {
                Some(art) => art,
                None => div()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(icon_name, 16., color))
                    .into_any_element(),
            })
            .child(div().truncate().child(label))
            .on_click(cx.listener(move |this, _, window, cx| this.go_root(route.clone(), window, cx)))
    }

    /// What scrolls in the page: the route and room to scroll it clear of the player.
    pub(crate) fn render_page_content(
        &self,
        t: &Theme,
        content_w: f32,
        search_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page_theme = self.page_palette(cx).map(|p| t.tinted(p));
        let page_t = page_theme.as_ref().unwrap_or(t);
        self.scroll_dependent_until.set(0.);
        div()
            .child(page_transition(
                self.route_generation,
                self.nav_direction,
                false,
                self.render_route(page_t, content_w, search_focused, cx),
            ))
            .child(div().h(px(110.)))
            .into_any_element()
    }

    /// The page beside the floating sidebar. `content` is the scrolling page's content as
    /// its own view (see `Region::Content`); pages that scroll themselves render inline.
    pub(crate) fn render_main(
        &self,
        t: &Theme,
        content_w: f32,
        search_focused: bool,
        content: gpui::AnyView,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let page_theme = self.page_palette(cx).map(|p| t.tinted(p));
        let page_t = page_theme.as_ref().unwrap_or(t);
        // Background color blends between pages instead of snapping.
        let page_bg = self.fx.borrow_mut().page_bg.update(page_t.window);
        let (queue, notice) = {
            let fx = self.fx.borrow();
            (fx.queue, fx.notice)
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
                    .child(page_transition(
                        self.route_generation,
                        direction,
                        true,
                        self.render_route(page_t, content_w, search_focused, cx),
                    ))
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
                    .map(|d| overlay_scrollbar(d, page_t, true))
                    .child(content)
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
                    div().absolute().left(px(SIDEBAR_W)).right_0().bottom(px(80.)).flex().justify_center().child(
                        div()
                            .id("toast")
                            .px_3()
                            .py_1p5()
                            .rounded_full()
                            .glass_style(panel_glass())
                            .bg(t.glass)
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.show_key_help = false;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("key-help")
                    .w(px(460.))
                    .p_6()
                    .rounded(px(16.))
                    .glass_style(panel_glass())
                    .bg(t.glass)
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
                    .child(div().font_family("Menlo").text_size(px(11.)).text_color(t.text_secondary).child(
                        "{ \"team_id\": \"…\", \"key_id\": \"…\", \"private_key_path\": \"/path/AuthKey_….p8\" }",
                    ))
                    .child(div().text_color(t.text_secondary).child("Then quit and reopen Encore."))
                    .child(div().pt_2().flex().justify_end().gap_2().child(
                        pill_button(t, "key-help-done", "check", "OK", true).on_click(cx.listener(|this, _, _, cx| {
                            this.show_key_help = false;
                            cx.notify();
                        })),
                    )),
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
            .glass_style(panel_glass())
            .bg(t.glass)
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
            deferred(anchored().position(menu.position).snap_to_window_with_margin(px(8.)).child(list.with_animation(
                "menu-in",
                pop_in(),
                |el, p| el.opacity(p).mt(px((1. - p) * -6.)),
            )))
            .with_priority(1)
            .into_any_element(),
        )
    }
}
