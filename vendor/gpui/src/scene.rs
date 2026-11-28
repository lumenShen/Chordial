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
    pub(crate) shadows: Vec<Shadow>,
    pub(crate) quads: Vec<Quad>,
    pub(crate) paths: Vec<Path<ScaledPixels>>,
    pub(crate) underlines: Vec<Underline>,
    pub(crate) monochrome_sprites: Vec<MonochromeSprite>,
    pub(crate) polychrome_sprites: Vec<PolychromeSprite>,
    pub(crate) surfaces: Vec<PaintSurface>,
}

/// Applies a uniform scale, translation, opacity, and small blur kernel to a
/// subtree's paint primitives.
///
/// This is a scene-level filter: it does not introduce a render target or a
/// platform-specific shader path. Blur is approximated by replaying the
/// subtree's primitives at eight low-alpha offsets. Hitboxes are transformed
/// separately during prepaint so interaction stays aligned with the visual.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintFilter {
    /// Opacity multiplier applied to the subtree.
    pub opacity: f32,
    /// Uniform scale applied around `origin`.
    pub scale: f32,
    /// Scale anchor in logical window pixels.
    pub origin: Point<Pixels>,
    /// Blur radius (standard deviation) in logical pixels.
    pub blur_radius: Pixels,
}

impl Default for PaintFilter {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            scale: 1.0,
            origin: Point::default(),
            blur_radius: Pixels::ZERO,
        }
    }
}

impl PaintFilter {
    fn is_identity(self) -> bool {
        (self.opacity - 1.0).abs() < f32::EPSILON
            && (self.scale - 1.0).abs() < f32::EPSILON
            && self.blur_radius.0.abs() < f32::EPSILON
    }

    pub(crate) fn transform_point(self, point: Point<Pixels>) -> Point<Pixels> {
        let dx = point.x.0 - self.origin.x.0;
        let dy = point.y.0 - self.origin.y.0;
        Point::new(
            (self.origin.x.0 + dx * self.scale).into(),
            (self.origin.y.0 + dy * self.scale).into(),
        )
    }

    pub(crate) fn transform_bounds(self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        Bounds {
            origin: self.transform_point(bounds.origin),
            size: crate::size(
                (bounds.size.width.0 * self.scale).into(),
                (bounds.size.height.0 * self.scale).into(),
            ),
        }
    }
}

/// Snapshot of scene-vector lengths before a filtered subtree paints.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SceneCheckpoint {
    paint_operations: usize,
    shadows: usize,
    quads: usize,
    paths: usize,
    underlines: usize,
    monochrome_sprites: usize,
    polychrome_sprites: usize,
    surfaces: usize,
}

#[derive(Clone, Copy)]
struct PrimitiveFilter {
    scale: f32,
    origin: Point<ScaledPixels>,
    opacity: f32,
    dx: f32,
    dy: f32,
}

impl PrimitiveFilter {
    fn new(filter: PaintFilter, scale_factor: f32) -> Self {
        Self {
            scale: filter.scale.clamp(0.01, 4.0),
            origin: Point::new(
                (filter.origin.x.0 * scale_factor).into(),
                (filter.origin.y.0 * scale_factor).into(),
            ),
            opacity: filter.opacity.clamp(0.0, 1.0),
            dx: 0.0,
            dy: 0.0,
        }
    }

    fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.dx = dx;
        self.dy = dy;
        self
    }

    fn point(self, point: Point<ScaledPixels>) -> Point<ScaledPixels> {
        let dx = point.x.0 - self.origin.x.0;
        let dy = point.y.0 - self.origin.y.0;
        Point::new(
            (self.origin.x.0 + dx * self.scale + self.dx).into(),
            (self.origin.y.0 + dy * self.scale + self.dy).into(),
        )
    }

    fn bounds(self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        Bounds {
            origin: self.point(bounds.origin),
            size: crate::size(
                (bounds.size.width.0 * self.scale).into(),
                (bounds.size.height.0 * self.scale).into(),
            ),
        }
    }

    fn corners(self, corners: Corners<ScaledPixels>) -> Corners<ScaledPixels> {
        corners.map(|value| (value.0 * self.scale).into())
    }

    fn edges(self, edges: Edges<ScaledPixels>) -> Edges<ScaledPixels> {
        edges.map(|value| (value.0 * self.scale).into())
    }
}

impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub(crate) fn checkpoint(&self) -> SceneCheckpoint {
        SceneCheckpoint {
            paint_operations: self.paint_operations.len(),
            shadows: self.shadows.len(),
            quads: self.quads.len(),
            paths: self.paths.len(),
            underlines: self.underlines.len(),
            monochrome_sprites: self.monochrome_sprites.len(),
            polychrome_sprites: self.polychrome_sprites.len(),
            surfaces: self.surfaces.len(),
        }
    }

    /// Transform the primitives added since `checkpoint`, then optionally
    /// append a low-alpha multi-sample blur pass. The original primitives stay
    /// in place so hit testing and the sharp centre remain crisp.
    pub(crate) fn apply_filter(
        &mut self,
        checkpoint: SceneCheckpoint,
        filter: PaintFilter,
        scale_factor: f32,
    ) {
        if filter.is_identity() {
            return;
        }
        let blur_radius = filter.blur_radius.0;
        let filter = PrimitiveFilter::new(filter, scale_factor);

        for shadow in &mut self.shadows[checkpoint.shadows..] {
            filter_shadow(shadow, filter, 1.0);
        }
        for quad in &mut self.quads[checkpoint.quads..] {
            filter_quad(quad, filter, 1.0);
        }
        for path in &mut self.paths[checkpoint.paths..] {
            filter_path(path, filter, 1.0);
        }
        for underline in &mut self.underlines[checkpoint.underlines..] {
            filter_underline(underline, filter, 1.0);
        }
        for sprite in &mut self.monochrome_sprites[checkpoint.monochrome_sprites..] {
            filter_monochrome_sprite(sprite, filter, 1.0);
        }
        for sprite in &mut self.polychrome_sprites[checkpoint.polychrome_sprites..] {
            filter_polychrome_sprite(sprite, filter, 1.0);
        }
        for surface in &mut self.surfaces[checkpoint.surfaces..] {
            surface.bounds = filter.bounds(surface.bounds);
        }

        let operations = &mut self.paint_operations[checkpoint.paint_operations..];
        for operation in operations.iter_mut() {
            filter_operation(operation, filter);
        }

        let radius = blur_radius * scale_factor.max(1.0);
        if radius <= 0.01 || filter.opacity <= 0.001 {
            return;
        }
        let samples = operations.to_vec();
        // Eight offsets around the centre. Total sample alpha is kept well
        // below the sharp copy so the group softens without visibly doubling.
        const DIRECTIONS: [(f32, f32); 8] = [
            (1.0, 0.0),
            (0.707_106_8, 0.707_106_8),
            (0.0, 1.0),
            (-0.707_106_8, 0.707_106_8),
            (-1.0, 0.0),
            (-0.707_106_8, -0.707_106_8),
            (0.0, -1.0),
            (0.707_106_8, -0.707_106_8),
        ];
        const SAMPLE_ALPHA: f32 = 0.04;
        let distance = radius * 0.75;
        for (dx, dy) in DIRECTIONS {
            let sample = PrimitiveFilter {
                scale: 1.0,
                origin: Point::default(),
                opacity: 1.0,
                dx: dx * distance,
                dy: dy * distance,
            };
            self.replay_filtered_operations(&samples, sample, SAMPLE_ALPHA);
        }
    }

    fn replay_filtered_operations(
        &mut self,
        operations: &[PaintOperation],
        filter: PrimitiveFilter,
        opacity: f32,
    ) {
        for operation in operations {
            match operation {
                PaintOperation::Primitive(primitive) => match primitive {
                    // Shadows already carry a blurred kernel, and surfaces are
                    // platform-owned textures. Duplicating either is expensive
                    // and visually muddy; transform the original only.
                    Primitive::Shadow(_) | Primitive::Surface(_) => {}
                    _ => {
                        let mut primitive = primitive.clone();
                        filter_primitive(&mut primitive, filter, opacity);
                        self.insert_primitive(primitive);
                    }
                },
                PaintOperation::StartLayer(bounds) => {
                    self.push_layer(filter.bounds(*bounds));
                }
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = self.primitive_bounds.insert(bounds);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
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
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.order = order;
                self.shadows.push(shadow.clone());
            }
            Primitive::Quad(quad) => {
                quad.order = order;
                self.quads.push(quad.clone());
            }
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path.clone());
            }
            Primitive::Underline(underline) => {
                underline.order = order;
                self.underlines.push(underline.clone());
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(sprite.clone());
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.order = order;
                self.polychrome_sprites.push(sprite.clone());
            }
            Primitive::Surface(surface) => {
                surface.order = order;
                self.surfaces.push(surface.clone());
            }
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
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
        }
    }
}

fn filter_operation(operation: &mut PaintOperation, filter: PrimitiveFilter) {
    match operation {
        PaintOperation::Primitive(primitive) => filter_primitive(primitive, filter, 1.0),
        PaintOperation::StartLayer(bounds) => *bounds = filter.bounds(*bounds),
        PaintOperation::EndLayer => {}
    }
}

fn filter_primitive(primitive: &mut Primitive, filter: PrimitiveFilter, opacity: f32) {
    match primitive {
        Primitive::Shadow(shadow) => filter_shadow(shadow, filter, opacity),
        Primitive::Quad(quad) => filter_quad(quad, filter, opacity),
        Primitive::Path(path) => filter_path(path, filter, opacity),
        Primitive::Underline(underline) => filter_underline(underline, filter, opacity),
        Primitive::MonochromeSprite(sprite) => filter_monochrome_sprite(sprite, filter, opacity),
        Primitive::PolychromeSprite(sprite) => filter_polychrome_sprite(sprite, filter, opacity),
        Primitive::Surface(surface) => surface.bounds = filter.bounds(surface.bounds),
    }
}

fn filter_shadow(shadow: &mut Shadow, filter: PrimitiveFilter, opacity: f32) {
    shadow.bounds = filter.bounds(shadow.bounds);
    shadow.corner_radii = filter.corners(shadow.corner_radii);
    shadow.blur_radius = (shadow.blur_radius.0 * filter.scale).into();
    shadow.color = shadow.color.opacity(filter.opacity * opacity);
}

fn filter_quad(quad: &mut Quad, filter: PrimitiveFilter, opacity: f32) {
    quad.bounds = filter.bounds(quad.bounds);
    quad.corner_radii = filter.corners(quad.corner_radii);
    quad.border_widths = filter.edges(quad.border_widths);
    quad.background = quad.background.opacity(filter.opacity * opacity);
    quad.border_color = quad.border_color.opacity(filter.opacity * opacity);
}

fn filter_path(path: &mut Path<ScaledPixels>, filter: PrimitiveFilter, opacity: f32) {
    path.bounds = filter.bounds(path.bounds);
    for vertex in &mut path.vertices {
        vertex.xy_position = filter.point(vertex.xy_position);
    }
    path.start = filter.point(path.start);
    path.current = filter.point(path.current);
    path.color = path.color.opacity(filter.opacity * opacity);
}

fn filter_underline(underline: &mut Underline, filter: PrimitiveFilter, opacity: f32) {
    underline.bounds = filter.bounds(underline.bounds);
    underline.thickness = (underline.thickness.0 * filter.scale).into();
    underline.color = underline.color.opacity(filter.opacity * opacity);
}

fn filter_monochrome_sprite(sprite: &mut MonochromeSprite, filter: PrimitiveFilter, opacity: f32) {
    sprite.bounds = filter.bounds(sprite.bounds);
    sprite.color = sprite.color.opacity(filter.opacity * opacity);
    if sprite.mask != SpriteMask::NONE {
        let light = filter.point(point(sprite.mask.light_from.into(), 0.0.into()));
        let dark = filter.point(point(sprite.mask.dark_from.into(), 0.0.into()));
        sprite.mask.light_from = light.x.0;
        sprite.mask.dark_from = dark.x.0;
    }
}

fn filter_polychrome_sprite(sprite: &mut PolychromeSprite, filter: PrimitiveFilter, opacity: f32) {
    sprite.bounds = filter.bounds(sprite.bounds);
    sprite.corner_radii = filter.corners(sprite.corner_radii);
    sprite.opacity *= filter.opacity * opacity;
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
    Shadow,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    PolychromeSprite,
    Surface,
}

#[derive(Clone)]
pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
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
        }
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

/// 单色字形 sprite 的水平渐变遮罩（AMLL `mask-image` 扫光的等价物）。
///
/// 逐 fragment 计算：
/// `alpha *= mix(light, dark, saturate((x - light_from) / (dark_from - light_from)))`
/// 其中 `x` 是设备像素坐标。`light_from < dark_from` 时左侧亮、右侧暗，中间是
/// 线性渐变；`light == dark` 时退化为整块 alpha（AMLL 非活跃行的均匀暗色）。
/// 四个字段依次对应 shader 里的 `SpriteMask`。
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct SpriteMask {
    /// X coordinate (in device pixels) where the mask is fully `light`.
    pub light_from: f32,
    /// X coordinate (in device pixels) where the mask is fully `dark`.
    pub dark_from: f32,
    /// Alpha multiplier to the left of the transition.
    pub light: f32,
    /// Alpha multiplier to the right of the transition.
    pub dark: f32,
}

impl SpriteMask {
    /// A mask that leaves the glyph alpha unchanged.
    pub const NONE: Self = Self {
        light_from: 0.0,
        dark_from: 0.0,
        light: 1.0,
        dark: 1.0,
    };

    /// Creates a horizontal alpha gradient: fully `light` at and left of
    /// `light_from`, fully `dark` at and right of `dark_from`, linear between.
    pub fn new(light_from: f32, dark_from: f32, light: f32, dark: f32) -> Self {
        Self {
            light_from,
            dark_from,
            light,
            dark,
        }
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
    pub mask: SpriteMask,
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
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
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
    pub(crate) start: Point<P>,
    pub(crate) current: Point<P>,
    pub(crate) contour_count: usize,
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

    /// Translate this path by the given offset.
    ///
    /// Used by cached icon geometry: the path is built once in local
    /// coordinates and only shifted to the element bounds each frame.
    pub fn translated(&self, by: Point<Pixels>) -> Path<Pixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: Bounds {
                origin: self.bounds.origin + by,
                size: self.bounds.size,
            },
            content_mask: self.content_mask.clone(),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| PathVertex {
                    xy_position: vertex.xy_position + by,
                    st_position: vertex.st_position,
                    content_mask: vertex.content_mask.clone(),
                })
                .collect(),
            start: self.start + by,
            current: self.current + by,
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
mod filter_tests {
    use super::*;
    use crate::{point, px, size};

    fn scaled(value: f32) -> ScaledPixels {
        value.into()
    }

    fn test_quad(x: f32, y: f32, width: f32, height: f32) -> Quad {
        Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: Bounds {
                origin: point(scaled(x), scaled(y)),
                size: size(scaled(width), scaled(height)),
            },
            content_mask: ContentMask {
                bounds: Bounds {
                    origin: point(scaled(-1000.0), scaled(-1000.0)),
                    size: size(scaled(2000.0), scaled(2000.0)),
                },
            },
            background: crate::solid_background(crate::white()),
            border_color: crate::white(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        }
    }

    fn filter(scale: f32, opacity: f32, blur: f32) -> PaintFilter {
        PaintFilter {
            opacity,
            scale,
            origin: point(px(110.0), px(20.0)),
            blur_radius: px(blur),
        }
    }

    #[test]
    fn scale_keeps_the_anchor_fixed() {
        let mut scene = Scene::default();
        let checkpoint = scene.checkpoint();
        scene.insert_primitive(test_quad(10.0, 20.0, 100.0, 50.0));
        scene.apply_filter(checkpoint, filter(0.5, 1.0, 0.0), 1.0);

        let bounds = scene.quads[0].bounds;
        assert_eq!(bounds.origin.x.0, 60.0);
        assert_eq!(bounds.origin.y.0, 20.0);
        assert_eq!(bounds.size.width.0, 50.0);
        assert_eq!(bounds.size.height.0, 25.0);
        assert_eq!(bounds.origin.x.0 + bounds.size.width.0, 110.0);
    }

    #[test]
    fn opacity_multiplies_primitive_alpha() {
        let mut scene = Scene::default();
        let checkpoint = scene.checkpoint();
        scene.insert_primitive(test_quad(0.0, 0.0, 10.0, 10.0));
        scene.apply_filter(checkpoint, filter(1.0, 0.5, 0.0), 1.0);

        assert!((scene.quads[0].background.solid.a - 0.5).abs() < 1e-6);
        assert!((scene.quads[0].border_color.a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn zero_blur_does_not_add_samples() {
        let mut scene = Scene::default();
        let checkpoint = scene.checkpoint();
        scene.insert_primitive(test_quad(0.0, 0.0, 10.0, 10.0));
        scene.apply_filter(checkpoint, filter(1.0, 0.8, 0.0), 1.0);

        assert_eq!(scene.paint_operations.len(), 1);
        assert_eq!(scene.quads.len(), 1);
    }

    #[test]
    fn blur_adds_eight_low_alpha_samples() {
        let mut scene = Scene::default();
        let checkpoint = scene.checkpoint();
        scene.insert_primitive(test_quad(0.0, 0.0, 10.0, 10.0));
        scene.apply_filter(checkpoint, filter(1.0, 0.5, 2.0), 1.0);

        assert_eq!(scene.paint_operations.len(), 9);
        assert_eq!(scene.quads.len(), 9);
        for sample in &scene.quads[1..] {
            assert!((sample.background.solid.a - 0.02).abs() < 1e-6);
        }
    }

    #[test]
    fn blur_replays_layers_in_balanced_pairs() {
        let mut scene = Scene::default();
        let checkpoint = scene.checkpoint();
        scene.push_layer(Bounds {
            origin: point(scaled(0.0), scaled(0.0)),
            size: size(scaled(10.0), scaled(10.0)),
        });
        scene.insert_primitive(test_quad(0.0, 0.0, 10.0, 10.0));
        scene.pop_layer();
        scene.apply_filter(checkpoint, filter(1.0, 0.5, 2.0), 1.0);

        let starts = scene
            .paint_operations
            .iter()
            .filter(|operation| matches!(operation, PaintOperation::StartLayer(_)))
            .count();
        let ends = scene
            .paint_operations
            .iter()
            .filter(|operation| matches!(operation, PaintOperation::EndLayer))
            .count();
        assert_eq!(starts, 9);
        assert_eq!(starts, ends);
    }
}
