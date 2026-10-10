//! Hostile cases for `Object`, `Reflect`, `bind` and the property paths
//! (M7 feature 2a): huge lengths and indices, long prototype chains, many
//! keys, deep chains of bound functions. A case ends with a result, an
//! error or a termination at the deadline; none may overflow the Rust
//! stack, allocate in proportion to a length that the script chose, or run
//! past the deadline.

mod common;

use std::time::{Duration, Instant};

use common::{on_big_stack, run, run_in, runtime, runtime_with};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(100);

/// Runs `source` with a deadline; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + DEADLINE));
    let result = rt.eval(source);
    rt.set_deadline(None);
    (result, start.elapsed())
}

fn assert_clean(rt: &Runtime) {
    assert_eq!(rt.stack_depths(), (0, 0));
    assert_eq!(rt.heap().scope_depth(), 0);
    assert_eq!(rt.heap().no_gc_depth(), 0);
    assert_eq!(rt.heap().stats().total_stale_roots, 0);
}

#[test]
fn huge_lengths_fail_or_work_without_allocating() {
    let cases = [
        (
            "var a = Object.defineProperty([], 'length', { value: 4294967295 }); Object.freeze(a); \
             print(a.length, Object.isFrozen(a), Object.getOwnPropertyNames(a).join());",
            "4294967295 true length",
        ),
        (
            "function f() { return arguments.length; } \
             try { Reflect.apply(f, null, { length: 4294967295 }); } catch (e) { print(e.constructor.name, e.message); }",
            "RangeError Invalid array length",
        ),
        (
            "function f() { return arguments.length; } \
             try { Reflect.construct(f, { length: 9007199254740991 }); } catch (e) { print(e.constructor.name, e.message); }",
            "RangeError Invalid array length",
        ),
        (
            "function f() { } \
             try { f.apply(null, { length: 4294967294 }); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var a = []; a[4294967294] = 1; Object.seal(a); \
             print(Object.keys(a).join(), Object.entries(a)[0].join(), Object.isSealed(a), Object.isFrozen(a));",
            "4294967294 4294967294,1 true false",
        ),
        (
            "var a = []; a.length = 4294967295; \
             print(Object.keys(a).length, Object.values(a).length, Object.assign({}, a).hasOwnProperty('0'), \
             Object.getOwnPropertyNames(a).join(), Reflect.ownKeys(a).length);",
            "0 0 false length 1",
        ),
        (
            "var o = { length: 4294967295 }; \
             try { Function.prototype.apply.call(function () { }, null, o); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var a = []; a[1e9] = 1; a[5] = 2; var d = Object.getOwnPropertyDescriptors(a); \
             print(Object.keys(d).join());",
            "5,1000000000,length",
        ),
        (
            // The string of a String object is not copied into keys one by one for `hasOwn`.
            "var t = ''; for (var i = 0; i < 1000; i++) t += 'x'; var s = new String(t); \
             print(Object.keys(s).length, Object.hasOwn(s, 999), Object.hasOwn(s, 1000), Object.isFrozen(Object.freeze(s)));",
            "1000 true false true",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(run(source, false), expected, "{source}");
    }
}

#[test]
fn stress_mode_agrees_on_the_hostile_cases() {
    let source = "var a = Object.defineProperty([], 'length', { value: 4294967295 }); \
        Object.freeze(a); var o = {}; for (var i = 0; i < 300; i++) o['k' + i] = i; \
        var d = Object.getOwnPropertyDescriptors(o); var c = Object.create(null, d); \
        print(Object.keys(c).length, Object.entries(o).length, Object.isFrozen(Object.freeze(c)));";
    assert_eq!(run(source, true), "300 300 true");
    assert_eq!(run(source, false), "300 300 true");
}

#[test]
fn a_long_prototype_chain_charges_the_cycle_check() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "var top = {}; for (var i = 0; i < 300000; i++) top = Object.create(top); \
                     var low = {}; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        // Every call walks the whole chain (no cycle, so it succeeds), and
        // the loop has no other backward-jump cost worth the name.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "while (true) { Object.setPrototypeOf(low, top); Object.setPrototypeOf(low, null); }",
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
        // The same through `__proto__` and `isPrototypeOf`.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "while (true) { low.__proto__ = top; low.__proto__ = null; Object.prototype.isPrototypeOf.call(low, top); }",
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
        // A cycle through the chain is found.
        assert_eq!(
            run_in(
                &mut rt,
                "var r; try { Object.setPrototypeOf(top, Object.create(null)); Object.setPrototypeOf(Object.getPrototypeOf(top), top); } catch (e) { r = e.message; } print(r);"
            ),
            "Cyclic __proto__ value"
        );
    });
}

#[test]
fn key_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "var big = {}; for (var i = 0; i < 1000000; i++) big['k' + i] = i; \
                     var descs = {}; for (var j = 0; j < 200000; j++) descs['k' + j] = {}; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        let cases = [
            "while (true) Object.getOwnPropertyNames(big);",
            "while (true) Object.keys(big);",
            "while (true) Object.values(big);",
            "while (true) Object.entries(big);",
            "while (true) Reflect.ownKeys(big);",
            "while (true) Object.assign({}, big);",
            "while (true) Object.getOwnPropertyDescriptors(big);",
            "while (true) Object.isFrozen(big);",
            "while (true) Object.isSealed(big);",
            "while (true) Object.defineProperties({}, descs);",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // One call on a copy, ended by the deadline in the middle of it.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "var c = Object.assign({}, big); Object.freeze(c); Object.freeze(Object.seal(c));",
        );
        assert!(
            matches!(
                result,
                Ok(_) | Err(ScriptError::Terminated(Termination::TimeLimit))
            ),
            "{result:?}"
        );
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        assert_clean(&rt);
        assert_eq!(run_in(&mut rt, "print(1 + 1)"), "2");
    });
}

#[test]
fn sparse_and_huge_arrays_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            "var a = []; a.length = 4294967295; while (true) { Object.freeze(a); Object.keys(a); Object.assign({}, a); }",
            "var a = []; a[4294967294] = 1; while (true) { Object.entries(a); Object.isFrozen(a); Object.getOwnPropertyDescriptors(a); }",
            "var a = [1, 2, 3]; while (true) { Object.defineProperty(a, 'length', { value: 4294967295 }); a.length = 3; }",
            "var o = { length: 4294967295 }; while (true) { try { Reflect.apply(function () { }, null, o); } catch (e) { } }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn bound_chains_are_bounded_without_rust_recursion() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // `bind` of `bind` of ... : the name grows with the depth, so the
        // 5,461st level fails; the chain below it works.
        let out = run_in(
            &mut rt,
            "var f = function () { return 'ok'; }; var n = 0; \
             try { for (;;) { f = f.bind(); n++; } } catch (e) { print(e.constructor.name, e.message, n > 4000); } \
             var g = f.bind;",
        );
        assert_eq!(out, "RangeError Invalid string length true");
        // Every level adds bound arguments: a call that cannot fit them
        // into the value stack is a RangeError.
        let out = run_in(
            &mut rt,
            "var args = [null]; for (var i = 0; i < 100000; i++) args.push(i); \
             var h = function () { return arguments.length; }; \
             for (var j = 0; j < 60; j++) { h = Function.prototype.bind.apply(h, args); } \
             try { print(h()); } catch (e) { print(e.constructor.name, e.message); }",
        );
        assert_eq!(out, "RangeError Maximum call stack size exceeded");
        assert_clean(&rt);
    });
}

#[test]
fn bound_arguments_are_charged_per_call() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // One million bound arguments, copied at every call.
        let build = "function g() { return arguments.length; } var a = [null]; a.length = 1000001; \
                     var f = Function.prototype.bind.apply(g, a); undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        let (result, elapsed) = eval_with_deadline(&mut rt, "while (true) f();");
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
    });
}

#[test]
fn keys_of_a_long_string_object_count_against_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 128 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // The string fits; the list of its keys (24 bytes per unit) does not.
        rt.eval(
            "var t = 'a'; for (var i = 0; i < 24; i++) t += t; var s = new String(t); undefined;",
        )
        .unwrap();
        let cases = [
            "Object.keys(s)",
            "Object.values(s)",
            "Object.entries(s)",
            "Object.getOwnPropertyNames(s)",
            "Reflect.ownKeys(s)",
            "Object.assign({}, s)",
            "Object.freeze(s)",
            "Object.seal(s)",
            "Object.isFrozen(Object.preventExtensions(s))",
            "Object.getOwnPropertyDescriptors(s)",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::HeapLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
        }
    });
}
