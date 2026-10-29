//! The rendered text of an element for `innerText` and `outerText` (HTML
//! §3.2.7, "rendered text collection steps",
//! <https://html.spec.whatwg.org/multipage/dom.html#rendered-text-collection-steps>;
//! M8 feature 7b2).
//!
//! The walk follows the specification's list of items: strings, and
//! "required line break counts" (1 at the edges of a block-level box or a
//! table caption, 2 at the edges of a `p`), merged at the end (leading and
//! trailing counts go, a run of counts becomes as many line feeds as its
//! largest count). A `br` is a line feed, a table cell that is not the
//! last of its row adds a tab, a row that is not the last of its table a
//! line feed; these strings separate runs of counts.
//!
//! "Being rendered" comes from the styles, not from boxes: an element has
//! a box when its style and its ancestors' styles are not `display: none`
//! and no ancestor is a replaced element or a closed `details` that hides
//! it. So a query needs a style flush, not a layout. The text of a text
//! node is white-space processed as an inline formatting context does it
//! (collapsible spaces at the start of a line go, a space before a line
//! break or the end of the block goes, `text-transform` applies as layout
//! applies it); soft wraps do not matter, since a collapsible space at a
//! soft wrap stays in `innerText`.
//!
//! Measured in Chromium 148 (`tools/probes/web-inner-text.json`): floats,
//! absolutely positioned boxes and flex and grid items are blocks (their
//! computed `display` is blockified); an inline-block keeps the line breaks
//! of its blocks; `select` lists its options on lines of their own; the
//! children of `textarea`, `input`, `img`, `canvas`, `video`, `audio`,
//! `iframe`, `progress` and `meter` give nothing; a closed `details` gives
//! its summary; SVG `text` is a block. Departures: `content-visibility`
//! (swb has none), the math elements (Chromium maps `mi` to mathematical italic
//! letters), and a part of a line (the `innerText` of an inline element
//! starts a line in swb; Chromium keeps a space at its start when the line
//! has text before it).

use swb_dom::{Document, NodeData, NodeId, local_name, ns};
use swb_style::{ComputedStyle, Display, StyleMap, TextTransform, Visibility};

use super::DocumentState;

impl DocumentState {
    /// The rendered text of `element` after a style flush (a forced run in
    /// a script task); `None` if it is not being rendered or not in the
    /// document.
    pub(crate) fn script_inner_text(&mut self, element: NodeId) -> Option<String> {
        self.process_changes();
        if !self.dom.is_connected(element) {
            return None;
        }
        let outer = std::mem::replace(&mut self.in_query, true);
        self.update_style();
        self.in_query = outer;
        let styles = self.styles.as_ref()?;
        if !being_rendered(&self.dom, styles, element) {
            return None;
        }
        Some(rendered_text(&self.dom, styles, element))
    }
}

/// True if `element` has a box: it has a style that is not `display:
/// none` (descendants of such elements have none), and no ancestor hides
/// its children.
fn being_rendered(dom: &Document, styles: &StyleMap, element: NodeId) -> bool {
    if styles
        .get(element)
        .is_none_or(|s| s.display == Display::None)
    {
        return false;
    }
    let mut child = element;
    for ancestor in dom.ancestors(element) {
        match children_of(dom, styles, ancestor) {
            Children::None => return false,
            Children::Options if !is_option(dom, child) => return false,
            Children::Summary(summary) if Some(child) != summary => return false,
            _ => {}
        }
        child = ancestor;
    }
    true
}

/// Which children of an element are rendered.
enum Children {
    All,
    /// None: a replaced element or a control.
    None,
    /// The `option` and `optgroup` children (`select`).
    Options,
    /// Only the summary (a closed `details`).
    Summary(Option<NodeId>),
}

fn children_of(dom: &Document, styles: &StyleMap, element: NodeId) -> Children {
    let Some(e) = dom.element(element) else {
        return Children::All;
    };
    if e.name.ns != ns!(html) {
        return Children::All;
    }
    match &*e.name.local {
        "input" | "textarea" | "img" | "canvas" | "video" | "audio" | "iframe" | "embed"
        | "progress" | "meter" => Children::None,
        "select" => Children::Options,
        "details"
            if styles
                .pseudo(element, swb_style::PseudoKind::DetailsContent)
                .is_some_and(|s| s.contents_hidden) =>
        {
            Children::Summary(dom.details_summary(element))
        }
        _ => Children::All,
    }
}

fn is_option(dom: &Document, node: NodeId) -> bool {
    dom.is_html_element(node, &local_name!("option"))
        || dom.is_html_element(node, &local_name!("optgroup"))
}

/// A step of the walk.
enum Step {
    Enter(NodeId),
    /// The end of an element: the item that follows its children (a tab,
    /// a line feed), its required line break count, and whether it is an
    /// atomic inline whose content is a line of its own.
    Leave {
        after: Option<&'static str>,
        breaks: u8,
        atomic: bool,
    },
}

/// HTML "the innerText getter steps" for a rendered `root`.
fn rendered_text(dom: &Document, styles: &StyleMap, root: NodeId) -> String {
    // The text starts a line.
    let mut out = Collector {
        line_start: true,
        ..Collector::default()
    };
    let mut stack = vec![Step::Enter(root)];
    while let Some(step) = stack.pop() {
        let node = match step {
            Step::Enter(node) => node,
            Step::Leave {
                after,
                breaks,
                atomic,
            } => {
                if atomic {
                    out.atomic_end();
                }
                if let Some(text) = after {
                    out.forced(text);
                }
                out.breaks(breaks);
                continue;
            }
        };
        match &dom.node(node).data {
            NodeData::Text(text) => {
                let style = dom
                    .parent(node)
                    .and_then(|p| styles.get(p).or_else(|| option_style(dom, styles, p)));
                if let Some(style) = style {
                    out.text(text, style);
                }
            }
            NodeData::Element(_) => enter(dom, styles, node, node == root, &mut out, &mut stack),
            _ => {}
        }
    }
    out.finish()
}

/// The style of an `option` or `optgroup` that has none of its own (the
/// options of a drop-down `select` get no boxes): the `select`'s.
fn option_style<'a>(
    dom: &Document,
    styles: &'a StyleMap,
    element: NodeId,
) -> Option<&'a std::sync::Arc<ComputedStyle>> {
    if !is_option(dom, element) {
        return None;
    }
    dom.ancestors(element)
        .find(|&a| dom.is_html_element(a, &local_name!("select")))
        .and_then(|select| styles.get(select))
}

/// The steps of an element: its items before its children, its children
/// on the stack, and the items after them. The element whose text is
/// asked for (`root`) gives only the items of its children.
fn enter(
    dom: &Document,
    styles: &StyleMap,
    node: NodeId,
    root: bool,
    out: &mut Collector,
    stack: &mut Vec<Step>,
) {
    let option = is_option(dom, node) && styles.get(node).is_none();
    let Some(style) = styles.get(node).or_else(|| option_style(dom, styles, node)) else {
        return;
    };
    if style.display == Display::None {
        return;
    }
    let children = children_of(dom, styles, node);
    let visible = style.visibility == Visibility::Visible && !root;
    let has_box = style.display != Display::Contents && !root;
    let breaks = if !visible || !has_box {
        0
    } else if dom.is_html_element(node, &local_name!("p")) {
        2
    } else {
        u8::from(option || is_block_level(dom, node, style))
    };
    out.breaks(breaks);
    if visible && dom.is_html_element(node, &local_name!("br")) {
        out.forced("\n");
    }
    let atomic = has_box && is_atomic_inline(style, &children);
    if atomic {
        out.atomic_start(matches!(children, Children::None));
    }
    let after = if !visible {
        None
    } else if style.display == Display::TableCell && has_next(dom, styles, node, Display::TableCell)
    {
        Some("\t")
    } else if style.display == Display::TableRow && has_next_row(dom, styles, node) {
        Some("\n")
    } else {
        None
    };
    stack.push(Step::Leave {
        after,
        breaks,
        atomic: atomic && !matches!(children, Children::None),
    });
    let first = stack.len();
    match children {
        Children::None => {}
        Children::All => stack.extend(dom.children(node).map(Step::Enter)),
        Children::Options => stack.extend(
            dom.children(node)
                .filter(|&c| is_option(dom, c))
                .map(Step::Enter),
        ),
        Children::Summary(summary) => stack.extend(summary.map(Step::Enter)),
    }
    stack[first..].reverse();
}

/// True for the boxes whose edges are line breaks: block-level boxes, table
/// captions and SVG `text` (a block in Chromium).
fn is_block_level(dom: &Document, node: NodeId, style: &ComputedStyle) -> bool {
    if dom
        .element(node)
        .is_some_and(|e| e.name.ns == ns!(svg) && &*e.name.local == "text")
    {
        return true;
    }
    match style.display {
        Display::TableCaption => true,
        d => !d.is_inline_level() && !d.is_table_internal() && d != Display::Contents,
    }
}

/// True for an inline-level box that is not a part of the line's text:
/// an inline-block, inline flex, grid or table, or a replaced element.
fn is_atomic_inline(style: &ComputedStyle, children: &Children) -> bool {
    style.display.is_inline_level()
        && (style.display != Display::Inline || matches!(children, Children::None))
}

/// True if a rendered sibling element with display `display` follows
/// `node`.
fn has_next(dom: &Document, styles: &StyleMap, node: NodeId, display: Display) -> bool {
    std::iter::successors(dom.next_sibling_element(node), |&n| {
        dom.next_sibling_element(n)
    })
    .any(|n| styles.get(n).is_some_and(|s| s.display == display))
}

/// True if another rendered row of the table follows the row `node`: a
/// sibling, or a row of a following row group.
fn has_next_row(dom: &Document, styles: &StyleMap, node: NodeId) -> bool {
    if has_next(dom, styles, node, Display::TableRow) {
        return true;
    }
    let Some(group) = dom.parent(node) else {
        return false;
    };
    let is_group = |n: NodeId| {
        styles.get(n).is_some_and(|s| {
            matches!(
                s.display,
                Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
            )
        })
    };
    if !is_group(group) {
        return false;
    }
    std::iter::successors(dom.next_sibling_element(group), |&n| {
        dom.next_sibling_element(n)
    })
    .any(|n| {
        styles
            .get(n)
            .is_some_and(|s| s.display == Display::TableRow)
            || (is_group(n)
                && dom.element_children(n).any(|r| {
                    styles
                        .get(r)
                        .is_some_and(|s| s.display == Display::TableRow)
                }))
    })
}

/// An item of the list.
enum Item {
    Text(String),
    /// A required line break count.
    Breaks(u8),
}

/// Collects the items, with the white-space state of the current line.
#[derive(Default)]
struct Collector {
    items: Vec<Item>,
    /// A collapsible space waits for the next text of the line.
    pending_space: bool,
    /// True at the start of a line: collapsible spaces go.
    line_start: bool,
    /// The last character of the line so far (`text-transform:
    /// capitalize` continues a word across elements).
    previous: Option<char>,
}

impl Collector {
    fn new_line(&mut self) {
        self.pending_space = false;
        self.line_start = true;
        self.previous = None;
    }

    /// A required line break count (0 is nothing).
    fn breaks(&mut self, count: u8) {
        if count > 0 {
            self.new_line();
            self.items.push(Item::Breaks(count));
        }
    }

    /// A string that ends the line: the line feed of a `br` or of a row,
    /// the tab of a cell.
    fn forced(&mut self, text: &'static str) {
        self.new_line();
        self.items.push(Item::Text(text.to_owned()));
    }

    /// The start of an atomic inline: the space before it stays; a box
    /// with content starts a line of its own inside.
    fn atomic_start(&mut self, empty: bool) {
        self.flush_space();
        self.line_start = !empty;
        if empty {
            self.previous = None;
        }
    }

    fn atomic_end(&mut self) {
        self.pending_space = false;
        self.line_start = false;
        self.previous = None;
    }

    fn flush_space(&mut self) {
        if self.pending_space {
            self.pending_space = false;
            self.items.push(Item::Text(" ".to_owned()));
        }
        self.line_start = false;
    }

    /// The text of a text node with the style of its parent. A text that is
    /// not visible changes the white-space state but gives no characters.
    fn text(&mut self, data: &str, style: &ComputedStyle) {
        let visible = style.visibility == Visibility::Visible;
        let mut text = String::new();
        if style.white_space.collapses_spaces() {
            let pre_line = style.white_space == swb_style::WhiteSpace::PreLine;
            for c in data.chars() {
                if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                    if pre_line && c == '\n' {
                        self.pending_space = false;
                        if visible {
                            text.push('\n');
                        }
                        self.line_start = true;
                    } else if !self.line_start {
                        self.pending_space = true;
                    }
                    continue;
                }
                if self.pending_space {
                    self.pending_space = false;
                    if visible {
                        text.push(' ');
                    }
                }
                if visible {
                    text.push(c);
                }
                self.line_start = false;
            }
        } else {
            if data.is_empty() {
                return;
            }
            if visible {
                if self.pending_space {
                    text.push(' ');
                }
                text.push_str(data);
            }
            self.pending_space = false;
            self.line_start = data.ends_with('\n');
        }
        if text.is_empty() {
            return;
        }
        let text = if style.text_transform == TextTransform::None {
            text
        } else {
            swb_layout::transform_text(&text, style.text_transform, self.previous)
        };
        self.previous = text.chars().next_back();
        self.items.push(Item::Text(text));
    }

    /// The items joined: leading and trailing required line break counts
    /// go, a run of them becomes its largest count of line feeds.
    fn finish(self) -> String {
        let mut out = String::new();
        let mut run: Option<u8> = None;
        for item in self.items {
            match item {
                Item::Breaks(count) => {
                    if !out.is_empty() {
                        run = Some(run.map_or(count, |r| r.max(count)));
                    }
                }
                Item::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(count) = run.take() {
                        out.extend(std::iter::repeat_n('\n', usize::from(count)));
                    }
                    out.push_str(&text);
                }
            }
        }
        out
    }
}
