//! `clip-path` with a basic shape or a geometry box on a box (CSS Masking 1
//! §5, <https://drafts.fxtf.org/css-masking-1/#the-clip-path>; the shapes
//! are CSS Shapes 1 §3,
//! <https://drafts.csswg.org/css-shapes-1/#basic-shape-functions>, placed
//! by `swb_layout::shape_geometry`).
//!
//! The box starts a group when it paints ([`crate::DisplayItem::PushSvgClip`],
//! or a plain [`crate::DisplayItem::PushClip`] for a rectangle): the group
//! is multiplied by the coverage of the shape, and what lies outside it is
//! not hit. The shape is placed in the reference box:
//!
//! - `border-box` (the default), `stroke-box` and `view-box`: the border
//!   box, with the rounding of `border-radius`;
//! - `padding-box` and `content-box` (and `fill-box`, which is the content
//!   box for a box with a CSS layout box): the box, with the border radius
//!   reduced by the border (and padding) widths;
//! - `margin-box`: the border box grown by the margins in px (a
//!   percentage or `auto` margin counts as 0), with the radii grown by
//!   them. Chromium's curvature for it was not measured.

use swb_layout::{
    BoxFragment, CornerRadii, Edges, Point, Rect, ShapeGeometry, box_geometry, shape_geometry,
};
use swb_style::{ClipPath as ClipPathValue, GeometryBox, LengthPercentage, LengthPercentageOrAuto};

use crate::display_list::{DisplayItem, path_clip_item, resolve_radii};

/// The items that start and end the clip group of a box.
pub(crate) struct BoxClip {
    /// Starts the group.
    pub(crate) push: DisplayItem,
    /// Ends it.
    pub(crate) pop: DisplayItem,
}

/// The clip of `b`, whose parent's border-box origin is `origin`, if its
/// `clip-path` is a basic shape or a geometry box.
pub(crate) fn box_clip(b: &BoxFragment, origin: Point) -> Option<BoxClip> {
    let ClipPathValue::Shape(clip) = &b.style.clip_path else {
        return None;
    };
    let (rect, radii) = reference_box(b, origin, clip.reference_box());
    let geometry = match &clip.shape {
        None => box_geometry(rect, radii.as_ref()),
        Some(shape) => shape_geometry(shape, rect),
    };
    Some(match geometry {
        ShapeGeometry::Empty => rect_clip(Rect::new(rect.x, rect.y, 0.0, 0.0)),
        ShapeGeometry::Rect(rect) => rect_clip(rect),
        ShapeGeometry::Path {
            path,
            rule,
            transform,
            bounds,
        } => BoxClip {
            push: path_clip_item(path, rule, transform, bounds, true),
            pop: DisplayItem::PopSvgClip,
        },
    })
}

fn rect_clip(rect: Rect) -> BoxClip {
    BoxClip {
        push: DisplayItem::PushClip(rect),
        pop: DisplayItem::PopClip,
    }
}

/// The reference box of a shape and its corner radii, if any is not zero.
fn reference_box(
    b: &BoxFragment,
    origin: Point,
    which: GeometryBox,
) -> (Rect, Option<CornerRadii>) {
    let border = b.border_rect.translate(origin);
    let outer = resolve_radii(&b.style, border);
    let (rect, radii) = match which {
        GeometryBox::BorderBox | GeometryBox::StrokeBox | GeometryBox::ViewBox => (border, outer),
        GeometryBox::PaddingBox => (border.inset(&b.border), shrink(outer, &b.border)),
        GeometryBox::ContentBox | GeometryBox::FillBox => {
            let padding = border.inset(&b.border);
            (
                padding.inset(&b.padding),
                shrink(shrink(outer, &b.border), &b.padding),
            )
        }
        GeometryBox::MarginBox => margin_box(b, border, outer),
    };
    let rounded = radii.iter().any(|&(h, v)| h > 0.0 && v > 0.0);
    (rect, rounded.then_some(radii))
}

/// The radii inside `edges` (the border or the padding): each radius
/// minus the width of the sides it touches, at least 0.
fn shrink(radii: CornerRadii, edges: &Edges) -> CornerRadii {
    let [tl, tr, br, bl] = radii;
    let by = |(h, v): (f32, f32), x: f32, y: f32| ((h - x).max(0.0), (v - y).max(0.0));
    [
        by(tl, edges.left, edges.top),
        by(tr, edges.right, edges.top),
        by(br, edges.right, edges.bottom),
        by(bl, edges.left, edges.bottom),
    ]
}

/// The margin box and its radii: a rounded corner grows by the margins
/// at it, a square one stays square.
fn margin_box(b: &BoxFragment, border: Rect, outer: CornerRadii) -> (Rect, CornerRadii) {
    let margin = |m: &LengthPercentageOrAuto| match m.non_auto() {
        Some(LengthPercentage::Px(px)) => *px,
        _ => 0.0,
    };
    let s = &b.style;
    let (top, right, bottom, left) = (
        margin(&s.margin_top),
        margin(&s.margin_right),
        margin(&s.margin_bottom),
        margin(&s.margin_left),
    );
    let rect = Rect::new(
        border.x - left,
        border.y - top,
        border.width + left + right,
        border.height + top + bottom,
    );
    let grow = |(h, v): (f32, f32), x: f32, y: f32| {
        if h > 0.0 && v > 0.0 {
            (h + x.max(0.0), v + y.max(0.0))
        } else {
            (0.0, 0.0)
        }
    };
    let [tl, tr, br, bl] = outer;
    (
        rect,
        [
            grow(tl, left, top),
            grow(tr, right, top),
            grow(br, right, bottom),
            grow(bl, left, bottom),
        ],
    )
}
