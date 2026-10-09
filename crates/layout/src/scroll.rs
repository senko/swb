//! Scroll containers (CSS Overflow 3, <https://www.w3.org/TR/css-overflow-3/>):
//! the scrollable overflow rectangle of each scroll container, and the
//! positions of fragments when scroll containers are scrolled.
//!
//! Layout stores the scrollable overflow rectangle (§2.2,
//! <https://www.w3.org/TR/css-overflow-3/#scrollable>) in the fragment of
//! each scroll container ([`BoxFragment::scrollable_overflow`]). It is
//! computed as Chromium computes it (measured with Chromium 148), the union
//! of:
//!
//! - the padding box;
//! - the in-flow content with the padding around it (the "end padding" of
//!   §2.2): the margin boxes of the in-flow children and floats before
//!   relative positioning, the line boxes up to the end of their content,
//!   and the auto height of the content (with the collapsed margins of the
//!   last child). Flex items count with their margin boxes; the in-flow
//!   content of a grid container is its tracks after content alignment
//!   (its items count only as descendants, below);
//! - the border boxes of the descendants for which the scroll container is
//!   in the chain of containing blocks, with their own overflow if it is
//!   `visible` (limited to their padding box on axes with `overflow:
//!   clip`), without margins or padding; a transformed box and its
//!   content with their transformed bounds. Text counts with its rectangle.
//!   On a line where white space hangs at the end, text and inline boxes
//!   count only up to the end of the line's content
//!   ([`text_overflow_rect`], [`box_overflow_rect`]). Boxes with zero width
//!   or height do not count.
//!
//! The parts above and to the left of the padding box cannot be scrolled
//! to (the unreachable scrollable overflow region; swb has only
//! left-to-right, top-to-bottom writing modes), so the rectangle starts at
//! the padding box.
//!
//! Scroll offsets are engine state, keyed by the element of the scroll
//! container ([`ScrollOffsets`]). [`ScrollState`] applies them during a
//! walk of the fragment tree: the content of a scroll container moves by
//! minus its offset, except the absolutely positioned and fixed
//! descendants whose containing block is outside the scroll container (a
//! fixed box without a transformed ancestor never moves). Paint, hit
//! testing and the engine's element positions use the same rule. A
//! positioned inline box around blocks is not an ancestor of the blocks in
//! the fragment tree; the blocks carry
//! [`BoxFragment::in_positioned_inline`] instead.

use std::collections::HashMap;

use swb_dom::NodeId;
use swb_style::{ComputedStyle, Position, WhiteSpace};

use crate::block::{ContainingBlock, margin_or_zero, relative_offset};
use crate::fragment::{BoxContent, BoxFragment, Fragment, TextFragment};
use crate::geom::{Matrix, Point, Rect, Size, clamp_length};
use crate::positioned::{
    is_absolute_containing_block, is_fixed_containing_block, transform_matrix,
};

/// The scroll offsets of scroll containers, supplied by the engine.
pub trait ScrollOffsets {
    /// The scroll offset of the scroll container of element `node` (zero if
    /// it is not scrolled). Positive values scroll the content up and to
    /// the left.
    fn scroll_offset(&self, node: NodeId) -> Point;
}

/// No scroll container is scrolled.
pub struct NoScroll;

impl ScrollOffsets for NoScroll {
    fn scroll_offset(&self, _node: NodeId) -> Point {
        Point::default()
    }
}

impl<S: std::hash::BuildHasher> ScrollOffsets for HashMap<NodeId, Point, S> {
    fn scroll_offset(&self, node: NodeId) -> Point {
        self.get(&node).copied().unwrap_or_default()
    }
}

/// The largest scroll offset on each axis for content of `content` px in
/// a scrollport of `port` px. Whole pixels, as in Chromium 148: both sizes
/// are rounded first (content of 200.4 px in a 100 px scrollport gives
/// 100, a page of 1200.7 px in a 600 px viewport gives 601).
pub fn scroll_range(content: Size, port: Size) -> Point {
    Point::new(
        (content.width.round() - port.width.round()).max(0.0),
        (content.height.round() - port.height.round()).max(0.0),
    )
}

/// `offset` clamped to `0..=max` on each axis; NaN becomes 0. `max` must
/// not be negative.
pub fn clamp_scroll_offset(offset: Point, max: Point) -> Point {
    let clamp = |v: f32, max: f32| if v.is_nan() { 0.0 } else { v.min(max).max(0.0) };
    Point::new(clamp(offset.x, max.x), clamp(offset.y, max.y))
}

/// True if a box with this style is a scroll container (CSS Overflow 3
/// §3, <https://www.w3.org/TR/css-overflow-3/#scroll-container>). Layout
/// gives only block, flex and grid containers a scrollable overflow
/// rectangle; replaced elements, tables, table cells and form controls
/// only clip (ADR 0019).
pub(crate) fn is_scroll_container(style: &ComputedStyle) -> bool {
    style.overflow_x.is_scroll_container() || style.overflow_y.is_scroll_container()
}

/// True if the containing block of the absolutely positioned boxes in `b`
/// is `b` or a box below the parent of `b`: `b` is positioned or
/// transformed, or it is a block inside a positioned inline box, whose
/// fragments are siblings of `b`.
fn contains_absolute_box(b: &BoxFragment) -> bool {
    is_absolute_containing_block(b) || b.in_positioned_inline
}

/// The scroll offsets that apply to the boxes during a walk of the
/// fragment tree. Start with the default state at the root.
///
/// A scroll container moves the boxes for which it is in the chain of
/// containing blocks. An absolutely positioned box whose containing block
/// (the nearest positioned or transformed ancestor) is outside a scroll
/// container does not move with it; a fixed box moves only with the
/// scroll containers above its containing block (the nearest transformed
/// ancestor), so without one with no element.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    /// The offsets of the scroll containers below the nearest positioned
    /// or transformed ancestor (and not that ancestor itself): an
    /// absolutely positioned box does not move with them.
    since_positioned: Point,
    /// The offsets of the scroll containers below the nearest transformed
    /// ancestor (all of them without one), since the nearest fixed box: a
    /// fixed box does not move with them.
    since_fixed: Point,
}

impl ScrollState {
    /// The state at the root of the fragment tree (also the `Default`).
    pub const DEFAULT: ScrollState = ScrollState {
        since_positioned: Point::new(0.0, 0.0),
        since_fixed: Point::new(0.0, 0.0),
    };

    /// The absolute position that box `b` is placed against: `origin` is
    /// the absolute border-box origin of its parent with the parent's
    /// scroll offsets applied (as [`ScrollState::enter`] returns it). For
    /// an absolutely positioned or fixed box, the offsets of the scroll
    /// containers that are not in its chain of containing blocks are
    /// taken back.
    pub fn origin_of(&self, b: &BoxFragment, origin: Point) -> Point {
        match b.style.position {
            Position::Fixed => origin + self.since_fixed,
            Position::Absolute => origin + self.since_positioned,
            _ => origin,
        }
    }

    /// The origin for the children of `b` and the state for them.
    /// `border_origin` is the absolute border-box origin of `b` (from
    /// [`ScrollState::origin_of`]). The children of a scroll container move
    /// by minus its offset (clamped to its scroll range).
    pub fn enter(
        &self,
        b: &BoxFragment,
        border_origin: Point,
        offsets: &dyn ScrollOffsets,
    ) -> (Point, ScrollState) {
        let mut state = *self;
        match b.style.position {
            Position::Fixed => state = ScrollState::default(),
            // The box itself did not move with the scroll containers below
            // its containing block.
            Position::Absolute => {
                state.since_fixed = state.since_fixed - state.since_positioned;
            }
            _ => {}
        }
        if contains_absolute_box(b) {
            state.since_positioned = Point::default();
        }
        let fixed_containing_block = is_fixed_containing_block(b);
        if fixed_containing_block {
            state.since_fixed = Point::default();
        }
        let offset = b.scroll_offset(offsets);
        if offset == Point::default() {
            return (border_origin, state);
        }
        // Boxes whose containing block is `b` move with its content.
        if !fixed_containing_block {
            state.since_fixed = state.since_fixed + offset;
        }
        if !is_absolute_containing_block(b) {
            state.since_positioned = state.since_positioned + offset;
        }
        (border_origin - offset, state)
    }
}

/// The size of the scrollable area: the union of the viewport and all
/// content that is not clipped, with transforms (as in Chromium). Also on
/// an axis where the viewport's overflow is `hidden` (or `clip`, which
/// applies as `hidden` to the viewport): scripts can scroll it there, the
/// user cannot (as in Chromium). Boxes fixed to the viewport do not count,
/// and an absolutely positioned or fixed box is clipped only by the boxes
/// in its containing block chain. Content after the end of a line where
/// white space hangs does not count ([`text_overflow_rect`],
/// [`box_overflow_rect`]). Rectangles with zero width or height
/// count with their position: Chromium counts the line boxes of zero-size
/// inline content (but not a zero-size block, a deviation).
pub(crate) fn viewport_scroll_size(root: Option<&BoxFragment>, viewport: Size) -> Size {
    /// Whether the content of a box is clipped, and whether the content
    /// of the containing blocks of its absolutely positioned and fixed
    /// descendants is.
    #[derive(Clone, Copy, Default)]
    struct Clipped {
        content: bool,
        absolute: bool,
        fixed: bool,
    }
    /// The state of the walk above a box.
    #[derive(Clone, Copy)]
    struct Above<'m> {
        origin: Point,
        matrix: &'m Matrix,
        clipped: Clipped,
        /// True if an ancestor is transformed (fixed boxes count).
        transformed: bool,
        /// The end of the line's content for inline content on a line with
        /// hanging white space ([`child_limit`]).
        limit: Option<f32>,
    }
    fn visit(b: &BoxFragment, above: Above<'_>, extent: &mut Rect) {
        let rect = b.border_rect.translate(above.origin);
        let state = above.clipped;
        let clipped = match b.style.position {
            Position::Fixed if !above.transformed => return,
            Position::Fixed => state.fixed,
            Position::Absolute => state.absolute,
            _ => state.content,
        };
        let own = transform_matrix(b, rect).map(|t| above.matrix.multiply(&t));
        let matrix = own.as_ref().unwrap_or(above.matrix);
        if !clipped {
            let area = box_overflow_rect(b, above.limit).translate(above.origin);
            *extent = extent.union(&matrix.map_rect(&area));
        }
        let content = clipped || (b.style.overflow_x.clips() && b.style.overflow_y.clips());
        let inner = Clipped {
            content,
            absolute: if is_absolute_containing_block(b) {
                content
            } else if b.in_positioned_inline {
                // The containing block is the inline box around `b`.
                clipped
            } else {
                state.absolute
            },
            fixed: if is_fixed_containing_block(b) {
                content
            } else {
                state.fixed
            },
        };
        let transformed = above.transformed || own.is_some();
        // Nothing inside can extend the area: all content is clipped (fixed
        // boxes without a transformed ancestor never count).
        if inner.content && inner.absolute && (inner.fixed || !transformed) {
            return;
        }
        let below = Above {
            origin: rect.origin(),
            matrix,
            clipped: inner,
            transformed,
            limit: child_limit(b, above.limit),
        };
        for child in b.children.iter() {
            match child {
                Fragment::Box(child) => visit(child, below, extent),
                Fragment::Text(t) if !content => {
                    let text = text_overflow_rect(t, below.limit).translate(rect.origin());
                    *extent = extent.union(&matrix.map_rect(&text));
                }
                Fragment::Text(_) => {}
            }
        }
    }
    let mut extent = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    if let Some(root) = root {
        let above = Above {
            origin: Point::default(),
            matrix: &Matrix::IDENTITY,
            clipped: Clipped::default(),
            transformed: false,
            limit: None,
        };
        visit(root, above, &mut extent);
    }
    let width = extent.right().max(viewport.width);
    let height = extent.bottom().max(viewport.height);
    Size::new(clamp_length(width), clamp_length(height))
}

/// The scrollable overflow rectangle of scroll container `b`, relative to
/// its border-box origin (see the module documentation). `inflow` is the
/// extent of its in-flow content from the content-box origin
/// ([`crate::block::ChildrenLayout::inflow`]).
pub(crate) fn scrollable_overflow(b: &BoxFragment, inflow: Size) -> Rect {
    let padding_box = b.scrollport();
    // The in-flow content starts at the content box; with the padding
    // around it, at the padding box.
    let extent = Extent {
        left: padding_box.x,
        top: padding_box.y,
        right: padding_box
            .right()
            .max(padding_box.x + b.padding.horizontal() + inflow.width),
        bottom: padding_box
            .bottom()
            .max(padding_box.y + b.padding.vertical() + inflow.height),
    };
    with_descendants(b, extent)
}

/// The scrollable overflow rectangle of scroll container `b` after its
/// absolutely positioned descendants were laid out (they are placeholders
/// when `block::layout_sized` computes the rectangle): its rectangle with
/// the descendants added again.
pub(crate) fn with_out_of_flow(b: &BoxFragment, overflow: Rect) -> Rect {
    let extent = Extent {
        left: overflow.x,
        top: overflow.y,
        right: overflow.right(),
        bottom: overflow.bottom(),
    };
    with_descendants(b, extent)
}

/// `extent` with the contributions of the descendants of scroll container
/// `b`, as a rectangle from its padding box.
fn with_descendants(b: &BoxFragment, mut extent: Extent) -> Rect {
    let padding_box = b.scrollport();
    // Not `contains_absolute_box`: the positioned inline box around a
    // scroll container is outside it.
    let contains = Contains {
        absolute: is_absolute_containing_block(b),
        fixed: is_fixed_containing_block(b),
    };
    for child in b.children.iter() {
        add_contribution(child, Point::default(), contains, None, &mut extent);
    }
    Rect::new(
        padding_box.x,
        padding_box.y,
        extent.right - padding_box.x,
        extent.bottom - padding_box.y,
    )
}

/// The rectangle of a text fragment for scrollable overflow (relative to
/// its parent): without the white space at its end with `white-space:
/// pre-wrap`, which can hang at the end of a line (CSS Text 3 §4.1.3,
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>; white space at
/// the end of a fragment in the middle of a line or that fits is inside
/// other content or the line, so leaving it out changes nothing, except in
/// a relatively positioned inline box), and only up to `limit`, the end of
/// its line's content when white space hangs there ([`child_limit`]). The
/// viewport's scroll size uses it too.
pub(crate) fn text_overflow_rect(t: &TextFragment, limit: Option<f32>) -> Rect {
    if t.style.white_space != WhiteSpace::PreWrap {
        return clamp_right(t.rect, limit);
    }
    let hanging = t
        .text
        .chars()
        .rev()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .count();
    // Spaces and tabs have one glyph each.
    let Some(first) = t.glyphs.len().checked_sub(hanging) else {
        return clamp_right(t.rect, limit);
    };
    let rect = match t.glyphs.get(first) {
        Some(g) if hanging > 0 => Rect::new(
            t.rect.x,
            t.rect.y,
            // Not `clamp`: the width is negative with a negative
            // `letter-spacing`.
            g.x.min(t.rect.width).max(0.0),
            t.rect.height,
        ),
        _ => t.rect,
    };
    clamp_right(rect, limit)
}

/// The border box of a box fragment for scrollable overflow (relative to
/// its parent): for an inline box, only up to the end of its line's
/// content when white space hangs at the end of the line
/// ([`BoxFragment::hanging_from`], and `limit` from an enclosing inline
/// box). Atomic inlines and blocks count whole. The viewport's scroll size
/// uses it too.
pub(crate) fn box_overflow_rect(b: &BoxFragment, limit: Option<f32>) -> Rect {
    let rect = b.border_rect;
    if !b.is_inline {
        return rect;
    }
    // The own end in the parent's coordinates.
    let own = b.hanging_from.map(|end| rect.x + end);
    let limit = match (own, limit) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    clamp_right(rect, limit)
}

/// The limit for the children of `b` (in its coordinates) on a line with
/// hanging white space: the end of the line's content. Only inline boxes
/// pass it on; `limit` is the limit for `b` itself (in its parent's
/// coordinates).
pub(crate) fn child_limit(b: &BoxFragment, limit: Option<f32>) -> Option<f32> {
    if !b.is_inline {
        return None;
    }
    let inherited = limit.map(|l| l - b.border_rect.x);
    match (inherited, b.hanging_from) {
        (Some(a), Some(h)) => Some(a.min(h)),
        (a, h) => a.or(h),
    }
}

/// `r` without the part to the right of `limit`; a rectangle wholly to
/// the right of it becomes empty at `limit` (the viewport's scroll size
/// also counts the positions of empty rectangles).
fn clamp_right(mut r: Rect, limit: Option<f32>) -> Rect {
    if let Some(limit) = limit {
        let right = r.right().min(limit);
        r.x = r.x.min(limit);
        r.width = (right - r.x).max(0.0);
    }
    r
}

/// The edges of the scrollable overflow. The rectangle starts at the
/// padding box; the left and top edges count for the transformed bounds
/// of transformed boxes.
#[derive(Clone, Copy, Debug)]
struct Extent {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Extent {
    const NONE: Extent = Extent {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };

    /// Adds a rectangle; one with zero width or height does not count (as
    /// in Chromium).
    fn add(&mut self, r: Rect) {
        if r.width > 0.0 && r.height > 0.0 {
            self.left = self.left.min(r.x);
            self.top = self.top.min(r.y);
            self.right = self.right.max(r.right());
            self.bottom = self.bottom.max(r.bottom());
        }
    }

    /// Adds another extent.
    fn add_extent(&mut self, other: Extent) {
        self.left = self.left.min(other.left);
        self.top = self.top.min(other.top);
        self.right = self.right.max(other.right);
        self.bottom = self.bottom.max(other.bottom);
    }

    /// The extent as a rectangle, if it is not empty.
    fn rect(&self) -> Option<Rect> {
        (self.right > self.left && self.bottom > self.top).then(|| {
            Rect::new(
                self.left,
                self.top,
                self.right - self.left,
                self.bottom - self.top,
            )
        })
    }
}

/// Whether a box between a scroll container (included) and a fragment
/// contains absolutely positioned boxes, and whether one contains fixed
/// boxes: such boxes count in the scroll container.
#[derive(Clone, Copy, Debug)]
struct Contains {
    absolute: bool,
    fixed: bool,
}

/// Adds what fragment `f` contributes to the scrollable overflow of a
/// scroll container: its border box (or text rectangle) and, if it does
/// not clip, the contributions of its children; a transformed box with the
/// transformed bounds of both (as in Chromium). `origin` is the position
/// of its parent's border-box origin relative to the scroll container's;
/// `contains` says which positioned boxes have their containing block
/// inside the scroll container; `limit` is the end of the line's content
/// for inline content on a line with hanging white space ([`child_limit`]).
///
/// The recursion depth is bounded by the box tree depth (see
/// `box_tree.rs`).
fn add_contribution(
    f: &Fragment,
    origin: Point,
    contains: Contains,
    limit: Option<f32>,
    extent: &mut Extent,
) {
    let b = match f {
        Fragment::Text(t) => {
            extent.add(text_overflow_rect(t, limit).translate(origin));
            return;
        }
        Fragment::Box(b) => b,
    };
    // A box whose containing block is outside the scroll container belongs
    // to the scrollable overflow of that containing block.
    match b.style.position {
        Position::Fixed if !contains.fixed => return,
        Position::Absolute if !contains.absolute => return,
        _ => {}
    }
    let rect = b.border_rect.translate(origin);
    match transform_matrix(b, rect) {
        Some(m) => {
            let mut own = Extent::NONE;
            add_box(b, origin, contains, limit, &mut own);
            if let Some(r) = own.rect() {
                extent.add(m.map_rect(&r));
            }
        }
        None => add_box(b, origin, contains, limit, extent),
    }
}

/// Adds the border box of `b` and the contributions of its children (see
/// [`add_contribution`]), without its transform.
fn add_box(
    b: &BoxFragment,
    origin: Point,
    contains: Contains,
    limit: Option<f32>,
    extent: &mut Extent,
) {
    let rect = b.border_rect.translate(origin);
    if b.content != BoxContent::GeometryOnly {
        extent.add(box_overflow_rect(b, limit).translate(origin));
    }
    let style = &b.style;
    // Hidden contents (paint containment) are clipped.
    if style.contents_hidden || style.overflow_x.clips() && style.overflow_y.clips() {
        return;
    }
    let mut inner = Extent::NONE;
    let contains = Contains {
        absolute: contains.absolute || contains_absolute_box(b),
        fixed: contains.fixed || is_fixed_containing_block(b),
    };
    let limit = child_limit(b, limit);
    for child in b.children.iter() {
        add_contribution(child, rect.origin(), contains, limit, &mut inner);
    }
    // `overflow: clip` on one axis clips the content at the padding box on
    // that axis (`overflow-clip-margin` is not supported).
    let padding_box = b.padding_rect().translate(origin);
    if style.overflow_x.clips() {
        inner.left = inner.left.max(padding_box.x);
        inner.right = inner.right.min(padding_box.right());
    }
    if style.overflow_y.clips() {
        inner.top = inner.top.max(padding_box.y);
        inner.bottom = inner.bottom.min(padding_box.bottom());
    }
    extent.add_extent(inner);
}

/// The in-flow extent of the children of a block container
/// ([`crate::block::ChildrenLayout::inflow`]), collected while block
/// layout places them.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InflowExtent(Size);

impl InflowExtent {
    /// Adds a block-level child after it is positioned (with its relative
    /// offset): in-flow blocks and floats count with their margin boxes;
    /// blocks inside inline boxes (`in_inline`) and absolutely positioned
    /// boxes do not count.
    pub(crate) fn add(&mut self, b: &BoxFragment, cb: ContainingBlock, in_inline: bool) {
        if in_inline || b.style.is_absolutely_positioned() {
            return;
        }
        let (right, bottom) = margin_box_end(b, cb);
        self.0.width = self.0.width.max(right);
        self.0.height = self.0.height.max(bottom);
    }

    /// The extent, at least as high as the content.
    pub(crate) fn finish(self, content_height: f32) -> Size {
        Size::new(self.0.width, self.0.height.max(content_height))
    }
}

/// The right and bottom margin edges of the in-flow boxes and floats among
/// `fragments` (relative to the container's content box), before relative
/// positioning: their contribution to the in-flow content of a scroll
/// container. Absolutely positioned boxes do not count; text fragments
/// count with their rectangle. `cb` is the containing block of the
/// fragments.
pub(crate) fn margin_box_extent(fragments: &[Fragment], cb: ContainingBlock) -> Size {
    let mut extent = Size::default();
    for f in fragments {
        let (right, bottom) = match f {
            Fragment::Text(t) => (t.rect.right(), t.rect.bottom()),
            Fragment::Box(b) if b.style.is_absolutely_positioned() => continue,
            Fragment::Box(b) => margin_box_end(b, cb),
        };
        extent.width = extent.width.max(right);
        extent.height = extent.height.max(bottom);
    }
    extent
}

/// The right and bottom margin edges of a fragment before relative
/// positioning. `auto` margins count as zero; the right margin of an
/// over-constrained block counts with its specified value (as in
/// Chromium).
fn margin_box_end(b: &BoxFragment, cb: ContainingBlock) -> (f32, f32) {
    let (dx, dy) = relative_offset(&b.style, cb);
    let right = b.border_rect.right() - dx + margin_or_zero(&b.style.margin_right, cb.width);
    let bottom = b.border_rect.bottom() - dy + margin_or_zero(&b.style.margin_bottom, cb.width);
    (right, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{body, layout_html};

    /// The scrollable overflow size of the scroll container `#s`.
    fn overflow_size(html: &str) -> (f32, f32) {
        let l = layout_html(&body(html));
        let node = l.node("s");
        let mut found = None;
        l.tree.walk(|f, _| {
            if let crate::FragmentRef::Box(b) = f
                && b.node == Some(node)
            {
                found = b.scrollable_overflow;
            }
        });
        let r = found.expect("#s is a scroll container");
        (r.width, r.height)
    }

    // The expected values are Chromium's `scrollWidth` and `scrollHeight`
    // (Chromium 148).

    const S: &str = "id=s style='overflow:auto;width:100px;height:100px;padding:10px";

    #[test]
    fn floats_count_with_margins_and_end_padding() {
        let html = format!(
            "<div {S}'><div style='float:left;width:50px;height:300px;\
             margin-bottom:20px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (120.0, 340.0));
        // Next to line boxes, a float counts as a descendant.
        let html = format!(
            "<div {S}'>text <span style='float:right;width:50px;height:250px;\
             margin-bottom:15px'></span> more text</div>"
        );
        assert_eq!(overflow_size(&html), (120.0, 260.0));
    }

    #[test]
    fn in_flow_children_count_with_margins_and_end_padding() {
        let html = format!("<div {S}'><div style='height:200px;margin-bottom:30px'></div></div>");
        assert_eq!(overflow_size(&html), (120.0, 250.0));
        let html = format!(
            "<div {S}'><div style='width:300px;height:20px;margin-right:30px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (350.0, 120.0));
    }

    #[test]
    fn descendants_count_without_margins_or_padding() {
        let html = format!(
            "<div {S}'><div style='height:50px'><div style='height:300px;width:250px;\
             margin:0 40px 40px 0'></div></div></div>"
        );
        assert_eq!(overflow_size(&html), (260.0, 310.0));
    }

    #[test]
    fn relative_offsets_move_the_border_box_but_not_the_in_flow_bounds() {
        let html = format!(
            "<div {S}'><div style='position:relative;top:100px;left:100px;height:50px;\
             width:50px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (160.0, 160.0));
        let html = format!(
            "<div {S}'><div style='position:relative;top:-100px;left:-100px;height:300px;\
             width:300px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (320.0, 320.0));
    }

    #[test]
    fn content_above_and_left_of_the_padding_box_is_unreachable() {
        let html = format!(
            "<div {S}'><div style='margin:-50px 0 0 -50px;width:300px;height:300px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (270.0, 270.0));
    }

    #[test]
    fn border_is_outside_the_scrollable_overflow() {
        let html = format!(
            "<div {S};border:5px solid'><div style='height:200px;width:200px'></div></div>"
        );
        let l = layout_html(&body(&html));
        let mut overflow = None;
        l.tree.walk(|f, _| {
            if let crate::FragmentRef::Box(b) = f
                && b.node == Some(l.node("s"))
            {
                overflow = b.scrollable_overflow;
            }
        });
        assert_eq!(overflow, Some(Rect::new(5.0, 5.0, 220.0, 220.0)));
    }

    #[test]
    fn absolutely_positioned_boxes_count_only_inside_their_containing_block() {
        let html = format!(
            "<div {S}'><div style='position:absolute;top:200px;left:200px;height:50px;\
             width:50px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (120.0, 120.0));
        let html = format!(
            "<div {S}'><div style='position:relative;height:20px'><div style='position:absolute;\
             height:50px;width:300px'></div></div></div>"
        );
        assert_eq!(overflow_size(&html).0, 310.0);
        let html =
            format!("<div {S}'><div style='position:fixed;height:50px;width:300px'></div></div>");
        assert_eq!(overflow_size(&html), (120.0, 120.0));
    }

    #[test]
    fn lines_count_up_to_their_content_with_end_padding() {
        // A long word: its width plus both paddings.
        let (width, _) = overflow_size(&format!("<div {S}'>{}</div>", "a".repeat(26)));
        let (nested, _) = overflow_size(&format!("<div {S}'><div>{}</div></div>", "a".repeat(26)));
        assert!(width > 200.0);
        assert_eq!(width, nested + 10.0);
        // Three lines of 20 px in a 30 px box.
        let html = "<div id=s style='overflow:auto;width:100px;height:30px;padding:10px'>a<br>b<br>c</div>";
        assert_eq!(overflow_size(html), (120.0, 80.0));
        // The end margin of an inline box counts.
        let html = format!(
            "<div {S};white-space:nowrap'><span id=a style='margin-right:50px'>{}</span></div>",
            "a".repeat(16)
        );
        let l = layout_html(&body(&html));
        let span = l.rect("a");
        assert_eq!(overflow_size(&html).0, span.right() + 50.0 + 10.0);
    }

    #[test]
    fn hanging_spaces_do_not_count() {
        // Chromium 148: 116 px (the padding box); with `pre` the spaces
        // count.
        let html = |white_space: &str| {
            format!(
                "<div id=s style='overflow:auto;width:100px;height:60px;\
                 padding:7px 11px 13px 5px;white-space:{white_space};font:16px/20px monospace'>\
                 aaaaaaaaaa            </div>"
            )
        };
        assert_eq!(overflow_size(&html("pre-wrap")).0, 116.0);
        assert!(overflow_size(&html("pre")).0 > 200.0);
        // Chromium 148: 100 px also when the spaces are in an inline box,
        // whose border box covers them.
        let spaces = " ".repeat(18);
        let html = format!(
            "<div id=s style='white-space:pre-wrap;overflow:auto;width:100px;\
             font:16px monospace'>aa <span>bb{spaces}</span></div>"
        );
        assert_eq!(overflow_size(&html).0, 100.0);
        // Hanging tabs too (Chromium 148: 240 px for 25 characters).
        let html = format!(
            "<div id=s style='white-space:pre-wrap;overflow:auto;width:100px;\
             font:16px monospace'>{}\t\t</div>",
            "a".repeat(25)
        );
        let width = overflow_size(&html).0;
        assert!((239.0..241.0).contains(&width), "{width}");
        // The container's left padding moves the line (Chromium 148: 349).
        let html = "<div id=s style='overflow:auto;width:300px;height:60px;font:16px monospace'>\
                    <div style='width:50px;padding-left:20px;white-space:pre-wrap'>aaa\
                    <span style='padding-left:300px'>     </span></div></div>";
        let width = overflow_size(html).0;
        assert!((348.0..350.0).contains(&width), "{width}");
        // Relatively positioned inline content counts only up to the end
        // of the unshifted line (Chromium 148: 134 and 100).
        let relative = |content: &str| {
            overflow_size(&format!(
                "<div id=s style='overflow:auto;width:100px;height:60px;\
                 white-space:pre-wrap;font:16px monospace'>{content}</div>"
            ))
            .0
        };
        let width =
            relative("<span style='position:relative;left:50px'>bbbbbbbbbbbbbb     </span>");
        assert!((133.0..135.0).contains(&width), "{width}");
        let width = relative("aaaaaa <span style='position:relative;left:50px'>bb</span>     ");
        assert_eq!(width, 100.0);
        // Without hanging white space, the shifted content counts (184).
        let width = relative("<span style='position:relative;left:50px'>bbbbbbbbbbbbbb</span>");
        assert!(width > 184.0, "{width}");
        // Spaces that fit at a forced break or the end of the content do
        // not hang (Chromium 148: 129, 158 and 129).
        let near = |width: f32, expected: f32| (width - expected).abs() < 1.0;
        let width = relative("<span style='position:relative;left:100px'>aaa</span>   ");
        assert!(near(width, 128.8), "{width}");
        let width = relative("<span style='position:relative;left:100px'>aaa   </span>");
        assert!(near(width, 157.6), "{width}");
        let width = relative("<span style='position:relative;left:100px'>aaa</span>   <br>b");
        assert!(near(width, 128.8), "{width}");
        // On a line where white space hangs, all inline content counts only
        // up to the end of the line, also in a box that ends before it
        // (Chromium 148: 100).
        let spaces = " ".repeat(20);
        let width = relative(&format!(
            "<span>aaa<span style='position:relative;left:100px'>bb</span></span>{spaces}"
        ));
        assert_eq!(width, 100.0);
        // A negative text width (negative `letter-spacing`) does not panic.
        let l = layout_html(&body(
            "<div style='white-space:pre-wrap;letter-spacing:-50px'>ab   </div>",
        ));
        assert!(l.tree.scroll_size.width.is_finite());
    }

    #[test]
    fn inline_box_padding_counts_without_end_padding() {
        let html = format!("<div {S}'><span id=a style='padding-bottom:300px'>x</span></div>");
        let l = layout_html(&body(&html));
        assert_eq!(overflow_size(&html).1, l.rect("a").bottom());
    }

    #[test]
    fn flex_items_count_with_their_margin_boxes() {
        let html = format!(
            "<div {S};display:flex'><div style='flex:none;width:150px;height:150px;\
             margin:5px 15px 25px 5px'></div></div>"
        );
        assert_eq!(overflow_size(&html), (190.0, 200.0));
    }

    #[test]
    fn relative_flex_items_use_the_clamped_container_height() {
        // `top: 10%` is 10 px of the height that max-height clamps; the
        // in-flow extent does not include it.
        let html = "<div id=s style='display:flex; overflow:auto; width:100px; height:300px; \
                    max-height:100px'><div style='flex:none; width:50px; height:200px; \
                    margin-bottom:50px; position:relative; top:10%'></div></div>";
        assert_eq!(overflow_size(html), (100.0, 250.0));
    }

    #[test]
    fn a_block_in_an_inline_box_counts_without_its_margins() {
        let html = format!(
            "<div {S}'><span><div style='width:300px;height:10px;margin-right:30px'></div>\
             </span></div>"
        );
        assert_eq!(overflow_size(&html), (310.0, 120.0));
    }

    #[test]
    fn nested_scroll_containers_and_clipping_boxes_count_with_their_border_box() {
        let html = format!(
            "<div {S}'><div style='overflow:auto;width:50px;height:50px;margin-bottom:20px'>\
             <div style='width:500px;height:500px'></div></div></div>"
        );
        assert_eq!(overflow_size(&html), (120.0, 120.0));
        let html = format!(
            "<div {S}'><div style='overflow-x:clip;width:50px;height:50px'>\
             <div style='width:300px;height:300px'></div></div></div>"
        );
        assert_eq!(overflow_size(&html), (120.0, 310.0));
    }

    #[test]
    fn empty_boxes_do_not_count() {
        let html = format!(
            "<div {S}'><div style='height:10px'><div style='height:0;width:400px'></div>\
             </div></div>"
        );
        assert_eq!(overflow_size(&html), (120.0, 120.0));
    }

    #[test]
    fn collapsed_end_margins_of_the_last_child_count() {
        let html = "<div id=s style='overflow:auto;width:100px;height:100px'><div>\
                    <div style='height:200px;margin-bottom:30px'></div></div></div>";
        assert_eq!(overflow_size(html), (100.0, 230.0));
    }

    #[test]
    fn scroll_state_moves_content_but_not_boxes_with_an_outer_containing_block() {
        let l = layout_html(&body(
            "<div id=s style='overflow:auto;width:100px;height:100px'>\
             <div id=in style='height:300px'></div>\
             <div id=abs style='position:absolute;width:10px;height:10px'></div>\
             <div id=fix style='position:fixed;width:10px;height:10px'></div>\
             <div style='position:relative'>\
             <div id=inner style='position:absolute;width:10px;height:10px'></div></div></div>",
        ));
        let mut offsets = HashMap::new();
        offsets.insert(l.node("s"), Point::new(0.0, 50.0));
        let scrolled = l.tree.element_boxes_scrolled(&offsets, Point::default());
        let plain = l.tree.element_boxes();
        let dy = |id: &str| scrolled[&l.node(id)].y - plain[&l.node(id)].y;
        assert_eq!(dy("s"), 0.0);
        assert_eq!(dy("in"), -50.0);
        assert_eq!(dy("abs"), 0.0);
        assert_eq!(dy("fix"), 0.0);
        assert_eq!(dy("inner"), -50.0);
    }

    #[test]
    fn scroll_offsets_are_clamped_to_the_scroll_range() {
        let l = layout_html(&body(
            "<div id=s style='overflow:auto;width:100px;height:100px'>\
             <div id=in style='height:300px'></div></div>",
        ));
        let mut offsets = HashMap::new();
        offsets.insert(l.node("s"), Point::new(500.0, 500.0));
        let scrolled = l.tree.element_boxes_scrolled(&offsets, Point::default());
        assert_eq!(scrolled[&l.node("in")].y, -200.0);
        assert_eq!(scrolled[&l.node("in")].x, 0.0);
    }
}
