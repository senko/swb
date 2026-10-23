//! The generated glue of the browser's IDL (`crates/web/idl/`, generated
//! into `src/generated/`) registers in a runtime without the behaviour of
//! features 4c and 5a: a test host implements the traits with stand-ins.
//! The tests check the interface objects, the prototype chains, the
//! property names, descriptors, `length` and `name` of the members, the
//! constants, the argument checks of the glue, and `DOMException`, against
//! Chromium 148 (`tools/probes/web-bindings.json`). Every case runs with
//! and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of a test runtime")]

use swb_js::{
    Gc, GenericData, HeapConfig, HostState, NativeCall, NativeReturn, Object, Runtime,
    RuntimeConfig, Tracer, VmError, VmResult,
};
use swb_web::bindings::{self as b, Cx, Exception, GlueState, Host, NodeId, Platform};
use swb_web::generated;
use swb_web::generated::types::{
    AddEventListenerOptionsOrBoolean, EventInit, EventListener, EventListenerOptionsOrBoolean,
    GetRootNodeOptions,
};

#[derive(Default)]
struct TestHost {
    glue: GlueState,
    wrappers: Vec<Gc<Object>>,
}

impl HostState for TestHost {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        self.glue.trace(tracer);
        for wrapper in &self.wrappers {
            tracer.object(*wrapper);
        }
    }
}

/// The data of an `Event` (what the constructor got).
struct EventData {
    kind: String,
    init: EventInit,
}

impl GenericData for EventData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.kind.capacity()
    }
}

/// The data of an `EventTarget` made by its constructor.
struct TargetData;

impl GenericData for TargetData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

fn missing<T>() -> b::Result<T> {
    Err(Exception::type_error("not in this test"))
}

fn event<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a EventData> {
    let swb_js::HostData::Generic(data) = this.data else {
        return missing();
    };
    Ok(cx
        .rt
        .heap()
        .generic::<EventData>(data)
        .map_err(VmError::from)?)
}

impl Host for TestHost {
    fn glue_state(&mut self) -> &mut GlueState {
        &mut self.glue
    }

    fn wrap_node(rt: &mut Runtime, realm: u32, node: NodeId) -> VmResult<Gc<Object>> {
        let host = rt
            .host_data_mut::<TestHost>()
            .ok_or(VmError::type_error("no host"))?;
        if let Some(wrapper) = host.wrappers.get(node.index()) {
            return Ok(*wrapper);
        }
        let wrapper = rt.new_host_object(realm, generated::NODE, b::node_data(node))?;
        let host = rt
            .host_data_mut::<TestHost>()
            .ok_or(VmError::type_error("no host"))?;
        host.wrappers.push(wrapper);
        Ok(wrapper)
    }

    fn reflect_get(_cx: &mut Cx<'_>, _element: NodeId, _name: &str) -> b::Result<Option<String>> {
        missing()
    }

    fn reflect_set(_cx: &mut Cx<'_>, _element: NodeId, _name: &str, _value: &str) -> b::Result<()> {
        missing()
    }

    fn reflect_remove(_cx: &mut Cx<'_>, _element: NodeId, _name: &str) -> b::Result<()> {
        missing()
    }

    fn reflect_url(_cx: &mut Cx<'_>, _element: NodeId, _value: &str) -> Option<String> {
        None
    }
}

impl generated::event_target::EventTargetImpl for TestHost {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(TargetData)?;
        Ok(cx.new_instance::<TestHost>(data)?)
    }

    fn add_event_listener(
        _cx: &mut Cx<'_>,
        _this: Platform,
        _type: String,
        _callback: Option<EventListener>,
        _options: AddEventListenerOptionsOrBoolean,
    ) -> b::Result<()> {
        Ok(())
    }

    fn remove_event_listener(
        _cx: &mut Cx<'_>,
        _this: Platform,
        _type: String,
        _callback: Option<EventListener>,
        _options: EventListenerOptionsOrBoolean,
    ) -> b::Result<()> {
        Ok(())
    }

    fn dispatch_event(_cx: &mut Cx<'_>, _this: Platform, _event: Platform) -> b::Result<bool> {
        Ok(true)
    }
}

impl generated::event::EventImpl for TestHost {
    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(event(cx, this)?.kind.clone())
    }
    fn target(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }
    fn src_element(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }
    fn current_target(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }
    fn event_phase(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }
    fn cancel_bubble(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }
    fn set_cancel_bubble(_cx: &mut Cx<'_>, _this: Platform, _value: bool) -> b::Result<()> {
        Ok(())
    }
    fn bubbles(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.init.bubbles)
    }
    fn cancelable(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.init.cancelable)
    }
    fn return_value(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(true)
    }
    fn set_return_value(_cx: &mut Cx<'_>, _this: Platform, _value: bool) -> b::Result<()> {
        Ok(())
    }
    fn default_prevented(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }
    fn composed(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.init.composed)
    }
    fn is_trusted(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }
    fn time_stamp(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }
    fn constructor(cx: &mut Cx<'_>, kind: String, init: EventInit) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(EventData { kind, init })?;
        Ok(cx.new_instance::<TestHost>(data)?)
    }
    fn composed_path(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<Gc<Object>>> {
        Ok(Vec::new())
    }
    fn stop_propagation(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }
    fn stop_immediate_propagation(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }
    fn prevent_default(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }
    fn init_event(
        _cx: &mut Cx<'_>,
        _this: Platform,
        _kind: String,
        _bubbles: bool,
        _cancelable: bool,
    ) -> b::Result<()> {
        Ok(())
    }
}

impl generated::node::NodeImpl for TestHost {
    fn node_type(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<u16> {
        Ok(1)
    }
    fn node_name(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok("DIV".to_owned())
    }
    fn base_uri(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok("about:blank".to_owned())
    }
    fn is_connected(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<bool> {
        Ok(false)
    }
    fn parent_node(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(None)
    }
    fn first_child(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(None)
    }
    fn last_child(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(None)
    }
    fn previous_sibling(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(None)
    }
    fn next_sibling(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(None)
    }
    fn node_value(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<String>> {
        Ok(None)
    }
    fn set_node_value(_cx: &mut Cx<'_>, _this: NodeId, _value: Option<String>) -> b::Result<()> {
        Ok(())
    }
    fn text_content(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<Option<String>> {
        Ok(Some(String::new()))
    }
    fn set_text_content(_cx: &mut Cx<'_>, _this: NodeId, _value: Option<String>) -> b::Result<()> {
        Ok(())
    }
    fn get_root_node(
        _cx: &mut Cx<'_>,
        this: NodeId,
        _options: GetRootNodeOptions,
    ) -> b::Result<NodeId> {
        Ok(this)
    }
    fn has_child_nodes(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<bool> {
        Ok(false)
    }
    fn normalize(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<()> {
        Ok(())
    }
    fn clone_node(_cx: &mut Cx<'_>, _this: NodeId, _subtree: bool) -> b::Result<NodeId> {
        missing()
    }
    fn is_equal_node(_cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        Ok(other == Some(this))
    }
    fn is_same_node(_cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        Ok(other == Some(this))
    }
    fn compare_document_position(
        _cx: &mut Cx<'_>,
        _this: NodeId,
        _other: NodeId,
    ) -> b::Result<u16> {
        Ok(1)
    }
    fn contains(_cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        Ok(other == Some(this))
    }
    fn lookup_prefix(
        _cx: &mut Cx<'_>,
        _this: NodeId,
        _namespace: Option<String>,
    ) -> b::Result<Option<String>> {
        Ok(None)
    }
    fn lookup_namespace_uri(
        _cx: &mut Cx<'_>,
        _this: NodeId,
        _prefix: Option<String>,
    ) -> b::Result<Option<String>> {
        Ok(None)
    }
    fn is_default_namespace(
        _cx: &mut Cx<'_>,
        _this: NodeId,
        _namespace: Option<String>,
    ) -> b::Result<bool> {
        Ok(false)
    }
    fn insert_before(
        _cx: &mut Cx<'_>,
        _this: NodeId,
        node: NodeId,
        _child: Option<NodeId>,
    ) -> b::Result<NodeId> {
        Ok(node)
    }
    fn append_child(_cx: &mut Cx<'_>, this: NodeId, node: NodeId) -> b::Result<NodeId> {
        if node == this {
            return Err(Exception::dom(
                "HierarchyRequestError",
                "The new child element contains the parent.",
            ));
        }
        Ok(node)
    }
    fn replace_child(
        _cx: &mut Cx<'_>,
        _this: NodeId,
        _node: NodeId,
        child: NodeId,
    ) -> b::Result<NodeId> {
        Ok(child)
    }
    fn remove_child(_cx: &mut Cx<'_>, _this: NodeId, child: NodeId) -> b::Result<NodeId> {
        Ok(child)
    }
}

/// The nodes of a stand-in `NodeList`.
struct ListData(Vec<NodeId>);

impl GenericData for ListData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<NodeId>()
    }
}

fn list<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a [NodeId]> {
    let swb_js::HostData::Generic(data) = this.data else {
        return missing();
    };
    Ok(&cx
        .rt
        .heap()
        .generic::<ListData>(data)
        .map_err(VmError::from)?
        .0)
}

impl generated::node_list::NodeListImpl for TestHost {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        u32::try_from(list(cx, this)?.len()).map_err(|_| Exception::range_error("long"))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        Ok(list(cx, this)?.get(index as usize).copied())
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

/// `makeList(...nodes)`: a stand-in `NodeList` of the nodes.
fn make_list(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut nodes = Vec::new();
    for i in 0..call.argc() {
        let node = rt
            .host_object(rt.arg(call, i))
            .and_then(|(_, data)| b::node_of(data))
            .ok_or(VmError::type_error("not a node"))?;
        nodes.push(node);
    }
    let data = rt.alloc_host_data(ListData(nodes))?;
    let object = rt.new_host_object(call.realm(), generated::NODE_LIST, data)?;
    Ok(NativeReturn::Value(object.into()))
}

fn make_node(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let count = rt.host_data::<TestHost>().map_or(0, |h| h.wrappers.len());
    let id = NodeId::from_index(count).ok_or(VmError::type_error("node"))?;
    Ok(NativeReturn::Value(
        TestHost::wrap_node(rt, call.realm(), id)?.into(),
    ))
}

const PRELUDE: &str = "function t(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.name + ': ' + e.message; } }";

fn runtime(stress: bool) -> Runtime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(config).unwrap();
    generated::define_classes::<TestHost>(&mut rt).unwrap();
    rt.set_host_data(TestHost::default());
    generated::create_interfaces::<TestHost>(&mut rt, 0).unwrap();
    rt.define_global_function("makeNode", 0, make_node).unwrap();
    rt.define_global_function("makeList", 0, make_list).unwrap();
    rt.eval(PRELUDE).unwrap();
    rt
}

fn check(cases: &[(&str, &str)]) {
    let cases: Vec<(String, String)> = cases
        .iter()
        .map(|&(s, e)| (s.to_owned(), e.to_owned()))
        .collect();
    let failures = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let mut failures = Vec::new();
            for (source, expected) in &cases {
                for stress in [false, true] {
                    let mut rt = runtime(stress);
                    let actual = match rt.eval(source) {
                        Ok(value) => rt.to_rust_string(value).unwrap_or_default(),
                        Err(error) => format!("error: {error}"),
                    };
                    if actual != *expected {
                        failures.push(format!(
                            "{source} (stress {stress})\n  expected: {expected}\n  actual:   {actual}"
                        ));
                    }
                    assert_eq!(rt.heap().stats().total_stale_roots, 0, "{source}");
                }
            }
            failures
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn node_and_event_target_interfaces() {
    check(&[
        // Chromium's list without ownerDocument, parentElement and
        // childNodes (feature 4c adds them).
        (
            "JSON.stringify(Object.getOwnPropertyNames(Node.prototype))",
            "[\"nodeType\",\"nodeName\",\"baseURI\",\"isConnected\",\"parentNode\",\"firstChild\",\"lastChild\",\"previousSibling\",\"nextSibling\",\"nodeValue\",\"textContent\",\"ELEMENT_NODE\",\"ATTRIBUTE_NODE\",\"TEXT_NODE\",\"CDATA_SECTION_NODE\",\"ENTITY_REFERENCE_NODE\",\"ENTITY_NODE\",\"PROCESSING_INSTRUCTION_NODE\",\"COMMENT_NODE\",\"DOCUMENT_NODE\",\"DOCUMENT_TYPE_NODE\",\"DOCUMENT_FRAGMENT_NODE\",\"NOTATION_NODE\",\"DOCUMENT_POSITION_DISCONNECTED\",\"DOCUMENT_POSITION_PRECEDING\",\"DOCUMENT_POSITION_FOLLOWING\",\"DOCUMENT_POSITION_CONTAINS\",\"DOCUMENT_POSITION_CONTAINED_BY\",\"DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC\",\"appendChild\",\"cloneNode\",\"compareDocumentPosition\",\"contains\",\"getRootNode\",\"hasChildNodes\",\"insertBefore\",\"isDefaultNamespace\",\"isEqualNode\",\"isSameNode\",\"lookupNamespaceURI\",\"lookupPrefix\",\"normalize\",\"removeChild\",\"replaceChild\",\"constructor\"]",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyNames(Node))",
            "[\"length\",\"name\",\"prototype\",\"ELEMENT_NODE\",\"ATTRIBUTE_NODE\",\"TEXT_NODE\",\"CDATA_SECTION_NODE\",\"ENTITY_REFERENCE_NODE\",\"ENTITY_NODE\",\"PROCESSING_INSTRUCTION_NODE\",\"COMMENT_NODE\",\"DOCUMENT_NODE\",\"DOCUMENT_TYPE_NODE\",\"DOCUMENT_FRAGMENT_NODE\",\"NOTATION_NODE\",\"DOCUMENT_POSITION_DISCONNECTED\",\"DOCUMENT_POSITION_PRECEDING\",\"DOCUMENT_POSITION_FOLLOWING\",\"DOCUMENT_POSITION_CONTAINS\",\"DOCUMENT_POSITION_CONTAINED_BY\",\"DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC\"]",
        ),
        // Chromium also has `when` (Observable API).
        (
            "JSON.stringify(Object.getOwnPropertyNames(EventTarget.prototype)) + JSON.stringify(Object.getOwnPropertyNames(EventTarget))",
            "[\"addEventListener\",\"dispatchEvent\",\"removeEventListener\",\"constructor\"][\"length\",\"name\",\"prototype\"]",
        ),
        (
            "['appendChild', 'insertBefore', 'replaceChild', 'removeChild', 'cloneNode', 'isEqualNode', 'isSameNode', 'compareDocumentPosition', 'contains', 'lookupPrefix', 'lookupNamespaceURI', 'isDefaultNamespace', 'hasChildNodes', 'normalize', 'getRootNode'].map((n) => n + ' ' + Node.prototype[n].length).join()",
            "appendChild 1,insertBefore 2,replaceChild 2,removeChild 1,cloneNode 0,isEqualNode 1,isSameNode 1,compareDocumentPosition 1,contains 1,lookupPrefix 1,lookupNamespaceURI 1,isDefaultNamespace 1,hasChildNodes 0,normalize 0,getRootNode 0",
        ),
        (
            "['addEventListener', 'removeEventListener', 'dispatchEvent'].map((n) => n + ' ' + EventTarget.prototype[n].length + ' ' + EventTarget.prototype[n].name).join()",
            "addEventListener 2 addEventListener,removeEventListener 2 removeEventListener,dispatchEvent 1 dispatchEvent",
        ),
        (
            "['nodeType', 'nodeName', 'baseURI', 'isConnected', 'parentNode', 'firstChild', 'lastChild', 'previousSibling', 'nextSibling', 'nodeValue', 'textContent'].map((n) => { const d = Object.getOwnPropertyDescriptor(Node.prototype, n); return [n, typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name, d.set ? d.set.name + ' ' + d.set.length : '-'].join(' '); }).join(',')",
            "nodeType function undefined true true get nodeType -,nodeName function undefined true true get nodeName -,baseURI function undefined true true get baseURI -,isConnected function undefined true true get isConnected -,parentNode function undefined true true get parentNode -,firstChild function undefined true true get firstChild -,lastChild function undefined true true get lastChild -,previousSibling function undefined true true get previousSibling -,nextSibling function undefined true true get nextSibling -,nodeValue function function true true get nodeValue set nodeValue 1,textContent function function true true get textContent set textContent 1",
        ),
        (
            "(() => { const d = Object.getOwnPropertyDescriptor(Node.prototype, 'ELEMENT_NODE'); const e = Object.getOwnPropertyDescriptor(Node, 'DOCUMENT_POSITION_CONTAINED_BY'); return [d.value, d.writable, d.enumerable, d.configurable, e.value, e.writable, e.enumerable, e.configurable].join(); })()",
            "1,false,true,false,16,false,true,false",
        ),
        (
            "[EventTarget.length, typeof new EventTarget(), Object.prototype.toString.call(new EventTarget()), Object.getPrototypeOf(Node.prototype) === EventTarget.prototype, Object.getPrototypeOf(Node) === EventTarget, Object.getPrototypeOf(EventTarget.prototype) === Object.prototype, Object.getPrototypeOf(EventTarget) === Function.prototype].join()",
            "0,object,[object EventTarget],true,true,true,true",
        ),
        (
            "(() => { const d = Object.getOwnPropertyDescriptor(Node.prototype, 'appendChild'); return [d.writable, d.enumerable, d.configurable, String(d.value), Object.prototype.hasOwnProperty.call(d.value, 'prototype'), t(() => new d.value())].join('|'); })()",
            "true|true|true|function appendChild() { [native code] }|false|TypeError: d.value is not a constructor",
        ),
        (
            "[JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'Node')), t(() => new Node()), t(() => Node()), t(() => EventTarget()), String(Node)].join('|')",
            "{\"writable\":true,\"enumerable\":false,\"configurable\":true}|TypeError: Failed to construct 'Node': Illegal constructor|TypeError: Illegal constructor|TypeError: Failed to construct 'EventTarget': Please use the 'new' operator, this DOM object constructor cannot be called as a function.|function Node() { [native code] }",
        ),
        (
            "class T extends EventTarget {} const x = new T(); [x instanceof T, x instanceof EventTarget, typeof x.addEventListener].join()",
            "true,true,function",
        ),
    ]);
}

/// `NodeList` (DOM §4.2.10.1) as Chromium 148 shows it
/// (`tools/probes/web-legacy.json`).
#[test]
fn node_list_interface() {
    check(&[
        (
            "[Object.getOwnPropertyNames(NodeList.prototype), Reflect.ownKeys(NodeList.prototype).map(String)].join(' | ')",
            "entries,keys,values,forEach,length,item,constructor | entries,keys,values,forEach,length,item,constructor,Symbol(Symbol.toStringTag),Symbol(Symbol.iterator)",
        ),
        (
            "const p = NodeList.prototype; [p[Symbol.iterator] === Array.prototype.values, p.forEach === Array.prototype.forEach, p.entries === Array.prototype.entries, p.keys === Array.prototype.keys, p.values === Array.prototype.values, p.item.length, t(() => new NodeList())].join()",
            "true,true,true,true,true,1,TypeError: Failed to construct 'NodeList': Illegal constructor",
        ),
        (
            "const a = makeNode(); const l = makeList(a, makeNode()); [Object.keys(l), l[0] === a, l.item(0) === a, l.length, l[2], [...l].length, 1 in l, Object.prototype.toString.call(l)].join(' | ')",
            "0,1 | true | true | 2 |  | 2 | true | [object NodeList]",
        ),
        (
            "const l = makeList(makeNode()); [t(() => { 'use strict'; l[0] = 1; }), t(() => { 'use strict'; delete l[0]; }), t(() => Object.defineProperty(l, 0, { value: 1 })), t(() => Object.freeze(l))].join(' | ')",
            "TypeError: Failed to set an indexed property [0] on 'NodeList': Indexed property setter is not supported. | TypeError: Failed to delete an indexed property [0] from 'NodeList': Index property deleter is not supported. | TypeError: Failed to set an indexed property [0] on 'NodeList': Index property setter is not supported. | TypeError: Cannot freeze",
        ),
    ]);
}

#[test]
fn glue_checks_of_node_and_event_target() {
    check(&[
        (
            "const n = makeNode(); [t(() => n.appendChild()), t(() => n.appendChild({})), t(() => n.appendChild(null)), t(() => n.appendChild(undefined)), t(() => n.insertBefore()), t(() => n.insertBefore(makeNode())), t(() => n.insertBefore(makeNode(), {})), t(() => n.insertBefore(makeNode(), undefined).nodeName)].join('|')",
            "TypeError: Failed to execute 'appendChild' on 'Node': 1 argument required, but only 0 present.|TypeError: Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.|TypeError: Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.|TypeError: Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.|TypeError: Failed to execute 'insertBefore' on 'Node': 2 arguments required, but only 0 present.|TypeError: Failed to execute 'insertBefore' on 'Node': 2 arguments required, but only 1 present.|TypeError: Failed to execute 'insertBefore' on 'Node': parameter 2 is not of type 'Node'.|DIV",
        ),
        (
            "const n = makeNode(); [t(() => Node.prototype.isSameNode.call(n)), t(() => n.contains()), t(() => n.contains(undefined)), t(() => n.contains(5)), n.contains(n)].join('|')",
            "TypeError: Failed to execute 'isSameNode' on 'Node': 1 argument required, but only 0 present.|TypeError: Failed to execute 'contains' on 'Node': 1 argument required, but only 0 present.|false|TypeError: Failed to execute 'contains' on 'Node': parameter 1 is not of type 'Node'.|true",
        ),
        (
            "const n = makeNode(); try { n.appendChild(n); } catch (e) { [e instanceof DOMException, e.name, e.code, e.message, String(e)].join('|') }",
            "true|HierarchyRequestError|3|Failed to execute 'appendChild' on 'Node': The new child element contains the parent.|HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent.",
        ),
        (
            "const n = makeNode(); [t(() => n.addEventListener('x', 5)), t(() => n.addEventListener('x', 'str')), t(() => n.addEventListener('x', null)), t(() => n.addEventListener('x')), t(() => n.addEventListener('x', {})), t(() => n.addEventListener()), t(() => n.addEventListener('x', () => 1, 7))].join('|')",
            "TypeError: Failed to execute 'addEventListener' on 'EventTarget': parameter 2 is not of type 'Object'.|TypeError: Failed to execute 'addEventListener' on 'EventTarget': parameter 2 is not of type 'Object'.|undefined|TypeError: Failed to execute 'addEventListener' on 'EventTarget': 2 arguments required, but only 1 present.|undefined|TypeError: Failed to execute 'addEventListener' on 'EventTarget': 2 arguments required, but only 0 present.|undefined",
        ),
        (
            "const f = Object.getOwnPropertyDescriptor(Node.prototype, 'nodeType').get; [t(() => f.call(5)), t(() => f.call('s')), t(() => Node.prototype.appendChild.call(5, makeNode())), t(() => f.call(new EventTarget())), f.call(makeNode())].join('|')",
            "TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Illegal invocation|1",
        ),
        (
            "const n = makeNode(); [n.nodeType, n.parentNode, n.getRootNode() === n, n.isSameNode(n), n.isSameNode(null), n.textContent, n.nodeValue].map(String).join()",
            "1,null,true,true,false,,null",
        ),
        (
            "[t(() => new EventTarget().dispatchEvent()), t(() => new EventTarget().dispatchEvent({})), new EventTarget().dispatchEvent(new Event('x'))].join('|')",
            "TypeError: Failed to execute 'dispatchEvent' on 'EventTarget': 1 argument required, but only 0 present.|TypeError: Failed to execute 'dispatchEvent' on 'EventTarget': parameter 1 is not of type 'Event'.|true",
        ),
    ]);
}

#[test]
fn event_interface() {
    check(&[
        (
            "[t(() => new Event('x', 5)), t(() => new Event('x', null).bubbles), t(() => new Event('x', undefined).bubbles), t(() => new Event('x', { bubbles: 1 }).bubbles), t(() => new Event('x', 'str')), t(() => new Event('x', () => 1).bubbles), t(() => new Event())].join('|')",
            "TypeError: Failed to construct 'Event': The provided value is not of type 'EventInit'.|false|false|true|TypeError: Failed to construct 'Event': The provided value is not of type 'EventInit'.|false|TypeError: Failed to construct 'Event': 1 argument required, but only 0 present.",
        ),
        (
            "const order = []; const init = {}; for (const k of ['composed', 'cancelable', 'bubbles']) Object.defineProperty(init, k, { get() { order.push(k); return 1; } }); const e = new Event('x', init); [order.join(), e.type, e.bubbles, e.cancelable, e.composed].join('|')",
            "bubbles,cancelable,composed|x|true|true|true",
        ),
        (
            "t(() => new Event('x', { get bubbles() { throw new SyntaxError('in getter'); } }))",
            "SyntaxError: in getter",
        ),
        (
            "(() => { const e = new Event('x'); const d = Object.getOwnPropertyDescriptor(e, 'isTrusted'); const o = Object.getOwnPropertyDescriptor(new Event('y'), 'isTrusted'); return [JSON.stringify(Object.getOwnPropertyNames(e)), typeof d.get, d.enumerable, d.configurable, d.get.name, Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted') === undefined, Event.length, d.get === o.get].join('|'); })()",
            "[\"isTrusted\"]|function|true|false|get isTrusted|true|1|true",
        ),
        // Spec order of the attributes (Chromium lists its own IDL order).
        (
            "JSON.stringify(Object.getOwnPropertyNames(Event.prototype))",
            "[\"type\",\"target\",\"srcElement\",\"currentTarget\",\"eventPhase\",\"cancelBubble\",\"bubbles\",\"cancelable\",\"returnValue\",\"defaultPrevented\",\"composed\",\"timeStamp\",\"NONE\",\"CAPTURING_PHASE\",\"AT_TARGET\",\"BUBBLING_PHASE\",\"composedPath\",\"initEvent\",\"preventDefault\",\"stopImmediatePropagation\",\"stopPropagation\",\"constructor\"]",
        ),
        (
            "class C extends Event {} const c = new C('q'); [c instanceof C, c.type, Object.getOwnPropertyNames(c).join()].join()",
            "true,q,isTrusted",
        ),
        ("JSON.stringify(new Event('x').composedPath())", "[]"),
    ]);
}

#[test]
fn dom_exception() {
    check(&[
        (
            "(() => { const e = new DOMException(); return [e.name, e.message, e.code, String(e), Object.prototype.toString.call(e)].join('|'); })()",
            "Error||0|Error|[object DOMException]",
        ),
        (
            "(() => { const e = new DOMException('m', 'SyntaxError'); return [e.name, e.message, e.code, String(e), new DOMException('m', 'Bogus').code, new DOMException(undefined, undefined).name, new DOMException(null, null).message, new DOMException(1, 2).name].join('|'); })()",
            "SyntaxError|m|12|SyntaxError: m|0|Error|null|2",
        ),
        (
            "['IndexSizeError', 'HierarchyRequestError', 'WrongDocumentError', 'InvalidCharacterError', 'NoModificationAllowedError', 'NotFoundError', 'NotSupportedError', 'InUseAttributeError', 'InvalidStateError', 'SyntaxError', 'InvalidModificationError', 'NamespaceError', 'InvalidAccessError', 'TypeMismatchError', 'SecurityError', 'NetworkError', 'AbortError', 'URLMismatchError', 'QuotaExceededError', 'TimeoutError', 'InvalidNodeTypeError', 'DataCloneError', 'EncodingError', 'NotReadableError', 'OperationError', 'ValidationError'].map((n) => new DOMException('', n).code).join()",
            "1,3,4,5,7,8,9,10,11,12,13,14,15,17,18,19,20,21,22,23,24,25,0,0,0,0",
        ),
        (
            "(() => { const e = new DOMException('m'); return [e instanceof Error, e instanceof DOMException, Object.getPrototypeOf(DOMException.prototype) === Error.prototype, Object.getPrototypeOf(DOMException) === Function.prototype, DOMException.length, DOMException.name, Error.prototype.isPrototypeOf(e)].join(); })()",
            "true,true,true,true,0,DOMException,true",
        ),
        (
            "(() => { const e = new DOMException('m', 'NotFoundError'); return [JSON.stringify(Object.getOwnPropertyNames(e)), JSON.stringify(Object.getOwnPropertyNames(DOMException)), typeof e.stack, 'stack' in e].join('|'); })()",
            "[]|[\"length\",\"name\",\"prototype\",\"INDEX_SIZE_ERR\",\"DOMSTRING_SIZE_ERR\",\"HIERARCHY_REQUEST_ERR\",\"WRONG_DOCUMENT_ERR\",\"INVALID_CHARACTER_ERR\",\"NO_DATA_ALLOWED_ERR\",\"NO_MODIFICATION_ALLOWED_ERR\",\"NOT_FOUND_ERR\",\"NOT_SUPPORTED_ERR\",\"INUSE_ATTRIBUTE_ERR\",\"INVALID_STATE_ERR\",\"SYNTAX_ERR\",\"INVALID_MODIFICATION_ERR\",\"NAMESPACE_ERR\",\"INVALID_ACCESS_ERR\",\"VALIDATION_ERR\",\"TYPE_MISMATCH_ERR\",\"SECURITY_ERR\",\"NETWORK_ERR\",\"ABORT_ERR\",\"URL_MISMATCH_ERR\",\"QUOTA_EXCEEDED_ERR\",\"TIMEOUT_ERR\",\"INVALID_NODE_TYPE_ERR\",\"DATA_CLONE_ERR\"]|undefined|false",
        ),
        // Chromium lists code, name, message (its IDL order); swb the
        // specification's order.
        (
            "JSON.stringify(Object.getOwnPropertyNames(DOMException.prototype).slice(0, 4)) + Object.getOwnPropertyNames(DOMException.prototype).pop()",
            "[\"name\",\"message\",\"code\",\"INDEX_SIZE_ERR\"]constructor",
        ),
        (
            "(() => { const d = Object.getOwnPropertyDescriptor(DOMException.prototype, 'name'); const c = Object.getOwnPropertyDescriptor(DOMException, 'SYNTAX_ERR'); const p = Object.getOwnPropertyDescriptor(DOMException.prototype, 'SYNTAX_ERR'); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, c.value, c.writable, c.enumerable, c.configurable, p.value, p.writable, p.enumerable, p.configurable].join(); })()",
            "function,,true,true,get name,12,false,true,false,12,false,true,false",
        ),
        (
            "[t(() => DOMException()), t(() => Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call({})), t(() => Object.getOwnPropertyDescriptor(DOMException.prototype, 'message').get.call(new Error('x'))), t(() => new DOMException(Symbol()))].join('|')",
            "TypeError: Failed to construct 'DOMException': Please use the 'new' operator, this DOM object constructor cannot be called as a function.|TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Failed to construct 'DOMException': Cannot convert a Symbol value to a string",
        ),
        (
            "(() => { class E extends DOMException { constructor() { super('sub', 'AbortError'); } } const e = new E(); return [e instanceof E, e.name, e.code, Object.getPrototypeOf(e) === E.prototype].join(); })()",
            "true,AbortError,20,true",
        ),
        (
            "(() => { const e = new DOMException('m', 'AbortError'); e.name = 'x'; return [e.name, Object.getOwnPropertyNames(e).length, Object.prototype.toString.call(DOMException.prototype)].join(); })()",
            "AbortError,0,[object DOMException]",
        ),
    ]);
}
