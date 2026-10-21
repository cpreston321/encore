//! The window's main areas as cached views. GPUI replays a cached view's last frame until
//! that view is notified, so a progress tick redraws only the player, and scrolling or
//! hovering a page leaves the sidebar alone. Each frame then fits a 120Hz refresh.

use gpui::{AnyView, Entity, StyleRefinement, Subscription, Window, size};

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Region {
    Page,
    /// What scrolls inside the page. Scrolling notifies only the page, so this view replays
    /// its last frame moved by the scroll instead of rendering (`AnyView::scroll_cached`).
    Content,
    Sidebar,
    Player,
}

pub(crate) struct RegionView {
    app: Entity<MusicApp>,
    region: Region,
    _observe: Subscription,
}

impl RegionView {
    fn new(app: Entity<MusicApp>, region: Region, cx: &mut Context<Self>) -> Self {
        // App state changes redraw every region; only region-local changes (scrolling,
        // hover fades, the progress bar) skip the others.
        let _observe = cx.observe(&app, |_, _, cx| cx.notify());
        Self { app, region, _observe }
    }
}

impl Render for RegionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let region = self.region;
        self.app.update(cx, |app, cx| app.render_region(region, window, cx))
    }
}

pub(crate) struct Regions {
    page: Entity<RegionView>,
    content: Entity<RegionView>,
    sidebar: Entity<RegionView>,
    player: Entity<RegionView>,
}

impl Regions {
    pub(crate) fn new(cx: &mut Context<MusicApp>) -> Self {
        let app = cx.entity();
        let mut make = |region| cx.new(|cx| RegionView::new(app.clone(), region, cx));
        Self {
            page: make(Region::Page),
            content: make(Region::Content),
            player: make(Region::Player),
            sidebar: make(Region::Sidebar),
        }
    }

    /// Back to front: the page, the player floating over it, then the sidebar.
    pub(crate) fn views(&self) -> [AnyView; 3] {
        let cached = |view: &Entity<RegionView>| {
            AnyView::from(view.clone()).cached(StyleRefinement::default().absolute().inset_0())
        };
        [cached(&self.page), cached(&self.player), cached(&self.sidebar)]
    }
}

impl MusicApp {
    /// The page content as a scroll replay. It draws a viewport beyond the page above and
    /// below, and renders again while scroll-dependent parts (parallax headers) are on screen.
    fn content_view(&self, window: &Window) -> AnyView {
        let view = self.regions.as_ref().map(|r| r.content.clone()).expect("regions exist before they render");
        let scrolled = -f32::from(self.scroll.offset().y);
        let translate = scrolled >= self.scroll_dependent_until.get();
        AnyView::from(view).scroll_cached(size(px(0.), window.viewport_size().height), translate)
    }
}

impl MusicApp {
    fn render_region(&mut self, region: Region, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::for_appearance(window.appearance());
        // Pages span the whole window (art and tints run under the floating sidebar); this is
        // the visible width beside it.
        let content_w = f32::from(window.viewport_size().width) - SIDEBAR_W;
        let element = match region {
            Region::Page => {
                #[cfg(any(debug_assertions, feature = "tour"))]
                self.step_tour_scroll(window);
                let search_focused = self.search_focus.is_focused(window);
                let content = self.content_view(window);
                div()
                    .size_full()
                    .flex()
                    .child(self.render_main(&t, content_w, search_focused, content, cx))
                    .into_any_element()
            }
            Region::Content => {
                let search_focused = self.search_focus.is_focused(window);
                self.render_page_content(&t, content_w, search_focused, cx)
            }
            Region::Player => {
                let pill_w = (content_w - 48.).clamp(440., 780.);
                if self.progress.read(cx).hover_amount().1 {
                    window.request_animation_frame();
                }
                div()
                    .size_full()
                    .child(
                        div()
                            .absolute()
                            .left(px(SIDEBAR_W))
                            .right_0()
                            .bottom(px(14.))
                            .flex()
                            .justify_center()
                            .child(self.render_player(&t, pill_w, cx)),
                    )
                    .into_any_element()
            }
            Region::Sidebar => {
                // On artwork-tinted pages the sidebar takes the page's colors, keeping the
                // app's red for selection.
                let sidebar_t = self.page_palette(cx).map(|p| Theme { accent: t.accent, ..t.tinted(p) }).unwrap_or(t);
                div().size_full().child(self.render_sidebar(&sidebar_t, cx)).into_any_element()
            }
        };
        if self.fx.borrow().region_animating(region) {
            window.request_animation_frame();
        }
        element
    }
}
