//! Box tree construction, layout algorithms and the fragment tree.
//!
//! [`layout`] takes a document with computed styles and produces a
//! [`FragmentTree`]: positioned boxes and glyph runs in CSS pixels.
//!
//! Supported: block layout with margin collapsing, inline layout with line
//! breaking and vertical alignment, list markers, replaced elements
//! (images, video and audio with their controls, see `media.rs`), form
//! controls, flex layout, grid layout, table layout, floats and clearance
//! (`floats.rs`), relative, absolute, fixed and sticky positioning
//! (`positioned.rs`), transforms (paint applies them), the scrollable
//! overflow of scroll containers (see `scroll.rs`).
//!
//! All lengths and coordinates stay within ±[`swb_style::Length::MAX_PX`],
//! and box nesting is limited (see `box_tree.rs`), so that hostile content
//! cannot produce infinite values or unbounded recursion.
//!
//! Modules: `align` (self-alignment helpers), `block` (block layout, sizes
//! and margins), `box_tree` (the box tree), `collapsed_margin` (margin
//! collapsing), `control` (form controls), `flex`, `floats`, `fonts`,
//! `fragment` (the fragment tree), `geom` (points, sizes, rectangles),
//! `grid`, `inline` (line layout), `intrinsic` (min- and max-content
//! sizes), `list_marker`, `media` (video and audio controls),
//! `positioned` (out-of-flow, sticky and transforms), `replaced`, `scroll`
//! (scrollable overflow), `source_map` (text offsets) and `table`.

mod align;
mod block;
mod box_tree;
mod collapsed_margin;
mod control;
mod flex;
mod floats;
mod fonts;
mod fragment;
mod geom;
mod grid;
mod inline;
mod intrinsic;
mod list_marker;
mod media;
mod positioned;
mod replaced;
mod scroll;
mod source_map;
mod table;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use swb_dom::{Document, NodeId, local_name};
use swb_style::{ComputedStyle, Overflow, StyleMap};
use swb_text::FontContext;

pub use control::{Control, ControlKind, FormControls, MAX_SELECT_OPTIONS, NoFormControls};
pub use fragment::{
    BoxContent, BoxFragment, CanvasBackground, Caret, CellPaint, CollapsedEdge, ControlContent,
    Fragment, FragmentRef, FragmentTree, MediaContent, MediaPart, MediaPartKind, MediaText,
    PartBackground, PositionedGlyph, TablePaint, TextFragment,
};
pub use geom::{Edges, Matrix, Point, Rect, Size};
pub use positioned::{
    Ancestry, GroupTransform, Placeholder, StickyCache, StickyConstraints, clip_property_area,
    forms_stacking_context, has_transform, is_absolute_containing_block, is_fixed_containing_block,
    transform_matrix,
};
pub use replaced::{DEFAULT_OBJECT_SIZE, NaturalSize};
pub use scroll::{NoScroll, ScrollOffsets, ScrollState, clamp_scroll_offset, scroll_range};

use block::ContainingBlock;
use box_tree::{BuildContext, InlineFormattingContext};

/// Natural sizes of replaced elements (images, video posters), supplied by
/// the engine.
pub trait ReplacedSizes {
    /// The natural dimensions of the image of replaced element `node` (the
    /// source of an `<img>`, the poster of a `<video>`), or `None` if it is
    /// not loaded or broken.
    fn natural_size(&self, node: NodeId) -> Option<NaturalSize>;
}

/// A `ReplacedSizes` that knows no sizes.
pub struct NoReplacedSizes;

impl ReplacedSizes for NoReplacedSizes {
    fn natural_size(&self, _node: NodeId) -> Option<NaturalSize> {
        None
    }
}

/// Inputs for layout.
pub struct LayoutInput<'a> {
    /// The document.
    pub document: &'a Document,
    /// Computed styles of the document's elements.
    pub styles: &'a StyleMap,
    /// The viewport (initial containing block) size in CSS px.
    pub viewport: Size,
    /// Natural sizes of images and video posters.
    pub replaced: &'a dyn ReplacedSizes,
    /// The states of form controls.
    pub controls: &'a dyn FormControls,
}

/// State shared by all layout functions during one layout pass.
pub(crate) struct LayoutContext<'a> {
    pub(crate) fonts: &'a mut FontContext,
    /// True if the document is in quirks mode.
    pub(crate) quirks: bool,
    /// True in quirks mode and limited-quirks mode: the line height quirks
    /// apply (see `inline/mod.rs`).
    pub(crate) line_height_quirks: bool,
    /// Shaped text per inline formatting context, keyed by its number.
    shaped: HashMap<usize, Rc<inline::ShapedText>>,
    /// Laid-out boxes per box and constraints: flex items (see
    /// [`block::layout_flex_item`]), grid items and table cells.
    pub(crate) layouts: LayoutCache,
    /// The number of flex item and table cell layouts that were not in the
    /// cache.
    pub(crate) uncached_layouts: usize,
    /// The number of placeholders of absolutely positioned boxes created
    /// (see `positioned.rs`).
    pub(crate) placeholders: usize,
    /// The number of block boxes, independent boxes and line boxes laid
    /// out: a measure of layout work (see `block::fit_independent`).
    pub(crate) layout_units: u64,
    /// Data of the tables of this layout pass.
    pub(crate) tables: table::TableCache,
    /// Data of the grid containers of this layout pass.
    pub(crate) grids: grid::GridCache,
    /// The block formatting contexts being laid out (the first
    /// `bfc_depth`), innermost last; the others are kept for reuse.
    bfcs: Vec<floats::Bfc>,
    bfc_depth: usize,
    /// The work budget of float layout (see [`floats::WORK_BUDGET`]) that
    /// no BFC on the stack holds.
    float_budget: u64,
    /// True once the float work budget of this layout pass ran out.
    pub(crate) float_budget_spent: bool,
}

/// True once a spent float work budget was logged: the warning appears
/// once per process, not once per layout pass.
static FLOAT_BUDGET_WARNED: AtomicBool = AtomicBool::new(false);

impl<'a> LayoutContext<'a> {
    pub(crate) fn new(fonts: &'a mut FontContext) -> Self {
        LayoutContext {
            fonts,
            quirks: false,
            line_height_quirks: false,
            shaped: HashMap::new(),
            layouts: LayoutCache::default(),
            uncached_layouts: 0,
            placeholders: 0,
            layout_units: 0,
            tables: table::TableCache::default(),
            grids: grid::GridCache::default(),
            bfcs: Vec::new(),
            bfc_depth: 0,
            float_budget: floats::WORK_BUDGET,
            float_budget_spent: false,
        }
    }

    /// The innermost block formatting context.
    pub(crate) fn bfc(&mut self) -> &mut floats::Bfc {
        if self.bfc_depth == 0 {
            self.push_bfc();
        }
        &mut self.bfcs[self.bfc_depth - 1]
    }

    /// Starts a new block formatting context; it takes over the work
    /// budget.
    pub(crate) fn push_bfc(&mut self) {
        let budget = match self.bfc_depth.checked_sub(1) {
            Some(outer) => std::mem::take(&mut self.bfcs[outer].budget),
            None => std::mem::take(&mut self.float_budget),
        };
        match self.bfcs.get_mut(self.bfc_depth) {
            Some(spare) => spare.reset(budget),
            None => self.bfcs.push(floats::Bfc::new(budget)),
        }
        self.bfc_depth += 1;
    }

    /// Ends the innermost block formatting context; returns the bottom of
    /// its lowest float. The remaining work budget goes back to the outer
    /// context.
    pub(crate) fn pop_bfc(&mut self) -> Option<f32> {
        let depth = self.bfc_depth.checked_sub(1)?;
        self.bfc_depth = depth;
        let bfc = &mut self.bfcs[depth];
        let budget = std::mem::take(&mut bfc.budget);
        let bottom = bfc.exclusions.bottom();
        if budget == 0 && !self.float_budget_spent {
            self.float_budget_spent = true;
            if FLOAT_BUDGET_WARNED.swap(true, Ordering::Relaxed) {
                log::debug!("float layout work budget spent");
            } else {
                log::warn!(
                    "float layout work budget spent; later floats and boxes go below all floats"
                );
            }
        }
        match depth.checked_sub(1) {
            Some(outer) => self.bfcs[outer].budget = budget,
            None => self.float_budget = budget,
        }
        bottom
    }

    /// The shaped text of an inline formatting context (cached).
    pub(crate) fn shaped(&mut self, ifc: &InlineFormattingContext) -> Rc<inline::ShapedText> {
        if let Some(s) = self.shaped.get(&ifc.id) {
            return Rc::clone(s);
        }
        let shaped = Rc::new(inline::shape_ifc(self.fonts, ifc));
        self.shaped.insert(ifc.id, Rc::clone(&shaped));
        shaped
    }
}

/// The layout cache of flex items and table cells, which their containers
/// lay out several times. Entries are cheap: a fragment's children are
/// shared (`Arc`), so a nested subtree is stored once, not once per level,
/// and a cache hit copies only the top fragment.
#[derive(Default)]
pub(crate) struct LayoutCache {
    entries: HashMap<LayoutKey, BoxFragment>,
}

impl LayoutCache {
    pub(crate) fn get(&self, key: &LayoutKey) -> Option<&BoxFragment> {
        self.entries.get(key)
    }

    pub(crate) fn insert(&mut self, key: LayoutKey, fragment: BoxFragment) {
        self.entries.insert(key, fragment);
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

/// A box (by its number) and the constraints it is laid out with: the key
/// of the layout cache. Sizes are compared by their bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LayoutKey {
    box_id: usize,
    content_width: u32,
    content_height: Option<u32>,
    cb_width: u32,
    cb_height: Option<u32>,
}

impl LayoutKey {
    pub(crate) fn new(
        box_id: usize,
        content_width: f32,
        content_height: Option<f32>,
        cb: ContainingBlock,
    ) -> Self {
        LayoutKey {
            box_id,
            content_width: content_width.to_bits(),
            content_height: content_height.map(f32::to_bits),
            cb_width: cb.width.to_bits(),
            cb_height: cb.height.map(f32::to_bits),
        }
    }
}

/// Lays out a document.
pub fn layout(input: &LayoutInput<'_>, fonts: &mut FontContext) -> FragmentTree {
    layout_with(input, &mut LayoutContext::new(fonts))
}

/// Lays out a document with an existing layout context.
pub(crate) fn layout_with(input: &LayoutInput<'_>, ctx: &mut LayoutContext<'_>) -> FragmentTree {
    ctx.quirks = input.document.quirks_mode == swb_dom::QuirksMode::Quirks;
    ctx.line_height_quirks = input.document.quirks_mode != swb_dom::QuirksMode::NoQuirks;
    let viewport_overflow = viewport_overflow(input.document, input.styles);
    let build = BuildContext {
        doc: input.document,
        styles: input.styles,
        replaced: input.replaced,
        controls: input.controls,
        overflow_source: viewport_overflow.map(|(node, _, _)| node),
    };
    let root_box = box_tree::build_root(&build);
    let icb = ContainingBlock {
        width: input.viewport.width,
        height: Some(input.viewport.height),
    };
    let root = root_box.map(|root_box| {
        let mut root = block::layout_root(ctx, &root_box, icb);
        positioned::place_out_of_flow(ctx, &root_box, &mut root, input.viewport);
        root
    });
    let scroll_size = scroll::viewport_scroll_size(root.as_ref(), input.viewport);
    FragmentTree {
        root,
        canvas_background: canvas_background(input.document, input.styles),
        scroll_size,
        viewport_overflow: viewport_overflow
            .map_or((Overflow::Visible, Overflow::Visible), |(_, x, y)| (x, y)),
        viewport: input.viewport,
    }
}

/// The element whose `overflow` applies to the viewport, and that value
/// (CSS Overflow 3 §3.3,
/// <https://www.w3.org/TR/css-overflow-3/#overflow-propagation>): the root
/// element's, or, if the root is an HTML `html` element with `overflow:
/// visible`, the `body` element's. `None` if neither has a non-visible
/// overflow.
fn viewport_overflow(doc: &Document, styles: &StyleMap) -> Option<(NodeId, Overflow, Overflow)> {
    let visible =
        |s: &ComputedStyle| s.overflow_x == Overflow::Visible && s.overflow_y == Overflow::Visible;
    let root = doc.document_element()?;
    let root_style = styles.get(root)?;
    let (source, style) = if !visible(root_style) {
        (root, root_style)
    } else if doc.is_html_element(root, &local_name!("html"))
        && let Some(body) = doc.body()
        && doc.parent(body) == Some(root)
        && let Some(body_style) = styles.get(body)
        && !visible(body_style)
    {
        (body, body_style)
    } else {
        return None;
    };
    Some((source, style.overflow_x, style.overflow_y))
}

/// The background that paints the canvas: the root element's, or the
/// body's if the root has no background (CSS 2.2 §14.2,
/// <https://www.w3.org/TR/CSS22/colors.html#background>).
fn canvas_background(doc: &Document, styles: &StyleMap) -> Option<CanvasBackground> {
    let root = doc.document_element()?;
    let root_style = styles.get(root)?;
    if has_background(root_style) {
        return Some(CanvasBackground {
            source: root,
            style: Arc::clone(root_style),
        });
    }
    if doc.is_html_element(root, &local_name!("html"))
        && let Some(body) = doc.body()
        && let Some(body_style) = styles.get(body)
        && has_background(body_style)
    {
        return Some(CanvasBackground {
            source: body,
            style: Arc::clone(body_style),
        });
    }
    None
}

/// True if a style has a background color or image.
pub(crate) fn has_background(style: &ComputedStyle) -> bool {
    !style.background_color.resolve(style.color).is_transparent()
        || style.background_image.iter().any(Option::is_some)
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::layout_html;

    #[test]
    fn body_overflow_applies_to_the_viewport() {
        let l = layout_html(
            "<!DOCTYPE html><style>html, body { height: 100% } body { overflow-x: hidden }\
             </style><body style='margin:0'><div style='height:2000px; width:3000px'></div>",
        );
        // The hidden axis keeps its extent: scripts can scroll it.
        assert_eq!(l.tree.scroll_size, Size::new(3000.0, 2000.0));
        assert_eq!(l.tree.viewport_overflow, (Overflow::Hidden, Overflow::Auto));
        let body = l.tree.border_boxes(l.doc.body().expect("body"));
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn transformed_boxes_count_with_their_transformed_bounds() {
        // Scaled 4 times about its center (750, 50): 550 to 950.
        let l = layout_html(
            "<!DOCTYPE html><body style='margin:0'>\
             <div style='margin-left:700px; width:100px; height:100px; transform:scale(4)'></div>",
        );
        assert_eq!(l.tree.scroll_size.width, 950.0);
    }

    #[test]
    fn clipped_boxes_in_a_block_in_a_positioned_inline_box_do_not_count() {
        // The inline box is the containing block, inside the clipping box.
        let l = layout_html(
            "<!DOCTYPE html><body style='margin:0'><div style='overflow:hidden; height:100px'>\
             <span style='position:relative'><div><div style='position:absolute; top:3000px;\
             width:10px; height:10px'></div></div></span></div>",
        );
        assert_eq!(l.tree.scroll_size.height, 600.0);
    }

    #[test]
    fn root_overflow_hidden_keeps_the_scroll_size() {
        let l = layout_html(
            "<!DOCTYPE html><style>html { overflow: hidden }</style>\
             <body style='margin:0'><div style='height:2000px'></div>",
        );
        assert_eq!(l.tree.scroll_size, Size::new(800.0, 2000.0));
        assert_eq!(
            l.tree.viewport_overflow,
            (Overflow::Hidden, Overflow::Hidden)
        );
    }

    #[test]
    fn hanging_spaces_do_not_extend_the_scroll_size() {
        // Chromium 148: 800 px for `pre-wrap`; `pre` keeps its spaces.
        let spaces = " ".repeat(200);
        let html = |white_space: &str| {
            format!(
                "<!DOCTYPE html><body style='margin:0'><div style='width:100px;\
                 white-space:{white_space};font:16px monospace'>bbbb{spaces}</div>"
            )
        };
        assert_eq!(layout_html(&html("pre-wrap")).tree.scroll_size.width, 800.0);
        assert!(layout_html(&html("pre")).tree.scroll_size.width > 1900.0);
        // Also when an inline box covers the spaces.
        let in_span = format!(
            "<!DOCTYPE html><body style='margin:0;white-space:pre-wrap;font:16px monospace'>\
             aa <span>bb{spaces}</span>"
        );
        assert_eq!(layout_html(&in_span).tree.scroll_size.width, 800.0);
        // A line break after them is a box without width at their end; it
        // does not count either (Chromium 148: the viewport width).
        let with_br = format!(
            "<!DOCTYPE html><body style='margin:0;white-space:pre-wrap;font:16px monospace'>\
             aa{spaces}<br>b"
        );
        assert_eq!(layout_html(&with_br).tree.scroll_size.width, 800.0);
        // Zero-size inline content counts with its position (Chromium
        // counts its line box: 1500).
        let empty_span = "<!DOCTYPE html><body style='margin:0'>\
                          <span style='margin-left:1500px'></span>";
        assert_eq!(layout_html(empty_span).tree.scroll_size.width, 1500.0);
    }

    #[test]
    fn propagated_overflow_does_not_make_body_a_formatting_context() {
        // With `overflow: visible` used, the body's margin collapses with
        // the paragraph's margin.
        let l = layout_html(
            "<!DOCTYPE html><style>body { overflow: hidden; margin: 8px } \
             p { margin-top: 20px }</style><body><p id=p>text",
        );
        let body = l.tree.border_boxes(l.doc.body().expect("body"))[0];
        assert_eq!(body.y, 20.0);
        assert_eq!(l.rect("p").y, 20.0);
    }

    #[test]
    fn huge_lengths_give_finite_geometry() {
        let l = layout_html(
            "<!DOCTYPE html><body>\
             <div style='width:1e39px; padding:0 1e39px'>a b c</div>\
             <div style='margin:0 -1e39px'>a b c</div>\
             <div style='letter-spacing:-1e39px; word-spacing:1e39px'>a b c d e</div>\
             <div style='line-height:1e39'>x</div><div style='line-height:1e39px'>x</div>\
             <span style='display:inline-block; vertical-align:1e39px'>v</span>\
             <div style='display:flex'><div style='flex-grow:1e39'>a</div>\
             <div style='flex-grow:1e39'>b</div></div>\
             <div style='display:flex; width:100px'><div style='flex-shrink:1e39; width:1e39px'>a\
             </div><div style='flex-shrink:1e39; width:1e39px'>b</div></div>\
             <table style='border-spacing:1e39px'><tr><td style='width:1e39px'>a</td>\
             <td style='width:1e39px; height:1e39px'>b</td></tr>\
             <tr style='height:1e39px'><td colspan=2 style='padding:1e39px'>c</td></tr></table>\
             <table style='width:1e39px; height:1e39px; border-collapse:collapse'>\
             <tr><td style='border:1e39px solid'>d</td></tr></table>",
        );
        assert!(l.tree.scroll_size.width.is_finite() && l.tree.scroll_size.height.is_finite());
        let mut all_finite = true;
        l.tree.walk(|f, origin| {
            let r = match f {
                FragmentRef::Box(b) => b.border_rect,
                FragmentRef::Text(t) => t.rect,
            };
            all_finite &= [origin.x, origin.y, r.x, r.y, r.width, r.height]
                .iter()
                .all(|v| v.is_finite());
        });
        assert!(all_finite);
        assert!(l.root().border_rect.height <= swb_style::Length::MAX_PX * 16.0);
    }
}
