//! Lazy interfaces (ADR 0028 section 5, "Lazy interfaces notes"; section
//! 6, item 9): the interface objects on the global object and the
//! interfaces of host objects are created at their first use, and no path
//! that reads property values gives a script the internal marker.
//!
//! The host here has the class tree `Base <- Derived <- Leaf`, a `Win`
//! global object, the lazy global properties `Base`, `Derived`, `Leaf` and
//! the factory `MakeLeaf` (the prototype of `Leaf`), and natives that make
//! host objects (`make`), report the created interfaces (`created`) and
//! collect (`gc`).

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use swb_js::{
    Attributes, ClassId, ClassSpec, HeapConfig, HostData, HostState, InternalError, LazyHooks,
    NativeCall, NativeReturn, Runtime, RuntimeConfig, ScriptError, Termination, Tracer, Value,
    VmError, VmResult,
};

use common::{assert_clean, on_big_stack};

/// The host state: the classes, the order in which interfaces were
/// created, the factory function once created, the printed lines.
struct LazyHost {
    base: ClassId,
    derived: ClassId,
    leaf: ClassId,
    created: Vec<&'static str>,
    factory: Option<Value>,
    lines: Vec<String>,
}

impl HostState for LazyHost {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(factory) = self.factory {
            tracer.value(factory);
        }
    }
}

fn host(rt: &mut Runtime) -> VmResult<&mut LazyHost> {
    rt.host_data_mut::<LazyHost>()
        .ok_or(VmError::type_error("no host"))
}

/// The lazy global properties, in their order: the name and the number.
const GLOBALS: [(&str, u32); 4] = [("Base", 0), ("Derived", 1), ("MakeLeaf", 3), ("Leaf", 2)];

/// The interface hook: the ancestors first, then the class with a method
/// `hello` on its prototype; `Leaf` also makes its factory.
fn create_interface(rt: &mut Runtime, realm: u32, class: ClassId) -> VmResult<()> {
    if rt.interface(realm, class).is_some() {
        return Ok(());
    }
    if let Some(parent) = rt.class_parent(class) {
        create_interface(rt, realm, parent)?;
    }
    let interface = rt.create_interface(realm, class)?;
    rt.define_method(
        realm,
        interface.prototype,
        "hello",
        0,
        hello,
        Attributes::MEMBER,
    )?;
    let state = host(rt)?;
    let name = if class == state.base {
        "Base"
    } else if class == state.derived {
        "Derived"
    } else {
        "Leaf"
    };
    state.created.push(name);
    if name == "Leaf" {
        let factory =
            rt.define_constructor(realm, interface.prototype, "MakeLeaf", 0, make_leaf)?;
        host(rt)?.factory = Some(factory.into());
    }
    Ok(())
}

/// The global hook: the interface object of the class, or the factory.
fn global_value(rt: &mut Runtime, realm: u32, id: u32) -> VmResult<Value> {
    let state = host(rt)?;
    let class = match id {
        0 => state.base,
        1 => state.derived,
        2 | 3 => state.leaf,
        _ => {
            return Err(VmError::Internal(InternalError::Invariant(
                "an unknown lazy global",
            )));
        }
    };
    let interface = rt.interface_or_create(realm, class)?;
    if id == 3 {
        return host(rt)?
            .factory
            .ok_or(VmError::Internal(InternalError::Invariant("no factory")));
    }
    Ok(interface.object.into())
}

fn hello(rt: &mut Runtime, _call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(rt.string_value("hello")?))
}

/// `new MakeLeaf()`: a host object of `Leaf`.
fn make_leaf(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let class = host(rt)?.leaf;
    let proto = rt.prototype_from_new_target(call, class)?;
    let object = rt.new_host_object_with_proto(Some(proto), class, HostData::Word(1))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `make(name)`: a host object of the class (its interface is created
/// if needed).
fn make(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let name = rt.to_rust_string(rt.arg(call, 0))?;
    let state = host(rt)?;
    let class = match name.as_str() {
        "Base" => state.base,
        "Derived" => state.derived,
        _ => state.leaf,
    };
    let object = rt.new_host_object(call.realm(), class, HostData::Word(2))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `created()`: the interfaces created so far, in order.
fn created(rt: &mut Runtime, _call: &NativeCall) -> VmResult<NativeReturn> {
    let text = host(rt)?.created.join(",");
    Ok(NativeReturn::Value(rt.string_value(&text)?))
}

fn print(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let text = rt.display(rt.arg(call, 0));
    host(rt)?.lines.push(text);
    Ok(NativeReturn::Value(Value::Undefined))
}

/// A runtime with the lazy host; `stress`: the GC stress mode.
fn lazy_runtime(stress: bool) -> Runtime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(config).unwrap();
    let win = rt.define_class(ClassSpec::new("Win")).unwrap();
    let base = rt.define_class(ClassSpec::new("Base")).unwrap();
    let derived = rt
        .define_class(ClassSpec::new("Derived").parent(base))
        .unwrap();
    let leaf = rt
        .define_class(ClassSpec::new("Leaf").parent(derived))
        .unwrap();
    rt.set_host_data(LazyHost {
        base,
        derived,
        leaf,
        created: Vec::new(),
        factory: None,
        lines: Vec::new(),
    });
    rt.set_lazy_hooks(LazyHooks {
        interface: create_interface,
        global: global_value,
    });
    for (name, id) in GLOBALS {
        rt.define_lazy_global(0, name, id).unwrap();
    }
    rt.create_interface(0, win).unwrap();
    rt.init_global(0, win, HostData::Word(0)).unwrap();
    for (name, length, func) in [
        ("make", 1, make as swb_js::NativeFn),
        ("created", 0, created),
        ("print", 1, print),
        ("gc", 0, common::gc),
    ] {
        rt.define_global_function(name, length, func).unwrap();
    }
    rt
}

/// The printed lines and the result of a script.
fn run_lazy(rt: &mut Runtime, source: &str) -> String {
    let result = rt.eval(source);
    let mut lines = std::mem::take(&mut host(rt).unwrap().lines);
    match result {
        Ok(value) if !value.is_undefined() => lines.push(format!("=> {}", rt.display(value))),
        Ok(_) => {}
        Err(error) => lines.push(error.to_string()),
    }
    lines.join("\n")
}

/// Runs each (source, expected) case in a new runtime, with and without
/// the GC stress mode.
fn check(cases: &[(&str, &str)]) {
    let cases: Vec<(String, String)> = cases
        .iter()
        .map(|&(source, expected)| (source.to_owned(), expected.to_owned()))
        .collect();
    let failures = on_big_stack(move || {
        let mut failures = Vec::new();
        for (source, expected) in &cases {
            for stress in [false, true] {
                let mut rt = lazy_runtime(stress);
                let actual = run_lazy(&mut rt, source);
                if actual != *expected {
                    failures.push(format!(
                        "{source} (stress {stress})\n  expected: {expected}\n  actual:   {actual}"
                    ));
                }
                assert_clean(&rt);
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn keys_and_existence_need_no_creation() {
    check(&[
        (
            "Object.getOwnPropertyNames(globalThis).filter(k => /^(Base|Derived|Leaf|MakeLeaf)$/.test(k)).join() + ' ' + created()",
            "=> Base,Derived,MakeLeaf,Leaf ",
        ),
        (
            "Reflect.ownKeys(globalThis).filter(k => typeof k === 'string' && /^(Base|Derived|Leaf|MakeLeaf)$/.test(k)).join() + ' ' + created()",
            "=> Base,Derived,MakeLeaf,Leaf ",
        ),
        (
            "['Leaf' in globalThis, globalThis.hasOwnProperty('Base'), Object.keys(globalThis).includes('Leaf'), Object.prototype.propertyIsEnumerable.call(globalThis, 'Leaf')].join() + ' ' + created()",
            "=> true,true,false,false ",
        ),
        (
            "var seen = []; for (var k in globalThis) if (k === 'Leaf') seen.push(k); seen.length + ' ' + Object.values(globalThis).length + ' ' + JSON.stringify(Object.entries(globalThis).filter(e => e[0] === 'Leaf')) + ' ' + created()",
            // The enumerable values are the two `var`s of the script.
            "=> 0 2 [] ",
        ),
    ]);
}

#[test]
fn every_read_path_gives_the_interface_object() {
    check(&[
        // The global name: the general lookup, then the cache entry.
        (
            "function f() { return Leaf; } [typeof f(), f() === f(), f().name, created()].join()",
            "=> function,true,Leaf,Base,Derived,Leaf",
        ),
        (
            "typeof Derived + ' ' + created()",
            "=> function Base,Derived",
        ),
        ("typeof Missing", "=> undefined"),
        (
            "[globalThis.Leaf === Leaf, globalThis['Le' + 'af'] === Leaf, this.Base === Base].join()",
            "=> true,true,true",
        ),
        (
            "[Reflect.get(globalThis, 'Leaf') === Leaf, Reflect.get(globalThis, 'Base', {}) === Base].join()",
            "=> true,true",
        ),
        (
            "var o = Object.create(globalThis); var r = []; for (var i = 0; i < 3; i++) r.push(o.Leaf === Leaf); r.join()",
            "=> true,true,true",
        ),
        (
            "with (globalThis) { typeof Leaf + ' ' + Leaf.name }",
            "=> function Leaf",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(globalThis, 'Leaf'); [d.value === Leaf, d.writable, d.enumerable, d.configurable].join()",
            "=> true,true,false,true",
        ),
        (
            "var d = Reflect.getOwnPropertyDescriptor(globalThis, 'Base'); typeof d.value",
            "=> function",
        ),
        (
            "var all = Object.getOwnPropertyDescriptors(globalThis); [all.Leaf.value === Leaf, all.MakeLeaf.value === MakeLeaf, created()].join()",
            "=> true,true,Base,Derived,Leaf",
        ),
        (
            "var p = new Proxy(globalThis, {}); [p.Leaf === Leaf, Reflect.getOwnPropertyDescriptor(p, 'Base').value === Base].join()",
            "=> true,true",
        ),
        (
            "var p = new Proxy(globalThis, { get(t, k, r) { return Reflect.get(t, k, r); } }); p.Derived === Derived",
            "=> true",
        ),
        (
            "Object.prototype.__lookupGetter__.call(globalThis, 'Leaf') + ' ' + created()",
            "=> undefined Base,Derived,Leaf",
        ),
        (
            "var target = {}; Object.assign(target, globalThis); 'Leaf' in target",
            "=> false",
        ),
        ("new Function('return Leaf')() === Leaf", "=> true"),
        ("eval('Leaf') === Leaf", "=> true"),
        ("(0, eval)('typeof Base')", "=> function"),
        (
            "print(Leaf); print(MakeLeaf)",
            "function Leaf() { [native code] }\nfunction MakeLeaf() { [native code] }",
        ),
    ]);
}

#[test]
fn interfaces_have_one_object_whatever_path_comes_first() {
    check(&[
        (
            "var x = make('Leaf'); [x.constructor === Leaf, Object.getPrototypeOf(x) === Leaf.prototype, x instanceof Base, x.hello(), created()].join()",
            "=> true,true,true,hello,Base,Derived,Leaf",
        ),
        (
            "var L = Leaf; var x = make('Leaf'); [x.constructor === L, x instanceof Derived].join()",
            "=> true,true",
        ),
        (
            "var y = new MakeLeaf(); [y instanceof Leaf, MakeLeaf.prototype === Leaf.prototype, created()].join()",
            "=> true,true,Base,Derived,Leaf",
        ),
        ("make('Derived'); created()", "=> Base,Derived"),
        (
            "class X extends Leaf {} [Object.getPrototypeOf(X) === Leaf, Object.getPrototypeOf(X.prototype) === Leaf.prototype].join()",
            "=> true,true",
        ),
        (
            "gc(); var x = make('Leaf'); gc(); [x.constructor === Leaf, Leaf.prototype.hello.call(x)].join()",
            "=> true,hello",
        ),
    ]);
}

#[test]
fn changes_before_the_first_use_act_on_the_property() {
    check(&[
        // `delete`: the name is gone, but a host object of a subclass
        // still gets the whole chain.
        (
            "delete globalThis.Derived; var x = make('Leaf'); var p = Object.getPrototypeOf(Object.getPrototypeOf(x)); [typeof Derived, 'Derived' in globalThis, p.constructor.name, x instanceof Base, created()].join()",
            "=> undefined,false,Derived,true,Base,Derived,Leaf",
        ),
        (
            "delete Leaf; typeof Leaf + ' ' + created()",
            "=> undefined ",
        ),
        // Assignment replaces the value without a creation.
        (
            "Base = 1; var x = make('Leaf'); [Base, Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(x))).constructor.name].join()",
            "=> 1,Base",
        ),
        ("'use strict'; Leaf = 7; [Leaf, created()].join()", "=> 7,"),
        (
            "function f() { Derived = 2; return Derived; } f() + f() + ' ' + created()",
            "=> 4 ",
        ),
        // A definition with a value replaces the value; one that keeps the
        // value creates it first.
        (
            "Object.defineProperty(globalThis, 'Leaf', { value: 5 }); [Leaf, created()].join()",
            "=> 5,",
        ),
        (
            "Object.defineProperty(globalThis, 'Leaf', { get() { return 6; } }); [Leaf, created()].join()",
            "=> 6,",
        ),
        (
            "Object.defineProperty(globalThis, 'Leaf', { enumerable: true }); [Object.keys(globalThis).includes('Leaf'), Object.values(globalThis).includes(Leaf), created()].join()",
            "=> true,true,Base,Derived,Leaf",
        ),
        (
            "Object.defineProperty(globalThis, 'Base', { writable: false, configurable: false }); Base = 3; Object.defineProperty(globalThis, 'Base', { value: Base }); typeof Base",
            "=> function",
        ),
        (
            "Object.freeze(globalThis); [Object.isFrozen(globalThis), typeof Leaf, Object.getOwnPropertyDescriptor(globalThis, 'Leaf').writable].join()",
            "=> true,function,false",
        ),
        (
            "Object.seal(globalThis); [Object.isSealed(globalThis), typeof Derived].join()",
            "=> true,function",
        ),
        // Declarations of scripts.
        (
            "function Base() { return 'mine'; } [Base(), typeof Base.prototype.hello, created()].join()",
            "=> mine,undefined,",
        ),
        ("var Leaf; typeof Leaf", "=> function"),
        (
            "let Derived = 3; [Derived, globalThis.Derived === undefined, typeof globalThis.Derived].join()",
            "=> 3,false,function",
        ),
    ]);
}

#[test]
fn json_and_the_console_never_see_the_marker() {
    check(&[
        ("typeof JSON.stringify(globalThis)", "=> string"),
        (
            "JSON.stringify({ a: Object.getOwnPropertyDescriptor(globalThis, 'Leaf').writable })",
            "=> {\"a\":true}",
        ),
        ("print(globalThis); typeof created()", "#<Win>\n=> string"),
    ]);
}

/// The heap limit during a creation ends the task (and the host drops
/// the runtime); nothing panics and the runtime is left clean.
#[test]
fn a_failure_during_a_creation_terminates_without_a_crash() {
    on_big_stack(|| {
        for stress in [false, true] {
            for source in [
                "(function () { return Leaf; })",
                "(function () { try { return make('Leaf'); } catch (e) { return 'caught'; } })",
            ] {
                let mut rt = lazy_runtime(stress);
                let function = rt.eval(source).unwrap();
                // Room for nothing more: the creation passes the limit.
                let size = rt.heap().heap_size();
                rt.set_heap_limit(size + 512);
                let result = rt.call_function(function, Value::Undefined, &[]);
                assert!(
                    matches!(result, Err(ScriptError::Terminated(Termination::HeapLimit))),
                    "{source}: {result:?}"
                );
                assert_clean(&rt);
                // The failed creation left no interface behind.
                let state = host(&mut rt).unwrap();
                let classes = [state.base, state.derived, state.leaf];
                for class in classes {
                    assert!(rt.interface(0, class).is_none(), "{source}");
                }
                // Nothing broke: with room, the same read works.
                rt.set_heap_limit(size + (1 << 20));
                let out = run_lazy(
                    &mut rt,
                    "typeof Leaf + ' ' + (make('Leaf') instanceof Leaf)",
                );
                assert_eq!(out, "=> function true", "{source}");
                assert_clean(&rt);
            }
        }
    });
}

/// A hook that fails gives an error at the access, not a marker.
#[test]
fn a_failing_hook_is_an_error_at_the_access() {
    on_big_stack(|| {
        let mut rt = lazy_runtime(false);
        rt.define_lazy_global(0, "Broken", 99).unwrap();
        let out = run_lazy(&mut rt, "Broken");
        assert!(out.contains("internal error"), "{out}");
        assert_clean(&rt);
        // The property still holds the marker; a delete removes it.
        let out = run_lazy(&mut rt, "delete globalThis.Broken; typeof Broken");
        assert_eq!(out, "=> undefined");
    });
}
