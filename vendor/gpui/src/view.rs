use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, Bounds, ContentMask, Context, Element, ElementId,
    Entity, EntityId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Length,
    PaintIndex, Pixels, Point, PrepaintStateIndex, Render, Size, Style, StyleRefinement, TextStyle,
    WeakEntity, point, size,
};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use std::rc::Rc;
use std::{any::TypeId, fmt, ops::Range};

struct AnyViewState {
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    cache_key: ViewCacheKey,
    accessed_entities: FxHashSet<EntityId>,
    /// For a scroll replay: the area its last render drew, in window coordinates, which
    /// covers its scroll container plus the overscan.
    drawn: Bounds<Pixels>,
    /// For a scroll replay: how far this frame's paint moves the last frame's.
    paint_delta: Point<Pixels>,
    /// For a scroll replay: the opacity it was drawn with, and the draw that recorded its
    /// ranges. A cached parent that replays its own frame doesn't visit this view, which
    /// leaves the ranges pointing into an older frame.
    opacity: f32,
    recorded_in: u64,
}


#[derive(Clone, Copy, Debug)]
struct ScrollReplay {
    overscan: Size<Pixels>,
    translate: bool,
}

type RenderedLayout = Option<(AnyElement, FxHashSet<EntityId>)>;

#[derive(Default)]
struct ViewCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
}

impl<V: Render> Element for Entity<V> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::View(self.entity_id()))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut element = window.timed(crate::FramePhase::Render, |window| {
            self.update(cx, |view, cx| view.render(window, cx).into_any_element())
        });
        let layout_id = window.with_rendered_view(self.entity_id(), |window| {
            element.request_layout(window, cx)
        });
        (layout_id, element)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.set_view_id(self.entity_id());
        window.with_rendered_view(self.entity_id(), |window| element.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rendered_view(self.entity_id(), |window| element.paint(window, cx));
    }
}

/// A dynamically-typed handle to a view, which can be downcast to a [Entity] for a specific type.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    cached_style: Option<Rc<StyleRefinement>>,
    scroll_replay: Option<ScrollReplay>,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
            cached_style: None,
            scroll_replay: None,
        }
    }
}

impl AnyView {
    /// Indicate that this view should be cached when using it as an element.
    /// When using this method, the view's previous layout and paint will be recycled from the previous frame if [Context::notify] has not been called since it was rendered.
    /// The one exception is when [Window::refresh] is called, in which case caching is ignored.
    pub fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style.into());
        self
    }

    /// Caches the view as the content of a scroll container. Like [`AnyView::cached`] it
    /// replays its last frame until it's notified, but it lays out at its content's size
    /// (measured whenever it renders), and when only its position has changed, because its
    /// container scrolled, it replays that frame moved by the scroll: primitives, hitboxes,
    /// cursor regions and listeners, without rendering, laying out or painting.
    ///
    /// Each render draws `overscan` beyond the container's content mask on each side, so a
    /// scroll reveals content that's already drawn. It renders again once a scroll would
    /// show past that, when the move would change which hitboxes the mouse is over, while
    /// something is dragged, and when the last frame has deferred draws or input handlers.
    /// Pass `translate: false` for frames that must render, such as when something drawn
    /// depends on the scroll offset.
    pub fn scroll_cached(mut self, overscan: Size<Pixels>, translate: bool) -> Self {
        self.scroll_replay = Some(ScrollReplay { overscan, translate });
        self
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
                cached_style: self.cached_style,
                scroll_replay: self.scroll_replay,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// Gets the entity id of this handle.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

impl Element for AnyView {
    type RequestLayoutState = RenderedLayout;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::View(self.entity_id()))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        window.with_rendered_view(self.entity_id(), |window| {
            // Disable caching when inspecting so that mouse_hit_test has all hitboxes.
            let caching_disabled = window.is_inspector_picking(cx);
            if self.scroll_replay.is_some() && !caching_disabled {
                return self.scroll_request_layout(_id.unwrap(), window, cx);
            }
            match self.cached_style.as_ref() {
                Some(style) if !caching_disabled => {
                    let mut root_style = Style::default();
                    root_style.refine(style);
                    let layout_id = window.request_layout(root_style, None, cx);
                    (layout_id, None)
                }
                _ => {
                    let mut element = (self.render)(self, window, cx);
                    let layout_id = element.request_layout(window, cx);
                    (layout_id, Some((element, FxHashSet::default())))
                }
            }
        })
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        window.set_view_id(self.entity_id());
        window.with_rendered_view(self.entity_id(), |window| {
            if let Some(replay) = self.scroll_replay
                && !window.is_inspector_picking(cx)
            {
                return self.scroll_prepaint(replay, global_id.unwrap(), bounds, element, window, cx);
            }
            if let Some((mut element, _)) = element.take() {
                element.prepaint(window, cx);
                return Some(element);
            }

            window.with_element_state::<AnyViewState, _>(
                global_id.unwrap(),
                |element_state, window| {
                    let content_mask = window.content_mask();
                    let text_style = window.text_style();

                    if let Some(mut element_state) = element_state
                        && element_state.cache_key.bounds == bounds
                        && element_state.cache_key.content_mask == content_mask
                        && element_state.cache_key.text_style == text_style
                        && !window.dirty_views.contains(&self.entity_id())
                        && !window.refreshing
                    {
                        let prepaint_start = window.prepaint_index();
                        window.reuse_prepaint(element_state.prepaint_range.clone());
                        cx.entities
                            .extend_accessed(&element_state.accessed_entities);
                        let prepaint_end = window.prepaint_index();
                        element_state.prepaint_range = prepaint_start..prepaint_end;

                        return (None, element_state);
                    }

                    let refreshing = window.begin_cache_miss();
                    let prepaint_start = window.prepaint_index();
                    let (mut element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                        let mut element = (self.render)(self, window, cx);
                        element.layout_as_root(bounds.size.into(), window, cx);
                        element.prepaint_at(bounds.origin, window, cx);
                        element
                    });

                    let prepaint_end = window.prepaint_index();
                    window.end_cache_miss(refreshing);

                    (
                        Some(element),
                        AnyViewState {
                            accessed_entities,
                            prepaint_range: prepaint_start..prepaint_end,
                            paint_range: PaintIndex::default()..PaintIndex::default(),
                            cache_key: ViewCacheKey {
                                bounds,
                                content_mask,
                                text_style,
                            },
                            drawn: Bounds::default(),
                            paint_delta: Point::default(),
                            opacity: 1.,
                            recorded_in: 0,
                        },
                    )
                },
            )
        })
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rendered_view(self.entity_id(), |window| {
            let caching_disabled = window.is_inspector_picking(cx);
            if self.scroll_replay.is_some() && !caching_disabled {
                self.scroll_paint(global_id.unwrap(), element, window, cx);
            } else if self.cached_style.is_some() && !caching_disabled {
                window.with_element_state::<AnyViewState, _>(
                    global_id.unwrap(),
                    |element_state, window| {
                        let mut element_state = element_state.unwrap();

                        let paint_start = window.paint_index();

                        if let Some(element) = element {
                            let refreshing = window.begin_cache_miss();
                            element.paint(window, cx);
                            window.end_cache_miss(refreshing);
                        } else {
                            window.reuse_paint(element_state.paint_range.clone());
                        }

                        let paint_end = window.paint_index();
                        element_state.paint_range = paint_start..paint_end;

                        ((), element_state)
                    },
                )
            } else {
                element.as_mut().unwrap().paint(window, cx);
            }
        });
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = Entity<V>;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl AnyView {
    fn scroll_request_layout(
        &mut self,
        id: &GlobalElementId,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, RenderedLayout) {
        let clean = !window.dirty_views.contains(&self.entity_id()) && !window.refreshing_all();
        let reuse_size = clean
            .then(|| {
                window.peek_element_state::<AnyViewState, _>(id, |state| {
                    (state.recorded_in + 1 == window.draw_count()).then_some(state.cache_key.bounds.size)
                })
            })
            .flatten()
            .flatten();
        if let Some(reuse_size) = reuse_size {
            let mut style = Style::default();
            style.size = reuse_size.map(|s| Length::Definite(s.into()));
            style.flex_shrink = 0.;
            return (window.request_layout(style, None, cx), None);
        }
        let refreshing = window.begin_cache_miss();
        let ((element, layout_id), accessed) = cx.detect_accessed_entities(|cx| {
            let mut element = (self.render)(self, window, cx);
            let layout_id = element.request_layout(window, cx);
            (element, layout_id)
        });
        window.end_cache_miss(refreshing);
        (layout_id, Some((element, accessed)))
    }

    fn scroll_prepaint(
        &mut self,
        replay: ScrollReplay,
        global_id: &GlobalElementId,
        bounds: Bounds<Pixels>,
        layout: &mut RenderedLayout,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let content_mask = window.content_mask();
        let text_style = window.text_style();
        let overscan = replay.overscan;
        let drawn = Bounds {
            origin: content_mask.bounds.origin - point(overscan.width, overscan.height),
            size: size(
                content_mask.bounds.size.width + overscan.width * 2.,
                content_mask.bounds.size.height + overscan.height * 2.,
            ),
        };
        let (opacity, draw_count) = (window.element_opacity, window.draw_count());
        window.with_element_state::<AnyViewState, _>(global_id, |state, window| {
            let new_state = |prepaint_range, accessed_entities, size| AnyViewState {
                prepaint_range,
                paint_range: PaintIndex::default()..PaintIndex::default(),
                cache_key: ViewCacheKey {
                    bounds: Bounds {
                        origin: bounds.origin,
                        size,
                    },
                    content_mask: content_mask.clone(),
                    text_style: text_style.clone(),
                },
                accessed_entities,
                drawn,
                paint_delta: Point::default(),
                opacity,
                recorded_in: draw_count,
            };

            if let Some((mut element, mut accessed)) = layout.take() {
                window.count_scroll_draw(false);
                let refreshing = window.begin_cache_miss();
                let start = window.prepaint_index();
                let (_, prepaint_accessed) = cx.detect_accessed_entities(|cx| {
                    window.with_scroll_clip(drawn, false, |window| element.prepaint(window, cx))
                });
                let end = window.prepaint_index();
                window.end_cache_miss(refreshing);
                accessed.extend(prepaint_accessed);
                return (Some(element), new_state(start..end, accessed, bounds.size));
            }

            if let Some(mut state) = state
                && state.cache_key.content_mask == content_mask
                && state.cache_key.text_style == text_style
                && state.cache_key.bounds.size == bounds.size
                && state.recorded_in + 1 == draw_count
                && state.opacity == opacity
                && !window.dirty_views.contains(&self.entity_id())
                && !window.refreshing_all()
            {
                let delta = bounds.origin - state.cache_key.bounds.origin;
                let visible = window.scroll_clip_for(&content_mask);
                let moved_drawn = state.drawn + delta;
                let still_drawn = moved_drawn.origin.x <= visible.bounds.origin.x
                    && moved_drawn.origin.y <= visible.bounds.origin.y
                    && moved_drawn.bottom_right().x >= visible.bounds.bottom_right().x
                    && moved_drawn.bottom_right().y >= visible.bounds.bottom_right().y;
                if (delta == Point::default())
                    || (replay.translate
                        && still_drawn
                        && window.can_replay_translated(
                            &state.prepaint_range,
                            &state.paint_range,
                            delta,
                            &visible,
                            cx,
                        ))
                {
                    let start = window.prepaint_index();
                    if delta == Point::default() {
                        window.reuse_prepaint(state.prepaint_range.clone());
                    } else {
                        window.reuse_prepaint_translated(
                            state.prepaint_range.clone(),
                            delta,
                            &visible,
                        );
                    }
                    cx.entities.extend_accessed(&state.accessed_entities);
                    if delta != Point::default() {
                        window.count_scroll_draw(true);
                    }
                    state.prepaint_range = start..window.prepaint_index();
                    state.cache_key.bounds = bounds;
                    state.drawn = moved_drawn;
                    state.paint_delta = delta;
                    state.recorded_in = window.draw_count();
                    return (None, state);
                }
            }

            window.count_scroll_draw(false);
            let refreshing = window.begin_cache_miss();
            let start = window.prepaint_index();
            let ((element, measured), accessed) = cx.detect_accessed_entities(|cx| {
                window.with_scroll_clip(drawn, false, |window| {
                    let mut element = (self.render)(self, window, cx);
                    let measured = element.layout_as_root(bounds.size.into(), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    (element, measured)
                })
            });
            let end = window.prepaint_index();
            window.end_cache_miss(refreshing);
            if measured != bounds.size {
                // Laid out at its old size this frame; render inline at the new one next.
                window.request_animation_frame();
            }
            (Some(element), new_state(start..end, accessed, measured))
        })
    }

    fn scroll_paint(
        &mut self,
        global_id: &GlobalElementId,
        element: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_element_state::<AnyViewState, _>(global_id, |state, window| {
            let mut state = state.unwrap();
            let paint_range = window.with_scroll_clip(state.drawn, true, |window| {
                let start = window.paint_index();
                if let Some(element) = element {
                    let refreshing = window.begin_cache_miss();
                    element.paint(window, cx);
                    window.end_cache_miss(refreshing);
                } else {
                    window.reuse_paint_translated(state.paint_range.clone(), state.paint_delta);
                }
                start..window.paint_index()
            });
            state.paint_range = paint_range;
            ((), state)
        })
    }
}

impl IntoElement for AnyView {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// A weak, dynamically-typed view handle that does not prevent the view from being released.
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl AnyWeakView {
    /// Convert to a strongly-typed handle if the referenced view has not yet been released.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
            cached_style: None,
            scroll_replay: None,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        window.timed(crate::FramePhase::Render, |window| {
            view.update(cx, |view, cx| view.render(window, cx).into_any_element())
        })
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}
