//! The vendored gpui's scroll replay (`AnyView::scroll_cached`): a scroll moves the content's
//! last frame instead of rendering it, and clicks land on what's under the mouse after the move.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyView, AppContext, Context, Entity, InteractiveElement, IntoElement, Modifiers, ParentElement, Render,
    ScrollHandle, StatefulInteractiveElement, StyleRefinement, Styled, TestAppContext, VisualTestContext, Window, div,
    point, px, size,
};

const ROW_H: f32 = 40.;
const VIEWPORT_H: f32 = 300.;

struct Content {
    renders: Rc<Cell<usize>>,
    clicked: Rc<Cell<Option<usize>>>,
}

impl Render for Content {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div().children((0..100).map(|ix| {
            let clicked = self.clicked.clone();
            div()
                .id(ix)
                .h(px(ROW_H))
                .w_full()
                .bg(gpui::red())
                .child(format!("row {ix}"))
                .on_click(move |_, _, _| clicked.set(Some(ix)))
        }))
    }
}

struct Page {
    content: Entity<Content>,
    scroll: ScrollHandle,
    translate: bool,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let content = AnyView::from(self.content.clone()).scroll_cached(size(px(0.), px(VIEWPORT_H)), self.translate);
        div().size_full().child(
            div()
                .id("scroll")
                .w(px(400.))
                .h(px(VIEWPORT_H))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(content),
        )
    }
}

/// Like Encore's window: the page and a sibling (the player) as cached views.
struct Root {
    page: Entity<Page>,
    sibling: Entity<Sibling>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let cached = |view: AnyView| view.cached(StyleRefinement::default().absolute().inset_0());
        div().size_full().child(cached(self.page.clone().into())).child(cached(self.sibling.clone().into()))
    }
}

struct Sibling;

impl Render for Sibling {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct Harness {
    page: Entity<Page>,
    sibling: Entity<Sibling>,
    renders: Rc<Cell<usize>>,
    clicked: Rc<Cell<Option<usize>>>,
}

impl Harness {
    fn new(cx: &mut TestAppContext) -> (Self, &mut VisualTestContext) {
        let renders = Rc::new(Cell::new(0));
        let clicked = Rc::new(Cell::new(None));
        let (r, c) = (renders.clone(), clicked.clone());
        let (root, cx) = cx.add_window_view(move |_, cx| Root {
            page: cx.new(|cx| Page {
                content: cx.new(|_| Content { renders: r, clicked: c }),
                scroll: ScrollHandle::new(),
                translate: true,
            }),
            sibling: cx.new(|_| Sibling),
        });
        let (page, sibling) = root.read_with(cx, |root, _| (root.page.clone(), root.sibling.clone()));
        // Park the mouse outside the scroll view so it hovers nothing that moves.
        cx.simulate_mouse_move(point(px(600.), px(600.)), None, Modifiers::none());
        (Self { page, sibling, renders, clicked }, cx)
    }

    /// Scrolls to `y` and returns whether the content rendered.
    fn scroll_to(&self, y: f32, cx: &mut VisualTestContext) -> bool {
        let before = self.renders.get();
        self.page.update(cx, |page, cx| {
            page.scroll.set_offset(point(px(0.), px(-y)));
            cx.notify();
        });
        cx.run_until_parked();
        self.renders.get() != before
    }

    /// The row a click at `y` lands on. Clicking refreshes the window, which renders the
    /// content again, so the mouse goes back outside the scroll view afterwards.
    fn row_at(&self, y: f32, cx: &mut VisualTestContext) -> Option<usize> {
        self.clicked.set(None);
        cx.simulate_click(point(px(10.), px(y)), Modifiers::none());
        cx.simulate_mouse_move(point(px(600.), px(600.)), None, Modifiers::none());
        self.clicked.get()
    }

    /// The rows drawn, as (top, visible height), from their quads.
    fn drawn_rows(cx: &mut VisualTestContext) -> Vec<(f32, f32)> {
        let quads = cx.update(|window, _| window.drawn_quads());
        quads
            .into_iter()
            .filter(|(bounds, _)| bounds.size.height == px(ROW_H))
            .map(|(bounds, mask)| {
                let visible = bounds.intersect(&mask);
                (f32::from(bounds.origin.y), f32::from(visible.size.height).max(0.))
            })
            .collect()
    }

    fn replays(cx: &mut VisualTestContext) -> u32 {
        cx.update(|window, _| window.frame_timings().scroll_replays)
    }
}

#[gpui::test]
fn scrolling_moves_the_last_frame(cx: &mut TestAppContext) {
    let (h, cx) = Harness::new(cx);

    assert!(!h.scroll_to(100., cx), "a scroll within the overscan doesn't render");
    assert_eq!(Harness::replays(cx), 1);
    assert_eq!(h.row_at(30., cx), Some(3), "hitboxes move with the content");

    assert!(!h.scroll_to(250., cx));
    assert!(!h.scroll_to(300., cx));
    assert_eq!(h.row_at(30., cx), Some(8));

    // Past the overscan drawn at the last render, it renders again.
    assert!(h.scroll_to(950., cx));
    assert_eq!(Harness::replays(cx), 0);
    assert_eq!(h.row_at(30., cx), Some(24));

    assert!(!h.scroll_to(750., cx));
    assert_eq!(h.row_at(30., cx), Some(19));
}

#[gpui::test]
fn renders_when_told_not_to_translate(cx: &mut TestAppContext) {
    let (h, cx) = Harness::new(cx);
    h.page.update(cx, |page, _| page.translate = false);
    assert!(h.scroll_to(10., cx));
    assert_eq!(Harness::replays(cx), 0);
}

#[gpui::test]
fn renders_when_the_mouse_would_hover_something_else(cx: &mut TestAppContext) {
    let (h, cx) = Harness::new(cx);
    cx.simulate_mouse_move(point(px(10.), px(30.)), None, Modifiers::none());
    // Row 0 (0–40) stays under the mouse.
    assert!(!h.scroll_to(5., cx));
    // Row 1 moves under it.
    assert!(h.scroll_to(15., cx));
}

#[gpui::test]
fn renders_when_notified(cx: &mut TestAppContext) {
    let (h, cx) = Harness::new(cx);
    let content = h.page.read_with(cx, |page, _| page.content.clone());
    let before = h.renders.get();
    h.page.update(cx, |page, cx| {
        content.update(cx, |_, cx| cx.notify());
        page.scroll.set_offset(point(px(0.), px(-50.)));
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(h.renders.get(), before + 1);
    assert_eq!(h.row_at(30., cx), Some(1));
}

#[gpui::test]
fn renders_after_a_frame_that_replayed_its_parent(cx: &mut TestAppContext) {
    let (h, cx) = Harness::new(cx);
    assert!(!h.scroll_to(100., cx));
    // Only the sibling draws, so the page replays its own frame without visiting the content.
    h.sibling.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(h.scroll_to(150., cx));
    assert_eq!(h.row_at(30., cx), Some(4));
}

#[gpui::test]
fn replays_draw_what_scrolls_into_view(cx: &mut TestAppContext) {
    let (h, cx) = Harness::new(cx);
    assert!(!h.scroll_to(270., cx));
    let rows = Harness::drawn_rows(cx);
    // Rows 6 to 14 overlap the 300pt view at 270; row 14 was below it when last rendered.
    let tops: Vec<f32> = rows.iter().map(|(top, _)| *top).collect();
    let expected: Vec<f32> = (6..15).map(|ix| ix as f32 * ROW_H - 270.).collect();
    assert_eq!(tops, expected);
    // Clipped to the view: the first and last rows show only their visible part.
    assert_eq!(rows.first().unwrap().1, 10.);
    assert_eq!(rows.last().unwrap().1, 10.);
}
