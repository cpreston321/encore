//! The window's main areas as cached views. GPUI replays a cached view's last frame until
//! that view is notified, so a progress tick redraws only the player, and scrolling or
//! hovering a page leaves the sidebar alone. Each frame then fits a 120Hz refresh.

use gpui::{AnyView, Entity, StyleRefinement, Subscription, Window};

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Region {
    Page,
    Sidebar,
    Player,
}

pub(crate) struct RegionView {
    app: Entity<MusicApp>,
    region: Region,
    _observe: Subscription,
    motion: Option<gpui::Task<()>>,
}

impl RegionView {
    fn new(app: Entity<MusicApp>, region: Region, cx: &mut Context<Self>) -> Self {
        // App state changes redraw every region; only region-local changes (scrolling,
        // hover fades, the progress bar) skip the others.
        let _observe = cx.observe(&app, |_, _, cx| cx.notify());
        Self { app, region, _observe, motion: None }
    }
}

impl Render for RegionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let region = self.region;
        let (element, moving) = self.app.update(cx, |app, cx| {
            app.fx.borrow_mut().motion_page = false;
            let element = app.render_region(region, window, cx);
            (element, app.fx.borrow().motion_page)
        });
        // Animated artwork on the page asks for its next frame here, so only the page redraws.
        if moving && self.motion.is_none() {
            self.motion = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(crate::app::MOTION_FRAME).await;
                this.update(cx, |this, cx| {
                    this.motion = None;
                    cx.notify();
                })
                .ok();
            }));
        }
        element
    }
}

pub(crate) struct Regions {
    page: Entity<RegionView>,
    sidebar: Entity<RegionView>,
    player: Entity<RegionView>,
}

impl Regions {
    pub(crate) fn new(cx: &mut Context<MusicApp>) -> Self {
        let app = cx.entity();
        let mut make = |region| cx.new(|cx| RegionView::new(app.clone(), region, cx));
        Self { page: make(Region::Page), player: make(Region::Player), sidebar: make(Region::Sidebar) }
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
                div().size_full().flex().child(self.render_main(&t, content_w, search_focused, cx)).into_any_element()
            }
            Region::Player => {
                let pill_w = (content_w - 48.).clamp(440., 780.);
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
