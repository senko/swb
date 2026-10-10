//! Hostile cases for errors, `stack`, `Function.prototype.toString` and the
//! `Array` methods of M7 feature 2c: a limit on frames that a script
//! chooses, huge names and messages, endless iterables, array-likes with a
//! huge `length`, huge function bodies, and a `stack` getter that reads
//! `stack` again. A case ends with a result, an error or a termination at
//! the deadline or the heap limit; none may overflow the Rust stack,
//! allocate in proportion to a value that the script chose, or run past the
//! deadline. Stress mode checks the rooting of the cases that finish.

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
fn endless_and_huge_inputs_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var endless = {}; endless[Symbol.iterator] = function () { \
             return { next: function () { return { done: false, value: 1 }; } }; };",
        )
        .unwrap();
        let cases = [
            // `AggregateError` over an iterable that never ends.
            "new AggregateError(endless);",
            "while (true) new AggregateError([1, 2, 3]);",
            // `indexOf` and `slice` over array-likes of length 2^53 - 1.
            "Array.prototype.indexOf.call({ length: 9007199254740991 }, 'x');",
            "Array.prototype.indexOf.call({ length: 9007199254740991 }, 'x', -9007199254740991);",
            "Array.prototype.slice.call({ length: 4294967295 });",
            "Array.prototype.slice.call({ length: 4294967295 }, 0, 4294967295);",
            // Capturing stacks without end.
            "while (true) new Error('x');",
            "while (true) Error.captureStackTrace({});",
            "while (true) { var e = new TypeError('x'); e.stack; }",
            // `captureStackTrace` looks for a function that is not on a
            // stack 8,000 frames deep.
            "function notThere() {} function deep(n) { if (n) return deep(n - 1); while (true) Error.captureStackTrace({}, notThere); } deep(8000);",
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
fn a_huge_stack_trace_limit_is_capped() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "Error.stackTraceLimit = 1e9;
            function r(n) {
                if (n) return r(n - 1);
                var e;
                for (var i = 0; i < 300; i++) e = new Error('x');
                return e;
            }
            var e = r(9000);
            print(e.stack.length < 200 * 60, e.stack.length > 100);
            Error.stackTraceLimit = Infinity;
            var e2 = r(9000);
            print(e2.stack.length < 200 * 60);
            var o = {}; Error.captureStackTrace(o); print(o.stack.length < 200 * 60);";
        let start = Instant::now();
        assert_eq!(run_in(&mut rt, source), "true true\ntrue\ntrue");
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_clean(&rt);
    });
}

#[test]
fn huge_names_and_messages_end_with_an_error_or_a_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        let build = "var s = 'xy'; for (var i = 0; i < 24; i++) s += s; ";
        let cases = [
            "Error.prototype.toString.call({ name: s, message: s });",
            "var e = new Error(s); e.name = s; e.stack;",
            "var out = []; for (var i = 0; i < 50; i++) out.push(Error.prototype.toString.call({ name: s, message: s }));",
            "var o = { name: s, message: s }; for (var i = 0; i < 50; i++) Error.captureStackTrace(o);",
            "function f() { return new Error(s); } f.name; var g = f.bind(); Object.defineProperty(g, 'name', { value: s }); for (var i = 0; i < 50; i++) g().stack;",
        ];
        for source in cases {
            let source = format!("{build}{source}");
            let start = Instant::now();
            rt.set_deadline(Some(start + Duration::from_secs(20)));
            let result = rt.eval(&source);
            rt.set_deadline(None);
            // Either a value, a RangeError for the string length, or a
            // termination: never a panic and never a hang.
            match &result {
                Ok(_) | Err(ScriptError::Uncaught { .. } | ScriptError::Terminated(_)) => {}
                other => panic!("{source}: {other:?}"),
            }
            assert!(start.elapsed() < Duration::from_secs(15), "{source}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn function_to_string_of_a_huge_body_charges_its_copies() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 128 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // A 10 MB function body (a comment: the compile data of 5 million
        // statements would use the heap limit before `toString` runs).
        let body = "x".repeat(10_000_000 - 4);
        let source =
            format!("function big() {{/*{body}*/}} var text = big.toString(); text.length;");
        let start = Instant::now();
        rt.set_deadline(Some(start + Duration::from_secs(20)));
        let result = rt.eval(&source);
        let expected = format!("function big() {{/*{body}*/}}").len() as i32;
        assert_eq!(result, Ok(Value::Int(expected)));
        // Copies in a loop end at the heap limit or the deadline.
        rt.set_deadline(Some(Instant::now() + DEADLINE));
        let result = rt.eval("var keep = []; while (true) keep.push(big.toString());");
        rt.set_deadline(None);
        assert!(is_limit(&result), "{result:?}");
        assert!(start.elapsed() < Duration::from_secs(15));
        assert_clean(&rt);
    });
}

#[test]
fn a_stack_getter_that_reads_the_stack_again_does_not_recurse() {
    check_cases(&[(
        "function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
         t(function () {
             var e = new Error('m');
             var depth = 0;
             Object.defineProperty(e, 'name', { get: function () { depth++; return typeof e.stack; } });
             return typeof e.stack + ',' + depth;
         });
         t(function () {
             var e = new Error('m');
             Object.defineProperty(e, 'message', { get: function () { throw new RangeError('boom'); } });
             var first; try { e.stack; } catch (x) { first = x.message; }
             var second; try { e.stack; } catch (x) { second = x.message; }
             return first + ',' + second;
         });
         t(function () {
             var e = new Error('m');
             Object.defineProperty(e, 'name', { get: function () { e.stack = 'replaced'; return 'N'; } });
             return e.stack;
         });
         t(function () {
             var e = new Error('m');
             Error.captureStackTrace(e);
             var o = Object.create(e);
             Error.captureStackTrace(o);
             return typeof o.stack + ',' + typeof e.stack;
         });",
        "string,1
boom,boom
replaced
string,string",
    )]);
}

#[test]
fn retained_errors_with_deep_stacks_end_at_the_heap_limit() {
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
            "var keep = []; function r(n) { if (n) return r(n - 1); for (;;) keep.push(new Error('x')); } Error.stackTraceLimit = 1e9; r(300);",
            "var keep = []; function r(n) { if (n) return r(n - 1); for (;;) { var o = {}; Error.captureStackTrace(o); keep.push(new TypeError('x')); } } r(300);",
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
fn errors_survive_collections_with_their_stacks() {
    check_cases(&[(
        "var kept = [];
         function make(n) { return n ? make(n - 1) : new TypeError('deep ' + n); }
         for (var i = 0; i < 40; i++) {
             kept.push(make(3));
             if (i % 10 == 0) gc();
         }
         gc();
         var all = kept.map(function (e) { return e.stack.length; });
         print(all.length, all[0] > 20);
         var fresh = [1, 2, 3].map(function (x) { try { null.x; } catch (e) { return e; } });
         gc();
         print(fresh[0].stack.length > 20, fresh[2].message);
         function Holder() { this.e = new Error('held'); gc(); }
         var h = new Holder(); gc();
         print(typeof h.e.stack, h.e.stack.length > 10);",
        "40 true
true Cannot read properties of null (reading 'x')
string true",
    )]);
}
