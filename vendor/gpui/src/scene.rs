// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Pixels,
    Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
pub(crate) type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

pub(crate) type DrawOrder = u32;

#[derive(Default)]
pub(crate) struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    clip_stack: Vec<ContentMask<ScaledPixels>>,
    pub(crate) shadows: Vec<Shadow>,
    pub(crate) quads: Vec<Quad>,
    pub(crate) paths: Vec<Path<ScaledPixels>>,
    pub(crate) underlines: Vec<Underline>,
    pub(crate) monochrome_sprites: Vec<MonochromeSprite>,
    pub(crate) polychrome_sprites: Vec<PolychromeSprite>,
    pub(crate) surfaces: Vec<PaintSurface>,
    pub(crate) backdrop_blurs: Vec<BackdropBlur>,
    /// A visible sprite has `motion`, so the window keeps presenting this scene.
    pub(crate) has_motion: bool,
    /// Seconds of motion so far, read by the shader for sprites with `motion`. Set by the
    /// window before each present.
    pub(crate) motion_time: std::cell::Cell<f32>,
}

impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.clip_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
        self.backdrop_blurs.clear();
        self.has_motion = false;
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let clipped = match self.clip_stack.last() {
            Some(clip) => bounds.intersect(&clip.bounds),
            None => bounds,
        };
        let order = self
            .primitive_bounds
            .insert(if clipped.is_empty() { bounds } else { clipped });
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    /// Clips what's drawn until the matching `pop_clip` without changing the recorded
    /// primitives, which keep their own masks so a scroll replay can reveal more of them.
    pub(crate) fn push_clip(&mut self, mask: ContentMask<ScaledPixels>) {
        let clip = match self.clip_stack.last() {
            Some(outer) => intersect_scaled_masks(&mask, outer),
            None => mask.clone(),
        };
        self.clip_stack.push(clip);
        self.paint_operations.push(PaintOperation::PushClip(mask));
    }

    pub(crate) fn pop_clip(&mut self) {
        self.clip_stack.pop();
        self.paint_operations.push(PaintOperation::PopClip);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let recorded = primitive.into();
        let recorded_bounds = recorded
            .bounds()
            .intersect(&recorded.content_mask().bounds);
        if recorded_bounds.is_empty() {
            return;
        }
        let shown = match self.clip_stack.last() {
            None => Some(recorded.clone()),
            Some(clip) if recorded_bounds.intersects(&clip.bounds) => {
                let mut shown = recorded.clone();
                shown.set_content_mask(intersect_scaled_masks(recorded.content_mask(), clip));
                Some(shown)
            }
            // Outside the clip it isn't drawn, but it's recorded for a scroll replay to reveal.
            Some(_) => None,
        };
        if let Some(shown) = shown {
            self.insert_shown(shown);
        }
        self.paint_operations
            .push(PaintOperation::Primitive(recorded));
    }

    fn insert_shown(&mut self, primitive: Primitive) {
        let clipped_bounds = primitive
            .bounds()
            .intersect(&primitive.content_mask().bounds);
        if clipped_bounds.is_empty() {
            return;
        }

        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
        match primitive {
            Primitive::Shadow(mut shadow) => {
                shadow.order = order;
                self.shadows.push(shadow);
            }
            Primitive::Quad(mut quad) => {
                quad.order = order;
                self.quads.push(quad);
            }
            Primitive::Path(mut path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path);
            }
            Primitive::Underline(mut underline) => {
                underline.order = order;
                self.underlines.push(underline);
            }
            Primitive::MonochromeSprite(mut sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(sprite);
            }
            Primitive::PolychromeSprite(mut sprite) => {
                sprite.order = order;
                // A sliver at the edge of a scroll view is usually under a toolbar or header,
                // and not worth presenting for.
                let area = |b: Bounds<ScaledPixels>| b.size.width.0 * b.size.height.0;
                self.has_motion |=
                    sprite.motion.strength > 0. && area(clipped_bounds) >= 0.1 * area(sprite.bounds);
                self.polychrome_sprites.push(sprite);
            }
            Primitive::Surface(mut surface) => {
                surface.order = order;
                self.surfaces.push(surface);
            }
            Primitive::BackdropBlur(mut blur) => {
                blur.order = order;
                self.backdrop_blurs.push(blur);
            }
        }
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
                PaintOperation::PushClip(mask) => self.push_clip(mask.clone()),
                PaintOperation::PopClip => self.pop_clip(),
            }
        }
    }

    /// Replays part of the previous scene moved by `delta`.
    pub(crate) fn replay_translated(
        &mut self,
        range: Range<usize>,
        prev_scene: &Scene,
        delta: Point<ScaledPixels>,
    ) {
        if delta.x.0 == 0. && delta.y.0 == 0. {
            return self.replay(range, prev_scene);
        }
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => {
                    let mut primitive = primitive.clone();
                    primitive.translate(delta);
                    self.insert_primitive(primitive)
                }
                PaintOperation::StartLayer(bounds) => self.push_layer(moved(*bounds, delta)),
                PaintOperation::EndLayer => self.pop_layer(),
                PaintOperation::PushClip(mask) => self.push_clip(translated_mask(mask, delta)),
                PaintOperation::PopClip => self.pop_clip(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.surfaces.sort_by_key(|surface| surface.order);
        self.backdrop_blurs.sort_by_key(|blur| blur.order);
    }

    /// For each backdrop blur, in `backdrop_blurs` order, a hash of everything drawn under its
    /// sample area before it, so a renderer can reuse last frame's blur while it's unchanged.
    /// `None` means the backdrop can change while the scene doesn't: GPU-animated sprites
    /// (`motion`) or video surfaces are under it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn backdrop_signatures(&self) -> Vec<Option<u64>> {
        let mut signatures: Vec<Option<u64>> = Vec::with_capacity(self.backdrop_blurs.len());
        for blur in &self.backdrop_blurs {
            signatures.push(self.backdrop_signature(blur, &signatures));
        }
        signatures
    }

    fn backdrop_signature(&self, blur: &BackdropBlur, earlier: &[Option<u64>]) -> Option<u64> {
        use std::hash::Hasher;
        let area = blur.bounds;
        let below = blur.order;
        let under = |bounds: Bounds<ScaledPixels>, mask: &ContentMask<ScaledPixels>| {
            bounds.intersect(&mask.bounds).intersects(&area)
        };
        let mut h = collections::FxHasher::default();
        let mut sig = Sig(&mut h);
        for shadow in self.shadows.iter().take_while(|p| p.order < below) {
            if under(shadow.bounds.dilate(shadow.blur_radius * 3.), &shadow.content_mask) {
                sig.u(1);
                sig.bounds(&shadow.bounds);
                sig.mask(&shadow.content_mask);
                sig.corners(&shadow.corner_radii);
                sig.hsla(&shadow.color);
                sig.f(shadow.blur_radius.0);
            }
        }
        for quad in self.quads.iter().take_while(|p| p.order < below) {
            if under(quad.bounds, &quad.content_mask) {
                sig.u(2);
                sig.bounds(&quad.bounds);
                sig.mask(&quad.content_mask);
                sig.background(&quad.background);
                sig.hsla(&quad.border_color);
                sig.corners(&quad.corner_radii);
                let e = &quad.border_widths;
                sig.fs(&[e.top.0, e.right.0, e.bottom.0, e.left.0]);
                sig.u(quad.border_style as u32);
            }
        }
        for path in self.paths.iter().take_while(|p| p.order < below) {
            if under(path.bounds, &path.content_mask) {
                sig.u(3);
                sig.bounds(&path.bounds);
                sig.mask(&path.content_mask);
                sig.background(&path.color);
                for v in &path.vertices {
                    sig.fs(&[v.xy_position.x.0, v.xy_position.y.0, v.st_position.x, v.st_position.y]);
                }
            }
        }
        for underline in self.underlines.iter().take_while(|p| p.order < below) {
            if under(underline.bounds, &underline.content_mask) {
                sig.u(4);
                sig.bounds(&underline.bounds);
                sig.mask(&underline.content_mask);
                sig.hsla(&underline.color);
                sig.f(underline.thickness.0);
                sig.u(underline.wavy);
            }
        }
        for sprite in self.monochrome_sprites.iter().take_while(|p| p.order < below) {
            // A transformed sprite can draw outside its bounds, but not outside its mask.
            if under(sprite.content_mask.bounds, &sprite.content_mask) {
                sig.u(5);
                sig.bounds(&sprite.bounds);
                sig.mask(&sprite.content_mask);
                sig.hsla(&sprite.color);
                sig.tile(&sprite.tile);
                let m = &sprite.transformation;
                sig.fs(&[
                    m.rotation_scale[0][0],
                    m.rotation_scale[0][1],
                    m.rotation_scale[1][0],
                    m.rotation_scale[1][1],
                    m.translation[0],
                    m.translation[1],
                ]);
            }
        }
        for sprite in self.polychrome_sprites.iter().take_while(|p| p.order < below) {
            if under(sprite.bounds, &sprite.content_mask) {
                if sprite.motion.strength > 0. {
                    return None;
                }
                sig.u(6);
                sig.bounds(&sprite.bounds);
                sig.mask(&sprite.content_mask);
                sig.corners(&sprite.corner_radii);
                sig.tile(&sprite.tile);
                sig.bounds(&sprite.texture_crop);
                sig.u(sprite.grayscale as u32);
                sig.f(sprite.opacity);
            }
        }
        for surface in self.surfaces.iter().take_while(|p| p.order < below) {
            if under(surface.bounds, &surface.content_mask) {
                return None;
            }
        }
        for (other, signature) in self.backdrop_blurs.iter().zip(earlier) {
            if other.order < below && under(other.bounds, &other.content_mask) {
                sig.u(7);
                sig.u64((*signature)?);
                sig.bounds(&other.bounds);
                sig.bounds(&other.shape);
                sig.mask(&other.content_mask);
                sig.corners(&other.corner_radii);
                sig.hsla(&other.tint);
                sig.fs(&[
                    other.radius.0,
                    other.saturation,
                    other.brightness,
                    other.rim,
                    other.rim_width.0,
                    other.refraction.0,
                    other.opacity,
                ]);
            }
        }
        Some(h.finish())
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub(crate) fn batches(&self) -> impl Iterator<Item = PrimitiveBatch<'_>> {
        BatchIterator {
            shadows: &self.shadows,
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            quads: &self.quads,
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths: &self.paths,
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines: &self.underlines,
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites: &self.monochrome_sprites,
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            polychrome_sprites: &self.polychrome_sprites,
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces: &self.surfaces,
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
            backdrop_blurs: &self.backdrop_blurs,
            backdrop_blurs_start: 0,
            backdrop_blurs_iter: self.backdrop_blurs.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    // First, so a blur draws before anything else at its order.
    BackdropBlur,
    Shadow,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    PolychromeSprite,
    Surface,
}

struct Sig<'a, H: std::hash::Hasher>(&'a mut H);

impl<H: std::hash::Hasher> Sig<'_, H> {
    fn u(&mut self, v: u32) {
        self.0.write_u32(v);
    }
    fn u64(&mut self, v: u64) {
        self.0.write_u64(v);
    }
    fn f(&mut self, v: f32) {
        self.0.write_u32(v.to_bits());
    }
    fn fs(&mut self, vs: &[f32]) {
        for v in vs {
            self.f(*v);
        }
    }
    fn bounds(&mut self, b: &Bounds<ScaledPixels>) {
        self.fs(&[b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0]);
    }
    fn corners(&mut self, c: &Corners<ScaledPixels>) {
        self.fs(&[c.top_left.0, c.top_right.0, c.bottom_right.0, c.bottom_left.0]);
    }
    fn mask(&mut self, m: &ContentMask<ScaledPixels>) {
        self.bounds(&m.bounds);
        self.corners(&m.corner_radii);
    }
    fn hsla(&mut self, c: &Hsla) {
        self.fs(&[c.h, c.s, c.l, c.a]);
    }
    fn background(&mut self, b: &Background) {
        self.u(b.tag as u32);
        self.u(b.color_space as u32);
        self.hsla(&b.solid);
        self.f(b.gradient_angle_or_pattern_height);
        for stop in &b.colors {
            self.hsla(&stop.color);
            self.f(stop.percentage);
        }
    }
    fn tile(&mut self, t: &AtlasTile) {
        self.u(t.texture_id.index);
        self.u(t.texture_id.kind as u32);
        self.u(t.tile_id.0);
        let b = &t.bounds;
        for v in [b.origin.x, b.origin.y, b.size.width, b.size.height] {
            self.u(i32::from(v) as u32);
        }
    }
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
    PushClip(ContentMask<ScaledPixels>),
    PopClip,
}

fn intersect_scaled_masks(
    a: &ContentMask<ScaledPixels>,
    b: &ContentMask<ScaledPixels>,
) -> ContentMask<ScaledPixels> {
    let unscaled = |m: &ContentMask<ScaledPixels>| ContentMask {
        bounds: m.bounds.map(|v| crate::px(v.0)),
        corner_radii: m.corner_radii.map(|v| crate::px(v.0)),
    };
    let mask = unscaled(a).intersect(&unscaled(b));
    ContentMask {
        bounds: mask.bounds.map(|v| ScaledPixels(v.0)),
        corner_radii: mask.corner_radii.map(|v| ScaledPixels(v.0)),
    }
}

fn moved(bounds: Bounds<ScaledPixels>, delta: Point<ScaledPixels>) -> Bounds<ScaledPixels> {
    Bounds { origin: bounds.origin + delta, size: bounds.size }
}

fn translated_mask(mask: &ContentMask<ScaledPixels>, delta: Point<ScaledPixels>) -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds: moved(mask.bounds, delta),
        corner_radii: mask.corner_radii.clone(),
    }
}

#[derive(Clone)]
pub(crate) enum Primitive {
    Shadow(Shadow),
    Quad(Quad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
    BackdropBlur(BackdropBlur),
}

impl Primitive {
    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
            Primitive::BackdropBlur(blur) => &blur.bounds,
        }
    }

    fn set_content_mask(&mut self, mask: ContentMask<ScaledPixels>) {
        match self {
            Primitive::Shadow(shadow) => shadow.content_mask = mask,
            Primitive::Quad(quad) => quad.content_mask = mask,
            Primitive::Path(path) => {
                for vertex in &mut path.vertices {
                    vertex.content_mask = mask.clone();
                }
                path.content_mask = mask;
            }
            Primitive::Underline(underline) => underline.content_mask = mask,
            Primitive::MonochromeSprite(sprite) => sprite.content_mask = mask,
            Primitive::PolychromeSprite(sprite) => sprite.content_mask = mask,
            Primitive::Surface(surface) => surface.content_mask = mask,
            Primitive::BackdropBlur(blur) => blur.content_mask = mask,
        }
    }

    fn translate(&mut self, d: Point<ScaledPixels>) {
        let mask = translated_mask(self.content_mask(), d);
        match self {
            Primitive::Shadow(shadow) => shadow.bounds = moved(shadow.bounds, d),
            Primitive::Quad(quad) => quad.bounds = moved(quad.bounds, d),
            Primitive::Path(path) => {
                path.bounds = moved(path.bounds, d);
                for vertex in &mut path.vertices {
                    vertex.xy_position = vertex.xy_position + d;
                }
            }
            Primitive::Underline(underline) => underline.bounds = moved(underline.bounds, d),
            Primitive::MonochromeSprite(sprite) => {
                sprite.bounds = moved(sprite.bounds, d);
                // The matrix works in window coordinates (it rotates about the sprite's
                // center), so conjugate it with the move: t' = t + d - R·d.
                let m = &mut sprite.transformation;
                let r = m.rotation_scale;
                m.translation[0] += d.x.0 - (r[0][0] * d.x.0 + r[0][1] * d.y.0);
                m.translation[1] += d.y.0 - (r[1][0] * d.x.0 + r[1][1] * d.y.0);
            }
            Primitive::PolychromeSprite(sprite) => sprite.bounds = moved(sprite.bounds, d),
            Primitive::Surface(surface) => surface.bounds = moved(surface.bounds, d),
            Primitive::BackdropBlur(blur) => {
                blur.bounds = moved(blur.bounds, d);
                blur.shape = moved(blur.shape, d);
            }
        }
        self.set_content_mask(mask);
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
            Primitive::BackdropBlur(blur) => &blur.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows: &'a [Shadow],
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    quads: &'a [Quad],
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths: &'a [Path<ScaledPixels>],
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines: &'a [Underline],
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites: &'a [MonochromeSprite],
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    polychrome_sprites: &'a [PolychromeSprite],
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces: &'a [PaintSurface],
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
    backdrop_blurs: &'a [BackdropBlur],
    backdrop_blurs_start: usize,
    backdrop_blurs_iter: Peekable<slice::Iter<'a, BackdropBlur>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
            (
                self.backdrop_blurs_iter.peek().map(|b| b.order),
                PrimitiveKind::BackdropBlur,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(
                    &self.shadows[shadows_start..shadows_end],
                ))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(&self.quads[quads_start..quads_end]))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(&self.paths[paths_start..paths_end]))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(
                    &self.underlines[underlines_start..underlines_end],
                ))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites: &self.monochrome_sprites[sprites_start..sprites_end],
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = self.polychrome_sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites: &self.polychrome_sprites[sprites_start..sprites_end],
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(
                    &self.surfaces[surfaces_start..surfaces_end],
                ))
            }
            PrimitiveKind::BackdropBlur => {
                let start = self.backdrop_blurs_start;
                let mut end = start + 1;
                self.backdrop_blurs_iter.next();
                while self
                    .backdrop_blurs_iter
                    .next_if(|blur| (blur.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    end += 1;
                }
                self.backdrop_blurs_start = end;
                Some(PrimitiveBatch::BackdropBlurs(&self.backdrop_blurs[start..end]))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveBatch<'a> {
    Shadows(&'a [Shadow]),
    Quads(&'a [Quad]),
    Paths(&'a [Path<ScaledPixels>]),
    Underlines(&'a [Underline]),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        sprites: &'a [MonochromeSprite],
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        sprites: &'a [PolychromeSprite],
    },
    Surfaces(&'a [PaintSurface]),
    BackdropBlurs(&'a [BackdropBlur]),
}

#[derive(Default, Debug, Clone)]
#[repr(C)]
pub(crate) struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: u32,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

/// Blurs what's already drawn behind `shape`, like a frosted-glass backdrop. Only the Metal
/// renderer draws it; the others skip it.
#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct BackdropBlur {
    pub order: DrawOrder,
    /// The area the blur reads: `shape` plus room for the blur to spread in from around it.
    /// It decides draw order, so everything under that area is drawn first.
    pub bounds: Bounds<ScaledPixels>,
    pub shape: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    /// Gaussian standard deviation.
    pub radius: ScaledPixels,
    /// 1 keeps the colors; Apple's materials use about 1.8.
    pub saturation: f32,
    /// Multiplies the blurred colors.
    pub brightness: f32,
    /// Drawn over the blur inside the shape, below the rim.
    pub tint: Hsla,
    /// Strength of the specular rim and the shade on the far edge; 0 draws neither.
    pub rim: f32,
    pub rim_width: ScaledPixels,
    /// Width of the band along the edge where the backdrop bends inward; 0 for none.
    pub refraction: ScaledPixels,
    pub opacity: f32,
}

impl From<BackdropBlur> for Primitive {
    fn from(blur: BackdropBlur) -> Self {
        Primitive::BackdropBlur(blur)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(mut self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub grayscale: bool,
    /// The texture is in Display P3 rather than sRGB.
    pub display_p3: bool,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
    /// The part of the texture to draw, as fractions of its width and height. Sampling a
    /// sub-rectangle pans or zooms an image while its bounds, and so its rounded corners,
    /// stay put.
    pub texture_crop: Bounds<ScaledPixels>,
    /// A slow zoom and pan the shader animates over time; overrides `texture_crop`.
    pub motion: SpriteMotion,
}

/// Parameters of a sprite's animated pan and zoom (see `StyledImage::motion`). A `strength`
/// of 0 means no motion.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct SpriteMotion {
    /// Offset into the motion's cycle, in radians.
    pub phase: f32,
    /// How far it zooms in; 0.1 shows about 90% of the image.
    pub strength: f32,
}

impl PolychromeSprite {
    /// A `texture_crop` covering the whole texture.
    pub const WHOLE_TEXTURE: Bounds<ScaledPixels> = Bounds {
        origin: Point { x: ScaledPixels(0.), y: ScaledPixels(0.) },
        size: Size { width: ScaledPixels(1.), height: ScaledPixels(1.) },
    };
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(target_os = "macos")]
    pub image_buffer: core_video::pixel_buffer::CVPixelBuffer,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PathId(pub(crate) usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub(crate) id: PathId,
    pub(crate) order: DrawOrder,
    pub(crate) bounds: Bounds<P>,
    pub(crate) content_mask: ContentMask<P>,
    pub(crate) vertices: Vec<PathVertex<P>>,
    pub(crate) color: Background,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    pub(crate) fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub(crate) xy_position: Point<P>,
    pub(crate) st_position: Point<f32>,
    pub(crate) content_mask: ContentMask<P>,
}

impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}

#[cfg(test)]
mod backdrop_signature_tests {
    use super::*;
    use crate::{TileId, bounds, point, size};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
        bounds(point(ScaledPixels(x), ScaledPixels(y)), size(ScaledPixels(w), ScaledPixels(h)))
    }

    fn quad(b: Bounds<ScaledPixels>, color: Hsla) -> Quad {
        Quad {
            bounds: b,
            content_mask: ContentMask { bounds: rect(0., 0., 1000., 1000.), ..Default::default() },
            background: color.into(),
            ..Default::default()
        }
    }

    fn blur(shape: Bounds<ScaledPixels>) -> BackdropBlur {
        BackdropBlur {
            order: 0,
            bounds: shape.dilate(ScaledPixels(30.)),
            shape,
            corner_radii: Corners::default(),
            content_mask: ContentMask { bounds: rect(0., 0., 1000., 1000.), ..Default::default() },
            radius: ScaledPixels(10.),
            saturation: 1.6,
            brightness: 1.,
            tint: Hsla::default(),
            rim: 1.,
            rim_width: ScaledPixels(2.),
            refraction: ScaledPixels(12.),
            opacity: 1.,
        }
    }

    fn signatures(build: impl FnOnce(&mut Scene)) -> Vec<Option<u64>> {
        let mut scene = Scene::default();
        build(&mut scene);
        scene.finish();
        scene.backdrop_signatures()
    }

    #[test]
    fn only_what_is_under_a_blur_changes_its_signature() {
        let bar = rect(100., 100., 200., 50.);
        let base = |color: Hsla, far: Hsla, above: Hsla| {
            signatures(|scene| {
                scene.insert_primitive(quad(rect(0., 0., 400., 400.), color));
                scene.insert_primitive(quad(rect(800., 800., 50., 50.), far));
                scene.insert_primitive(blur(bar));
                scene.insert_primitive(quad(bar, above));
            })
        };
        let a = base(crate::red(), crate::blue(), crate::white());
        assert!(a[0].is_some());
        assert_eq!(a, base(crate::red(), crate::blue(), crate::white()));
        assert_eq!(a, base(crate::red(), crate::green(), crate::black()));
        assert_ne!(a, base(crate::yellow(), crate::blue(), crate::white()));
    }

    #[test]
    fn moved_backdrops_change_the_signature() {
        let bar = rect(100., 100., 200., 50.);
        let at = |x: f32| {
            signatures(|scene| {
                scene.insert_primitive(quad(rect(x, 90., 40., 40.), crate::red()));
                scene.insert_primitive(blur(bar));
            })
        };
        assert_ne!(at(120.), at(140.));
    }

    #[test]
    fn moving_sprites_and_nested_blurs() {
        let bar = rect(100., 100., 200., 50.);
        let sprite = |motion: f32| PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false,
            opacity: 1.,
            bounds: rect(120., 90., 60., 60.),
            content_mask: ContentMask { bounds: rect(0., 0., 1000., 1000.), ..Default::default() },
            corner_radii: Corners::default(),
            tile: AtlasTile {
                texture_id: AtlasTextureId { index: 0, kind: crate::AtlasTextureKind::Polychrome },
                tile_id: TileId(1),
                padding: 0,
                bounds: Default::default(),
            },
            texture_crop: PolychromeSprite::WHOLE_TEXTURE,
            motion: SpriteMotion { phase: 0., strength: motion },
            display_p3: false,
        };
        let with = |motion: f32| {
            signatures(|scene| {
                scene.insert_primitive(sprite(motion));
                scene.insert_primitive(blur(bar));
                scene.insert_primitive(blur(rect(150., 120., 60., 60.)));
            })
        };
        let still = with(0.);
        assert!(still.iter().all(Option::is_some));
        assert_eq!(with(0.1), vec![None, None], "a blur over a moving one changes with it");
    }
}
