//! Host classes (ADR 0028 sections 4 and 6): the class tree `A <- B <- C`
//! of the test host, interface objects and prototypes, the brand check,
//! the host-defined global object, and the inline caches over host
//! objects. Messages and property attributes as Chromium gives them for
//! `Node` and `EventTarget` (`tools/probes/web-embedding.json`).

mod common;

use common::host::check_host_cases;

#[test]
fn interface_objects_have_name_length_and_a_fixed_prototype() {
    check_host_cases(&[
        (
            "[A.name, A.length, B.name, B.length, C.length].join()",
            "=> A,1,B,0,0",
        ),
        (
            "typeof A + ' ' + String(A)",
            "=> function function A() { [native code] }",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(A, 'prototype'); [d.value === A.prototype, d.writable, d.enumerable, d.configurable].join()",
            "=> true,false,false,false",
        ),
        // The enumerable getter on the prototype throws for the prototype
        // itself (Chromium: the same for `Node.prototype`).
        (
            "JSON.stringify(A.prototype)",
            "Uncaught TypeError: Illegal invocation",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyDescriptor(A.prototype, 'constructor').writable) + Object.getOwnPropertyDescriptor(A.prototype, 'constructor').enumerable",
            "=> truefalse",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'A'))",
            "=> {\"writable\":true,\"enumerable\":false,\"configurable\":true}",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyDescriptor(A, 'name')) + JSON.stringify(Object.getOwnPropertyDescriptor(A, 'length'))",
            "=> {\"value\":\"A\",\"writable\":false,\"enumerable\":false,\"configurable\":true}{\"value\":1,\"writable\":false,\"enumerable\":false,\"configurable\":true}",
        ),
        (
            "[Object.getPrototypeOf(C) === B, Object.getPrototypeOf(B) === A, Object.getPrototypeOf(A) === Function.prototype].join()",
            "=> true,true,true",
        ),
        (
            "[Object.getPrototypeOf(C.prototype) === B.prototype, Object.getPrototypeOf(A.prototype) === Object.prototype, C.prototype.constructor === C].join()",
            "=> true,true,true",
        ),
        (
            "Object.prototype.toString.call(B.prototype) + Object.prototype.toString.call(make('C')) + Object.prototype.toString.call(C)",
            "=> [object B][object C][object Function]",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyDescriptor(A.prototype, Symbol.toStringTag))",
            "=> {\"value\":\"A\",\"writable\":false,\"enumerable\":false,\"configurable\":true}",
        ),
        (
            "Object.getOwnPropertyNames(A).join() + ' ' + Object.getOwnPropertyNames(A.prototype).join()",
            "=> length,name,prototype,CONSTANT constructor,hello,id,CONSTANT",
        ),
    ]);
}

#[test]
fn members_have_the_webidl_attributes() {
    check_host_cases(&[
        (
            "var d = Object.getOwnPropertyDescriptor(A.prototype, 'id'); [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length].join()",
            "=> function,,true,true,get id,0",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(A.prototype, 'hello'); [d.writable, d.enumerable, d.configurable, d.value.name, d.value.length].join()",
            "=> true,true,true,hello,0",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(A, 'CONSTANT'); [d.value, d.writable, d.enumerable, d.configurable].join()",
            "=> 7,false,true,false",
        ),
    ]);
}

#[test]
fn instanceof_and_methods_follow_the_class_tree() {
    check_host_cases(&[
        (
            "var c = make('C'); [c instanceof C, c instanceof B, c instanceof A, c instanceof Object, make('B') instanceof C].join()",
            "=> true,true,true,true,false",
        ),
        (
            "var c = make('C'); [c.hello(), c.b(), c.c(), c.id].join()",
            "=> A#1,B#1,C#1,1",
        ),
        (
            "var a = new A(); [a.hello(), a.id, typeof a.b].join()",
            "=> A#1,1,undefined",
        ),
        // A subclass in script: the constructor takes the prototype of
        // `new.target`; the brand is still `A`.
        (
            "class D extends A { d() { return 'D' + this.id; } } var d = new D(); [d.d(), d.hello(), d instanceof A, Object.getPrototypeOf(d) === D.prototype].join()",
            "=> D1,A#1,true,true",
        ),
        // Expandos are ordinary own properties.
        (
            "var c = make('C'); c.x = 5; c.x + Object.keys(c).join()",
            "=> 5x",
        ),
    ]);
}

/// A legacy factory function (`Runtime::define_constructor`): a native
/// constructor with `name`, `length` and a fixed `prototype` that is the
/// interface prototype; it sees `new.target`, so subclasses work.
#[test]
fn a_legacy_factory_function_is_a_constructor_with_the_interface_prototype() {
    check_host_cases(&[
        (
            "[Make2.name, Make2.length, Make2.prototype === A.prototype, Object.getPrototypeOf(Make2) === Function.prototype, Object.getOwnPropertyNames(Make2).join()].join()",
            "=> Make2,2,true,true,length,name,prototype",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(Make2, 'prototype'); [d.writable, d.enumerable, d.configurable].join()",
            "=> false,false,false",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(globalThis, 'Make2'); [d.writable, d.enumerable, d.configurable].join()",
            "=> true,false,true",
        ),
        (
            "var a = new Make2(); [a instanceof A, a.hello(), Object.getPrototypeOf(a) === A.prototype].join()",
            "=> true,A#1,true",
        ),
        (
            "class E extends Make2 {} var e = new E(); [e instanceof E, e instanceof A, e.id].join()",
            "=> true,true,1",
        ),
        (
            "Reflect.construct(Make2, [], Object) instanceof A",
            "=> false",
        ),
        (
            "Make2()",
            "Uncaught TypeError: Please use the 'new' operator",
        ),
        (
            "Make2.call(new A())",
            "Uncaught TypeError: Please use the 'new' operator",
        ),
        (
            "Make2.prototype = {}; Make2.prototype === A.prototype",
            "=> true",
        ),
    ]);
}

#[test]
fn a_wrong_this_is_an_illegal_invocation() {
    check_host_cases(&[
        (
            "B.prototype.b.call(new A())",
            "Uncaught TypeError: Illegal invocation",
        ),
        (
            "C.prototype.c.call(make('B'))",
            "Uncaught TypeError: Illegal invocation",
        ),
        (
            "A.prototype.hello.call({})",
            "Uncaught TypeError: Illegal invocation",
        ),
        (
            "A.prototype.hello.call(Object.create(A.prototype))",
            "Uncaught TypeError: Illegal invocation",
        ),
        // `undefined` stands for the global object, which is no `A`.
        (
            "A.prototype.hello.call(undefined)",
            "Uncaught TypeError: Illegal invocation",
        ),
        (
            "Object.getOwnPropertyDescriptor(A.prototype, 'id').get.call(globalThis)",
            "Uncaught TypeError: Illegal invocation",
        ),
        ("A.prototype.hello.call(make('C'))", "=> A#1"),
        (
            "try { A.prototype.id } catch (e) { e instanceof TypeError }",
            "=> true",
        ),
    ]);
}

#[test]
fn interface_objects_throw_chromium_messages() {
    check_host_cases(&[
        (
            "new B()",
            "Uncaught TypeError: Failed to construct 'B': Illegal constructor",
        ),
        ("B()", "Uncaught TypeError: Illegal constructor"),
        (
            "A()",
            "Uncaught TypeError: Failed to construct 'A': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        ),
        (
            "class E extends B {} new E()",
            "Uncaught TypeError: Failed to construct 'B': Illegal constructor",
        ),
    ]);
}

#[test]
fn the_global_object_is_a_window() {
    check_host_cases(&[
        (
            "[globalThis instanceof Window, globalThis instanceof EventTarget, Object.getPrototypeOf(globalThis) === Window.prototype, this === globalThis].join()",
            "=> true,true,true,true",
        ),
        (
            "Object.prototype.toString.call(globalThis) + Object.prototype.toString.call(Object.getPrototypeOf(Window.prototype))",
            "=> [object Window][object WindowProperties]",
        ),
        (
            "Object.getPrototypeOf(Object.getPrototypeOf(Window.prototype)) === EventTarget.prototype",
            "=> true",
        ),
        // Global functions and `var`s are own properties as before.
        (
            "var v = 3; function f() { return v + Math.max(1, 2); } f() + f()",
            "=> 10",
        ),
    ]);
}

/// Messages name a host object by the constructor of its prototype chain,
/// as V8 does (Chromium 148 gives `#<HTMLDivElement>` and `#<Window>`).
#[test]
fn messages_name_host_objects_by_their_constructor() {
    check_host_cases(&[
        (
            "'use strict'; var o = make('C'); Object.defineProperty(o, 'x', { value: 1 }); try { o.x = 2; } catch (e) { String(e); }",
            "=> TypeError: Cannot assign to read only property 'x' of object '#<C>'",
        ),
        (
            "'use strict'; Object.defineProperty(globalThis, 'qq', { value: 1 }); try { globalThis.qq = 2; } catch (e) { String(e); }",
            "=> TypeError: Cannot assign to read only property 'qq' of object '#<Window>'",
        ),
        // An exotic host object (hooks) is named the same way, where its
        // refusal hook leaves the message to the engine.
        (
            "'use strict'; var l = makeList('a'); Object.defineProperty(l, 'k', { value: 1 }); try { delete l.k; } catch (e) { String(e); }",
            "=> TypeError: Cannot delete property 'k' of #<List>",
        ),
        (
            "'use strict'; var l = makeList('a'); try { Array.prototype.pop.call(l); } catch (e) { String(e); }",
            "=> TypeError: List refused Delete of [0]",
        ),
    ]);
}

#[test]
fn named_properties_on_the_chain_of_the_global_object_are_live() {
    check_host_cases(&[
        // A name that only the named properties object has.
        (
            "setNamed('item', 5); [typeof item, item, 'item' in globalThis, globalThis.hasOwnProperty('item')].join()",
            "=> number,5,true,false",
        ),
        // The caches of global reads see a change of the named properties:
        // the read crosses the exotic object and is never cached.
        (
            "function read() { return typeof item === 'undefined' ? 'none' : item; } var r = [read(), read()]; setNamed('item', 1); r.push(read(), read()); setNamed('item', 2); r.push(read()); setNamed('item', undefined); r.push(read()); r.join()",
            "=> none,none,1,1,2,none",
        ),
        // The same through `globalThis.name` and through a with-free name
        // in a loop.
        (
            "var out = []; for (var i = 0; i < 3; i++) { setNamed('n', i * 10); out.push(globalThis.n, n); } out.join()",
            "=> 0,0,10,10,20,20",
        ),
        // An own global shadows the named property, and the cache of the
        // own property stays valid.
        (
            "setNamed('own', 1); var own = 2; function g() { return own; } [g(), g(), own].join()",
            "=> 2,2,2",
        ),
        // The object refuses definitions.
        (
            "setNamed('x', 1); Reflect.defineProperty(Object.getPrototypeOf(Window.prototype), 'y', { value: 1 })",
            "=> false",
        ),
        ("item", "Uncaught ReferenceError: item is not defined"),
    ]);
}

#[test]
fn caches_of_ordinary_sites_do_not_store_exotic_host_objects() {
    check_host_cases(&[
        // One site sees a plain object, then a map, then the map after a
        // change: each read is right.
        (
            "function get(o) { return o.x; } var m = makeMap(); var r = [get({ x: 1 }), get({ x: 1 }), get(m)]; m.x = 2; r.push(get(m), get(m)); m.x = 3; r.push(get(m)); r.join()",
            "=> 1,1,,2,2,3",
        ),
        // A map on the prototype chain of an ordinary object.
        (
            "var m = makeMap(); var o = Object.create(m); function get() { return o.x; } var r = [get()]; m.x = 'a'; r.push(get(), get()); m.x = 'b'; r.push(get()); r.join()",
            "=> ,a,a,b",
        ),
        // Writes through one site go to the hooks each time.
        (
            "function put(o, v) { o.x = v; } var m = makeMap(); put({}, 1); put(m, 1); put(m, 2); put(m, { toString() { return 'w'; } }); m.x",
            "=> w",
        ),
        // Wrappers of classes without hooks are cacheable and correct.
        (
            "function id(o) { return o.id; } var r = []; for (var i = 0; i < 3; i++) r.push(id(make('C'))); r.join()",
            "=> 1,2,3",
        ),
    ]);
}
