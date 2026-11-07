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
    forms_stacking_context, has_transform, is_absolute_containing_block, is_fixed_containing_block,
    rounded_path,
};
use swb_style::{
    BackgroundBox, BorderStyle, ComputedStyle, Display, FillRule, Image, PointerEvents, Position,
    Rgba, TextDecorationLine, Visibility, ZIndex,
};

use super::items::{
    DisplayItem, DisplayList, Highlights, ImageRef, ImageSizes, Radii, SELECTION_BACKGROUND,
    SELECTION_TEXT, Scrolling,
};
use crate::group_bounds::finish_groups;
use crate::mask::{self, MaskBoxes, MaskLayer};
use crate::rope::ItemRope;
use crate::scroll_indicator::element_scroll_indicators;
use crate::{background, control, media};

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
        tree_order: tree
            .root
            .as_ref()
            .map(positioned_tree_order)
            .unwrap_or_default(),
        backdrop_roots: tree.root.as_ref().map(backdrop_roots).unwrap_or_default(),
        clips: Vec::new(),
        absolute_clips: 0,
        fixed_clips: 0,
        canvas_source: None,
        root_canvas: ItemRope::default(),
        offsets: scrolling.offsets,
        scroll: ScrollState::default(),
        indicators: scrolling.indicators,
        scale: scrolling.scale,
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
        let areas = BackgroundAreas {
            border,
            padding,
            painting: PaintingArea::Canvas(canvas),
        };
        let start = builder.list.len();
        builder.background(bg, &areas, [(0.0, 0.0); 4]);
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
    if let Some(root) = &tree.root {
        let ancestry = Ancestry::root(tree.viewport);
        builder.box_contents(root, Point::default(), &[], &ancestry, true);
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
    // A transformed box counts as positioned (CSS Transforms 1 §3).
    if style.position != Position::Static || has_transform(b) {
        PaintKind::Positioned
    } else if style.is_floating() {
        PaintKind::Float
    } else if b.is_inline {
        PaintKind::Inline
    } else if style.display.is_inline_level()
        || style.opacity < 1.0
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
/// box of a fragment tree, by address. The paint phases find positioned
/// boxes in another order (those in floats and inline content later);
/// with equal z-index they paint in tree order. The recursion depth is
/// bounded by the box tree depth (see `swb_layout`'s `box_tree.rs`).
fn positioned_tree_order(root: &BoxFragment) -> HashMap<*const BoxFragment, u32> {
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
    visit(root, &mut 0, &mut order);
    order
}

/// The boxes that are backdrop roots (Filter Effects 2 §2) and have a
/// descendant with a `backdrop-filter`, by address. The recursion depth is
/// bounded by the box tree depth (see `swb_layout`'s `box_tree.rs`).
fn backdrop_roots(root: &BoxFragment) -> HashSet<*const BoxFragment> {
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
    visit(root, &mut roots);
    roots
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
}

/// A text decoration propagated from an ancestor.
#[derive(Clone, Copy)]
struct Decoration {
    lines: TextDecorationLine,
    color: Rgba,
}

/// The areas that a background uses, in document coordinates.
struct BackgroundAreas {
    border: Rect,
    padding: Rect,
    painting: PaintingArea,
}

/// Where the layers of a background are painted.
#[derive(Clone, Copy)]
enum PaintingArea {
    /// The box's own area that `background-clip` selects.
    Own,
    /// The canvas, for the root background. A gradient fills all of it.
    Canvas(Rect),
    /// A table cell, for the background of a row, row group, column or
    /// column group: the layers are positioned in the part's areas and
    /// painted in the cell (CSS 2.2 §17.5.1).
    Cell(Rect),
}

impl BackgroundAreas {
    fn of(b: &BoxFragment, origin: Point) -> Self {
        BackgroundAreas {
            border: b.border_rect.translate(origin),
            padding: b.padding_rect().translate(origin),
            painting: PaintingArea::Own,
        }
    }

    fn area(&self, which: BackgroundBox) -> Rect {
        match which {
            BackgroundBox::BorderBox => self.border,
            // `content-box` is painted as `padding-box` (not supported yet).
            _ => self.padding,
        }
    }

    fn painting_area(&self, clip: BackgroundBox) -> Rect {
        match self.painting {
            PaintingArea::Own => self.area(clip),
            PaintingArea::Canvas(area) | PaintingArea::Cell(area) => area,
        }
    }
}

/// What [`Builder::open_group`] started for a box.
struct Group {
    /// The index of the `PushOpacity` item, if the box has opacity < 1.
    opacity_item: Option<usize>,
    /// The index of the `PushMask` item, if the box is masked.
    mask_item: Option<usize>,
    /// The item that ends the clip of the `clip-path` of the box (a basic
    /// shape or a geometry box), if it has one.
    clip_pop: Option<DisplayItem>,
    /// True if the box started a filter group.
    filter: bool,
    /// The number of groups (opacity, mask, clip path, filter) that the box
    /// started: each one is a layer in the rasterizer.
    layers: u8,
    /// True if [`Builder::open_group`] already pushed the backdrop item of
    /// the box (before its opacity group).
    backdrop_done: bool,
    /// True if the box opened a stacking context.
    context: bool,
    /// `contexts.len()` before the box's own context.
    outer_depth: usize,
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
        // Only positioned and transformed boxes come here (`walk`).
        debug_assert!(positioned || has_transform(b));
        // A positioned or transformed box is painted after the normal-flow
        // content of the enclosing stacking context, in z-index order
        // (`auto` as 0; a transformed box counts as positioned, CSS
        // Transforms 1 §3). It forms a stacking context if it has an
        // integer z-index, a transform, or fixed or sticky positioning.
        let z = match style.z_index {
            ZIndex::Integer(z) if positioned => z,
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
        self.list
            .extend(clips.iter().map(|&(_, radii)| unclip_item(radii)));
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
        let transforms = self.open_transforms(&groups);
        let group = self.open_group(b, origin, stacking_context);
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
        if self.indicators && b.style.visibility == Visibility::Visible {
            self.list
                .extend(element_scroll_indicators(b, rect, self.offsets));
        }
        self.outline(b, origin);
        self.close_backdrop(backdrop_root);
        if clip_property {
            self.pop_clip();
        }
        self.close_group(&group, b, origin, positioned);
        self.close_transforms(transforms);
    }

    /// Starts the transform groups `groups` of a box (see
    /// [`Ancestry::group_transforms`]). Returns the indices of the
    /// `PushTransform` items.
    fn open_transforms(&mut self, groups: &[GroupTransform]) -> std::ops::Range<usize> {
        let start = self.list.len();
        self.list
            .extend(groups.iter().map(|transform| DisplayItem::PushTransform {
                transform: transform.clone(),
                bounds: Rect::default(),
                hit_bounds: Rect::default(),
                fixed_bounds: None,
            }));
        start..self.list.len()
    }

    /// Ends the transform groups that [`Builder::open_transforms`]
    /// started. Their bounds are computed when the list is finished
    /// (`group_bounds.rs`).
    fn close_transforms(&mut self, transforms: std::ops::Range<usize>) {
        self.list
            .extend(transforms.map(|_| DisplayItem::PopTransform));
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
        let group = self.open_group(b, origin, false);
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
            if self.indicators && b.style.visibility == Visibility::Visible {
                self.list
                    .extend(element_scroll_indicators(b, rect, self.offsets));
            }
            self.outline(b, origin);
        }
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

    /// Starts the opacity group, the mask group, the clip of a basic shape
    /// and the stacking context of a box, as needed. `origin` is the
    /// parent's border-box origin. The effects apply in the order of CSS
    /// Masking 1 §3: the clip first, then the mask, then the opacity.
    fn open_group(&mut self, b: &BoxFragment, origin: Point, stacking_context: bool) -> Group {
        let opacity = b.style.opacity;
        let outer_depth = self.contexts.len();
        // The opacity of a box with a `backdrop-filter` applies to the
        // filtered backdrop and to the box separately (measured in Chromium
        // 148: the backdrop is drawn with the opacity, then the box's group
        // with it), so the backdrop item comes before the opacity group.
        // With a mask, a clip path or a filter on the box, they apply to
        // both, and the opacity group holds both (departure).
        let backdrop_done = opacity < 1.0
            && opacity > 0.0
            && !b.style.has_mask()
            && !b.style.clip_path.is_shape()
            && !b.style.has_filter()
            && self.backdrop_item(b, origin, (0, opacity), outer_depth == 0);
        let opacity_item = (opacity < 1.0).then(|| {
            self.list.push(DisplayItem::PushOpacity {
                opacity: opacity.max(0.0),
                bounds: Rect::default(),
                fixed_bounds: None,
                escapes_clips: false,
            });
            self.list.len() - 1
        });
        // The layers are computed when the group ends.
        let mask_item = b.style.has_mask().then(|| {
            self.list.push(DisplayItem::PushMask {
                bounds: Rect::default(),
                layers: Arc::from([]),
            });
            self.list.len() - 1
        });
        let clip = crate::clip_path::box_clip(b, origin);
        let clip_pop = clip.map(|clip| {
            self.list.push(clip.push);
            clip.pop
        });
        // The filter applies first, inside the clip (Filter Effects 1 §5).
        // A `url()` and functions that change nothing paint no group.
        let filtered = b.style.has_filter();
        let filter = filtered && self.push_filter(b);
        if (opacity_item.is_some() || mask_item.is_some() || clip_pop.is_some() || filtered)
            && outer_depth == 0
        {
            let canvas = std::mem::take(&mut self.root_canvas);
            self.list.append(canvas);
        }
        let context = stacking_context
            || opacity_item.is_some()
            || mask_item.is_some()
            || clip_pop.is_some()
            || filtered
            || b.style.has_backdrop_filter()
            || b.style.will_change_makes_stacking_context()
            || outer_depth == 0;
        if context {
            self.contexts.push(Vec::new());
        }
        let layers = u8::from(opacity_item.is_some())
            + u8::from(mask_item.is_some())
            + u8::from(matches!(clip_pop, Some(DisplayItem::PopSvgClip)))
            + u8::from(filter);
        Group {
            opacity_item,
            mask_item,
            clip_pop,
            filter,
            layers,
            backdrop_done,
            context,
            outer_depth,
        }
    }

    /// Draws the filtered backdrop of a box (its `backdrop-filter`) and
    /// starts the layer of a backdrop root with `backdrop-filter`
    /// descendants that has none. Call it after [`Builder::open_group`],
    /// before the box paints. Returns true if it started a layer, which
    /// [`Builder::close_backdrop`] ends.
    ///
    /// The root layer starts after the box's own backdrop item: the
    /// backdrop of a descendant holds what the box painted before it, not
    /// the filtered backdrop of the box (measured in Chromium 148).
    fn open_backdrop(&mut self, b: &BoxFragment, origin: Point, group: &Group) -> bool {
        let style = &b.style;
        if !group.backdrop_done {
            self.backdrop_item(b, origin, (group.layers, 1.0), group.outer_depth == 0);
        }
        let root = self.backdrop_roots.contains(&std::ptr::from_ref(b))
            && (group.layers == 0 || style.has_backdrop_filter());
        if root {
            self.list.push(DisplayItem::PushOpacity {
                opacity: 1.0,
                bounds: Rect::default(),
                fixed_bounds: None,
                escapes_clips: false,
            });
        }
        root
    }

    /// Pushes the backdrop item of a box with a `backdrop-filter` that
    /// paints: `layers` groups of the box are open, and `alpha` is the
    /// opacity that the filtered backdrop is drawn with. Returns true if
    /// it pushed the item. The root element has none: nothing is painted
    /// before it but the canvas.
    fn backdrop_item(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        (layers, alpha): (u8, f32),
        is_root: bool,
    ) -> bool {
        let style = &b.style;
        let Some(list) = style.backdrop_filter.list() else {
            return false;
        };
        if is_root
            || !crate::filter::paints(list)
            || style.visibility != Visibility::Visible
            || b.content == BoxContent::GeometryOnly
        {
            return false;
        }
        let rect = b.border_rect.translate(origin);
        if !(rect.width > 0.0 && rect.height > 0.0) {
            return false;
        }
        let radii = resolve_radii(style, rect);
        let rounded = radii.iter().any(|&(h, v)| h > 0.0 && v > 0.0);
        self.list.push(DisplayItem::BackdropFilter {
            filter: Arc::clone(list),
            current_color: style.color,
            rect,
            radii: rounded.then_some(radii),
            alpha,
            layers,
        });
        true
    }

    /// Ends the layer that [`Builder::open_backdrop`] started.
    fn close_backdrop(&mut self, started: bool) {
        if started {
            self.list.push(DisplayItem::PopOpacity);
        }
    }

    /// Starts the filter group of a box if its `filter` paints. Returns
    /// true if it did. The bounds are computed when the list is finished.
    fn push_filter(&mut self, b: &BoxFragment) -> bool {
        let Some(list) = b.style.filter.list() else {
            return false;
        };
        if !crate::filter::paints(list) {
            return false;
        }
        self.list.push(DisplayItem::PushFilter {
            filter: Arc::clone(list),
            current_color: b.style.color,
            bounds: Rect::default(),
            fixed_bounds: None,
            escapes_clips: false,
        });
        true
    }

    /// Ends the mask and opacity groups of a box and records the areas
    /// they draw. `origin` is the parent's border-box origin; `positioned`
    /// the area of the positioned descendants, if a `no-clip` mask needs
    /// it.
    fn close_group(
        &mut self,
        group: &Group,
        b: &BoxFragment,
        origin: Point,
        positioned: Option<Rect>,
    ) {
        if group.filter {
            self.list.push(DisplayItem::PopFilter);
        }
        if let Some(pop) = &group.clip_pop {
            self.list.push(pop.clone());
        }
        if let Some(at) = group.mask_item {
            // With `no-clip`, the area of the descendants with their own
            // layer in Chromium: positioned ones, and nested groups (with
            // the positioned descendants inside them). Each `no-clip` mask
            // reads the items of its content once.
            let layered = mask::needs_positioned_area(&b.style)
                .then(|| {
                    positioned
                        .into_iter()
                        .chain(mask::group_area(self.list.iter_from(at + 1)))
                        .reduce(|a, b| a.union(&b))
                })
                .flatten();
            let boxes = MaskBoxes::of(b, origin, layered);
            let layers = mask::layers(&b.style, &boxes, self.images);
            // The bounds are the content inside this extent (computed when
            // the list is finished, `group_bounds.rs`).
            let extent = layers
                .iter()
                .filter_map(MaskLayer::extent)
                .reduce(|a, b| a.union(&b))
                .unwrap_or_default();
            if let Some(DisplayItem::PushMask {
                bounds: bounds_slot,
                layers: layers_slot,
            }) = self.list.get_mut(at)
            {
                *bounds_slot = extent;
                *layers_slot = Arc::from(layers);
            }
            self.list.push(DisplayItem::PopMask);
        }
        if group.opacity_item.is_some() {
            self.list.push(DisplayItem::PopOpacity);
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
            && style.pointer_events != PointerEvents::None
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
                    let part_areas = BackgroundAreas {
                        border: area,
                        padding: area,
                        painting: PaintingArea::Cell(part.clip.translate(rect.origin())),
                    };
                    self.background(&part.style, &part_areas, [(0.0, 0.0); 4]);
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
                areas = BackgroundAreas {
                    border: border_rect,
                    padding: border_rect.inset(&b.border),
                    painting: PaintingArea::Own,
                };
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
        if !paints_canvas && !native {
            self.background(style, &areas, radii);
        }
        if paint_border && !native {
            self.border(b, border_rect, radii);
        }
        if let BoxContent::Control(c) = &b.content {
            self.list.extend(control::native_look(c, style, rect));
        }
        let content = b.content_rect().translate(origin);
        match &b.content {
            BoxContent::Image(node) => self.replaced_image(*node, style, content),
            BoxContent::Media(m) => {
                if let Some(fill) = media::default_poster(m, content) {
                    self.list.push(fill);
                } else if !m.audio {
                    self.replaced_image(m.node, style, content);
                }
                self.list.extend(media::controls(m, content));
            }
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
                    interactive: style.pointer_events != PointerEvents::None,
                });
                if rounded.is_some() {
                    self.list.push(unclip_item(rounded));
                }
            }
            _ => {}
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
        if !(style.overflow_x.clips() || style.overflow_y.clips()) {
            return false;
        }
        let rect = b.padding_rect().translate(origin);
        let radii = padding_radii(b, origin).filter(|r| rounded_clip(rect, r).is_some());
        self.list.push(clip_item(rect, radii));
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
        let outer = outset(rect, w + style.outline_offset);
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
            (outset(rect, distance), radii)
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

    fn background(&mut self, style: &ComputedStyle, areas: &BackgroundAreas, radii: Radii) {
        let color = style.background_color.resolve(style.color);
        // The color is painted with the clip of the bottom layer.
        let color_clip = style
            .background_clip
            .last()
            .copied()
            .unwrap_or(BackgroundBox::BorderBox);
        if !color.is_transparent() {
            self.list.push(DisplayItem::Rect {
                rect: areas.painting_area(color_clip),
                radii: match areas.painting {
                    PaintingArea::Own => radii,
                    PaintingArea::Canvas(_) | PaintingArea::Cell(_) => [(0.0, 0.0); 4],
                },
                color,
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
            let clip = areas.painting_area(clip);
            match image {
                Image::Url(url) => {
                    let image_ref = ImageRef::Url(Arc::clone(url));
                    let (
                        Some(natural),
                        Some(size),
                        Some(position_x),
                        Some(position_y),
                        Some(&repeat),
                    ) = (
                        self.images.size(&image_ref),
                        layer_value(&style.background_size, i),
                        layer_value(&style.background_position_x, i),
                        layer_value(&style.background_position_y, i),
                        layer_value(&style.background_repeat, i),
                    )
                    else {
                        continue;
                    };
                    let layer = background::Layer {
                        size,
                        position_x,
                        position_y,
                        repeat,
                    };
                    let (tile, area) = background::tile(&layer, positioning, clip, &natural);
                    self.list.push(DisplayItem::Image {
                        image: image_ref,
                        rect: area,
                        tile,
                        clip,
                    });
                }
                Image::LinearGradient(gradient) => {
                    // Gradients are not tiled yet: one gradient fills the
                    // positioning area (the whole canvas for the canvas).
                    let rect = match areas.painting {
                        PaintingArea::Own => clip.intersection(&positioning).unwrap_or(clip),
                        PaintingArea::Canvas(_) => clip,
                        PaintingArea::Cell(_) => positioning,
                    };
                    self.list.push(DisplayItem::LinearGradient {
                        rect,
                        clip,
                        gradient: Arc::clone(gradient),
                        current_color: style.color,
                    });
                }
            }
        }
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
        if style.pointer_events != PointerEvents::None {
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
        let thickness = (t.font_size / 16.0).max(1.0).round();
        for d in decorations {
            let line_color = color.unwrap_or(d.color);
            if d.lines.contains(TextDecorationLine::UNDERLINE) {
                let y = (baseline.y + (t.font_size / 9.0).max(1.0)).round();
                self.decoration_line(rect.x, y, rect.width, thickness, line_color);
            }
            if d.lines.contains(TextDecorationLine::OVERLINE) {
                self.decoration_line(rect.x, rect.y.round(), rect.width, thickness, line_color);
            }
        }
        self.list.push(DisplayItem::Text {
            origin: baseline,
            font: t.font,
            size: t.font_size,
            glyphs: Arc::clone(&t.glyphs),
            color: color.unwrap_or(t.style.color),
        });
        for d in decorations {
            if d.lines.contains(TextDecorationLine::LINE_THROUGH) {
                let y = (baseline.y - t.font_size * 0.3).round();
                self.decoration_line(rect.x, y, rect.width, thickness, color.unwrap_or(d.color));
            }
        }
    }

    fn decoration_line(&mut self, x: f32, y: f32, width: f32, thickness: f32, color: Rgba) {
        self.list.push(DisplayItem::Rect {
            rect: Rect::new(x, y, width, thickness),
            radii: [(0.0, 0.0); 4],
            color,
        });
    }
}

/// The border box of `b` united with the boxes and text of its descendants
/// (Chromium's focus ring encloses them, for example an image in a link).
/// The content of a box that clips its overflow does not count. `origin`
/// is the absolute position of the parent's border-box origin.
pub(crate) fn with_descendants(b: &BoxFragment, origin: Point) -> Rect {
    let rect = b.border_rect.translate(origin);
    if b.style.overflow_x.clips() || b.style.overflow_y.clips() {
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

/// `rect` grown by `amount` on every side (shrunk if negative).
pub(crate) fn outset(rect: Rect, amount: f32) -> Rect {
    Rect::new(
        rect.x - amount,
        rect.y - amount,
        rect.width + 2.0 * amount,
        rect.height + 2.0 * amount,
    )
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
    if !(style.overflow_x.clips() && style.overflow_y.clips()) {
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
    inner
        .iter()
        .any(|&(h, v)| h > 0.0 && v > 0.0)
        .then_some(inner)
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
