//! Style and layout queries of scripts (ADR 0028 sections 9 and 10):
//! `getComputedStyle` (the computed style of an element or of its
//! pseudo-element, the resolved value of a property) and the layout flush
//! of a script task, which needs the page's font context: the page lends
//! it with the document state for each task ([`crate::Page`]'s
//! `take_document_state`), and [`DocumentState::with_lent_fonts`] gives it
//! to a query. The geometry of 7b (`getBoundingClientRect`, `offset*`,
//! `scroll*`, `elementFromPoint`) uses the same path.
//!
//! The values that layout resolves come from the box of the element in the
//! fragment tree ([`UsedBox`]): an index of all boxes is built after a
//! layout at the first query that needs it, so reading the width of every
//! element costs one walk of the fragment tree.

use std::collections::HashMap;
use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::{BoxFragment, Fragment, FragmentTree, Rect, Size};
use swb_style::cssom::resolved::{self, BoxKind, PseudoElement, Resolver, UsedBox};
use swb_style::{ComputedStyle, Display, Position, PseudoKind};
use swb_text::FontContext;

use super::DocumentState;

/// The used boxes of a layout, by element and pseudo-element.
pub(crate) type BoxIndex = HashMap<(NodeId, Option<PseudoKind>), UsedBox>;

impl DocumentState {
    /// Runs `f` with the font context that the page lent with the state
    /// for a script task; `None` outside a task (the page keeps the
    /// context then).
    pub(crate) fn with_lent_fonts<R>(
        &mut self,
        f: impl FnOnce(&mut Self, &mut FontContext) -> R,
    ) -> Option<R> {
        let mut fonts = self.lent_fonts.take()?;
        let result = f(self, &mut fonts);
        self.lent_fonts = Some(fonts);
        Some(result)
    }

    /// A layout flush for a query of a script task (the runs count as
    /// forced). False if there is no layout: the style sheets have not
    /// arrived, or the state is not lent to a task.
    pub(crate) fn script_layout_flush(&mut self) -> bool {
        self.process_changes();
        self.with_lent_fonts(DocumentState::query_layout);
        self.fragments.is_some()
    }

    /// The computed style of `node` or of its pseudo-element `pseudo`, for
    /// `getComputedStyle`; `None` if the element is not in the document (or
    /// the style sheets have not arrived). A pseudo-element gets a style
    /// also when it has no box.
    pub(crate) fn query_computed_style(
        &mut self,
        node: NodeId,
        pseudo: Option<PseudoElement>,
    ) -> Option<Arc<ComputedStyle>> {
        if !self.dom.is_connected(node) {
            return None;
        }
        let style = self.query_element_style(node)?;
        let kind = match pseudo {
            None => return Some(style),
            Some(PseudoElement::Unstyled) => {
                return Some(Arc::new(ComputedStyle::inherit_from(&style)));
            }
            Some(PseudoElement::Styled(kind)) => kind,
        };
        if let Some(style) = self.pseudo_styles.get(&(node, kind)) {
            return Some(Arc::clone(style));
        }
        let chain: Vec<(NodeId, Arc<ComputedStyle>)> = self
            .dom
            .ancestors(node)
            .take_while(|&n| n != NodeId::DOCUMENT)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .chain(std::iter::once(node))
            .filter_map(|n| self.current_style(n).map(|s| (n, s)))
            .collect();
        let stylist = self.stylist.as_ref()?;
        let env = self.media_environment();
        let style = Arc::new(swb_style::compute_pseudo_element_style(
            &self.dom,
            stylist,
            &env,
            &self.states,
            self.base_url.value(),
            &chain,
            kind,
        )?);
        self.pseudo_styles.insert((node, kind), Arc::clone(&style));
        Some(style)
    }

    /// The style of `node` from the full styles if they are current, else
    /// from the element styles of queries.
    fn current_style(&self, node: NodeId) -> Option<Arc<ComputedStyle>> {
        let full = (!self.styles_stale)
            .then(|| self.styles.as_ref().and_then(|s| s.get(node)))
            .flatten();
        full.or_else(|| self.element_styles.get(&node)).cloned()
    }

    /// The resolved value of `property` of `node` (or of its
    /// pseudo-element) for `getComputedStyle` (CSSOM §9): empty if the
    /// element has no style. A value that layout resolves flushes the
    /// layout first.
    pub(crate) fn resolved_value(
        &mut self,
        node: NodeId,
        pseudo: Option<PseudoElement>,
        property: &str,
    ) -> String {
        let styled = match pseudo {
            None => Some(None),
            Some(PseudoElement::Styled(kind)) => Some(Some(kind)),
            Some(PseudoElement::Unstyled) => None,
        };
        let used = match styled {
            Some(kind) if resolved::needs_layout(property) && self.dom.is_connected(node) => {
                self.query_used_box(node, kind)
            }
            _ => None,
        };
        let Some(style) = self.query_computed_style(node, pseudo) else {
            return String::new();
        };
        resolved::resolved_value(
            property,
            &Resolver {
                style: &style,
                used: used.as_ref(),
                pseudo: pseudo.and_then(PseudoElement::kind),
            },
        )
    }

    /// The used box of `node` (or of its pseudo-element) after a layout
    /// flush; `None` if it has no box.
    pub(crate) fn query_used_box(
        &mut self,
        node: NodeId,
        pseudo: Option<PseudoKind>,
    ) -> Option<UsedBox> {
        if !self.script_layout_flush() {
            return None;
        }
        let serial = self.layout_serial;
        if self.used_boxes.as_ref().is_none_or(|(s, _)| *s != serial) {
            let tree = self.fragments.as_ref()?;
            self.used_boxes = Some((serial, index_boxes(tree, self.view.viewport)));
        }
        self.used_boxes.as_ref()?.1.get(&(node, pseudo)).cloned()
    }
}

/// What the walk keeps of an ancestor box.
struct Frame {
    /// The border box in document coordinates (layout positions, without
    /// scrolling).
    rect: Rect,
    padding: [f32; 4],
    border: [f32; 4],
    is_inline: bool,
    display: Display,
    /// The flex direction is a row.
    row: bool,
    /// It is the containing block of absolutely positioned boxes.
    positioned: bool,
    transformed: bool,
    scroll_container: bool,
}

impl Frame {
    fn padding_box(&self) -> Rect {
        inset(self.rect, self.border)
    }

    fn content_box(&self) -> Rect {
        inset(inset(self.rect, self.border), self.padding)
    }
}

/// `rect` without the sides `edges` (top, right, bottom, left).
fn inset(rect: Rect, [top, right, bottom, left]: [f32; 4]) -> Rect {
    Rect::new(
        rect.x + left,
        rect.y + top,
        (rect.width - left - right).max(0.0),
        (rect.height - top - bottom).max(0.0),
    )
}

/// The used boxes of all elements and pseudo-elements of a layout (the
/// first fragment of each).
fn index_boxes(tree: &FragmentTree, viewport: Size) -> BoxIndex {
    let mut index = BoxIndex::new();
    if let Some(root) = &tree.root {
        let mut stack = Vec::new();
        visit(root, 0.0, 0.0, viewport, &mut stack, &mut index);
    }
    index
}

/// Adds the box `b`, whose parent's border box is at (`x`, `y`), and its
/// descendants. The depth of the fragment tree is bounded by layout's box
/// depth limit.
fn visit(
    b: &BoxFragment,
    x: f32,
    y: f32,
    viewport: Size,
    stack: &mut Vec<Frame>,
    index: &mut BoxIndex,
) {
    let rect = Rect::new(
        x + b.border_rect.x,
        y + b.border_rect.y,
        b.border_rect.width,
        b.border_rect.height,
    );
    let edges = |e: &swb_layout::Edges| [e.top, e.right, e.bottom, e.left];
    let style = &b.style;
    if let Some(node) = b.node {
        index
            .entry((node, b.pseudo))
            .or_insert_with(|| used_box(b, rect, viewport, stack));
    }
    stack.push(Frame {
        rect,
        padding: edges(&b.padding),
        border: edges(&b.border),
        is_inline: b.is_inline,
        display: style.display,
        row: style.flex_direction.is_row(),
        positioned: style.position != Position::Static,
        transformed: style.has_transform(),
        scroll_container: style.overflow_x.is_scroll_container()
            || style.overflow_y.is_scroll_container(),
    });
    let children = b
        .children
        .iter()
        .chain(b.hidden.iter().flat_map(|h| h.iter()));
    for child in children {
        if let Fragment::Box(child) = child {
            visit(child, rect.x, rect.y, viewport, stack, index);
        }
    }
    stack.pop();
}

/// The used box of `b` at `rect`, inside the boxes `stack`.
fn used_box(b: &BoxFragment, rect: Rect, viewport: Size, stack: &[Frame]) -> UsedBox {
    let style = &b.style;
    let icb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let parent = stack.iter().rev().find(|f| !f.is_inline);
    let (kind, cb) = match style.position {
        _ if b.is_inline => (BoxKind::Inline, parent.map_or(icb, Frame::content_box)),
        Position::Absolute => (
            BoxKind::Absolute,
            stack
                .iter()
                .rev()
                .find(|f| f.positioned || f.transformed)
                .map_or(icb, Frame::padding_box),
        ),
        Position::Fixed => (
            BoxKind::Absolute,
            stack
                .iter()
                .rev()
                .find(|f| f.transformed)
                .map_or(icb, Frame::padding_box),
        ),
        _ => {
            let direct = stack.last();
            let kind = match direct.map(|f| f.display) {
                Some(Display::Flex | Display::InlineFlex) => BoxKind::FlexItem {
                    row: direct.is_some_and(|f| f.row),
                },
                Some(Display::Grid | Display::InlineGrid) => BoxKind::GridItem,
                _ if style.is_floating()
                    || style.display.is_inline_level()
                    || style.display.is_table_internal()
                    || matches!(style.display, Display::Table) =>
                {
                    BoxKind::Other
                }
                _ => BoxKind::Block,
            };
            (kind, parent.map_or(icb, Frame::content_box))
        }
    };
    let scrollport = stack.iter().rev().find(|f| f.scroll_container).map_or(
        (viewport.width, viewport.height),
        |f| {
            let p = f.padding_box();
            (p.width, p.height)
        },
    );
    UsedBox {
        kind,
        x: rect.x - cb.x,
        y: rect.y - cb.y,
        width: rect.width,
        height: rect.height,
        padding: [
            b.padding.top,
            b.padding.right,
            b.padding.bottom,
            b.padding.left,
        ],
        border: [b.border.top, b.border.right, b.border.bottom, b.border.left],
        cb_width: cb.width,
        cb_height: cb.height,
        scrollport,
    }
}
