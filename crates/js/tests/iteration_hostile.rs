//! Hostile cases for symbols, `@@toPrimitive`, `@@hasInstance`, the array
//! iterator, `Object.fromEntries` and `Object.groupBy` (M7 feature 2b):
//! array-likes with a huge `length`, iterators that never end, a registry
//! filled with strings, recursion through the hooks, long chains of bound
//! functions. A case ends with a result, an error or a termination at the
//! deadline or the heap limit; none may overflow the Rust stack, allocate
//! in proportion to a length that the script chose, or run past the
//! deadline.
//!
//! The last tests are cases where swb follows the specification and V8
//! does not (stress mode checks them too).

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, on_big_stack, run_in, runtime, runtime_with};
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

fn is_limit(result: &Result<Value, ScriptError>) -> bool {
    matches!(
        result,
        Err(ScriptError::Terminated(
            Termination::TimeLimit | Termination::HeapLimit
        ))
    )
}

#[test]
fn endless_iterations_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let endless = "var endless = {}; endless[Symbol.iterator] = function () { \
            return { next: function () { return { done: false, value: ['k', 1] }; } }; };";
        rt.eval(endless).unwrap();
        let cases = [
            // The array iterator over an array-like of length 2^53 - 1.
            "var it = Array.prototype.values.call({ length: 9007199254740991 }); while (true) it.next();",
            "var it = Array.prototype.entries.call({ length: 9007199254740991 }); while (true) it.next();",
            "var it = Array.prototype.keys.call({ length: Infinity }); while (true) it.next();",
            // The natives that consume an iterator.
            "Object.fromEntries(endless);",
            "Object.groupBy(endless, function () { return 'k'; });",
            "var big = {}; big[Symbol.iterator] = Array.prototype[Symbol.iterator]; big.length = 9007199254740991; \
             Object.groupBy(big, function (x, i) { return i; });",
            // Chains of calls.
            "while (true) Object.fromEntries([['a', 1], ['b', 2]]);",
            "while (true) Object.groupBy([1, 2, 3], function (x) { return x % 2; });",
            "while (true) { var s = Symbol('x'); Symbol.for('x'); Symbol.keyFor(s); }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(is_limit(&result), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "print(1 + 1)"), "2");
    });
}

#[test]
fn endless_iterations_with_growing_results_end_at_a_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        let cases = [
            "var n = 0; var it = {}; it[Symbol.iterator] = function () { return { next: function () { \
             return { done: false, value: ['k' + n++, n] }; } }; }; Object.fromEntries(it);",
            "var n = 0; var it = {}; it[Symbol.iterator] = function () { return { next: function () { \
             return { done: false, value: n++ }; } }; }; Object.groupBy(it, function () { return 'one'; });",
            "var n = 0; var it = {}; it[Symbol.iterator] = function () { return { next: function () { \
             return { done: false, value: n++ }; } }; }; Object.groupBy(it, function (x) { return 'g' + x; });",
            // The registry counts against the heap limit.
            "for (var i = 0; ; i++) Symbol.for('registered key number ' + i);",
            "var keep = []; for (var i = 0; ; i++) keep.push(Symbol('description of ' + i));",
        ];
        for source in cases {
            let start = Instant::now();
            rt.set_deadline(Some(start + Duration::from_secs(20)));
            let result = rt.eval(source);
            rt.set_deadline(None);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::HeapLimit))),
                "{source}: {result:?}"
            );
            assert!(start.elapsed() < Duration::from_secs(15), "{source}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn the_registry_keeps_its_symbols_alive() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var first = Symbol.for('first'); \
             for (var i = 0; i < 20000; i++) Symbol.for('k' + i); \
             gc(); gc(); \
             print(first === Symbol.for('first'), Symbol.keyFor(first), Symbol.for('k19999').description);";
        assert_eq!(run_in(&mut rt, source), "true first k19999");
        assert_clean(&rt);
        // Unregistered symbols are collected.
        let before = rt.heap().counts().symbols;
        let source = "for (var i = 0; i < 20000; i++) Symbol('temporary ' + i); gc();";
        run_in(&mut rt, source);
        let after = rt.heap().counts().symbols;
        assert!(after < before + 100, "{before} -> {after}");
    });
}

#[test]
fn recursion_through_the_hooks_is_a_range_error() {
    let cases = [
        (
            "var o = {}; o[Symbol.toPrimitive] = function () { return +o; }; \
             try { +o; } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var o = {}; o[Symbol.hasInstance] = function (v) { return v instanceof o; }; \
             try { 1 instanceof o; } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var o = {}; o[Symbol.iterator] = function () { return Object.fromEntries(o); }; \
             try { Object.fromEntries(o); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var it = {}; it[Symbol.iterator] = function () { return { next: function () { \
             return Object.groupBy(it, function () { return 1; }); } }; }; \
             try { Object.groupBy(it, function () { return 1; }); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var o = {}; o[Symbol.toStringTag] = 'x'; \
             Object.defineProperty(o, Symbol.toStringTag, { get: function () { return Object.prototype.toString.call(o); } }); \
             try { Object.prototype.toString.call(o); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
    ];
    check_cases(&cases);
}

#[test]
fn long_chains_of_bound_functions_work_in_instanceof() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "function F() {} var f = new F(); var b = F; \
             for (var i = 0; i < 5000; i++) b = b.bind(); \
             print(f instanceof b, {} instanceof b, b[Symbol.hasInstance](f));";
        assert_eq!(run_in(&mut rt, source), "true false true");
        assert_clean(&rt);
        // A hook at the end of the chain is called once.
        let source = "var n = 0; function G() {} Object.defineProperty(G, Symbol.hasInstance, \
             { value: function (v) { n++; return true; } }); var b = G; \
             for (var i = 0; i < 5000; i++) b = b.bind(); \
             print(1 instanceof b, n);";
        assert_eq!(run_in(&mut rt, source), "true 1");
        assert_clean(&rt);
        // A long prototype chain and `instanceof` in a loop end at the deadline.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "function P() {} var top = {}; for (var i = 0; i < 200000; i++) top = Object.create(top); \
             while (true) top instanceof P;",
        );
        assert!(is_limit(&result), "{result:?}");
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
    });
}

#[test]
fn a_huge_symbol_to_string_tag_counts_against_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 128 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        let source = "var t = 'a'; for (var i = 0; i < 26; i++) t += t; var o = {}; o[Symbol.toStringTag] = t; \
             var r = []; while (true) r.push(Object.prototype.toString.call(o));";
        let result = rt.eval(source);
        assert!(
            matches!(
                result,
                Err(ScriptError::Terminated(Termination::HeapLimit) | ScriptError::Uncaught { .. })
            ),
            "{result:?}"
        );
        assert_clean(&rt);
    });
}

#[test]
fn the_specification_wins_over_v8() {
    // After `length` or an element getter threw, the array iterator is
    // finished (§23.1.5.1: the closure of the generator has completed);
    // V8 keeps going.
    check_cases(&[
        (
            "var o = { length: 3 }; Object.defineProperty(o, '1', { get: function () { throw new RangeError('el'); } }); \
             var it = Array.prototype.values.call(o); it.next(); \
             try { it.next(); } catch (e) { print(e.message); } print(it.next().done);",
            "el\ntrue",
        ),
        (
            "var calls = 0; var o = {}; Object.defineProperty(o, 'length', { get: function () { calls++; throw new RangeError('ln'); } }); \
             var it = Array.prototype.keys.call(o); \
             try { it.next(); } catch (e) { print(e.message); } print(it.next().done, calls);",
            "ln\ntrue 1",
        ),
        // `Object.fromEntries` does not close an iterator whose `next`
        // throws or returns a non-object (§7.4.10 sets [[Done]]; V8 closes).
        (
            "var closed = 0; var it = {}; it[Symbol.iterator] = function () { return { \
             next: function () { throw new SyntaxError('nx'); }, 'return': function () { closed++; return {}; } }; }; \
             try { Object.fromEntries(it); } catch (e) { print(e.message); } \
             var it2 = {}; it2[Symbol.iterator] = function () { return { \
             next: function () { return 1; }, 'return': function () { closed++; return {}; } }; }; \
             try { Object.fromEntries(it2); } catch (e) { print(e.message); } print(closed);",
            "nx\nIterator result 1 is not an object\n0",
        ),
        // `return` runs once, after the entry check failed.
        (
            "var closed = 0; var it = {}; it[Symbol.iterator] = function () { return { \
             next: function () { return { done: false, value: 1 }; }, 'return': function () { closed++; return {}; } }; }; \
             try { Object.fromEntries(it); } catch (e) { print(e.message); } print(closed);",
            "Iterator value 1 is not an entry object\n1",
        ),
    ]);
}
