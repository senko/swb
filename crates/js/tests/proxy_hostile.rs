//! Hostile cases for proxies (M7 feature 9a): chains of a million
//! proxies, traps that call their own proxy or revoke it, `ownKeys`
//! results of a million keys, and traps that make a prototype chain
//! endless. A case ends with a result, an error or a termination at the
//! deadline; none may overflow the Rust stack (the tests run on a thread
//! with an 8 MiB stack, in the debug build of `cargo test`), allocate in
//! proportion to a length that the script chose, or run past the deadline.

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, on_big_stack, run_in, runtime, runtime_with};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// Runs `source` with a deadline; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    eval_within(rt, source, DEADLINE)
}

/// [`eval_with_deadline`] with another deadline.
fn eval_within(
    rt: &mut Runtime,
    source: &str,
    deadline: Duration,
) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + deadline));
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
fn a_chain_of_a_million_proxies_is_a_range_error() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var chain = function () { return 7; }; \
             for (var i = 0; i < 1000000; i++) chain = new Proxy(chain, {}); \
             function attempt(f) { try { return 'ok ' + f(); } catch (e) { return e.constructor.name + ': ' + e.message; } }",
        )
        .unwrap();
        let overflow = "RangeError: Maximum call stack size exceeded";
        let cases = [
            ("attempt(function () { return chain.x; })", overflow),
            ("attempt(function () { chain.x = 1; return 1; })", overflow),
            ("attempt(function () { return 'x' in chain; })", overflow),
            ("attempt(function () { return delete chain.x; })", overflow),
            (
                "attempt(function () { return Object.getOwnPropertyDescriptor(chain, 'x'); })",
                overflow,
            ),
            (
                "attempt(function () { return Object.defineProperty(chain, 'y', { value: 1 }); })",
                overflow,
            ),
            (
                "attempt(function () { return Reflect.ownKeys(chain); })",
                overflow,
            ),
            (
                "attempt(function () { return Object.getPrototypeOf(chain); })",
                overflow,
            ),
            (
                "attempt(function () { return Object.isExtensible(chain); })",
                overflow,
            ),
            (
                "attempt(function () { return Object.preventExtensions(chain); })",
                overflow,
            ),
            (
                "attempt(function () { return Object.setPrototypeOf(chain, null); })",
                overflow,
            ),
            (
                "attempt(function () { return ({}) instanceof chain; })",
                overflow,
            ),
            (
                "attempt(function () { for (var k in chain) {} return 1; })",
                overflow,
            ),
            (
                "attempt(function () { return JSON.stringify(chain); })",
                overflow,
            ),
            (
                "attempt(function () { return Object.keys(chain).length; })",
                overflow,
            ),
            (
                "attempt(function () { return Object.create(chain).x; })",
                overflow,
            ),
            // No trap on the way: `IsArray` and `[[Call]]` are loops.
            (
                "attempt(function () { return Array.isArray(chain); })",
                "ok false",
            ),
            (
                "attempt(function () { return typeof chain; })",
                "ok function",
            ),
            ("attempt(function () { return chain(); })", "ok 7"),
            (
                "attempt(function () { return new chain() instanceof Object; })",
                overflow,
            ),
        ];
        for (source, expected) in cases {
            // A walk of a million levels takes a while in a debug build.
            let (result, elapsed) = eval_within(&mut rt, source, Duration::from_secs(30));
            let text = result.map(|v| rt.display(v));
            assert_eq!(text.as_deref(), Ok(expected), "{source}");
            assert!(elapsed < Duration::from_secs(30), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "print(1 + 1)"), "2");
    });
}

#[test]
fn traps_that_call_their_own_proxy_are_a_range_error() {
    let cases = [
        (
            "var p = new Proxy({}, { get: function (t, k, r) { return r[k]; } }); \
             try { p.x; } catch (e) { print(e.constructor.name, e.message); }",
            "RangeError Maximum call stack size exceeded",
        ),
        (
            "var p = new Proxy({}, { has: function (t, k) { return k in p; } }); \
             try { 'x' in p; } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var p = new Proxy({}, { ownKeys: function () { return Reflect.ownKeys(p); } }); \
             try { Object.keys(p); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var p = new Proxy(function () {}, { apply: function () { return p(); } }); \
             try { p(); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var p = new Proxy(function () {}, { construct: function () { return new p(); } }); \
             try { new p(); } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            "var h = new Proxy({}, { get: function (t, k) { return h[k]; } }); var p = new Proxy({}, h); \
             try { p.x; } catch (e) { print(e.constructor.name); }",
            "RangeError",
        ),
        (
            // The trap revokes the proxy that it serves: the running
            // operation ends with the target it read, the next one throws.
            "var r = Proxy.revocable({ a: 1 }, { get: function (t, k) { r.revoke(); return t[k]; } }); \
             print(r.proxy.a); try { r.proxy.a; } catch (e) { print(e.message); }",
            "1\nCannot perform 'get' on a proxy that has been revoked",
        ),
        (
            "var r = Proxy.revocable({ a: 1 }, { ownKeys: function (t) { r.revoke(); gc(); return ['a']; } }); \
             try { Object.keys(r.proxy); } catch (e) { print(e.message); }",
            "Cannot perform 'getOwnPropertyDescriptor' on a proxy that has been revoked",
        ),
    ];
    check_cases(&cases);
}

#[test]
fn endless_prototype_chains_of_traps_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            "var p = new Proxy({}, { getPrototypeOf: function () { return p; } }); ({}) instanceof function () {}; \
             var F = function () {}; p instanceof F;",
            "var p = new Proxy({}, { getPrototypeOf: function () { return p; } }); for (var k in p) {}",
            "var p = new Proxy({}, { getPrototypeOf: function () { return p; } }); Object.prototype.isPrototypeOf.call(Array.prototype, p);",
            "var n = 0; var p = new Proxy({}, { getPrototypeOf: function () { return p; }, \
             ownKeys: function () { return ['k' + (n++ % 100)]; } }); for (var k in p) {}",
            "var list = new Proxy({}, { get: function (t, k) { return k === 'length' ? 4294967295 : 'k'; } }); \
             var p = new Proxy({}, { ownKeys: function () { return list; } }); Reflect.ownKeys(p);",
            "var p = new Proxy([], { get: function (t, k) { return k === 'length' ? 4294967295 : undefined; } }); \
             JSON.stringify(p);",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(
                        Termination::TimeLimit | Termination::HeapLimit
                    ))
                ),
                "{source}: {result:?}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn a_million_keys_check_in_linear_time() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var keys = []; for (var i = 0; i < 1000000; i++) keys.push('k' + i); \
             var t = {}; for (var i = 0; i < keys.length; i++) t[keys[i]] = i; Object.preventExtensions(t); \
             var reversed = keys.slice().reverse(); \
             var n = Reflect.ownKeys(new Proxy(t, { ownKeys: function () { return reversed; } })).length; \
             keys.push('k5'); var message; \
             try { Reflect.ownKeys(new Proxy({}, { ownKeys: function () { return keys; } })); } catch (e) { message = e.message; } \
             keys.pop(); keys.pop(); var missing; \
             try { Reflect.ownKeys(new Proxy(t, { ownKeys: function () { return keys; } })); } catch (e) { missing = e.message; } \
             print(n, message, missing);";
        let start = Instant::now();
        let output = run_in(&mut rt, source);
        assert_eq!(
            output,
            "1000000 'ownKeys' on proxy: trap returned duplicate entries 'ownKeys' on proxy: trap result did not include 'k999999'"
        );
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "{:?}",
            start.elapsed()
        );
        assert_clean(&rt);
    });
}

#[test]
fn growing_trap_results_end_at_the_heap_limit() {
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
            // An `ownKeys` result with a new key at each index.
            "var big = new Proxy([], { get: function (t, k) { return k === 'length' ? 1e9 : 'key' + k; } }); \
             Reflect.ownKeys(new Proxy({}, { ownKeys: function () { return big; } }));",
            // A `for`-`in` walk over a chain whose `ownKeys` traps give new keys.
            "var n = 0; var p = new Proxy({}, { getPrototypeOf: function () { return p; }, \
             ownKeys: function () { var out = []; for (var i = 0; i < 1000; i++) out.push('k' + n++); return out; } }); \
             for (var k in p) {}",
        ];
        for source in cases {
            let start = Instant::now();
            rt.set_deadline(Some(start + Duration::from_secs(30)));
            let result = rt.eval(source);
            rt.set_deadline(None);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(
                        Termination::HeapLimit | Termination::TimeLimit
                    ))
                ),
                "{source}: {result:?}"
            );
            assert!(start.elapsed() < Duration::from_secs(40), "{source}");
            assert_clean(&rt);
        }
    });
}
