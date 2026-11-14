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
//! its summary; SVG `text` is a block; `content-visibility: hidden` gives
//! nothing. Departures: the math elements (Chromium maps `mi` to
//! mathematical italic letters), and a part of a line (the `innerText` of an inline element
//! starts a line in swb; Chromium keeps a space at its start when the line
//! has text before it).

use swb_dom::{Document, FlatScopes, NodeData, NodeId, local_name, ns};
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
    if styles.get(element).is_some_and(|s| s.skips_contents()) {
        return Children::None;
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
    /// A node and its tree scope (the root of its tree: the document or a
    /// shadow root).
    Enter(NodeId, NodeId),
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
    let mut walk = Walk {
        scope: dom.root(root),
        scopes: FlatScopes::default(),
    };
    let mut stack = vec![Step::Enter(root, walk.scope)];
    while let Some(step) = stack.pop() {
        let (node, scope) = match step {
            Step::Enter(node, scope) => (node, scope),
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
                // The flat parent (ADR 0036 section 4): a text node that no
                // slot takes is not being rendered (measured, case
                // `unslotted-computed-style`: a host's `innerText` leaves
                // it out).
                let style = dom
                    .flat_parent(node)
                    .and_then(|p| styles.get(p).or_else(|| option_style(dom, styles, p)));
                if let Some(style) = style {
                    out.text(text, style, scope == walk.scope);
                }
            }
            NodeData::Element(_) => {
                let at = Place {
                    node,
                    scope,
                    root: node == root,
                };
                enter(dom, styles, at, &mut walk, &mut out, &mut stack);
            }
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

/// The shadow trees of the walk (ADR 0036 sub-feature 5b). Below a host,
/// the walk follows the flat tree, so that the white space of the light
/// text around shadow content collapses as layout collapses it; only the
/// text of the tree of the element whose text is asked for gives
/// characters (measured in Chromium 148, case `render-reads` of
/// `tools/probes/web-shadow-dom.json`: the `innerText` of a paragraph
/// leaves the shadow text of a host out and keeps the spaces on both sides
/// of it).
struct Walk {
    /// The tree scope of the element whose text is asked for.
    scope: NodeId,
    /// The tree scopes of the flat tree below the element.
    scopes: FlatScopes,
}

/// An element of the walk.
#[derive(Clone, Copy)]
struct Place {
    node: NodeId,
    /// Its tree scope.
    scope: NodeId,
    /// True for the element whose text is asked for.
    root: bool,
}

/// The steps of an element: its items before its children, its children
/// on the stack, and the items after them. The element whose text is
/// asked for (`root`) gives only the items of its children; an element
/// of another tree scope gives none.
fn enter(
    dom: &Document,
    styles: &StyleMap,
    at: Place,
    walk: &mut Walk,
    out: &mut Collector,
    stack: &mut Vec<Step>,
) {
    let Place { node, scope, root } = at;
    let option = is_option(dom, node) && styles.get(node).is_none();
    let Some(style) = styles.get(node).or_else(|| option_style(dom, styles, node)) else {
        return;
    };
    if style.display == Display::None {
        return;
    }
    // The contents of `content-visibility: hidden` are not rendered text, and
    // the element gives no line breaks (measured in Chromium 148,
    // `cv-hidden-inner-text`).
    if !root && style.skips_contents() {
        return;
    }
    let children = children_of(dom, styles, node);
    let visible = style.visibility == Visibility::Visible && !root && scope == walk.scope;
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
        Children::All => {
            let scope = walk.scopes.of_children(dom, node, scope);
            stack.extend(dom.flat_children(node).map(|c| Step::Enter(c, scope)));
        }
        Children::Options => stack.extend(
            dom.children(node)
                .filter(|&c| is_option(dom, c))
                .map(|c| Step::Enter(c, scope)),
        ),
        Children::Summary(summary) => stack.extend(summary.map(|c| Step::Enter(c, scope))),
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
    /// The waiting space comes from text of the tree whose text is asked
    /// for (not from the shadow text around it).
    pending_shown: bool,
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
            if self.pending_shown {
                self.items.push(Item::Text(" ".to_owned()));
            }
        }
        self.line_start = false;
    }

    /// The text of a text node with the style of its parent. A text that is
    /// not visible (or not `shown`: in another tree scope) changes the
    /// white-space state but gives no characters.
    fn text(&mut self, data: &str, style: &ComputedStyle, shown: bool) {
        let visible = shown && style.visibility == Visibility::Visible;
        // A collapsible space before the text of another tree belongs to
        // the light text, and layout keeps it (case `render-reads`: the
        // paragraph's text is "by  end"); a space of the other tree gives
        // nothing.
        let keeps_space = |pending_shown: bool| pending_shown && (visible || !shown);
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
                        self.pending_shown = shown;
                    }
                    continue;
                }
                if self.pending_space {
                    self.pending_space = false;
                    if keeps_space(self.pending_shown) {
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
            if self.pending_space && keeps_space(self.pending_shown) {
                text.push(' ');
            }
            if visible {
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
