//! CSS counters and list item numbers.
//!
//! [`resolve`] runs at the end of style computation. It visits the
//! elements and pseudo-elements that generate boxes in the order in which
//! Chromium builds its layout tree (an element, its `::marker`, its
//! `::before`, its children, its `::after`) and:
//!
//! - applies `counter-reset`, `counter-increment` and `counter-set` and
//!   the implicit `list-item` counter, and replaces `counter()` and
//!   `counters()` in the `content` of pseudo-elements with their text
//!   (<https://www.w3.org/TR/css-lists-3/#auto-numbering>);
//! - computes the ordinal value of each list item, which layout shows in
//!   markers with `content: normal`
//!   (<https://html.spec.whatwg.org/multipage/grouping-content.html#ordinal-value>).
//!
//! Elements with `display: none` (and their subtrees) and the content of
//! replaced elements and form controls do not take part, and elements
//! with an atomic box are not list items (`element_kinds.rs`).
//!
//! # Chromium compatibility
//!
//! Where Chromium 148 differs from CSS Lists 3, swb follows Chromium
//! (measured with Chromium's DOM snapshots; Blink's
//! `CountersAttachmentContext` and `ListItemOrdinal`, BSD-3, were read
//! for ideas to explain the measurements):
//!
//! - A `counter-reset` on an element that inherits a counter of the same
//!   name from an ancestor's scope creates a counter whose scope is the
//!   element and its descendants only. In the specification the scope
//!   also includes the following siblings. A reset that replaces the
//!   counter of a previous sibling, or that creates the first counter of
//!   its name in scope, has the specification's scope.
//! - Elements and pseudo-elements with `display: contents` do not change
//!   counters; the children and pseudo-elements of such an element do.
//!   `::marker` and `wbr` ignore the counter properties.
//! - `counter()` and `counters()` of a counter that does not exist give
//!   0 and do not instantiate it.
//! - An increment that would overflow an existing counter is ignored.
//!   The value of one element (its reset plus its increments) saturates.
//! - The `list-item` counter: HTML `li` list items increment it (by -1 in
//!   a reversed `ol`), and HTML `ol`, `ul`, `menu` and `dir` that are not
//!   list items reset it, unless their counter properties name
//!   `list-item`. Other list items do not increment it. An `ol` resets it
//!   to `start - 1` (`start + 1` when reversed); without `start`, to 0,
//!   or to 1 when reversed, and `value` does not change it. The computed
//!   counter properties show none of this (Chromium has no presentational
//!   hints for them), and `reversed()` is not supported.
//! - The reset to 1 of a reversed list without `start`, and that `value`
//!   does not change the counter, are what Chromium shows after the first
//!   layout of a page: a timing artefact. After a later change that makes
//!   Chromium update counters (measured: inserting an element whose
//!   `::before` uses `counter()`, setting `counter-increment` on an
//!   element, or toggling `display` on `body`; inserting a plain `li` is
//!   not enough), all lists of the page change: a reversed list
//!   without `start` resets the counter to its number of items plus 1 (so
//!   the first item shows the number of items), and an `li` with `value`
//!   gets that value in a new counter nested in the list's, so the next
//!   item continues from the list's counter (`value=20`, then 1). swb runs
//!   no scripts, so it keeps the values of the first layout.
//! - Markers with `content: normal` do not use the `list-item` counter
//!   but the ordinal value: the ordinal of the previous list item of the
//!   same list plus 1 (-1 in a reversed `ol`), or for the first item the
//!   `ol`'s start (a reversed `ol` without `start` starts at its number
//!   of list items). On the item, `counter-set: list-item` sets the
//!   value, `counter-increment: list-item` sets the step, and the `value`
//!   attribute sets the value. The list of an item is its nearest `ol`,
//!   `ul` or `menu` ancestor that has a box, else its parent.
//!   `::before` and `::after` with `display: list-item` take numbers too
//!   (layout does not draw their markers).
//!
//! Not supported: style containment (`contain: style`, also in `content`
//! and `strict`), which in Chromium limits the scope of counters and
//! makes the element a list owner.
//!
//! # Cost
//!
//! Each counter name has a stack of the counters in scope, and each tree
//! depth a list of the counters whose scope ends with the element at that
//! depth, so the work is linear in the number of elements and counter
//! operations. The counter properties of an element are combined per
//! name once per distinct set of counter lists (elements that match the
//! same rules share their lists), so the names are not hashed for every
//! element. The item counts of reversed lists come from one more walk
//! over the document, made only if a reversed list without `start` has
//! an item. The parser limits each counter property to 256 counters, the
//! tree depth limits the nesting of counters with the same name, and the
//! counter text of one document is limited to [`MAX_COUNTER_TEXT`] (each
//! value counts at least one byte). Each run of counters and short
//! strings in a `content` value becomes one string, so the allocations
//! that the result keeps do not grow with the number of counters; after
//! the limit, counters are removed without text. The directive cache has
//! at most 1024 entries.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use log::warn;
use swb_dom::{Document, ElementData, NodeId, local_name};

use crate::ComputedStyle;
use crate::counter_style::counter_text;
use crate::element_kinds;
use crate::hints::parse_integer;
use crate::style_map::{PseudoKind, StyleMap};
use crate::values::{Content, ContentItem, CounterList, Display, ListStyleType};

/// The maximum length in bytes of the text of all `counter()` and
/// `counters()` items of a document (a `counters()` with a long separator
/// in deeply nested scopes could otherwise produce gigabytes). Counter
/// text beyond it is dropped.
const MAX_COUNTER_TEXT: usize = 4 << 20;

/// Strings in a `content` value up to this length (in bytes) are copied
/// into the text of the counters next to them; longer strings stay
/// shared. A copied string needs no more memory than the item it
/// replaces.
const MAX_MERGED_STRING: usize = size_of::<ContentItem>();

/// The maximum number of entries of the directive cache. A page with a
/// different set of counter lists on every element would otherwise keep
/// a copy of all of them; the cache is emptied when it is full.
const MAX_CACHED_DIRECTIVES: usize = 1024;

/// The implicit counter of list items.
const LIST_ITEM: &str = "list-item";

/// The index of [`LIST_ITEM`] among the counter names.
const LIST_ITEM_INDEX: usize = 0;

/// Resolves the counters of the document: replaces `counter()` and
/// `counters()` in the `content` of the pseudo-elements in `map` with
/// their text and stores the ordinal value of each list item.
pub(crate) fn resolve(doc: &Document, map: &mut StyleMap) {
    let Some(root) = doc.document_element() else {
        return;
    };
    let mut pass = Pass {
        doc,
        counters: Counters::new(),
        lists: Lists::default(),
    };
    // The root element has depth 1; the document is depth 0.
    let mut stack = vec![Visit::Enter(root, 1)];
    let mut children = Vec::new();
    while let Some(visit) = stack.pop() {
        match visit {
            Visit::Enter(node, depth) => {
                let Some(style) = map.get(node).map(Arc::clone) else {
                    continue;
                };
                if style.display == Display::None {
                    continue;
                }
                pass.enter(node, &style, depth, map);
                stack.push(Visit::Leave(node, depth));
                if renders_children(doc.element(node)) {
                    children.clear();
                    children.extend(doc.element_children(node));
                    stack.extend(children.iter().rev().map(|&c| Visit::Enter(c, depth + 1)));
                }
            }
            Visit::Leave(node, depth) => pass.leave(node, depth, map),
        }
    }
}

/// A step of a walk over the elements, with data `T` (the depth in the
/// counter walk).
#[derive(Clone, Copy)]
enum Visit<T> {
    Enter(NodeId, T),
    Leave(NodeId, T),
}

/// False for elements whose element children generate no boxes
/// ([`element_kinds::renders_children`]).
fn renders_children(element: Option<&ElementData>) -> bool {
    element.is_none_or(element_kinds::renders_children)
}

/// True for a list item: an element with `display: list-item` that can
/// be one ([`element_kinds::cannot_be_list_item`]). Layout draws markers
/// only for list items.
fn is_list_item(element: Option<&ElementData>, style: &ComputedStyle) -> bool {
    style.display == Display::ListItem && !element.is_some_and(element_kinds::cannot_be_list_item)
}

/// True for the elements that own the list items inside them (`ol`, `ul`
/// and `menu`; the caller checks that the element has a box).
fn is_list_owner(element: Option<&ElementData>) -> bool {
    element.is_some_and(|e| {
        e.is_html()
            && matches!(
                e.local_name(),
                &local_name!("ol") | &local_name!("ul") | &local_name!("menu")
            )
    })
}

/// An integer attribute (`start`, `value`) by the HTML rules for parsing
/// integers; values outside the `i32` range are errors, as in Chromium.
fn integer_attribute(element: &ElementData, name: &str) -> Option<i32> {
    let value = parse_integer(element.attr(name)?)?;
    i32::try_from(value).ok()
}

/// Saturates a value to the `i32` range.
fn saturate(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// The counter walk over the document.
struct Pass<'a> {
    doc: &'a Document,
    counters: Counters,
    lists: Lists,
}

impl Pass<'_> {
    /// Visits element `node` (not `display: none`) before its children.
    fn enter(&mut self, node: NodeId, style: &ComputedStyle, depth: usize, map: &mut StyleMap) {
        let element = self.doc.element(node);
        let mut list = None;
        if is_list_item(element, style) {
            let parent = self.doc.parent(node).unwrap_or(NodeId::DOCUMENT);
            let enclosing = self.lists.enclosing(parent);
            let ordinal = self.lists.ordinal(self.doc, map, element, style, enclosing);
            map.set_list_item_ordinal(node, ordinal);
            list = Some(enclosing);
        }
        if style.display != Display::Contents {
            if is_list_owner(element) {
                self.lists.owners.push((depth, node));
            }
            let names_list_item = !element.is_some_and(element_kinds::ignores_counter_properties)
                && self.counters.apply(style, depth);
            if !names_list_item && let Some(e) = element {
                self.implicit_list_item(e, style, list, depth);
            }
        }
        if let Some(marker) = map.pseudo_mut(node, PseudoKind::Marker) {
            self.counters.resolve_content(marker);
        }
        self.pseudo(node, PseudoKind::Before, depth + 1, map);
    }

    /// Visits element `node` after its children.
    fn leave(&mut self, node: NodeId, depth: usize, map: &mut StyleMap) {
        self.pseudo(node, PseudoKind::After, depth + 1, map);
        self.counters.leave(depth);
        if self.lists.owners.last() == Some(&(depth, node)) {
            self.lists.owners.pop();
        }
    }

    /// Visits the `::before` or `::after` of `node`, if it is generated.
    fn pseudo(&mut self, node: NodeId, kind: PseudoKind, depth: usize, map: &mut StyleMap) {
        let Some(display) = map.pseudo(node, kind).map(|s| s.display) else {
            return;
        };
        if display == Display::ListItem {
            // A pseudo-element list item takes a number in its list, as in
            // Chromium (layout does not draw markers of pseudo-elements).
            if let Some(style) = map.pseudo(node, kind).map(Arc::clone) {
                let list = self.lists.enclosing(node);
                self.lists.ordinal(self.doc, map, None, &style, list);
            }
        }
        let Some(style) = map.pseudo_mut(node, kind) else {
            return;
        };
        // With `display: contents`, the pseudo-element has no box of its
        // own, and Chromium ignores its counter properties.
        if display != Display::Contents {
            self.counters.apply(style, depth);
        }
        self.counters.resolve_content(style);
        self.counters.leave(depth);
    }

    /// The implicit `list-item` counter of an element whose counter
    /// properties do not name it (see the module documentation). `list`
    /// is the list of a list item.
    fn implicit_list_item(
        &mut self,
        element: &ElementData,
        style: &ComputedStyle,
        list: Option<NodeId>,
        depth: usize,
    ) {
        if !element.is_html() {
            return;
        }
        let name = element.local_name();
        let generates_counter = matches!(
            name,
            &local_name!("li")
                | &local_name!("ol")
                | &local_name!("ul")
                | &local_name!("menu")
                | &local_name!("dir")
        );
        if !generates_counter {
            return;
        }
        if style.display == Display::ListItem {
            let reversed = list.is_some_and(|l| reversed_ol(self.doc, l));
            let step = if reversed { -1 } else { 1 };
            self.counters
                .update(LIST_ITEM_INDEX, Change::Add(step), depth);
        } else if *name == local_name!("ol") {
            let reversed = element.has_attr("reversed");
            // Without `start`, the initial value is 1, or 0 for a
            // reversed list (Chromium's first layout; see the module
            // documentation).
            let initial = match integer_attribute(element, "start") {
                Some(start) => i64::from(start),
                None if reversed => 0,
                None => 1,
            };
            let value = saturate(initial + if reversed { 1 } else { -1 });
            self.counters.reset(LIST_ITEM_INDEX, value, depth);
        } else if *name != local_name!("li") {
            self.counters.reset(LIST_ITEM_INDEX, 0, depth);
        }
    }
}

/// True if `node` is an HTML `ol` with the `reversed` attribute.
fn reversed_ol(doc: &Document, node: NodeId) -> bool {
    doc.element(node)
        .is_some_and(|e| e.is_html_named(&local_name!("ol")) && e.has_attr("reversed"))
}

/// The counter properties of one element for one counter name, combined
/// as CSS Lists 3 §4.1 and §4.2 say: of several `counter-reset` and
/// `counter-set` values only the last one is honored, increments
/// compound, and a set value replaces the reset value plus the increment
/// (<https://www.w3.org/TR/css-lists-3/#counter-reset>). Sums saturate,
/// as measured in Chromium.
#[derive(Clone, Copy, Debug, Default)]
struct Directive {
    /// The last `counter-reset` value.
    reset: Option<i32>,
    /// The sum of the `counter-increment` values (saturated).
    increment: Option<i32>,
    /// The last `counter-set` value.
    set: Option<i32>,
}

impl Directive {
    /// Adds one value of a counter property.
    fn add(&mut self, property: Property, value: i32) {
        match property {
            Property::Reset => self.reset = Some(value),
            Property::Increment => {
                self.increment = Some(self.increment.unwrap_or(0).saturating_add(value));
            }
            Property::Set => self.set = Some(value),
        }
    }

    /// The set value, else the reset value plus the increment.
    fn combined(self) -> i32 {
        match self.set {
            Some(value) => value,
            None => saturate(
                i64::from(self.reset.unwrap_or(0)) + i64::from(self.increment.unwrap_or(0)),
            ),
        }
    }

    /// The directive of counter `name` in `style`.
    fn for_name(style: &ComputedStyle, name: &str) -> Directive {
        let mut d = Directive::default();
        for (_, property, value) in counter_values(style).filter(|&(n, ..)| &**n == name) {
            d.add(property, value);
        }
        d
    }
}

/// A counter property.
#[derive(Clone, Copy, Debug)]
enum Property {
    Reset,
    Increment,
    Set,
}

/// The values of the counter properties of `style`: names, properties
/// and integers.
fn counter_values(style: &ComputedStyle) -> impl Iterator<Item = (&Arc<str>, Property, i32)> {
    let reset = style
        .counter_reset
        .iter()
        .map(|(n, v)| (n, Property::Reset, v));
    let increment = style
        .counter_increment
        .iter()
        .map(|(n, v)| (n, Property::Increment, v));
    let set = style.counter_set.iter().map(|(n, v)| (n, Property::Set, v));
    reset.chain(increment).chain(set)
}

/// A change of the innermost counter of a name.
#[derive(Clone, Copy, Debug)]
enum Change {
    Set(i32),
    Add(i32),
}

/// A counter in scope.
#[derive(Clone, Copy, Debug)]
struct Entry {
    value: i32,
    /// The depth of the element (or pseudo-element) that created it.
    creator_depth: usize,
    /// The depth of the element whose end ends its scope: the creator's
    /// parent, or the creator itself for a counter that is nested in a
    /// counter of an ancestor's scope.
    owner_depth: usize,
}

/// The CSS counters in scope at the current position of the walk.
struct Counters {
    /// Counter names and their indices.
    names: HashMap<Arc<str>, usize>,
    /// For each name, the counters in scope, outermost first.
    stacks: Vec<Vec<Entry>>,
    /// For each depth, the names of the counters whose scope ends with
    /// the element at that depth.
    scopes: Vec<Vec<usize>>,
    /// The directives of the element being visited.
    scratch: Vec<(usize, Directive)>,
    /// For each name, its position in `scratch` (`usize::MAX` if none).
    slots: Vec<usize>,
    /// The combined directives of each combination of counter lists, by
    /// the addresses of the lists. Elements that match the same rules share
    /// their lists, so the names of a list are looked up once, not for
    /// each element.
    cache: HashMap<[usize; 3], CachedDirectives>,
    /// The counter text that the document may still produce.
    text_left: usize,
}

/// The combined directives of a combination of counter lists, with name
/// indices.
struct CachedDirectives {
    /// Copies of the lists, so that their addresses stay valid (and
    /// unique) while the cache exists.
    _lists: [CounterList; 3],
    directives: Rc<[(usize, Directive)]>,
}

impl Counters {
    fn new() -> Self {
        let mut counters = Counters {
            names: HashMap::new(),
            stacks: Vec::new(),
            scopes: Vec::new(),
            scratch: Vec::new(),
            slots: Vec::new(),
            cache: HashMap::new(),
            text_left: MAX_COUNTER_TEXT,
        };
        let index = counters.intern(&Arc::from(LIST_ITEM));
        debug_assert_eq!(index, LIST_ITEM_INDEX);
        counters
    }

    /// The index of a counter name.
    fn intern(&mut self, name: &Arc<str>) -> usize {
        if let Some(&index) = self.names.get(name) {
            return index;
        }
        let index = self.stacks.len();
        self.names.insert(Arc::clone(name), index);
        self.stacks.push(Vec::new());
        self.slots.push(usize::MAX);
        index
    }

    /// Applies the counter properties of the element or pseudo-element
    /// with `style` at `depth`. Returns true if they name `list-item`.
    fn apply(&mut self, style: &ComputedStyle, depth: usize) -> bool {
        if style.counter_reset.is_empty()
            && style.counter_increment.is_empty()
            && style.counter_set.is_empty()
        {
            return false;
        }
        let directives = self.directives(style);
        let mut names_list_item = false;
        for &(name, d) in directives.iter() {
            names_list_item |= name == LIST_ITEM_INDEX;
            if d.reset.is_some() {
                self.reset(name, d.combined(), depth);
            } else if let Some(value) = d.set {
                self.update(name, Change::Set(value), depth);
            } else if let Some(value) = d.increment {
                self.update(name, Change::Add(value), depth);
            }
        }
        names_list_item
    }

    /// The counter properties of `style` combined per name, from the cache
    /// when the same lists were seen before.
    fn directives(&mut self, style: &ComputedStyle) -> Rc<[(usize, Directive)]> {
        let lists = [
            &style.counter_reset,
            &style.counter_increment,
            &style.counter_set,
        ];
        let key = lists.map(CounterList::address);
        if let Some(cached) = self.cache.get(&key) {
            return Rc::clone(&cached.directives);
        }
        self.scratch.clear();
        for (name, property, value) in counter_values(style) {
            self.directive(name).add(property, value);
        }
        for &(name, _) in &self.scratch {
            self.slots[name] = usize::MAX;
        }
        let directives: Rc<[(usize, Directive)]> = Rc::from(self.scratch.as_slice());
        let cached = CachedDirectives {
            _lists: lists.map(CounterList::clone),
            directives: Rc::clone(&directives),
        };
        if self.cache.len() >= MAX_CACHED_DIRECTIVES {
            self.cache.clear();
        }
        self.cache.insert(key, cached);
        directives
    }

    /// The directive of `name` in `scratch`, added if missing.
    fn directive(&mut self, name: &Arc<str>) -> &mut Directive {
        let index = self.intern(name);
        let mut slot = self.slots[index];
        if slot == usize::MAX {
            slot = self.scratch.len();
            self.scratch.push((index, Directive::default()));
            self.slots[index] = slot;
        }
        &mut self.scratch[slot].1
    }

    /// `counter-reset` of counter `name` to `value` by the element at
    /// `depth`.
    fn reset(&mut self, name: usize, value: i32, depth: usize) {
        match self.stacks[name].last_mut() {
            // The counter of a previous sibling (or of the parent's
            // `::before`): the new counter replaces it.
            Some(top) if top.creator_depth == depth => top.value = value,
            // A counter of an ancestor's scope: the new counter is nested
            // in it, and (as in Chromium) its scope ends with the element.
            Some(_) => self.push(name, value, depth, depth),
            None => self.push(name, value, depth, depth - 1),
        }
    }

    /// `counter-set` or `counter-increment` of counter `name` by the
    /// element at `depth`. Without a counter of that name, the element
    /// instantiates one with the value 0 first.
    fn update(&mut self, name: usize, change: Change, depth: usize) {
        if let Some(top) = self.stacks[name].last_mut() {
            top.value = match change {
                Change::Set(value) => value,
                Change::Add(delta) => top.value.checked_add(delta).unwrap_or(top.value),
            };
        } else {
            let (Change::Set(value) | Change::Add(value)) = change;
            self.push(name, value, depth, depth - 1);
        }
    }

    fn push(&mut self, name: usize, value: i32, creator_depth: usize, owner_depth: usize) {
        self.stacks[name].push(Entry {
            value,
            creator_depth,
            owner_depth,
        });
        if self.scopes.len() <= owner_depth {
            self.scopes.resize_with(owner_depth + 1, Vec::new);
        }
        self.scopes[owner_depth].push(name);
    }

    /// Ends the scope of the counters owned by the element at `depth`.
    fn leave(&mut self, depth: usize) {
        let Some(names) = self.scopes.get_mut(depth) else {
            return;
        };
        for name in names.drain(..).rev() {
            let entry = self.stacks[name].pop();
            debug_assert!(entry.is_some_and(|e| e.owner_depth == depth));
        }
    }

    /// Replaces `counter()` and `counters()` in the `content` of a
    /// pseudo-element style with their text. Each run of counters and
    /// short strings becomes one string, so that a long `content` value
    /// does not keep one allocation per counter; longer strings stay
    /// shared. Once the counter text of the document has reached its
    /// limit, counters give no text.
    fn resolve_content(&mut self, style: &mut Arc<ComputedStyle>) {
        let Content::Items(items) = &style.content else {
            return;
        };
        if !items
            .iter()
            .any(|item| matches!(item, ContentItem::Counter { .. }))
        {
            return;
        }
        let mut resolved = Vec::new();
        let mut run: Option<String> = None;
        for item in items.iter() {
            match item {
                ContentItem::String(text) if text.len() <= MAX_MERGED_STRING => {
                    run.get_or_insert_default().push_str(text);
                }
                ContentItem::Counter {
                    name,
                    separator,
                    style,
                } => {
                    let text = run.get_or_insert_default();
                    self.push_text(text, name, separator.as_deref(), *style);
                }
                other => {
                    resolved.extend(run.take().map(|t| ContentItem::String(Arc::from(t))));
                    resolved.push(other.clone());
                }
            }
        }
        resolved.extend(run.map(|t| ContentItem::String(Arc::from(t))));
        Arc::make_mut(style).content = Content::Items(Arc::from(resolved));
    }

    /// Appends the text of `counter(name, style)` (no separator) or
    /// `counters(name, separator, style)` to `out`, within the text limit
    /// of the document. A counter that does not exist has the value 0.
    fn push_text(
        &mut self,
        out: &mut String,
        name: &str,
        separator: Option<&str>,
        style: ListStyleType,
    ) {
        const ZERO: &[Entry] = &[Entry {
            value: 0,
            creator_depth: 0,
            owner_depth: 0,
        }];
        if self.text_left == 0 {
            warn_text_limit();
            return;
        }
        let entries = self
            .names
            .get(name)
            .and_then(|&index| self.stacks.get(index))
            .filter(|stack| !stack.is_empty())
            .map_or(ZERO, Vec::as_slice);
        let entries = match (separator, entries.split_last()) {
            (None, Some((last, _))) => std::slice::from_ref(last),
            _ => entries,
        };
        let mut left = self.text_left;
        for (i, entry) in entries.iter().enumerate() {
            let start = out.len();
            if i > 0 {
                out.push_str(separator.unwrap_or_default());
            }
            counter_text(style, entry.value, out);
            // Each value costs at least one byte, so that values without
            // text (`counters(c, "", none)`) are limited too.
            let cost = (out.len() - start).max(1);
            if cost > left {
                out.truncate(start);
                left = 0;
                warn_text_limit();
                break;
            }
            left -= cost;
        }
        self.text_left = left;
    }
}

/// Logs that a document reached [`MAX_COUNTER_TEXT`], once per process
/// (the limit is reached again on every style pass of the document).
fn warn_text_limit() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        warn!(
            "counters: more than {MAX_COUNTER_TEXT} bytes of counter text in a document; the rest is dropped"
        );
    }
}

/// The ordinal values of list items.
#[derive(Default)]
struct Lists {
    /// The list owners (`ol`, `ul`, `menu` with a box) among the
    /// ancestors of the current element, with their depths.
    owners: Vec<(usize, NodeId)>,
    /// The ordinal of the last list item of each list.
    last: HashMap<NodeId, i32>,
    /// The number of list items of each reversed `ol`, computed when the
    /// first one is needed.
    counts: Option<HashMap<NodeId, i64>>,
}

impl Lists {
    /// The list of an item whose parent is `parent` (for a
    /// pseudo-element: its element): the nearest list owner among the
    /// ancestors of the item, else `parent`.
    fn enclosing(&self, parent: NodeId) -> NodeId {
        self.owners.last().map_or(parent, |&(_, owner)| owner)
    }

    /// The ordinal value of a list item of `list`. `element` is `None`
    /// for a pseudo-element.
    fn ordinal(
        &mut self,
        doc: &Document,
        map: &StyleMap,
        element: Option<&ElementData>,
        style: &ComputedStyle,
        list: NodeId,
    ) -> i32 {
        let directive = Directive::for_name(style, LIST_ITEM);
        let reversed = reversed_ol(doc, list);
        let value = if let Some(set) = directive.set {
            set
        } else if let Some(value) = element
            .filter(|e| e.is_html_named(&local_name!("li")))
            .and_then(|e| integer_attribute(e, "value"))
        {
            value
        } else {
            let step = if directive.increment.is_some() {
                directive.combined()
            } else if reversed {
                -1
            } else {
                1
            };
            let base = match self.last.get(&list) {
                Some(&previous) => i64::from(previous),
                None => match doc.element(list) {
                    Some(ol) if ol.is_html_named(&local_name!("ol")) => {
                        let initial = match integer_attribute(ol, "start") {
                            Some(start) => i64::from(start),
                            None if reversed => self
                                .counts
                                .get_or_insert_with(|| reversed_list_counts(doc, map))
                                .get(&list)
                                .copied()
                                .unwrap_or(0),
                            None => 1,
                        };
                        initial + if reversed { 1 } else { -1 }
                    }
                    _ => 0,
                },
            };
            saturate(base + i64::from(step))
        };
        self.last.insert(list, value);
        value
    }
}

/// The number of list items of each reversed `ol`, as Chromium counts
/// them: the list items among its descendants (and their
/// pseudo-elements) that are not inside a nested list owner. One
/// post-order walk over the document, so the cost is linear also when
/// `display: contents` lists, which are not list owners, are nested.
fn reversed_list_counts(doc: &Document, map: &StyleMap) -> HashMap<NodeId, i64> {
    let mut counts = HashMap::new();
    let Some(root) = doc.document_element() else {
        return counts;
    };
    // The list items found so far in the content of each element on the
    // current path; the first entry is the document.
    let mut found: Vec<i64> = vec![0];
    let mut stack = vec![Visit::Enter(root, ())];
    let mut children = Vec::new();
    while let Some(visit) = stack.pop() {
        match visit {
            Visit::Enter(node, ()) => {
                let Some(style) = map.get(node) else {
                    continue;
                };
                if style.display == Display::None {
                    continue;
                }
                let pseudo_items = [PseudoKind::Before, PseudoKind::After]
                    .into_iter()
                    .filter(|&kind| {
                        map.pseudo(node, kind)
                            .is_some_and(|s| s.display == Display::ListItem)
                    })
                    .count();
                found.push(i64::try_from(pseudo_items).unwrap_or(0));
                stack.push(Visit::Leave(node, ()));
                if renders_children(doc.element(node)) {
                    children.clear();
                    children.extend(doc.element_children(node));
                    stack.extend(children.iter().rev().map(|&c| Visit::Enter(c, ())));
                }
            }
            Visit::Leave(node, ()) => {
                let inner = found.pop().unwrap_or(0);
                if reversed_ol(doc, node) {
                    counts.insert(node, inner);
                }
                let Some(style) = map.get(node) else {
                    continue;
                };
                let element = doc.element(node);
                // The items inside a list owner are its own, and the owner
                // itself does not count (also if it is a list item).
                let owner = style.display != Display::Contents && is_list_owner(element);
                if !owner && let Some(outer) = found.last_mut() {
                    *outer += inner + i64::from(is_list_item(element, style));
                }
            }
        }
    }
    counts
}

#[cfg(test)]
mod tests;
