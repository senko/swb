//! `NodeList` (DOM §4.2.10.1, <https://dom.spec.whatwg.org/#interface-nodelist>)
//! and `HTMLCollection` (§4.2.10.2): the live lists of `childNodes` and
//! `children`.
//!
//! A list keeps its parent and a cache of its nodes with the DOM generation
//! it was built for (ADR 0028 section 4). Every indexed read asks for the
//! number of items (`index_count`) and then reads one item, so without the
//! cache `for (let i = 0; i < c.length; i++) c[i]` would walk the children
//! twice per step; with it, the loop walks them once while the DOM does not
//! change. A change anywhere in the document makes the cache stale (the
//! generation is one counter per arena).
//!
//! Stale caches are dropped: the lists that hold a cache are registered in
//! the binding state, and the first list that builds a cache after a change
//! clears the registered ones. So a page that makes many lists and changes
//! the DOM does not keep the old children alive. An `HTMLCollection` also
//! caches its supported property names (the `id` of each element and the
//! `name` of each HTML element), because every property read on the object
//! asks whether the key is one of them.

use std::collections::HashSet;

use swb_dom::{NodeId, ns};
use swb_js::{Gc, Generic, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom, state_mut};

/// What a list lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListKind {
    /// `Node.childNodes`: all children.
    ChildNodes,
    /// `ParentNode.children`: the element children.
    Children,
}

/// A list's nodes at one DOM generation.
struct Built {
    generation: u64,
    nodes: Vec<NodeId>,
    /// The supported property names of an `HTMLCollection`, in order, with
    /// a set for the membership test; built at the first use.
    names: Option<(Vec<String>, HashSet<String>)>,
}

/// The data of a `childNodes` or `children` list.
struct ListData {
    kind: ListKind,
    parent: NodeId,
    /// The cache, if built and not dropped.
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

/// A new list of `parent` (the caller keeps it in the side table).
pub(super) fn new_list(
    rt: &mut Runtime,
    realm: u32,
    parent: NodeId,
    kind: ListKind,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(ListData {
        kind,
        parent,
        built: None,
    })?;
    let class = match kind {
        ListKind::ChildNodes => g::NODE_LIST,
        ListKind::Children => g::HTML_COLLECTION,
    };
    rt.new_host_object(realm, class, data)
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

/// Makes sure that the cache of the list is current, dropping the stale
/// caches of the other lists first.
fn ensure_built(cx: &mut Cx<'_>, data: Gc<Generic>) -> b::Result<()> {
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
    let (kind, parent) = {
        let list = list_of(cx.rt, data)?;
        (list.kind, list.parent)
    };
    let dom = dom(cx.rt)?;
    let nodes: Vec<NodeId> = match kind {
        ListKind::ChildNodes => dom.children(parent).collect(),
        ListKind::Children => dom.element_children(parent).collect(),
    };
    list_mut(cx.rt, data)?.built = Some(Built {
        generation,
        nodes,
        names: None,
    });
    state_mut(cx.rt)?.list_registry.push(data);
    Ok(())
}

/// Runs `f` on the current nodes of the list.
fn with_nodes<T>(cx: &mut Cx<'_>, this: Platform, f: impl FnOnce(&[NodeId]) -> T) -> b::Result<T> {
    let data = handle(this)?;
    ensure_built(cx, data)?;
    let built = list_of(cx.rt, data)?
        .built
        .as_ref()
        .ok_or_else(|| Exception::type_error("the list has no cache"))?;
    Ok(f(&built.nodes))
}

/// The supported property names of an `HTMLCollection` (DOM §4.2.10.2):
/// the `id` of each element and the `name` of each HTML element, in tree
/// order, without duplicates.
fn ensure_names(cx: &mut Cx<'_>, data: Gc<Generic>) -> b::Result<()> {
    ensure_built(cx, data)?;
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

impl g::node_list::NodeListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let len = with_nodes(cx, this, <[NodeId]>::len)?;
        u32::try_from(len).map_err(|_| Exception::range_error("too many nodes"))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        with_nodes(cx, this, |nodes| nodes.get(index as usize).copied())
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

impl g::html_collection::HTMLCollectionImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let len = with_nodes(cx, this, <[NodeId]>::len)?;
        u32::try_from(len).map_err(|_| Exception::range_error("too many nodes"))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        with_nodes(cx, this, |nodes| nodes.get(index as usize).copied())
    }

    /// The first element whose `id` is `name`, else the first HTML element
    /// whose `name` attribute is `name`. (DOM §4.2.10.2 says the first
    /// element that matches either; Chromium looks at the ids first, and
    /// was measured so.)
    fn named_item(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Option<NodeId>> {
        if name.is_empty() {
            return Ok(None);
        }
        let nodes = with_nodes(cx, this, <[NodeId]>::to_vec)?;
        let dom = dom(cx.rt)?;
        let by_id = nodes.iter().copied().find(|&n| {
            dom.element(n)
                .is_some_and(|e| e.id() == Some(name.as_str()))
        });
        Ok(by_id.or_else(|| {
            nodes.into_iter().find(|&n| {
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
            let change = "document.title = 'x'; document.body.childNodes.length";
            assert_eq!(count(rt, change), (Evaluation::Value("2000".to_owned()), 1));
        });
    }

    /// The first list that builds a cache after a change drops the stale
    /// caches of the others.
    #[test]
    fn stale_caches_are_dropped() {
        let html = format!("<body>{}", "<p></p>".repeat(50));
        with_runtime(html, |rt| {
            let make = "window.lists = [...document.body.children].map((p) => p.childNodes); window.lists.map((l) => l.length).length";
            assert_eq!(count(rt, make).0, Evaluation::Value("50".to_owned()));
            assert!(super::registered(&rt.rt) >= 50);
            let change = "document.body.appendChild(document.createElement('i')); document.body.childNodes.length";
            assert_eq!(count(rt, change).0, Evaluation::Value("51".to_owned()));
            assert_eq!(
                super::registered(&rt.rt),
                1,
                "only the list built after the change"
            );
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
}
