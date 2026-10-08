//! Box tree construction: turns the DOM and computed styles into boxes.
//!
//! Follows CSS 2.2 §9.2 (<https://www.w3.org/TR/CSS22/visuren.html#box-gen>)
//! and CSS Display 3 (<https://www.w3.org/TR/css-display-3/>):
//!
//! - A block container holds either only block-level boxes or only inline
//!   content (an inline formatting context). Inline content next to
//!   block-level boxes is wrapped in anonymous block boxes.
//! - A block-level box inside an inline box splits the inline box: the
//!   inline content before and after the block goes into separate anonymous
//!   blocks, and the inline box is continued in both.
//! - Inline content is stored as a flat list of items with start/end
//!   markers for inline boxes, which makes line breaking straightforward.
//! - A float belongs to the inline content around it (also at its start,
//!   as in Chromium); it becomes a block-level box only if that inline
//!   content is empty. Inline content that is empty but has a whole inline
//!   box keeps its anonymous block, so that the inline box gets a box.
//!
//! White-space processing (CSS Text 3 §4.1.1) happens when an inline
//! formatting context is finished, because spaces collapse across inline
//! box boundaries.
//!
//! Boxes nest at most [`MAX_BOX_DEPTH`] levels deep. The content of
//! elements below that depth is flattened into the box at the limit (only
//! text and line breaks are kept). This bounds the recursion of box
//! construction, layout and paint, also for documents that were not built
//! by the HTML parser (which has its own depth limit).
//!
//! Generated text (the `content` of `::before`, `::after` and `::marker`,
//! and list marker numbers) is limited to [`MAX_GENERATED_TEXT`] bytes per
//! box tree.

use std::ops::Range;
use std::sync::Arc;

use swb_dom::{Document, NodeData, NodeId, local_name};
use swb_style::{
    ComputedStyle, Display, LengthPercentageOrAuto, Overflow, PseudoKind, StyleMap, TextTransform,
    WhiteSpace,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::control::FormControls;
use crate::media::Media;
use crate::source_map::{CharSource, SourceMap};
use crate::{NaturalSize, ReplacedSizes};

/// The maximum nesting depth of boxes (elements, pseudo-elements and
/// `display: contents` elements count). With this limit, box construction,
/// layout and paint run on a 2 MiB stack (the default stack size of Rust
/// threads); see `tests/deep_nesting.rs`. Chromium's HTML parser limits the
/// DOM depth to 512 for the same reason.
pub(crate) const MAX_BOX_DEPTH: usize = 256;

/// The maximum length in bytes of all generated text of one box tree. One
/// `content` string in a style sheet can apply to every element, so a
/// small page could otherwise produce hundreds of megabytes of text (100 KB
/// on 3,000 elements took 13 GB in layout). Real pages generate a few
/// kilobytes. 1 MiB of short words still takes about 250 MB in layout.
/// The text after the limit is dropped. Counter text also has a limit of
/// its own (4 MiB, ADR 0007). Chromium has no such limit.
pub(crate) const MAX_GENERATED_TEXT: usize = 1 << 20;

/// The style and origin of a box.
#[derive(Clone, Debug)]
pub(crate) struct BoxBase {
    pub(crate) node: Option<NodeId>,
    pub(crate) pseudo: Option<PseudoKind>,
    pub(crate) style: Arc<ComputedStyle>,
    /// A number that identifies the box during one layout pass. The parts
    /// of an inline box that is split by a block share the number.
    pub(crate) id: usize,
}

impl BoxBase {
    /// The element whose own box this is. `None` for anonymous boxes and
    /// for pseudo-element boxes (their `node` is the originating element).
    pub(crate) fn element(&self) -> Option<NodeId> {
        self.node.filter(|_| self.pseudo.is_none())
    }
}

/// A block-level box.
#[derive(Debug)]
pub(crate) enum BlockLevelBox {
    /// A block container in the normal flow that does not establish a new
    /// block formatting context.
    Block {
        base: BoxBase,
        contents: BlockContainer,
        marker: Option<Marker>,
    },
    /// A block-level box that establishes an independent formatting context
    /// (flow-root, flex, replaced, ...).
    Independent(IndependentBox),
    /// A float.
    Float(IndependentBox),
    /// An absolutely positioned box.
    AbsolutelyPositioned(IndependentBox),
    /// An in-flow block-level box inside inline boxes (which it splits).
    InInline(Box<BlockInInline>),
}

/// The most inline boxes around a block-level child that get a box around
/// it (the innermost ones).
const MAX_INLINE_WRAPPERS: usize = 8;

/// A block-level box inside inline boxes. The inline boxes get a box around
/// it (only for geometry, as in Chromium's block-in-inline).
#[derive(Debug)]
pub(crate) struct BlockInInline {
    /// The open inline boxes, outermost first (at most the innermost
    /// [`MAX_INLINE_WRAPPERS`]).
    pub(crate) inline_boxes: Vec<BoxBase>,
    pub(crate) block: BlockLevelBox,
    /// True if any open inline box is positioned (all of them, not only
    /// `inline_boxes`): it is the containing block of the absolutely
    /// positioned boxes in the block (see `BoxFragment::in_positioned_inline`).
    pub(crate) in_positioned_inline: bool,
}

/// The contents of a block container.
#[derive(Debug)]
pub(crate) enum BlockContainer {
    /// Only block-level children.
    Blocks(Vec<BlockLevelBox>),
    /// Only inline-level content.
    Inline(InlineFormattingContext),
}

/// A box that establishes an independent formatting context.
#[derive(Debug)]
pub(crate) struct IndependentBox {
    pub(crate) base: BoxBase,
    pub(crate) contents: IndependentContents,
    pub(crate) marker: Option<Marker>,
}

/// The kind of formatting context an independent box establishes.
#[derive(Debug)]
pub(crate) enum IndependentContents {
    /// A new block formatting context with flow content.
    Flow(BlockContainer),
    /// A flex container with its children: the flex items and the
    /// absolutely positioned children. Every flex item establishes an
    /// independent formatting context.
    Flex(Vec<IndependentBox>),
    /// A grid container with its children: the grid items (built as flex
    /// items are) and the absolutely positioned children.
    Grid(Vec<IndependentBox>),
    /// A replaced element (an image, a video or audio controls).
    Replaced(Replaced),
    /// A table (CSS 2.2 §17): its captions, columns and row groups.
    Table(crate::table::TableBox),
    /// A form control (see `control.rs`).
    Control(Box<crate::control::ControlBox>),
}

/// A replaced element.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Replaced {
    pub(crate) node: NodeId,
    /// The natural dimensions; `None` for an image that is not loaded or
    /// is broken.
    pub(crate) natural_size: Option<NaturalSize>,
    /// The media element, or `None` for an image.
    pub(crate) media: Option<Media>,
}

/// A list item marker.
#[derive(Debug)]
pub(crate) struct Marker {
    pub(crate) base: BoxBase,
    pub(crate) text: String,
    pub(crate) outside: bool,
}

/// An inline formatting context: a flat list of inline items and the text
/// they refer to.
#[derive(Debug, Default)]
pub(crate) struct InlineFormattingContext {
    /// A number that identifies the context during one layout pass (the
    /// key of the shaping cache).
    pub(crate) id: usize,
    pub(crate) items: Vec<InlineItem>,
    /// All text of the context after white-space processing. Atomic inlines
    /// are represented by U+FFFC so that line breaking sees them.
    pub(crate) text: String,
}

impl InlineFormattingContext {
    /// True if the context generates no line box that is not empty: it has
    /// no text, no atomic inlines, no forced line breaks, and no inline box
    /// with a non-zero margin, border or padding on an inline side (CSS 2.2
    /// §9.4.2). Floats and absolutely positioned boxes do not count.
    pub(crate) fn is_empty(&self) -> bool {
        let mut open: Vec<&ComputedStyle> = Vec::new();
        for item in &self.items {
            match item {
                InlineItem::Text { range, .. } => {
                    if !range.is_empty() {
                        return false;
                    }
                }
                InlineItem::StartBox { base, continued } => {
                    if !continued && has_inline_start_edge(&base.style) {
                        return false;
                    }
                    open.push(&base.style);
                }
                InlineItem::EndBox { split } => {
                    if open.pop().is_some_and(|s| !split && has_inline_end_edge(s)) {
                        return false;
                    }
                }
                InlineItem::Atomic { .. } | InlineItem::LineBreak(_) => return false,
                InlineItem::Float(_) | InlineItem::AbsolutelyPositioned(_) => {}
            }
        }
        true
    }

    /// True if an absolutely positioned box comes after a float (its static
    /// position is next to the float, so the content stays inline even if
    /// it makes no line box).
    pub(crate) fn has_absolute_after_float(&self) -> bool {
        let mut float = false;
        for item in &self.items {
            match item {
                InlineItem::Float(_) => float = true,
                InlineItem::AbsolutelyPositioned(_) if float => return true,
                _ => {}
            }
        }
        false
    }

    /// True if an absolutely positioned box is inside an inline box (which
    /// can be its containing block, so the content stays inline even if it
    /// makes no line box).
    pub(crate) fn has_absolute_in_inline_box(&self) -> bool {
        let mut depth = 0_usize;
        for item in &self.items {
            match item {
                InlineItem::StartBox { .. } => depth += 1,
                InlineItem::EndBox { .. } => depth = depth.saturating_sub(1),
                InlineItem::AbsolutelyPositioned(_) if depth > 0 => return true,
                _ => {}
            }
        }
        false
    }
}

impl InlineFormattingContext {
    /// True if the context has an inline box that a block-level child does
    /// not split (the parts of a split box get their box from the wrapper
    /// around the block, see `BlockInInline`).
    pub(crate) fn has_whole_inline_box(&self) -> bool {
        let mut open: Vec<bool> = Vec::new();
        for item in &self.items {
            match item {
                InlineItem::StartBox { continued, .. } => open.push(*continued),
                InlineItem::EndBox { split } => {
                    let whole = open.pop() == Some(false) && !split;
                    if whole {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
}

/// True if a margin, border or padding on the inline-start (left) side of
/// an inline box is non-zero.
pub(crate) fn has_inline_start_edge(style: &ComputedStyle) -> bool {
    nonzero_margin(&style.margin_left)
        || !style.padding_left.is_zero()
        || style.border_left_width > 0.0
}

/// True if a margin, border or padding on the inline-end (right) side of
/// an inline box is non-zero.
pub(crate) fn has_inline_end_edge(style: &ComputedStyle) -> bool {
    nonzero_margin(&style.margin_right)
        || !style.padding_right.is_zero()
        || style.border_right_width > 0.0
}

fn nonzero_margin(m: &LengthPercentageOrAuto) -> bool {
    m.non_auto().is_some_and(|lp| !lp.is_zero())
}

/// One item of an inline formatting context.
#[derive(Debug)]
pub(crate) enum InlineItem {
    /// The start of an inline box. `continued` is true if the box was split
    /// by a block-level child and this is not its first part.
    StartBox { base: BoxBase, continued: bool },
    /// The end of the innermost open inline box. `split` is true if the box
    /// continues after a block-level child.
    EndBox { split: bool },
    /// Text: a range of [`InlineFormattingContext::text`]. `node` is the
    /// text node, or the element for generated content. `source` maps the
    /// range to offsets in the text node; generated content has none.
    Text {
        node: NodeId,
        style: Arc<ComputedStyle>,
        range: Range<usize>,
        source: Option<SourceMap>,
    },
    /// An atomic inline (inline-block, image, ...). Its position in the text
    /// is the U+FFFC at `offset`.
    Atomic {
        inner: IndependentBox,
        offset: usize,
    },
    /// A forced line break: a `<br>` element (with its box) or a preserved
    /// newline.
    LineBreak(Option<BoxBase>),
    /// A float that starts in this inline context.
    Float(IndependentBox),
    /// An absolutely positioned box that starts in this inline context.
    AbsolutelyPositioned(IndependentBox),
}

/// Inputs for box construction.
pub(crate) struct BuildContext<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) styles: &'a StyleMap,
    pub(crate) replaced: &'a dyn ReplacedSizes,
    pub(crate) controls: &'a dyn FormControls,
    /// The element whose `overflow` applies to the viewport (CSS Overflow 3
    /// §3.3). Its box gets the used overflow `visible`.
    pub(crate) overflow_source: Option<NodeId>,
}

impl BuildContext<'_> {
    /// The base of the box of element `node` with computed style `style`.
    pub(crate) fn element_base(&self, node: NodeId, style: &Arc<ComputedStyle>) -> BoxBase {
        let style = if self.overflow_source == Some(node) {
            let mut used = ComputedStyle::clone(style);
            used.overflow_x = Overflow::Visible;
            used.overflow_y = Overflow::Visible;
            Arc::new(used)
        } else {
            Arc::clone(style)
        };
        BoxBase {
            node: Some(node),
            pseudo: None,
            style,
            id: 0,
        }
    }
}

/// The state of box construction.
#[derive(Default)]
pub(crate) struct BuildState {
    /// The nesting depth of the box being built.
    pub(crate) depth: usize,
    /// The last number given out by [`BuildState::next_id`].
    last_id: usize,
    /// True once content was flattened because of [`MAX_BOX_DEPTH`].
    flattened: bool,
    /// The bytes of generated text so far, at most [`MAX_GENERATED_TEXT`].
    generated_text: usize,
    /// True once generated text was cut because of [`MAX_GENERATED_TEXT`].
    generated_cut: bool,
}

impl BuildState {
    pub(crate) fn next_id(&mut self) -> usize {
        self.last_id += 1;
        self.last_id
    }

    /// The text of the `content` of a pseudo-element with style `style`,
    /// limited by [`MAX_GENERATED_TEXT`]. All generated text of a box tree
    /// goes through this method or [`BuildState::generated`].
    pub(crate) fn generated_content(&mut self, style: &ComputedStyle) -> Option<String> {
        swb_style::content_text(style).map(|text| self.generated(text))
    }

    /// `text` of a pseudo-element or marker, cut at a character boundary
    /// to the part that fits into [`MAX_GENERATED_TEXT`].
    fn generated(&mut self, mut text: String) -> String {
        let left = MAX_GENERATED_TEXT.saturating_sub(self.generated_text);
        if text.len() > left {
            if !self.generated_cut {
                self.generated_cut = true;
                log::warn!(
                    "more than {MAX_GENERATED_TEXT} bytes of generated content text; the rest is dropped"
                );
            }
            let mut end = left;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        self.generated_text += text.len();
        text
    }

    /// `base` with a new box number.
    pub(crate) fn numbered(&mut self, mut base: BoxBase) -> BoxBase {
        base.id = self.next_id();
        base
    }

    /// The base of an anonymous box that inherits from `parent`.
    pub(crate) fn anonymous(&mut self, parent: &ComputedStyle, display: Display) -> BoxBase {
        let mut style = ComputedStyle::anonymous_from(parent);
        style.display = display;
        BoxBase {
            node: None,
            pseudo: None,
            style: Arc::new(style),
            id: self.next_id(),
        }
    }
}

/// Builds the box of the root element.
pub(crate) fn build_root(ctx: &BuildContext<'_>) -> Option<IndependentBox> {
    let root = ctx.doc.document_element()?;
    let style = ctx.styles.get(root)?;
    if style.display == Display::None {
        return None;
    }
    let mut state = BuildState::default();
    let base = state.numbered(ctx.element_base(root, style));
    Some(build_independent(ctx, base, &mut state))
}

pub(crate) fn build_independent(
    ctx: &BuildContext<'_>,
    base: BoxBase,
    state: &mut BuildState,
) -> IndependentBox {
    let element = base.element();
    if let Some(replaced) = element.and_then(|node| replaced(ctx, node)) {
        return IndependentBox {
            contents: IndependentContents::Replaced(replaced),
            base,
            marker: None,
        };
    }
    if let Some(node) = element
        && let Some(contents) = crate::control::build(ctx, node, &base, state)
    {
        return IndependentBox {
            contents,
            base,
            marker: None,
        };
    }
    let marker = element.and_then(|n| build_marker(ctx, n, &base.style, state));
    let contents = match base.style.display {
        Display::Flex | Display::InlineFlex => {
            IndependentContents::Flex(build_flex_items(ctx, &base, state))
        }
        Display::Grid | Display::InlineGrid => {
            IndependentContents::Grid(build_flex_items(ctx, &base, state))
        }
        Display::Table | Display::InlineTable => {
            IndependentContents::Table(crate::table::build_table(ctx, &base, state))
        }
        _ => IndependentContents::Flow(build_block_container(ctx, &base, state)),
    };
    IndependentBox {
        base,
        contents,
        marker,
    }
}

/// The replaced element `node` (an image, a video or audio), or `None` if
/// it is not one. `<canvas>` is not: without scripts, it shows its
/// fallback content (as in Chromium with JavaScript disabled).
fn replaced(ctx: &BuildContext<'_>, node: NodeId) -> Option<Replaced> {
    let media = if ctx.doc.element(node)?.is_html_named(&local_name!("img")) {
        None
    } else {
        Some(Media::of(ctx.doc, node)?)
    };
    let natural_size = ctx.replaced.natural_size(node);
    Some(Replaced {
        node,
        // A video without a loaded poster, and audio, have no natural
        // size.
        natural_size: if media.is_some() {
            Some(natural_size.unwrap_or_default())
        } else {
            natural_size
        },
        media,
    })
}

/// True for elements whose box is atomic like a replaced element: images,
/// media elements and form controls.
pub(crate) fn is_atomic(ctx: &BuildContext<'_>, node: NodeId) -> bool {
    is_replaced(ctx, node) || ctx.controls.is_control(node)
}

/// True for a replaced element (see [`replaced`]).
fn is_replaced(ctx: &BuildContext<'_>, node: NodeId) -> bool {
    ctx.doc
        .element(node)
        .is_some_and(swb_style::is_replaced_element)
}

/// The marker of list item `node` (an element, not a pseudo-element). Its
/// text comes from style (`StyleMap::list_marker_text`).
fn build_marker(
    ctx: &BuildContext<'_>,
    node: NodeId,
    style: &ComputedStyle,
    state: &mut BuildState,
) -> Option<Marker> {
    if style.display != Display::ListItem {
        return None;
    }
    let marker_style = ctx.styles.pseudo(node, PseudoKind::Marker)?;
    let text = state.generated(ctx.styles.list_marker_text(node, style)?);
    if text.is_empty() {
        return None;
    }
    Some(Marker {
        base: BoxBase {
            node: Some(node),
            pseudo: Some(PseudoKind::Marker),
            style: Arc::clone(marker_style),
            id: state.next_id(),
        },
        text,
        outside: style.list_style_position == swb_style::ListStylePosition::Outside,
    })
}

/// Builds the contents of a block container element (or pseudo-element).
pub(crate) fn build_block_container(
    ctx: &BuildContext<'_>,
    base: &BoxBase,
    state: &mut BuildState,
) -> BlockContainer {
    let mut builder = ContainerBuilder::new(Arc::clone(&base.style));
    if let Some(node) = base.element() {
        builder.push_children(ctx, node, state);
    } else if let Some(node) = base.node
        && let Some(text) = state.generated_content(&base.style)
    {
        builder.inline.push_generated(node, &base.style, &text);
    }
    builder.finish(state)
}

/// Builds the children of a flex or grid container: every in-flow child
/// becomes an item; contiguous inline content is wrapped in anonymous
/// blocks. Floats are items (`float` does not apply to them).
pub(crate) fn build_flex_items(
    ctx: &BuildContext<'_>,
    base: &BoxBase,
    state: &mut BuildState,
) -> Vec<IndependentBox> {
    let blocks = match build_block_container(ctx, base, state) {
        BlockContainer::Blocks(blocks) => blocks,
        BlockContainer::Inline(ifc) if ifc.is_empty() => Vec::new(),
        BlockContainer::Inline(ifc) => vec![BlockLevelBox::Block {
            base: state.anonymous(&base.style, Display::Block),
            contents: BlockContainer::Inline(ifc),
            marker: None,
        }],
    };
    blocks.into_iter().map(into_flex_item).collect()
}

/// A block-level box as a flex item: every flex item establishes an
/// independent formatting context.
fn into_flex_item(block: BlockLevelBox) -> IndependentBox {
    match block {
        BlockLevelBox::Block {
            base,
            contents,
            marker,
        } => IndependentBox {
            base,
            contents: IndependentContents::Flow(contents),
            marker,
        },
        BlockLevelBox::Independent(ib)
        | BlockLevelBox::Float(ib)
        | BlockLevelBox::AbsolutelyPositioned(ib) => ib,
        // Flex items are blockified, so they are never inside inline boxes.
        BlockLevelBox::InInline(b) => into_flex_item(b.block),
    }
}

/// Collects the children of one block container.
pub(crate) struct ContainerBuilder {
    style: Arc<ComputedStyle>,
    blocks: Vec<BlockLevelBox>,
    inline: InlineBuilder,
    /// Inline boxes that are open at the current position.
    open_inline_boxes: Vec<BoxBase>,
}

impl ContainerBuilder {
    pub(crate) fn new(style: Arc<ComputedStyle>) -> Self {
        ContainerBuilder {
            style,
            blocks: Vec::new(),
            inline: InlineBuilder::default(),
            open_inline_boxes: Vec::new(),
        }
    }

    fn push_children(&mut self, ctx: &BuildContext<'_>, parent: NodeId, state: &mut BuildState) {
        let parent_style = ctx.styles.get(parent).cloned();
        if parent_style.is_some() {
            self.push_pseudo(ctx, parent, PseudoKind::Before, state);
        }
        // Consecutive table-internal children share one anonymous table.
        let mut table: Option<AnonymousTable> = None;
        for child in ctx.doc.children(parent) {
            if let Some(pending) = &mut table {
                if pending.takes(ctx, child, parent_style.as_ref(), state) {
                    continue;
                }
                self.push_anonymous_table(table.take(), parent_style.as_ref(), ctx, state);
            }
            match &ctx.doc.node(child).data {
                NodeData::Text(text) => {
                    if let Some(style) = &parent_style {
                        self.inline.push_text(child, style, text);
                    }
                }
                NodeData::Element(_) if is_table_internal(ctx, child) => {
                    let mut pending = self.anonymous_table(state);
                    pending.builder.push_element(ctx, child, state);
                    table = Some(pending);
                }
                NodeData::Element(_) => self.push_element(ctx, child, state),
                _ => {}
            }
        }
        self.push_anonymous_table(table, parent_style.as_ref(), ctx, state);
        if parent_style.is_some() {
            self.push_pseudo(ctx, parent, PseudoKind::After, state);
        }
    }

    /// A builder for an anonymous table around misparented table-internal
    /// boxes (CSS 2.2 §17.2.1): an `inline-table` inside an inline box, a
    /// `table` otherwise.
    fn anonymous_table(&mut self, state: &mut BuildState) -> AnonymousTable {
        let (parent, display) = match self.open_inline_boxes.last() {
            Some(inline) => (&inline.style, Display::InlineTable),
            None => (&self.style, Display::Table),
        };
        let base = state.anonymous(parent, display);
        AnonymousTable {
            builder: crate::table::TableBuilder::new(Arc::clone(&base.style), 1),
            base,
            spaces: Vec::new(),
        }
    }

    /// Adds a pending anonymous table and the white space after it.
    fn push_anonymous_table(
        &mut self,
        table: Option<AnonymousTable>,
        parent_style: Option<&Arc<ComputedStyle>>,
        ctx: &BuildContext<'_>,
        state: &mut BuildState,
    ) {
        let Some(table) = table else {
            return;
        };
        let contents = table.builder.finish(state);
        self.push_table_box(table.base, contents, state);
        if let Some(style) = parent_style {
            for space in table.spaces {
                if let Some(text) = ctx.doc.node(space).as_text() {
                    self.inline.push_text(space, style, text);
                }
            }
        }
    }

    /// Adds an anonymous table box.
    fn push_table_box(
        &mut self,
        base: BoxBase,
        contents: crate::table::TableBox,
        state: &mut BuildState,
    ) {
        let inline = base.style.display == Display::InlineTable;
        let table = IndependentBox {
            base,
            contents: IndependentContents::Table(contents),
            marker: None,
        };
        if inline {
            self.inline.push(RawItem::Atomic(table));
        } else {
            self.push_block(BlockLevelBox::Independent(table), state);
        }
    }

    /// Adds the text of a text node whose parent has style `style`.
    pub(crate) fn push_text(&mut self, node: NodeId, style: &Arc<ComputedStyle>, text: &str) {
        self.inline.push_text(node, style, text);
    }

    /// Adds generated text (the `content` of a pseudo-element) of
    /// `element`.
    pub(crate) fn push_generated(
        &mut self,
        element: NodeId,
        style: &Arc<ComputedStyle>,
        text: &str,
    ) {
        self.inline.push_generated(element, style, text);
    }

    fn push_pseudo(
        &mut self,
        ctx: &BuildContext<'_>,
        node: NodeId,
        kind: PseudoKind,
        state: &mut BuildState,
    ) {
        let Some(style) = ctx.styles.pseudo(node, kind) else {
            return;
        };
        if state.depth >= MAX_BOX_DEPTH {
            return;
        }
        let base = BoxBase {
            node: Some(node),
            pseudo: Some(kind),
            style: Arc::clone(style),
            id: state.next_id(),
        };
        state.depth += 1;
        self.push_box(ctx, base, state);
        state.depth -= 1;
    }

    /// Adds the box of element `node` (or its children, or its text below
    /// the depth limit).
    pub(crate) fn push_element(
        &mut self,
        ctx: &BuildContext<'_>,
        node: NodeId,
        state: &mut BuildState,
    ) {
        let Some(style) = ctx.styles.get(node) else {
            return;
        };
        if style.display == Display::None {
            return;
        }
        if state.depth >= MAX_BOX_DEPTH {
            self.push_flattened(ctx, node, state);
            return;
        }
        state.depth += 1;
        if style.display == Display::Contents {
            self.push_children(ctx, node, state);
        } else if ctx
            .doc
            .element(node)
            .is_some_and(|e| e.is_html_named(&local_name!("br")))
        {
            let base = state.numbered(ctx.element_base(node, style));
            self.inline.push(RawItem::LineBreak(Some(base)));
        } else {
            let base = state.numbered(ctx.element_base(node, style));
            self.push_box(ctx, base, state);
        }
        state.depth -= 1;
    }

    /// Adds the text and line breaks of the subtree of `root` as inline
    /// content of this container, without boxes. Used below
    /// [`MAX_BOX_DEPTH`]; iterative, so that the depth of the subtree does
    /// not matter.
    pub(crate) fn push_flattened(
        &mut self,
        ctx: &BuildContext<'_>,
        root: NodeId,
        state: &mut BuildState,
    ) {
        if !state.flattened {
            state.flattened = true;
            log::warn!("boxes nested deeper than {MAX_BOX_DEPTH} levels; flattening the content");
        }
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match &ctx.doc.node(node).data {
                NodeData::Text(text) => {
                    if let Some(style) = ctx.doc.parent(node).and_then(|p| ctx.styles.get(p)) {
                        self.inline.push_text(node, style, text);
                    }
                }
                NodeData::Element(element) => {
                    if ctx
                        .styles
                        .get(node)
                        .is_none_or(|s| s.display == Display::None)
                    {
                        continue;
                    }
                    if element.is_html_named(&local_name!("br")) {
                        self.inline.push(RawItem::LineBreak(None));
                        continue;
                    }
                    let first = stack.len();
                    stack.extend(ctx.doc.children(node));
                    stack[first..].reverse();
                }
                _ => {}
            }
        }
    }

    /// Adds the box of an element or pseudo-element.
    pub(crate) fn push_box(
        &mut self,
        ctx: &BuildContext<'_>,
        base: BoxBase,
        state: &mut BuildState,
    ) {
        let style = Arc::clone(&base.style);
        let atomic = base.element().is_some_and(|n| is_atomic(ctx, n));
        if style.display.is_table_internal() && !atomic {
            // A misparented table part on its own (a pseudo-element; runs of
            // elements are collected in `push_children`).
            let mut table = self.anonymous_table(state);
            table.builder.push_pseudo(ctx, base, state);
            let contents = table.builder.finish(state);
            self.push_table_box(table.base, contents, state);
            return;
        }
        if style.is_absolutely_positioned() {
            let inner = build_independent(ctx, base, state);
            // Inside an inline box, the box stays inline: the inline box
            // can be its containing block, and the box is then in its
            // fragments (it needs no `in_positioned_inline` mark). After a
            // float it stays inline too: it keeps its place in tree order,
            // and its static position is next to the float.
            if self.inline.has_content()
                || self.inline.has_float()
                || !self.open_inline_boxes.is_empty()
            {
                self.inline.push(RawItem::AbsolutelyPositioned(inner));
            } else {
                self.blocks.push(BlockLevelBox::AbsolutelyPositioned(inner));
            }
            return;
        }
        if style.is_floating() {
            // A float belongs to the inline content around it; it becomes
            // block-level only if that content turns out empty (see
            // `flush_inline`). As in Chromium, a float before text is a
            // float of that text's inline formatting context.
            let inner = build_independent(ctx, base, state);
            self.inline.push(RawItem::Float(inner));
            return;
        }
        if style.display == Display::Inline && !atomic {
            self.inline.push(RawItem::StartBox {
                base: base.clone(),
                continued: false,
            });
            self.open_inline_boxes.push(base.clone());
            if base.pseudo.is_some() {
                if let Some(node) = base.node
                    && let Some(text) = state.generated_content(&style)
                {
                    self.inline.push_generated(node, &style, &text);
                }
            } else if let Some(node) = base.node {
                self.push_children(ctx, node, state);
            }
            self.open_inline_boxes.pop();
            self.inline.push(RawItem::EndBox { split: false });
            return;
        }
        if style.display.is_inline_level() {
            let inner = build_independent(ctx, base, state);
            self.inline.push(RawItem::Atomic(inner));
            return;
        }
        // A block-level box.
        let block = build_block_level(ctx, base, state);
        self.push_block(block, state);
    }

    /// True if any open inline box is positioned (all of them, not only the
    /// ones that get wrapper boxes; see `BlockInInline::in_positioned_inline`).
    fn in_positioned_inline(&self) -> bool {
        self.open_inline_boxes
            .iter()
            .any(|b| b.style.position != swb_style::Position::Static)
    }

    /// Adds a float at block level. Inside a positioned inline box
    /// (`in_positioned_inline`), it gets the mark of that box (it needs no
    /// inline box wrappers: out-of-flow boxes do not split inline boxes).
    fn push_float_block(&mut self, block: BlockLevelBox, in_positioned_inline: bool) {
        if in_positioned_inline {
            self.blocks
                .push(BlockLevelBox::InInline(Box::new(BlockInInline {
                    inline_boxes: Vec::new(),
                    block,
                    in_positioned_inline: true,
                })));
        } else {
            self.blocks.push(block);
        }
    }

    /// Adds a block-level box, splitting open inline boxes around it.
    fn push_block(&mut self, block: BlockLevelBox, state: &mut BuildState) {
        let open = self.open_inline_boxes.clone();
        for _ in &open {
            self.inline.push(RawItem::EndBox { split: true });
        }
        self.flush_inline(state);
        if open.is_empty() {
            self.blocks.push(block);
        } else {
            // The innermost boxes only, so that memory does not grow with
            // the nesting depth times the number of blocks.
            let skip = open.len().saturating_sub(MAX_INLINE_WRAPPERS);
            let in_positioned_inline = self.in_positioned_inline();
            self.blocks
                .push(BlockLevelBox::InInline(Box::new(BlockInInline {
                    inline_boxes: open[skip..].to_vec(),
                    block,
                    in_positioned_inline,
                })));
        }
        for base in open {
            self.inline.push(RawItem::StartBox {
                base,
                continued: true,
            });
        }
    }

    /// Wraps the pending inline content in an anonymous block, if it has
    /// any content or inline boxes (an empty inline box gets an empty line
    /// box, as in Chromium).
    fn flush_inline(&mut self, state: &mut BuildState) {
        let inline = std::mem::take(&mut self.inline);
        let ifc = inline.finish(state);
        if ifc.is_empty()
            && !ifc.has_whole_inline_box()
            && !ifc.has_absolute_in_inline_box()
            && !ifc.has_absolute_after_float()
        {
            // Out-of-flow boxes inside whitespace-only inline content still
            // need a place in the tree. A float inside a positioned inline
            // box (whose start is among the items, also when it continues
            // from an earlier part) gets that box's mark.
            let mut open: Vec<bool> = Vec::new();
            for item in ifc.items {
                match item {
                    InlineItem::StartBox { base, .. } => {
                        open.push(base.style.position != swb_style::Position::Static);
                    }
                    InlineItem::EndBox { .. } => {
                        open.pop();
                    }
                    InlineItem::Float(b) => {
                        let marked = open.contains(&true);
                        self.push_float_block(BlockLevelBox::Float(b), marked);
                    }
                    InlineItem::AbsolutelyPositioned(b) => {
                        self.blocks.push(BlockLevelBox::AbsolutelyPositioned(b));
                    }
                    _ => {}
                }
            }
        } else {
            self.blocks.push(BlockLevelBox::Block {
                base: state.anonymous(&self.style, Display::Block),
                contents: BlockContainer::Inline(ifc),
                marker: None,
            });
        }
    }

    /// Finishes the container.
    pub(crate) fn finish(mut self, state: &mut BuildState) -> BlockContainer {
        if self.blocks.is_empty() {
            return BlockContainer::Inline(std::mem::take(&mut self.inline).finish(state));
        }
        self.flush_inline(state);
        BlockContainer::Blocks(self.blocks)
    }
}

fn build_block_level(
    ctx: &BuildContext<'_>,
    base: BoxBase,
    state: &mut BuildState,
) -> BlockLevelBox {
    let style = &base.style;
    // A multi-column container establishes a new block formatting context
    // (CSS Multicol 1 §2); its content is laid out in one column (no
    // multi-column layout yet), which is what Chromium shows when only one
    // column fits.
    let multicol = style.column_width.is_some() || style.column_count.is_some();
    let establishes_bfc = !matches!(style.display, Display::Block | Display::ListItem)
        || style.overflow_x.is_scroll_container()
        || style.overflow_y.is_scroll_container()
        || multicol
        || base.element().is_some_and(|n| is_atomic(ctx, n));
    if establishes_bfc {
        return BlockLevelBox::Independent(build_independent(ctx, base, state));
    }
    let marker = base
        .element()
        .and_then(|n| build_marker(ctx, n, &base.style, state));
    let contents = build_block_container(ctx, &base, state);
    BlockLevelBox::Block {
        base,
        contents,
        marker,
    }
}

/// A pending anonymous table around misparented table parts, and the
/// collapsible white space after its last part (dropped if another table
/// part follows).
struct AnonymousTable {
    base: BoxBase,
    builder: crate::table::TableBuilder,
    spaces: Vec<NodeId>,
}

impl AnonymousTable {
    /// Takes `child` of the parent (with style `parent_style`) into the
    /// pending anonymous table if it continues the run: a table part, an
    /// element without a box (`display: none`, `<script>`), a comment, or
    /// collapsible white space (kept for after the table, dropped if
    /// another table part follows). Returns false if the run ends.
    fn takes(
        &mut self,
        ctx: &BuildContext<'_>,
        child: NodeId,
        parent_style: Option<&Arc<ComputedStyle>>,
        state: &mut BuildState,
    ) -> bool {
        match &ctx.doc.node(child).data {
            NodeData::Element(_) if is_table_internal(ctx, child) => {
                self.spaces.clear();
                self.builder.push_element(ctx, child, state);
                true
            }
            NodeData::Element(_) => ctx
                .styles
                .get(child)
                .is_none_or(|s| s.display == Display::None),
            NodeData::Text(text) => {
                let collapsible =
                    parent_style.is_some_and(|style| is_collapsible_white_space(style, text));
                if collapsible {
                    self.spaces.push(child);
                }
                collapsible
            }
            _ => true,
        }
    }
}

/// True if `node` is an element with an internal table display type.
/// Replaced elements and form controls are not table parts: they stay
/// atomic block-level boxes (inside a table, in an anonymous cell), as in
/// Chromium.
fn is_table_internal(ctx: &BuildContext<'_>, node: NodeId) -> bool {
    ctx.doc.element(node).is_some()
        && !is_atomic(ctx, node)
        && ctx
            .styles
            .get(node)
            .is_some_and(|s| s.display.is_table_internal())
}

/// An inline formatting context with the text of form control `node`:
/// the value of a text field (`selectable`: its caret stops are byte
/// offsets in `text`) or a label (generated content).
pub(crate) fn control_text(
    node: NodeId,
    style: &Arc<ComputedStyle>,
    text: &str,
    selectable: bool,
    state: &mut BuildState,
) -> InlineFormattingContext {
    let mut builder = InlineBuilder::default();
    if selectable {
        builder.push_text(node, style, text);
    } else {
        builder.push_generated(node, style, text);
    }
    builder.finish(state)
}

/// Inline content before white-space processing.
#[derive(Default)]
struct InlineBuilder {
    items: Vec<RawItem>,
    /// True once an item other than collapsible white space or an inline
    /// box marker was pushed (kept up to date so the check is O(1)).
    has_content: bool,
    /// True once a float was pushed.
    has_float: bool,
}

enum RawItem {
    StartBox {
        base: BoxBase,
        continued: bool,
    },
    EndBox {
        split: bool,
    },
    Text {
        node: NodeId,
        style: Arc<ComputedStyle>,
        text: String,
        generated: bool,
    },
    Atomic(IndependentBox),
    LineBreak(Option<BoxBase>),
    Float(IndependentBox),
    AbsolutelyPositioned(IndependentBox),
}

/// True for the characters that white-space processing treats as
/// collapsible white space (CSS Text 3 §4.1.1, plus form feed and carriage
/// return as in Chromium).
pub(crate) fn is_collapsible_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C')
}

/// True if `text` is only collapsible white space in `style`.
pub(crate) fn is_collapsible_white_space(style: &ComputedStyle, text: &str) -> bool {
    style.white_space.collapses_spaces() && text.chars().all(is_collapsible_space)
}

impl InlineBuilder {
    /// Adds the text of a text node.
    fn push_text(&mut self, node: NodeId, style: &Arc<ComputedStyle>, text: &str) {
        self.push_text_item(node, style, text, false);
    }

    /// Adds generated text (the `content` of a pseudo-element) of `element`.
    fn push_generated(&mut self, element: NodeId, style: &Arc<ComputedStyle>, text: &str) {
        self.push_text_item(element, style, text, true);
    }

    fn push_text_item(
        &mut self,
        node: NodeId,
        style: &Arc<ComputedStyle>,
        text: &str,
        generated: bool,
    ) {
        if text.is_empty() {
            return;
        }
        self.push(RawItem::Text {
            node,
            style: Arc::clone(style),
            text: text.to_owned(),
            generated,
        });
    }

    fn push(&mut self, item: RawItem) {
        let content = match &item {
            RawItem::Text { style, text, .. } => !is_collapsible_white_space(style, text),
            RawItem::StartBox { .. } | RawItem::EndBox { .. } | RawItem::Float(_) => false,
            _ => true,
        };
        self.has_content |= content;
        self.has_float |= matches!(item, RawItem::Float(_));
        self.items.push(item);
    }

    /// True if there is anything other than collapsible white space.
    fn has_content(&self) -> bool {
        self.has_content
    }

    /// True if a float was pushed.
    fn has_float(&self) -> bool {
        self.has_float
    }

    /// Performs white-space processing and produces the final context.
    fn finish(self, state: &mut BuildState) -> InlineFormattingContext {
        let mut ifc = InlineFormattingContext {
            id: state.next_id(),
            ..InlineFormattingContext::default()
        };
        // True if the last character emitted was a collapsible space (or we
        // are at the start), so that the next collapsible space is removed.
        let mut after_space = true;
        for item in self.items {
            match item {
                RawItem::StartBox { base, continued } => {
                    ifc.items.push(InlineItem::StartBox { base, continued });
                }
                RawItem::EndBox { split } => ifc.items.push(InlineItem::EndBox { split }),
                RawItem::Text {
                    node,
                    style,
                    text,
                    generated,
                } => {
                    let source = TextSource {
                        node,
                        style: &style,
                        generated,
                    };
                    process_text(&mut ifc, source, &text, &mut after_space);
                }
                RawItem::Atomic(inner) => {
                    let offset = ifc.text.len();
                    ifc.text.push('\u{FFFC}');
                    ifc.items.push(InlineItem::Atomic { inner, offset });
                    after_space = false;
                }
                RawItem::LineBreak(base) => {
                    ifc.items.push(InlineItem::LineBreak(base));
                    after_space = true;
                }
                RawItem::Float(b) => ifc.items.push(InlineItem::Float(b)),
                RawItem::AbsolutelyPositioned(b) => {
                    ifc.items.push(InlineItem::AbsolutelyPositioned(b));
                }
            }
        }
        ifc
    }
}

/// White-space processing for one text item (CSS Text 3 §4.1.1,
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>), followed by
/// `text-transform`. Appends text items (and line breaks for preserved
/// newlines) to `ifc`. Generated text (`content` of pseudo-elements) has
/// no source map: it cannot be selected.
fn process_text(
    ifc: &mut InlineFormattingContext,
    source: TextSource<'_>,
    text: &str,
    after_space: &mut bool,
) {
    let ws = source.style.white_space;
    let mut segment = Segment::new(ifc);
    if ws.collapses_spaces() {
        let mut chars = text.char_indices().peekable();
        while let Some((start, c)) = chars.next() {
            if !is_collapsible_space(c) {
                segment.push(ifc, c, start, c.len_utf8());
                *after_space = false;
                continue;
            }
            // A run of collapsible white space. Spaces and tabs around
            // segment breaks are removed; CR LF is one segment break.
            let mut breaks = usize::from(matches!(c, '\n' | '\r'));
            let mut previous = c;
            while let Some(&(_, next)) = chars.peek().filter(|(_, n)| is_collapsible_space(*n)) {
                breaks += usize::from(next == '\r' || (next == '\n' && previous != '\r'));
                previous = next;
                chars.next();
            }
            if ws == WhiteSpace::PreLine && breaks > 0 {
                for _ in 0..breaks {
                    segment = segment.finish(ifc, &source);
                    ifc.items.push(InlineItem::LineBreak(None));
                }
                *after_space = true;
            } else if !*after_space {
                segment.push(ifc, ' ', start, 1);
                *after_space = true;
            }
        }
    } else {
        for (start, c) in text.char_indices() {
            match c {
                '\n' => {
                    segment = segment.finish(ifc, &source);
                    ifc.items.push(InlineItem::LineBreak(None));
                }
                '\r' => {}
                c => segment.push(ifc, c, start, c.len_utf8()),
            }
        }
        *after_space = false;
    }
    segment.finish(ifc, &source);
}

/// The node and style of a text item, and whether it is generated
/// content.
#[derive(Clone, Copy)]
struct TextSource<'a> {
    node: NodeId,
    style: &'a Arc<ComputedStyle>,
    generated: bool,
}

/// The text of a text item being built (up to a preserved newline or the
/// end of the text), with the origin of each character.
struct Segment {
    /// The start of the segment in the context's text.
    start: usize,
    chars: Vec<CharSource>,
}

impl Segment {
    fn new(ifc: &InlineFormattingContext) -> Self {
        Segment {
            start: ifc.text.len(),
            chars: Vec::new(),
        }
    }

    /// Appends `c`, which comes from `node_len` bytes at `node` in the
    /// node's data.
    fn push(&mut self, ifc: &mut InlineFormattingContext, c: char, node: usize, node_len: usize) {
        let to_u32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        self.chars.push(CharSource {
            item: to_u32(ifc.text.len() - self.start),
            item_len: to_u32(c.len_utf8()),
            node: to_u32(node),
            node_len: to_u32(node_len),
            exact: true,
        });
        ifc.text.push(c);
    }

    /// Applies `text-transform`, adds the text item, and returns an empty
    /// segment that starts at the end of the text.
    fn finish(mut self, ifc: &mut InlineFormattingContext, source: &TextSource<'_>) -> Segment {
        let start = self.start;
        if ifc.text.len() > start {
            let style = source.style;
            if style.text_transform != TextTransform::None {
                let previous = previous_char(ifc, start);
                let (transformed, lengths) =
                    apply_text_transform(&ifc.text[start..], style.text_transform, previous);
                self.remap(&lengths);
                ifc.text.truncate(start);
                ifc.text.push_str(&transformed);
            }
            ifc.items.push(InlineItem::Text {
                node: source.node,
                style: Arc::clone(style),
                range: start..ifc.text.len(),
                source: (!source.generated).then(|| SourceMap::new(&self.chars)),
            });
        }
        Segment::new(ifc)
    }

    /// Updates the item offsets of the characters after `text-transform`.
    /// `lengths` has, for each character, its new length and whether it
    /// stayed one character of the same length.
    fn remap(&mut self, lengths: &[(usize, bool)]) {
        let mut item = 0;
        for (c, &(len, exact)) in self.chars.iter_mut().zip(lengths) {
            c.item = u32::try_from(item).unwrap_or(u32::MAX);
            c.item_len = u32::try_from(len).unwrap_or(u32::MAX);
            c.exact &= exact;
            item += len;
        }
    }
}

/// The character before text offset `start` (where a new text item
/// begins), for word boundaries across elements; `None` after a forced line
/// break, which always ends a word.
fn previous_char(ifc: &InlineFormattingContext, start: usize) -> Option<char> {
    for item in ifc.items.iter().rev() {
        match item {
            InlineItem::LineBreak(_) => return None,
            InlineItem::Text { .. } | InlineItem::Atomic { .. } => break,
            _ => {}
        }
    }
    ifc.text[..start].chars().next_back()
}

/// Applies `text-transform` to `text` (CSS Text 3 §2.1,
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>). Returns
/// the new text and, for each character of `text`, its length in the new
/// text and whether it stayed one character of the same length.
/// `previous` is the character before the text in the inline formatting
/// context: with `capitalize`, a word that starts at the beginning of
/// `text` and directly follows a letter or digit continues a word of a
/// preceding element and is not changed. Words are found with the word
/// boundaries of UAX #29.
fn apply_text_transform(
    text: &str,
    transform: TextTransform,
    previous: Option<char>,
) -> (String, Vec<(usize, bool)>) {
    let capitalized = match transform {
        TextTransform::Capitalize => capitalized_offsets(text, previous),
        _ => Vec::new(),
    };
    let mut out = String::with_capacity(text.len());
    let mut lengths = Vec::with_capacity(text.len());
    for (offset, c) in text.char_indices() {
        let before = out.len();
        match transform {
            TextTransform::Uppercase => out.extend(c.to_uppercase()),
            // Lowercase per character, except the final sigma below.
            TextTransform::Lowercase => out.extend(c.to_lowercase()),
            TextTransform::Capitalize if capitalized.binary_search(&offset).is_ok() => {
                out.extend(c.to_uppercase());
            }
            _ => out.push(c),
        }
        let new = &out[before..];
        let mut chars = new.chars();
        let single = chars.next().is_some() && chars.next().is_none();
        lengths.push((new.len(), single && new.len() == c.len_utf8()));
    }
    if transform == TextTransform::Lowercase && text.contains('Σ') {
        // `str::to_lowercase` maps a final 'Σ' to 'ς' (Unicode
        // Final_Sigma). Both have the same length as 'σ', so the lengths
        // stay valid.
        out = text.to_lowercase();
    }
    (out, lengths)
}

/// The byte offsets of the letters that `capitalize` puts in uppercase.
fn capitalized_offsets(text: &str, previous: Option<char>) -> Vec<usize> {
    let continues_word = previous.is_some_and(char::is_alphanumeric);
    text.split_word_bound_indices()
        .filter(|&(offset, segment)| {
            segment.chars().next().is_some_and(char::is_alphabetic)
                && !(offset == 0 && continues_word)
        })
        .map(|(offset, _)| offset)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text items of an inline formatting context built from `text`
    /// in an element with `style`: the item text and the node offset of
    /// each item offset (the end included).
    fn processed(style: &str, text: &str) -> Vec<(String, Vec<u32>)> {
        let doc = swb_dom::parse_html(&format!("<p style='{style}'>{text}</p>"));
        let styles = crate::test_support::styles_for(&doc);
        let p = doc
            .find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("p")))
            .expect("a p element");
        let mut builder = InlineBuilder::default();
        for child in doc.children(p) {
            if let Some(data) = doc.node(child).as_text() {
                builder.push_text(child, styles.get(p).expect("p has a style"), data);
            }
        }
        let mut state = BuildState::default();
        let ifc = builder.finish(&mut state);
        ifc.items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Text { range, source, .. } => {
                    let map = source.as_ref().expect("text nodes have a source map");
                    let offsets = (0..=range.len()).map(|i| map.node_offset(i)).collect();
                    Some((ifc.text[range.clone()].to_owned(), offsets))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn generated_text_is_cut_at_the_limit() {
        let mut state = BuildState::default();
        assert_eq!(state.generated("ab".to_owned()), "ab");
        // Fill the budget up to 1 byte before a 2-byte character.
        let filler = "x".repeat(MAX_GENERATED_TEXT - 3);
        assert_eq!(state.generated(filler.clone()).len(), filler.len());
        assert_eq!(state.generated("éé".to_owned()), "");
        assert_eq!(state.generated("y".to_owned()), "y");
        assert_eq!(state.generated("z".to_owned()), "");
        assert_eq!(state.generated_text, MAX_GENERATED_TEXT);
        assert!(state.generated_cut);
    }

    #[test]
    fn generated_text_cut_inside_a_character_keeps_the_whole_characters() {
        let mut state = BuildState::default();
        state.generated("x".repeat(MAX_GENERATED_TEXT - 3));
        assert_eq!(state.generated("éé".to_owned()), "é");
        assert_eq!(state.generated_text, MAX_GENERATED_TEXT - 1);
    }

    #[test]
    fn white_space_collapsing_keeps_source_offsets() {
        assert_eq!(
            processed("", "  a \n\t b  "),
            vec![("a b ".to_owned(), vec![2, 3, 7, 8, 9])]
        );
    }

    #[test]
    fn pre_line_removes_spaces_around_newlines() {
        // The parser turns CR LF into LF.
        let items = processed("white-space:pre-line", "a  \r\n  b \n\nc");
        let texts: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
        assert_eq!(items[1].1, vec![6, 7]);
        assert_eq!(items[2].1, vec![10, 11]);
    }

    #[test]
    fn carriage_returns_are_segment_breaks() {
        // Character references keep CR in the DOM: CR LF is one break, a
        // lone CR another.
        let items = processed("white-space:pre-line", "a&#13;&#10;b&#13;c");
        let texts: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
        assert_eq!(items[2].1, vec![5, 6]);
        let items = processed("", "a&#13;&#10;b&#13;c");
        assert_eq!(items, vec![("a b c".to_owned(), vec![0, 1, 3, 4, 5, 6])]);
        let items = processed("white-space:pre", "a&#13;&#10;b");
        assert_eq!(items[0].0, "a");
        assert_eq!(items[1], ("b".to_owned(), vec![3, 4]));
    }

    #[test]
    fn lowercase_keeps_the_final_sigma() {
        let items = processed("text-transform:lowercase", "ΟΔΟΣ ΑΣ");
        assert_eq!(items[0].0, "οδος ας");
        assert_eq!(items[0].1.len(), "οδος ας".len() + 1);
    }

    #[test]
    fn preserved_text_keeps_offsets() {
        let items = processed("white-space:pre", "a\n b");
        assert_eq!(items[0], ("a".to_owned(), vec![0, 1]));
        assert_eq!(items[1], (" b".to_owned(), vec![2, 3, 4]));
    }

    #[test]
    fn text_transform_keeps_character_boundaries() {
        // 'ß' becomes "SS"; offsets inside it map to its end.
        let items = processed("text-transform:uppercase", "aßc");
        assert_eq!(items[0], ("ASSC".to_owned(), vec![0, 1, 3, 3, 4]));
        let items = processed("text-transform:capitalize", "ǰx yz");
        assert_eq!(items[0].0, "J\u{30C}x Yz");
        assert_eq!(items[0].1, vec![0, 2, 2, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn capitalize_words() {
        let capitalize = |text: &str, previous: Option<char>| {
            apply_text_transform(text, TextTransform::Capitalize, previous).0
        };
        assert_eq!(capitalize("hello big-world", None), "Hello Big-World");
        assert_eq!(capitalize("(hello) \"quoted\"", None), "(Hello) \"Quoted\"");
        assert_eq!(capitalize("don't 3rd", None), "Don't 3rd");
        // A word that continues across an element boundary.
        assert_eq!(capitalize("bar baz", Some('o')), "bar Baz");
        assert_eq!(capitalize("bar", Some(' ')), "Bar");
    }

    fn layout_text(html: &str) -> String {
        let l = crate::test_support::layout_html(html);
        l.texts().into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn text_transform_applies_to_every_preserved_line() {
        let text = layout_text(
            "<pre style='text-transform:uppercase'>abc\ndef</pre>\
             <div style='white-space:pre-line; text-transform:uppercase'>ghi\njkl</div>",
        );
        assert_eq!(text, "ABCDEFGHIJKL");
    }

    #[test]
    fn capitalize_continues_words_across_elements() {
        let text = layout_text(
            "<p style='text-transform:capitalize'>foo<b>bar</b> (hello) <i>x</i>y<br>z</p>",
        );
        assert_eq!(text, "Foobar (Hello) XyZ");
    }
}
