//! Box tree construction, layout algorithms and the fragment tree.
//!
//! [`layout`] takes a document with computed styles and produces a
//! [`FragmentTree`]: positioned boxes and glyph runs in CSS pixels.
//!
//! Supported: block layout with margin collapsing, inline layout with line
//! breaking and vertical alignment, list markers, replaced elements
//! (images), flex layout, relative positioning. Floats and absolute
//! positioning are approximated (see `block.rs`).
//!
//! All lengths and coordinates stay within ±[`swb_style::Length::MAX_PX`],
//! and box nesting is limited (see `box_tree.rs`), so that hostile content
//! cannot produce infinite values or unbounded recursion.

mod block;
mod box_tree;
mod flex;
mod fonts;
mod fragment;
mod geom;
mod inline;
mod intrinsic;
mod list_marker;
mod replaced;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use swb_dom::{Document, NodeId, local_name};
use swb_style::{ComputedStyle, Overflow, StyleMap};
use swb_text::FontContext;

pub use fragment::{
    BoxContent, BoxFragment, CanvasBackground, Fragment, FragmentRef, FragmentTree,
    PositionedGlyph, TextFragment,
};
pub use geom::{Edges, Point, Rect, Size};

use block::ContainingBlock;
use box_tree::{BuildContext, InlineFormattingContext};
use geom::clamp_length;

/// Natural sizes of replaced elements (images), supplied by the engine.
pub trait ReplacedSizes {
    /// The natural width and height of the replaced element `node`, in CSS
    /// px, or `None` if unknown (not loaded or broken).
    fn natural_size(&self, node: NodeId) -> Option<(f32, f32)>;
}

/// A `ReplacedSizes` that knows no sizes.
pub struct NoReplacedSizes;

impl ReplacedSizes for NoReplacedSizes {
    fn natural_size(&self, _node: NodeId) -> Option<(f32, f32)> {
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
}

/// State shared by all layout functions during one layout pass.
pub(crate) struct LayoutContext<'a> {
    pub(crate) fonts: &'a mut FontContext,
    /// Shaped text per inline formatting context, keyed by its number.
    shaped: HashMap<usize, Rc<inline::ShapedText>>,
    /// Laid-out flex items per item and constraints (see
    /// [`block::layout_flex_item`]).
    pub(crate) flex_items: FlexItemCache,
    /// The number of flex item layouts that were not in the cache.
    pub(crate) flex_item_layouts: usize,
}

impl<'a> LayoutContext<'a> {
    pub(crate) fn new(fonts: &'a mut FontContext) -> Self {
        LayoutContext {
            fonts,
            shaped: HashMap::new(),
            flex_items: FlexItemCache::default(),
            flex_item_layouts: 0,
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

/// The flex item layout cache. Entries are cheap: a fragment's children
/// are shared (`Arc`), so a nested flex item's subtree is stored once, not
/// once per level, and a cache hit copies only the top fragment.
#[derive(Default)]
pub(crate) struct FlexItemCache {
    entries: HashMap<LayoutKey, BoxFragment>,
}

impl FlexItemCache {
    pub(crate) fn get(&self, key: &LayoutKey) -> Option<&BoxFragment> {
        self.entries.get(key)
    }

    pub(crate) fn insert(&mut self, key: LayoutKey, fragment: BoxFragment) {
        self.entries.insert(key, fragment);
    }
}

/// A box (by its number) and the constraints it is laid out with: the key
/// of the flex item layout cache. Sizes are compared by their bits.
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
    let viewport_overflow = viewport_overflow(input.document, input.styles);
    let build = BuildContext {
        doc: input.document,
        styles: input.styles,
        replaced: input.replaced,
        overflow_source: viewport_overflow.map(|(node, _, _)| node),
    };
    let root_box = box_tree::build_root(&build);
    let icb = ContainingBlock {
        width: input.viewport.width,
        height: Some(input.viewport.height),
    };
    let root = root_box.map(|root_box| block::layout_root(ctx, &root_box, icb));
    let (overflow_x, overflow_y) =
        viewport_overflow.map_or((Overflow::Visible, Overflow::Visible), |(_, x, y)| (x, y));
    let scroll_size = scroll_size(root.as_ref(), input.viewport, overflow_x, overflow_y);
    FragmentTree {
        root,
        canvas_background: canvas_background(input.document, input.styles),
        scroll_size,
    }
}

/// The element whose `overflow` applies to the viewport, and that value
/// (CSS Overflow 3 §3.3): the root element's, or, if the root is an HTML
/// `html` element with `overflow: visible`, the `body` element's. `None` if
/// neither has a non-visible overflow.
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
/// body's if the root has no background (CSS 2.2 §14.2).
fn canvas_background(doc: &Document, styles: &StyleMap) -> Option<CanvasBackground> {
    let root = doc.document_element()?;
    let root_style = styles.get(root)?;
    let has_background = |s: &ComputedStyle| {
        !s.background_color.resolve(s.color).is_transparent()
            || s.background_image.iter().any(Option::is_some)
    };
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

/// The size of the scrollable area: the union of the viewport and all
/// content that is not clipped. On an axis where the viewport's overflow
/// is `hidden` or `clip`, the viewport does not scroll and the size is the
/// viewport's.
fn scroll_size(
    root: Option<&BoxFragment>,
    viewport: Size,
    overflow_x: Overflow,
    overflow_y: Overflow,
) -> Size {
    let mut extent = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    if let Some(root) = root {
        fn visit(b: &BoxFragment, origin: Point, extent: &mut Rect) {
            let rect = b.border_rect.translate(origin);
            *extent = extent.union(&rect);
            if b.style.overflow_x.clips() && b.style.overflow_y.clips() {
                return;
            }
            for child in b.children.iter() {
                match child {
                    Fragment::Box(cb) => visit(cb, rect.origin(), extent),
                    Fragment::Text(t) => *extent = extent.union(&t.rect.translate(rect.origin())),
                }
            }
        }
        visit(root, Point::default(), &mut extent);
    }
    let fixed = |o: Overflow| matches!(o, Overflow::Hidden | Overflow::Clip);
    let width = if fixed(overflow_x) {
        viewport.width
    } else {
        extent.right().max(viewport.width)
    };
    let height = if fixed(overflow_y) {
        viewport.height
    } else {
        extent.bottom().max(viewport.height)
    };
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
        assert_eq!(l.tree.scroll_size, Size::new(800.0, 2000.0));
        let body = l.tree.border_boxes(l.doc.body().expect("body"));
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn root_overflow_hidden_disables_scrolling() {
        let l = layout_html(
            "<!DOCTYPE html><style>html { overflow: hidden }</style>\
             <body><div style='height:2000px'></div>",
        );
        assert_eq!(l.tree.scroll_size, Size::new(800.0, 600.0));
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
             </div><div style='flex-shrink:1e39; width:1e39px'>b</div></div>",
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
