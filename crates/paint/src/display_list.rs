//! The display list: drawing commands in CSS px, in paint order.
//!
//! Built from the fragment tree by [`build_display_list`]. Paint order
//! follows a simplified form of CSS 2.2 Appendix E
//! (<https://www.w3.org/TR/CSS22/zindex.html>): a stacking context paints
//! its normal-flow content in three phases (backgrounds and borders of
//! in-flow block boxes, then floats, then inline content, each in tree
//! order), then positioned and transformed boxes in z-index order (`auto`
//! counts as 0), and in tree order for equal values. Floats, atomic
//! inline-level boxes and flex and grid items (CSS Flexbox 1 §5.4) paint
//! all their phases as a unit, as if they established a stacking context;
//! their positioned descendants belong to the enclosing stacking context.
//! A box with `z-index: auto` and `position: relative` or `absolute`
//! (without a transform or opacity) does not form a stacking context: its
//! positioned descendants take part in the enclosing one.
//! Deliberate simplifications: a box with opacity < 1, a `filter` or a mask
//! that is not positioned paints as a unit in the inline content phase (Appendix E
//! paints it with the positioned boxes); outlines are painted after each
//! box's content, not in a last phase; the image of a block-level replaced
//! element is painted with its background; a float inside a positioned
//! inline box paints in the float phase of the inline box's container, not
//! with the inline box's stacking context; an inline box split over lines
//! has an opacity, filter or mask group per line fragment, not one per
//! element.
//!
//! Transforms, fixed boxes and sticky boxes are transform groups
//! ([`DisplayItem::PushTransform`]). The offsets of fixed and sticky boxes
//! depend on the scroll offset, so the rasterizer and hit testing compute
//! them; the list does not change when the page scrolls. A positioned box
//! keeps only the overflow clips of the boxes in its containing block
//! chain (CSS Overflow 3 §3): an absolutely positioned box is not clipped
//! by a box between it and its containing block, and a fixed box only by
//! the `clip` properties of its ancestors (and the clips of a transformed
//! containing block). A stacking
//! context paints its positioned descendants outside its own overflow
//! clip; each of them repeats the clips that apply to it. Deliberate
//! simplification: the clips around a stacking context also clip the
//! positioned descendants in it whose containing block is outside those
//! clips (a relative containing block, then a static box with
//! `overflow: hidden`, then a box with opacity, then the absolutely
//! positioned box: the overflow clip applies; ADR 0016).
//!
//! The builder collects the items in chunks (`rope.rs`), so that moving
//! the items of positioned boxes is linear, and computes the bounds of
//! all groups in one pass at the end (`group_bounds.rs`).
//!
//! The list also contains hit regions in paint order, so that hit testing
//! ([`DisplayList::hit_test`]) finds what is painted on top.
//!
//! The content of a scroll container is moved by its scroll offset inside
//! its clip; its own background and border stay (CSS Overflow 3 §2.3,
//! <https://www.w3.org/TR/css-overflow-3/#scrolling>). The offsets come
//! from the engine ([`Scrolling`]); a change of an offset rebuilds the
//! display list, not the layout. `swb_layout::ScrollState` decides which
//! boxes move (not the absolutely positioned boxes whose containing block
//! is outside the scroll container).

mod builder;
mod items;

pub use builder::build_display_list;
pub(crate) use builder::{layer_value, outset, path_clip_item, resolve_radii, with_descendants};
pub use items::{
    DisplayItem, DisplayList, Highlights, Hit, ImageRef, ImageSizes, NoHighlights, Radii,
    SELECTION_BACKGROUND, Scrolling,
};
pub(crate) use items::{UNBOUNDED, path_bounds, stroke_extent};

#[cfg(test)]
mod tests;
