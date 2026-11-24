//! The painted bounds of the boxes of animated elements (ADR 0044 section 8;
//! M13 feature 3c): what the visibility test of the frame loop intersects
//! with the viewport. One walk of the fragment tree places every fragment
//! as paint does (`FragmentTree::walk_painted`: scroll offsets, transforms,
//! fixed and sticky offsets at the viewport scroll offset) and adds its
//! painted rectangle, grown by what the box paints outside it (shadows,
//! outlines, filters), to the bounds of the keys that the caller names for
//! it.

use std::collections::HashMap;

use swb_dom::NodeId;
use swb_layout::{FragmentRef, FragmentTree, Point, Rect, ScrollOffsets};
use swb_style::ComputedStyle;

use crate::filter::css_output;

/// The rectangle that contains what a box with `style` and the border box
/// `rect` paints: its shadows, its outline and the output of its filters
/// grow it (CSS px).
pub(crate) fn visual_rect(style: &ComputedStyle, rect: Rect) -> Rect {
    let mut grow = 0.0f32;
    for s in style.box_shadow.iter().filter(|s| !s.inset) {
        grow = grow.max(s.x.abs().max(s.y.abs()) + 3.0 * s.blur.max(0.0) + s.spread.max(0.0));
    }
    for s in style.text_shadow.iter() {
        grow = grow.max(s.x.abs().max(s.y.abs()) + 3.0 * s.blur.max(0.0));
    }
    if style.outline_width > 0.0 && style.outline_style != swb_style::OutlineStyle::None {
        grow = grow.max(style.outline_width + style.outline_offset.max(0.0));
    }
    let grow = if grow.is_finite() { grow } else { 1e9 };
    let rect = Rect::new(
        rect.x - grow,
        rect.y - grow,
        rect.width + 2.0 * grow,
        rect.height + 2.0 * grow,
    );
    css_output(rect, style.filter.functions())
}

/// The keys of a fragment for [`animation_bounds`]: (key, grow).
pub type Keys = Vec<(NodeId, bool)>;

/// The painted bounds (document coordinates at the viewport scroll offset
/// `scroll`, CSS px) of the keys that `keys` gives for the fragments of
/// `tree`: `keys` is called once per fragment and pushes (key, grow) pairs;
/// with `grow` the fragment counts with [`visual_rect`], else with its
/// border box (the clip of a scroll container). A key without fragments is
/// not in the result.
pub fn animation_bounds(
    tree: &FragmentTree,
    offsets: &dyn ScrollOffsets,
    scroll: Point,
    keys: &mut dyn FnMut(FragmentRef<'_>, &mut Keys),
) -> HashMap<NodeId, Rect> {
    let mut bounds: HashMap<NodeId, Rect> = HashMap::new();
    let mut found = Vec::new();
    tree.walk_painted(offsets, scroll, |fragment, rect, matrix| {
        found.clear();
        keys(fragment, &mut found);
        if found.is_empty() {
            return;
        }
        let style = match fragment {
            FragmentRef::Box(b) => &b.style,
            FragmentRef::Text(t) => &t.style,
        };
        let grown = matrix.map_rect(&visual_rect(style, rect));
        let plain = matrix.map_rect(&rect);
        for &(key, grow) in &found {
            let r = if grow { grown } else { plain };
            bounds
                .entry(key)
                .and_modify(|b| *b = b.union(&r))
                .or_insert(r);
        }
    });
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_style::BoxShadow;

    #[test]
    fn shadows_and_outlines_grow_the_visual_rect() {
        let mut style = (*ComputedStyle::initial()).clone();
        let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert_eq!(visual_rect(&style, rect), rect);
        style.outline_width = 3.0;
        style.outline_offset = 2.0;
        // `outline-style: none` draws nothing.
        assert_eq!(visual_rect(&style, rect), rect);
        style.outline_style = swb_style::OutlineStyle::Solid;
        assert_eq!(visual_rect(&style, rect), Rect::new(5.0, 5.0, 30.0, 30.0));
        style.box_shadow = std::sync::Arc::from(vec![BoxShadow {
            color: swb_style::Color::default(),
            x: 4.0,
            y: -6.0,
            blur: 2.0,
            spread: 1.0,
            inset: false,
        }]);
        // |y| 6 + 3 x blur 2 + spread 1 = 13.
        assert_eq!(visual_rect(&style, rect), Rect::new(-3.0, -3.0, 46.0, 46.0));
    }
}
