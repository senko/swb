//! Paints the content of an inline `<svg>` element (ADR 0023) into the
//! display list: path items with their transforms from user units to the
//! list's coordinates, and opacity groups.
//!
//! The content is clipped to the content box when the element's
//! `overflow` clips (the user-agent style sheet sets `overflow: hidden`).
//! Chromium clips at the content box, not the padding box (measured,
//! `tools/probes/inline-svg.json`, case `paint-viewbox-mapping`).
//!
//! A clip path (`clip-path: url(#id)`) that is one axis-aligned rectangle
//! becomes a clip rectangle, which the rasterizer snaps to device pixels
//! (Chromium anti-aliases its edge: a deviation of at most one pixel row
//! or column, and none for the icons that clip to their own `viewBox`).
//! Any other clip path becomes a group that the rasterizer multiplies by
//! the coverage of the clip shapes ([`DisplayItem::PushSvgClip`]).

use std::sync::Arc;

use swb_layout::svg::{ClipPath, ClipRegion, SvgContent, SvgDrawItem};
use swb_layout::{Matrix, Rect, Size};
use swb_style::ComputedStyle;

use crate::display_list::{DisplayItem, path_bounds};
use crate::rope::ItemRope;

/// `content` moved to a whole device pixel at `scale` device pixels per
/// CSS px. Chromium paints the content of an `<svg>` from the pixel-snapped
/// origin of its content box (measured on the Ars Technica logo at
/// y = 297.5: the circle covers whole rows from 298), without changing
/// its size.
fn snap_origin(content: Rect, scale: f32) -> Rect {
    if !(scale.is_finite() && scale > 0.0) {
        return content;
    }
    let snap = |v: f32| (v * scale).round() / scale;
    Rect::new(
        snap(content.x),
        snap(content.y),
        content.width,
        content.height,
    )
}

/// Appends the items of `svg` drawn into the content box `content` (list
/// coordinates) of an element with style `style`.
pub(crate) fn paint(
    list: &mut ItemRope,
    svg: &SvgContent,
    style: &ComputedStyle,
    content: Rect,
    scale: f32,
) {
    let content = snap_origin(content, scale);
    let mut items = Vec::new();
    svg.draw(Size::new(content.width, content.height), &mut items);
    if items.is_empty() {
        return;
    }
    let clips_overflow = style.overflow_x.clips() || style.overflow_y.clips();
    if clips_overflow {
        list.push(DisplayItem::PushClip(content));
    }
    let origin = Matrix::translate(content.x, content.y);
    // The kind of each open clip.
    let mut clips: Vec<ClipKind> = Vec::new();
    let viewport = Rect::new(0.0, 0.0, content.width, content.height);
    for item in items {
        let next = match item {
            SvgDrawItem::PushOpacity(opacity) => DisplayItem::PushOpacity {
                opacity: opacity.max(0.0),
                bounds: Rect::default(),
                fixed_bounds: None,
                escapes_clips: false,
            },
            SvgDrawItem::PopOpacity => DisplayItem::PopOpacity,
            // A clip rectangle that contains the content box clips nothing
            // that the content box clip does not (the usual icon: a
            // `clipPath` of the size of the `viewBox`).
            SvgDrawItem::PushClip(ClipRegion::Rect(rect))
                if clips_overflow && rect.contains_rect(&viewport, 1e-3) =>
            {
                clips.push(ClipKind::Redundant);
                continue;
            }
            SvgDrawItem::PushClip(ClipRegion::Rect(rect)) => {
                clips.push(ClipKind::Rect);
                DisplayItem::PushClip(origin.map_rect(&rect))
            }
            SvgDrawItem::PushClip(ClipRegion::Path(clip)) => {
                clips.push(ClipKind::Path);
                DisplayItem::PushSvgClip {
                    bounds: clip_extent(&clip, &origin),
                    clip,
                    transform: origin,
                    rect_fallback: false,
                }
            }
            SvgDrawItem::PopClip => match clips.pop() {
                Some(ClipKind::Rect) => DisplayItem::PopClip,
                Some(ClipKind::Path) => DisplayItem::PopSvgClip,
                Some(ClipKind::Redundant) | None => continue,
            },
            SvgDrawItem::Fill {
                path,
                transform,
                color,
                rule,
                anti_alias,
            } => DisplayItem::FillPath {
                path,
                transform: origin.multiply(&transform),
                color,
                rule,
                anti_alias,
            },
            SvgDrawItem::Stroke {
                path,
                transform,
                color,
                stroke,
                anti_alias,
            } => DisplayItem::StrokePath {
                path,
                transform: origin.multiply(&transform),
                color,
                stroke,
                anti_alias,
            },
            SvgDrawItem::Hit(hit) => DisplayItem::HitShape {
                node: hit.node,
                path: Arc::clone(&hit.path),
                transform: origin.multiply(&hit.transform),
                fill: hit.fill,
                stroke_width: hit.stroke_width,
            },
        };
        list.push(next);
    }
    if clips_overflow {
        list.push(DisplayItem::PopClip);
    }
}

/// What a pushed clip became in the display list.
enum ClipKind {
    /// A clip rectangle.
    Rect,
    /// A group that is multiplied by the coverage of a clip path.
    Path,
    /// Nothing.
    Redundant,
}

/// An area (list coordinates, `origin` from the content box) that contains
/// everything inside the clip path: the shapes' bounds, inside the extent of
/// the `outer` region.
fn clip_extent(clip: &ClipPath, origin: &Matrix) -> Rect {
    let shapes = clip
        .shapes
        .iter()
        .map(|s| path_bounds(&s.path, &origin.multiply(&s.transform), 0.0))
        .reduce(|a, b| a.union(&b));
    let Some(shapes) = shapes else {
        return Rect::default();
    };
    match &clip.outer {
        None => shapes,
        Some(outer) => shapes
            .intersection(&region_extent(outer, origin))
            .unwrap_or_default(),
    }
}

fn region_extent(region: &ClipRegion, origin: &Matrix) -> Rect {
    match region {
        ClipRegion::Rect(r) => origin.map_rect(r),
        ClipRegion::Path(p) => clip_extent(p, origin),
    }
}
