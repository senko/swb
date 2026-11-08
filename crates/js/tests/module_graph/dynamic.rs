//! `import()` and `import.meta` (ADR 0034 section 4, sub-feature 3a-3):
//! the referrer of each kind of code, the checks of the options with
//! Chromium 148's messages (probe case `msg-import-call-options`), the
//! failures of each step as rejections, namespaces, modules that wait on
//! top-level `await`, `import.meta` and its hook (case
//! `msg-import-call-meta`), terminations, and hostile counts with time
//! limits. Most tests run in the four modes (with and without the GC
//! stress mode, answers now and later).

use super::*;

/// Answers the loads that wait (in the `Later` mode) and runs the jobs
/// until nothing is left; the first error.
fn settle(rt: &mut Runtime) -> Result<(), String> {
    loop {
        let waiting = loader(rt).waiting.pop_front();
        if let Some((token, name)) = waiting {
            let result = module_of(rt, &name);
            rt.finish_loading_imported_module(token, result)
                .map_err(|error| format!("answer: {error:?}"))?;
        } else if rt.pending_jobs() > 0 {
            rt.run_jobs().map_err(|error| format!("jobs: {error}"))?;
        } else {
            return Ok(());
        }
    }
}

/// Runs a classic script with the Script Record number `number`.
fn script(rt: &mut Runtime, source: &str, number: Option<u32>) -> Value {
    rt.eval_named_at(0, source, "script.js", (0, 0), number)
        .unwrap()
}

/// The `import()` requests so far, one per line.
fn referrers(rt: &mut Runtime) -> String {
    std::mem::take(&mut loader(rt).referrers).join("\n")
}

const A: (&str, &str) = ("a.js", "export const v = 'a';");

#[test]
fn each_kind_of_code_has_its_referrer() {
    let output = in_all_modes(
        &[
            A,
            (
                "main.js",
                "import('./a.js');
                 export function f() { return import('./a.js'); }
                 export const g = new Function(\"return import('./a.js')\");
                 eval(\"import('./a.js')\");
                 globalThis.fromModule = { f, g };",
            ),
        ],
        |rt| {
            let mut out = Vec::new();
            script(rt, "import('./main.js')", Some(7));
            settle(rt).unwrap();
            out.push(referrers(rt));
            // Functions keep the module of their code; eval code and the
            // functions of `Function` get the active script.
            script(
                rt,
                "fromModule.f(); fromModule.g(); import('./a.js');
                 eval(\"import('./a.js')\"); (0, eval)(\"import('./a.js')\");
                 new Function(\"return import('./a.js')\")();
                 Promise.resolve().then(fromModule.f);
                 eval(\"globalThis.ef = () => import('./a.js')\");
                 (0, eval)(\"globalThis.ief = () => import('./a.js')\");
                 globalThis.ff = new Function(\"return import('./a.js')\");",
                Some(8),
            );
            settle(rt).unwrap();
            // Their functions keep the script of their creation.
            script(rt, "ef(); ief(); ff();", Some(10));
            settle(rt).unwrap();
            out.push(referrers(rt));
            // Code without a script: the realm, or the script below it.
            script(
                rt,
                "import('./a.js'); globalThis.h = function () { return import('./a.js'); };
                 Promise.resolve().then(h);",
                None,
            );
            settle(rt).unwrap();
            script(rt, "h()", Some(9));
            let h = script(rt, "h", None);
            rt.call_function(h, Value::Undefined, &[]).unwrap();
            settle(rt).unwrap();
            out.push(referrers(rt));
            // An event handler with and without a Script Record.
            for number in [Some(11), None] {
                let handler = rt
                    .compile_event_handler(&swb_js::EventHandler {
                        realm: 0,
                        name: "onclick",
                        params: &["event"],
                        body: "return import('./a.js')",
                        scope: &[],
                        script_name: "page.html",
                        host_defined: number,
                    })
                    .unwrap();
                rt.call_function(handler.into(), Value::Undefined, &[])
                    .unwrap();
            }
            settle(rt).unwrap();
            out.push(referrers(rt));
            out.join("\n--\n")
        },
    );
    assert_eq!(
        output,
        "main.js from script 7\na.js from module main.js\na.js from module main.js\n--\n\
         a.js from module main.js\na.js from module main.js\na.js from script 8\n\
         a.js from script 8\na.js from script 8\na.js from script 8\na.js from module main.js\n\
         a.js from script 8\na.js from script 8\na.js from script 8\n--\n\
         a.js from realm 0\na.js from realm 0\na.js from script 9\na.js from realm 0\n--\n\
         a.js from script 11\na.js from realm 0"
    );
}

/// A script that runs `import()` with each case of the probe case
/// `msg-import-call-options` (without the module types, which the host
/// checks) and prints the results in order.
const OPTION_CASES: &str = "
    function R(p){return p.then(function(v){return 'ok'},function(e){return e.constructor.name+': '+e.message})}
    var w1={};w1[Symbol('k')]='v';var w2={};Object.defineProperty(w2,'k',{value:'v',enumerable:false});
    var px=new Proxy({},{ownKeys:function(){throw new RangeError('keys')}});
    var log=[];
    var C=[
    ['opt-num',function(){return import('./a.js',1)}],
    ['opt-null',function(){return import('./a.js',null)}],
    ['opt-str',function(){return import('./a.js','x')}],
    ['opt-undef',function(){return import('./a.js',undefined)}],
    ['with-num',function(){return import('./a.js',{with:1})}],
    ['with-null',function(){return import('./a.js',{with:null})}],
    ['with-str',function(){return import('./a.js',{with:'x'})}],
    ['with-undef',function(){return import('./a.js',{with:undefined})}],
    ['with-getter',function(){return import('./a.js',{get with(){log.push('get');throw new RangeError('g')}})}],
    ['with-proxy',function(){return import('./a.js',{with:px})}],
    ['val-num',function(){return import('./a.js',{with:{type:1}})}],
    ['val-obj',function(){return import('./a.js',{with:{type:{}}})}],
    ['key-bad',function(){return import('./a.js',{with:{k:'v'}})}],
    ['key-bad-val-num',function(){return import('./a.js',{with:{k:'v',type:1}})}],
    ['val-num-key-bad',function(){return import('./a.js',{with:{type:1,k:'v'}})}],
    ['key-two-bad',function(){return import('./a.js',{with:{k1:'v',k2:'v'}})}],
    ['key-sym',function(){return import('./a.js',{with:w1})}],
    ['key-nonenum',function(){return import('./a.js',{with:w2})}],
    ['spec-throw',function(){return import({toString:function(){throw new RangeError('ts')}})}],
    ['spec-sym',function(){return import(Symbol('s'))}],
    ['spec-throw-opt-num',function(){return import({toString:function(){throw new RangeError('ts')}},1)}],
    ['spec-obj',function(){return import({toString:function(){return './a.js'}})}],
    ['getters-first',function(){return import('./a.js',{with:{get type(){log.push('type');return 1},get k(){log.push('k');return 'v'}}})}]
    ];
    Promise.all(C.map(function(c){return R(c[1]()).then(function(v){return c[0]+'='+v})}))
      .then(function(a){print(a.join('\\n')); print(log.join())});";

#[test]
fn the_options_are_checked_as_in_chromium() {
    let output = in_all_modes(&[A], |rt| {
        script(rt, OPTION_CASES, Some(1));
        settle(rt).unwrap();
        let classic = printed(rt);
        // The same in module code.
        loader(rt)
            .sources
            .insert("cases.js".to_owned(), OPTION_CASES.to_owned());
        script(rt, "import('./cases.js')", Some(1));
        settle(rt).unwrap();
        assert_eq!(printed(rt), classic);
        // The requests that reached the host: only the valid ones.
        let count = loader(rt).referrers.len();
        format!("{classic}\n{count}")
    });
    // Chromium 148, `msg-import-call-options` (classic and module scripts).
    let chromium = "opt-num=TypeError: The second argument to import() must be an object
opt-null=TypeError: The second argument to import() must be an object
opt-str=TypeError: The second argument to import() must be an object
opt-undef=ok
with-num=TypeError: The 'with' option must be an object
with-null=TypeError: The 'with' option must be an object
with-str=TypeError: The 'with' option must be an object
with-undef=ok
with-getter=RangeError: g
with-proxy=RangeError: keys
val-num=TypeError: Import attribute value must be a string
val-obj=TypeError: Import attribute value must be a string
key-bad=TypeError: Invalid attribute key \"k\".
key-bad-val-num=TypeError: Import attribute value must be a string
val-num-key-bad=TypeError: Import attribute value must be a string
key-two-bad=TypeError: Invalid attribute key \"k1\".
key-sym=ok
key-nonenum=ok
spec-throw=RangeError: ts
spec-sym=TypeError: Cannot convert a Symbol value to a string
spec-throw-opt-num=RangeError: ts
spec-obj=ok
getters-first=TypeError: Import attribute value must be a string";
    // The getters of all entries run before the check of the values.
    assert_eq!(output, format!("{chromium} get,type,k\n11"));
}

#[test]
fn each_failure_rejects_the_promise() {
    let output = in_all_modes(
        &[
            A,
            ("bad.js", "var x = ;"),
            ("deep.js", "import './bad.js';"),
            ("link.js", "import { zz } from './a.js';"),
            (
                "throw.js",
                "print('throw runs'); throw new RangeError('t');",
            ),
            ("tla.js", "await 0; throw new EvalError('late');"),
            ("ok.js", "export default 'fine';"),
        ],
        |rt| {
            script(
                rt,
                "var names = ['nope.js', 'bad.js', 'deep.js', 'link.js', 'throw.js', 'tla.js', 'ok.js', 'throw.js'];
                 Promise.allSettled(names.map(n => import('./' + n))).then(results => {
                   results.slice(0, 7).forEach((r, i) => print(names[i], r.status === 'fulfilled'
                     ? r.value.default : r.reason.constructor.name + ': ' + r.reason.message));
                   print('same error', results[4].reason === results[7].reason);
                 });",
                Some(1),
            );
            settle(rt).unwrap();
            printed(rt)
        },
    );
    assert_eq!(
        output,
        "throw runs \
         nope.js TypeError: Cannot find module 'nope.js' \
         bad.js SyntaxError: Unexpected token ';' \
         deep.js SyntaxError: Unexpected token ';' \
         link.js SyntaxError: The requested module './a.js' does not provide an export named 'zz' \
         throw.js RangeError: t \
         tla.js EvalError: late \
         ok.js fine \
         same error true"
    );
}

#[test]
fn the_reaction_alone_keeps_the_module() {
    // The host forgets each module after its answer, so the reaction data
    // of `ContinueDynamicImport` is the only reference between the answer
    // and the evaluation (the collector runs at every allocation in the
    // stress mode).
    let output = in_all_modes(&[A], |rt| {
        loader(rt).forget = true;
        script(
            rt,
            "import('./a.js').then(ns => print(ns.v)); \
             for (var i = 0; i < 200; i++) ({ i });",
            Some(1),
        );
        settle(rt).unwrap();
        printed(rt)
    });
    assert_eq!(output, "a");
}

#[test]
fn a_module_gives_one_namespace() {
    let output = in_all_modes(
        &[
            A,
            ("main.js", "import * as ns from './a.js'; export { ns };"),
        ],
        |rt| {
            script(
                rt,
                "Promise.all([import('./a.js'), import('./a.js'), import('./main.js')])
                   .then(([x, y, m]) => print(x === y, m.ns === x,
                     Object.prototype.toString.call(x), Object.keys(x)));",
                Some(1),
            );
            settle(rt).unwrap();
            printed(rt)
        },
    );
    assert_eq!(output, "true true [object Module] v");
}

#[test]
fn an_import_waits_for_top_level_await() {
    let output = in_all_modes(
        &[
            (
                "slow.js",
                "print('slow start'); await new Promise(r => globalThis.release = r);
                 print('slow end'); export const v = 1;",
            ),
            (
                "user.js",
                "import { v } from './slow.js'; print('user', v);",
            ),
        ],
        |rt| {
            let promise = script(
                rt,
                "globalThis.p = import('./user.js'); import('./slow.js').then(ns => print('got', ns.v)); p",
                Some(1),
            );
            settle(rt).unwrap();
            let before = format!("{} | {}", printed(rt), state(rt, promise));
            rt.collect();
            script(rt, "release()", None);
            settle(rt).unwrap();
            let promise = script(rt, "p", None);
            format!("{before} | {} | {}", printed(rt), state(rt, promise))
        },
    );
    assert_eq!(
        output,
        "slow start | pending | slow end user 1 got 1 | fulfilled"
    );
}

#[test]
fn a_module_can_import_itself() {
    // In its body (the import settles after the body) and after a
    // top-level `await` that waits for its own evaluation, which never
    // ends (the specification's deadlock).
    let output = in_all_modes(
        &[
            (
                "self.js",
                "export const v = 1; import('./self.js').then(ns => print('self', ns.v));",
            ),
            (
                "wait.js",
                "print('wait'); await import('./wait.js'); print('never');",
            ),
        ],
        |rt| {
            let promise = script(
                rt,
                "import('./self.js'); globalThis.w = import('./wait.js'); w",
                Some(1),
            );
            settle(rt).unwrap();
            format!("{} | {}", printed(rt), state(rt, promise))
        },
    );
    assert_eq!(output, "wait self 1 | pending");
}

#[test]
fn import_meta_is_one_object_per_module() {
    let output = in_all_modes(
        &[
            (
                "m.js",
                "export const a = import.meta, b = (() => import.meta)();
                 export function c() { return import.meta; }",
            ),
            ("n.js", "export const a = import.meta;"),
        ],
        |rt| {
            script(
                rt,
                "Promise.all([import('./m.js'), import('./n.js')]).then(([m, n]) => print(
                   m.a === m.b, m.a === m.c(), m.a === n.a, Object.getPrototypeOf(m.a),
                   Object.keys(m.a), m.a.url, n.a.url, Object.isExtensible(m.a)));",
                Some(1),
            );
            settle(rt).unwrap();
            printed(rt)
        },
    );
    // Chromium 148 (`msg-import-call-meta`): the same object each time, a
    // null prototype, extensible; one object per module.
    assert_eq!(output, "true true false null url mem:m.js mem:n.js true");
}

#[test]
fn import_meta_without_the_hook_is_empty() {
    on_big_stack(|| {
        let mut rt = runtime_with(
            &[(
                "m.js",
                "export const k = Object.keys(import.meta).length + ' ' + Object.getPrototypeOf(import.meta);",
            )],
            true,
            false,
        );
        rt.set_module_hooks(ModuleHooks {
            load_imported_module: load,
            import_meta_properties: None,
        });
        script(&mut rt, "import('./m.js').then(ns => print(ns.k))", Some(1));
        settle(&mut rt).unwrap();
        assert_eq!(printed(&mut rt), "0 null");
    });
}

#[test]
fn without_hooks_both_are_refused_as_before() {
    // The page installs no module hooks before 3b-1: the refusal of the
    // compiler stays, at the same offset (`MODULE_DEPARTURES` of
    // `crates/engine/tests/probe_net.rs` is unchanged).
    on_big_stack(|| {
        let mut rt = Runtime::new(RuntimeConfig::default()).unwrap();
        let error = rt.eval("var x;\nimport('./a.js');").unwrap_err();
        assert!(
            matches!(error, ScriptError::Compile { offset: 7, ref message, .. }
            if message == "not supported yet (import())")
        );
        // Eval code and the `Function` constructor throw the refusal.
        for source in ["eval('1; import(1)')", "new Function('return import(1)')"] {
            let error = rt.eval(source).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Uncaught SyntaxError: not supported yet (import())",
                "{source}"
            );
        }
        let error = rt
            .eval_module_at(0, "let a;\nimport.meta.url;", "m.js", (0, 0))
            .unwrap_err();
        assert!(error.is_unsupported());
        assert!(
            matches!(error, ScriptError::Compile { offset: 7, ref message, .. }
            if message == "not supported yet (import.meta)")
        );
    });
}

// --- Termination ---

#[test]
fn a_termination_in_the_evaluation_leaves_the_promise_pending() {
    on_big_stack(|| {
        for (stress, later) in [(false, false), (true, true)] {
            let mut rt = runtime_with(
                &[("loop.js", "print('loop'); for (;;) {}"), A],
                stress,
                later,
            );
            long_deadline(&mut rt);
            script(&mut rt, "globalThis.p = import('./loop.js')", Some(1));
            rt.set_deadline(Some(Instant::now() + Duration::from_millis(50)));
            let error = settle(&mut rt).unwrap_err();
            assert!(error.contains("terminated"), "{error}");
            long_deadline(&mut rt);
            assert_eq!(rt.stack_depths(), (0, 0));
            // The promise stays pending; another import of the module waits
            // for ever; other imports still work.
            script(&mut rt, "globalThis.q = import('./loop.js')", Some(1));
            script(&mut rt, "import('./a.js').then(ns => print(ns.v))", Some(1));
            settle(&mut rt).unwrap();
            rt.collect();
            let states = script(
                &mut rt,
                "var s = []; p.then(() => s.push('p'), () => s.push('p')); \
                 q.then(() => s.push('q'), () => s.push('q')); s",
                None,
            );
            let _ = states;
            settle(&mut rt).unwrap();
            let count = script(&mut rt, "s.length", None);
            assert_eq!(count.as_number(), Some(0.0));
            assert_eq!(printed(&mut rt), "loop a");
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

/// A native that asks for a termination and lets the script go on (the
/// termination takes effect at the next check of the countdown).
#[allow(clippy::unnecessary_wraps, reason = "the signature of natives")]
fn terminate_soon(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    drop(rt.terminate("test"));
    Ok(NativeReturn::Value(Value::Undefined))
}

#[test]
fn a_termination_after_the_answer_leaves_no_state_behind() {
    // The termination comes in the graph load of the answer (now: in the
    // `import()`; later: in `finish_loading_imported_module`), in the link
    // (in the job), or in a body. Then the module imports again.
    on_big_stack(|| {
        let sources = [
            ("a.js", "import './b.js'; export const v = 'a';"),
            ("b.js", "export function f() {}"),
        ];
        for (stress, later) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut rt = runtime_with(&sources, stress, later);
            rt.define_global_function("terminateSoon", 0, terminate_soon)
                .unwrap();
            long_deadline(&mut rt);
            let result = rt.eval_named_at(
                0,
                "terminateSoon(); globalThis.p = import('./a.js');",
                "s.js",
                (0, 0),
                Some(1),
            );
            // Now: the graph walk of the import ends the script. Later: the
            // script ends; the answer is not affected (a new task).
            let settled = settle(&mut rt);
            assert!(
                matches!(result, Err(ScriptError::Terminated(_))) || settled.is_err() || later,
                "{result:?} {settled:?}"
            );
            assert_eq!(rt.stack_depths(), (0, 0));
            script(&mut rt, "import('./a.js').then(ns => print(ns.v))", Some(1));
            settle(&mut rt).unwrap();
            assert_eq!(printed(&mut rt), "a");
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

// --- Robustness (ADR 0034 section 6) ---

#[test]
fn import_100000_times_of_one_specifier() {
    on_big_stack(|| {
        let mut rt = runtime_with(&[A], false, false);
        rt.set_deadline(Some(Instant::now() + Duration::from_secs(60)));
        script(&mut rt, "import('./a.js')", Some(1));
        settle(&mut rt).unwrap();
        rt.collect();
        let baseline = rt.heap().counts().objects;
        let (result, time) = timed(|| {
            script(
                &mut rt,
                "globalThis.n = 0; for (let i = 0; i < 100000; i++) import('./a.js').then(() => n++);",
                Some(1),
            );
            settle(&mut rt)
        });
        result.unwrap();
        assert_eq!(script(&mut rt, "n", None).as_number(), Some(100_000.0));
        assert!(time < Duration::from_secs(30), "{time:?}");
        rt.collect();
        // Nothing of the imports stays.
        let after = rt.heap().counts().objects;
        assert!(after < baseline + 100, "{baseline} {after}");
    });
}

#[test]
fn import_10000_distinct_specifiers() {
    on_big_stack(|| {
        let sources: Vec<(String, String)> = (0..10_000)
            .map(|i| (format!("m{i}.js"), format!("export default {i};")))
            .collect();
        let mut rt = hostile_runtime(&sources, 60);
        let (result, time) = timed(|| {
            script(
                &mut rt,
                "globalThis.s = 0; for (let i = 0; i < 10000; i++) import('./m' + i + '.js').then(ns => s += ns.default);",
                Some(1),
            );
            settle(&mut rt)
        });
        result.unwrap();
        assert_eq!(script(&mut rt, "s", None).as_number(), Some(49_995_000.0));
        assert!(time < Duration::from_secs(30), "{time:?}");
    });
}

#[test]
fn import_inside_a_chain_of_10000_modules() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let text = if i + 1 < count {
                format!(
                    "import {{ v as w }} from './m{}.js'; export const v = w + 1;",
                    i + 1
                )
            } else {
                // The leaf imports the root of the chain, which is still
                // evaluating.
                "export const v = 0; globalThis.inner = import('./m0.js');".to_owned()
            };
            sources.push((format!("m{i}.js"), text));
        }
        let mut rt = hostile_runtime(&sources, 60);
        let (result, time) = timed(|| {
            script(
                &mut rt,
                "import('./m0.js').then(ns => { print('outer', ns.v); return inner; })
                   .then(ns => print('inner', ns.v));",
                Some(1),
            );
            settle(&mut rt)
        });
        result.unwrap();
        assert_eq!(printed(&mut rt), "outer 9999 inner 9999");
        assert!(time < Duration::from_secs(30), "{time:?}");
        assert_eq!(rt.stack_depths(), (0, 0));
    });
}
