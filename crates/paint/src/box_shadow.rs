//! `box-shadow` (CSS Backgrounds 3 §7,
//! <https://www.w3.org/TR/css-backgrounds-3/#box-shadow>) as display items,
//! built from the groups that already exist: a blur is a filter group
//! (`blur()` of half the blur radius, the Gaussian's standard deviation),
//! a clip is an SVG clip group (ADR 0023). ADR 0045 explains the choice.
//!
//! - An outer shadow is the rounded rectangle of the border box moved by
//!   the offsets and grown by the spread, blurred, and clipped to what lies
//!   outside the border box (a clip path of both shapes with the even-odd
//!   rule). Without a blur, a shadow that surrounds the border box (the
//!   offsets are 0, the spread is not negative; the usual ring) is one
//!   even-odd path without a group. With a `corner-shape` the shadow and
//!   the hole are the path of the box ([`crate::corner_shape`]). If the
//!   box has an opaque background (which covers the border box) and no
//!   shaped corner, the clip is left out.
//! - An inner shadow is the part of the padding box outside the "hole":
//!   the padding box moved by the offsets and shrunk by the spread. It is
//!   drawn as the even-odd path of a large rectangle and the hole, blurred,
//!   inside a clip to the padding box.
//!
//! The spread changes the radii as the specification says: a positive
//! spread adds to a radius, less if the radius is smaller than the spread
//! (the factor `1 + (r / spread - 1)^3`), a negative spread subtracts, to
//! a minimum of 0; a square corner stays square.
//!
//! At most [`MAX_SHADOWS`] shadows of a box paint (the first ones of the
//! list, which are on top); Chromium paints all of them, and the cost of a
//! list of thousands of shadows has no use.

use std::sync::Arc;

use swb_layout::svg::{PathSegment, SvgPath};
use swb_layout::{Matrix, Point, Rect, has_rounded_corner, rounded_path};
use swb_style::{BoxShadow, FillRule, FilterFunction, Rgba};

use crate::corner_shape;
use crate::display_list::{DisplayItem, Radii, path_clip_item};

/// The most shadows of one box that paint.
pub(crate) const MAX_SHADOWS: usize = 64;

/// The largest value of an offset, a spread or a blur radius that the
/// geometry uses, so that sums of them stay finite.
const LIMIT: f32 = 1.0e6;

/// The items of the outer shadows of a box with border box `rect` and
/// corner radii `radii`, below its background. `opaque` is true if the
/// background covers the border box with an opaque color (it is ignored
/// with `shapes`: a shaped corner leaves part of the border box
/// uncovered). `shapes` are the `corner-shape` parameters if a corner is not
/// round: the shadow and the hole have the shape of the box.
pub(crate) fn outer(
    shadows: &[BoxShadow],
    current_color: Rgba,
    rect: Rect,
    radii: Radii,
    opaque: bool,
    shapes: Option<[f32; 4]>,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    let all = &shadows[..shadows.len().min(MAX_SHADOWS)];
    for shadow in all.iter().rev().filter(|s| !s.inset) {
        outer_shadow(
            &mut items,
            shadow,
            current_color,
            rect,
            radii,
            opaque,
            shapes,
        );
    }
    items
}

/// The items of the inner shadows of a box with padding box `padding` and
/// corner radii `radii` (inside the border), above its background.
pub(crate) fn inner(
    shadows: &[BoxShadow],
    current_color: Rgba,
    padding: Rect,
    radii: Radii,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    let all = &shadows[..shadows.len().min(MAX_SHADOWS)];
    for shadow in all.iter().rev().filter(|s| s.inset) {
        inner_shadow(&mut items, shadow, current_color, padding, radii);
    }
    items
}

fn clamp(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-LIMIT, LIMIT)
    }
}

/// The radii of the shadow shape: the radii of the box grown by `spread`.
fn spread_radii(radii: Radii, spread: f32) -> Radii {
    let adjust = |r: f32| -> f32 {
        if r <= 0.0 {
            0.0
        } else if spread == 0.0 {
            r
        } else if spread > 0.0 {
            let ratio = r / spread;
            if ratio >= 1.0 {
                r + spread
            } else {
                r + spread * (1.0 + (ratio - 1.0).powi(3))
            }
        } else {
            (r + spread).max(0.0)
        }
    };
    radii.map(|(h, v)| (adjust(h), adjust(v)))
}

/// A filter group item for a blur of radius `blur` (the standard deviation
/// is half of it).
fn blur_group(blur: f32, current_color: Rgba) -> DisplayItem {
    DisplayItem::PushFilter {
        filter: Arc::from([FilterFunction::Blur(blur / 2.0)]),
        current_color,
        bounds: Rect::default(),
        fixed_bounds: None,
        escapes_clips: false,
    }
}

/// The segments of a rectangle with rounded corners (a plain rectangle if
/// no corner is rounded), or `None` for an empty one. With `shapes` the
/// corners have those `corner-shape`s.
fn shape_segments(rect: Rect, radii: Radii, shapes: Option<[f32; 4]>) -> Option<Vec<PathSegment>> {
    if !(rect.width > 0.0 && rect.height > 0.0) {
        return None;
    }
    if let Some(k) = shapes.filter(|_| has_rounded_corner(&radii)) {
        return corner_shape::path(rect, radii, k).map(|p| p.segments().to_vec());
    }
    if has_rounded_corner(&radii) {
        return rounded_path(rect, &radii).map(|p| p.segments().to_vec());
    }
    Some(rect_segments(rect))
}

fn rect_segments(r: Rect) -> Vec<PathSegment> {
    vec![
        PathSegment::MoveTo(Point::new(r.x, r.y)),
        PathSegment::LineTo(Point::new(r.right(), r.y)),
        PathSegment::LineTo(Point::new(r.right(), r.bottom())),
        PathSegment::LineTo(Point::new(r.x, r.bottom())),
        PathSegment::Close,
    ]
}

/// A path item filling `segments` with the even-odd rule.
fn even_odd(segments: &[PathSegment], color: Rgba) -> Option<DisplayItem> {
    Some(DisplayItem::FillPath {
        path: Arc::new(SvgPath::from_segments(segments)?),
        transform: Matrix::IDENTITY,
        color,
        rule: FillRule::EvenOdd,
        anti_alias: true,
    })
}

fn grown(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.width + 2.0 * by, r.height + 2.0 * by)
}

/// The fill of an outer shadow's shape: a rounded rectangle, or the path
/// of the box with its `corner-shape`s.
fn shadow_fill(rect: Rect, radii: Radii, color: Rgba, shapes: Option<[f32; 4]>) -> DisplayItem {
    shapes
        .and_then(|k| corner_shape::fill(rect, radii, k, color))
        .unwrap_or(DisplayItem::Rect { rect, radii, color })
}

fn outer_shadow(
    items: &mut Vec<DisplayItem>,
    shadow: &BoxShadow,
    current_color: Rgba,
    rect: Rect,
    radii: Radii,
    opaque: bool,
    shapes: Option<[f32; 4]>,
) {
    let color = shadow.color.resolve(current_color);
    if color.is_transparent() {
        return;
    }
    let (x, y, spread) = (clamp(shadow.x), clamp(shadow.y), clamp(shadow.spread));
    let blur = clamp(shadow.blur).max(0.0);
    let shape = grown(rect.translate(Point::new(x, y)), spread);
    if !(shape.width > 0.0 && shape.height > 0.0) {
        return;
    }
    let shape_radii = spread_radii(radii, spread);
    let surrounds = x == 0.0 && y == 0.0 && spread >= 0.0;
    if blur == 0.0 && surrounds {
        // A ring: the shadow shape without the border box.
        let (Some(mut segments), Some(hole)) = (
            shape_segments(shape, shape_radii, shapes),
            shape_segments(rect, radii, shapes),
        ) else {
            return;
        };
        segments.extend(hole);
        items.extend(even_odd(&segments, color));
        return;
    }
    let fill = shadow_fill(shape, shape_radii, color, shapes);
    let reach = 1.5 * blur + 2.0;
    let extent = grown(shape, reach).union(&grown(rect, 1.0));
    let clipped = !opaque || shapes.is_some();
    if clipped {
        let (Some(mut segments), Some(hole)) = (
            shape_segments(extent, [(0.0, 0.0); 4], None),
            shape_segments(rect, radii, shapes),
        ) else {
            return;
        };
        segments.extend(hole);
        let Some(path) = SvgPath::from_segments(&segments) else {
            return;
        };
        items.push(path_clip_item(
            path,
            FillRule::EvenOdd,
            Matrix::IDENTITY,
            extent,
            false,
        ));
    }
    if blur > 0.0 {
        items.push(blur_group(blur, current_color));
    }
    items.push(fill);
    if blur > 0.0 {
        items.push(DisplayItem::PopFilter);
    }
    if clipped {
        items.push(DisplayItem::PopSvgClip);
    }
}

fn inner_shadow(
    items: &mut Vec<DisplayItem>,
    shadow: &BoxShadow,
    current_color: Rgba,
    padding: Rect,
    radii: Radii,
) {
    let color = shadow.color.resolve(current_color);
    if color.is_transparent() || !(padding.width > 0.0 && padding.height > 0.0) {
        return;
    }
    let (x, y, spread) = (clamp(shadow.x), clamp(shadow.y), clamp(shadow.spread));
    let blur = clamp(shadow.blur).max(0.0);
    let hole = grown(padding.translate(Point::new(x, y)), -spread);
    let hole_radii = spread_radii(radii, -spread);
    let reach = 1.5 * blur + x.abs() + y.abs() + spread.abs() + 2.0;
    let area = grown(padding, reach);
    let mut segments = rect_segments(area);
    if let Some(h) = shape_segments(hole, hole_radii, None) {
        segments.extend(h);
    }
    let Some(fill) = even_odd(&segments, color) else {
        return;
    };
    // The clip to the padding box.
    let rounded = has_rounded_corner(&radii);
    let clip = if rounded {
        shape_segments(padding, radii, None)
            .and_then(|s| SvgPath::from_segments(&s))
            .map(|path| path_clip_item(path, FillRule::NonZero, Matrix::IDENTITY, padding, false))
    } else {
        Some(DisplayItem::PushClip(padding))
    };
    let Some(clip) = clip else {
        return;
    };
    items.push(clip);
    if blur > 0.0 {
        items.push(blur_group(blur, current_color));
    }
    items.push(fill);
    if blur > 0.0 {
        items.push(DisplayItem::PopFilter);
    }
    items.push(if rounded {
        DisplayItem::PopSvgClip
    } else {
        DisplayItem::PopClip
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_style::Color;

    fn shadow(x: f32, y: f32, blur: f32, spread: f32, inset: bool) -> BoxShadow {
        BoxShadow {
            color: Color::Rgba(Rgba::BLACK),
            x,
            y,
            blur,
            spread,
            inset,
        }
    }

    const BOX: Rect = Rect::new(10.0, 10.0, 60.0, 60.0);
    const SQUARE: Radii = [(0.0, 0.0); 4];

    #[test]
    fn a_ring_is_one_path() {
        let items = outer(
            &[shadow(0.0, 0.0, 0.0, 2.0, false)],
            Rgba::BLACK,
            BOX,
            SQUARE,
            false,
            None,
        );
        assert!(matches!(items.as_slice(), [DisplayItem::FillPath { .. }]));
    }

    #[test]
    fn a_blurred_shadow_is_clipped_and_blurred() {
        let list = [shadow(0.0, 4.0, 6.0, -1.0, false)];
        let items = outer(&list, Rgba::BLACK, BOX, SQUARE, false, None);
        assert!(matches!(
            items.as_slice(),
            [
                DisplayItem::PushSvgClip { .. },
                DisplayItem::PushFilter { .. },
                DisplayItem::Rect { .. },
                DisplayItem::PopFilter,
                DisplayItem::PopSvgClip
            ]
        ));
        // An opaque background covers the border box: no clip.
        let items = outer(&list, Rgba::BLACK, BOX, SQUARE, true, None);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn inner_shadows_stay_inside_the_padding_box() {
        let items = inner(
            &[shadow(0.0, 2.0, 4.0, 0.0, true)],
            Rgba::BLACK,
            BOX,
            SQUARE,
        );
        assert!(matches!(items.first(), Some(DisplayItem::PushClip(r)) if *r == BOX));
        assert!(matches!(items.last(), Some(DisplayItem::PopClip)));
        let none = outer(
            &[shadow(0.0, 2.0, 4.0, 0.0, true)],
            Rgba::BLACK,
            BOX,
            SQUARE,
            false,
            None,
        );
        assert_eq!(none, Vec::new());
    }

    #[test]
    fn spread_changes_radii_as_the_specification_says() {
        let r = spread_radii([(10.0, 10.0), (0.0, 0.0), (2.0, 2.0), (10.0, 4.0)], 4.0);
        assert_eq!(r[0], (14.0, 14.0));
        assert_eq!(r[1], (0.0, 0.0));
        // 2 / 4 = 0.5: 2 + 4 * (1 + (-0.5)^3) = 2 + 3.5.
        assert!((r[2].0 - 5.5).abs() < 1e-4);
        let r = spread_radii([(10.0, 3.0); 4], -5.0);
        assert_eq!(r[0], (5.0, 0.0));
    }

    /// A `corner-shape` other than `round` keeps the clip of an opaque box
    /// (the corners of the border box are not covered) and gives the
    /// shadow and the hole the shape of the box.
    #[test]
    fn a_shaped_corner_keeps_the_clip_and_the_shape() {
        let list = [shadow(0.0, 4.0, 6.0, -1.0, false)];
        let round = [(12.0, 12.0); 4];
        let items = outer(&list, Rgba::BLACK, BOX, round, true, None);
        assert_eq!(items.len(), 3);
        let scoop = Some([-1.0, 1.0, 1.0, 1.0]);
        let items = outer(&list, Rgba::BLACK, BOX, round, true, scoop);
        assert!(matches!(
            items.as_slice(),
            [
                DisplayItem::PushSvgClip { .. },
                DisplayItem::PushFilter { .. },
                DisplayItem::FillPath { .. },
                DisplayItem::PopFilter,
                DisplayItem::PopSvgClip
            ]
        ));
    }

    #[test]
    fn at_most_the_first_shadows_paint() {
        let many = vec![shadow(1.0, 1.0, 0.0, 0.0, false); 10_000];
        let items = outer(&many, Rgba::BLACK, BOX, SQUARE, true, None);
        assert_eq!(items.len(), MAX_SHADOWS);
    }
}
