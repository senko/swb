//! The bounds of the groups of a finished display list (transform, opacity
//! and mask groups), in one pass over the items: each item adds its area
//! to the innermost open group, and a group adds its area (mapped by its
//! transform) to the group around it when it ends. So the work is linear
//! in the items, also for deeply nested groups.
//!
//! Content fixed to the viewport moves with the scroll offset, which the
//! list does not know: its area is kept apart, in viewport coordinates
//! (`fixed_bounds` of transform and opacity groups), and the rasterizer and
//! hit testing move it by the scroll offset.
//!
//! [`transform_ends`] finds the end of each transform group, for the
//! rasterizer and hit testing, which skip groups.

use swb_layout::{GroupTransform, Rect, StickyCache};

use crate::display_list::DisplayItem;

/// The areas of the content of a group.
#[derive(Clone, Copy, Debug, Default)]
struct Areas {
    /// What the content draws in the group's coordinates (without content
    /// fixed to the viewport).
    paint: Option<Rect>,
    /// `paint` and the hit regions.
    hit: Option<Rect>,
    /// What content fixed to the viewport draws, in viewport coordinates.
    fixed_paint: Option<Rect>,
    /// `fixed_paint` and the hit regions of that content.
    fixed_hit: Option<Rect>,
    /// True if the content includes content fixed to the viewport, which
    /// the enclosing clips do not clip.
    escapes: bool,
}

impl Areas {
    fn add(&mut self, other: Areas) {
        self.paint = union_areas(self.paint, other.paint);
        self.hit = union_areas(self.hit, other.hit);
        self.fixed_paint = union_areas(self.fixed_paint, other.fixed_paint);
        self.fixed_hit = union_areas(self.fixed_hit, other.fixed_hit);
        self.escapes |= other.escapes;
    }
}

/// The union of two optional areas.
pub(crate) fn union_areas(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(&b)),
        (a, b) => a.or(b),
    }
}

/// Sets the bounds of every group of `items` (see the module
/// documentation). The builder puts placeholders there: the extent of the
/// mask layers in `PushMask::bounds`, and nothing in the others.
pub(crate) fn finish_groups(items: &mut [DisplayItem]) {
    // The open groups: the index of the `Push…` item and the areas of the
    // content so far.
    let mut stack: Vec<(usize, Areas)> = Vec::new();
    let mut sticky = StickyCache::default();
    for i in 0..items.len() {
        let Some(item) = items.get(i) else {
            break;
        };
        match item {
            DisplayItem::PushTransform { .. }
            | DisplayItem::PushOpacity { .. }
            | DisplayItem::PushMask { .. }
            | DisplayItem::PushSvgClip { .. } => stack.push((i, Areas::default())),
            DisplayItem::PopTransform
            | DisplayItem::PopOpacity
            | DisplayItem::PopMask
            | DisplayItem::PopSvgClip => {
                end_group(items, &mut stack, &mut sticky);
            }
            DisplayItem::HitShape {
                path,
                transform,
                stroke_width,
                ..
            } => {
                if let Some((_, top)) = stack.last_mut() {
                    let grow = stroke_width.unwrap_or(0.0);
                    let rect = crate::display_list::path_bounds(path, transform, grow);
                    top.hit = union_areas(top.hit, Some(rect));
                }
            }
            DisplayItem::PushViewportClip => {
                if let Some((_, top)) = stack.last_mut() {
                    top.escapes = true;
                }
            }
            DisplayItem::HitRegion { rect, .. } => {
                if let Some((_, top)) = stack.last_mut() {
                    top.hit = union_areas(top.hit, Some(*rect));
                }
            }
            other => {
                // Items outside groups need no bounds.
                if let Some((_, top)) = stack.last_mut()
                    && let Some(b) = other.bounds()
                {
                    top.paint = union_areas(top.paint, Some(b));
                    top.hit = union_areas(top.hit, Some(b));
                }
            }
        }
    }
    // Groups without an end (the builder always ends them).
    while !stack.is_empty() {
        end_group(items, &mut stack, &mut sticky);
    }
}

/// Ends the innermost group of the open groups `stack`: stores its bounds
/// and adds its areas to the group around it.
fn end_group(items: &mut [DisplayItem], stack: &mut Vec<(usize, Areas)>, sticky: &mut StickyCache) {
    let Some((at, areas)) = stack.pop() else {
        return;
    };
    let added = items
        .get_mut(at)
        .map_or_else(Areas::default, |item| close(item, areas, sticky));
    if let Some((_, outer)) = stack.last_mut() {
        outer.add(added);
    }
}

/// Stores the bounds of a group with the content `areas` in its `Push…`
/// item, and returns the areas that the group adds to the group around it.
fn close(item: &mut DisplayItem, areas: Areas, sticky: &mut StickyCache) -> Areas {
    match item {
        DisplayItem::PushTransform {
            transform,
            bounds,
            hit_bounds,
            fixed_bounds,
        } => {
            let moved = |f: &dyn Fn(Rect) -> Rect| Areas {
                paint: areas.paint.map(f),
                hit: areas.hit.map(f),
                ..areas
            };
            match transform {
                // Content fixed to the viewport is in viewport coordinates
                // in the group, and so is the group's own content.
                GroupTransform::Fixed => {
                    let paint = union_areas(areas.paint, areas.fixed_paint);
                    let hit = union_areas(areas.hit, areas.fixed_hit);
                    *bounds = paint.unwrap_or_default();
                    *hit_bounds = hit.unwrap_or_default();
                    *fixed_bounds = None;
                    Areas {
                        paint: None,
                        hit: None,
                        fixed_paint: paint,
                        fixed_hit: hit,
                        escapes: areas.escapes,
                    }
                }
                GroupTransform::Matrix(m) => {
                    *bounds = areas.paint.unwrap_or_default();
                    *hit_bounds = areas.hit.unwrap_or_default();
                    *fixed_bounds = areas.fixed_hit;
                    moved(&|r| m.map_rect(&r))
                }
                GroupTransform::Sticky(constraints) => {
                    *bounds = areas.paint.unwrap_or_default();
                    *hit_bounds = areas.hit.unwrap_or_default();
                    *fixed_bounds = areas.fixed_hit;
                    let (low, high) = constraints.offset_range_with(sticky);
                    moved(&|r| {
                        Rect::new(
                            r.x + low.x,
                            r.y + low.y,
                            r.width + high.x - low.x,
                            r.height + high.y - low.y,
                        )
                    })
                }
            }
        }
        DisplayItem::PushOpacity {
            bounds,
            fixed_bounds,
            escapes_clips,
            ..
        } => {
            *bounds = areas.paint.unwrap_or_default();
            *fixed_bounds = areas.fixed_paint;
            *escapes_clips = areas.escapes;
            areas
        }
        // The clip path hides everything outside its extent (SVG content
        // has no content fixed to the viewport).
        DisplayItem::PushSvgClip { bounds, .. } => {
            let extent = *bounds;
            let shown = areas.paint.and_then(|p| p.intersection(&extent));
            *bounds = shown.unwrap_or_default();
            Areas {
                paint: shown,
                ..areas
            }
        }
        // The mask hides everything outside the extent of its layers, also
        // content fixed to the viewport (and the enclosing clips clip it):
        // what the group draws is inside `shown`. Hit testing ignores
        // masks.
        DisplayItem::PushMask { bounds, .. } => {
            let extent = *bounds;
            let shown = if areas.fixed_paint.is_some() {
                Some(extent)
            } else {
                areas.paint.and_then(|p| p.intersection(&extent))
            };
            *bounds = shown.unwrap_or_default();
            Areas {
                paint: shown,
                fixed_paint: None,
                escapes: false,
                ..areas
            }
        }
        _ => Areas::default(),
    }
}

/// The index of the matching `PopTransform` of each `PushTransform` in
/// `items` (the length of `items` if there is none); 0 for other items.
pub(crate) fn transform_ends(items: &[DisplayItem]) -> Vec<usize> {
    let mut ends = vec![0; items.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            DisplayItem::PushTransform { .. } => open.push(i),
            DisplayItem::PopTransform => {
                if let Some(end) = open.pop().and_then(|start| ends.get_mut(start)) {
                    *end = i;
                }
            }
            _ => {}
        }
    }
    for start in open {
        if let Some(end) = ends.get_mut(start) {
            *end = items.len();
        }
    }
    ends
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use swb_dom::NodeId;
    use swb_layout::{Matrix, StickyConstraints};

    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> DisplayItem {
        DisplayItem::Rect {
            rect: Rect::new(x, y, w, h),
            radii: [(0.0, 0.0); 4],
            color: swb_style::Rgba::BLACK,
        }
    }

    fn push(transform: GroupTransform) -> DisplayItem {
        DisplayItem::PushTransform {
            transform,
            bounds: Rect::default(),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        }
    }

    fn opacity() -> DisplayItem {
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: Rect::default(),
            fixed_bounds: None,
            escapes_clips: false,
        }
    }

    #[test]
    fn nested_transforms_map_their_content() {
        let mut items = vec![
            push(GroupTransform::Matrix(Matrix::translate(100.0, 0.0))),
            rect(0.0, 0.0, 10.0, 10.0),
            push(GroupTransform::Matrix(Matrix::translate(0.0, 500.0))),
            rect(0.0, 0.0, 10.0, 10.0),
            DisplayItem::HitRegion {
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                node: NodeId::DOCUMENT,
            },
            DisplayItem::PopTransform,
            DisplayItem::PopTransform,
        ];
        finish_groups(&mut items);
        // The inner group draws its rectangle and has a larger hit region;
        // the outer one has the inner one moved down.
        assert!(
            matches!(items[2], DisplayItem::PushTransform { bounds, hit_bounds, .. }
            if bounds == Rect::new(0.0, 0.0, 10.0, 10.0)
                && hit_bounds == Rect::new(0.0, 0.0, 20.0, 20.0))
        );
        assert!(
            matches!(items[0], DisplayItem::PushTransform { bounds, hit_bounds, .. }
            if bounds == Rect::new(0.0, 0.0, 10.0, 510.0)
                && hit_bounds == Rect::new(0.0, 0.0, 20.0, 520.0))
        );
    }

    #[test]
    fn sticky_groups_count_with_the_range_of_their_offsets() {
        let sticky = StickyConstraints {
            rect: Rect::new(0.0, 100.0, 10.0, 10.0),
            limit: Rect::new(0.0, 0.0, 100.0, 300.0),
            insets: [Some(0.0), None, None, None],
            scrollport: None,
            viewport: swb_layout::Size::new(800.0, 600.0),
            shift_box: None,
            shift_containing_block: None,
        };
        let mut items = vec![
            opacity(),
            push(GroupTransform::Sticky(Arc::new(sticky))),
            rect(0.0, 100.0, 10.0, 10.0),
            DisplayItem::PopTransform,
            DisplayItem::PopOpacity,
        ];
        finish_groups(&mut items);
        assert!(matches!(items[0], DisplayItem::PushOpacity { bounds, .. }
            if bounds == Rect::new(0.0, 100.0, 10.0, 200.0)));
    }

    #[test]
    fn fixed_content_is_kept_apart() {
        // A small fixed box in an opacity group: the group's own content
        // and the fixed content (viewport coordinates) are bounded. The
        // opacity layer is not larger for a hit region.
        let mut items = vec![
            opacity(),
            rect(0.0, 1000.0, 10.0, 10.0),
            DisplayItem::PushViewportClip,
            push(GroupTransform::Fixed),
            rect(5.0, 5.0, 4.0, 4.0),
            DisplayItem::HitRegion {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                node: NodeId::DOCUMENT,
            },
            DisplayItem::PopTransform,
            DisplayItem::PopClip,
            DisplayItem::PopOpacity,
        ];
        finish_groups(&mut items);
        assert!(matches!(
            items[3],
            DisplayItem::PushTransform { bounds, hit_bounds, fixed_bounds: None, .. }
                if bounds == Rect::new(5.0, 5.0, 4.0, 4.0)
                    && hit_bounds == Rect::new(0.0, 0.0, 100.0, 100.0)
        ));
        assert!(matches!(
            items[0],
            DisplayItem::PushOpacity { bounds, fixed_bounds: Some(fixed), escapes_clips: true, .. }
                if bounds == Rect::new(0.0, 1000.0, 10.0, 10.0)
                    && fixed == Rect::new(5.0, 5.0, 4.0, 4.0)
        ));
        // Around a transform group, the fixed content counts with its hit
        // region (for hit testing).
        items.insert(0, push(GroupTransform::Matrix(Matrix::IDENTITY)));
        items.push(DisplayItem::PopTransform);
        finish_groups(&mut items);
        assert!(matches!(
            items[0],
            DisplayItem::PushTransform { bounds, fixed_bounds: Some(fixed), .. }
                if bounds == Rect::new(0.0, 1000.0, 10.0, 10.0)
                    && fixed == Rect::new(0.0, 0.0, 100.0, 100.0)
        ));
    }
}
