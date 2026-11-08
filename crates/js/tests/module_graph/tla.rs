//! Top-level `await` (ADR 0034 section 3, sub-feature 3a-2): the order of
//! async evaluation (Chromium 148's cases `tla-siblings` and
//! `tla-dependency` of `tools/probes/web-module-graph.json`), rejections and
//! their propagation, cycles with async modules, cached errors, answers of
//! the loader that come later, terminations at each point, and hostile
//! graphs with time and memory limits. Most tests run in four modes (with
//! and without the GC stress mode, answers now and later).

use super::*;

/// Loads and links the graph of `root`, answering the loads that wait.
fn linked(rt: &mut Runtime, root: &str) -> Module {
    let module = module_of(rt, root).unwrap();
    let promise = rt.load_requested_modules(module).unwrap();
    while let Some((token, name)) = loader(rt).waiting.pop_front() {
        let result = module_of(rt, &name);
        rt.finish_loading_imported_module(token, result).unwrap();
    }
    assert!(
        matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ),
        "the load of {root}"
    );
    rt.link(module).unwrap();
    module
}

/// Links and evaluates `root`; returns its promise.
fn start(rt: &mut Runtime, root: &str) -> Value {
    let module = linked(rt, root);
    rt.evaluate(module).unwrap()
}

#[test]
fn probe_tla_siblings() {
    // Case `tla-siblings`: `a1 b c1 a2 c2 m1`.
    assert_eq!(
        run_graph(
            &[
                (
                    "m1.js",
                    "import './a.js';import './b.js';import './c.js';print('m1')"
                ),
                ("a.js", "print('a1');await 0;print('a2')"),
                ("b.js", "print('b')"),
                ("c.js", "print('c1');await null;print('c2')"),
            ],
            "m1.js"
        ),
        "a1\nb\nc1\na2\nc2\nm1"
    );
}

#[test]
fn probe_tla_dependency() {
    // Case `tla-dependency` without the document's events: the dependency
    // waits (the test settles its promise later); a module evaluated
    // meanwhile runs first; then `a-after m1`.
    let output = in_all_modes(
        &[
            ("m1.js", "import './a.js';print('m1')"),
            (
                "a.js",
                "print('a-before');await new Promise(function(r){globalThis.release=r});print('a-after')",
            ),
            ("m2.js", "print('m2')"),
        ],
        |rt| {
            let m1 = start(rt, "m1.js");
            rt.run_jobs().unwrap();
            let before = state(rt, m1);
            let m2 = start(rt, "m2.js");
            let m2_state = state(rt, m2);
            rt.collect();
            rt.eval("release()").unwrap();
            rt.run_jobs().unwrap();
            format!("{} | {before} {m2_state} {}", printed(rt), state(rt, m1))
        },
    );
    assert_eq!(
        output,
        "a-before m2 a-after m1 | pending fulfilled fulfilled"
    );
}

#[test]
fn the_body_runs_as_an_async_function() {
    // `for await` and `await` in a computed key at the top level, an
    // exception caught across an `await`, `this` and `await` in a nested
    // arrow function, and exports that importers see after the wait.
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { v, k } from './dep.js'; print(v, k, typeof this);"
                ),
                (
                    "dep.js",
                    "for await (const x of [Promise.resolve(1), 2]) print('x', x);
                     class C { [await 'm']() {} }
                     export const k = Object.getOwnPropertyNames(C.prototype).join();
                     try { await Promise.reject(new Error('caught')); } catch (e) { print(e.message); }
                     export let v = await (async () => this)();"
                ),
            ],
            "main.js"
        ),
        "x 1\nx 2\ncaught\nundefined constructor,m undefined"
    );
}

#[test]
fn probe_tla_stacks() {
    // Cases `msg-tla-stack` and `msg-tla-dependency-stack` (Chromium 148):
    // a module that awaits shows as `at async x.js:2:5` below the async
    // function that it waits for; after its own `await` the module frame is
    // the only one.
    assert_eq!(
        run_graph(
            &[(
                "x.js",
                "async function f(){await 0;throw new Error('in f')}\n\
                     try{await f()}catch(e){print('A '+e.stack)}\n\
                     print('B '+new Error('before').stack);\n\
                     await 0;\n\
                     print('C '+new Error('after').stack);\n\
                     (function g(){print('D '+new Error('in g').stack)})();\n\
                     throw new TypeError('thrown')"
            ),],
            "x.js"
        ),
        "A Error: in f\n    at f (x.js:1:34)\n    at async x.js:2:5\n\
         B Error: before\n    at x.js:3:12\n\
         C Error: after\n    at x.js:5:12\n\
         D Error: in g\n    at g (x.js:6:26)\n    at x.js:6:52\n\
         error: TypeError: thrown @x.js:7:7"
    );
    assert_eq!(
        run_graph(
            &[
                ("x.js", "import './d.js';print('x')"),
                (
                    "d.js",
                    "await new Promise(function(r){r()});\n\
                     print('E '+new Error('in d').stack);\n\
                     throw new RangeError('from d')"
                ),
            ],
            "x.js"
        ),
        "E Error: in d\n    at d.js:2:12\nerror: RangeError: from d @d.js:3:7"
    );
}

#[test]
fn importers_wait_and_other_modules_run_meanwhile() {
    // `b` imports the async module and runs after it; `c` does not and
    // runs while `a` waits.
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { v } from './a.js'; import './b.js'; import './c.js'; print('main', v);"
                ),
                ("a.js", "await 0; export let v = 'late';"),
                (
                    "b.js",
                    "import * as ns from './a.js'; print('b', E(() => ns.v));"
                ),
                ("c.js", "print('c');"),
            ],
            "main.js"
        ),
        "c\nb late\nmain late"
    );
}

/// A reaction that prints its data and the kind of the settlement.
fn note_settlement(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    _value: Value,
) -> VmResult<Value> {
    let name = rt.to_rust_string(data)?;
    loader(rt).printed.push(format!("{name}:{kind:?}"));
    Ok(Value::Undefined)
}

#[test]
fn a_rejection_reaches_the_parents_and_a_second_root() {
    let output = in_all_modes(
        &[
            (
                "rej.js",
                "print('rej'); await 0; throw new RangeError('r2');",
            ),
            ("p.js", "import './rej.js'; print('p runs');"),
            ("x.js", "import './p.js'; print('x runs');"),
            ("y.js", "import './rej.js'; print('y runs');"),
            ("z.js", "import './rej.js'; print('z runs');"),
        ],
        |rt| {
            let x = start(rt, "x.js");
            let y = start(rt, "y.js");
            // `p` is a root too (Evaluate of a module that waits).
            let p = loader(rt).modules["p.js"];
            let p = rt.evaluate(p).unwrap();
            // The order of the rejections is the order of the reaction
            // jobs: a module's promise, then those of its parents (the
            // recursion of `AsyncModuleExecutionRejected` in the current
            // draft, test262 `rejection-order.js`).
            for (promise, name) in [(y, "'y'"), (p, "'p'"), (x, "'x'")] {
                let name = rt.eval(name).unwrap();
                rt.react_to_promise(promise, note_settlement, name).unwrap();
            }
            let pending = format!("{} {}", state(rt, x), state(rt, y));
            rt.run_jobs().unwrap();
            let after = format!("{} | {}", state(rt, x), state(rt, y));
            // A root evaluated later gets the cached error at once.
            let z = start(rt, "z.js");
            let cached = state(rt, z);
            let (_, ex) = rt.promise_settlement(x).unwrap();
            let (_, ez) = rt.promise_settlement(z).unwrap();
            assert_eq!(ex, ez, "the same error object");
            assert_eq!(rt.take_unhandled_rejections().len(), 0);
            format!("{} | {pending} | {after} | {cached}", printed(rt))
        },
    );
    assert_eq!(
        output,
        "rej p:Reject x:Reject y:Reject | pending pending | rejected: RangeError: r2 @rej.js:1:30 | rejected: RangeError: r2 @rej.js:1:30 | rejected: RangeError: r2 @rej.js:1:30"
    );
}

#[test]
fn a_synchronous_error_after_an_async_dependency() {
    // `s` runs from the job of `a`'s fulfillment and throws: its parents
    // get the error; the sibling `t` still runs.
    let output = in_all_modes(
        &[
            (
                "main.js",
                "import './s.js'; import './t.js'; print('main');",
            ),
            ("a.js", "print('a'); await 0;"),
            (
                "s.js",
                "import './a.js'; print('s'); throw new Error('in s');",
            ),
            ("t.js", "import './a.js'; print('t');"),
        ],
        |rt| {
            let main = start(rt, "main.js");
            rt.run_jobs().unwrap();
            format!("{} | {}", printed(rt), state(rt, main))
        },
    );
    assert_eq!(output, "a s t | rejected: Error: in s @s.js:1:36");
}

#[test]
fn a_cycle_with_an_async_module_settles_its_roots_promise() {
    let output = in_all_modes(
        &[
            ("a.js", "import './b.js'; print('a');"),
            (
                "b.js",
                "import './a.js'; print('b1'); await 0; print('b2');",
            ),
            ("c.js", "import './b.js'; print('c');"),
        ],
        |rt| {
            let a = start(rt, "a.js");
            // `b` is in the cycle of `a`: Evaluate of `b` gives the promise
            // of the cycle root.
            let b = loader(rt).modules["b.js"];
            let again = rt.evaluate(b).unwrap();
            assert_eq!(a, again);
            // `c` imports a member of the cycle while it waits: `c` waits
            // for the cycle root.
            let c = start(rt, "c.js");
            let pending = format!("{} {}", state(rt, a), state(rt, c));
            rt.run_jobs().unwrap();
            format!(
                "{} | {pending} | {} {}",
                printed(rt),
                state(rt, a),
                state(rt, c)
            )
        },
    );
    assert_eq!(output, "b1 b2 a c | pending pending | fulfilled fulfilled");
}

#[test]
fn a_cycle_whose_async_member_rejects() {
    let output = in_all_modes(
        &[
            ("a.js", "import './b.js'; print('a');"),
            ("b.js", "import './a.js'; await 0; throw 'cyc';"),
            ("c.js", "import './a.js'; print('c');"),
        ],
        |rt| {
            let a = start(rt, "a.js");
            rt.run_jobs().unwrap();
            let c = start(rt, "c.js");
            format!("{} | {} | {}", printed(rt), state(rt, a), state(rt, c))
        },
    );
    assert_eq!(output, " | rejected: cyc | rejected: cyc");
}

#[test]
fn a_parent_in_a_cycle_whose_root_was_rejected_does_not_run() {
    // The cycle {r, p}: `r` imports `b`, which awaits once and throws, so
    // `r` is rejected while `p` still waits for `a` (4 awaits). When `a`
    // ends, `GatherAvailableAncestors` must skip `p`: the cycle root `r`
    // has an error. Expected (Node 22): `b a`, then the rejection.
    let output = in_all_modes(
        &[
            ("main.js", "import './r.js'; print('main');"),
            ("r.js", "import './p.js'; import './b.js'; print('r');"),
            ("p.js", "import './r.js'; import './a.js'; print('p RUNS');"),
            ("a.js", "await 0; await 0; await 0; await 0; print('a');"),
            ("b.js", "await 0; print('b'); throw 'b err';"),
        ],
        |rt| {
            let main = start(rt, "main.js");
            rt.run_jobs().unwrap();
            format!("{} | {}", printed(rt), state(rt, main))
        },
    );
    assert_eq!(output, "b a | rejected: b err");
}

#[test]
fn gathered_ancestors_run_in_their_order() {
    // The order of `[[AsyncEvaluationOrder]]`: when `a` ends, `x` and `y`
    // (its parents) and `z` (the parent of `x`) can run. `z` got its
    // number before `y`, so it runs before `y`, although the walk of the
    // parents finds `y` first.
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import './z.js'; import './y.js'; print('main');"
                ),
                ("z.js", "import './x.js'; print('z');"),
                ("x.js", "import './a.js'; print('x');"),
                ("y.js", "import './a.js'; print('y');"),
                ("a.js", "await 0; print('a');"),
            ],
            "main.js"
        ),
        "a\nx\nz\ny\nmain"
    );
    // `p` and `q` both wait for `a`; `q` also for `b`, which ends later.
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import './p.js'; import './q.js'; print('main');"
                ),
                ("p.js", "import './a.js'; print('p');"),
                ("q.js", "import './b.js'; import './a.js'; print('q');"),
                ("a.js", "await 0; print('a');"),
                ("b.js", "await 0; await 0; print('b');"),
            ],
            "main.js"
        ),
        "a\np\nb\nq\nmain"
    );
}

#[test]
fn an_async_error_is_cached_for_a_later_evaluate() {
    let output = in_all_modes(
        &[
            (
                "bad.js",
                "print('bad'); await 0; throw new TypeError('boom');",
            ),
            ("r1.js", "import './bad.js'; print('r1');"),
            ("r2.js", "import './bad.js'; print('r2');"),
        ],
        |rt| {
            let r1 = start(rt, "r1.js");
            rt.run_jobs().unwrap();
            let bad = loader(rt).modules["bad.js"];
            let direct = rt.evaluate(bad).unwrap();
            let r2 = start(rt, "r2.js");
            rt.run_jobs().unwrap();
            format!(
                "{} | {} | {} | {}",
                printed(rt),
                state(rt, r1),
                state(rt, direct),
                state(rt, r2)
            )
        },
    );
    assert_eq!(
        output,
        "bad | rejected: TypeError: boom @bad.js:1:30 | rejected: TypeError: boom @bad.js:1:30 | rejected: TypeError: boom @bad.js:1:30"
    );
}

#[test]
fn a_dependency_evaluated_alone_first() {
    // Evaluate of a dependency, then of its importer while it waits: the
    // importer waits for it; both promises settle.
    let output = in_all_modes(
        &[
            ("dep.js", "print('dep1'); await 0; print('dep2');"),
            ("user.js", "import './dep.js'; print('user');"),
        ],
        |rt| {
            let dep = start(rt, "dep.js");
            let user = start(rt, "user.js");
            let pending = format!("{} {}", state(rt, dep), state(rt, user));
            rt.run_jobs().unwrap();
            format!(
                "{} | {pending} | {} {}",
                printed(rt),
                state(rt, dep),
                state(rt, user)
            )
        },
    );
    assert_eq!(
        output,
        "dep1 dep2 user | pending pending | fulfilled fulfilled"
    );
}

// --- Termination (ADR 0034 section 6) ---

/// Sets a deadline of 50 ms from now.
fn short_deadline(rt: &mut Runtime) {
    rt.set_deadline(Some(Instant::now() + Duration::from_millis(50)));
}

#[test]
fn a_termination_in_the_synchronous_part_of_evaluate() {
    // `a` waits when `loop` ends the task: `a` finishes in a later job;
    // `main` never runs and its promise stays pending.
    let output = in_all_modes(
        &[
            (
                "main.js",
                "import './a.js'; import './loop.js'; print('main');",
            ),
            ("a.js", "print('a1'); await 0; print('a2');"),
            ("loop.js", "for (;;) {}"),
            ("user.js", "import './main.js'; print('user');"),
        ],
        |rt| {
            let main = linked(rt, "main.js");
            short_deadline(rt);
            let result = rt.evaluate(main);
            assert!(
                matches!(result, Err(ScriptError::Terminated(_))),
                "{result:?}"
            );
            long_deadline(rt);
            rt.run_jobs().unwrap();
            let promise = rt.evaluate(main).unwrap();
            let user = start(rt, "user.js");
            rt.run_jobs().unwrap();
            format!(
                "{} | {} {}",
                printed(rt),
                state(rt, promise),
                state(rt, user)
            )
        },
    );
    assert_eq!(output, "a1 a2 | pending pending");
}

#[test]
fn a_termination_in_a_resumed_body() {
    let output = in_all_modes(
        &[
            ("main.js", "import './a.js'; print('main');"),
            ("a.js", "print('a1'); await 0; print('a2'); for (;;) {}"),
            ("user.js", "import './a.js'; print('user');"),
        ],
        |rt| {
            let main = start(rt, "main.js");
            short_deadline(rt);
            let result = rt.run_jobs();
            assert!(
                matches!(result, Err(ScriptError::Terminated(_))),
                "{result:?}"
            );
            long_deadline(rt);
            rt.run_jobs().unwrap();
            // The module never runs again; a new importer waits forever.
            let user = start(rt, "user.js");
            rt.run_jobs().unwrap();
            format!("{} | {} {}", printed(rt), state(rt, main), state(rt, user))
        },
    );
    assert_eq!(output, "a1 a2 | pending pending");
}

#[test]
fn a_termination_in_an_ancestor_run_from_a_job() {
    // `s` (synchronous) runs from the job of `a`'s fulfillment and loops:
    // `t`, gathered with it, never runs; nor does anything above them.
    let output = in_all_modes(
        &[
            (
                "main.js",
                "import './s.js'; import './t.js'; print('main');",
            ),
            ("a.js", "print('a'); await 0;"),
            ("s.js", "import './a.js'; print('s'); for (;;) {}"),
            ("t.js", "import './a.js'; print('t');"),
            ("user.js", "import './t.js'; print('user');"),
        ],
        |rt| {
            let main = start(rt, "main.js");
            short_deadline(rt);
            let result = rt.run_jobs();
            assert!(
                matches!(result, Err(ScriptError::Terminated(_))),
                "{result:?}"
            );
            long_deadline(rt);
            rt.run_jobs().unwrap();
            let user = start(rt, "user.js");
            rt.run_jobs().unwrap();
            let a = loader(rt).modules["a.js"];
            let a = rt.evaluate(a).unwrap();
            format!(
                "{} | {} {} {}",
                printed(rt),
                state(rt, main),
                state(rt, user),
                state(rt, a)
            )
        },
    );
    assert_eq!(output, "a s | pending pending fulfilled");
}

#[test]
fn a_termination_in_the_first_run_of_an_async_body() {
    // The body of `b` loops before its first `await`.
    let output = in_all_modes(
        &[
            ("main.js", "import './b.js'; print('main');"),
            (
                "b.js",
                "print('b'); if (globalThis.n === undefined) for (;;) {} await 0;",
            ),
        ],
        |rt| {
            let main = linked(rt, "main.js");
            short_deadline(rt);
            let result = rt.evaluate(main);
            assert!(
                matches!(result, Err(ScriptError::Terminated(_))),
                "{result:?}"
            );
            long_deadline(rt);
            rt.run_jobs().unwrap();
            let promise = rt.evaluate(main).unwrap();
            format!("{} | {}", printed(rt), state(rt, promise))
        },
    );
    assert_eq!(output, "b | pending");
}

// --- Robustness (ADR 0034 section 6) ---

/// Runs `root` in a runtime with a deadline of `seconds`: load, link,
/// evaluate and the jobs; returns the output and the time.
fn hostile_run(sources: &[(String, String)], root: &str, seconds: u64) -> (String, Duration) {
    let mut rt = hostile_runtime(sources, seconds);
    let (output, time) = timed(|| run_graph_in(&mut rt, root));
    assert_eq!(rt.stack_depths(), (0, 0));
    (output, time)
}

#[test]
fn a_chain_of_10000_modules_that_each_await() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let import = if i + 1 < count {
                format!("import './m{}.js';", i + 1)
            } else {
                String::new()
            };
            sources.push((
                format!("m{i}.js"),
                format!("{import} await 0; globalThis.n = (globalThis.n || 0) + 1;"),
            ));
        }
        sources.push((
            "main.js".to_owned(),
            "import './m0.js'; print(globalThis.n);".to_owned(),
        ));
        let (output, time) = hostile_run(&sources, "main.js", 20);
        assert_eq!(output, "10000");
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn a_chain_of_10000_synchronous_modules_above_one_async_module() {
    // `GatherAvailableAncestors` walks 10,000 levels.
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let text = if i + 1 < count {
                format!(
                    "import './m{}.js'; globalThis.n = (globalThis.n || 0) + 1;",
                    i + 1
                )
            } else {
                "await 0; globalThis.n = 1;".to_owned()
            };
            sources.push((format!("m{i}.js"), text));
        }
        sources.push((
            "main.js".to_owned(),
            "import './m0.js'; print(globalThis.n);".to_owned(),
        ));
        let (output, time) = hostile_run(&sources, "main.js", 20);
        assert_eq!(output, "10000");
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn one_module_with_10000_async_dependencies() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        let mut main = String::new();
        for i in 0..count {
            sources.push((
                format!("d{i}.js"),
                "await 0; globalThis.n = (globalThis.n || 0) + 1;".to_owned(),
            ));
            let _ = write!(main, "import './d{i}.js';");
        }
        main.push_str("print(globalThis.n);");
        sources.push(("main.js".to_owned(), main));
        let (output, time) = hostile_run(&sources, "main.js", 20);
        assert_eq!(output, "10000");
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn ten_thousand_async_parents_of_a_module_that_rejects() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        let mut main = String::new();
        for i in 0..count {
            sources.push((
                format!("p{i}.js"),
                "import './x.js'; globalThis.ran = true;".to_owned(),
            ));
            let _ = write!(main, "import './p{i}.js';");
        }
        main.push_str("print('main runs');");
        sources.push(("main.js".to_owned(), main));
        sources.push((
            "x.js".to_owned(),
            "await 0; throw new Error('x');".to_owned(),
        ));
        let mut rt = hostile_runtime(&sources, 20);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "error: Error: x @x.js:1:16");
        assert_eq!(rt.eval("globalThis.ran").unwrap(), Value::Undefined);
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn a_chain_of_10000_async_parents_of_a_module_that_rejects() {
    // `AsyncModuleExecutionRejected` walks 10,000 levels.
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let text = if i + 1 < count {
                format!("import './m{}.js'; globalThis.ran = true;", i + 1)
            } else {
                "await 0; throw 'deep';".to_owned()
            };
            sources.push((format!("m{i}.js"), text));
        }
        sources.push(("main.js".to_owned(), "import './m0.js';".to_owned()));
        let mut rt = hostile_runtime(&sources, 20);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "error: deep");
        assert_eq!(rt.eval("globalThis.ran").unwrap(), Value::Undefined);
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn a_ring_of_10000_modules_with_one_async_module() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let next = (i + 1) % count;
            let wait = if i == 5000 { "await 0;" } else { "" };
            sources.push((
                format!("m{i}.js"),
                format!("import './m{next}.js'; {wait} globalThis.n = (globalThis.n || 0) + 1;"),
            ));
        }
        sources.push((
            "main.js".to_owned(),
            "import './m0.js'; print(globalThis.n);".to_owned(),
        ));
        let (output, time) = hostile_run(&sources, "main.js", 20);
        assert_eq!(output, "10000");
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn a_body_that_never_settles() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with(
                &[
                    (
                        "never.js",
                        "print('w'); await new Promise(() => {}); print('never');",
                    ),
                    ("user.js", "import './never.js'; print('user');"),
                ],
                stress,
                false,
            );
            rt.set_deadline(Some(Instant::now() + Duration::from_secs(10)));
            let user = start(&mut rt, "user.js");
            let (result, time) = timed(|| rt.run_jobs());
            result.unwrap();
            assert!(time < Duration::from_secs(1), "{time:?}");
            rt.collect();
            assert_eq!(state(&rt, user), "pending");
            let never = loader(&mut rt).modules["never.js"];
            let promise = rt.evaluate(never).unwrap();
            assert_eq!(state(&rt, promise), "pending");
            assert_eq!(printed(&mut rt), "w");
            // Nothing can resume the body: its frame is garbage, the
            // records stay.
            let before = rt.heap().counts().objects;
            rt.collect();
            assert!(rt.heap().counts().objects <= before);
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}
