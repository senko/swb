//! Sizing of replaced elements (images).
//!
//! CSS 2.2 §10.3.2 and §10.6.2, with the min/max rules of §10.4:
//! <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>.

use swb_style::ComputedStyle;

use crate::block::{BoxEdges, ContainingBlock, resolve_max_size, resolve_size};
use crate::box_tree::Replaced;

/// The used content width and height of a replaced element.
pub(crate) fn used_size(
    style: &ComputedStyle,
    replaced: &Replaced,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> (f32, f32) {
    let h_edges = edges.sum().horizontal();
    let v_edges = edges.sum().vertical();
    let width = resolve_size(&style.width, Some(cb.width), style.box_sizing, h_edges);
    let height = resolve_size(&style.height, cb.height, style.box_sizing, v_edges);
    let natural = replaced.natural_size;
    let ratio = style
        .aspect_ratio
        .or_else(|| natural.and_then(|(w, h)| (h > 0.0).then_some(w / h)));

    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (
            w,
            ratio.map_or_else(|| natural.map_or(0.0, |n| n.1), |r| w / r),
        ),
        (None, Some(h)) => (
            ratio.map_or_else(|| natural.map_or(0.0, |n| n.0), |r| h * r),
            h,
        ),
        (None, None) => natural.unwrap_or((0.0, 0.0)),
    };

    // Min/max constraints, keeping the aspect ratio when both dimensions
    // are automatic (a simplified form of the §10.4 table).
    let min_w =
        resolve_size(&style.min_width, Some(cb.width), style.box_sizing, h_edges).unwrap_or(0.0);
    let max_w = resolve_max_size(&style.max_width, Some(cb.width), style.box_sizing, h_edges)
        .unwrap_or(f32::INFINITY);
    let min_h =
        resolve_size(&style.min_height, cb.height, style.box_sizing, v_edges).unwrap_or(0.0);
    let max_h = resolve_max_size(&style.max_height, cb.height, style.box_sizing, v_edges)
        .unwrap_or(f32::INFINITY);

    let clamped_w = w.min(max_w).max(min_w);
    let clamped_h = h.min(max_h).max(min_h);
    match (width.is_none(), height.is_none(), ratio) {
        (true, true, Some(r)) if r > 0.0 => {
            if clamped_w != w {
                (clamped_w, (clamped_w / r).min(max_h).max(min_h))
            } else if clamped_h != h {
                ((clamped_h * r).min(max_w).max(min_w), clamped_h)
            } else {
                (w, h)
            }
        }
        (true, false, Some(r)) if r > 0.0 => ((clamped_h * r).min(max_w).max(min_w), clamped_h),
        (false, true, Some(r)) if r > 0.0 => (clamped_w, (clamped_w / r).min(max_h).max(min_h)),
        _ => (clamped_w, clamped_h),
    }
}
