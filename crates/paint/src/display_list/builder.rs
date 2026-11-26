//! The builder of the display list: the walk of the fragment tree in paint
//! order (stacking contexts, floats, positioned and transformed boxes, clips,
//! groups for opacity, filters, masks and backdrops) that produces the items of
//! `items.rs`. The paint order and its simplifications are in the module
//! documentation of `display_list`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::svg::{ClipPath, SvgPath};
use swb_layout::{
    Ancestry, BoxContent, BoxFragment, CollapsedEdge, Fragment, FragmentTree, GroupTransform,
    Matrix, Point, Rect, ScrollOffsets, ScrollState, TextFragment, clip_property_area,
    forms_stacking_context, has_rounded_corner, has_transform, is_absolute_containing_block,
    is_fixed_containing_block, rounded_path,
};
use swb_style::animation::AnimationOwner;
use swb_style::{
    BackgroundBox, BorderStyle, ComputedStyle, Display, FillRule, Image, Position, Rgba,
    TextDecorationLine, Visibility, ZIndex,
};

use super::items::{
    DisplayItem, DisplayList, Highlights, ImageRef, ImageSizes, Radii, SELECTION_BACKGROUND,
    SELECTION_TEXT, Scrolling,
};
use crate::background::{BackgroundAreas, PaintingArea};
use crate::group_bounds::finish_groups;
use crate::mask::{self, MaskBoxes, MaskLayer};
use crate::resize_handle::resize_handle;
use crate::rope::ItemRope;
use crate::scroll_indicator::element_scroll_indicators;
use crate::{background, box_shadow, control, corner_shape, media};

mod groups;
mod layers;
mod text_clip;

use layers::Piece;
use text_clip::TextMask;

/// Builds the display list for the whole document, with the selection
/// highlight and the scroll offsets of scroll containers.
pub fn build_display_list(
    tree: &FragmentTree,
    images: &dyn ImageSizes,
    highlights: &dyn Highlights,
    scrolling: &Scrolling<'_>,
) -> DisplayList {
    let mut builder = Builder {
        list: ItemRope::default(),
        images,
        highlights,
        contexts: Vec::new(),
        tree_order: positioned_tree_order(tree.roots()),
        backdrop_roots: backdrop_roots(tree),
        blend_contexts: blend_contexts(tree),
        clips: Vec::new(),
        absolute_clips: 0,
        fixed_clips: 0,
        canvas_source: None,
        root_canvas: ItemRope::default(),
        offsets: scrolling.offsets,
        scroll: ScrollState::default(),
        indicators: scrolling.indicators,
        scale: scrolling.scale,
        composited: scrolling.composited,
        text_clip_shapes: 0,
        text_clip_warned: false,
        pieces: Vec::new(),
    };
    if let Some(canvas_background) = &tree.canvas_background {
        let bg = &canvas_background.style;
        let canvas = Rect::new(0.0, 0.0, tree.scroll_size.width, tree.scroll_size.height);
        // Images are positioned relative to the root element (CSS
        // Backgrounds 3 §2.11.1,
        // <https://www.w3.org/TR/css-backgrounds-3/#root-background>); the
        // canvas is the painting area.
        let (border, padding) = tree.root.as_ref().map_or((canvas, canvas), |root| {
            (root.border_rect, root.padding_rect())
        });
        let areas = BackgroundAreas::flat(border, padding, PaintingArea::Canvas(canvas));
        let start = builder.list.len();
        builder.background(bg, &areas, [(0.0, 0.0); 4], None);
        builder.canvas_source = Some(canvas_background.source);
        // The opacity and the mask of the root element apply to the canvas
        // background too (as in Chromium): it is painted inside the root's
        // groups.
        if tree.root.as_ref().is_some_and(|r| {
            r.style.has_mask()
                || r.style.opacity < 1.0
                || r.style.clip_path.is_shape()
                || r.style.has_filter()
        }) {
            builder.root_canvas = builder.list.split_off(start);
        }
    }
    let ancestry = Ancestry::root(tree.viewport);
    if let Some(root) = &tree.root {
        builder.box_contents(root, Point::default(), &[], &ancestry, true);
    }
    for b in &tree.top_layer {
        builder.top_layer_box(b, &ancestry);
    }
    let mut items = builder.list.into_vec();
    finish_groups(&mut items);
    DisplayList { items }
}

/// The phases in which a stacking context (or a box that paints like one)
/// paints its normal-flow descendants (CSS 2.2 Appendix E, steps 4, 5 and
/// 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Backgrounds and borders of in-flow block-level boxes.
    Backgrounds,
    /// Floats, each painted as a unit.
    Floats,
    /// Inline content: text, inline boxes, atomic inline-level boxes.
    Foreground,
}

/// How a box takes part in the paint phases of its stacking context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaintKind {
    /// Painted after the normal flow, in z-index order.
    Positioned,
    /// Painted as a unit in the float phase, as if it established a
    /// stacking context.
    Float,
    /// Painted as a unit in the foreground phase: atomic inline-level
    /// boxes, flex and grid items, and other non-positioned stacking
    /// contexts (opacity < 1, masks).
    Atomic,
    /// An inline box: painted in the foreground phase.
    Inline,
    /// An in-flow block-level box: each phase paints its part.
    Block,
}

fn paint_kind(b: &BoxFragment) -> PaintKind {
    let style = &b.style;
    // A transformed box counts as positioned (CSS Transforms 1 §3). So
    // does a flex or grid item with an integer `z-index`: it paints in
    // z-index order although it is static.
    if style.position != Position::Static
        || has_transform(b)
        || (style.flex_or_grid_item && style.z_index != ZIndex::Auto)
    {
        PaintKind::Positioned
    } else if style.is_floating() {
        PaintKind::Float
    } else if b.is_inline {
        PaintKind::Inline
    } else if style.display.is_inline_level()
        || style.opacity < 1.0
        || style.has_blend_mode()
        || style.isolation == swb_style::Isolation::Isolate
        || style.has_layout_containment()
        || style.has_paint_containment()
        || style.has_filter_effect()
        || style.will_change_makes_stacking_context()
        || style.has_mask()
        || style.clip_path.is_shape()
    {
        PaintKind::Atomic
    } else {
        PaintKind::Block
    }
}

/// True if the rectangles are equal within 0.01 px.
fn same_rect(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() < 0.01
        && (a.y - b.y).abs() < 0.01
        && (a.width - b.width).abs() < 0.01
        && (a.height - b.height).abs() < 0.01
}

/// A positioned box painted after the normal flow of its stacking context.
struct Deferred {
    z: i32,
    /// The box's index in tree order (see [`positioned_tree_order`]):
    /// boxes with equal z-index paint in tree order, also when the paint
    /// phases find them in another order.
    order: u32,
    items: ItemRope,
    /// The number of clips that the items repeat around the box: they
    /// start with a `PushViewportClip` (for a box fixed to the viewport)
    /// and the `PushClip`s of the clips that apply to the box, and end with
    /// as many `PopClip`s.
    clips: usize,
}

/// The index in tree order (pre-order) of each positioned or transformed
/// box below the fragments `roots` (the root and the top layer), by
/// address. The paint phases find positioned boxes in another order (those
/// in floats and inline content later); with equal z-index they paint in
/// tree order. The recursion depth is bounded by the box tree depth (see
/// `swb_layout`'s `box_tree.rs`).
fn positioned_tree_order<'a>(
    roots: impl Iterator<Item = &'a BoxFragment>,
) -> HashMap<*const BoxFragment, u32> {
    fn visit(b: &BoxFragment, next: &mut u32, order: &mut HashMap<*const BoxFragment, u32>) {
        for child in b.children.iter() {
            if let Fragment::Box(cb) = child {
                *next = next.saturating_add(1);
                if paint_kind(cb) == PaintKind::Positioned {
                    order.insert(std::ptr::from_ref(cb), *next);
                }
                visit(cb, next, order);
            }
        }
    }
    let mut order = HashMap::new();
    let mut next = 0;
    for root in roots {
        visit(root, &mut next, &mut order);
    }
    order
}

/// The boxes that are backdrop roots (Filter Effects 2 §2) and have a
/// descendant with a `backdrop-filter`, by address, in the trees of the
/// root and of the top layer (the root element is the backdrop root of a
/// box of the top layer that is none itself). The recursion depth is
/// bounded by the box tree depth (see `swb_layout`'s `box_tree.rs`).
fn backdrop_roots(tree: &FragmentTree) -> HashSet<*const BoxFragment> {
    /// Returns true if `b` or a box below it has a `backdrop-filter`.
    fn visit(b: &BoxFragment, roots: &mut HashSet<*const BoxFragment>) -> bool {
        let mut below = false;
        for child in b.children.iter() {
            if let Fragment::Box(cb) = child {
                below |= visit(cb, roots);
            }
        }
        if below && b.style.is_backdrop_root() {
            roots.insert(std::ptr::from_ref(b));
        }
        below || b.style.has_backdrop_filter()
    }
    let mut roots = HashSet::new();
    for root in tree.roots() {
        visit(root, &mut roots);
    }
    roots
}

/// The boxes that form a stacking context (other than the root of a tree)
/// that has a box with a `mix-blend-mode` in it, by address. A stacking
/// context is an isolated group (CSS Compositing and Blending 1 §3.2,
/// measured in Chromium 148): the blend sees only what the context
/// painted before the box, so such a context paints into a layer. The
/// recursion depth is bounded by the box tree depth (see `swb_layout`'s
/// `box_tree.rs`).
fn blend_contexts(tree: &FragmentTree) -> HashSet<*const BoxFragment> {
    /// Returns true if `b` or a box below it, outside the stacking contexts
    /// below `b`, has a `mix-blend-mode`: it blends in the context above.
    fn visit(b: &BoxFragment, is_root: bool, found: &mut HashSet<*const BoxFragment>) -> bool {
        // An element of an inline `<svg>` blends in the context of the
        // `<svg>` box (or in the box's own context).
        let mut below = matches!(&b.content, BoxContent::Svg(svg) if svg.has_blend());
        for child in b.children.iter() {
            if let Fragment::Box(cb) = child {
                below |= visit(cb, false, found);
            }
        }
        if is_root || forms_stacking_context(b) {
            if below && !is_root {
                found.insert(std::ptr::from_ref(b));
            }
            b.style.has_blend_mode()
        } else {
            below || b.style.has_blend_mode()
        }
    }
    let mut found = HashSet::new();
    for root in tree.roots() {
        visit(root, true, &mut found);
    }
    found
}

/// True if the group of the `mix-blend-mode` of `b` must enclose its
/// transform groups: the transform of `groups` is not a translation, so
/// the rasterizer draws the box into a transparent layer, and a blend
/// inside it would see nothing to blend with.
fn blends_outside_transform(b: &BoxFragment, groups: &[GroupTransform]) -> bool {
    b.style.has_blend_mode()
        && groups
            .iter()
            .any(|g| matches!(g, GroupTransform::Matrix(m) if !m.is_translation()))
}

/// An open clip (overflow or `clip` property).
struct OpenClip {
    /// The clip rectangle (for overflow, the padding box).
    rect: Rect,
    /// The stacking context the clip belongs to: the number of open
    /// stacking contexts at the clipping box, including the box's own
    /// context. The positioned descendants in that context repeat it.
    context_depth: usize,
    /// True for the clip of the `clip` property, which also clips fixed
    /// descendants.
    property: bool,
    /// The corner radii of the padding edge, if the clip is rounded
    /// (`overflow` with `border-radius`, CSS Backgrounds 3 §4.2).
    radii: Option<Radii>,
}

struct Builder<'a> {
    /// The items painted so far, in chunks: moving the items of positioned
    /// boxes between stacking contexts moves chunks (see `rope.rs`).
    list: ItemRope,
    images: &'a dyn ImageSizes,
    highlights: &'a dyn Highlights,
    /// One entry per open stacking context: its positioned descendants, in
    /// the order the paint phases find them.
    contexts: Vec<Vec<Deferred>>,
    /// The tree order of the positioned boxes (see
    /// [`positioned_tree_order`]).
    tree_order: HashMap<*const BoxFragment, u32>,
    /// The backdrop roots with a `backdrop-filter` below them (see
    /// [`backdrop_roots`]).
    backdrop_roots: HashSet<*const BoxFragment>,
    /// The stacking contexts with a `mix-blend-mode` box in them (see
    /// [`blend_contexts`]).
    blend_contexts: HashSet<*const BoxFragment>,
    /// The clips (overflow and `clip` property) of the boxes that are being
    /// painted, outermost first.
    clips: Vec<OpenClip>,
    /// The number of entries of `clips` that apply to absolutely
    /// positioned descendants: the clips of the containing block and its
    /// ancestors.
    absolute_clips: usize,
    /// The number of entries of `clips` that apply to fixed descendants:
    /// those of the nearest transformed ancestor and above (0 without
    /// one). The `clip` properties after them apply too.
    fixed_clips: usize,
    /// The element whose background paints the canvas; its own box does
    /// not paint the background again.
    canvas_source: Option<NodeId>,
    /// The canvas background of a root element with opacity or a mask,
    /// which its groups paint first.
    root_canvas: ItemRope,
    /// The scroll offsets of scroll containers.
    offsets: &'a dyn ScrollOffsets,
    /// The scroll offsets that apply to the children of the box being
    /// painted.
    scroll: ScrollState,
    /// True to draw overlay scroll indicators.
    indicators: bool,
    /// Device pixels per CSS px.
    scale: f32,
    /// The owners of the composited transform groups (see
    /// [`Scrolling::composited`]).
    composited: Option<&'a HashSet<AnimationOwner>>,
    /// The shapes that `background-clip: text` groups clip by so far (see
    /// [`text_clip::MAX_SHAPES_TOTAL`]).
    text_clip_shapes: usize,
    /// True once the limits warned (once per display list).
    text_clip_warned: bool,
    /// The buffer of [`Builder::background`], kept between boxes.
    pieces: Vec<Piece>,
}

/// A text decoration propagated from an ancestor.
#[derive(Clone, Copy)]
struct Decoration {
    lines: TextDecorationLine,
    color: Rgba,
}

impl Builder<'_> {
    fn box_fragment(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        decorations: &[Decoration],
        ancestry: &Ancestry,
    ) {
        // An absolutely positioned or fixed box does not move with the
        // scroll containers outside its containing block.
        let origin = self.scroll.origin_of(b, origin);
        let style = &b.style;
        let positioned = style.position != Position::Static;
        // Only positioned and transformed boxes, and flex and grid items
        // with a `z-index`, come here (`walk`).
        debug_assert!(positioned || has_transform(b) || style.z_index_applies());
        // A positioned or transformed box is painted after the normal-flow
        // content of the enclosing stacking context, in z-index order
        // (`auto` as 0; a transformed box counts as positioned, CSS
        // Transforms 1 §3). It forms a stacking context if it has an
        // integer z-index, a transform, or fixed or sticky positioning.
        let z = match style.z_index {
            ZIndex::Integer(z) if style.z_index_applies() => z,
            _ => 0,
        };
        let context = forms_stacking_context(b);
        // The clips between the stacking context and the box still apply,
        // if they belong to the box's containing block chain; the `clip`
        // properties of ancestors also apply to fixed boxes (Chromium). A
        // box fixed to the viewport replaces all clips and repeats only the
        // `clip` properties.
        let viewport_fixed = ancestry.is_viewport_fixed(b);
        let depth = self.contexts.len();
        let applies = |i: usize, c: &OpenClip| match style.position {
            _ if viewport_fixed => c.property,
            Position::Absolute => i < self.absolute_clips && c.context_depth == depth,
            Position::Fixed => (i < self.fixed_clips || c.property) && c.context_depth == depth,
            _ => c.context_depth == depth,
        };
        let clips: Vec<(Rect, Option<Radii>)> = self
            .clips
            .iter()
            .enumerate()
            .filter(|&(i, c)| applies(i, c))
            .map(|(_, c)| (c.rect, c.radii))
            .collect();
        // The box's place in the stacking context comes before its
        // positioned descendants, which it does not contain if it forms no
        // stacking context.
        let order = self
            .tree_order
            .get(&std::ptr::from_ref(b))
            .copied()
            .unwrap_or(u32::MAX);
        let slot = self.contexts.last_mut().map(|c| {
            c.push(Deferred {
                z,
                order,
                items: ItemRope::default(),
                // The viewport clip and the repeated clips.
                clips: clips.len() + usize::from(viewport_fixed),
            });
            c.len() - 1
        });
        // The box paints into the list and its items are split off (they
        // keep their chunks, see `rope.rs`).
        let start = self.list.len();
        if viewport_fixed {
            self.list.push(DisplayItem::PushViewportClip);
        }
        self.list
            .extend(clips.iter().map(|&(r, radii)| clip_item(r, radii)));
        self.box_contents(b, origin, decorations, ancestry, context);
        // The ends in reverse order: a rounded clip (a group) and a plain
        // clip must nest, as the damage of a frame needs (`crate::damage`).
        self.list
            .extend(clips.iter().rev().map(|&(_, radii)| unclip_item(radii)));
        if viewport_fixed {
            self.list.push(DisplayItem::PopClip);
        }
        if let Some(deferred) = slot
            .zip(self.contexts.last_mut())
            .and_then(|(slot, context)| context.get_mut(slot))
        {
            deferred.items = self.list.split_off(start);
        }
    }

    /// Paints a box of the top layer (ADR 0037 section 8): a stacking
    /// context of its own after the root's, whatever its `z-index`, with
    /// no clip or group of its DOM ancestors; a box fixed to the viewport
    /// is clipped to it.
    fn top_layer_box(&mut self, b: &BoxFragment, ancestry: &Ancestry) {
        // An outer context: the box is not the root (whose background
        // paints the canvas), and its own context holds its descendants.
        self.contexts.push(Vec::new());
        let viewport_fixed = ancestry.is_viewport_fixed(b);
        if viewport_fixed {
            self.list.push(DisplayItem::PushViewportClip);
        }
        self.box_contents(b, Point::default(), &[], ancestry, true);
        if viewport_fixed {
            self.list.push(DisplayItem::PopClip);
        }
        let outer = self.contexts.pop();
        debug_assert!(
            outer.is_some_and(|c| c.is_empty()),
            "the box has its own context"
        );
    }

    /// Paints a box and its descendants. A stacking context root collects
    /// its positioned descendants and paints them at the end: negative
    /// z-index below its normal-flow content, the others after its content.
    /// They are outside the box's own overflow clip; each positioned
    /// descendant repeats the clips that apply to it (see `box_fragment`).
    fn box_contents(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        decorations: &[Decoration],
        ancestry: &Ancestry,
        stacking_context: bool,
    ) {
        let rect = b.border_rect.translate(origin);
        let groups = ancestry.group_transforms(b, rect);
        // A blend must apply to the transformed box: its group encloses the
        // transform layer, which is transparent around the box.
        let early = blends_outside_transform(b, &groups).then(|| self.push_opacity(b));
        let composited = self.composited.is_some_and(|set| {
            b.node.is_some_and(|node| {
                set.contains(&AnimationOwner {
                    node,
                    pseudo: b.pseudo,
                })
            })
        });
        let transforms = self.open_transforms(&groups, composited);
        let group = self.open_group(b, origin, stacking_context, early);
        let is_root = group.outer_depth == 0;
        // The clips of a stacking context root belong to the context: its
        // positioned descendants repeat them.
        let clip_depth = self.contexts.len();
        let outer_clips = (self.absolute_clips, self.fixed_clips);
        let clip_property = self.push_clip_property(b, rect, clip_depth);
        let backdrop_root = self.open_backdrop(b, origin, &group);
        self.paint_box(b, origin, is_root);
        let own = child_decorations(b, decorations);
        let negative_z_at = self.list.len();
        let clips_outside = self.clips.len();
        let clipped = self.push_overflow_clip(b, origin, clip_depth);
        if is_absolute_containing_block(b) {
            self.absolute_clips = self.clips.len();
        } else if b.in_positioned_inline {
            // The containing block of the absolutely positioned boxes in a
            // block inside a positioned inline box is that inline box,
            // outside the block's own clip.
            self.absolute_clips = clips_outside;
        }
        if is_fixed_containing_block(b) {
            self.fixed_clips = self.clips.len();
        }
        // The children of a scroll container move by its scroll offset.
        let (child_origin, child_scroll) = self.scroll.enter(b, rect.origin(), self.offsets);
        let outer_scroll = std::mem::replace(&mut self.scroll, child_scroll);
        let shift = Point::new(child_origin.x - rect.x, child_origin.y - rect.y);
        let inner = ancestry.enter(b, rect, &groups, shift);
        for phase in [Phase::Backgrounds, Phase::Floats, Phase::Foreground] {
            self.walk(b, child_origin, &own, phase, false, &inner);
        }
        self.scroll = outer_scroll;
        self.box_foreground_end(b, rect.origin());
        if clipped {
            self.pop_clip();
        }
        let positioned = if group.context {
            self.paint_deferred(negative_z_at, mask::needs_positioned_area(&b.style))
        } else {
            None
        };
        (self.absolute_clips, self.fixed_clips) = outer_clips;
        // Above the content and the positioned descendants of the box's own
        // stacking context, inside the padding box.
        self.paint_box_chrome(b, rect);
        self.outline(b, origin);
        self.close_backdrop(backdrop_root);
        if clip_property {
            self.pop_clip();
        }
        self.close_group(&group, b, origin, positioned);
        self.close_transforms(transforms);
        if group.hoisted {
            self.list.push(DisplayItem::PopOpacity);
        }
    }

    /// Starts the clip of the `clip` property (CSS 2.2 §11.1.2) of an
    /// absolutely positioned box whose border box is at `rect`, if it has
    /// one. It clips the box and all its descendants, fixed ones too (as in
    /// Chromium). Returns true if it did.
    fn push_clip_property(&mut self, b: &BoxFragment, rect: Rect, context_depth: usize) -> bool {
        let Some(area) = clip_property_area(b, rect) else {
            return false;
        };
        self.list.push(DisplayItem::PushClip(area));
        self.clips.push(OpenClip {
            rect: area,
            context_depth,
            property: true,
            radii: None,
        });
        true
    }

    /// Paints one phase of the descendants of `b` (whose border box is at
    /// `origin`): the children of in-flow block boxes, recursively, and the
    /// boxes that the phase paints as a unit. Positioned boxes are deferred
    /// to their stacking context in the first phase that reaches them
    /// (`in_inline`: the children of inline boxes, which only the
    /// foreground phase walks).
    fn walk(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        decorations: &[Decoration],
        phase: Phase,
        in_inline: bool,
        ancestry: &Ancestry,
    ) {
        let registers = if in_inline {
            Phase::Foreground
        } else {
            Phase::Backgrounds
        };
        // Flex and grid items paint as a unit (CSS Flexbox 1 §5.4, CSS Grid
        // 2 §9).
        let items = matches!(
            b.style.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        );
        for child in b.children.iter() {
            let cb = match child {
                Fragment::Text(t) => {
                    if phase == Phase::Foreground {
                        self.text(t, origin, decorations);
                    }
                    continue;
                }
                Fragment::Box(cb) => cb,
            };
            let kind = match paint_kind(cb) {
                PaintKind::Block if items => PaintKind::Atomic,
                kind => kind,
            };
            match (kind, phase) {
                (PaintKind::Positioned, p) if p == registers => {
                    self.box_fragment(cb, origin, decorations, ancestry);
                }
                (PaintKind::Float, Phase::Floats) | (PaintKind::Atomic, Phase::Foreground) => {
                    self.box_contents(cb, origin, decorations, ancestry, false);
                }
                (PaintKind::Inline, Phase::Foreground) => {
                    self.inline_box(cb, origin, decorations, ancestry);
                }
                (PaintKind::Block, phase) => {
                    self.block_phase(cb, origin, decorations, ancestry, phase);
                }
                _ => {}
            }
        }
    }

    /// Paints an inline box (in the foreground phase): its background,
    /// border and content, inside its opacity and mask groups.
    fn inline_box(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        decorations: &[Decoration],
        ancestry: &Ancestry,
    ) {
        let group = self.open_group(b, origin, false, None);
        let backdrop_root = self.open_backdrop(b, origin, &group);
        self.paint_box(b, origin, false);
        let negative_z_at = self.list.len();
        let own = child_decorations(b, decorations);
        let rect = b.border_rect.translate(origin);
        let inner = ancestry.enter(b, rect, &[], Point::default());
        self.walk(b, rect.origin(), &own, Phase::Foreground, true, &inner);
        let positioned = if group.context {
            self.paint_deferred(negative_z_at, mask::needs_positioned_area(&b.style))
        } else {
            None
        };
        self.outline(b, origin);
        self.close_backdrop(backdrop_root);
        self.close_group(&group, b, origin, positioned);
    }

    /// One phase of an in-flow block-level box that is neither positioned
    /// nor transformed: its background and border in the background phase,
    /// then the same phase of its children inside its overflow clip (moved
    /// by its scroll offset); in the foreground phase also what it paints
    /// after its children, its scroll indicators and its outline.
    fn block_phase(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        decorations: &[Decoration],
        ancestry: &Ancestry,
        phase: Phase,
    ) {
        if phase == Phase::Backgrounds {
            self.paint_box(b, origin, false);
        }
        let rect = b.border_rect.translate(origin);
        let own = child_decorations(b, decorations);
        let outer_clips = (self.absolute_clips, self.fixed_clips);
        let clips_outside = self.clips.len();
        let clipped = self.push_overflow_clip(b, origin, self.contexts.len());
        if b.in_positioned_inline {
            // See `box_contents`.
            self.absolute_clips = clips_outside;
        }
        let (child_origin, child_scroll) = self.scroll.enter(b, rect.origin(), self.offsets);
        let outer_scroll = std::mem::replace(&mut self.scroll, child_scroll);
        let shift = Point::new(child_origin.x - rect.x, child_origin.y - rect.y);
        let inner = ancestry.enter(b, rect, &[], shift);
        self.walk(b, child_origin, &own, phase, false, &inner);
        self.scroll = outer_scroll;
        if phase == Phase::Foreground {
            self.box_foreground_end(b, rect.origin());
        }
        if clipped {
            self.pop_clip();
        }
        (self.absolute_clips, self.fixed_clips) = outer_clips;
        if phase == Phase::Foreground {
            self.paint_box_chrome(b, rect);
            self.outline(b, origin);
        }
    }

    /// The overlay scroll indicators (for the GUI) and the resize handle of a
    /// box whose border box is `rect`, above its content.
    fn paint_box_chrome(&mut self, b: &BoxFragment, rect: Rect) {
        if self.indicators && b.style.visibility == Visibility::Visible {
            self.list
                .extend(element_scroll_indicators(b, rect, self.offsets));
        }
        self.list.extend(resize_handle(b, rect));
    }

    /// What a box paints after its content: the collapsed borders of a
    /// table and the caret of a form control. `origin` is its border-box
    /// origin.
    fn box_foreground_end(&mut self, b: &BoxFragment, origin: Point) {
        if let BoxContent::Table(table) = &b.content
            && let Some(collapsed) = &table.collapsed
        {
            self.collapsed_borders(collapsed, origin);
        }
        if let BoxContent::Control(c) = &b.content
            && b.style.visibility == Visibility::Visible
        {
            self.list.extend(control::caret(c, &b.style, origin));
        }
    }

    /// The box's own hit region, background, border and replaced content.
    fn paint_box(&mut self, b: &BoxFragment, origin: Point, is_root: bool) {
        let style = &b.style;
        if style.visibility != Visibility::Visible || b.content == BoxContent::GeometryOnly {
            // SVG shapes have their own `visibility` (inherited, so a
            // shape can be visible in a hidden `<svg>`).
            if let BoxContent::Svg(svg) = &b.content {
                let content = b.content_rect().translate(origin);
                crate::inline_svg::paint(&mut self.list, svg, style, content, self.scale);
            }
            return;
        }
        let rect = b.border_rect.translate(origin);
        if let Some(node) = b.node
            && style.receives_pointer_events()
        {
            self.list.push(DisplayItem::HitRegion { rect, node });
        }
        let mut areas = BackgroundAreas::of(b, origin);
        let mut border_rect = rect;
        let mut paint_border = true;
        match &b.content {
            // Cells paint the backgrounds of rows and row groups.
            BoxContent::TablePart => return,
            BoxContent::TableCell(cell) => {
                // The part's background is positioned in the part (a
                // gradient is continuous across its cells) and painted in
                // the cell.
                for part in &cell.backgrounds {
                    let area = part.area.translate(rect.origin());
                    let clip = part.clip.translate(rect.origin());
                    let part_areas = BackgroundAreas::flat(area, area, PaintingArea::Cell(clip));
                    self.background(&part.style, &part_areas, [(0.0, 0.0); 4], None);
                }
                if cell.hidden {
                    return;
                }
                paint_border = !cell.collapsed_borders;
            }
            // The table's background and border surround the grid, not the
            // captions; collapsed borders are painted after the cells.
            BoxContent::Table(table) => {
                border_rect = table.grid.translate(rect.origin());
                let padding = border_rect.inset(&b.border);
                areas = BackgroundAreas::flat(border_rect, padding, PaintingArea::Own);
                paint_border = table.collapsed.is_none();
            }
            BoxContent::None
            | BoxContent::Image(_)
            | BoxContent::GeometryOnly
            | BoxContent::Control(_)
            | BoxContent::Media(_)
            | BoxContent::Svg(_)
            | BoxContent::Frame(_)
            | BoxContent::Placeholder(_) => {}
        }
        let radii = resolve_radii(style, border_rect);
        // The root's background (or the body's, if it was propagated)
        // paints the canvas instead.
        let paints_canvas =
            is_root || (b.pseudo.is_none() && b.node.is_some() && b.node == self.canvas_source);
        // Controls with the native look draw their own background and
        // border.
        let native = matches!(&b.content, BoxContent::Control(c) if c.native);
        self.outer_shadows(b, border_rect, radii, native);
        if !paints_canvas && !native {
            self.box_background(b, origin, &areas, radii);
        }
        self.inner_shadows(b, border_rect, radii, native);
        if paint_border && !native {
            self.border(b, border_rect, radii);
        }
        if let BoxContent::Control(c) = &b.content {
            self.list.extend(control::native_look(c, style, rect));
        }
        let content = b.content_rect().translate(origin);
        match &b.content {
            BoxContent::Image(node) => self.replaced_image(*node, style, content),
            BoxContent::Media(m) => self.media_content(b, origin, m, content),
            BoxContent::Svg(svg) => {
                crate::inline_svg::paint(&mut self.list, svg, style, content, self.scale);
            }
            BoxContent::Frame(node) if content.width > 0.0 && content.height > 0.0 => {
                // The user-agent style sheet gives an `iframe` `overflow:
                // clip !important`, so the rounded inner border edge clips
                // the frame; the rasterizer clips to the rectangle.
                let padding = b.padding_rect().translate(origin);
                let rounded =
                    padding_radii(b, origin).filter(|r| rounded_clip(padding, r).is_some());
                if rounded.is_some() {
                    self.list.push(clip_item(padding, rounded));
                }
                self.list.push(DisplayItem::Frame {
                    node: *node,
                    rect: content,
                    interactive: style.receives_pointer_events(),
                });
                if rounded.is_some() {
                    self.list.push(unclip_item(rounded));
                }
            }
            _ => {}
        }
    }

    /// The content of a media element in its content box `content`: the
    /// default poster, the poster or frame of a video, and the controls. The
    /// `border-radius` clips the content of a video (the user-agent style
    /// sheet gives it `overflow: clip`), as it clips a frame.
    fn media_content(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        m: &swb_layout::MediaContent,
        content: Rect,
    ) {
        let padding = b.padding_rect().translate(origin);
        let rounded = (!m.audio)
            .then(|| padding_radii(b, origin))
            .flatten()
            .filter(|r| rounded_clip(padding, r).is_some());
        if rounded.is_some() {
            self.list.push(clip_item(padding, rounded));
        }
        if let Some(fill) = media::default_poster(m, content) {
            self.list.push(fill);
        }
        // The poster or the frame of a video, over the default poster (which
        // only a video without a `poster` attribute has: its frames cover it,
        // ADR 0047 section 9).
        if !m.audio {
            self.replaced_image(m.node, &b.style, content);
        }
        self.list.extend(media::controls(m, content));
        if rounded.is_some() {
            self.list.push(unclip_item(rounded));
        }
    }

    /// The image of replaced element `node` (an `<img>`, the poster of a
    /// video) in its content box `content`, placed with `object-fit` and
    /// `object-position`, and clipped to the content box. Deliberate
    /// deviation: the clip ignores `overflow` (the user-agent style sheet
    /// sets `overflow: clip` on images and videos; with an author's
    /// `overflow: visible`, Chromium paints the overflowing part too).
    fn replaced_image(&mut self, node: NodeId, style: &ComputedStyle, content: Rect) {
        let image = ImageRef::Node(node);
        let rect = self.images.size(&image).map_or(content, |natural| {
            background::object_rect(style.object_fit, &style.object_position, content, &natural)
        });
        self.list.push(DisplayItem::Image {
            image,
            rect,
            tile: rect,
            clip: content,
        });
    }

    /// Starts the overflow clip of a box, if it has one. Returns true if it
    /// did.
    fn push_overflow_clip(&mut self, b: &BoxFragment, origin: Point, context_depth: usize) -> bool {
        let style = &b.style;
        if !(style.clips_x() || style.clips_y()) {
            return false;
        }
        let rect = b.padding_rect().translate(origin);
        let radii = padding_radii(b, origin).filter(|r| rounded_clip(rect, r).is_some());
        let shaped = radii.and_then(|r| {
            let k = corner_shape::shapes(style)?;
            corner_shape::clip(rect, r, k)
        });
        self.list
            .push(shaped.unwrap_or_else(|| clip_item(rect, radii)));
        self.clips.push(OpenClip {
            rect,
            context_depth,
            property: false,
            radii,
        });
        true
    }

    /// Ends the innermost open clip.
    fn pop_clip(&mut self) {
        if let Some(clip) = self.clips.pop() {
            self.list.push(unclip_item(clip.radii));
        }
    }

    /// Paints the positioned descendants of the stacking context that is
    /// closing: negative z-index at `negative_z_at` (below the normal-flow
    /// content), the others at the end. With `want_area`, returns the area
    /// that they draw.
    fn paint_deferred(&mut self, negative_z_at: usize, want_area: bool) -> Option<Rect> {
        let mut deferred = self.contexts.pop().unwrap_or_default();
        // The ink of each box inside its own clips, not those of the boxes
        // around it (for `no-clip` masks).
        let area = want_area
            .then(|| {
                deferred
                    .iter()
                    .filter_map(|d| {
                        let own = d.items.len().saturating_sub(2 * d.clips);
                        mask::clipped_ink(d.items.iter_from(d.clips).take(own))
                    })
                    .reduce(|a, b| a.union(&b))
            })
            .flatten();
        deferred.sort_by_key(|d| (d.z, d.order));
        let split = deferred.partition_point(|d| d.z < 0);
        let positive = deferred.split_off(split);
        let mut negative = ItemRope::default();
        for d in deferred {
            negative.append(d.items);
        }
        self.list.insert(negative_z_at, negative);
        for d in positive {
            self.list.append(d.items);
        }
        area
    }

    /// The outer `box-shadow`s of a box, below its background.
    fn outer_shadows(&mut self, b: &BoxFragment, rect: Rect, radii: Radii, native: bool) {
        let s = &b.style;
        if s.box_shadow.is_empty() || native {
            return;
        }
        // An opaque background color that covers the border box hides the
        // part of the shadow below the box: no clip needed.
        // (With a `corner-shape` other than `round` the shortcut does not
        // apply: the corners of the border box stay uncovered.)
        let opaque = s.background_color.resolve(s.color).a == 255
            && s.background_clip
                .last()
                .copied()
                .unwrap_or(BackgroundBox::BorderBox)
                == BackgroundBox::BorderBox;
        let shapes = corner_shape::shapes(s);
        let items = box_shadow::outer(&s.box_shadow, s.color, rect, radii, opaque, shapes);
        self.list.extend(items);
    }

    /// The inner `box-shadow`s of a box, above its background, inside its
    /// border.
    fn inner_shadows(&mut self, b: &BoxFragment, rect: Rect, radii: Radii, native: bool) {
        let s = &b.style;
        if s.box_shadow.is_empty() || native {
            return;
        }
        let padding = rect.inset(&b.border);
        let items = box_shadow::inner(
            &s.box_shadow,
            s.color,
            padding,
            crate::clip_path::shrink(radii, &b.border),
        );
        self.list.extend(items);
    }

    fn outline(&mut self, b: &BoxFragment, origin: Point) {
        let style = &b.style;
        if style.visibility != Visibility::Visible {
            return;
        }
        let rect = b.border_rect.translate(origin);
        let Some(border_style) = style.outline_style.border_style() else {
            self.focus_ring(style, with_descendants(b, origin));
            return;
        };
        if border_style == BorderStyle::None || style.outline_width <= 0.0 {
            return;
        }
        let w = style.outline_width;
        let outer = rect.outset(w + style.outline_offset);
        let color = style.outline_color.resolve(style.color);
        self.list.push(DisplayItem::Border {
            rect: outer,
            widths: [w; 4],
            colors: [color; 4],
            styles: [border_style; 4],
            radii: [(0.0, 0.0); 4],
        });
    }

    /// Paints `outline-style: auto` as Chromium does around `rect` (the
    /// border box and its descendants): a ring of 2 px in the outline
    /// color, centered on `outline-offset` outside the border box (1 px
    /// further in if the box has a border on every side), with a ring of
    /// 1 px in white around it for contrast. The corners are rounded. The
    /// outline width does not matter. An empty box gets no ring. (Measured
    /// with Chromium 148.) Deliberate simplification: the line fragments of
    /// a wrapped inline box get one ring each; Chromium merges them.
    fn focus_ring(&mut self, style: &ComputedStyle, rect: Rect) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let min_border = style
            .border_top_width
            .min(style.border_right_width)
            .min(style.border_bottom_width)
            .min(style.border_left_width);
        let center = style.outline_offset - if min_border >= 1.0 { 1.0 } else { 0.0 };
        let radii = resolve_radii(style, rect);
        let ring = |distance: f32| -> (Rect, Radii) {
            // The radius of the ring's center line is at least 2 px.
            let radius = |r: f32| (r + distance).max(2.0 + distance - center);
            let radii = radii.map(|(h, v)| (radius(h), radius(v)));
            (rect.outset(distance), radii)
        };
        // Both rings or none: the color ring must not be degenerate.
        let (color_rect, _) = ring(center + 1.0);
        if color_rect.width <= 4.0 || color_rect.height <= 4.0 {
            return;
        }
        let color = style.outline_color.resolve(style.color);
        for (outer, width, color) in [(center + 2.0, 1.0, Rgba::WHITE), (center + 1.0, 2.0, color)]
        {
            let (rect, radii) = ring(outer);
            self.list.push(DisplayItem::Border {
                rect,
                widths: [width; 4],
                colors: [color; 4],
                styles: [BorderStyle::Solid; 4],
                radii,
            });
        }
    }

    /// Paints the background of box `b` (its parent's border box starts at
    /// `origin`): with the mask of its text if a layer says
    /// `background-clip: text` and paints (ADR 0046).
    fn box_background(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        areas: &BackgroundAreas,
        radii: Radii,
    ) {
        let style = &b.style;
        let paints = !style.background_color.resolve(style.color).is_transparent()
            || style.background_image.iter().any(Option::is_some);
        let text = (paints
            && style.background_clip.contains(&BackgroundBox::Text)
            && matches!(areas.painting, PaintingArea::Own))
        .then(|| self.text_mask(b, origin))
        .flatten();
        self.background(style, areas, radii, text.as_ref());
    }

    /// Paints the background of a box: the color, then the image layers,
    /// each clipped by its `background-clip` with the corner radii of the
    /// clip box (`layers.rs`) and, for `text`, by the mask of the text
    /// (`text`: `None` paints `text` as `border-box`).
    fn background(
        &mut self,
        style: &ComputedStyle,
        areas: &BackgroundAreas,
        radii: Radii,
        text: Option<&TextMask>,
    ) {
        let color = style.background_color.resolve(style.color);
        // The color is painted with the clip of the bottom layer.
        let color_clip = style
            .background_clip
            .last()
            .copied()
            .unwrap_or(BackgroundBox::BorderBox);
        let kinds = corner_shape::shapes(style);
        let own = matches!(areas.painting, PaintingArea::Own);
        let rounded = |shape: (Rect, Radii)| Some(shape).filter(|s| has_rounded_corner(&s.1));
        // The buffer is reused: most boxes have one piece.
        let mut pieces = std::mem::take(&mut self.pieces);
        if !color.is_transparent() {
            let (rect, shaped_radii) = match areas.painting {
                PaintingArea::Own => areas.color_area(style, color_clip, radii),
                PaintingArea::Canvas(_) | PaintingArea::Cell(_) => {
                    (areas.painting_area(color_clip), [(0.0, 0.0); 4])
                }
            };
            // A rounded box with image layers clips the color and the
            // layers together, so that the anti-aliased edge does not show
            // the color around the layers (Chromium clips the whole
            // background in one layer). Not where the color keeps out of
            // the borders (`color_area`).
            let shape = areas.clip_shape(color_clip, radii);
            let has_images = style.background_image.iter().any(Option::is_some);
            let together = own && has_images && rect == shape.0 && shaped_radii == shape.1;
            let (round, item) = if together {
                let square = DisplayItem::Rect {
                    rect,
                    radii: [(0.0, 0.0); 4],
                    color,
                };
                (rounded(shape), square)
            } else {
                // The item has the rounded shape itself.
                let shaped = kinds.and_then(|k| corner_shape::fill(rect, shaped_radii, k, color));
                let fill = DisplayItem::Rect {
                    rect,
                    radii: shaped_radii,
                    color,
                };
                (None, shaped.unwrap_or(fill))
            };
            pieces.push(Piece {
                clip: color_clip,
                round,
                item,
            });
        }
        // Layers are painted bottom (last) to top (first).
        for (i, image) in style.background_image.iter().enumerate().rev() {
            let (Some(image), Some(&origin), Some(&clip)) = (
                image,
                layer_value(&style.background_origin, i),
                layer_value(&style.background_clip, i),
            ) else {
                continue;
            };
            let positioning = areas.area(origin);
            let clip_area = areas.painting_area(clip);
            let item = match image {
                Image::Url(url) => self.image_layer(style, i, url, positioning, clip_area),
                Image::LinearGradient(gradient) => {
                    Self::gradient_layer(style, areas, i, gradient, positioning, clip_area)
                }
            };
            if let Some(item) = item {
                let round = own.then(|| areas.clip_shape(clip, radii)).and_then(rounded);
                pieces.push(Piece { clip, round, item });
            }
        }
        self.emit_pieces(&mut pieces, text, areas.border, kinds);
        self.pieces = pieces;
    }

    /// The item of layer `i` of the background images, an image at `url`
    /// (`None` while its size is unknown).
    fn image_layer(
        &self,
        style: &ComputedStyle,
        i: usize,
        url: &Arc<str>,
        positioning: Rect,
        clip: Rect,
    ) -> Option<DisplayItem> {
        let image = ImageRef::Url(Arc::clone(url));
        let natural = self.images.size(&image)?;
        let layer = background::Layer {
            size: layer_value(&style.background_size, i)?,
            position_x: layer_value(&style.background_position_x, i)?,
            position_y: layer_value(&style.background_position_y, i)?,
            repeat: *layer_value(&style.background_repeat, i)?,
        };
        let (tile, area) = background::tile(&layer, positioning, clip, &natural);
        Some(DisplayItem::Image {
            image,
            rect: area,
            tile,
            clip,
        })
    }

    /// The item of layer `i` of the background images, a gradient. One
    /// gradient fills the positioning area (the whole canvas for the
    /// canvas) when the layer has the default size, position and repeat;
    /// otherwise the item holds the layer's first tile (`background-size`,
    /// `-position`, `-repeat`) and the rasterizer repeats it. `None` for an
    /// empty tile.
    fn gradient_layer(
        style: &ComputedStyle,
        areas: &BackgroundAreas,
        i: usize,
        gradient: &Arc<swb_style::LinearGradient>,
        positioning: Rect,
        clip: Rect,
    ) -> Option<DisplayItem> {
        let item = |rect: Rect, tile: Rect| DisplayItem::LinearGradient {
            rect,
            tile,
            clip,
            gradient: Arc::clone(gradient),
            current_color: style.color,
        };
        let rect = match areas.painting {
            PaintingArea::Own => clip.intersection(&positioning).unwrap_or(clip),
            PaintingArea::Canvas(_) => clip,
            PaintingArea::Cell(_) => positioning,
        };
        let tiling = match (
            areas.painting,
            layer_value(&style.background_size, i),
            layer_value(&style.background_position_x, i),
            layer_value(&style.background_position_y, i),
            layer_value(&style.background_repeat, i),
        ) {
            (
                PaintingArea::Own | PaintingArea::Canvas(_),
                Some(size),
                Some(position_x),
                Some(position_y),
                Some(&repeat),
            ) => {
                let layer = background::Layer {
                    size,
                    position_x,
                    position_y,
                    repeat,
                };
                Some(background::gradient_tiling(&layer, positioning, clip)?)
            }
            _ => None,
        };
        let Some(tiling) = tiling else {
            return Some(item(rect, rect));
        };
        // The default layer: one tile that is the positioning area, painted
        // as one gradient.
        let t = tiling.tile;
        let plain = tiling.single && same_rect(&t, &positioning);
        Some(if plain {
            // The gradient's box is the positioning area, also where the
            // clip is smaller (`content-box`).
            let own = matches!(areas.painting, PaintingArea::Own);
            item(rect, if own { t } else { rect })
        } else {
            item(tiling.area, t)
        })
    }

    /// Paints the borders of a table with collapsing borders. `origin` is
    /// the table's border-box origin.
    fn collapsed_borders(&mut self, edges: &[CollapsedEdge], origin: Point) {
        for edge in edges {
            let width = if edge.vertical {
                edge.rect.width
            } else {
                edge.rect.height
            };
            if width <= 0.0 || edge.style == BorderStyle::None || edge.style == BorderStyle::Hidden
            {
                continue;
            }
            // A one-sided border: the left side of a vertical segment, the
            // top side of a horizontal one.
            let widths = if edge.vertical {
                [0.0, 0.0, 0.0, width]
            } else {
                [width, 0.0, 0.0, 0.0]
            };
            self.list.push(DisplayItem::Border {
                rect: edge.rect.translate(origin),
                widths,
                colors: [edge.color; 4],
                styles: [edge.style; 4],
                radii: [(0.0, 0.0); 4],
            });
        }
    }

    fn border(&mut self, b: &BoxFragment, rect: Rect, radii: Radii) {
        let s = &b.style;
        let widths = [b.border.top, b.border.right, b.border.bottom, b.border.left];
        if widths.iter().all(|w| *w <= 0.0) {
            return;
        }
        let colors = [
            s.border_top_color.resolve(s.color),
            s.border_right_color.resolve(s.color),
            s.border_bottom_color.resolve(s.color),
            s.border_left_color.resolve(s.color),
        ];
        let styles = [
            s.border_top_style,
            s.border_right_style,
            s.border_bottom_style,
            s.border_left_style,
        ];
        // A border of one width, color and solid style on a box with
        // `corner-shape` is the ring between two outlines.
        let uniform = widths.iter().all(|w| *w == widths[0])
            && colors.iter().all(|c| *c == colors[0])
            && styles.iter().all(|st| *st == BorderStyle::Solid);
        if uniform
            && has_rounded_corner(&radii)
            && let Some(k) = corner_shape::shapes(s)
            && let Some(ring) = corner_shape::ring(rect, radii, k, widths[0], colors[0])
        {
            self.list.push(ring);
            return;
        }
        self.list.push(DisplayItem::Border {
            rect,
            widths,
            colors,
            styles,
            radii,
        });
    }

    /// Paints a text fragment with the decorations of its ancestors. The
    /// decoration of the element that contains the text is already in
    /// `decorations` (added by that element's box).
    fn text(&mut self, t: &TextFragment, origin: Point, decorations: &[Decoration]) {
        let style = &t.style;
        if style.visibility != Visibility::Visible {
            return;
        }
        let rect = t.rect.translate(origin);
        if style.receives_pointer_events() {
            self.list
                .push(DisplayItem::HitRegion { rect, node: t.node });
        }
        if t.glyphs.is_empty() {
            return;
        }
        let baseline = Point::new(rect.x, rect.y + t.baseline);
        // The highlight fills the line box, as in Chromium.
        let selected = self
            .highlights
            .selected(t.node)
            .filter(|_| t.is_selectable())
            .and_then(|(start, end)| t.x_range(start, end))
            .map(|(x0, x1)| Rect::new(rect.x + x0, rect.y + t.line_top, x1 - x0, t.line_height));
        if let Some(selected) = selected {
            self.list.push(DisplayItem::Rect {
                rect: selected,
                radii: [(0.0, 0.0); 4],
                color: SELECTION_BACKGROUND,
            });
        }
        self.text_and_decorations(t, rect, baseline, decorations, None);
        if let Some(selected) = selected {
            // The selected part again, in the selection color.
            self.list.push(DisplayItem::PushClip(selected));
            self.text_and_decorations(t, rect, baseline, decorations, Some(SELECTION_TEXT));
            self.list.push(DisplayItem::PopClip);
        }
    }

    /// Paints the glyphs and the decorations of a text fragment, in their
    /// own colors or all in `color`. Underlines and overlines are painted
    /// below the text, line-through above it.
    fn text_and_decorations(
        &mut self,
        t: &TextFragment,
        rect: Rect,
        baseline: Point,
        decorations: &[Decoration],
        color: Option<Rgba>,
    ) {
        for d in decorations {
            let line_color = color.unwrap_or(d.color);
            for line in [TextDecorationLine::UNDERLINE, TextDecorationLine::OVERLINE] {
                if d.lines.contains(line) {
                    let r = decoration_rect(line, rect, baseline.y, t.font_size);
                    self.decoration_line(r, line_color);
                }
            }
        }
        let style = &t.style;
        if color.is_none() {
            self.text_shadows(t, baseline);
        }
        self.list.push(DisplayItem::Text {
            origin: baseline,
            font: t.font,
            size: t.font_size,
            glyphs: Arc::clone(&t.glyphs),
            color: color.unwrap_or_else(|| style.text_fill_color.resolve(style.color)),
        });
        let stroke = style.text_stroke_color.resolve(style.color);
        if color.is_none() && style.text_stroke_width > 0.0 && !stroke.is_transparent() {
            self.list.push(DisplayItem::TextStroke {
                origin: baseline,
                font: t.font,
                size: t.font_size,
                glyphs: Arc::clone(&t.glyphs),
                width: style.text_stroke_width,
                color: stroke,
            });
        }
        for d in decorations {
            if d.lines.contains(TextDecorationLine::LINE_THROUGH) {
                let r = decoration_rect(
                    TextDecorationLine::LINE_THROUGH,
                    rect,
                    baseline.y,
                    t.font_size,
                );
                self.decoration_line(r, color.unwrap_or(d.color));
            }
        }
    }

    /// Paints the `text-shadow` of a text fragment: the shadows below the
    /// text, the last one of the list first. A blurred shadow is the text
    /// in the shadow's color, moved by the offsets, in a group that blurs
    /// it (the standard deviation is half the blur radius, CSS Text
    /// Decoration 3 §4). The first [`MAX_SHADOWS`] shadows paint.
    fn text_shadows(&mut self, t: &TextFragment, baseline: Point) {
        let style = &t.style;
        let count = style.text_shadow.len().min(box_shadow::MAX_SHADOWS);
        for shadow in style.text_shadow[..count].iter().rev() {
            let color = shadow.color.resolve(style.color);
            if color.is_transparent() {
                continue;
            }
            let blurred = shadow.blur > 0.0;
            if blurred {
                self.list.push(DisplayItem::PushFilter {
                    filter: Arc::from([swb_style::FilterFunction::Blur(shadow.blur / 2.0)]),
                    current_color: style.color,
                    bounds: Rect::default(),
                    fixed_bounds: None,
                    escapes_clips: false,
                });
            }
            self.list.push(DisplayItem::Text {
                origin: Point::new(baseline.x + shadow.x, baseline.y + shadow.y),
                font: t.font,
                size: t.font_size,
                glyphs: Arc::clone(&t.glyphs),
                color,
            });
            if blurred {
                self.list.push(DisplayItem::PopFilter);
            }
        }
    }

    fn decoration_line(&mut self, rect: Rect, color: Rgba) {
        self.list.push(DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color,
        });
    }
}

/// The rectangle of the decoration line `line` (one of `underline`,
/// `overline` and `line-through`) of a text fragment whose box is `rect`,
/// with its baseline at `baseline_y`.
fn decoration_rect(line: TextDecorationLine, rect: Rect, baseline_y: f32, font_size: f32) -> Rect {
    let thickness = (font_size / 16.0).max(1.0).round();
    let y = if line == TextDecorationLine::UNDERLINE {
        (baseline_y + (font_size / 9.0).max(1.0)).round()
    } else if line == TextDecorationLine::OVERLINE {
        rect.y.round()
    } else {
        (baseline_y - font_size * 0.3).round()
    };
    Rect::new(rect.x, y, rect.width, thickness)
}

/// The border box of `b` united with the boxes and text of its descendants
/// (Chromium's focus ring encloses them, for example an image in a link).
/// The content of a box that clips its overflow does not count. `origin`
/// is the absolute position of the parent's border-box origin.
pub(crate) fn with_descendants(b: &BoxFragment, origin: Point) -> Rect {
    let rect = b.border_rect.translate(origin);
    if b.style.clips_x() || b.style.clips_y() {
        return rect;
    }
    let own = rect.origin();
    b.children.iter().fold(rect, |acc, child| {
        let r = match child {
            Fragment::Box(child) => with_descendants(child, own),
            Fragment::Text(t) => t.rect.translate(own),
        };
        if r.width > 0.0 || r.height > 0.0 {
            acc.union(&r)
        } else {
            acc
        }
    })
}

/// The value of a background property for layer `i`. The lists of the
/// properties repeat as needed (CSS Backgrounds 3 §2.1,
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>); `None` only for an
/// empty list.
pub(crate) fn layer_value<T>(list: &[T], i: usize) -> Option<&T> {
    list.get(i % list.len().max(1))
}

/// The decorations that the content of `b` gets: those propagated from
/// its ancestors and its own. Decorations propagate to inline content and
/// in-flow blocks, but not into atomic inlines, floats or absolutely
/// positioned boxes.
fn child_decorations(b: &BoxFragment, decorations: &[Decoration]) -> Vec<Decoration> {
    let style = &b.style;
    let mut own: Vec<Decoration> = if b.is_inline || is_block_container_for_text(style) {
        decorations.to_vec()
    } else {
        Vec::new()
    };
    if !style.text_decoration_line.is_empty() {
        own.push(Decoration {
            lines: style.text_decoration_line,
            color: style.text_decoration_color.resolve(style.color),
        });
    }
    own
}

/// True for block containers whose text inherits their decorations
/// (everything except atomic inlines, floats and positioned boxes).
fn is_block_container_for_text(style: &ComputedStyle) -> bool {
    !style.display.is_inline_level() && !style.is_floating() && !style.is_absolutely_positioned()
}

/// The corner radii of the padding edge of `b` (at `origin`), or `None`
/// if the corners are square or both axes do not clip: the border radius
/// minus the border width on each side, at least 0 (CSS Backgrounds 3
/// §4.2, <https://www.w3.org/TR/css-backgrounds-3/#corner-shaping>).
fn padding_radii(b: &BoxFragment, origin: Point) -> Option<Radii> {
    let style = &b.style;
    if !(style.clips_x() && style.clips_y()) {
        return None;
    }
    let outer = resolve_radii(style, b.border_rect.translate(origin));
    let e = &b.border;
    let inner = [
        (
            (outer[0].0 - e.left).max(0.0),
            (outer[0].1 - e.top).max(0.0),
        ),
        (
            (outer[1].0 - e.right).max(0.0),
            (outer[1].1 - e.top).max(0.0),
        ),
        (
            (outer[2].0 - e.right).max(0.0),
            (outer[2].1 - e.bottom).max(0.0),
        ),
        (
            (outer[3].0 - e.left).max(0.0),
            (outer[3].1 - e.bottom).max(0.0),
        ),
    ];
    has_rounded_corner(&inner).then_some(inner)
}

/// The item that starts a clip to `rect`, rounded by `radii`: a plain
/// clip rectangle, or a group that the rasterizer multiplies by the
/// coverage of the rounded rectangle (like an SVG clip path).
fn clip_item(rect: Rect, radii: Option<Radii>) -> DisplayItem {
    radii
        .and_then(|radii| rounded_clip(rect, &radii))
        .unwrap_or(DisplayItem::PushClip(rect))
}

/// The item that ends the clip that [`clip_item`] started.
fn unclip_item(radii: Option<Radii>) -> DisplayItem {
    // `Builder::push_overflow_clip` keeps radii only if `rounded_clip`
    // accepts them, so `clip_item` never falls back to a plain clip here.
    if radii.is_some() {
        DisplayItem::PopSvgClip
    } else {
        DisplayItem::PopClip
    }
}

/// A group clipped to the rounded rectangle, or `None` for an empty
/// rectangle or one that is not finite. The radii are scaled down to fit
/// (CSS Backgrounds 3 §4.5).
fn rounded_clip(rect: Rect, radii: &Radii) -> Option<DisplayItem> {
    let path = rounded_path(rect, radii)?;
    Some(path_clip_item(
        path,
        FillRule::NonZero,
        Matrix::IDENTITY,
        rect,
        false,
    ))
}

/// The group that is clipped to `path` (in the coordinates that
/// `transform` maps to the list's) with `rule`; `bounds` is the extent of
/// the path in the list's coordinates. If the layer or the coverage does
/// not fit into the budgets, the group is clipped to `bounds` instead of
/// being hidden. `backdrop_root` says whether the group is a backdrop root
/// (`clip-path`: yes, the rounded overflow clip: no).
pub(crate) fn path_clip_item(
    path: SvgPath,
    rule: FillRule,
    transform: Matrix,
    bounds: Rect,
    backdrop_root: bool,
) -> DisplayItem {
    use swb_layout::svg::ClipShape;
    let clip = ClipPath {
        shapes: vec![ClipShape {
            path: Arc::new(path),
            transform: Matrix::IDENTITY,
            rule,
            clip: None,
        }],
        outer: None,
    };
    DisplayItem::PushSvgClip {
        clip: Arc::new(clip),
        transform,
        bounds,
        rect_fallback: true,
        backdrop_root,
    }
}

/// Resolves `border-*-radius` against the border box, scaling down
/// overlapping radii (CSS Backgrounds 3 §4.5,
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-overlap>).
pub(crate) fn resolve_radii(style: &ComputedStyle, rect: Rect) -> Radii {
    let r = |c: &swb_style::CornerRadius| {
        (
            c.horizontal.resolve(rect.width).max(0.0),
            c.vertical.resolve(rect.height).max(0.0),
        )
    };
    let mut radii = [
        r(&style.border_top_left_radius),
        r(&style.border_top_right_radius),
        r(&style.border_bottom_right_radius),
        r(&style.border_bottom_left_radius),
    ];
    let sums = [
        (radii[0].0 + radii[1].0, rect.width),
        (radii[1].1 + radii[2].1, rect.height),
        (radii[2].0 + radii[3].0, rect.width),
        (radii[3].1 + radii[0].1, rect.height),
    ];
    let f = sums
        .iter()
        .filter(|(sum, _)| *sum > 0.0)
        .map(|(sum, len)| len / sum)
        .fold(1.0_f32, f32::min);
    if f < 1.0 {
        for (h, v) in &mut radii {
            *h *= f;
            *v *= f;
        }
    }
    radii
}
