//! A test host for the embedding API of host classes (ADR 0028 section 6):
//! a class tree `A <- B <- C` with methods and accessors, a global object
//! of class `Window` (`EventTarget <- Window`) with a named properties
//! object (`WindowProperties`) on its prototype chain, a legacy indexed
//! object (`List`, array-like hooks), a named-property object (`NamedMap`,
//! hooks that read a map and convert with `ToString` on set) and natives
//! for host data, termination and collections.

use swb_js::Gc;
use swb_js::{
    Attributes, ClassId, ClassSpec, HeapConfig, HostData, HostHooks, HostState, NativeCall,
    NativeFn, NativeReturn, Object, Property, PropertyDescriptor, PropertyKey, Refusal, Runtime,
    RuntimeConfig, SetAnswer, Tracer, Value, VmError, VmResult,
};

/// The classes of the test host, in the order of their registration.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Classes {
    pub(crate) event_target: ClassId,
    pub(crate) window: ClassId,
    pub(crate) window_properties: ClassId,
    pub(crate) a: ClassId,
    pub(crate) b: ClassId,
    pub(crate) c: ClassId,
    pub(crate) list: ClassId,
    pub(crate) map: ClassId,
}

/// The host state: printed lines, the values that the host keeps (traced),
/// the items of the lists, the entries of the maps and the named
/// properties of the global object.
pub(crate) struct TestHost {
    pub(crate) lines: Vec<String>,
    pub(crate) classes: Classes,
    pub(crate) kept: Vec<Value>,
    pub(crate) lists: Vec<Vec<Value>>,
    pub(crate) maps: Vec<Vec<(String, String)>>,
    pub(crate) named: Vec<(String, Value)>,
    pub(crate) next_word: u64,
}

impl HostState for TestHost {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for value in &self.kept {
            tracer.value(*value);
        }
        for list in &self.lists {
            for value in list {
                tracer.value(*value);
            }
        }
        for (_, value) in &self.named {
            tracer.value(*value);
        }
    }
}

/// The test host of a runtime.
pub(crate) fn host(rt: &mut Runtime) -> VmResult<&mut TestHost> {
    rt.host_data_mut::<TestHost>()
        .ok_or(VmError::type_error("no test host"))
}

/// The classes of the runtime's test host.
fn classes(rt: &mut Runtime) -> VmResult<Classes> {
    Ok(host(rt)?.classes)
}

/// The word of a host object's data.
fn word(data: HostData) -> VmResult<u64> {
    match data {
        HostData::Word(word) => Ok(word),
        HostData::Generic(_) => Err(VmError::type_error("generic data")),
    }
}

/// A new string, recorded in the native's handle scope.
fn string(rt: &mut Runtime, text: &str) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(rt.string_value(text)?))
}

// --- The class tree ---

/// `new A()`: a host object of class `A` (or of the subclass of
/// `new.target`) with the next word.
fn a_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = classes(rt)?.a;
    let proto = rt.prototype_from_new_target(call, class)?;
    let host = host(rt)?;
    host.next_word += 1;
    let word = host.next_word;
    let object = rt.new_host_object_with_proto(Some(proto), class, HostData::Word(word))?;
    Ok(NativeReturn::Value(object.into()))
}

fn a_hello(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = classes(rt)?.a;
    let word = word(rt.this_host_data(call, class)?)?;
    string(rt, &format!("A#{word}"))
}

fn a_id(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = classes(rt)?.a;
    let word = word(rt.this_host_data(call, class)?)?;
    Ok(NativeReturn::Value(Value::Int(word as i32)))
}

fn b_method(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = classes(rt)?.b;
    let word = word(rt.this_host_data(call, class)?)?;
    string(rt, &format!("B#{word}"))
}

fn c_method(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = classes(rt)?.c;
    let word = word(rt.this_host_data(call, class)?)?;
    string(rt, &format!("C#{word}"))
}

/// `make(name)`: a new host object of class `B` or `C` (no constructor).
fn make(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let name = rt.to_rust_string(rt.arg(call, 0))?;
    let all = classes(rt)?;
    let class = match name.as_str() {
        "B" => all.b,
        "C" => all.c,
        _ => all.a,
    };
    let host = host(rt)?;
    host.next_word += 1;
    let word = host.next_word;
    let object = rt.new_host_object(call.realm(), class, HostData::Word(word))?;
    Ok(NativeReturn::Value(object.into()))
}

// --- The legacy indexed object ---

/// The items of the list `object`.
fn list_items(rt: &mut Runtime, object: Gc<Object>) -> VmResult<&mut Vec<Value>> {
    let class = classes(rt)?.list;
    let index = word(rt.host_data_of(object.into(), class)?)? as usize;
    host(rt)?
        .lists
        .get_mut(index)
        .ok_or(VmError::type_error("no such list"))
}

fn list_get_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let PropertyKey::Index(index) = key else {
        return Ok(None);
    };
    if index == 666 {
        return Err(VmError::type_error("the list hook throws"));
    }
    Ok(list_items(rt, object)?
        .get(index as usize)
        .map(|&value| Property::Data {
            value,
            writable: false,
            enumerable: true,
            configurable: true,
        }))
}

fn list_define(
    _: &mut Runtime,
    _: Gc<Object>,
    key: PropertyKey,
    _: &PropertyDescriptor,
) -> VmResult<Option<bool>> {
    Ok(matches!(key, PropertyKey::Index(_)).then_some(false))
}

fn list_delete(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<Option<bool>> {
    let PropertyKey::Index(index) = key else {
        return Ok(None);
    };
    Ok(Some(index as usize >= list_items(rt, object)?.len()))
}

/// Legacy platform objects stay extensible (Web IDL §3.9.5).
fn refuse_prevent_extensions(_: &mut Runtime, _: Gc<Object>) -> VmResult<Option<bool>> {
    Ok(Some(false))
}

/// The message of a refused index: the host words it.
fn list_refusal(
    _: &mut Runtime,
    _: Gc<Object>,
    key: PropertyKey,
    why: Refusal,
) -> VmResult<Option<String>> {
    Ok(match key {
        PropertyKey::Index(index) => Some(format!("List refused {why:?} of [{index}]")),
        _ => None,
    })
}

fn list_own_keys(
    rt: &mut Runtime,
    object: Gc<Object>,
    keys: &mut Vec<PropertyKey>,
) -> VmResult<()> {
    let length = list_items(rt, object)?.len();
    keys.extend((0..length as u32).map(PropertyKey::Index));
    Ok(())
}

fn list_length(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = rt.this_or_global(call)?;
    let Value::Object(object) = this else {
        return Err(VmError::type_error("Illegal invocation"));
    };
    let length = list_items(rt, object)?.len();
    Ok(NativeReturn::Value(Value::Int(length as i32)))
}

/// `makeList(...items)`.
fn make_list(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let items: Vec<Value> = (0..call.argc()).map(|i| rt.arg(call, i)).collect();
    let class = classes(rt)?.list;
    let host = host(rt)?;
    host.lists.push(items);
    let index = host.lists.len() as u64 - 1;
    let object = rt.new_host_object(call.realm(), class, HostData::Word(index))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `listPush(list, value)`.
fn list_push(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (list, value) = (rt.arg(call, 0), rt.arg(call, 1));
    let Value::Object(object) = list else {
        return Err(VmError::type_error("not a list"));
    };
    list_items(rt, object)?.push(value);
    Ok(NativeReturn::Value(Value::Undefined))
}

// --- The named-property object ---

/// The entries of the map `object`.
fn map_entries(rt: &mut Runtime, object: Gc<Object>) -> VmResult<&mut Vec<(String, String)>> {
    let class = classes(rt)?.map;
    let index = word(rt.host_data_of(object.into(), class)?)? as usize;
    host(rt)?
        .maps
        .get_mut(index)
        .ok_or(VmError::type_error("no such map"))
}

/// The name of a string key (not an index, not a symbol).
fn name_of(rt: &Runtime, key: PropertyKey) -> Option<String> {
    match key {
        PropertyKey::String(_) => rt.key_to_string(key),
        _ => None,
    }
}

fn map_get_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let Some(name) = name_of(rt, key) else {
        return Ok(None);
    };
    let found = map_entries(rt, object)?
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|(_, value)| value.clone());
    let Some(text) = found else {
        return Ok(None);
    };
    let value = rt.string_value(&text)?;
    Ok(Some(Property::Data {
        value,
        writable: true,
        enumerable: true,
        configurable: true,
    }))
}

/// Stores `ToString(value)` under `name` (the conversion can run script
/// code, which can change the map).
fn map_store(rt: &mut Runtime, object: Gc<Object>, name: String, value: Value) -> VmResult<()> {
    let text = rt.to_rust_string(value)?;
    let entries = map_entries(rt, object)?;
    match entries.iter_mut().find(|(entry, _)| *entry == name) {
        Some(entry) => entry.1 = text,
        None => entries.push((name, text)),
    }
    Ok(())
}

fn map_set(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    value: Value,
    receiver: Value,
) -> VmResult<SetAnswer> {
    let Some(name) = name_of(rt, key) else {
        return Ok(SetAnswer::Ordinary);
    };
    if receiver != Value::Object(object) {
        return Ok(SetAnswer::WithStored);
    }
    if name == "stop" {
        return Err(rt.terminate("map limit"));
    }
    map_store(rt, object, name, value)?;
    Ok(SetAnswer::Done(true))
}

fn map_define(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    desc: &PropertyDescriptor,
) -> VmResult<Option<bool>> {
    let Some(name) = name_of(rt, key) else {
        return Ok(None);
    };
    if desc.get.is_some() || desc.set.is_some() {
        return Ok(Some(false));
    }
    map_store(rt, object, name, desc.value.unwrap_or_default())?;
    Ok(Some(true))
}

fn map_delete(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<Option<bool>> {
    let Some(name) = name_of(rt, key) else {
        return Ok(None);
    };
    let entries = map_entries(rt, object)?;
    let before = entries.len();
    entries.retain(|(entry, _)| *entry != name);
    Ok((entries.len() != before).then_some(true))
}

fn map_own_keys(rt: &mut Runtime, object: Gc<Object>, keys: &mut Vec<PropertyKey>) -> VmResult<()> {
    let names: Vec<String> = map_entries(rt, object)?
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    for name in names {
        keys.push(rt.key_from_str(&name)?);
    }
    Ok(())
}

/// `makeMap()`.
fn make_map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = classes(rt)?.map;
    let host = host(rt)?;
    host.maps.push(Vec::new());
    let index = host.maps.len() as u64 - 1;
    let object = rt.new_host_object(call.realm(), class, HostData::Word(index))?;
    Ok(NativeReturn::Value(object.into()))
}

// --- The named properties of the global object ---

fn named_get_own_property(
    rt: &mut Runtime,
    _: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let Some(name) = name_of(rt, key) else {
        return Ok(None);
    };
    Ok(host(rt)?
        .named
        .iter()
        .find(|(entry, _)| *entry == name)
        .map(|&(_, value)| Property::Data {
            value,
            writable: true,
            enumerable: false,
            configurable: true,
        }))
}

fn refuse_define(
    _: &mut Runtime,
    _: Gc<Object>,
    _: PropertyKey,
    _: &PropertyDescriptor,
) -> VmResult<Option<bool>> {
    Ok(Some(false))
}

/// `setNamed(name, value)`: adds or replaces a named property of the
/// global object (as an element with an id would); `undefined` removes it.
fn set_named(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let name = rt.to_rust_string(rt.arg(call, 0))?;
    let value = rt.arg(call, 1);
    let host = host(rt)?;
    host.named.retain(|(entry, _)| *entry != name);
    if !value.is_undefined() {
        host.named.push((name, value));
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

// --- Host data, termination, output ---

/// `print(...)`.
fn print(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut parts = Vec::new();
    for i in 0..call.argc() {
        parts.push(rt.to_rust_string(rt.arg(call, i))?);
    }
    host(rt)?.lines.push(parts.join(" "));
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `keep(value)`: the host holds the value (traced through the host
/// state).
fn keep(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    host(rt)?.kept.push(value);
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `forget()`: the host drops the values that it held.
fn forget(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    host(rt)?.kept.clear();
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `terminateNow()`: ends the task (a DOM limit would).
fn terminate_now(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    Err(rt.terminate("test limit"))
}

/// `swallow(f)`: calls `f` and drops its error (a native that should not,
/// but a termination must still end the task).
fn swallow(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = rt.arg(call, 0);
    let _ = rt.call(f, Value::Undefined, &[]);
    Ok(NativeReturn::Value(Value::Int(1)))
}

/// `gc()`.
fn gc(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.collect();
    Ok(NativeReturn::Value(Value::Undefined))
}

// --- Setup ---

/// Registers the classes (in a fixed order: the numbers are 0 to 7).
fn define_classes(rt: &mut Runtime) -> VmResult<Classes> {
    let event_target = rt.define_class(ClassSpec::new("EventTarget"))?;
    let window = rt.define_class(ClassSpec::new("Window").parent(event_target))?;
    let window_properties =
        rt.define_class(ClassSpec::new("WindowProperties").hooks(HostHooks {
            get_own_property: Some(named_get_own_property),
            define_own_property: Some(refuse_define),
            ..HostHooks::default()
        }))?;
    let a = rt.define_class(ClassSpec::new("A").constructor(a_constructor, 1))?;
    let b = rt.define_class(ClassSpec::new("B").parent(a))?;
    let c = rt.define_class(ClassSpec::new("C").parent(b))?;
    let list = rt.define_class(ClassSpec::new("List").hooks(HostHooks {
        get_own_property: Some(list_get_own_property),
        define_own_property: Some(list_define),
        delete: Some(list_delete),
        own_keys: Some(list_own_keys),
        prevent_extensions: Some(refuse_prevent_extensions),
        refusal: Some(list_refusal),
        ..HostHooks::default()
    }))?;
    let map = rt.define_class(ClassSpec::new("NamedMap").hooks(HostHooks {
        get_own_property: Some(map_get_own_property),
        define_own_property: Some(map_define),
        set: Some(map_set),
        delete: Some(map_delete),
        own_keys: Some(map_own_keys),
        ..HostHooks::default()
    }))?;
    Ok(Classes {
        event_target,
        window,
        window_properties,
        a,
        b,
        c,
        list,
        map,
    })
}

/// Creates the interfaces of realm 0 and exposes them on the global
/// object (not `WindowProperties`, which has none).
fn create_interfaces(rt: &mut Runtime, classes: &Classes) -> VmResult<()> {
    let global = rt.global(0)?;
    for class in [
        classes.event_target,
        classes.window,
        classes.a,
        classes.b,
        classes.c,
        classes.list,
        classes.map,
    ] {
        let interface = rt.create_interface(0, class)?;
        let name = rt.class_name(class).unwrap_or_default().to_owned();
        rt.define_value(global, &name, interface.object.into(), Attributes::HIDDEN)?;
    }
    let members: [(ClassId, &str, u32, NativeFn); 3] = [
        (classes.a, "hello", 0, a_hello),
        (classes.b, "b", 0, b_method),
        (classes.c, "c", 0, c_method),
    ];
    for (class, name, length, func) in members {
        let prototype = rt.interface(0, class).map(|i| i.prototype);
        let prototype = prototype.ok_or(VmError::type_error("no interface"))?;
        rt.define_method(0, prototype, name, length, func, Attributes::MEMBER)?;
    }
    let a = rt
        .interface(0, classes.a)
        .ok_or(VmError::type_error("no A"))?;
    rt.define_accessor(0, a.prototype, "id", (Some(a_id), None), Attributes::MEMBER)?;
    rt.define_value(a.object, "CONSTANT", Value::Int(7), Attributes::CONSTANT)?;
    rt.define_value(a.prototype, "CONSTANT", Value::Int(7), Attributes::CONSTANT)?;
    let list = rt
        .interface(0, classes.list)
        .ok_or(VmError::type_error("no List"))?;
    rt.define_accessor(
        0,
        list.prototype,
        "length",
        (Some(list_length), None),
        Attributes::MEMBER,
    )?;
    Ok(())
}

/// Makes the global object a `Window` whose prototype chain is
/// `Window.prototype`, `WindowProperties`, `EventTarget.prototype`,
/// `Object.prototype`.
fn create_global(rt: &mut Runtime, classes: &Classes) -> VmResult<()> {
    let event_target = rt
        .interface(0, classes.event_target)
        .ok_or(VmError::type_error("no EventTarget"))?;
    let window = rt
        .interface(0, classes.window)
        .ok_or(VmError::type_error("no Window"))?;
    let named = rt.new_host_object_with_proto(
        Some(event_target.prototype),
        classes.window_properties,
        HostData::Word(0),
    )?;
    // Nothing collects before the object is on the chain.
    if !rt
        .heap_mut()
        .set_prototype_of(window.prototype, Some(named))?
    {
        return Err(VmError::type_error("the chain was refused"));
    }
    // The host defines the tag with the ordinary definition (the hook
    // refuses definitions by scripts).
    let tag = rt.string_value("WindowProperties")?;
    let key = rt.to_string_tag_key();
    let attributes = Attributes {
        writable: false,
        enumerable: false,
        configurable: true,
    };
    rt.define_value_with_key(named, key, tag, attributes)?;
    rt.init_global(0, classes.window, HostData::Word(0))?;
    // Immutable prototypes (Web IDL §3.7.3, §3.7.4.4, §3.8.1).
    let global = rt.global(0)?;
    for object in [global, named, window.prototype, event_target.prototype] {
        rt.set_immutable_prototype(object)?;
    }
    Ok(())
}

/// A runtime with the test host (`stress`: the GC stress mode).
pub(crate) fn host_runtime(stress: bool) -> Runtime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(config).unwrap();
    let classes = define_classes(&mut rt).unwrap();
    rt.set_host_data(TestHost {
        lines: Vec::new(),
        classes,
        kept: Vec::new(),
        lists: Vec::new(),
        maps: Vec::new(),
        named: Vec::new(),
        next_word: 0,
    });
    create_interfaces(&mut rt, &classes).unwrap();
    create_global(&mut rt, &classes).unwrap();
    let functions: [(&str, u32, NativeFn); 12] = [
        ("print", 1, print),
        ("make", 1, make),
        ("makeList", 0, make_list),
        ("listPush", 2, list_push),
        ("makeMap", 0, make_map),
        ("setNamed", 2, set_named),
        ("keep", 1, keep),
        ("forget", 0, forget),
        ("terminateNow", 0, terminate_now),
        ("swallow", 1, swallow),
        ("gc", 0, gc),
        ("makeA", 0, a_constructor),
    ];
    for (name, length, func) in functions {
        rt.define_global_function(name, length, func).unwrap();
    }
    rt
}

/// The printed lines and the result of a script (as the common harness
/// gives them).
pub(crate) fn run_host(rt: &mut Runtime, source: &str) -> String {
    let result = rt
        .eval(source)
        .and_then(|value| rt.run_jobs().map(|()| value));
    let mut lines = std::mem::take(&mut host(rt).unwrap().lines);
    match result {
        Ok(value) => {
            if !value.is_undefined() {
                lines.push(format!("=> {}", rt.display(value)));
            }
        }
        Err(error) => lines.push(error.to_string()),
    }
    lines.join("\n")
}

/// Runs each (source, expected) case in a new host runtime, with and
/// without the GC stress mode, on a thread with an 8 MiB stack.
pub(crate) fn check_host_cases(cases: &[(&str, &str)]) {
    let cases: Vec<(String, String)> = cases
        .iter()
        .map(|&(source, expected)| (source.to_owned(), expected.to_owned()))
        .collect();
    let failures = super::on_big_stack(move || {
        let mut failures = Vec::new();
        for (source, expected) in &cases {
            for stress in [false, true] {
                let mut rt = host_runtime(stress);
                let actual = run_host(&mut rt, source);
                if actual != *expected {
                    failures.push(format!(
                        "{source} (stress {stress})\n  expected: {expected}\n  actual:   {actual}"
                    ));
                }
                assert_eq!(rt.heap().stats().total_stale_roots, 0, "{source}");
                super::assert_clean(&rt);
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
