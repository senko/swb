//! The paint-only refresh of a fragment tree (ADR 0044 section 9).
//!
//! When a restyle changed paint properties only, the layout stays and the
//! styles in the fragments are replaced ([`FragmentTree::apply_paint_changes`]).
//! Elements can share one style object (equal computed values), so a
//! fragment is matched to a changed style by its owner (the element and
//! pseudo-element that made it) and the object together:
//!
//! - A box fragment of a changed element or pseudo-element that holds the
//!   old style object gets the new object. The fragments of the root and
//!   body elements can hold a modified copy (the `overflow` of the
//!   viewport propagation): the paint properties are copied into the copy.
//! - A text fragment belongs to the element that contains its text node
//!   (the element of generated text), or to the anonymous or generated box
//!   around it, and follows that owner.
//! - Anonymous boxes and the text of controls derive their styles from the
//!   parent box: when an inherited paint property of the parent changed,
//!   they get the inherited paint properties of the new parent style.
//! - Where layout baked a value of a style into the fragments (the
//!   native look of a control, the backgrounds of table parts in the
//!   cells, collapsed borders, the content of inline SVG, the canvas
//!   background), the refresh derives it again or, where that is not
//!   cheap, reports [`Unsupported`], and the caller lays out again.
//! - Where a transform changed, the scrollable overflow of the scroll
//!   containers above it and the scroll size of the viewport are computed
//!   again.
//!
//! The check mode of the engine compares the result with a full layout
//! ([`FragmentTree::first_difference`], in `paint_restyle/difference.rs`).

use std::sync::Arc;

use swb_dom::{Document, NodeId};
use swb_style::{
    ComputedStyle, Display, PaintChanges, PseudoKind, StyleMap, StylePair,
    copy_inherited_paint_properties, copy_paint_properties,
};

use crate::fragment::{
    BoxContent, BoxFragment, CellPaint, Fragment, FragmentTree, PartBackground, TextFragment,
};
use crate::geom::Rect;
use crate::{canvas_background, has_background, scroll};

mod difference;

/// The refresh cannot keep the layout: the reason, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported(pub &'static str);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The old and new style of the parent box of the fragments being
/// visited (the parent changed).
struct Parent {
    old: Arc<ComputedStyle>,
    new: Arc<ComputedStyle>,
    /// An inherited paint property changed: derived styles derive again.
    inherited: bool,
    /// The parent is a form control: its text holds a style derived from
    /// the control's.
    control: bool,
    /// The parent is an anonymous or a generated box: its own style object
    /// belongs to it alone, so the text that holds it follows it.
    own_style: bool,
}

/// The replaced styles, indexed for the walk.
struct Refresh<'a> {
    doc: &'a Document,
    pairs: &'a [StylePair],
    /// The index of each pair by its element, sorted by node (the order of
    /// the pairs for one node is kept).
    owners: Vec<(NodeId, u32)>,
    /// The root and body elements, whose fragments can hold a modified
    /// copy of the style (the propagation of `overflow`).
    special: [Option<NodeId>; 2],
    /// A color of a table or a part of a table changed.
    table_colors: bool,
}

impl<'a> Refresh<'a> {
    fn new(changes: &'a PaintChanges, doc: &'a Document) -> Result<Refresh<'a>, Unsupported> {
        let pairs = &changes.styles[..];
        let mut owners = Vec::with_capacity(pairs.len());
        let mut table_colors = false;
        for (i, pair) in pairs.iter().enumerate() {
            if pair.pseudo == Some(PseudoKind::Placeholder) {
                // The text of the control derives its style from this one.
                return Err(Unsupported("the style of a placeholder changed"));
            }
            if pair.pseudo.is_none()
                && is_table_part(pair.new.display)
                && pair.new.display != Display::TableRow
                && has_background(&pair.old) != has_background(&pair.new)
            {
                // The cells of the table hold the backgrounds of the parts
                // that have one; a background of a row is added or taken
                // out in `toggle_row_background`, the others are not.
                return Err(Unsupported(
                    "the background of a table part appeared or went",
                ));
            }
            let index = u32::try_from(i).map_err(|_| Unsupported("too many styles"))?;
            owners.push((pair.node, index));
            table_colors |= pair.delta.colors && is_table_internal(pair.new.display);
        }
        owners.sort_by_key(|&(node, _)| node);
        Ok(Refresh {
            doc,
            pairs,
            owners,
            special: [doc.document_element(), doc.body()],
            table_colors,
        })
    }

    /// The pairs of the element `node` and its pseudo-elements.
    fn pairs_of(&self, node: NodeId) -> impl Iterator<Item = &'a StylePair> + '_ {
        let start = self.owners.partition_point(|&(n, _)| n < node);
        self.owners
            .iter()
            .skip(start)
            .take_while(move |&&(n, _)| n == node)
            .filter_map(|&(_, i)| self.pairs.get(i as usize))
    }

    /// The pair of a box fragment of `node` and `pseudo` that holds
    /// `style`: the pair whose old style is the object, or for the root and
    /// body elements, the pair of the element (the fragment can hold a
    /// modified copy).
    fn pair_of_box(
        &self,
        node: Option<NodeId>,
        pseudo: Option<PseudoKind>,
        style: &Arc<ComputedStyle>,
    ) -> Option<&'a StylePair> {
        let node = node?;
        let special = pseudo.is_none() && self.special.contains(&Some(node));
        self.pairs_of(node)
            .find(|pair| pair.pseudo == pseudo && (special || Arc::ptr_eq(&pair.old, style)))
    }

    /// The pair of an element (`node` is one) or pseudo-element of the
    /// owner of a text fragment (or of a shape of inline SVG) that holds
    /// `style`. The owner of a text node is its flat parent element.
    fn pair_of_text(&self, node: NodeId, style: &Arc<ComputedStyle>) -> Option<&'a StylePair> {
        let owner = if self.doc.node(node).is_element() {
            node
        } else {
            self.doc.flat_parent_element(node)?
        };
        self.pairs_of(owner)
            .find(|pair| Arc::ptr_eq(&pair.old, style))
    }

    /// Visits a box and its subtree. Returns true if a transform changed in
    /// it (the box or a descendant).
    fn visit_box(&self, b: &mut BoxFragment, parent: Option<&Parent>) -> Result<bool, Unsupported> {
        let old = Arc::clone(&b.style);
        let mut own_geometry = false;
        let mut inherited = false;
        // Whether a row got or lost a background: the cells hold them.
        let mut row_background = None;
        let new = match self.pair_of_box(b.node, b.pseudo, &old) {
            Some(pair) => {
                own_geometry = pair.delta.geometry;
                inherited = pair.delta.inherited;
                if pair.pseudo.is_none()
                    && pair.new.display == Display::TableRow
                    && has_background(&pair.old) != has_background(&pair.new)
                {
                    row_background = Some(has_background(&pair.new));
                }
                Some(if Arc::ptr_eq(&old, &pair.old) {
                    Arc::clone(&pair.new)
                } else {
                    let mut copy = (*old).clone();
                    copy_paint_properties(&pair.new, &mut copy);
                    Arc::new(copy)
                })
            }
            None => match parent {
                Some(p) if b.node.is_none() && p.inherited => {
                    if old.display == Display::InlineTable {
                        // Its style derives from an inline box that is not
                        // its parent in the fragment tree.
                        return Err(Unsupported("an anonymous inline table inherits"));
                    }
                    inherited = true;
                    Some(derived(&old, &p.new))
                }
                _ => None,
            },
        };
        if let BoxContent::Table(table) = &b.content
            && table.collapsed.is_some()
            && self.table_colors
        {
            return Err(Unsupported("a color of a collapsed table changed"));
        }
        let own_style = b.node.is_none() || b.pseudo.is_some();
        let child_parent = new.map(|new| {
            b.style = Arc::clone(&new);
            if let BoxContent::Control(control) = &mut b.content {
                control.native = crate::control::has_native_look(control.kind, &new);
            }
            Parent {
                old,
                new,
                inherited,
                control: matches!(b.content, BoxContent::Control(_)),
                own_style,
            }
        });
        if let Some(on) = row_background {
            toggle_row_background(b, on)?;
        }
        self.refresh_content(b)?;
        let mut below = false;
        for child in Arc::make_mut(&mut b.children).iter_mut() {
            below |= self.visit_fragment(child, child_parent.as_ref())?;
        }
        if let Some(hidden) = &mut b.hidden {
            for child in Arc::make_mut(hidden).iter_mut() {
                below |= self.visit_fragment(child, child_parent.as_ref())?;
            }
        }
        if below && b.scrollable_overflow.is_some() {
            scroll::refresh_scrollable_overflow(b);
        }
        Ok(own_geometry || below)
    }

    fn visit_fragment(
        &self,
        fragment: &mut Fragment,
        parent: Option<&Parent>,
    ) -> Result<bool, Unsupported> {
        match fragment {
            Fragment::Box(b) => self.visit_box(b, parent),
            Fragment::Text(t) => {
                self.visit_text(t, parent);
                Ok(false)
            }
        }
    }

    /// Replaces the style of a text fragment: by its owner, or as the
    /// anonymous or generated box around it, or derived for a control.
    fn visit_text(&self, t: &mut TextFragment, parent: Option<&Parent>) {
        if let Some(pair) = self.pair_of_text(t.node, &t.style) {
            t.style = Arc::clone(&pair.new);
            return;
        }
        let Some(p) = parent else {
            return;
        };
        if p.own_style && Arc::ptr_eq(&t.style, &p.old) {
            t.style = Arc::clone(&p.new);
        } else if p.control && p.inherited {
            t.style = derived(&t.style, &p.new);
        }
    }

    /// Replaces the styles that the content of a box holds.
    fn refresh_content(&self, b: &mut BoxFragment) -> Result<(), Unsupported> {
        if let BoxContent::TableCell(cell) = &mut b.content
            && cell.backgrounds.iter().any(|part| {
                part.node
                    .is_some_and(|node| self.pair_of_text(node, &part.style).is_some())
            })
        {
            for part in &mut Arc::make_mut(cell).backgrounds {
                let pair = part.node.and_then(|n| self.pair_of_text(n, &part.style));
                if let Some(pair) = pair {
                    part.style = Arc::clone(&pair.new);
                }
            }
        }
        if let BoxContent::Svg(svg) = &mut b.content {
            let pair_of =
                |node: Option<NodeId>, style: &Arc<ComputedStyle>| self.pair_of_text(node?, style);
            if svg.any_style(|node, style| pair_of(node, style).is_some()) {
                Arc::make_mut(svg)
                    .replace_styles(&mut |node, style| {
                        pair_of(node, style).map(|pair| Arc::clone(&pair.new))
                    })
                    .map_err(Unsupported)?;
            }
        }
        Ok(())
    }
}

/// Adds the background of row `b` (its new style is in `b.style`) to the
/// cells of the row, or takes it out (`on` false): the cells hold the
/// backgrounds of their row, row group and columns, with the rectangle of the
/// part relative to the cell (`table/fragments.rs`: the row is the last
/// part, painted over the whole cell).
fn toggle_row_background(b: &mut BoxFragment, on: bool) -> Result<(), Unsupported> {
    let (style, node) = (Arc::clone(&b.style), b.node);
    let (width, height) = (b.border_rect.width, b.border_rect.height);
    for child in Arc::make_mut(&mut b.children).iter_mut() {
        let Fragment::Box(cell) = child else {
            return Err(Unsupported("a row has a child that is not a cell"));
        };
        if on {
            let part = PartBackground {
                node,
                style: Arc::clone(&style),
                area: Rect::new(-cell.border_rect.x, 0.0, width, height),
                clip: Rect::new(0.0, 0.0, cell.border_rect.width, cell.border_rect.height),
            };
            match &mut cell.content {
                BoxContent::TableCell(paint) => Arc::make_mut(paint).backgrounds.push(part),
                BoxContent::None => {
                    cell.content = BoxContent::TableCell(Arc::new(CellPaint {
                        backgrounds: vec![part],
                        hidden: false,
                        collapsed_borders: false,
                    }));
                }
                _ => return Err(Unsupported("a table cell has other content")),
            }
        } else if let BoxContent::TableCell(paint) = &mut cell.content {
            let paint = Arc::make_mut(paint);
            paint.backgrounds.retain(|part| part.node != node);
            if paint.backgrounds.is_empty() && !paint.hidden && !paint.collapsed_borders {
                cell.content = BoxContent::None;
            }
        }
    }
    Ok(())
}

/// `style` with the inherited paint properties of `parent`: what
/// `ComputedStyle::anonymous_from(parent)` made again for a changed
/// parent.
fn derived(style: &Arc<ComputedStyle>, parent: &ComputedStyle) -> Arc<ComputedStyle> {
    let mut copy = (**style).clone();
    copy_inherited_paint_properties(parent, &mut copy);
    if copy == **style {
        Arc::clone(style)
    } else {
        Arc::new(copy)
    }
}

/// True for the boxes that paint their background inside the cells of a
/// table (`table/fragments.rs`).
fn is_table_part(display: Display) -> bool {
    matches!(
        display,
        Display::TableRowGroup
            | Display::TableHeaderGroup
            | Display::TableFooterGroup
            | Display::TableRow
            | Display::TableColumnGroup
            | Display::TableColumn
    )
}

/// True for a table and the boxes that take part in its borders.
fn is_table_internal(display: Display) -> bool {
    is_table_part(display)
        || matches!(
            display,
            Display::Table | Display::InlineTable | Display::TableCell
        )
}

impl FragmentTree {
    /// Replaces the styles of the fragments after a restyle that changed
    /// paint properties only (`changes`, ADR 0044 section 9), and computes
    /// the scrollable overflow again where a transform changed. `doc` and
    /// `styles` are those of the new styles. Returns true if a transform
    /// changed (the geometry that scripts read, hit testing and the scroll
    /// size can differ). On [`Unsupported`] the tree is in an unspecified
    /// state: lay out again.
    pub fn apply_paint_changes(
        &mut self,
        changes: &PaintChanges,
        doc: &Document,
        styles: &StyleMap,
    ) -> Result<bool, Unsupported> {
        let refresh = Refresh::new(changes, doc)?;
        let mut moved = false;
        if let Some(root) = &mut self.root {
            moved |= refresh.visit_box(root, None)?;
        }
        for top in &mut self.top_layer {
            moved |= refresh.visit_box(top, None)?;
        }
        self.canvas_background = canvas_background(doc, styles);
        if moved {
            self.scroll_size = scroll::viewport_scroll_size(self.roots(), self.viewport);
        }
        Ok(changes.styles.iter().any(|pair| pair.delta.geometry))
    }
}
