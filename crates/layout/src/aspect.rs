//! The preferred aspect ratio of boxes that are not replaced.
//!
//! CSS Box Sizing 4 §5 (`aspect-ratio`):
//! <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>. A block, flex or
//! grid container with an `aspect-ratio` gets the size of an `auto` axis
//! from the other axis, as a block-level box, a float, an inline-block, a
//! flex or grid item, or an absolutely positioned box. The ratio applies to
//! the content box, or to the border box with `box-sizing: border-box`
//! (§5.1).
//!
//! Chromium 148 (measured with `tools/probes/aspect-ratio.json`):
//!
//! - A block-level box with an `auto` width and a definite height takes
//!   its width from the height (it does not fill the containing block).
//!   With both `auto`, the width is as usual and the height follows from
//!   it; the minimum and maximum sizes then transfer through the ratio with
//!   the table of CSS 2.2 §10.4 (as for images, `replaced::constrain_both`).
//! - `min-height: auto` (§5.1.1) lets the box grow to the height of its
//!   content, unless it is a scroll container. An explicit `min-height`
//!   (even `0`) turns this off, and `max-height` still wins.
//! - Replaced elements, tables and form controls have their own rules
//!   (`replaced.rs`, `table/`, `control.rs`); this module is for blocks,
//!   flex containers and grid containers.

use swb_style::ComputedStyle;

use crate::block::{BoxEdges, SizeLimits, resolve_size};
use crate::box_tree::{IndependentBox, IndependentContents};
use crate::geom::clamp_length;
use crate::replaced::constrain_both;

/// The preferred aspect ratio of a box with `style` and the box edges that
/// it applies to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ratio {
    /// Width / height, finite and positive.
    ratio: f32,
    /// The horizontal and vertical padding and border that are inside the
    /// ratio's box: all of them with `box-sizing: border-box`, else 0.
    inside: (f32, f32),
}

impl Ratio {
    /// The ratio of a box with `style` and `edges`, or `None` if the box
    /// has no preferred aspect ratio.
    pub(crate) fn of(style: &ComputedStyle, edges: &BoxEdges) -> Option<Ratio> {
        let max = swb_style::Length::MAX_PX;
        let ratio = style
            .aspect_ratio
            .preferred(None)
            .filter(|r| r.is_finite() && *r > 0.0)
            .map(|r| r.max(1.0 / max).min(max))?;
        let sum = edges.sum();
        let inside = match style.box_sizing {
            swb_style::BoxSizing::BorderBox => (sum.horizontal(), sum.vertical()),
            swb_style::BoxSizing::ContentBox => (0.0, 0.0),
        };
        Some(Ratio { ratio, inside })
    }

    /// The content height for the content width `width`.
    pub(crate) fn height(self, width: f32) -> f32 {
        clamp_length(((width + self.inside.0) / self.ratio - self.inside.1).max(0.0))
    }

    /// The content width for the content height `height`.
    pub(crate) fn width(self, height: f32) -> f32 {
        clamp_length(((height + self.inside.1) * self.ratio - self.inside.0).max(0.0))
    }
}

/// True if the ratio of `ib` is for a block, flex or grid container.
pub(crate) fn applies_to(ib: &IndependentBox) -> bool {
    matches!(
        ib.contents,
        IndependentContents::Flow(_) | IndependentContents::Flex(_) | IndependentContents::Grid(_)
    )
}

/// True if `ib` is a block, flex or grid container with a preferred
/// aspect ratio.
pub(crate) fn has_ratio(ib: &IndependentBox) -> bool {
    // The edges change only the ratio's value, not whether there is one.
    applies_to(ib) && Ratio::of(&ib.base.style, &BoxEdges::default()).is_some()
}

/// The content width of a block-level box with an `auto` width: from its
/// definite height through the ratio, or else from `tentative` (the width
/// without the ratio: the available width or the shrink-to-fit width) with
/// the minimum and maximum sizes transferred. `None` if the box has no
/// ratio or its width is not `auto`. `cb_width` and `cb_height` are the
/// containing block's.
pub(crate) fn auto_width(
    style: &ComputedStyle,
    edges: &BoxEdges,
    (cb_width, cb_height): (f32, Option<f32>),
    tentative: impl FnOnce() -> f32,
) -> Option<f32> {
    if !style.width.is_auto() {
        return None;
    }
    let ratio = Ratio::of(style, edges)?;
    let sum = edges.sum();
    let heights = SizeLimits::of_height(style, cb_height, sum.vertical());
    let widths = SizeLimits::of_width(style, Some(cb_width), sum.horizontal());
    if let Some(h) = resolve_size(&style.height, cb_height, style.box_sizing, sum.vertical()) {
        return Some(widths.clamp(ratio.width(heights.clamp(h))));
    }
    let w = tentative();
    let h = ratio.height(w);
    let (ow, oh) = ratio.inside;
    let (w, _) = constrain_both(
        w + ow,
        h + oh,
        ratio.ratio,
        (widths.min() + ow, widths.max() + ow),
        (heights.min() + oh, heights.max() + oh),
    );
    Some(clamp_length((w - ow).max(0.0)))
}

/// The content height of a box with an `auto` height, a ratio and the
/// content width `width`, within `min-height` and `max-height`; `None` if
/// the height is not `auto` or there is no ratio. Percentage heights that
/// do not resolve (`cb_height` is `None`) count as `auto`. The result is
/// definite: percentage heights of the children resolve against it.
pub(crate) fn auto_height(
    style: &ComputedStyle,
    edges: &BoxEdges,
    width: f32,
    cb_height: Option<f32>,
) -> Option<f32> {
    let v = edges.sum().vertical();
    // The intrinsic size keywords (`fit-content`, `min-content`,
    // `max-content`) count as `auto` here: Chromium applies the ratio.
    if resolve_size(&style.height, cb_height, style.box_sizing, v).is_some() {
        return None;
    }
    let ratio = Ratio::of(style, edges)?;
    Some(SizeLimits::of_height(style, cb_height, v).clamp(ratio.height(width)))
}

/// True if a box whose height came from its ratio grows to the height of
/// its content (§5.1.1): its `min-height` is `auto` and it is not a scroll
/// container.
pub(crate) fn grows_to_content(style: &ComputedStyle) -> bool {
    style.min_height.is_auto() && !crate::scroll::is_scroll_container(style)
}

/// The content width of a box with a ratio and a definite `height`
/// (`px`, no percentage) for its min-content and max-content sizes.
/// `None` if the width is not `auto`, there is no ratio or the height is
/// not definite.
pub(crate) fn width_of_definite_height(style: &ComputedStyle) -> Option<f32> {
    if !style.width.is_auto() {
        return None;
    }
    let edges = BoxEdges::resolve(style, 0.0);
    let ratio = Ratio::of(style, &edges)?;
    let v = edges.sum().vertical();
    let h = resolve_size(&style.height, None, style.box_sizing, v)?;
    Some(ratio.width(SizeLimits::of_height(style, None, v).clamp(h)))
}

/// The content width of a box with a ratio and an `auto` width whose
/// content height is `height`: through the ratio, within `min-width` and
/// `max-width`. `None` if there is no ratio or the width is not `auto`.
pub(crate) fn width_from_height(
    style: &ComputedStyle,
    height: f32,
    cb_width: Option<f32>,
    edges: &BoxEdges,
) -> Option<f32> {
    if !style.width.is_auto() {
        return None;
    }
    transferred_width(style, height, cb_width, edges)
}

/// [`width_from_height`] for a box of any `width`: the transferred size
/// suggestion of a flex item (Flexbox 1 §4.5).
pub(crate) fn transferred_width(
    style: &ComputedStyle,
    height: f32,
    cb_width: Option<f32>,
    edges: &BoxEdges,
) -> Option<f32> {
    let ratio = Ratio::of(style, edges)?;
    let limits = SizeLimits::of_width(style, cb_width, edges.sum().horizontal());
    Some(limits.clamp(ratio.width(height)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Edges;

    fn ratio(border_box: bool) -> Ratio {
        let edges = BoxEdges {
            padding: Edges::new(10.0, 10.0, 10.0, 10.0),
            border: Edges::new(5.0, 5.0, 5.0, 5.0),
        };
        let mut style = (*ComputedStyle::initial()).clone();
        style.aspect_ratio = swb_style::AspectRatio {
            auto: false,
            ratio: Some(2.0),
        };
        if border_box {
            style.box_sizing = swb_style::BoxSizing::BorderBox;
        }
        Ratio::of(&style, &edges).expect("a ratio is set")
    }

    /// Chromium 148: `width: 200px` with `padding: 10px; border: 5px` and
    /// `aspect-ratio: 2/1` is 230×130 (content 200×100); with
    /// `box-sizing: border-box` it is 200×100.
    #[test]
    fn the_ratio_applies_to_the_content_or_the_border_box() {
        let content = ratio(false);
        assert_eq!(content.height(200.0), 100.0);
        assert_eq!(content.width(100.0), 200.0);
        let border = ratio(true);
        // Border box 200×100: content 170×70.
        assert_eq!(border.height(170.0), 70.0);
        assert_eq!(border.width(70.0), 170.0);
    }

    #[test]
    fn a_negative_content_size_is_zero() {
        let border = ratio(true);
        // Content 0 wide: the border box is 30 wide and 15 high, less than
        // its 30 px of padding and border.
        assert_eq!(border.height(0.0), 0.0);
        assert_eq!(border.width(0.0), 30.0);
    }

    // The expected sizes in the next tests were measured with Chromium 148
    // (`tools/probes/aspect-ratio.json`).
    fn sizes(html: &str, ids: &[&str]) -> Vec<(f32, f32)> {
        let l = crate::test_support::layout_html(&crate::test_support::body(html));
        ids.iter()
            .map(|id| {
                let r = l.rect(id);
                (r.width, r.height)
            })
            .collect()
    }

    #[test]
    fn blocks_take_the_size_of_an_auto_axis_from_the_other() {
        let s = sizes(
            "<style>.r { aspect-ratio: 2 / 1 }</style><div style='width: 400px'>\
             <div class=r id=a style='width: 200px'></div><div class=r id=b></div>\
             <div class=r id=c style='height: 30px'></div>\
             <div class=r id=d style='width: 100px; height: 30px'></div></div>",
            &["a", "b", "c", "d"],
        );
        assert_eq!(
            s,
            [(200.0, 100.0), (400.0, 200.0), (60.0, 30.0), (100.0, 30.0)]
        );
    }

    #[test]
    fn the_automatic_minimum_height_lets_a_block_grow_unless_it_scrolls() {
        let s = sizes(
            "<style>.r { aspect-ratio: 2 / 1; width: 200px } .t { height: 150px }</style>\
             <div class=r id=a><div class=t></div></div>\
             <div class=r id=b style='overflow: hidden'><div class=t></div></div>\
             <div class=r id=c style='overflow: clip'><div class=t></div></div>\
             <div class=r id=d style='min-height: 0'><div class=t></div></div>\
             <div class=r id=e style='max-height: 120px'><div class=t></div></div>",
            &["a", "b", "c", "d", "e"],
        );
        let heights: Vec<f32> = s.iter().map(|&(_, h)| h).collect();
        assert_eq!(heights, [150.0, 100.0, 150.0, 100.0, 120.0]);
    }

    #[test]
    fn minimum_and_maximum_sizes_transfer_through_the_ratio() {
        let s = sizes(
            "<style>.r { aspect-ratio: 2 / 1 }</style><div style='width: 400px'>\
             <div class=r id=a style='max-width: 200px'></div>\
             <div class=r id=b style='max-height: 50px'></div>\
             <div class=r id=c style='min-height: 300px'></div>\
             <div class=r id=d style='width: 100px; min-height: 80px'></div>\
             <div class=r id=e style='height: 100px; max-width: 150px'></div></div>",
            &["a", "b", "c", "d", "e"],
        );
        assert_eq!(
            s,
            [
                (200.0, 100.0),
                (100.0, 50.0),
                (600.0, 300.0),
                (100.0, 80.0),
                (150.0, 100.0)
            ]
        );
    }

    #[test]
    fn percentage_heights_resolve_against_the_height_from_the_ratio() {
        let s = sizes(
            "<div style='aspect-ratio: 2 / 1; width: 200px'><div id=c style='height: 50%'></div></div>",
            &["c"],
        );
        assert_eq!(s, [(200.0, 50.0)]);
    }

    #[test]
    fn flex_items_use_the_ratio_in_both_directions() {
        let s = sizes(
            "<style>.r { aspect-ratio: 2 / 1 }</style>\
             <div style='display: flex; width: 600px; align-items: flex-start'>\
             <div class=r id=a style='height: 60px'></div>\
             <div class=r id=b style='flex: 1'></div></div>\
             <div style='display: flex; flex-direction: column; width: 300px'>\
             <div class=r id=c></div>\
             <div class=r id=d style='align-self: flex-start; height: 40px'></div></div>",
            &["a", "b", "c", "d"],
        );
        assert_eq!(
            s,
            [(120.0, 60.0), (480.0, 240.0), (300.0, 150.0), (80.0, 40.0)]
        );
    }

    #[test]
    fn a_stretched_flex_item_has_the_transferred_minimum_width() {
        let s = sizes(
            "<div style='display: flex; width: 600px; height: 200px'>\
             <div id=a style='aspect-ratio: 2 / 1; width: 100px'></div>\
             <div id=b style='aspect-ratio: 2 / 1; height: 60px'></div>\
             <div id=c style='aspect-ratio: 2 / 1; flex: 1'></div></div>",
            &["a", "b", "c"],
        );
        // The last item is 400 px wide (its transferred minimum width), not
        // the 380 px that are left.
        assert_eq!(s, [(100.0, 200.0), (120.0, 60.0), (400.0, 200.0)]);
    }

    #[test]
    fn absolutely_positioned_boxes_use_the_ratio() {
        let s = sizes(
            "<div style='position: relative; width: 400px; height: 300px'>\
             <div id=a style='position: absolute; aspect-ratio: 2 / 1; width: 100px'></div>\
             <div id=b style='position: absolute; aspect-ratio: 2 / 1; height: 40px'></div>\
             <div id=c style='position: absolute; aspect-ratio: 2 / 1; top: 20px; bottom: 100px'></div>\
             <div id=d style='position: absolute; aspect-ratio: 2 / 1; top: 0; bottom: 0; left: 0; right: 0'></div></div>",
            &["a", "b", "c", "d"],
        );
        assert_eq!(
            s,
            [(100.0, 50.0), (80.0, 40.0), (360.0, 180.0), (400.0, 200.0)]
        );
    }

    #[test]
    fn a_float_or_inline_block_is_as_wide_as_its_height_makes_it() {
        let s = sizes(
            "<div id=a style='float: left'><div style='aspect-ratio: 2 / 1; height: 60px'></div></div>\
             <div style='clear: both'></div>\
             <div id=b style='display: inline-block; aspect-ratio: 2 / 1; height: 50px'></div>",
            &["a", "b"],
        );
        assert_eq!(s, [(120.0, 60.0), (100.0, 50.0)]);
    }
}
