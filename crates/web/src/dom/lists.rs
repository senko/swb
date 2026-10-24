//! `NodeList` (DOM §4.2.10.1, <https://dom.spec.whatwg.org/#interface-nodelist>)
//! and `HTMLCollection` (§4.2.10.2): the lists of `childNodes` and
//! `children`, the live lists of `getElementsByTagName`,
//! `getElementsByTagNameNS`, `getElementsByClassName`, `getElementsByName`
//! and the document collections (`forms`, `images`, `links`, `scripts`,
//! `anchors`, `embeds`, `plugins`, `applets`), the same-named collection
//! of `window[name]`, and the static list of `querySelectorAll`.
//!
//! A list keeps its root and its kind, and a cache of its nodes with the
//! DOM generation it was built for (ADR 0028 section 4). The cache is
//! lazy: a read of item `i` walks the tree only as far as item `i` and
//! keeps what it found, so `for (let i = 0; i < c.length; i++) c[i]` walks
//! once while the DOM does not change, and `getElementsByTagName('body')[0]`
//! stops at the first match. A change anywhere in the document makes the
//! cache stale (the generation is one counter per arena). `length` of
//! `children` and `childNodes` reads the counts that `dom` keeps for each
//! node, and an item of these lists is found from the nearer end, so a loop
//! that drains a node by `children[0]` costs O(1) per step.
//!
//! Stale caches are dropped: the lists that hold a cache and live as long
//! as the document (the lists in the side table) are registered in the
//! binding state, and the first list that builds a cache after a change
//! clears the registered ones. So a page that makes many lists and changes
//! the DOM does not keep the old children alive. The lists that scripts
//! make with `getElementsBy*` are not registered (the registry holds
//! strong handles, and these lists can be collected); they keep their own
//! cache until they are collected or read again. An `HTMLCollection` also
//! caches its supported property names (the `id` of each element and the
//! `name` of each HTML element), because every property read on the object
//! asks whether the key is one of them.

use std::collections::HashSet;

use swb_dom::{Document, ElementData, NodeId, QuirksMode, local_name, ns};
use swb_js::{Gc, Generic, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom, state_mut};

/// The collections of the document that `document.forms` and its siblings
/// return (HTML §3.1.5, "document.images" and the following).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DocCollection {
    /// `document.images`: the `img` elements.
    Images,
    /// `document.embeds`: the `embed` elements.
    Embeds,
    /// `document.plugins`: the same as `embeds`.
    Plugins,
    /// `document.links`: the `a` and `area` elements with an `href`.
    Links,
    /// `document.forms`: the `form` elements.
    Forms,
    /// `document.scripts`: the `script` elements.
    Scripts,
    /// `document.anchors`: the `a` elements with a `name`.
    Anchors,
    /// `document.applets`: always empty.
    Applets,
}

/// What a list lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ListKind {
    /// `Node.childNodes`: all children.
    ChildNodes,
    /// `ParentNode.children`: the element children.
    Children,
    /// `getElementsByTagName`: `name` as given, `lower` in ASCII lowercase
    /// (for the elements in the HTML namespace). The name `*` is
    /// [`ListKind::AllElements`].
    TagName { name: String, lower: String },
    /// `getElementsByTagNameNS`: a `None` namespace or local name is `*`;
    /// the empty namespace is no namespace.
    TagNameNs {
        namespace: Option<String>,
        local: Option<String>,
    },
    /// All descendant elements.
    AllElements,
    /// `getElementsByClassName`: the elements that have all the classes
    /// (ASCII case-insensitive in quirks mode).
    ClassNames {
        classes: Vec<String>,
        ignore_case: bool,
    },
    /// `getElementsByName`: the HTML elements with this `name`.
    Name(String),
    /// One of the collections of the document.
    Document(DocCollection),
    /// `window[name]` with several elements: the elements with this `id`,
    /// and the `embed`, `form`, `img` and `object` elements with this
    /// `name`.
    WindowName(String),
    /// `querySelectorAll`: the nodes found at the call (the list does not
    /// change).
    Static,
}

impl ListKind {
    /// True for a list of the class `NodeList`.
    fn is_node_list(&self) -> bool {
        matches!(
            self,
            ListKind::ChildNodes | ListKind::Name(_) | ListKind::Static
        )
    }

    /// True if the binding state registers the list for the drop of stale
    /// caches: the lists of the side table, which live as long as their
    /// node.
    fn registers(&self) -> bool {
        matches!(
            self,
            ListKind::ChildNodes | ListKind::Children | ListKind::Document(_)
        )
    }
}

/// A list's nodes at one DOM generation. The walk that finds them is lazy:
/// `nodes` holds the matches up to `visited` (the last node that the walk
/// looked at), and `complete` says that the walk reached the end. A read
/// of item `i` walks only as far as that item.
struct Built {
    generation: u64,
    nodes: Vec<NodeId>,
    visited: Option<NodeId>,
    complete: bool,
    /// The supported property names of an `HTMLCollection`, in order, with
    /// a set for the membership test; built at the first use.
    names: Option<(Vec<String>, HashSet<String>)>,
}

/// The data of a list.
struct ListData {
    kind: ListKind,
    root: NodeId,
    /// The cache, if built and not dropped. A [`ListKind::Static`] list
    /// always has it.
    built: Option<Built>,
}

impl GenericData for ListData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.built.as_ref().map_or(0, |b| {
            let names = b.names.as_ref().map_or(0, |(list, set)| {
                list.iter().map(|n| 2 * n.len() + 48).sum::<usize>() + set.capacity() * 8
            });
            b.nodes.capacity() * size_of::<NodeId>() + names
        })
    }
}

/// A new list of `root` (the caller keeps it in the side table if it must
/// be the same object each time).
pub(crate) fn new_list(
    rt: &mut Runtime,
    realm: u32,
    root: NodeId,
    kind: ListKind,
) -> VmResult<Gc<Object>> {
    let class = if kind.is_node_list() {
        g::NODE_LIST
    } else {
        g::HTML_COLLECTION
    };
    let data = rt.alloc_host_data(ListData {
        kind,
        root,
        built: None,
    })?;
    rt.new_host_object(realm, class, data)
}

/// A new static `NodeList` of `nodes`.
pub(crate) fn new_static_list(
    rt: &mut Runtime,
    realm: u32,
    nodes: Vec<NodeId>,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(ListData {
        kind: ListKind::Static,
        root: NodeId::DOCUMENT,
        built: Some(Built {
            generation: 0,
            nodes,
            visited: None,
            complete: true,
            names: None,
        }),
    })?;
    rt.new_host_object(realm, g::NODE_LIST, data)
}

/// The generic data of a list.
fn handle(this: Platform) -> b::Result<Gc<Generic>> {
    match this.data {
        HostData::Generic(data) => Ok(data),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

fn list_of(rt: &Runtime, data: Gc<Generic>) -> b::Result<&ListData> {
    Ok(rt.heap().generic::<ListData>(data).map_err(VmError::from)?)
}

fn list_mut(rt: &mut Runtime, data: Gc<Generic>) -> b::Result<&mut ListData> {
    Ok(rt
        .heap_mut()
        .generic_mut::<ListData>(data)
        .map_err(VmError::from)?)
}

/// True if the qualified name of `e` (`prefix:local`, or the local name)
/// is `name`.
fn qualified_name_is(e: &ElementData, name: &str) -> bool {
    match &e.name.prefix {
        Some(prefix) => name
            .strip_prefix(&**prefix)
            .and_then(|rest| rest.strip_prefix(':'))
            .is_some_and(|local| local == &*e.name.local),
        None => name == &*e.name.local,
    }
}

/// True if the element `e` is in the list of `kind`.
fn element_matches(e: &ElementData, kind: &ListKind) -> bool {
    match kind {
        ListKind::AllElements => true,
        ListKind::TagName { name, lower } => {
            qualified_name_is(e, if e.name.ns == ns!(html) { lower } else { name })
        }
        ListKind::TagNameNs { namespace, local } => {
            namespace.as_deref().is_none_or(|n| *e.name.ns == *n)
                && local.as_deref().is_none_or(|l| *e.name.local == *l)
        }
        ListKind::ClassNames {
            classes,
            ignore_case,
        } => {
            !classes.is_empty()
                && classes.iter().all(|wanted| {
                    e.classes().any(|have| {
                        if *ignore_case {
                            have.eq_ignore_ascii_case(wanted)
                        } else {
                            have == wanted
                        }
                    })
                })
        }
        ListKind::Name(name) => e.is_html() && e.attr("name") == Some(name.as_str()),
        ListKind::Document(which) => document_collection_has(e, *which),
        ListKind::WindowName(name) => e.id() == Some(name.as_str()) || named_by_window(e, name),
        ListKind::ChildNodes | ListKind::Children | ListKind::Static => false,
    }
}

/// True if the element is in the document collection.
fn document_collection_has(e: &ElementData, which: DocCollection) -> bool {
    if !e.is_html() {
        return false;
    }
    let local = e.local_name();
    match which {
        DocCollection::Images => *local == local_name!("img"),
        DocCollection::Embeds | DocCollection::Plugins => *local == local_name!("embed"),
        DocCollection::Links => {
            (*local == local_name!("a") || *local == local_name!("area")) && e.has_attr("href")
        }
        DocCollection::Forms => *local == local_name!("form"),
        DocCollection::Scripts => *local == local_name!("script"),
        DocCollection::Anchors => *local == local_name!("a") && e.has_attr("name"),
        DocCollection::Applets => false,
    }
}

/// True for an `embed`, `form`, `img` or `object` element whose `name` is
/// `name` (a name of the window's named properties).
pub(crate) fn named_by_window(e: &ElementData, name: &str) -> bool {
    e.is_html()
        && [
            local_name!("embed"),
            local_name!("form"),
            local_name!("img"),
            local_name!("object"),
        ]
        .contains(e.local_name())
        && e.attr("name") == Some(name)
}

/// True if the node `n` is in the list of `kind`.
fn node_in_list(dom: &Document, n: NodeId, kind: &ListKind) -> bool {
    match kind {
        ListKind::ChildNodes => true,
        ListKind::Children => dom.element(n).is_some(),
        ListKind::Static => false,
        _ => dom.element(n).is_some_and(|e| element_matches(e, kind)),
    }
}

/// The node that the walk of a list looks at after `visited` (the first
/// one if `None`): the next sibling for the lists of children, the next
/// node in tree order below `root` for the others.
fn next_to_visit(
    dom: &Document,
    root: NodeId,
    kind: &ListKind,
    visited: Option<NodeId>,
) -> Option<NodeId> {
    let child_list = matches!(kind, ListKind::ChildNodes | ListKind::Children);
    match (visited, child_list) {
        (None, true) => dom.node(root).first_child(),
        (Some(v), true) => dom.node(v).next_sibling(),
        (None, false) => dom.next_in_tree_order(root, root),
        (Some(v), false) => dom.next_in_tree_order(v, root),
    }
}

/// Continues the walk of a list until it has more than `upto` nodes
/// (`None`: to the end).
fn walk(built: &mut Built, dom: &Document, root: NodeId, kind: &ListKind, upto: Option<usize>) {
    while !built.complete && upto.is_none_or(|i| built.nodes.len() <= i) {
        match next_to_visit(dom, root, kind, built.visited) {
            None => built.complete = true,
            Some(n) => {
                built.visited = Some(n);
                if node_in_list(dom, n, kind) {
                    built.nodes.push(n);
                }
            }
        }
    }
}

/// The number of nodes of a list of children, from the counts that the
/// DOM keeps.
fn child_list_len(dom: &Document, root: NodeId, kind: &ListKind) -> Option<usize> {
    match kind {
        ListKind::ChildNodes => Some(dom.child_count(root)),
        ListKind::Children => Some(dom.element_child_count(root)),
        _ => None,
    }
}

/// Makes sure that the list has a cache of the current generation (it may
/// be empty or partial), dropping the stale caches of the registered
/// lists first.
fn fresh(cx: &mut Cx<'_>, data: Gc<Generic>) -> b::Result<()> {
    if list_of(cx.rt, data)?.kind == ListKind::Static {
        return Ok(());
    }
    let generation = dom(cx.rt)?.generation();
    if let Some(built) = &list_of(cx.rt, data)?.built
        && built.generation == generation
    {
        return Ok(());
    }
    let stale = {
        let state = state_mut(cx.rt)?;
        if state.list_registry_generation == generation {
            Vec::new()
        } else {
            state.list_registry_generation = generation;
            std::mem::take(&mut state.list_registry)
        }
    };
    for other in stale {
        list_mut(cx.rt, other)?.built = None;
    }
    #[cfg(test)]
    tests::REBUILDS.with(|r| r.set(r.get() + 1));
    let registers = list_of(cx.rt, data)?.kind.registers();
    list_mut(cx.rt, data)?.built = Some(Built {
        generation,
        nodes: Vec::new(),
        visited: None,
        complete: false,
        names: None,
    });
    if registers {
        state_mut(cx.rt)?.list_registry.push(data);
    }
    Ok(())
}

/// Makes sure that the cache of the list has more than `upto` nodes, or
/// all of them (`None`).
fn extend(cx: &mut Cx<'_>, data: Gc<Generic>, upto: Option<usize>) -> b::Result<()> {
    fresh(cx, data)?;
    let mut built = list_mut(cx.rt, data)?
        .built
        .take()
        .ok_or_else(|| Exception::type_error("the list has no cache"))?;
    if !built.complete {
        let list = list_of(cx.rt, data)?;
        walk(&mut built, dom(cx.rt)?, list.root, &list.kind, upto);
    }
    list_mut(cx.rt, data)?.built = Some(built);
    Ok(())
}

/// The cache of the list (after [`extend`]).
fn built_of(rt: &Runtime, data: Gc<Generic>) -> b::Result<&Built> {
    list_of(rt, data)?
        .built
        .as_ref()
        .ok_or_else(|| Exception::type_error("the list has no cache"))
}

/// The number of items. The lists of children read the counts of the DOM;
/// the others walk to the end (once, while the DOM does not change).
fn list_len(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
    let data = handle(this)?;
    let list = list_of(cx.rt, data)?;
    let counted = child_list_len(dom(cx.rt)?, list.root, &list.kind);
    let len = if let Some(len) = counted {
        len
    } else {
        extend(cx, data, None)?;
        built_of(cx.rt, data)?.nodes.len()
    };
    u32::try_from(len).map_err(|_| Exception::range_error("too many nodes"))
}

/// True if `index` is the index of an item.
fn list_has(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<bool> {
    let data = handle(this)?;
    let index = index as usize;
    let list = list_of(cx.rt, data)?;
    if let Some(len) = child_list_len(dom(cx.rt)?, list.root, &list.kind) {
        return Ok(index < len);
    }
    extend(cx, data, Some(index))?;
    Ok(built_of(cx.rt, data)?.nodes.len() > index)
}

/// Item `index` of the list. The walk goes only as far as the item and
/// keeps what it found. In a list of children the walk starts at the
/// nearer end when the cache is not near the item (a loop that takes the
/// last child and removes it).
fn list_item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
    let data = handle(this)?;
    let index = index as usize;
    let list = list_of(cx.rt, data)?;
    let document = dom(cx.rt)?;
    if let Some(len) = child_list_len(document, list.root, &list.kind) {
        if index >= len {
            return Ok(None);
        }
        let have = list
            .built
            .as_ref()
            .filter(|b| b.generation == document.generation())
            .map_or(0, |b| b.nodes.len());
        if index >= have && len - 1 - index < index - have {
            return Ok(nth_child_from_end(
                document,
                list.root,
                &list.kind,
                len - 1 - index,
            ));
        }
    }
    extend(cx, data, Some(index))?;
    Ok(built_of(cx.rt, data)?.nodes.get(index).copied())
}

/// The `back`th item from the end of a list of children (0: the last).
fn nth_child_from_end(
    dom: &Document,
    root: NodeId,
    kind: &ListKind,
    back: usize,
) -> Option<NodeId> {
    std::iter::successors(dom.node(root).last_child(), |&n| dom.node(n).prev_sibling())
        .filter(|&n| node_in_list(dom, n, kind))
        .nth(back)
}

/// The supported property names of an `HTMLCollection` (DOM §4.2.10.2):
/// the `id` of each element and the `name` of each HTML element, in tree
/// order, without duplicates.
fn ensure_names(cx: &mut Cx<'_>, data: Gc<Generic>) -> b::Result<()> {
    extend(cx, data, None)?;
    if list_of(cx.rt, data)?
        .built
        .as_ref()
        .is_some_and(|b| b.names.is_some())
    {
        return Ok(());
    }
    let mut list = Vec::new();
    let mut set = HashSet::new();
    {
        let dom = dom(cx.rt)?;
        let built = list_of(cx.rt, data)?
            .built
            .as_ref()
            .ok_or_else(|| Exception::type_error("the list has no cache"))?;
        for &node in &built.nodes {
            let Some(e) = dom.element(node) else {
                continue;
            };
            let id = e.id();
            let name = if e.name.ns == ns!(html) {
                e.attr("name").filter(|n| !n.is_empty())
            } else {
                None
            };
            for n in [id, name].into_iter().flatten() {
                if set.insert(n.to_owned()) {
                    list.push(n.to_owned());
                }
            }
        }
    }
    if let Some(built) = &mut list_mut(cx.rt, data)?.built {
        built.names = Some((list, set));
    }
    Ok(())
}

/// The number of registered lists that hold a cache (for the tests).
#[cfg(test)]
pub(crate) fn registered(rt: &Runtime) -> usize {
    crate::state::state(rt).map_or(0, |s| s.list_registry.len())
}

/// The class names of `getElementsByClassName`: the ASCII whitespace
/// separated tokens of the string.
fn class_tokens(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for token in text.split(|c: char| c.is_ascii_whitespace()) {
        if !token.is_empty() && !tokens.iter().any(|t| t == token) {
            tokens.push(token.to_owned());
        }
    }
    tokens
}

/// The list of `getElementsByTagName(name)` below `root`.
pub(super) fn tag_name(cx: &mut Cx<'_>, root: NodeId, name: &str) -> b::Result<Gc<Object>> {
    let kind = if name == "*" {
        ListKind::AllElements
    } else {
        ListKind::TagName {
            name: name.to_owned(),
            lower: name.to_ascii_lowercase(),
        }
    };
    let realm = cx.realm();
    Ok(new_list(cx.rt, realm, root, kind)?)
}

/// The list of `getElementsByTagNameNS(namespace, local)` below `root`.
pub(super) fn tag_name_ns(
    cx: &mut Cx<'_>,
    root: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> b::Result<Gc<Object>> {
    let namespace = match namespace {
        Some("*") => None,
        Some(other) => Some(other.to_owned()),
        None => Some(String::new()),
    };
    let local = (local != "*").then(|| local.to_owned());
    let realm = cx.realm();
    let kind = if namespace.is_none() && local.is_none() {
        ListKind::AllElements
    } else {
        ListKind::TagNameNs { namespace, local }
    };
    Ok(new_list(cx.rt, realm, root, kind)?)
}

/// The list of `getElementsByClassName(names)` below `root`.
pub(super) fn class_names(cx: &mut Cx<'_>, root: NodeId, names: &str) -> b::Result<Gc<Object>> {
    let dom = dom(cx.rt)?;
    let ignore_case = dom.quirks_mode_of(dom.node_document(root)) == QuirksMode::Quirks;
    let kind = ListKind::ClassNames {
        classes: class_tokens(names),
        ignore_case,
    };
    let realm = cx.realm();
    Ok(new_list(cx.rt, realm, root, kind)?)
}

/// The live `NodeList` of `getElementsByName(name)` of the document `root`.
pub(super) fn name(cx: &mut Cx<'_>, root: NodeId, name: &str) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    Ok(new_list(
        cx.rt,
        realm,
        root,
        ListKind::Name(name.to_owned()),
    )?)
}

/// The collection `which` of the document `root`.
pub(super) fn document_collection(
    cx: &mut Cx<'_>,
    root: NodeId,
    which: DocCollection,
) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    Ok(crate::state::same_object(
        cx.rt,
        root,
        crate::state::Slot::Collection(which),
        |rt| new_list(rt, realm, root, ListKind::Document(which)),
    )?)
}

/// The collection of the elements of the window's document that have the
/// property name `name` (`window[name]` with several elements).
pub(crate) fn window_name(rt: &mut Runtime, realm: u32, name: &str) -> VmResult<Gc<Object>> {
    new_list(
        rt,
        realm,
        NodeId::DOCUMENT,
        ListKind::WindowName(name.to_owned()),
    )
}

impl g::node_list::NodeListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        list_len(cx, this)
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        list_item(cx, this, index)
    }

    fn has_index(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<bool> {
        list_has(cx, this, index)
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

impl g::html_collection::HTMLCollectionImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        list_len(cx, this)
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        list_item(cx, this, index)
    }

    fn has_index(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<bool> {
        list_has(cx, this, index)
    }

    /// The first element whose `id` is `name`, else the first HTML element
    /// whose `name` attribute is `name`. (DOM §4.2.10.2 says the first
    /// element that matches either; Chromium looks at the ids first, and
    /// was measured so.)
    fn named_item(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Option<NodeId>> {
        if name.is_empty() {
            return Ok(None);
        }
        let data = handle(this)?;
        extend(cx, data, None)?;
        let nodes = &built_of(cx.rt, data)?.nodes;
        let dom = dom(cx.rt)?;
        let by_id = nodes.iter().copied().find(|&n| {
            dom.element(n)
                .is_some_and(|e| e.id() == Some(name.as_str()))
        });
        Ok(by_id.or_else(|| {
            nodes.iter().copied().find(|&n| {
                dom.element(n).is_some_and(|e| {
                    e.name.ns == ns!(html) && e.attr("name") == Some(name.as_str())
                })
            })
        }))
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }

    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        let data = handle(this)?;
        ensure_names(cx, data)?;
        Ok(list_of(cx.rt, data)?
            .built
            .as_ref()
            .and_then(|b| b.names.as_ref())
            .map(|(list, _)| list.clone())
            .unwrap_or_default())
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        let data = handle(this)?;
        ensure_names(cx, data)?;
        Ok(list_of(cx.rt, data)?
            .built
            .as_ref()
            .and_then(|b| b.names.as_ref())
            .is_some_and(|(_, set)| set.contains(name)))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::cell::Cell;

    use swb_dom::Document;

    use crate::{EngineHost, Evaluation, JsonForm, ScriptRuntime};

    thread_local! {
        /// The number of rebuilds of a list's cache on this thread.
        pub(super) static REBUILDS: Cell<usize> = const { Cell::new(0) };
    }

    struct Doc(Document);

    impl EngineHost for Doc {
        fn dom(&self) -> &Document {
            &self.0
        }
        fn dom_mut(&mut self) -> &mut Document {
            &mut self.0
        }
        fn url(&self) -> String {
            "about:blank".to_owned()
        }
        fn base_url(&mut self) -> String {
            "about:blank".to_owned()
        }
        fn parse_url(&mut self, _value: &str) -> Option<String> {
            None
        }
        fn character_set(&self) -> &'static str {
            "UTF-8"
        }
        fn content_type(&self) -> &'static str {
            "text/html"
        }
        fn into_any(self: Box<Self>) -> Box<dyn Any> {
            self
        }
    }

    /// Runs `body` on a thread with a big stack, with a runtime for `html`.
    fn with_runtime(html: String, body: impl FnOnce(&mut ScriptRuntime) + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(32 << 20)
            .spawn(move || {
                let mut rt = ScriptRuntime::new(crate::PageClock::real()).expect("a runtime");
                rt.lend(Box::new(Doc(swb_dom::parse_html_with(&html, true))));
                body(&mut rt);
            })
            .expect("a thread")
            .join()
            .expect("the test thread");
    }

    fn count(rt: &mut ScriptRuntime, source: &str) -> (Evaluation, usize) {
        let before = REBUILDS.with(Cell::get);
        let value = rt.evaluate(source, JsonForm::Strict);
        (value, REBUILDS.with(Cell::get) - before)
    }

    /// A loop over `childNodes` builds the list once while the DOM does
    /// not change, and again after a change.
    #[test]
    fn a_loop_over_child_nodes_builds_the_list_once() {
        let html = format!("<body>{}", "<p></p>".repeat(2000));
        with_runtime(html, |rt| {
            let walk = "{ const c = document.body.childNodes; let n = 0; for (let i = 0; i < c.length; i++) if (c[i]) n++; n; }";
            assert_eq!(count(rt, walk), (Evaluation::Value("2000".to_owned()), 1));
            assert_eq!(count(rt, walk).1, 0);
            let change = "document.title = 'x'; document.body.childNodes[0] ? 1 : 0";
            assert_eq!(count(rt, change), (Evaluation::Value("1".to_owned()), 1));
            // The length comes from the counts of the DOM: no cache.
            let length = "document.title = 'y'; document.body.childNodes.length";
            assert_eq!(count(rt, length), (Evaluation::Value("2000".to_owned()), 0));
        });
    }

    /// The first list that builds a cache after a change drops the stale
    /// caches of the others.
    #[test]
    fn stale_caches_are_dropped() {
        let html = format!("<body>{}", "<p><i></i></p>".repeat(50));
        with_runtime(html, |rt| {
            let make = "window.lists = [...document.body.children].map((p) => p.childNodes); window.lists.map((l) => l[0]).length";
            assert_eq!(count(rt, make).0, Evaluation::Value("50".to_owned()));
            assert!(super::registered(&rt.rt) >= 50);
            let change = "document.body.appendChild(document.createElement('i')); document.body.childNodes[0] ? 51 : 0";
            assert_eq!(count(rt, change).0, Evaluation::Value("51".to_owned()));
            assert_eq!(
                super::registered(&rt.rt),
                1,
                "only the list built after the change"
            );
        });
    }

    /// The lists that `getElementsBy*` and `querySelectorAll` make are not
    /// registered (the registry holds strong handles, and these lists can be
    /// collected), and a loop over one of them walks the tree once while the
    /// DOM does not change.
    #[test]
    fn lists_of_queries_are_not_registered() {
        let html = format!("<body>{}", "<p class=x></p>".repeat(500));
        with_runtime(html, |rt| {
            let make = "{ let n = 0; for (let i = 0; i < 50; i++) { n += document.getElementsByTagName('p').length + document.getElementsByClassName('x').length + document.querySelectorAll('p').length + document.getElementsByName('q').length; } n; }";
            let (value, rebuilds) = count(rt, make);
            assert_eq!(value, Evaluation::Value("75000".to_owned()));
            // Three live lists per round, one walk each.
            assert_eq!(rebuilds, 150);
            assert_eq!(super::registered(&rt.rt), 0);
            let walk = "{ const c = document.getElementsByTagName('*'); let n = 0; for (let i = 0; i < c.length; i++) if (c[i]) n++; n; }";
            let (value, rebuilds) = count(rt, walk);
            assert_eq!(value, Evaluation::Value("503".to_owned()));
            assert_eq!(rebuilds, 1, "a loop over a live list walks the tree once");
        });
    }

    /// Property reads on a collection do not walk it again: a loop over
    /// 3,000 children builds the cache and the names once.
    #[test]
    fn a_collection_loop_builds_once() {
        let html = format!("<body>{}", "<p id=x></p>".repeat(3000));
        with_runtime(html, |rt| {
            let walk = "{ const c = document.body.children; let n = 0; for (let i = 0; i < c.length; i++) if (c[i].id) n++; n; }";
            assert_eq!(count(rt, walk), (Evaluation::Value("3000".to_owned()), 1));
        });
    }

    /// A drain loop over `children` and `childNodes` reads the first item
    /// and the length at each step: the cost of a step does not depend on
    /// the number of children.
    #[test]
    fn drain_loops_walk_one_step_per_read() {
        let html = format!("<body><div id=r>{}</div>", "<p></p>".repeat(3000));
        with_runtime(html, |rt| {
            let drain = "{ const r = document.getElementById('r'); let n = 0; while (r.children.length) { r.removeChild(r.children[0]); n++; } n; }";
            assert_eq!(count(rt, drain).0, Evaluation::Value("3000".to_owned()));
            let refill = "{ const r = document.getElementById('r'); for (let i = 0; i < 100; i++) r.appendChild(document.createElement('p')); r.appendChild(document.createTextNode('t')); 0; }";
            count(rt, refill);
            // From the end, and by the order of the items.
            let back = "{ const r = document.getElementById('r'); let n = 0; while (r.childNodes.length) { r.removeChild(r.childNodes[r.childNodes.length - 1]); n++; } n; }";
            assert_eq!(count(rt, back).0, Evaluation::Value("101".to_owned()));
        });
    }

    /// An index read walks only as far as the item.
    #[test]
    fn an_item_read_walks_to_the_item_only() {
        let html = format!("<body>{}<i id=x></i>", "<p></p>".repeat(100));
        with_runtime(html, |rt| {
            let read = "{ const l = document.getElementsByTagName('*'); [l[1].tagName, l[2] === l[2], l.item(5) === l[5], l[1000] === undefined, l.length].join(); }";
            assert_eq!(
                count(rt, read).0,
                Evaluation::Value("\"HEAD,true,true,true,104\"".to_owned())
            );
        });
    }
}
