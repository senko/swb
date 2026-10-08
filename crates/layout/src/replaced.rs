//! Sizing of replaced elements (images, video, audio).
//!
//! CSS 2.2 §10.3.2 and §10.6.2, with the min/max rules of §10.4:
//! <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>.
//!
//! An image can lack a natural width, height or aspect ratio (SVG images,
//! CSS Images 3 §5.1: <https://www.w3.org/TR/css-images-3/#natural-dimensions>;
//! a video without a loaded poster and audio lack all three, see
//! `media.rs`). A missing dimension comes from the aspect ratio, or else
//! from the default object size (300×150). An image with only an aspect
//! ratio fills the available width, as in Chromium (CSS 2.2 leaves this
//! case undefined and suggests the same).
//!
//! The aspect ratio is the `aspect-ratio` property, or the natural ratio;
//! with `auto && <ratio>` the natural ratio wins (CSS Sizing 4 §7.1).

use swb_style::ComputedStyle;

use crate::block::{BoxEdges, ContainingBlock, SizeLimits, fill_width, resolve_size};
use crate::box_tree::{IndependentBox, IndependentContents, Replaced};
use crate::geom::clamp_length;

/// The default object size in CSS px
/// (<https://www.w3.org/TR/css-images-3/#default-object-size>).
pub const DEFAULT_OBJECT_SIZE: (f32, f32) = (300.0, 150.0);

/// The natural dimensions of an image. Any of them can be missing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NaturalSize {
    /// The natural width in CSS px.
    pub width: Option<f32>,
    /// The natural height in CSS px.
    pub height: Option<f32>,
    /// The natural aspect ratio (width / height), always positive.
    pub ratio: Option<f32>,
}

impl NaturalSize {
    /// Both dimensions, as raster images have them. The ratio exists if
    /// both are positive.
    pub fn fixed(width: f32, height: f32) -> Self {
        NaturalSize {
            width: Some(width),
            height: Some(height),
            ratio: (width > 0.0 && height > 0.0).then(|| width / height),
        }
    }
}

/// The natural size of an image that is not loaded or is broken.
const NO_IMAGE: NaturalSize = NaturalSize {
    width: Some(0.0),
    height: Some(0.0),
    ratio: None,
};

/// The used content width and height of a replaced element.
pub(crate) fn used_size(
    style: &ComputedStyle,
    replaced: &Replaced,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> (f32, f32) {
    sized(style, replaced, Some(cb.width), cb.height, edges)
}

/// The content width of a replaced element for intrinsic sizing:
/// percentages of the containing block's width count as `auto` (Chromium's
/// `ComputeMinAndMaxContentContributionForReplaced`), and an image with
/// only an aspect ratio contributes 0 unless its height is definite (as in
/// Chromium).
pub(crate) fn intrinsic_width(style: &ComputedStyle, replaced: &Replaced, edges: &BoxEdges) -> f32 {
    sized(style, replaced, None, None, edges).0
}

/// The content width of a replaced element as its natural width (CSS
/// default sizing without a containing block), for the content sizes of
/// the box itself. A definite height gives the width through the aspect
/// ratio (Chromium); otherwise an image with only an aspect ratio is 0
/// wide. The element's own `width`, `min-width` and `max-width` do not
/// count here: in Chromium, a row flex item with only a ratio and
/// `width: 100px` shrinks below 100px (layout test
/// `replaced-ratio-transfer`).
pub(crate) fn natural_content_width(
    style: &ComputedStyle,
    replaced: &Replaced,
    edges: &BoxEdges,
) -> f32 {
    let natural = replaced.natural_size.unwrap_or(NO_IMAGE);
    let ratio = aspect_ratio(style, &natural);
    let v_edges = edges.sum().vertical();
    if let (Some(r), Some(height)) = (
        ratio,
        resolve_size(&style.height, None, style.box_sizing, v_edges),
    ) {
        return clamp_length(SizeLimits::of_height(style, None, v_edges).clamp(height) * r);
    }
    clamp_length(auto_size(&natural, ratio, 0.0).0)
}

fn sized(
    style: &ComputedStyle,
    replaced: &Replaced,
    cb_width: Option<f32>,
    cb_height: Option<f32>,
    edges: &BoxEdges,
) -> (f32, f32) {
    let v_edges = edges.sum().vertical();
    let height = resolve_size(&style.height, cb_height, style.box_sizing, v_edges);
    sized_with_height(style, replaced, height, (cb_width, cb_height), edges)
}

/// [`sized`] with the content height `height` in place of the element's
/// own `height` (`None` for `auto`).
fn sized_with_height(
    style: &ComputedStyle,
    replaced: &Replaced,
    height: Option<f32>,
    (cb_width, cb_height): (Option<f32>, Option<f32>),
    edges: &BoxEdges,
) -> (f32, f32) {
    let h_edges = edges.sum().horizontal();
    let v_edges = edges.sum().vertical();
    let width = resolve_size(&style.width, cb_width, style.box_sizing, h_edges);
    let natural = replaced.natural_size.unwrap_or(NO_IMAGE);
    let ratio = aspect_ratio(style, &natural);

    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (
            w,
            ratio.map_or_else(
                || natural.height.unwrap_or(DEFAULT_OBJECT_SIZE.1),
                |r| w / r,
            ),
        ),
        (None, Some(h)) => (
            ratio.map_or_else(|| natural.width.unwrap_or(DEFAULT_OBJECT_SIZE.0), |r| h * r),
            h,
        ),
        (None, None) => {
            // An image with only an aspect ratio fills the available width;
            // without a containing block (intrinsic sizing) it is 0 wide.
            let available = cb_width.map_or(0.0, |cb| fill_width(style, cb, cb, edges));
            auto_size(&natural, ratio, available)
        }
    };

    // Min/max constraints (CSS 2.2 §10.4); a maximum below the minimum
    // counts as the minimum.
    let w_limits = SizeLimits::of_width(style, cb_width, h_edges);
    let (min_w, max_w) = (w_limits.min(), w_limits.max());
    let h_limits = SizeLimits::of_height(style, cb_height, v_edges);
    let (min_h, max_h) = (h_limits.min(), h_limits.max());
    let limit = |v: f32, min: f32, max: f32| v.min(max).max(min);

    let (w, h) = match (width.is_none(), height.is_none(), ratio) {
        (true, true, Some(r)) => constrain_both(w, h, r, (min_w, max_w), (min_h, max_h)),
        (true, false, Some(r)) => {
            let h = limit(h, min_h, max_h);
            (limit(h * r, min_w, max_w), h)
        }
        (false, true, Some(r)) => {
            let w = limit(w, min_w, max_w);
            (w, limit(w / r, min_h, max_h))
        }
        _ => (limit(w, min_w, max_w), limit(h, min_h, max_h)),
    };
    (clamp_length(w), clamp_length(h))
}

/// The table of CSS 2.2 §10.4 for a replaced element with the aspect ratio
/// `ratio` whose `width` and `height` are both `auto`: `w` × `h` is the
/// size before the constraints, and the result keeps the ratio where the
/// constraints allow it
/// (<https://www.w3.org/TR/CSS22/visudet.html#min-max-widths>). The table
/// uses `w / h`; this uses `ratio`, which is the same when both are
/// positive and stays defined when the size is 0 (an image with only a
/// ratio and no available width; Chromium keeps the ratio there too).
pub(crate) fn constrain_both(
    w: f32,
    h: f32,
    ratio: f32,
    (min_w, max_w): (f32, f32),
    (min_h, max_h): (f32, f32),
) -> (f32, f32) {
    let (over_w, under_w) = (w > max_w, w < min_w);
    let (over_h, under_h) = (h > max_h, h < min_h);
    let width_of = |height: f32| height * ratio;
    let height_of = |width: f32| width / ratio;
    match (over_w, under_w, over_h, under_h) {
        // max-width / w <= max-height / h, with w / h = ratio.
        (true, _, true, _) => {
            if max_w <= width_of(max_h) {
                (max_w, height_of(max_w).max(min_h))
            } else {
                (width_of(max_h).max(min_w), max_h)
            }
        }
        // min-width / w <= min-height / h, with w / h = ratio.
        (_, true, _, true) => {
            if min_w <= width_of(min_h) {
                (width_of(min_h).min(max_w), min_h)
            } else {
                (min_w, height_of(min_w).min(max_h))
            }
        }
        (_, true, true, _) => (min_w, max_h),
        (true, _, _, true) => (max_w, min_h),
        (true, ..) => (max_w, height_of(max_w).max(min_h)),
        (_, true, ..) => (min_w, height_of(min_w).min(max_h)),
        (.., true, _) => (width_of(max_h).max(min_w), max_h),
        (.., true) => (width_of(min_h).min(max_w), min_h),
        _ => (w, h),
    }
}

/// The content height of a flex item that is a replaced element, laid out
/// with the content width `width` (its flexed width in a row, or its cross
/// size in a column): its definite `height`, else `width` through the
/// aspect ratio within `min-height` and `max-height`, else its used height.
pub(crate) fn flex_item_height(
    style: &ComputedStyle,
    replaced: &Replaced,
    width: f32,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> f32 {
    let (_, used) = used_size(style, replaced, cb, edges);
    let v_edges = edges.sum().vertical();
    if resolve_size(&style.height, cb.height, style.box_sizing, v_edges).is_some() {
        return used;
    }
    let Some(ratio) = aspect_ratio(style, &replaced.natural_size.unwrap_or(NO_IMAGE)) else {
        return used;
    };
    clamp_length(SizeLimits::of_height(style, cb.height, v_edges).clamp(width / ratio))
}

/// The content height of an item of a column flex container that is an
/// image with an aspect ratio, for its flex base size and its automatic
/// minimum size (its min- and max-content height): `cross` (its content
/// width, if definite or stretched) through the ratio; otherwise the
/// automatic height without the element's own `height` (CSS 2.2 §10.6.2
/// and §10.4). So a definite `height` can shrink to this height, as in
/// Chromium. `None` for other boxes.
pub(crate) fn column_flex_height(
    ib: &IndependentBox,
    cross: Option<f32>,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> Option<f32> {
    let IndependentContents::Replaced(replaced) = &ib.contents else {
        return None;
    };
    let style = &ib.base.style;
    let ratio = aspect_ratio(style, &replaced.natural_size.unwrap_or(NO_IMAGE))?;
    Some(match cross {
        Some(width) => clamp_length(width / ratio),
        None => sized_with_height(style, replaced, None, (Some(cb.width), cb.height), edges).1,
    })
}

/// The content width of an item of a column flex container that is an
/// image with an aspect ratio and an automatic width that does not
/// stretch: its flexed content height through the ratio, within
/// `min-width` and `max-width` (as in Chromium; it can be wider than the
/// container). `None` for other boxes.
pub(crate) fn column_flex_width(
    ib: &IndependentBox,
    height: f32,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> Option<f32> {
    width_from_height(ib, height, Some(cb.width), edges)
}

/// The content width of an image with an aspect ratio and an automatic
/// width whose content height is `height`: the height through the ratio,
/// within `min-width` and `max-width` (percentages of `cb_width`; without
/// it, a percentage `min-width` is 0 and a percentage `max-width` is
/// `none`). Also for a block, flex or grid container with an
/// `aspect-ratio`. `None` for other boxes.
pub(crate) fn width_from_height(
    ib: &IndependentBox,
    height: f32,
    cb_width: Option<f32>,
    edges: &BoxEdges,
) -> Option<f32> {
    let style = &ib.base.style;
    let IndependentContents::Replaced(replaced) = &ib.contents else {
        // A block, flex or grid container with an `aspect-ratio`.
        return crate::aspect::applies_to(ib)
            .then(|| crate::aspect::width_from_height(style, height, cb_width, edges))
            .flatten();
    };
    if !style.width.is_auto() {
        return None;
    }
    let ratio = aspect_ratio(style, &replaced.natural_size.unwrap_or(NO_IMAGE))?;
    let h_edges = edges.sum().horizontal();
    let limits = SizeLimits::of_width(style, cb_width, h_edges);
    Some(clamp_length(limits.clamp(height * ratio)))
}

/// The max-content width of a row flex item that is an image with only an
/// aspect ratio, in the flex container's content box `cb`: its used width,
/// so it fills the line unless its size is definite (as in Chromium).
/// `None` for other boxes.
pub(crate) fn flex_width(
    ib: &IndependentBox,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> Option<f32> {
    let IndependentContents::Replaced(replaced) = &ib.contents else {
        return None;
    };
    let natural = replaced.natural_size?;
    has_only_ratio(&ib.base.style, &natural)
        .then(|| used_size(&ib.base.style, replaced, cb, edges).0)
}

/// True for an image with an aspect ratio but no natural width or height.
fn has_only_ratio(style: &ComputedStyle, natural: &NaturalSize) -> bool {
    natural.width.is_none() && natural.height.is_none() && aspect_ratio(style, natural).is_some()
}

/// The preferred aspect ratio: the `aspect-ratio` property or the natural
/// ratio (`auto && <ratio>` prefers the natural one), limited so that a
/// length divided or multiplied by it stays finite.
fn aspect_ratio(style: &ComputedStyle, natural: &NaturalSize) -> Option<f32> {
    let max = swb_style::Length::MAX_PX;
    style
        .aspect_ratio
        .preferred(natural.ratio)
        .filter(|r| r.is_finite() && *r > 0.0)
        .map(|r| r.max(1.0 / max).min(max))
}

/// The size when `width` and `height` are both `auto` (CSS 2.2 §10.3.2 and
/// §10.6.2). `stretch` is the available content width. The result is
/// clamped to the layout length limit.
fn auto_size(natural: &NaturalSize, ratio: Option<f32>, stretch: f32) -> (f32, f32) {
    let (width, height) = auto_size_unclamped(natural, ratio, stretch);
    (clamp_length(width), clamp_length(height))
}

fn auto_size_unclamped(natural: &NaturalSize, ratio: Option<f32>, stretch: f32) -> (f32, f32) {
    let width = match (natural.width, natural.height, ratio) {
        (Some(w), _, _) => w,
        (None, Some(h), Some(r)) => h * r,
        (None, None, Some(_)) => stretch,
        _ => DEFAULT_OBJECT_SIZE.0,
    };
    let height = match (natural.height, ratio) {
        (Some(h), _) => h,
        (None, Some(r)) => width / r,
        (None, None) => DEFAULT_OBJECT_SIZE.1,
    };
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn natural(width: Option<f32>, height: Option<f32>, ratio: Option<f32>) -> NaturalSize {
        NaturalSize {
            width,
            height,
            ratio,
        }
    }

    // Expected values measured with Chromium 148 (SVG images in a 400 px
    // wide container).
    #[test]
    fn auto_size_fills_missing_dimensions() {
        let auto = |n: NaturalSize| auto_size(&n, n.ratio, 400.0);
        assert_eq!(auto(NaturalSize::fixed(60.0, 30.0)), (60.0, 30.0));
        assert_eq!(auto(natural(None, None, Some(2.0))), (400.0, 200.0));
        assert_eq!(auto(natural(Some(60.0), None, None)), (60.0, 150.0));
        assert_eq!(auto(natural(None, Some(30.0), None)), (300.0, 30.0));
        assert_eq!(auto(natural(Some(60.0), None, Some(2.0))), (60.0, 30.0));
        assert_eq!(auto(natural(None, Some(30.0), Some(2.0))), (60.0, 30.0));
        assert_eq!(auto(natural(None, None, None)), (300.0, 150.0));
        assert_eq!(auto(NaturalSize::fixed(0.0, 20.0)), (0.0, 20.0));
    }

    #[test]
    fn extreme_ratios_give_finite_sizes() {
        let max = swb_style::Length::MAX_PX;
        // viewBox="0 0 1e-18 1e19": a tiny ratio with only a width.
        let tall = auto_size(&natural(Some(100.0), None, Some(1e-37)), Some(1e-37), 0.0);
        assert_eq!(tall, (100.0, max));
        let wide = auto_size(&natural(None, Some(100.0), Some(1e37)), Some(1e37), 0.0);
        assert_eq!(wide, (max, 100.0));
        let style = ComputedStyle::initial();
        let ratio = aspect_ratio(&style, &natural(None, None, Some(1e37)));
        assert_eq!(ratio, Some(max));
    }

    /// CSS 2.2 §10.4 for a 100×50 image, measured with Chromium 148.
    #[test]
    fn min_max_constraints_keep_the_ratio() {
        let c = |min_w: f32, max_w: f32, min_h: f32, max_h: f32| {
            constrain_both(100.0, 50.0, 2.0, (min_w, max_w), (min_h, max_h))
        };
        let inf = f32::INFINITY;
        assert_eq!(c(0.0, 80.0, 0.0, 20.0), (40.0, 20.0));
        assert_eq!(c(0.0, 30.0, 0.0, 40.0), (30.0, 15.0));
        assert_eq!(c(200.0, inf, 120.0, inf), (240.0, 120.0));
        assert_eq!(c(150.0, inf, 0.0, 40.0), (150.0, 40.0));
        assert_eq!(c(0.0, 50.0, 60.0, inf), (50.0, 60.0));
        assert_eq!(c(0.0, inf, 70.0, inf), (140.0, 70.0));
        assert_eq!(c(0.0, inf, 0.0, inf), (100.0, 50.0));
    }

    /// An image with only a 2:1 ratio and no available width is 0 × 0
    /// before the constraints; Chromium 148 still keeps the ratio
    /// (layout test `replaced-ratio-transfer`).
    #[test]
    fn min_max_constraints_keep_the_ratio_of_empty_sizes() {
        let c = |min_w: f32, max_w: f32, min_h: f32, max_h: f32| {
            constrain_both(0.0, 0.0, 2.0, (min_w, max_w), (min_h, max_h))
        };
        let inf = f32::INFINITY;
        assert_eq!(c(100.0, inf, 0.0, inf), (100.0, 50.0));
        assert_eq!(c(0.0, inf, 40.0, inf), (80.0, 40.0));
        assert_eq!(c(100.0, inf, 0.0, 20.0), (100.0, 20.0));
        assert_eq!(c(100.0, inf, 40.0, inf), (100.0, 50.0));
        assert_eq!(c(60.0, inf, 40.0, inf), (80.0, 40.0));
        assert_eq!(c(0.0, inf, 0.0, inf), (0.0, 0.0));
    }

    #[test]
    fn fixed_sizes_have_a_ratio_only_if_positive() {
        assert_eq!(NaturalSize::fixed(4.0, 2.0).ratio, Some(2.0));
        assert_eq!(NaturalSize::fixed(0.0, 2.0).ratio, None);
    }
}
