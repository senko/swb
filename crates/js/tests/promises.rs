//! `Promise` and the job queue (M7 feature 7b): the measured Chromium cases
//! (`tools/probes/js-promises.json`, with the order of the jobs), promises
//! and jobs that allocate and collect, and the embedding API of the job
//! queue (`run_jobs`, the rejection tracker, host reactions, errors and
//! terminations in jobs). Each case except the heavy ones also runs in the
//! GC stress mode.

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, check_chromium, on_big_stack, run_in, runtime};
use swb_js::{PromiseStatus, ReactionKind, Runtime, ScriptError, Termination, Value, VmResult};

/// The snippets of `tools/probes/js-promises.json` with Chromium 148's
/// results and job order (`data/promises_cases.expected`; the script and
/// the expected output are made by `data/gen-promise-cases.js` from the
/// output of `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/promises_cases.js"),
        include_str!("data/promises_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with chains of thousands of promises and jobs,
/// without the stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/promises_cases_heavy.js"),
        include_str!("data/promises_cases_heavy.expected"),
        &[false],
    );
}

/// Executors, handlers, thenables, `finally` callbacks, subclasses and
/// the combinators that allocate and collect while only the queue or the
/// reactions of pending promises hold their values (the expected output is
/// that of Node.js 22; `just jsdiff` compares it).
#[test]
fn jobs_with_collections() {
    check_cases(&[(
        include_str!("data/promises_gc.js"),
        include_str!("data/promises_gc.expected").trim_end_matches('\n'),
    )]);
}

/// The jobs run in order after the script, and each drain runs the jobs
/// that the drain enqueues.
#[test]
fn jobs_run_after_the_script_in_order() {
    check_cases(&[
        (
            "var log = []; Promise.resolve().then(() => log.push(1)).then(() => print(log.join()));\
             log.push(0); print('sync');",
            "sync\n0,1",
        ),
        (
            "Promise.reject(new Error('x')).catch(e => print(e.message)); undefined",
            "x",
        ),
    ]);
}

/// A pending promise in a global, a job queue that only this runtime
/// holds, and a collection between two scripts.
#[test]
fn pending_jobs_survive_a_collection() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            rt.eval(
                "var out = []; var p = Promise.resolve({ v: 'kept' }); \
                 for (var i = 0; i < 100; i++) p = p.then(x => ({ v: x.v }));",
            )
            .unwrap();
            assert_eq!(rt.pending_jobs(), 1);
            rt.collect();
            rt.eval("p.then(x => print(x.v));").unwrap();
            rt.collect();
            rt.run_jobs().unwrap();
            assert_eq!(rt.pending_jobs(), 0);
            assert_eq!(run_in(&mut rt, ""), "kept");
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

/// The rejection tracker: a rejection that gets a handler before the
/// host takes the list is not reported; each list starts empty.
#[test]
fn unhandled_rejections_are_tracked_until_handled() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var a = Promise.reject(new TypeError('a')); var b = Promise.reject(2); \
             b.catch(() => {}); var c = Promise.resolve().then(() => { throw 3; }); \
             Promise.reject(4).then(() => {}, () => {});",
        )
        .unwrap();
        rt.run_jobs().unwrap();
        let rejected = rt.take_unhandled_rejections();
        let reasons: Vec<String> = rejected
            .iter()
            .map(|p| {
                let (status, reason) = rt.promise_settlement(*p).unwrap();
                assert_eq!(status, PromiseStatus::Rejected);
                rt.error_report(reason)
            })
            .collect();
        assert_eq!(reasons, ["TypeError: a\n    at <anonymous>:1:24", "3"]);
        assert_eq!(rt.take_unhandled_rejections().len(), 0);
        // A handler that comes later does not report them again.
        rt.eval("a.catch(() => {}); c.catch(() => {});").unwrap();
        rt.run_jobs().unwrap();
        assert_eq!(rt.take_unhandled_rejections().len(), 0);
        // A rejection that a later job handles in the same drain.
        rt.eval("var d = Promise.reject(5); Promise.resolve().then(() => d.catch(() => {}));")
            .unwrap();
        rt.run_jobs().unwrap();
        assert_eq!(rt.take_unhandled_rejections().len(), 0);
    });
}

/// Many rejections that get handlers later: the tracker prunes its list,
/// so it does not grow with the handled ones.
#[test]
fn the_tracker_prunes_handled_rejections() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval("for (var i = 0; i < 100000; i++) Promise.reject(i).catch(() => {});")
            .unwrap();
        rt.run_jobs().unwrap();
        assert_eq!(rt.take_unhandled_rejections().len(), 0);
        rt.eval("var keep = []; for (var i = 0; i < 1000; i++) keep.push(Promise.reject(i));")
            .unwrap();
        assert_eq!(rt.take_unhandled_rejections().len(), 1000);
    });
}

/// The report of a rejection reason: the stack lines of an error object.
#[test]
fn error_reports_show_the_stack() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval_named(
            "function f() { return Promise.reject(new RangeError('deep')); }\nf();",
            "t.js",
        )
        .unwrap();
        let rejected = rt.take_unhandled_rejections();
        let (_, reason) = rt.promise_settlement(rejected[0]).unwrap();
        assert_eq!(
            rt.error_report(reason),
            "RangeError: deep\n    at f (t.js:1:38)\n    at t.js:2:1"
        );
        rt.eval_named(
            "var e = new Error('read'); e.stack; Promise.reject(e);",
            "u.js",
        )
        .unwrap();
        let rejected = rt.take_unhandled_rejections();
        let (_, reason) = rt.promise_settlement(rejected[0]).unwrap();
        assert_eq!(rt.error_report(reason), "Error: read\n    at u.js:1:9");
    });
}

/// A host reaction (`react_to_promise`): the continuation runs as a job
/// with its data and the settlement.
#[test]
fn host_reactions_run_as_jobs() {
    #[allow(clippy::unnecessary_wraps, reason = "the signature of continuations")]
    fn note(rt: &mut Runtime, data: Value, kind: ReactionKind, value: Value) -> VmResult<Value> {
        let line = format!("{kind:?} {} {}", rt.display(data), rt.display(value));
        if let Some(output) = rt.host_data_mut::<common::Output>() {
            output.0.borrow_mut().push(line);
        }
        Ok(Value::Undefined)
    }
    on_big_stack(|| {
        let mut rt = runtime(false);
        let fulfilled = rt.eval("Promise.resolve(7)").unwrap();
        assert!(rt.react_to_promise(fulfilled, note, Value::Int(1)).unwrap());
        let rejected = rt.eval("var r = Promise.reject('no'); r").unwrap();
        assert!(rt.react_to_promise(rejected, note, Value::Int(2)).unwrap());
        assert!(
            !rt.react_to_promise(Value::Int(3), note, Value::Int(3))
                .unwrap()
        );
        rt.run_jobs().unwrap();
        assert_eq!(run_in(&mut rt, ""), "Fulfill 1 7\nReject 2 no");
        // The reaction counts as a handler.
        assert_eq!(rt.take_unhandled_rejections().len(), 0);
    });
}

/// An exception that no promise takes (a capability function of a
/// subclass that throws) ends the drain with that error; the next call
/// runs the remaining jobs.
#[test]
fn errors_in_jobs_reach_the_host() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "function C(e) { e(() => { throw new Error('resolve threw'); }, () => {}); } \
             var p = Promise.resolve(1); p.constructor = { [Symbol.species]: C }; \
             p.then(v => v); Promise.resolve().then(() => print('after'));",
        )
        .unwrap();
        let error = rt.run_jobs().unwrap_err();
        assert!(
            matches!(&error, ScriptError::Uncaught { message, .. } if message == "resolve threw"),
            "{error:?}"
        );
        assert_eq!(rt.stack_depths(), (0, 0));
        rt.run_jobs().unwrap();
        assert_eq!(run_in(&mut rt, ""), "after");
    });
}

/// A termination inside a job reaches the host as a termination, never
/// as a rejection; the jobs that were not run stay in the queue until the
/// host drops them.
#[test]
fn a_termination_in_a_job_is_not_a_rejection() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var caught = false; \
             var p = Promise.resolve().then(() => { for (;;) {} }); \
             p.catch(() => { caught = true; }); \
             Promise.resolve().then(() => print('second'));",
        )
        .unwrap();
        let start = Instant::now();
        rt.set_deadline(Some(start + Duration::from_millis(100)));
        let error = rt.run_jobs().unwrap_err();
        assert_eq!(error, ScriptError::Terminated(Termination::TimeLimit));
        assert!(start.elapsed() < Duration::from_secs(2));
        rt.set_deadline(None);
        assert_eq!(rt.stack_depths(), (0, 0));
        assert_eq!(rt.heap().scope_depth(), 0);
        // The second job is still queued; `p` is still pending.
        assert_eq!(rt.pending_jobs(), 1);
        let p = rt.eval("p").unwrap();
        assert_eq!(rt.promise_settlement(p).unwrap().0, PromiseStatus::Pending);
        rt.clear_jobs();
        rt.run_jobs().unwrap();
        assert_eq!(run_in(&mut rt, "caught"), "=> false");
    });
}

/// `run_jobs` from a native function that script code called does
/// nothing: jobs run only when no frame is on the stack.
#[test]
fn jobs_do_not_run_inside_script_code() {
    #[allow(
        clippy::unnecessary_wraps,
        reason = "the signature of native functions"
    )]
    fn drain(rt: &mut Runtime, _: &swb_js::NativeCall) -> VmResult<swb_js::NativeReturn> {
        rt.run_jobs().ok();
        Ok(swb_js::NativeReturn::Value(Value::Undefined))
    }
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.define_global_function("drain", 0, drain).unwrap();
        assert_eq!(
            run_in(
                &mut rt,
                "var log = []; Promise.resolve().then(() => log.push('job')); drain(); \
                 log.push('script'); Promise.resolve().then(() => print(log.join())); undefined"
            ),
            "script,job"
        );
    });
}
