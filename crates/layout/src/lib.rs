//! Box tree construction, layout algorithms and the fragment tree.
//!
//! [`layout`] takes a document with computed styles and produces a
//! [`FragmentTree`]: positioned boxes and glyph runs in CSS pixels.
//!
//! Supported: block layout with margin collapsing, inline layout with line
//! breaking and vertical alignment, list markers, replaced elements
//! (images), form controls, flex layout, table layout, relative
//! positioning, the scrollable overflow of scroll containers (see
//! `scroll.rs`). Floats and absolute positioning are approximated (see
//! `block.rs`).
//!
//! All lengths and coordinates stay within ±[`swb_style::Length::MAX_PX`],
//! and box nesting is limited (see `box_tree.rs`), so that hostile content
//! cannot produce infinite values or unbounded recursion.

mod block;
mod box_tree;
mod control;
mod flex;
mod fonts;
mod fragment;
mod geom;
mod inline;
mod intrinsic;
mod list_marker;
mod replaced;
mod scroll;
mod source_map;
mod table;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use swb_dom::{Document, NodeId, local_name};
use swb_style::{ComputedStyle, Overflow, StyleMap};
use swb_text::FontContext;

pub use control::{Control, ControlKind, FormControls, MAX_SELECT_OPTIONS, NoFormControls};
pub use fragment::{
    BoxContent, BoxFragment, CanvasBackground, Caret, CellPaint, CollapsedEdge, ControlContent,
    Fragment, FragmentRef, FragmentTree, PartBackground, PositionedGlyph, TablePaint, TextFragment,
};
pub use geom::{Edges, Point, Rect, Size};
pub use replaced::{DEFAULT_OBJECT_SIZE, NaturalSize};
pub use scroll::{NoScroll, ScrollOffsets, ScrollState, clamp_scroll_offset, scroll_range};

use block::ContainingBlock;
use box_tree::{BuildContext, InlineFormattingContext};
use geom::clamp_length;

/// Natural sizes of replaced elements (images), supplied by the engine.
pub trait ReplacedSizes {
    /// The natural dimensions of the replaced element `node`, or `None` if
    /// its image is not loaded or broken.
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
    /// Natural sizes of images.
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
    /// [`block::layout_flex_item`]) and table cells.
    pub(crate) layouts: LayoutCache,
    /// The number of flex item and table cell layouts that were not in the
    /// cache.
    pub(crate) uncached_layouts: usize,
    /// Data of the tables of this layout pass.
    pub(crate) tables: table::TableCache,
}

impl<'a> LayoutContext<'a> {
    pub(crate) fn new(fonts: &'a mut FontContext) -> Self {
        LayoutContext {
            fonts,
            quirks: false,
            line_height_quirks: false,
            shaped: HashMap::new(),
            layouts: LayoutCache::default(),
            uncached_layouts: 0,
            tables: table::TableCache::default(),
        }
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
    let root = root_box.map(|root_box| block::layout_root(ctx, &root_box, icb));
    let scroll_size = scroll_size(root.as_ref(), input.viewport);
    FragmentTree {
        root,
        canvas_background: canvas_background(input.document, input.styles),
        scroll_size,
        viewport_overflow: viewport_overflow
            .map_or((Overflow::Visible, Overflow::Visible), |(_, x, y)| (x, y)),
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

/// The size of the scrollable area: the union of the viewport and all
/// content that is not clipped. Also on an axis where the viewport's
/// overflow is `hidden` (or `clip`, which applies as `hidden` to the
/// viewport): scripts can scroll it there, the user cannot (as in
/// Chromium). Content after the end of a line where white space hangs does
/// not count ([`scroll::text_overflow_rect`], [`scroll::box_overflow_rect`]).
/// Rectangles with zero width or height count with their position:
/// Chromium counts the line boxes of zero-size inline content (but not a
/// zero-size block, a deviation).
fn scroll_size(root: Option<&BoxFragment>, viewport: Size) -> Size {
    let mut extent = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    if let Some(root) = root {
        fn add(extent: &mut Rect, r: Rect) {
            *extent = extent.union(&r);
        }
        fn visit(b: &BoxFragment, origin: Point, limit: Option<f32>, extent: &mut Rect) {
            let rect = b.border_rect.translate(origin);
            add(
                extent,
                scroll::box_overflow_rect(b, limit).translate(origin),
            );
            if b.style.overflow_x.clips() && b.style.overflow_y.clips() {
                return;
            }
            let limit = scroll::child_limit(b, limit);
            for child in b.children.iter() {
                match child {
                    Fragment::Box(cb) => visit(cb, rect.origin(), limit, extent),
                    Fragment::Text(t) => {
                        let text = scroll::text_overflow_rect(t, limit);
                        add(extent, text.translate(rect.origin()));
                    }
                }
            }
        }
        visit(root, Point::default(), None, &mut extent);
    }
    let width = extent.right().max(viewport.width);
    let height = extent.bottom().max(viewport.height);
    Size::new(clamp_length(width), clamp_length(height))
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
