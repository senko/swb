//! `NodeList` (DOM §4.2.10.1, <https://dom.spec.whatwg.org/#interface-nodelist>):
//! the live list of `childNodes`.
//!
//! The list keeps its parent and a cache of the children with the DOM
//! generation it was built for (ADR 0028 section 4). Every indexed read
//! asks for the number of items (`index_count`) and then reads one item,
//! so without the cache `for (let i = 0; i < c.length; i++) c[i]` would
//! walk the children twice per step; with it, the loop walks them once
//! while the DOM does not change. A change anywhere in the document makes
//! the cache stale (the generation is one counter per arena).

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom};

/// The data of a `childNodes` list.
struct ChildList {
    parent: NodeId,
    /// The children at the generation `generation`, if built.
    cache: Option<(u64, Vec<NodeId>)>,
}

impl GenericData for ChildList {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.cache
            .as_ref()
            .map_or(0, |(_, nodes)| nodes.capacity() * size_of::<NodeId>())
    }
}

/// A new `childNodes` list of `parent` (the caller keeps it in the side
/// table).
pub(super) fn new_child_list(rt: &mut Runtime, realm: u32, parent: NodeId) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(ChildList {
        parent,
        cache: None,
    })?;
    rt.new_host_object(realm, g::NODE_LIST, data)
}

/// The generic data of a list.
fn handle(this: Platform) -> b::Result<Gc<swb_js::Generic>> {
    match this.data {
        HostData::Generic(data) => Ok(data),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// Runs `f` on the current children of the list, after a rebuild of the
/// cache if the DOM changed since it was built.
fn with_children<T>(
    cx: &mut Cx<'_>,
    this: Platform,
    f: impl FnOnce(&[NodeId]) -> T,
) -> b::Result<T> {
    let data = handle(this)?;
    let generation = dom(cx.rt)?.generation();
    let list = cx
        .rt
        .heap()
        .generic::<ChildList>(data)
        .map_err(VmError::from)?;
    if let Some((built, nodes)) = &list.cache
        && *built == generation
    {
        return Ok(f(nodes));
    }
    let parent = list.parent;
    #[cfg(test)]
    tests::REBUILDS.with(|r| r.set(r.get() + 1));
    let nodes: Vec<NodeId> = dom(cx.rt)?.children(parent).collect();
    let result = f(&nodes);
    let list = cx
        .rt
        .heap_mut()
        .generic_mut::<ChildList>(data)
        .map_err(VmError::from)?;
    list.cache = Some((generation, nodes));
    Ok(result)
}

impl g::node_list::NodeListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let len = with_children(cx, this, <[NodeId]>::len)?;
        u32::try_from(len).map_err(|_| Exception::range_error("too many nodes"))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        with_children(cx, this, |nodes| nodes.get(index as usize).copied())
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
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

    /// A loop over `childNodes` builds the list once while the DOM does
    /// not change, and again after a change.
    #[test]
    fn a_loop_over_child_nodes_builds_the_list_once() {
        std::thread::Builder::new()
            .stack_size(32 << 20)
            .spawn(|| {
                let html = format!("<body>{}", "<p></p>".repeat(2000));
                let mut rt = ScriptRuntime::new().expect("a runtime");
                rt.lend(Box::new(Doc(swb_dom::parse_html_with(&html, true))));
                let count = |rt: &mut ScriptRuntime, source: &str| {
                    let before = REBUILDS.with(Cell::get);
                    let value = rt.evaluate(source, JsonForm::Strict);
                    (value, REBUILDS.with(Cell::get) - before)
                };
                let walk = "{ const c = document.body.childNodes; let n = 0; for (let i = 0; i < c.length; i++) if (c[i]) n++; n; }";
                assert_eq!(count(&mut rt, walk), (Evaluation::Value("2000".to_owned()), 1));
                assert_eq!(count(&mut rt, walk).1, 0);
                let change = "document.title = 'x'; document.body.childNodes.length";
                assert_eq!(count(&mut rt, change), (Evaluation::Value("2000".to_owned()), 1));
            })
            .expect("a thread")
            .join()
            .expect("the test thread");
    }
}
