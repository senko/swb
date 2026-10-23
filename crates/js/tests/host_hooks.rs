//! Hooks for the internal methods of host objects (ADR 0028 section 6,
//! item 2): a legacy indexed object (`List`, Web IDL §3.9 with an indexed
//! getter), a named-property object (`NamedMap`, a named setter that
//! converts with `ToString`), hooks that throw, hooks that run script code
//! and hooks that reach each other.

mod common;

use common::host::{check_host_cases, host_runtime, run_host};

#[test]
fn an_indexed_object_is_array_like() {
    check_host_cases(&[
        (
            "var l = makeList('a', 'b', 'c'); [l.length, l[0], l[2], l[3], l['1']].join()",
            "=> 3,a,c,,b",
        ),
        (
            "var l = makeList('a', 'b'); Object.keys(l).join() + ' ' + JSON.stringify(l)",
            "=> 0,1 {\"0\":\"a\",\"1\":\"b\"}",
        ),
        (
            "var l = makeList(1, 2, 3); Array.prototype.map.call(l, (x) => x * 2).join()",
            "=> 2,4,6",
        ),
        // The enumerable `length` accessor of the prototype too (Chromium
        // lists `length` of a `NodeList` the same way).
        (
            "var l = makeList('x', 'y'); var out = []; for (var k in l) out.push(k); out.join()",
            "=> 0,1,length",
        ),
        (
            "var l = makeList('x', 'y'); [...Array.from(l)].join() + (1 in l) + (2 in l)",
            "=> x,ytruefalse",
        ),
        (
            "var l = makeList('x'); JSON.stringify(Object.getOwnPropertyDescriptor(l, 0))",
            "=> {\"value\":\"x\",\"writable\":false,\"enumerable\":true,\"configurable\":true}",
        ),
        // Writes and definitions of indices are refused; deletion of a
        // supported index fails, of another index succeeds.
        (
            "var l = makeList('x'); l[0] = 'y'; l[5] = 'z'; [l[0], l[5], l.length].join()",
            "=> x,,1",
        ),
        (
            "'use strict'; var l = makeList('x'); try { l[0] = 'y'; } catch (e) { e instanceof TypeError }",
            "=> true",
        ),
        (
            "var l = makeList('x'); [Reflect.defineProperty(l, 0, { value: 1 }), delete l[0], delete l[1]].join()",
            "=> false,false,true",
        ),
        (
            "var l = makeList('x'); Object.defineProperty(l, 1, { value: 1 })",
            "Uncaught TypeError: Cannot define property 1, object is not extensible",
        ),
        // The list changes under the script (the object reads the host at
        // each access, also through one cached site).
        (
            "var l = makeList(); function len(o) { return o.length + ':' + o[o.length - 1]; } var r = []; for (var i = 0; i < 3; i++) { listPush(l, i); r.push(len(l)); } r.join()",
            "=> 1:0,2:1,3:2",
        ),
        // Expandos with string keys are stored.
        (
            "var l = makeList('a'); l.extra = 1; [l.extra, Object.keys(l).join()].join(' ')",
            "=> 1 0,extra",
        ),
    ]);
}

#[test]
fn a_named_property_object_converts_with_to_string() {
    check_host_cases(&[
        (
            "var m = makeMap(); m.a = 1; m.b = true; [typeof m.a, m.a, m.b, Object.keys(m).join()].join()",
            "=> string,1,true,a,b",
        ),
        (
            "var m = makeMap(); m.x = { toString() { return 'converted'; } }; m.x",
            "=> converted",
        ),
        (
            "var m = makeMap(); m.a = 1; delete m.a; [m.a, 'a' in m, Object.keys(m).length].join()",
            "=> ,false,0",
        ),
        (
            "var m = makeMap(); Object.defineProperty(m, 'v', { value: 42 }); m.v + typeof m.v",
            "=> 42string",
        ),
        (
            "var m = makeMap(); Object.defineProperty(m, 'g', { get() { return 1; } })",
            "Uncaught TypeError: Cannot define property g, object is not extensible",
        ),
        (
            "var m = makeMap(); m.a = 'x'; var copy = { ...m }; JSON.stringify(copy)",
            "=> {\"a\":\"x\"}",
        ),
        (
            "var m = makeMap(); m.a = 1; Object.getOwnPropertyNames(m).join() + Object.entries(m).join()",
            "=> aa,1",
        ),
        // A symbol key is an ordinary property.
        (
            "var m = makeMap(); var s = Symbol('s'); m[s] = 5; [m[s], Object.keys(m).length].join()",
            "=> 5,0",
        ),
        // [[OwnPropertyKeys]] order: indices ascending, then names.
        (
            "var m = makeMap(); m[1] = 'a'; m[0] = 'b'; m.z = 'c'; Reflect.ownKeys(m).join() + ' ' + JSON.stringify(m)",
            "=> 0,1,z {\"0\":\"b\",\"1\":\"a\",\"z\":\"c\"}",
        ),
        // The stored keys follow the hook's within a group, without
        // duplicates; the hook refuses stored indices.
        (
            "var l = makeList('a', 'b'); var s = Symbol('s'); l[s] = 1; l.extra = 2; \
             Reflect.ownKeys(l).map(String).join()",
            "=> 0,1,extra,Symbol(s)",
        ),
        // `with` over a map: the names come from the hooks.
        (
            "var m = makeMap(); m.inside = 'w'; with (m) { inside + typeof outside }",
            "=> wundefined",
        ),
    ]);
}

#[test]
fn hooks_that_throw_and_hooks_that_run_script() {
    check_host_cases(&[
        // An exception of a hook reaches the script, which can catch it.
        (
            "var l = makeList(); try { l[666] } catch (e) { String(e) }",
            "=> TypeError: the list hook throws",
        ),
        (
            "var l = makeList(); 666 in l",
            "Uncaught TypeError: the list hook throws",
        ),
        // The conversion of a setter runs script code that throws.
        (
            "var m = makeMap(); try { m.a = { toString() { throw new RangeError('no'); } }; } catch (e) { String(e) + ' ' + ('a' in m) }",
            "=> RangeError: no false",
        ),
        // The conversion changes the map while the hook runs.
        (
            "var m = makeMap(); m.b = { toString() { m.a = 'inner'; return 'outer'; } }; [m.a, m.b].join()",
            "=> inner,outer",
        ),
        // A conversion that allocates and collects while the hook holds
        // its arguments.
        (
            "var m = makeMap(); m.big = { toString() { var a = []; for (var i = 0; i < 1000; i++) a.push({ i }); gc(); return 'v' + a.length; } }; m.big",
            "=> v1000",
        ),
    ]);
}

#[test]
fn hooks_that_reach_each_other_end_with_a_range_error() {
    // Each conversion writes the map again: the nesting charges the
    // recursion budget (hook levels and native re-entries) and ends with
    // V8's RangeError, not with a stack overflow.
    check_host_cases(&[(
        "var m = makeMap(); var depth = 0; var v = { toString() { depth++; m.x = v; return 'x'; } }; try { m.x = v; } catch (e) { String(e) + (depth > 10) }",
        "=> RangeError: Maximum call stack size exceededtrue",
    )]);
}

#[test]
fn the_runtime_stays_clean_after_hook_errors() {
    common::on_big_stack(|| {
        let mut rt = host_runtime(false);
        assert_eq!(
            run_host(&mut rt, "var l = makeList(); l[666]"),
            "Uncaught TypeError: the list hook throws"
        );
        common::assert_clean(&rt);
        assert_eq!(run_host(&mut rt, "makeList('a')[0]"), "=> a");
        assert_eq!(rt.recursion_budget(), swb_js::RecursionBudget::DEFAULT);
    });
}
