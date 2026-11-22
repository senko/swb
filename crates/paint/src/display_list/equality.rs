//! Equality of display items, for the damage of a frame (`crate::damage`,
//! ADR 0044 section 9): two equal items draw the same pixels in the same
//! state. Shared values (`Arc`) compare by pointer first, then by value; a
//! value without an equality (none at the moment) would count as
//! different, which costs damage, never a stale pixel.

use std::sync::Arc;

use super::items::DisplayItem;

/// True if `a` and `b` point to the same value or hold equal values.
fn same<T: PartialEq + ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

impl PartialEq for DisplayItem {
    #[allow(clippy::too_many_lines)] // One arm per item.
    fn eq(&self, other: &DisplayItem) -> bool {
        use DisplayItem as D;
        match (self, other) {
            (
                D::Rect { rect, radii, color },
                D::Rect {
                    rect: r2,
                    radii: q2,
                    color: c2,
                },
            ) => rect == r2 && radii == q2 && color == c2,
            (
                D::Border {
                    rect,
                    widths,
                    colors,
                    styles,
                    radii,
                },
                D::Border {
                    rect: r2,
                    widths: w2,
                    colors: c2,
                    styles: s2,
                    radii: q2,
                },
            ) => rect == r2 && widths == w2 && colors == c2 && styles == s2 && radii == q2,
            (
                D::Text {
                    origin,
                    font,
                    size,
                    glyphs,
                    color,
                },
                D::Text {
                    origin: o2,
                    font: f2,
                    size: s2,
                    glyphs: g2,
                    color: c2,
                },
            ) => origin == o2 && font == f2 && size == s2 && color == c2 && same(glyphs, g2),
            (
                D::TextStroke {
                    origin,
                    font,
                    size,
                    glyphs,
                    width,
                    color,
                },
                D::TextStroke {
                    origin: o2,
                    font: f2,
                    size: s2,
                    glyphs: g2,
                    width: w2,
                    color: c2,
                },
            ) => {
                origin == o2
                    && font == f2
                    && size == s2
                    && width == w2
                    && color == c2
                    && same(glyphs, g2)
            }
            (
                D::Image {
                    image,
                    rect,
                    tile,
                    clip,
                },
                D::Image {
                    image: i2,
                    rect: r2,
                    tile: t2,
                    clip: c2,
                },
            ) => image == i2 && rect == r2 && tile == t2 && clip == c2,
            (
                D::LinearGradient {
                    rect,
                    tile,
                    clip,
                    gradient,
                    current_color,
                },
                D::LinearGradient {
                    rect: r2,
                    tile: t2,
                    clip: c2,
                    gradient: g2,
                    current_color: k2,
                },
            ) => {
                rect == r2 && tile == t2 && clip == c2 && current_color == k2 && same(gradient, g2)
            }
            (D::PushClip(a), D::PushClip(b)) => a == b,
            (
                D::PushOpacity {
                    opacity,
                    blend,
                    bounds,
                    fixed_bounds,
                    escapes_clips,
                },
                D::PushOpacity {
                    opacity: o2,
                    blend: b2,
                    bounds: r2,
                    fixed_bounds: f2,
                    escapes_clips: e2,
                },
            ) => {
                opacity == o2
                    && blend == b2
                    && bounds == r2
                    && fixed_bounds == f2
                    && escapes_clips == e2
            }
            (
                D::PushFilter {
                    filter,
                    current_color,
                    bounds,
                    fixed_bounds,
                    escapes_clips,
                },
                D::PushFilter {
                    filter: l2,
                    current_color: c2,
                    bounds: r2,
                    fixed_bounds: f2,
                    escapes_clips: e2,
                },
            ) => {
                current_color == c2
                    && bounds == r2
                    && fixed_bounds == f2
                    && escapes_clips == e2
                    && same(filter, l2)
            }
            (
                D::BackdropFilter {
                    filter,
                    current_color,
                    rect,
                    radii,
                    alpha,
                    layers,
                },
                D::BackdropFilter {
                    filter: l2,
                    current_color: c2,
                    rect: r2,
                    radii: q2,
                    alpha: a2,
                    layers: n2,
                },
            ) => {
                current_color == c2
                    && rect == r2
                    && radii == q2
                    && alpha == a2
                    && layers == n2
                    && same(filter, l2)
            }
            (
                D::PushMask { bounds, layers },
                D::PushMask {
                    bounds: b2,
                    layers: l2,
                },
            ) => bounds == b2 && same(layers, l2),
            (
                D::PushSvgClip {
                    clip,
                    transform,
                    bounds,
                    rect_fallback,
                    backdrop_root,
                },
                D::PushSvgClip {
                    clip: c2,
                    transform: t2,
                    bounds: b2,
                    rect_fallback: f2,
                    backdrop_root: r2,
                },
            ) => {
                transform == t2
                    && bounds == b2
                    && rect_fallback == f2
                    && backdrop_root == r2
                    && same(clip, c2)
            }
            (
                D::PushTransform {
                    transform,
                    bounds,
                    hit_bounds,
                    fixed_bounds,
                },
                D::PushTransform {
                    transform: t2,
                    bounds: b2,
                    hit_bounds: h2,
                    fixed_bounds: f2,
                },
            ) => transform == t2 && bounds == b2 && hit_bounds == h2 && fixed_bounds == f2,
            (
                D::Polyline {
                    points,
                    width,
                    color,
                },
                D::Polyline {
                    points: p2,
                    width: w2,
                    color: c2,
                },
            ) => width == w2 && color == c2 && same(points, p2),
            (
                D::Polygon { points, color },
                D::Polygon {
                    points: p2,
                    color: c2,
                },
            ) => color == c2 && same(points, p2),
            (
                D::FillPath {
                    path,
                    transform,
                    color,
                    rule,
                    anti_alias,
                },
                D::FillPath {
                    path: p2,
                    transform: t2,
                    color: c2,
                    rule: r2,
                    anti_alias: a2,
                },
            ) => transform == t2 && color == c2 && rule == r2 && anti_alias == a2 && same(path, p2),
            (
                D::StrokePath {
                    path,
                    transform,
                    color,
                    stroke,
                    anti_alias,
                },
                D::StrokePath {
                    path: p2,
                    transform: t2,
                    color: c2,
                    stroke: s2,
                    anti_alias: a2,
                },
            ) => {
                transform == t2
                    && color == c2
                    && anti_alias == a2
                    && same(stroke, s2)
                    && same(path, p2)
            }
            (
                D::HitShape {
                    node,
                    path,
                    transform,
                    fill,
                    stroke_width,
                },
                D::HitShape {
                    node: n2,
                    path: p2,
                    transform: t2,
                    fill: f2,
                    stroke_width: w2,
                },
            ) => {
                node == n2 && transform == t2 && fill == f2 && stroke_width == w2 && same(path, p2)
            }
            (D::HitRegion { rect, node }, D::HitRegion { rect: r2, node: n2 }) => {
                rect == r2 && node == n2
            }
            (
                D::Frame {
                    node,
                    rect,
                    interactive,
                },
                D::Frame {
                    node: n2,
                    rect: r2,
                    interactive: i2,
                },
            ) => node == n2 && rect == r2 && interactive == i2,
            (D::PushViewportClip, D::PushViewportClip)
            | (D::PopClip, D::PopClip)
            | (D::PopOpacity, D::PopOpacity)
            | (D::PopFilter, D::PopFilter)
            | (D::PopMask, D::PopMask)
            | (D::PopSvgClip, D::PopSvgClip)
            | (D::PopTransform, D::PopTransform) => true,
            _ => false,
        }
    }
}
