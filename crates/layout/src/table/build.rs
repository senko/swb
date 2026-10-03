//! Box construction for tables, with the anonymous table object fixup of
//! CSS 2.2 §17.2.1 (<https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>)
//! and CSS Tables 3 §3.1
//! (<https://www.w3.org/TR/css-tables-3/#fixup-algorithm>):
//!
//! - Children of a table that are not rows, row groups, columns, column
//!   groups or captions are wrapped in an anonymous row group; children of
//!   a row group that are not rows in an anonymous row; children of a row
//!   that are not cells in an anonymous cell. Consecutive children share
//!   one anonymous box.
//! - Table-internal boxes outside a table are wrapped in an anonymous
//!   table (`ContainerBuilder` collects them, see `box_tree.rs`).
//! - Text that is only collapsible white space is dropped when its parent
//!   is a table, a row group or a row, as Chromium does (CSS 2.2 drops it
//!   only between table parts).
//! - Children of columns, and children of column groups that are not
//!   columns, are not rendered.
//!
//! Row groups are ordered as Chromium lays them out: the first header group
//! first, the first footer group last, all others (also further headers and
//! footers) in tree order between them.
//!
//! Each element and each anonymous table box counts as one level for the
//! box depth limit. Table parts that would be deeper than the limit keep
//! only their text (in an anonymous cell).

use std::sync::Arc;

use swb_dom::{NodeData, NodeId, local_name};
use swb_style::{ComputedStyle, Display, PseudoKind, content_text};

use super::{CellBox, ColumnBox, MAX_COLSPAN, MAX_ROWSPAN, RowBox, SectionBox, TableBox};
use crate::box_tree::{
    BoxBase, BuildContext, BuildState, ContainerBuilder, IndependentBox, MAX_BOX_DEPTH,
    build_independent, is_collapsible_space, is_replaced,
};

/// Builds the table of a table element or pseudo-element.
pub(crate) fn build_table(
    ctx: &BuildContext<'_>,
    base: &BoxBase,
    state: &mut BuildState,
) -> TableBox {
    let mut builder = TableBuilder::new(Arc::clone(&base.style), 0);
    match base.element() {
        Some(node) => builder.push_children(ctx, node, state),
        None => {
            if let (Some(text), Some(node)) = (content_text(&base.style), base.node) {
                builder.push_generated(ctx, node, &base.style, &text, state);
            }
        }
    }
    builder.finish(state)
}

/// The kind of box a display value makes inside a table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Caption,
    Column,
    ColumnGroup,
    Section,
    Row,
    Cell,
    /// Anything else: wrapped in anonymous table parts.
    Other,
}

fn kind(display: Display) -> Kind {
    match display {
        Display::TableCaption => Kind::Caption,
        Display::TableColumn => Kind::Column,
        Display::TableColumnGroup => Kind::ColumnGroup,
        Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
            Kind::Section
        }
        Display::TableRow => Kind::Row,
        Display::TableCell => Kind::Cell,
        _ => Kind::Other,
    }
}

/// True if `text` is only collapsible white space in `style`.
fn is_collapsible_white_space(style: &ComputedStyle, text: &str) -> bool {
    style.white_space.collapses_spaces() && text.chars().all(is_collapsible_space)
}

/// One child of a table part during construction.
enum Item<'a> {
    /// An element (its display is not `none` or `contents`).
    Element(NodeId, &'a Arc<ComputedStyle>),
    /// A pseudo-element box.
    Pseudo(BoxBase),
    /// A text node with the style of its parent.
    Text(NodeId, &'a Arc<ComputedStyle>, &'a str),
    /// Generated text of a pseudo-element `content`.
    Generated(NodeId, &'a Arc<ComputedStyle>, &'a str),
}

impl Item<'_> {
    /// The kind of box the item makes. Replaced elements are never table
    /// parts (Chromium keeps the image as a block-level box in an
    /// anonymous cell).
    fn kind(&self, ctx: &BuildContext<'_>) -> Kind {
        match self {
            Item::Element(node, _) if is_replaced(ctx, *node) => Kind::Other,
            Item::Element(_, style) => kind(style.display),
            Item::Pseudo(base) => kind(base.style.display),
            Item::Text(..) | Item::Generated(..) => Kind::Other,
        }
    }
}

/// Calls `f` for every child of `parent` that takes part in table
/// construction, including its `::before` and `::after` boxes. The children
/// of `display: contents` elements take their place. Collapsible white
/// space is dropped.
fn for_each_child(
    ctx: &BuildContext<'_>,
    parent: NodeId,
    state: &mut BuildState,
    f: &mut dyn FnMut(Item<'_>, &mut BuildState),
) {
    let Some(parent_style) = ctx.styles.get(parent) else {
        return;
    };
    pseudo(ctx, parent, PseudoKind::Before, state, f);
    for child in ctx.doc.children(parent) {
        match &ctx.doc.node(child).data {
            NodeData::Text(text) => {
                if !is_collapsible_white_space(parent_style, text) {
                    f(Item::Text(child, parent_style, text), state);
                }
            }
            NodeData::Element(_) => {
                let Some(style) = ctx.styles.get(child) else {
                    continue;
                };
                match style.display {
                    Display::None => {}
                    Display::Contents if state.depth < MAX_BOX_DEPTH => {
                        state.depth += 1;
                        for_each_child(ctx, child, state, f);
                        state.depth -= 1;
                    }
                    _ => f(Item::Element(child, style), state),
                }
            }
            _ => {}
        }
    }
    pseudo(ctx, parent, PseudoKind::After, state, f);
}

fn pseudo(
    ctx: &BuildContext<'_>,
    node: NodeId,
    kind: PseudoKind,
    state: &mut BuildState,
    f: &mut dyn FnMut(Item<'_>, &mut BuildState),
) {
    if let Some(style) = ctx.styles.pseudo(node, kind) {
        let base = BoxBase {
            node: Some(node),
            pseudo: Some(kind),
            style: Arc::clone(style),
            id: state.next_id(),
        };
        f(Item::Pseudo(base), state);
    }
}

/// Runs `f` one box level deeper, plus `extra` levels for anonymous boxes.
/// Returns `None` (and does not run `f`) if that exceeds the depth limit.
fn deeper<T>(
    state: &mut BuildState,
    extra: usize,
    f: impl FnOnce(&mut BuildState) -> T,
) -> Option<T> {
    let levels = 1 + extra;
    if state.depth + levels > MAX_BOX_DEPTH {
        return None;
    }
    state.depth += levels;
    let result = f(state);
    state.depth -= levels;
    Some(result)
}

/// Collects the children of a table (an element or an anonymous table).
pub(crate) struct TableBuilder {
    style: Arc<ComputedStyle>,
    /// The anonymous box levels above the table's children (1 for an
    /// anonymous table, whose level the element children do not count).
    extra_depth: usize,
    table: TableBox,
    header: Option<SectionBox>,
    footer: Option<SectionBox>,
    /// The anonymous row group that is open.
    section: Option<SectionBuilder>,
}

impl TableBuilder {
    /// Creates a builder for a table with style `style`. `extra_depth` is
    /// the number of anonymous box levels that are not counted in the
    /// build state's depth.
    pub(crate) fn new(style: Arc<ComputedStyle>, extra_depth: usize) -> Self {
        TableBuilder {
            style,
            extra_depth,
            table: TableBox::default(),
            header: None,
            footer: None,
            section: None,
        }
    }

    fn push_children(&mut self, ctx: &BuildContext<'_>, parent: NodeId, state: &mut BuildState) {
        for_each_child(ctx, parent, state, &mut |item, state| {
            self.push_item(ctx, item, state);
        });
    }

    /// Adds an element that is a child of an anonymous table.
    pub(crate) fn push_element(
        &mut self,
        ctx: &BuildContext<'_>,
        node: NodeId,
        state: &mut BuildState,
    ) {
        let Some(style) = ctx.styles.get(node) else {
            return;
        };
        match style.display {
            Display::None => {}
            Display::Contents => {
                // Its children are children of the table.
                let _ = deeper(state, self.extra_depth, |state| {
                    for_each_child(ctx, node, state, &mut |item, state| {
                        self.push_item(ctx, item, state);
                    });
                });
            }
            _ => self.push_item(ctx, Item::Element(node, style), state),
        }
    }

    /// Adds a pseudo-element box that is a child of an anonymous table.
    pub(crate) fn push_pseudo(
        &mut self,
        ctx: &BuildContext<'_>,
        base: BoxBase,
        state: &mut BuildState,
    ) {
        self.push_item(ctx, Item::Pseudo(base), state);
    }

    fn push_generated(
        &mut self,
        ctx: &BuildContext<'_>,
        node: NodeId,
        style: &Arc<ComputedStyle>,
        text: &str,
        state: &mut BuildState,
    ) {
        self.push_item(ctx, Item::Generated(node, style, text), state);
    }

    fn push_item(&mut self, ctx: &BuildContext<'_>, item: Item<'_>, state: &mut BuildState) {
        let extra = self.extra_depth;
        match item.kind(ctx) {
            Kind::Caption => {
                self.close_section(state);
                let Some(base) = element_base(ctx, item, state) else {
                    return;
                };
                match deeper(state, extra, |state| {
                    build_independent(ctx, base.clone(), state)
                }) {
                    Some(caption) => self.table.captions.push(caption),
                    None => self.push_flattened(ctx, &base, state),
                }
            }
            Kind::Column | Kind::ColumnGroup => {
                self.close_section(state);
                let Some(base) = element_base(ctx, item, state) else {
                    return;
                };
                if let Some(column) = deeper(state, extra, |state| build_column(ctx, base, state)) {
                    self.table.columns.push(column);
                }
            }
            Kind::Section => {
                self.close_section(state);
                let Some(base) = element_base(ctx, item, state) else {
                    return;
                };
                let display = base.style.display;
                let built = deeper(state, extra, |state| build_section(ctx, &base, state));
                match built {
                    Some(section) => self.add_section(section, display),
                    None => self.push_flattened(ctx, &base, state),
                }
            }
            Kind::Row | Kind::Cell | Kind::Other => {
                let style = &self.style;
                let section = self.section.get_or_insert_with(|| {
                    SectionBuilder::new(state.anonymous(style, Display::TableRowGroup), extra + 1)
                });
                section.push_item(ctx, item, state);
            }
        }
    }

    /// Keeps only the text of a table part that is too deep.
    fn push_flattened(&mut self, ctx: &BuildContext<'_>, base: &BoxBase, state: &mut BuildState) {
        if let Some(node) = base.element() {
            let style = &self.style;
            let extra = self.extra_depth;
            let section = self.section.get_or_insert_with(|| {
                SectionBuilder::new(state.anonymous(style, Display::TableRowGroup), extra + 1)
            });
            section.push_flattened(ctx, node, state);
        }
    }

    fn add_section(&mut self, section: SectionBox, display: Display) {
        match display {
            Display::TableHeaderGroup if self.header.is_none() => self.header = Some(section),
            Display::TableFooterGroup if self.footer.is_none() => self.footer = Some(section),
            _ => self.table.sections.push(section),
        }
    }

    fn close_section(&mut self, state: &mut BuildState) {
        if let Some(section) = self.section.take() {
            self.table.sections.push(section.finish(state));
        }
    }

    /// Finishes the table: puts the header group first and the footer group
    /// last.
    pub(crate) fn finish(mut self, state: &mut BuildState) -> TableBox {
        self.close_section(state);
        let mut table = self.table;
        if let Some(mut header) = self.header {
            header.is_body = false;
            table.sections.insert(0, header);
        }
        if let Some(mut footer) = self.footer {
            footer.is_body = false;
            table.sections.push(footer);
        }
        table
    }
}

/// The box base of an element or pseudo-element item; `None` for text.
fn element_base(ctx: &BuildContext<'_>, item: Item<'_>, state: &mut BuildState) -> Option<BoxBase> {
    match item {
        Item::Element(node, style) => Some(state.numbered(ctx.element_base(node, style))),
        Item::Pseudo(base) => Some(base),
        Item::Text(..) | Item::Generated(..) => None,
    }
}

fn build_section(ctx: &BuildContext<'_>, base: &BoxBase, state: &mut BuildState) -> SectionBox {
    let mut builder = SectionBuilder::new(base.clone(), 0);
    match base.element() {
        Some(node) => for_each_child(ctx, node, state, &mut |item, state| {
            builder.push_item(ctx, item, state);
        }),
        None => {
            if let (Some(text), Some(node)) = (content_text(&base.style), base.node) {
                builder.push_item(ctx, Item::Generated(node, &base.style, &text), state);
            }
        }
    }
    builder.finish(state)
}

/// Collects the rows of a row group.
struct SectionBuilder {
    base: BoxBase,
    extra_depth: usize,
    rows: Vec<RowBox>,
    /// The anonymous row that is open.
    row: Option<RowBuilder>,
}

impl SectionBuilder {
    fn new(base: BoxBase, extra_depth: usize) -> Self {
        SectionBuilder {
            base,
            extra_depth,
            rows: Vec::new(),
            row: None,
        }
    }

    fn push_item(&mut self, ctx: &BuildContext<'_>, item: Item<'_>, state: &mut BuildState) {
        let extra = self.extra_depth;
        if item.kind(ctx) == Kind::Row {
            self.close_row(state);
            let Some(base) = element_base(ctx, item, state) else {
                return;
            };
            match deeper(state, extra, |state| build_row(ctx, &base, state)) {
                Some(row) => self.rows.push(row),
                None => {
                    if let Some(node) = base.element() {
                        self.push_flattened(ctx, node, state);
                    }
                }
            }
            return;
        }
        self.anonymous_row(state).push_item(ctx, item, state);
    }

    fn anonymous_row(&mut self, state: &mut BuildState) -> &mut RowBuilder {
        let style = &self.base.style;
        let extra = self.extra_depth;
        self.row.get_or_insert_with(|| {
            RowBuilder::new(state.anonymous(style, Display::TableRow), extra + 1)
        })
    }

    fn push_flattened(&mut self, ctx: &BuildContext<'_>, node: NodeId, state: &mut BuildState) {
        self.anonymous_row(state).push_flattened(ctx, node, state);
    }

    fn close_row(&mut self, state: &mut BuildState) {
        if let Some(row) = self.row.take() {
            self.rows.push(row.finish(state));
        }
    }

    fn finish(mut self, state: &mut BuildState) -> SectionBox {
        self.close_row(state);
        SectionBox {
            base: self.base,
            rows: self.rows,
            is_body: true,
        }
    }
}

fn build_row(ctx: &BuildContext<'_>, base: &BoxBase, state: &mut BuildState) -> RowBox {
    let mut builder = RowBuilder::new(base.clone(), 0);
    match base.element() {
        Some(node) => for_each_child(ctx, node, state, &mut |item, state| {
            builder.push_item(ctx, item, state);
        }),
        None => {
            if let (Some(text), Some(node)) = (content_text(&base.style), base.node) {
                builder.push_item(ctx, Item::Generated(node, &base.style, &text), state);
            }
        }
    }
    builder.finish(state)
}

/// Collects the cells of a row.
struct RowBuilder {
    base: BoxBase,
    extra_depth: usize,
    cells: Vec<CellBox>,
    /// The anonymous cell that is open, and its content.
    anonymous: Option<(BoxBase, ContainerBuilder)>,
}

impl RowBuilder {
    fn new(base: BoxBase, extra_depth: usize) -> Self {
        RowBuilder {
            base,
            extra_depth,
            cells: Vec::new(),
            anonymous: None,
        }
    }

    fn push_item(&mut self, ctx: &BuildContext<'_>, item: Item<'_>, state: &mut BuildState) {
        let extra = self.extra_depth;
        if item.kind(ctx) == Kind::Cell {
            self.close_cell(state);
            let Some(base) = element_base(ctx, item, state) else {
                return;
            };
            match deeper(state, extra, |state| build_cell(ctx, base.clone(), state)) {
                Some(cell) => self.cells.push(cell),
                None => {
                    if let Some(node) = base.element() {
                        self.push_flattened(ctx, node, state);
                    }
                }
            }
            return;
        }
        // Other content goes into an anonymous cell, one level deeper.
        let content = self.anonymous_content(state);
        state.depth += extra + 1;
        match item {
            Item::Element(node, _) => content.push_element(ctx, node, state),
            Item::Pseudo(base) => content.push_box(ctx, base, state),
            Item::Text(node, style, text) => content.push_text(node, style, text),
            Item::Generated(node, style, text) => content.push_generated(node, style, text),
        }
        state.depth -= extra + 1;
    }

    fn anonymous_content(&mut self, state: &mut BuildState) -> &mut ContainerBuilder {
        let style = &self.base.style;
        let (_, content) = self.anonymous.get_or_insert_with(|| {
            let base = state.anonymous(style, Display::TableCell);
            let content = ContainerBuilder::new(Arc::clone(&base.style));
            (base, content)
        });
        content
    }

    fn push_flattened(&mut self, ctx: &BuildContext<'_>, node: NodeId, state: &mut BuildState) {
        self.anonymous_content(state)
            .push_flattened(ctx, node, state);
    }

    fn close_cell(&mut self, state: &mut BuildState) {
        if let Some((base, content)) = self.anonymous.take() {
            let inner = IndependentBox {
                base,
                contents: crate::box_tree::IndependentContents::Flow(content.finish(state)),
                marker: None,
            };
            self.cells.push(CellBox {
                inner,
                colspan: 1,
                rowspan: 1,
                nowrap: false,
            });
        }
    }

    fn finish(mut self, state: &mut BuildState) -> RowBox {
        self.close_cell(state);
        RowBox {
            base: self.base,
            cells: self.cells,
        }
    }
}

fn build_cell(ctx: &BuildContext<'_>, base: BoxBase, state: &mut BuildState) -> CellBox {
    let element = base
        .element()
        .and_then(|node| ctx.doc.element(node))
        .filter(|e| e.is_html_named(&local_name!("td")) || e.is_html_named(&local_name!("th")));
    let (colspan, rowspan, nowrap) = element.map_or((1, 1, false), |e| {
        let colspan = e
            .attr("colspan")
            .and_then(parse_non_negative_integer)
            .filter(|&v| v > 0)
            .map_or(1, |v| v.min(MAX_COLSPAN));
        let rowspan = e
            .attr("rowspan")
            .and_then(parse_non_negative_integer)
            .map_or(1, |v| v.min(MAX_ROWSPAN));
        (colspan, rowspan, e.has_attr("nowrap"))
    });
    CellBox {
        inner: build_independent(ctx, base, state),
        colspan,
        rowspan,
        nowrap,
    }
}

/// Builds a column or a column group with its columns.
fn build_column(ctx: &BuildContext<'_>, base: BoxBase, state: &mut BuildState) -> ColumnBox {
    let is_group = base.style.display == Display::TableColumnGroup;
    let element = base.element();
    let span = element
        .and_then(|node| ctx.doc.element(node))
        .filter(|e| {
            e.is_html_named(&local_name!("col")) || e.is_html_named(&local_name!("colgroup"))
        })
        .and_then(|e| e.attr("span"))
        .and_then(parse_non_negative_integer)
        .filter(|&v| v > 0)
        .map_or(1, |v| v.min(MAX_COLSPAN));
    let mut children = Vec::new();
    if is_group && let Some(node) = element {
        for child in ctx.doc.element_children(node) {
            let Some(style) = ctx.styles.get(child) else {
                continue;
            };
            if style.display != Display::TableColumn {
                continue;
            }
            let child_base = state.numbered(ctx.element_base(child, style));
            if let Some(column) = deeper(state, 0, |state| build_column(ctx, child_base, state)) {
                children.push(column);
            }
        }
    }
    ColumnBox {
        base,
        span,
        is_group,
        children,
    }
}

/// The HTML rules for parsing non-negative integers (leading white space,
/// an optional `+`, digits; the rest is ignored), saturating.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-non-negative-integers>
fn parse_non_negative_integer(input: &str) -> Option<usize> {
    let input = input.trim_start_matches(swb_dom::is_html_whitespace);
    let input = input.strip_prefix('+').unwrap_or(input);
    let digits = input.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    Some(input[..digits].bytes().fold(0usize, |v, d| {
        v.saturating_mul(10).saturating_add(usize::from(d - b'0'))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_negative_integers() {
        assert_eq!(parse_non_negative_integer(" 12abc"), Some(12));
        assert_eq!(parse_non_negative_integer("+3"), Some(3));
        assert_eq!(parse_non_negative_integer("-3"), None);
        assert_eq!(parse_non_negative_integer(""), None);
        assert_eq!(
            parse_non_negative_integer("99999999999999999999999999"),
            Some(usize::MAX)
        );
    }
}
