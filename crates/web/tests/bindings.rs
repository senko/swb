//! The generated glue of the test IDL (`tests/idl/test.idl`, generated
//! into `tests/generated/`) with a test implementation, run through
//! `swb_js::Runtime`: the conversions of each type and extended attribute,
//! Chromium's error messages, overloads, `[Reflect]`, dictionaries,
//! enumerations, callbacks, unions, sequences, records, promises,
//! `[Global]`, `[LegacyUnforgeable]`, `[Replaceable]`, `[PutForwards]`.
//! Every case runs with and without the GC stress mode. The expected
//! values come from Chromium where a conversion is observable with a real
//! API (`tools/probes/web-bindings.json`); the rest follow Web IDL.

#![allow(
    clippy::unwrap_used,
    clippy::many_single_char_names,
    clippy::unnecessary_wraps,
    reason = "a test host: the trait methods take the arguments of the test IDL"
)]

#[rustfmt::skip]
#[allow(dead_code, unreachable_pub, unused_qualifications, clippy::all, clippy::pedantic)]
mod generated;

use generated::types::{
    LongOrBoolean, LongOrDomString, LongSequenceOrLongRecordOrDomString, TestCallback,
    TestCallbackOrDomString, TestDict, TestElementOrDomString, TestListener, TestMode, TestOptions,
    TestOptionsOrBoolean,
};
use swb_js::{
    Gc, GenericData, HeapConfig, HostData, HostState, NativeCall, NativeReturn, Object, Runtime,
    RuntimeConfig, Tracer, Value, VmError, VmResult,
};
use swb_web::bindings::{self as b, Cx, Exception, GlueState, Host, NodeId, Platform};

/// A node of the fake document.
struct FakeNode {
    name: String,
    element: bool,
    attributes: Vec<(String, String)>,
}

/// The host state of the test runtime.
#[derive(Default)]
struct TestHost {
    glue: GlueState,
    nodes: Vec<FakeNode>,
    wrappers: Vec<Option<Gc<Object>>>,
    kept: Vec<Value>,
    global_value: i32,
}

impl HostState for TestHost {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        self.glue.trace(tracer);
        for wrapper in self.wrappers.iter().flatten() {
            tracer.object(*wrapper);
        }
        for value in &self.kept {
            tracer.value(*value);
        }
    }
}

/// The data of a `Tester`.
struct TesterData {
    label: String,
    counter: i32,
    element: Option<NodeId>,
}

impl GenericData for TesterData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.label.capacity()
    }
}

fn host(rt: &mut Runtime) -> b::Result<&mut TestHost> {
    rt.host_data_mut::<TestHost>()
        .ok_or_else(|| Exception::type_error("no host"))
}

fn new_node(rt: &mut Runtime, name: &str, element: bool) -> b::Result<NodeId> {
    let host = host(rt)?;
    host.nodes.push(FakeNode {
        name: name.to_owned(),
        element,
        attributes: Vec::new(),
    });
    NodeId::from_index(host.nodes.len() - 1).ok_or_else(|| Exception::type_error("too many nodes"))
}

fn node_mut(rt: &mut Runtime, id: NodeId) -> b::Result<&mut FakeNode> {
    host(rt)?
        .nodes
        .get_mut(id.index())
        .ok_or_else(|| Exception::type_error("no node"))
}

fn tester(rt: &mut Runtime, this: Platform) -> b::Result<&mut TesterData> {
    let HostData::Generic(data) = this.data else {
        return Err(Exception::type_error("not a tester"));
    };
    Ok(rt
        .heap_mut()
        .generic_mut::<TesterData>(data)
        .map_err(VmError::from)?)
}

impl Host for TestHost {
    fn glue_state(&mut self) -> &mut GlueState {
        &mut self.glue
    }

    fn wrap_node(rt: &mut Runtime, realm: u32, node: NodeId) -> VmResult<Gc<Object>> {
        let state = rt
            .host_data_mut::<TestHost>()
            .ok_or(VmError::type_error("no host"))?;
        if let Some(Some(wrapper)) = state.wrappers.get(node.index()) {
            return Ok(*wrapper);
        }
        let element = state.nodes.get(node.index()).is_some_and(|n| n.element);
        let class = if element {
            generated::TEST_ELEMENT
        } else {
            generated::NODE
        };
        let wrapper = rt.new_host_object(realm, class, b::node_data(node))?;
        b::glue::install_unforgeables::<TestHost>(rt, realm, class, wrapper)?;
        let state = rt
            .host_data_mut::<TestHost>()
            .ok_or(VmError::type_error("no host"))?;
        if state.wrappers.len() <= node.index() {
            state.wrappers.resize(node.index() + 1, None);
        }
        if let Some(slot) = state.wrappers.get_mut(node.index()) {
            *slot = Some(wrapper);
        }
        Ok(wrapper)
    }

    fn reflect_get(cx: &mut Cx<'_>, element: NodeId, name: &str) -> b::Result<Option<String>> {
        let node = node_mut(cx.rt, element)?;
        Ok(node
            .attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone()))
    }

    fn reflect_set(cx: &mut Cx<'_>, element: NodeId, name: &str, value: &str) -> b::Result<()> {
        let node = node_mut(cx.rt, element)?;
        match node.attributes.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => value.clone_into(&mut slot.1),
            None => node.attributes.push((name.to_owned(), value.to_owned())),
        }
        Ok(())
    }

    fn reflect_remove(cx: &mut Cx<'_>, element: NodeId, name: &str) -> b::Result<()> {
        node_mut(cx.rt, element)?
            .attributes
            .retain(|(n, _)| n != name);
        Ok(())
    }

    fn reflect_url(_cx: &mut Cx<'_>, _element: NodeId, value: &str) -> Option<String> {
        if value.contains(' ') {
            None
        } else if value.contains(':') {
            Some(value.to_owned())
        } else {
            Some(format!("http://example.test/{value}"))
        }
    }
}

impl generated::dom_exception::DOMExceptionImpl for TestHost {
    fn constructor(cx: &mut Cx<'_>, message: String, name: String) -> b::Result<Gc<Object>> {
        b::dom_exception::construct::<TestHost>(cx, message, name)
    }

    fn name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        b::dom_exception::name(cx, this)
    }

    fn message(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        b::dom_exception::message(cx, this)
    }

    fn code(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        b::dom_exception::code(cx, this)
    }
}

impl generated::window::WindowImpl for TestHost {
    fn global_value(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(host(cx.rt)?.global_value)
    }

    fn set_global_value(cx: &mut Cx<'_>, _this: Platform, value: i32) -> b::Result<()> {
        host(cx.rt)?.global_value = value;
        Ok(())
    }

    fn fixed(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("fixed".to_owned())
    }

    fn global_op(_cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(format!("global {:?}", this.data))
    }
}

impl generated::node::NodeImpl for TestHost {
    fn node_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(node_mut(cx.rt, this)?.name.clone())
    }
}

impl generated::test_element::TestElementImpl for TestHost {
    fn autocapitalize(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let value = <TestHost as Host>::reflect_get(cx, this, "autocapitalize")?;
        Ok(format!("got {}", value.unwrap_or_default()))
    }
}

fn debug_mode(mode: TestMode) -> &'static str {
    mode.as_str()
}

impl generated::tester::TesterImpl for TestHost {
    fn counter(cx: &mut Cx<'_>, this: Platform) -> b::Result<i32> {
        Ok(tester(cx.rt, this)?.counter)
    }

    fn set_counter(cx: &mut Cx<'_>, this: Platform, value: i32) -> b::Result<()> {
        tester(cx.rt, this)?.counter = value;
        Ok(())
    }

    fn label(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(tester(cx.rt, this)?.label.clone())
    }

    fn replaceable(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(1)
    }

    fn self_(_cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(this.object)
    }

    fn trusted(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(true)
    }

    fn element(cx: &mut Cx<'_>, this: Platform) -> b::Result<NodeId> {
        if let Some(id) = tester(cx.rt, this)?.element {
            return Ok(id);
        }
        let id = new_node(cx.rt, "kept", true)?;
        tester(cx.rt, this)?.element = Some(id);
        Ok(id)
    }

    fn constructor(cx: &mut Cx<'_>, label: String) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(TesterData {
            label,
            counter: 0,
            element: None,
        })?;
        Ok(cx.new_instance::<TestHost>(data)?)
    }

    fn integers(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: i8,
        b: u8,
        c: i16,
        d: u16,
        e: i32,
        f: u32,
        g: i64,
        h: u64,
    ) -> b::Result<String> {
        Ok(format!("{a} {b} {c} {d} {e} {f} {g} {h}"))
    }

    fn clamped(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: u8,
        b: i32,
        c: u16,
        d: i64,
    ) -> b::Result<String> {
        Ok(format!("{a} {b} {c} {d}"))
    }

    fn enforced(_cx: &mut Cx<'_>, _this: Platform, a: i32, b: u32, c: i64) -> b::Result<String> {
        Ok(format!("{a} {b} {c}"))
    }

    fn floats(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: f32,
        b: f32,
        c: f64,
        d: f64,
    ) -> b::Result<String> {
        Ok(format!("{a} {b} {c} {d}"))
    }

    fn strings(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: String,
        b: String,
        c: String,
        d: String,
    ) -> b::Result<String> {
        Ok(format!("{a}|{b}|{c}|{d}"))
    }

    fn nullables(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: Option<i32>,
        b: Option<String>,
        c: Option<NodeId>,
    ) -> b::Result<String> {
        Ok(format!(
            "{a:?} {b:?} {}",
            c.map_or(-1, |c| c.index() as i64)
        ))
    }

    fn optionals(
        cx: &mut Cx<'_>,
        _this: Platform,
        a: i32,
        b: String,
        c: Option<bool>,
        d: TestMode,
        e: Value,
        f: f64,
        g: Vec<i32>,
    ) -> b::Result<String> {
        let e = cx.rt.display(e);
        Ok(format!("{a} {b} {c:?} {} {e} {f} {g:?}", debug_mode(d)))
    }

    fn variadic(
        _cx: &mut Cx<'_>,
        _this: Platform,
        first: String,
        rest: Vec<i32>,
    ) -> b::Result<String> {
        Ok(format!("{first} {rest:?}"))
    }

    fn sequences(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: Vec<i32>,
        b: Vec<Vec<String>>,
    ) -> b::Result<String> {
        Ok(format!("{a:?} {b:?}"))
    }

    fn records(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: Vec<(String, i32)>,
        b: Vec<(String, NodeId)>,
    ) -> b::Result<String> {
        let b: Vec<(String, usize)> = b.into_iter().map(|(k, v)| (k, v.index())).collect();
        Ok(format!("{a:?} {b:?}"))
    }

    fn dictionary(cx: &mut Cx<'_>, _this: Platform, d: TestDict) -> b::Result<String> {
        let extra = d.extra.map(|v| cx.rt.display(v));
        let node = d.node.map(|n| n.map(NodeId::index));
        Ok(format!(
            "base={} name={} flag={} clamped={:?} enforced={:?} mode={} list={:?} map={:?} node={node:?} extra={extra:?} ratio={:?}",
            d.base,
            d.name,
            d.flag,
            d.clamped,
            d.enforced,
            debug_mode(d.mode),
            d.list,
            d.map,
            d.ratio
        ))
    }

    fn options(_cx: &mut Cx<'_>, _this: Platform, o: TestOptions) -> b::Result<String> {
        Ok(format!("capture={} label={:?}", o.capture, o.label))
    }

    fn echo_mode(_cx: &mut Cx<'_>, _this: Platform, m: TestMode) -> b::Result<TestMode> {
        Ok(m)
    }

    fn callback_function(cx: &mut Cx<'_>, _this: Platform, cb: TestCallback) -> b::Result<String> {
        let realm = cx.realm();
        cb.invoke::<TestHost>(cx.rt, realm, Value::Undefined, 42, Some("s".to_owned()))
    }

    fn callback_interface(
        cx: &mut Cx<'_>,
        this: Platform,
        listener: TestListener,
    ) -> b::Result<String> {
        let realm = cx.realm();
        listener.handle::<TestHost>(cx.rt, realm, this.object.into(), "x".to_owned())?;
        Ok("ok".to_owned())
    }

    fn promised(_cx: &mut Cx<'_>, _this: Platform, p: Gc<Object>) -> b::Result<Gc<Object>> {
        Ok(p)
    }

    fn rejects(_cx: &mut Cx<'_>, _this: Platform, kind: String) -> b::Result<Gc<Object>> {
        Err(match kind.as_str() {
            "dom" => Exception::dom("AbortError", "stopped"),
            "range" => Exception::range_error("too far"),
            _ => Exception::type_error("bad"),
        })
    }

    fn echo_any(_cx: &mut Cx<'_>, _this: Platform, value: Value) -> b::Result<Value> {
        Ok(value)
    }

    fn echo_object(_cx: &mut Cx<'_>, _this: Platform, value: Gc<Object>) -> b::Result<Gc<Object>> {
        Ok(value)
    }

    fn unions(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: TestElementOrDomString,
        b: LongOrBoolean,
        c: LongSequenceOrLongRecordOrDomString,
        d: TestOptionsOrBoolean,
    ) -> b::Result<String> {
        let a = match a {
            TestElementOrDomString::TestElement(id) => format!("element {}", id.index()),
            TestElementOrDomString::DomString(s) => format!("string {s}"),
        };
        let b = match b {
            LongOrBoolean::Long(n) => format!("long {n}"),
            LongOrBoolean::Boolean(v) => format!("boolean {v}"),
        };
        let c = match c {
            LongSequenceOrLongRecordOrDomString::LongSequence(v) => format!("sequence {v:?}"),
            LongSequenceOrLongRecordOrDomString::LongRecord(v) => format!("record {v:?}"),
            LongSequenceOrLongRecordOrDomString::DomString(s) => format!("string {s}"),
        };
        let d = match d {
            TestOptionsOrBoolean::TestOptions(o) => format!("options {}", o.capture),
            TestOptionsOrBoolean::Boolean(v) => format!("boolean {v}"),
        };
        Ok(format!("{a}; {b}; {c}; {d}"))
    }

    fn nullable_union(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: Option<LongOrDomString>,
    ) -> b::Result<String> {
        Ok(match a {
            None => "null".to_owned(),
            Some(LongOrDomString::Long(n)) => format!("long {n}"),
            Some(LongOrDomString::DomString(s)) => format!("string {s}"),
        })
    }

    fn handler(_cx: &mut Cx<'_>, _this: Platform, h: TestCallbackOrDomString) -> b::Result<String> {
        Ok(match h {
            TestCallbackOrDomString::TestCallback(_) => "callback".to_owned(),
            TestCallbackOrDomString::DomString(s) => format!("code {s}"),
        })
    }

    fn over_1(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("none".to_owned())
    }

    fn over_2(_cx: &mut Cx<'_>, _this: Platform, a: i32) -> b::Result<String> {
        Ok(format!("long {a}"))
    }

    fn over_3(_cx: &mut Cx<'_>, _this: Platform, a: String, b: i32) -> b::Result<String> {
        Ok(format!("string {a} {b}"))
    }

    fn over_4(_cx: &mut Cx<'_>, _this: Platform, a: NodeId, b: i32) -> b::Result<String> {
        Ok(format!("node {} {b}", a.index()))
    }

    fn over_5(_cx: &mut Cx<'_>, _this: Platform, a: Vec<i32>, b: i32) -> b::Result<String> {
        Ok(format!("sequence {a:?} {b}"))
    }

    fn over_6(_cx: &mut Cx<'_>, _this: Platform, a: TestOptions, b: i32) -> b::Result<String> {
        Ok(format!("options {} {b}", a.capture))
    }

    fn over_7(
        _cx: &mut Cx<'_>,
        _this: Platform,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
    ) -> b::Result<String> {
        Ok(format!("four {a} {b} {c} {d}"))
    }

    fn pick_1(_cx: &mut Cx<'_>, _this: Platform, a: NodeId) -> b::Result<String> {
        Ok(format!("node {}", a.index()))
    }

    fn pick_2(_cx: &mut Cx<'_>, _this: Platform, _a: TestCallback) -> b::Result<String> {
        Ok("callback".to_owned())
    }

    fn elements(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<NodeId>> {
        let count = host(cx.rt)?.nodes.len();
        if count < 2 {
            new_node(cx.rt, "a", true)?;
            new_node(cx.rt, "b", true)?;
        }
        Ok(vec![
            NodeId::from_index(0).ok_or_else(|| Exception::type_error("node"))?,
            NodeId::from_index(1).ok_or_else(|| Exception::type_error("node"))?,
        ])
    }

    fn record_out(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<(String, i32)>> {
        Ok(vec![("b".to_owned(), 2), ("a".to_owned(), 1)])
    }

    fn dict_out(cx: &mut Cx<'_>, _this: Platform) -> b::Result<TestDict> {
        let realm = cx.realm();
        let text = cx.rt.string_value("made")?;
        let object = cx.rt.new_plain_object(realm)?;
        b::to_js::dictionary_member(cx.rt, object, "name", text)?;
        let mut dict = TestDict::from_js(cx.rt, realm, object.into())?;
        dict.list = Some(vec!["x".to_owned()]);
        dict.node = Some(None);
        Ok(dict)
    }

    fn union_out(_cx: &mut Cx<'_>, _this: Platform, number: bool) -> b::Result<LongOrDomString> {
        Ok(if number {
            LongOrDomString::Long(5)
        } else {
            LongOrDomString::DomString("five".to_owned())
        })
    }

    fn maybe_list(
        _cx: &mut Cx<'_>,
        _this: Platform,
        present: bool,
    ) -> b::Result<Option<Vec<String>>> {
        Ok(present.then(|| vec!["p".to_owned(), "q".to_owned()]))
    }

    fn maybe_element(cx: &mut Cx<'_>, _this: Platform, present: bool) -> b::Result<Option<NodeId>> {
        if !present {
            return Ok(None);
        }
        Ok(Some(new_node(cx.rt, "maybe", true)?))
    }

    fn throw_dom(
        _cx: &mut Cx<'_>,
        _this: Platform,
        name: String,
        message: String,
    ) -> b::Result<()> {
        let name = b::dom_exception::NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map_or("NotSupportedError", |(n, _)| *n);
        Err(Exception::dom(name, message))
    }

    fn throw_type(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Err(Exception::type_error("wrong type"))
    }

    fn throw_range(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Err(Exception::range_error("out of range"))
    }

    fn custom(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
        let value = rt.arg(call, 0);
        let n = rt.number_of(value)?;
        Ok(NativeReturn::Value(Value::number(n * 2.0)))
    }

    fn version(_cx: &mut Cx<'_>) -> b::Result<String> {
        Ok("1".to_owned())
    }

    fn stringify(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(format!("Tester({})", tester(cx.rt, this)?.label))
    }
}

// --- Helpers for scripts ---

fn make_element(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    let name = rt.to_rust_string(value)?;
    let id = new_node(rt, &name, true).map_err(|_| VmError::type_error("node"))?;
    Ok(NativeReturn::Value(
        TestHost::wrap_node(rt, call.realm(), id)?.into(),
    ))
}

fn make_node(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let id = new_node(rt, "#text", false).map_err(|_| VmError::type_error("node"))?;
    Ok(NativeReturn::Value(
        TestHost::wrap_node(rt, call.realm(), id)?.into(),
    ))
}

fn node_arg(rt: &Runtime, call: &NativeCall) -> VmResult<NodeId> {
    rt.host_object(rt.arg(call, 0))
        .and_then(|(_, data)| b::node_of(data))
        .ok_or(VmError::type_error("not a node"))
}

fn get_attr(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let id = node_arg(rt, call)?;
    let value = rt.arg(call, 1);
    let name = rt.to_rust_string(value)?;
    let found = node_mut(rt, id)
        .map_err(|_| VmError::type_error("node"))?
        .attributes
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| v.clone());
    Ok(NativeReturn::Value(match found {
        Some(text) => rt.string_value(&text)?,
        None => Value::Null,
    }))
}

fn set_attr(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let id = node_arg(rt, call)?;
    let value = rt.arg(call, 1);
    let name = rt.to_rust_string(value)?;
    let value = rt.arg(call, 2);
    let value = rt.to_rust_string(value)?;
    let node = node_mut(rt, id).map_err(|_| VmError::type_error("node"))?;
    node.attributes.retain(|(n, _)| *n != name);
    node.attributes.push((name, value));
    Ok(NativeReturn::Value(Value::Undefined))
}

fn gc(rt: &mut Runtime, _call: &NativeCall) -> VmResult<NativeReturn> {
    rt.collect();
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The prelude of every case: `t(f)` gives the value or `Name: message`.
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
    rt.init_global(0, generated::WINDOW, HostData::Word(7))
        .unwrap();
    let functions: [(&str, u32, swb_js::NativeFn); 5] = [
        ("makeElement", 1, make_element),
        ("makeNode", 0, make_node),
        ("getAttr", 2, get_attr),
        ("setAttr", 3, set_attr),
        ("gc", 0, gc),
    ];
    for (name, length, func) in functions {
        rt.define_global_function(name, length, func).unwrap();
    }
    rt.eval(PRELUDE).unwrap();
    rt
}

/// Runs a case: the string of its value, then (after the jobs) ` | ` and
/// `out` if the case set it.
fn run(rt: &mut Runtime, source: &str) -> String {
    let result = match rt.eval(source) {
        Ok(value) => rt
            .to_rust_string(value)
            .unwrap_or_else(|e| format!("{e:?}")),
        Err(error) => format!("error: {error}"),
    };
    if let Err(error) = rt.run_jobs() {
        return format!("{result} | jobs: {error}");
    }
    match rt.eval("typeof out === 'undefined' ? undefined : String(out)") {
        Ok(Value::Undefined) => result,
        Ok(value) => format!(
            "{result} | {}",
            rt.to_rust_string(value).unwrap_or_default()
        ),
        Err(error) => format!("{result} | error: {error}"),
    }
}

/// Runs each (source, expected) case in a new runtime, with and without
/// the GC stress mode, on a thread with a big stack.
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
                    let actual = run(&mut rt, source);
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
fn integers() {
    check(&[
        (
            "new Tester().integers(-129, 256, 32768, -1, 2 ** 31, -1, 2 ** 63, -1)",
            "127 0 -32768 65535 -2147483648 4294967295 -9223372036854775808 18446744073709551615",
        ),
        // The tabIndex values of the probe (`long`, no attribute).
        (
            "['12abc', '12', 1.9, -1.9, 2 ** 32 + 5, 2 ** 31, -(2 ** 31) - 1, NaN, Infinity, null, true, '0x10', ' 7 ', { valueOf() { return 3; } }].map((v) => new Tester().integers(0, 0, 0, 0, v, 0, 0, 0).split(' ')[4]).join()",
            "0,12,1,-1,5,-2147483648,2147483647,0,0,0,1,16,7,3",
        ),
        (
            "new Tester().integers(0, 0, 0, 0, 0, -1, 0, 0).split(' ')[5]",
            "4294967295",
        ),
        (
            "new Tester().clamped(300, 2 ** 40, 2999.5, 2 ** 70) + ' ' + new Tester().clamped(-1, NaN, 3000.5, -(2 ** 70))",
            "255 2147483647 3000 9007199254740991 0 0 3000 -9007199254740991",
        ),
        (
            "[NaN, Infinity, 2 ** 31, -(2 ** 31) - 1, 1.9, '2', null].map((v) => t(() => new Tester().enforced(v, 0, 0))).join('|')",
            "TypeError: Failed to execute 'enforced' on 'Tester': Value is not of type 'long'.|TypeError: Failed to execute 'enforced' on 'Tester': Value is infinite and not of type 'long'.|TypeError: Failed to execute 'enforced' on 'Tester': Value is outside the 'long' value range.|TypeError: Failed to execute 'enforced' on 'Tester': Value is outside the 'long' value range.|1 0 0|2 0 0|0 0 0",
        ),
        (
            "[t(() => new Tester().enforced(0, -1, 0)), t(() => new Tester().enforced(0, 0, 2 ** 53)), new Tester().enforced(0, 0, 2 ** 53 - 1)].join('|')",
            "TypeError: Failed to execute 'enforced' on 'Tester': Value is outside the 'unsigned long' value range.|TypeError: Failed to execute 'enforced' on 'Tester': Value is outside the 'long long' value range.|0 0 9007199254740991",
        ),
        (
            "[t(() => new Tester().integers(10n, 0, 0, 0, 0, 0, 0, 0)), t(() => new Tester().integers(Symbol(), 0, 0, 0, 0, 0, 0, 0))].join('|')",
            "TypeError: Failed to execute 'integers' on 'Tester': Cannot convert a BigInt value to a number|TypeError: Failed to execute 'integers' on 'Tester': Cannot convert a Symbol value to a number",
        ),
    ]);
}

#[test]
fn floats_and_strings() {
    check(&[
        (
            "new Tester().floats(1.5, NaN, 2, -Infinity)",
            "1.5 NaN 2 -inf",
        ),
        (
            "[t(() => new Tester().floats(Infinity, 0, 0, 0)), t(() => new Tester().floats(1e39, 0, 0, 0)), t(() => new Tester().floats(0, 0, NaN, 0)), t(() => new Tester().floats(0, 0, 'x', 0))].join('|')",
            "TypeError: Failed to execute 'floats' on 'Tester': The provided float value is non-finite.|TypeError: Failed to execute 'floats' on 'Tester': The provided float value is non-finite.|TypeError: Failed to execute 'floats' on 'Tester': The provided double value is non-finite.|TypeError: Failed to execute 'floats' on 'Tester': The provided double value is non-finite.",
        ),
        (
            "new Tester().strings(null, null, 'a\\ud800', '\\u00e9')",
            "null||a\u{FFFD}|\u{e9}",
        ),
        (
            "[t(() => new Tester().strings(undefined, undefined, { toString() { return 'x'; } }, 12.5)), t(() => new Tester().strings('', '', '', '\\u0100')), t(() => new Tester().strings(Symbol(), '', '', ''))].join('|')",
            "undefined|undefined|x|12.5|TypeError: Failed to execute 'strings' on 'Tester': String contains non ISO-8859-1 code point.|TypeError: Failed to execute 'strings' on 'Tester': Cannot convert a Symbol value to a string",
        ),
        (
            "t(() => new Tester().strings({ toString() { throw new RangeError('mine'); } }, '', '', ''))",
            "RangeError: mine",
        ),
    ]);
}

#[test]
fn arguments_nullables_optionals_variadics() {
    check(&[
        (
            "new Tester().nullables(null, undefined, null)",
            "None None -1",
        ),
        (
            "const n = makeNode(); new Tester().nullables(3, 'x', n)",
            "Some(3) Some(\"x\") 0",
        ),
        (
            "t(() => new Tester().nullables(1, 2, {}))",
            "TypeError: Failed to execute 'nullables' on 'Tester': parameter 3 is not of type 'Node'.",
        ),
        (
            "[t(() => new Tester().nullables()), t(() => new Tester().nullables(1, 2))].join('|')",
            "TypeError: Failed to execute 'nullables' on 'Tester': 3 arguments required, but only 0 present.|TypeError: Failed to execute 'nullables' on 'Tester': 3 arguments required, but only 2 present.",
        ),
        ("new Tester().optionals()", "5 x None slow undefined 1.5 []"),
        (
            "new Tester().optionals(undefined, 'y', 0, 'two-words', null, undefined, [7])",
            "5 y Some(false) two-words null 1.5 [7]",
        ),
        ("new Tester().variadic('a', 1, '2', 3.7)", "a [1, 2, 3]"),
        ("new Tester().variadic('a')", "a []"),
        (
            "[t(() => new Tester().variadic()), t(() => new Tester().variadic('a', 1, Symbol()))].join('|')",
            "TypeError: Failed to execute 'variadic' on 'Tester': 1 argument required, but only 0 present.|TypeError: Failed to execute 'variadic' on 'Tester': Cannot convert a Symbol value to a number",
        ),
        (
            "[Tester.prototype.optionals.length, Tester.prototype.variadic.length, Tester.prototype.integers.length, Tester.prototype.over.length, Tester.prototype.pick.length].join()",
            "0,1,8,0,1",
        ),
        (
            "const log = []; const v = { valueOf() { log.push('v'); throw new RangeError('from valueOf'); } }; [t(() => new Tester().nullables(v, v, v)), log.join()].join('|')",
            "RangeError: from valueOf|v",
        ),
    ]);
}

#[test]
fn sequences_and_records() {
    check(&[
        (
            "new Tester().sequences([1, '2'], [['a'], ['b', 'c']])",
            "[1, 2] [[\"a\"], [\"b\", \"c\"]]",
        ),
        ("new Tester().sequences(new Set([3, 4]), [])", "[3, 4] []"),
        (
            "[t(() => new Tester().sequences(5, [])), t(() => new Tester().sequences({}, [])), t(() => new Tester().sequences({ [Symbol.iterator]: 5 }, [])), t(() => new Tester().sequences('ab', [])), t(() => new Tester().sequences([], [5]))].join('|')",
            "TypeError: Failed to execute 'sequences' on 'Tester': The provided value cannot be converted to a sequence.|TypeError: Failed to execute 'sequences' on 'Tester': The object must have a callable @@iterator property.|TypeError: Failed to execute 'sequences' on 'Tester': @@iterator must be a callable.|TypeError: Failed to execute 'sequences' on 'Tester': The provided value cannot be converted to a sequence.|TypeError: Failed to execute 'sequences' on 'Tester': The provided value cannot be converted to a sequence.",
        ),
        (
            "const log = []; const it = { [Symbol.iterator]() { log.push('iter'); let i = 0; return { next() { log.push('next ' + i); i++; return i > 2 ? { done: true } : { value: { valueOf() { log.push('conv'); return i; } }, done: false }; } }; } }; new Tester().sequences(it, []) + ' ' + log.join()",
            "[1, 2] [] iter,next 0,conv,next 1,conv,next 2",
        ),
        (
            "t(() => new Tester().records({ a: 1, b: '2', [Symbol.iterator]: 0 }, {}))",
            "TypeError: Failed to execute 'records' on 'Tester': Cannot convert a Symbol value to a string",
        ),
        (
            "new Tester().records(Object.defineProperty({ 2: 'b', 1: 'a', c: 3 }, 'h', { value: 1, enumerable: false }), {})",
            "[(\"1\", 0), (\"2\", 0), (\"c\", 3)] []",
        ),
        (
            "const n = makeNode(); new Tester().records({}, { x: n })",
            "[] [(\"x\", 0)]",
        ),
        (
            "[t(() => new Tester().records(5, {})), t(() => new Tester().records({}, { x: 5 })), t(() => new Tester().records({ [Symbol('s')]: 1 }, {}))].join('|')",
            "TypeError: Failed to execute 'records' on 'Tester': Only objects can be converted to record<K,V> types|TypeError: Failed to execute 'records' on 'Tester': Failed to convert value to 'Node'.|TypeError: Failed to execute 'records' on 'Tester': Cannot convert a Symbol value to a string",
        ),
        (
            "const log = []; const p = new Proxy({ a: 1, b: 2 }, { ownKeys(t) { log.push('ownKeys'); return Reflect.ownKeys(t); }, getOwnPropertyDescriptor(t, k) { log.push('gopd ' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); }, get(t, k) { log.push('get ' + String(k)); return Reflect.get(t, k); } }); new Tester().records(p, {}); log.join()",
            "ownKeys,gopd a,get a,gopd b,get b",
        ),
        (
            "new Tester().records({ '\\ud800': 1, a: 2, '\\ud801': 3 }, {})",
            "[(\"\u{fffd}\", 3), (\"a\", 2)] []",
        ),
    ]);
}

#[test]
fn dictionaries_and_enums() {
    check(&[
        (
            "new Tester().dictionary({ name: 'n', clamped: 300, enforced: 5, mode: 'two-words', list: ['a'], map: { k: 1 }, extra: 7, ratio: 0.5, node: null })",
            "base=1 name=n flag=false clamped=Some(255) enforced=Some(5) mode=two-words list=Some([\"a\"]) map=Some([(\"k\", 1)]) node=Some(None) extra=Some(\"7\") ratio=Some(0.5)",
        ),
        (
            "new Tester().dictionary({ name: 'n' })",
            "base=1 name=n flag=false clamped=None enforced=None mode=fast list=None map=None node=None extra=None ratio=None",
        ),
        (
            "[t(() => new Tester().dictionary({})), t(() => new Tester().dictionary(5)), t(() => new Tester().dictionary()), t(() => new Tester().dictionary({ name: 'n', mode: 'bogus' })), t(() => new Tester().dictionary({ name: 'n', ratio: NaN })), t(() => new Tester().dictionary({ name: 'n', enforced: -1 })), t(() => new Tester().dictionary({ name: 'n', node: 5 })), t(() => new Tester().dictionary({ name: 'n', list: 5 }))].join('|')",
            "TypeError: Failed to execute 'dictionary' on 'Tester': Failed to read the 'name' property from 'TestBase': Required member is undefined.|TypeError: Failed to execute 'dictionary' on 'Tester': The provided value is not of type 'TestDict'.|TypeError: Failed to execute 'dictionary' on 'Tester': 1 argument required, but only 0 present.|TypeError: Failed to execute 'dictionary' on 'Tester': Failed to read the 'mode' property from 'TestDict': The provided value 'bogus' is not a valid enum value of type TestMode.|TypeError: Failed to execute 'dictionary' on 'Tester': Failed to read the 'ratio' property from 'TestDict': The provided double value is non-finite.|TypeError: Failed to execute 'dictionary' on 'Tester': Failed to read the 'enforced' property from 'TestDict': Value is outside the 'unsigned long' value range.|TypeError: Failed to execute 'dictionary' on 'Tester': Failed to read the 'node' property from 'TestDict': Failed to convert value to 'Node'.|TypeError: Failed to execute 'dictionary' on 'Tester': Failed to read the 'list' property from 'TestDict': The provided value cannot be converted to a sequence.",
        ),
        (
            "const order = []; new Tester().dictionary(new Proxy({ name: 'n' }, { get(target, key) { order.push(String(key)); return target[key]; } })); order.join()",
            "base,name,clamped,enforced,extra,flag,list,map,mode,node,ratio",
        ),
        (
            "t(() => new Tester().dictionary({ get name() { throw new SyntaxError('in getter'); } }))",
            "SyntaxError: in getter",
        ),
        (
            "const f = () => 1; Object.defineProperty(f, 'name', { value: 'f' }); new Tester().dictionary(f).split(' ')[1]",
            "name=f",
        ),
        (
            "[new Tester().options(), new Tester().options(null), new Tester().options({ capture: 1, label: 5 })].join('|')",
            "capture=false label=None|capture=false label=None|capture=true label=Some(\"5\")",
        ),
        (
            "t(() => new Tester().options('str'))",
            "TypeError: Failed to execute 'options' on 'Tester': The provided value is not of type 'TestOptions'.",
        ),
        (
            "[new Tester().echoMode('two-words'), new Tester().echoMode(''), typeof new Tester().echoMode('slow')].join('|')",
            "two-words||string",
        ),
        (
            "t(() => new Tester().echoMode('Fast'))",
            "TypeError: Failed to execute 'echoMode' on 'Tester': The provided value 'Fast' is not a valid enum value of type TestMode.",
        ),
        (
            "JSON.stringify(new Tester().dictOut())",
            "{\"base\":1,\"name\":\"made\",\"flag\":false,\"list\":[\"x\"],\"mode\":\"fast\",\"node\":null}",
        ),
    ]);
}

#[test]
fn callbacks_promises_any_object() {
    check(&[
        (
            "new Tester().callbackFunction(function (n, s) { return [this, n, s, arguments.length].join(); })",
            "[object Window],42,s,2",
        ),
        (
            "new Tester().callbackFunction(function (n, s) { 'use strict'; return [this, n, s].join(); })",
            ",42,s",
        ),
        // V8 says "Class constructor  cannot be invoked without 'new'".
        (
            "t(() => new Tester().callbackFunction(class {}))",
            "TypeError: Class constructors cannot be invoked without 'new'",
        ),
        (
            "[t(() => new Tester().callbackFunction(5)), t(() => new Tester().callbackFunction({})), t(() => new Tester().callbackFunction())].join('|')",
            "TypeError: Failed to execute 'callbackFunction' on 'Tester': parameter 1 is not of type 'Function'.|TypeError: Failed to execute 'callbackFunction' on 'Tester': parameter 1 is not of type 'Function'.|TypeError: Failed to execute 'callbackFunction' on 'Tester': 1 argument required, but only 0 present.",
        ),
        (
            "t(() => new Tester().callbackFunction(() => { throw new EvalError('cb'); }))",
            "EvalError: cb",
        ),
        (
            "const tt = new Tester(); const log = []; const o = { handle(w) { log.push(this === o, w); } }; tt.callbackInterface(o); const f = function (w) { log.push(this === tt, w); }; f.handle = () => log.push('not called'); tt.callbackInterface(f); log.join()",
            "true,x,true,x",
        ),
        (
            "const log = []; const o = { get handle() { log.push('get'); return () => log.push('call'); } }; const tt = new Tester(); tt.callbackInterface(o); tt.callbackInterface(o); log.join()",
            "get,call,get,call",
        ),
        (
            "[t(() => new Tester().callbackInterface(5)), t(() => new Tester().callbackInterface('s')), t(() => new Tester().callbackInterface({})), t(() => new Tester().callbackInterface({ handle: 5 }))].join('|')",
            "TypeError: Failed to execute 'callbackInterface' on 'Tester': parameter 1 is not of type 'Object'.|TypeError: Failed to execute 'callbackInterface' on 'Tester': parameter 1 is not of type 'Object'.|TypeError: 'handle' of the callback object is not a function|TypeError: 'handle' of the callback object is not a function",
        ),
        (
            "const p = Promise.resolve(1); const tt = new Tester(); [tt.promised(p) === p, tt.promised(5) instanceof Promise].join(); ",
            "true,true",
        ),
        (
            "var out; new Tester().promised(5).then((v) => { out = v; }); 'pending'",
            "pending | 5",
        ),
        (
            "var out = []; const tt = new Tester(); for (const k of ['type', 'range', 'dom']) tt.rejects(k).catch((e) => out.push(e.name + ': ' + e.message + ' ' + (e instanceof DOMException))); 'sync'",
            "sync | TypeError: Failed to execute 'rejects' on 'Tester': bad false,RangeError: Failed to execute 'rejects' on 'Tester': too far false,AbortError: Failed to execute 'rejects' on 'Tester': stopped true",
        ),
        (
            "var out; new Tester().rejects().catch((e) => { out = e.message; }); 'ok'",
            "ok | Failed to execute 'rejects' on 'Tester': 1 argument required, but only 0 present.",
        ),
        (
            "const o = {}; const tt = new Tester(); [tt.echoAny(o) === o, tt.echoAny(undefined), tt.echoAny(null), tt.echoObject(o) === o, t(() => tt.echoAny())].join()",
            "true,,,true,TypeError: Failed to execute 'echoAny' on 'Tester': 1 argument required, but only 0 present.",
        ),
        (
            "t(() => new Tester().echoObject(5))",
            "TypeError: Failed to execute 'echoObject' on 'Tester': parameter 1 is not of type 'object'.",
        ),
        ("new Tester().custom(21)", "42"),
    ]);
}

#[test]
fn unions() {
    check(&[
        (
            "const e = makeElement('p'); new Tester().unions(e, 1, [1, 2], {})",
            "element 0; long 1; sequence [1, 2]; options false",
        ),
        (
            "new Tester().unions('s', true, { a: 1 }, true)",
            "string s; boolean true; record [(\"a\", 1)]; boolean true",
        ),
        (
            "new Tester().unions(makeNode(), 'x', 5, 0)",
            "string [object Node]; long 0; string 5; boolean false",
        ),
        (
            "new Tester().unions(null, null, null, null)",
            "string null; long 0; string null; options false",
        ),
        (
            "[new Tester().nullableUnion(null), new Tester().nullableUnion(undefined), new Tester().nullableUnion(5), new Tester().nullableUnion('x'), new Tester().nullableUnion(true)].join('|')",
            "null|null|long 5|string x|string true",
        ),
        (
            "[new Tester().handler(() => 1), new Tester().handler(5), new Tester().handler({})].join('|')",
            "callback|code 5|code [object Object]",
        ),
        (
            "[new Tester().unionOut(true), new Tester().unionOut(false), typeof new Tester().unionOut(true)].join()",
            "5,five,number",
        ),
    ]);
}

#[test]
fn overloads() {
    check(&[
        (
            "const tt = new Tester(); [tt.over(), tt.over(1), tt.over('a', 1), tt.over([1], 2), tt.over({}, 3), tt.over(null, 4), tt.over(5, 6), tt.over(1, 2, 3, 4), tt.over(1, 2, 3, 4, 5)].join('|')",
            "none|long 1|string a 1|sequence [1] 2|options false 3|options false 4|string 5 6|four 1 2 3 4|four 1 2 3 4",
        ),
        (
            "const e = makeElement('x'); new Tester().over(e, 1)",
            "node 0 1",
        ),
        (
            "t(() => new Tester().over(1, 2, 3))",
            "TypeError: Failed to execute 'over' on 'Tester': Overload resolution failed.",
        ),
        (
            "const n = makeNode(); [new Tester().pick(n), new Tester().pick(() => 1)].join('|')",
            "node 0|callback",
        ),
        (
            "[t(() => new Tester().pick()), t(() => new Tester().pick(5)), t(() => new Tester().pick({}))].join('|')",
            "TypeError: Failed to execute 'pick' on 'Tester': 1 argument required, but only 0 present.|TypeError: Failed to execute 'pick' on 'Tester': Overload resolution failed.|TypeError: Failed to execute 'pick' on 'Tester': Overload resolution failed.",
        ),
    ]);
}

#[test]
fn results_and_exceptions() {
    check(&[
        (
            "const tt = new Tester(); const a = tt.elements(); [a.length, a[0] === tt.elements()[0], a[0] instanceof TestElement, a[0].nodeName, Array.isArray(a)].join()",
            "2,true,true,a,true",
        ),
        (
            "JSON.stringify(new Tester().recordOut())",
            "{\"b\":2,\"a\":1}",
        ),
        (
            "[JSON.stringify(new Tester().maybeList(true)), String(new Tester().maybeList(false)), new Tester().maybeElement(true) instanceof TestElement, String(new Tester().maybeElement(false))].join('|')",
            "[\"p\",\"q\"]|null|true|null",
        ),
        (
            "const tt = new Tester(); try { tt.throwDom('NotFoundError', 'gone'); } catch (e) { [e instanceof DOMException, e.name, e.code, e.message, String(e), Object.getOwnPropertyNames(e).length].join('|') }",
            "true|NotFoundError|8|Failed to execute 'throwDom' on 'Tester': gone|NotFoundError: Failed to execute 'throwDom' on 'Tester': gone|0",
        ),
        (
            "[t(() => new Tester().throwType()), t(() => new Tester().throwRange())].join('|')",
            "TypeError: Failed to execute 'throwType' on 'Tester': wrong type|RangeError: Failed to execute 'throwRange' on 'Tester': out of range",
        ),
        (
            "[Tester.version(), typeof new Tester().version, String(new Tester('q')), `${new Tester()}`, Tester.prototype.toString.length].join('|')",
            "1|undefined|Tester(q)|Tester(t)|0",
        ),
        (
            "t(() => Tester.prototype.toString.call({}))",
            "TypeError: Illegal invocation",
        ),
    ]);
}

#[test]
fn attributes() {
    check(&[
        (
            "const tt = new Tester(); tt.counter = '7'; const a = tt.counter; tt.counter = 2 ** 32 + 1; [a, tt.counter].join()",
            "7,1",
        ),
        (
            "const g = Object.getOwnPropertyDescriptor(Tester.prototype, 'counter'); [t(() => g.get.call({})), t(() => g.set.call({}, 1)), t(() => g.get.call(undefined)), g.get.name, g.set.name, g.get.length, g.set.length, g.enumerable, g.configurable].join('|')",
            "TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Illegal invocation|get counter|set counter|0|1|true|true",
        ),
        (
            "const tt = new Tester(); Object.getOwnPropertyDescriptor(Tester.prototype, 'counter').set.call(tt); tt.counter",
            "0",
        ),
        (
            "const tt = new Tester('lab'); tt.label = 'x'; [tt.label, Object.getOwnPropertyDescriptor(Tester.prototype, 'label').set].join('|')",
            "lab|",
        ),
        (
            "'use strict'; t(() => { new Tester().label = 1; })",
            "TypeError: Cannot set property label of #<Tester> which has only a getter",
        ),
        (
            "const tt = new Tester(); const a = tt.replaceable; tt.replaceable = 5; const d = Object.getOwnPropertyDescriptor(tt, 'replaceable'); [a, tt.replaceable, d.value, d.writable, d.enumerable, d.configurable, new Tester().replaceable].join()",
            "1,5,5,true,true,true,1",
        ),
        (
            "const tt = new Tester(); tt.self = '9'; [tt.counter, tt.self === tt].join()",
            "9,true",
        ),
        (
            "const a = new Tester(); const b = new Tester(); const d = Object.getOwnPropertyDescriptor(a, 'trusted'); [a.trusted, typeof d.get, d.set, d.enumerable, d.configurable, d.get === Object.getOwnPropertyDescriptor(b, 'trusted').get, 'trusted' in Tester.prototype, t(() => { delete a.trusted; return a.trusted; })].join()",
            "true,function,,true,false,true,false,true",
        ),
        (
            "class S extends Tester {} const s = new S('q'); [s instanceof S, s.label, Object.getOwnPropertyDescriptor(s, 'trusted') !== undefined].join()",
            "true,q,true",
        ),
        (
            "const tt = new Tester(); [tt.element === tt.element, tt.element instanceof TestElement].join()",
            "true,true",
        ),
        (
            "const d = Object.getOwnPropertyDescriptor(Tester, 'ANSWER'); [Tester.ANSWER, Tester.YES, Tester.HALF, Tester.BIG, new Tester().BIG, d.writable, d.enumerable, d.configurable, Object.getOwnPropertyDescriptor(Tester.prototype, 'HALF').value].join()",
            "42,true,0.5,4294967295,4294967295,false,true,false,0.5",
        ),
        (
            "[Tester.length, new Tester().label, t(() => Tester()), t(() => new TestElement()), t(() => new Node())].join('|')",
            "0|t|TypeError: Failed to construct 'Tester': Please use the 'new' operator, this DOM object constructor cannot be called as a function.|TypeError: Failed to construct 'TestElement': Illegal constructor|TypeError: Failed to construct 'Node': Illegal constructor",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyNames(Tester.prototype).slice(0, 12))",
            "[\"counter\",\"label\",\"replaceable\",\"self\",\"element\",\"ANSWER\",\"YES\",\"HALF\",\"BIG\",\"callbackFunction\",\"callbackInterface\",\"clamped\"]",
        ),
        (
            "const names = Object.getOwnPropertyNames(Tester.prototype); [names[names.length - 1], names.includes('trusted'), JSON.stringify(Object.getOwnPropertyNames(Tester))].join('|')",
            "constructor|false|[\"length\",\"name\",\"prototype\",\"ANSWER\",\"YES\",\"HALF\",\"BIG\",\"version\"]",
        ),
    ]);
}

#[test]
fn reflect() {
    check(&[
        (
            "const e = makeElement('x'); const a = e.title; e.title = 5; [a === '', e.title, getAttr(e, 'title')].join('|')",
            "true|5|5",
        ),
        (
            "const e = makeElement('x'); const a = e.optionalText; e.optionalText = 'v'; const b = getAttr(e, 'data-x'); e.optionalText = null; [a, b, getAttr(e, 'data-x')].join('|')",
            "|v|",
        ),
        (
            "const e = makeElement('x'); const r = [e.dir]; for (const v of ['RTL', 'bogus', 'auto']) { setAttr(e, 'dir', v); r.push(e.dir); } e.dir = 'XyZ'; r.push(getAttr(e, 'dir')); r.join('|')",
            "|rtl||auto|XyZ",
        ),
        (
            "const e = makeElement('x'); const r = [e.type]; for (const v of ['TEXT', 'bogus', '', 'Checkbox', 'button']) { setAttr(e, 'type', v); r.push(e.type); } r.join('|')",
            "text|text|text|text|checkbox|radio",
        ),
        (
            "const e = makeElement('x'); const r = [String(e.crossOrigin)]; for (const v of ['', 'USE-CREDENTIALS', 'xyz']) { setAttr(e, 'crossorigin', v); r.push(e.crossOrigin); } e.crossOrigin = null; r.push(String(e.crossOrigin), String(getAttr(e, 'crossorigin'))); r.join('|')",
            "null|anonymous|use-credentials|anonymous|null|null",
        ),
        (
            "const e = makeElement('x'); const r = [e.href]; e.href = 'a'; r.push(e.href, getAttr(e, 'href')); e.href = 'x y'; r.push(e.href); e.href = 'https://b/'; r.push(e.href); r.join('|')",
            "|http://example.test/a|a|x y|https://b/",
        ),
        (
            "const e = makeElement('x'); const r = [e.hidden, getAttr(e, 'hidden')]; e.hidden = 1; r.push(getAttr(e, 'hidden'), e.hidden); e.hidden = 0; r.push(getAttr(e, 'hidden')); r.join('|')",
            "false|||true|",
        ),
        (
            "const e = makeElement('x'); const r = [e.tabIndex]; for (const v of ['12abc', ' -5', 'x', '99999999999']) { setAttr(e, 'tabindex', v); r.push(e.tabIndex); } e.tabIndex = '7.9'; r.push(getAttr(e, 'tabindex')); r.join('|')",
            "0|12|-5|0|0|7",
        ),
        (
            "const e = makeElement('x'); const r = [e.maxLength]; setAttr(e, 'maxlength', '-3'); r.push(e.maxLength); e.maxLength = 4; r.push(getAttr(e, 'maxlength')); r.push(t(() => { e.maxLength = -2; })); r.join('|')",
            "-1|-1|4|IndexSizeError: Failed to set the 'maxLength' property on 'TestElement': The value provided (-2) is not positive or 0.",
        ),
        (
            "const e = makeElement('x'); const r = [e.size]; setAttr(e, 'size', '0'); r.push(e.size); e.size = -1; r.push(getAttr(e, 'size')); e.size = 2 ** 31; r.push(getAttr(e, 'size')); r.join('|')",
            "20|0|20|20",
        ),
        (
            "const e = makeElement('x'); const r = [e.cols]; setAttr(e, 'cols', '0'); r.push(e.cols); r.push(t(() => { e.cols = 0; })); e.cols = 3; r.push(getAttr(e, 'cols')); r.join('|')",
            "1|1|IndexSizeError: Failed to set the 'cols' property on 'TestElement': The value provided is 0, which is an invalid size.|3",
        ),
        (
            "const e = makeElement('x'); const r = [e.rows]; setAttr(e, 'rows', '0'); r.push(e.rows); e.rows = 0; r.push(getAttr(e, 'rows')); r.join('|')",
            "2|2|2",
        ),
        (
            "const e = makeElement('x'); const r = [e.colSpan]; for (const v of ['5000', '0', '7']) { setAttr(e, 'colspan', v); r.push(e.colSpan); } e.colSpan = 0; r.push(getAttr(e, 'colspan')); r.join('|')",
            "1|1000|1|7|0",
        ),
        (
            "const e = makeElement('x'); setAttr(e, 'autocapitalize', 'on'); const a = e.autocapitalize; e.autocapitalize = 'off'; [a, getAttr(e, 'autocapitalize')].join('|')",
            "got on|off",
        ),
        (
            "const n = makeNode(); t(() => Object.getOwnPropertyDescriptor(TestElement.prototype, 'title').get.call(n))",
            "TypeError: Illegal invocation",
        ),
    ]);
}

#[test]
fn global_interface() {
    check(&[
        (
            "globalValue = 5; const d = Object.getOwnPropertyDescriptor(globalThis, 'globalValue'); [globalValue, typeof d.get, d.enumerable, d.configurable, 'globalValue' in Window.prototype, globalOp(), Object.getPrototypeOf(globalThis) === Window.prototype].join('|')",
            "5|function|true|true|false|global Word(7)|true",
        ),
        (
            "const d = Object.getOwnPropertyDescriptor(globalThis, 'fixed'); [fixed, d.configurable, d.enumerable].join()",
            "fixed,false,true",
        ),
        (
            "t(() => Object.getOwnPropertyDescriptor(globalThis, 'globalValue').get.call(new Tester()))",
            "TypeError: Illegal invocation",
        ),
    ]);
}

#[test]
fn dom_exception_in_the_test_set() {
    check(&[(
        "const e = new DOMException('m', 'SyntaxError'); [e.name, e.message, e.code, String(e), e instanceof Error].join('|')",
        "SyntaxError|m|12|SyntaxError: m|true",
    )]);
}

#[test]
fn objects_survive_collections() {
    check(&[(
        "const tt = new Tester('kept'); const e = tt.element; gc(); const l = []; for (let i = 0; i < 200; i++) l.push(new Tester('x' + i).dictionary({ name: 'n' + i, list: ['a', 'b'] })); gc(); [tt.label, tt.element === e, l[199].split(' ')[1]].join('|')",
        "kept|true|name=n199",
    )]);
}
