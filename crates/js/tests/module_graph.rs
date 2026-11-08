//! Module graphs (ADR 0034 sub-features 3a-1 to 3a-3; top-level `await`
//! in `module_graph/tla.rs`, `import()` and `import.meta` in
//! `module_graph/dynamic.rs`): `Runtime::parse_module`,
//! `load_requested_modules` with an in-memory loader (answers now or
//! later), `link` and `evaluate`. Cycles, live bindings, the dead zone,
//! star exports and their ambiguity, namespace objects, default exports,
//! errors with their messages and locations, cached errors, and hostile
//! graphs with time and memory limits. The probe values are Chromium 148's
//! (`tools/probes/web-module-graph.json`, cases `cycle`, `live-binding`,
//! `import-call-namespace` and `link-error-external`). Most tests run with
//! and without the GC stress mode.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{HashMap, VecDeque};
use std::fmt::Write;
use std::time::{Duration, Instant};

use common::on_big_stack;
use swb_js::{
    Gc, HeapConfig, LoadAnswer, LoadRequest, Module, ModuleHooks, NativeCall, NativeReturn, Object,
    PromiseStatus, ReactionKind, Referrer, Runtime, RuntimeConfig, ScriptError, ThrowKind, Tracer,
    Value, VmResult,
};

/// The host of the tests: the sources by name, the modules made so far,
/// the printed lines, and the requests that wait (in the `Later` mode).
#[derive(Default)]
struct Loader {
    sources: HashMap<String, String>,
    modules: HashMap<String, Module>,
    names: Vec<String>,
    printed: Vec<String>,
    later: bool,
    waiting: VecDeque<(u32, String)>,
    requests: Vec<String>,
    /// The `import()` requests: `name from referrer`.
    referrers: Vec<String>,
    /// The host keeps no module after its answer (the engine's own
    /// references are then the only ones).
    forget: bool,
}

impl swb_js::HostState for Loader {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for module in self.modules.values() {
            module.trace(tracer);
        }
    }
}

fn loader(rt: &mut Runtime) -> &mut Loader {
    rt.host_data_mut::<Loader>().unwrap()
}

/// `print(...)`.
fn print(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut parts = Vec::new();
    for i in 0..call.argc() {
        let value = rt.arg(call, i);
        parts.push(rt.to_rust_string(value)?);
    }
    loader(rt).printed.push(parts.join(" "));
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The module of a name: made on first use (a syntax error is the
/// `SyntaxError`), a missing source a `TypeError`.
fn module_of(rt: &mut Runtime, name: &str) -> Result<Module, Value> {
    if let Some(&module) = loader(rt).modules.get(name) {
        return Ok(module);
    }
    let Some(source) = loader(rt).sources.get(name).cloned() else {
        let error = rt
            .new_error(
                ThrowKind::TypeError,
                &format!("Cannot find module '{name}'"),
            )
            .unwrap();
        return Err(error.into());
    };
    let number = loader(rt).names.len() as u32;
    match rt.parse_module(0, &source, name, (0, 0), number) {
        Ok(module) => {
            let state = loader(rt);
            state.names.push(name.to_owned());
            if !state.forget {
                state.modules.insert(name.to_owned(), module);
            }
            Ok(module)
        }
        Err(ScriptError::Compile { message, .. }) => Err(rt
            .new_error(ThrowKind::SyntaxError, &message)
            .unwrap()
            .into()),
        Err(other) => panic!("{name}: {other}"),
    }
}

/// The specifier without `./`.
fn key_of(specifier: &str) -> String {
    specifier.trim_start_matches("./").to_owned()
}

/// The text of a referrer: `module NAME`, `script N` or `realm N`.
fn referrer_text(rt: &mut Runtime, referrer: Referrer) -> String {
    match referrer {
        Referrer::Module(module) => {
            let number = rt.module_host_defined(module).unwrap() as usize;
            format!("module {}", loader(rt).names[number])
        }
        Referrer::Script(number) => format!("script {number}"),
        Referrer::Realm(realm) => format!("realm {realm}"),
    }
}

/// `HostLoadImportedModule`.
#[allow(clippy::needless_pass_by_value, reason = "the signature of the hook")]
fn load(rt: &mut Runtime, request: LoadRequest) -> LoadAnswer {
    let name = key_of(&request.specifier);
    if request.dynamic {
        let referrer = referrer_text(rt, request.referrer);
        loader(rt).referrers.push(format!("{name} from {referrer}"));
    } else {
        assert!(matches!(request.referrer, Referrer::Module(_)));
    }
    loader(rt).requests.push(name.clone());
    if loader(rt).later {
        loader(rt).waiting.push_back((request.token, name));
        return LoadAnswer::Later;
    }
    LoadAnswer::Now(module_of(rt, &name))
}

/// A runtime with the loader and the sources.
fn runtime_with(sources: &[(&str, &str)], stress: bool, later: bool) -> Runtime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(config).unwrap();
    rt.set_host_data(Loader {
        sources: sources
            .iter()
            .map(|&(name, text)| (name.to_owned(), text.to_owned()))
            .collect(),
        later,
        ..Loader::default()
    });
    rt.define_global_function("print", 1, print).unwrap();
    rt.eval(
        "globalThis.E = function (f) { try { var v = f(); return v === undefined ? 'undefined' : v } \
         catch (e) { return e.constructor.name + ': ' + e.message } }",
    )
    .unwrap();
    rt.set_module_hooks(ModuleHooks {
        load_imported_module: load,
        import_meta_properties: Some(import_meta),
    });
    rt
}

/// `HostGetImportMetaProperties`: `url` is `mem:` and the module's name.
fn import_meta(rt: &mut Runtime, module: Module, meta: Gc<Object>) -> VmResult<()> {
    let number = rt.module_host_defined(module).unwrap() as usize;
    let url = format!("mem:{}", loader(rt).names[number]);
    let url = rt.string_value(&url)?;
    let key = rt.key_from_str("url")?;
    rt.create_data_property(meta, key, url)
}

/// The text of an error value with its location: `Name: message @name:line:col`.
fn error_text(rt: &Runtime, value: Value) -> String {
    let report = rt.error_report(value);
    let first = report.lines().next().unwrap_or("").to_owned();
    match rt.error_location(value) {
        Some(location) => format!(
            "{first} @{}:{}:{}",
            location.name, location.line, location.column
        ),
        None => first,
    }
}

/// Loads, links and evaluates the module `root`; returns the printed
/// lines, then the error of the step that failed, if any.
fn run_graph_in(rt: &mut Runtime, root: &str) -> String {
    let outcome = run_steps(rt, root);
    let mut lines = std::mem::take(&mut loader(rt).printed);
    if let Some(error) = outcome {
        lines.push(error);
    }
    lines.join("\n")
}

fn run_steps(rt: &mut Runtime, root: &str) -> Option<String> {
    let module = match module_of(rt, root) {
        Ok(module) => module,
        Err(error) => return Some(format!("parse: {}", error_text(rt, error))),
    };
    let promise = rt.load_requested_modules(module).unwrap();
    while let Some((token, name)) = loader(rt).waiting.pop_front() {
        let result = module_of(rt, &name);
        rt.finish_loading_imported_module(token, result).unwrap();
    }
    match rt.promise_settlement(promise) {
        Some((PromiseStatus::Fulfilled, _)) => {}
        Some((PromiseStatus::Rejected, error)) => {
            return Some(format!("load: {}", error_text(rt, error)));
        }
        other => return Some(format!("load: not settled {other:?}")),
    }
    if let Err(error) = rt.link(module) {
        return Some(match error {
            ScriptError::Uncaught { .. } => format!("link: {}", error_text(rt, rt.last_value())),
            other => format!("link: {other}"),
        });
    }
    let promise = match rt.evaluate(module) {
        Ok(promise) => promise,
        Err(error) => return Some(format!("evaluate: {error}")),
    };
    if let Err(error) = rt.run_jobs() {
        return Some(format!("jobs: {error}"));
    }
    match rt.promise_settlement(promise) {
        Some((PromiseStatus::Fulfilled, _)) => None,
        Some((PromiseStatus::Rejected, error)) => Some(format!("error: {}", error_text(rt, error))),
        other => Some(format!("evaluate: not settled {other:?}")),
    }
}

/// The state of a promise: `pending`, `fulfilled` or `rejected: error`.
fn state(rt: &Runtime, promise: Value) -> String {
    match rt.promise_settlement(promise) {
        Some((PromiseStatus::Pending, _)) => "pending".to_owned(),
        Some((PromiseStatus::Fulfilled, _)) => "fulfilled".to_owned(),
        Some((PromiseStatus::Rejected, error)) => format!("rejected: {}", error_text(rt, error)),
        None => "not a promise".to_owned(),
    }
}

/// The printed lines so far, joined by spaces.
fn printed(rt: &mut Runtime) -> String {
    std::mem::take(&mut loader(rt).printed).join(" ")
}

/// Sets a deadline of 30 s from now (after a short one; a mistake ends as
/// a termination, not a hang).
fn long_deadline(rt: &mut Runtime) {
    rt.set_deadline(Some(Instant::now() + Duration::from_secs(30)));
}

/// Runs `steps` with the sources in the four modes; all must agree and no
/// collection may find a stale root. Returns the output.
fn in_all_modes(sources: &[(&str, &str)], steps: fn(&mut Runtime) -> String) -> String {
    let sources: Vec<(String, String)> = sources
        .iter()
        .map(|&(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    on_big_stack(move || {
        let refs: Vec<(&str, &str)> = sources
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let mut outputs = Vec::new();
        for (stress, later) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut rt = runtime_with(&refs, stress, later);
            long_deadline(&mut rt);
            outputs.push(steps(&mut rt));
            assert_eq!(rt.heap().stats().total_stale_roots, 0, "stale roots");
            assert_eq!(rt.stack_depths(), (0, 0));
        }
        for output in &outputs[1..] {
            assert_eq!(&outputs[0], output, "the modes disagree");
        }
        outputs.remove(0)
    })
}

/// Runs a graph with and without the stress mode and with answers now and
/// later; all four must agree. Checks that no collection found a stale
/// root.
fn run_graph(sources: &[(&str, &str)], root: &str) -> String {
    let sources: Vec<(String, String)> = sources
        .iter()
        .map(|&(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    let root = root.to_owned();
    on_big_stack(move || {
        let refs: Vec<(&str, &str)> = sources
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let mut outputs = Vec::new();
        for (stress, later) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut rt = runtime_with(&refs, stress, later);
            outputs.push(run_graph_in(&mut rt, &root));
            assert_eq!(rt.heap().stats().total_stale_roots, 0, "stale roots");
        }
        for output in &outputs[1..] {
            assert_eq!(&outputs[0], output, "the modes disagree");
        }
        outputs.remove(0)
    })
}

#[test]
fn imports_see_the_exports_of_their_modules() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { a, b as c } from './dep.js'; import d from './dep.js'; print(a, c, d);"
                ),
                (
                    "dep.js",
                    "export const a = 1; export let b = 'two'; export default 3;"
                ),
            ],
            "main.js"
        ),
        "1 two 3"
    );
}

#[test]
fn probe_cycle() {
    // `tools/probes/web-module-graph.json`, case `cycle`: the hoisted
    // function of `a` exists when `b` runs; `a`'s `let` is in its dead
    // zone.
    assert_eq!(
        run_graph(
            &[
                // The probe's log is a classic script before the modules.
                (
                    "log.js",
                    "globalThis.L = []; globalThis.log = (x) => L.push(x);"
                ),
                (
                    "m1.js",
                    "import './log.js'; import {a} from './a.js'; log('m1:' + a); print(L.join(' '))"
                ),
                (
                    "a.js",
                    "import {b,fb} from './b.js';export let a='A';export function fa(){return 'fa'}log('a:'+b+':'+fb())"
                ),
                (
                    "b.js",
                    "import {a,fa} from './a.js';export let b='B';export function fb(){return 'fb'}log('b:'+fa()+':'+E(function(){return a}))"
                ),
            ],
            "m1.js"
        ),
        "b:fa:ReferenceError: Cannot access 'a' before initialization a:B:fb m1:A"
    );
}

#[test]
fn probe_live_binding() {
    // Case `live-binding`.
    assert_eq!(
        run_graph(
            &[
                (
                    "m1.js",
                    "import {c,inc} from './c.js';import * as ns from './c.js';print('c'+c);inc();print('c'+c+':'+ns.c+':'+E(function(){c=5})+':'+Object.keys(ns)+':'+ns[Symbol.toStringTag])"
                ),
                ("c.js", "export let c=1;export function inc(){c++}"),
            ],
            "m1.js"
        ),
        "c1\nc2:2:TypeError: Assignment to constant variable.:c,inc:Module"
    );
}

#[test]
fn probe_namespace_object() {
    // Case `import-call-namespace`, with a namespace import instead of
    // `import()` (sub-feature 3a-3).
    assert_eq!(
        run_graph(
            &[
                (
                    "m1.js",
                    "import * as ns from './ns.js';var d=Object.getOwnPropertyDescriptor(ns,'b');print([Object.prototype.toString.call(ns),Object.keys(ns).join(),Object.isExtensible(ns),Object.getPrototypeOf(ns),d.writable,d.enumerable,d.configurable,E(function(){ns.b=2}),E(function(){delete ns.b}),'default' in ns,ns.default].join(';'))"
                ),
                ("ns.js", "export let b=1,a=0;export default 'D'"),
            ],
            "m1.js"
        ),
        "[object Module];a,b,default;false;;true;true;false;TypeError: Cannot assign to property 'b' of [object Module];TypeError: Cannot delete property 'b' of [object Module];true;D"
    );
}

#[test]
fn namespace_internal_methods() {
    assert_eq!(
        run_graph(
            &[
                ("main.js", "import * as ns from './m.js';
                    print(Reflect.ownKeys(ns).map(String).join());
                    print(Object.setPrototypeOf(ns, null) === ns, Reflect.setPrototypeOf(ns, {}), Reflect.preventExtensions(ns));
                    print(E(() => Object.setPrototypeOf(ns, {})), E(() => String(ns)));
                    print(Reflect.set(ns, 'x', 1), Reflect.deleteProperty(ns, 'x'), Reflect.deleteProperty(ns, 'nope'));
                    print(Reflect.defineProperty(ns, 'x', {value: 1}), Reflect.defineProperty(ns, 'x', {value: 2}), Reflect.defineProperty(ns, 'x', {}));
                    print(Reflect.defineProperty(ns, 'x', {enumerable: false}), Reflect.defineProperty(ns, 'nope', {value: 1}));
                    print(Reflect.defineProperty(ns, 'x', {writable: false}), Reflect.defineProperty(ns, 'x', {writable: true}));
                    print(ns.nope, 'nope' in ns, Reflect.has(ns, Symbol.toStringTag), ns['0'], ns[0]);
                    print(E(() => Object.defineProperty(ns, Symbol.toStringTag, {value: 'x'})));
                    print(JSON.stringify(Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag)));
                    for (const k in ns) print('key', k);
                    print(Object.isFrozen(ns), Object.isSealed(ns));"),
                ("m.js", "export var x = 1; export const y = 2; const z = 3; export { z as '0', z as 'a b' };"),
            ],
            "main.js"
        ),
        "0,a b,x,y,Symbol(Symbol.toStringTag)\n\
         true false true\n\
         TypeError: [object Module] is not extensible TypeError: Cannot convert object to primitive value\n\
         false false true\n\
         true false true\n\
         false false\n\
         false true\n\
         undefined false true 3 3\n\
         TypeError: Cannot redefine property: Symbol(Symbol.toStringTag)\n\
         {\"value\":\"Module\",\"writable\":false,\"enumerable\":false,\"configurable\":false}\n\
         key 0\nkey a b\nkey x\nkey y\n\
         false true"
    );
}

#[test]
fn namespaces_in_the_dead_zone() {
    // §10.4.6: `[[HasProperty]]` reads no binding; `[[Get]]`,
    // `[[GetOwnProperty]]` and the keys' enumerability do.
    assert_eq!(
        run_graph(
            &[
                ("a.js", "import * as nb from './b.js'; export let la = 1; print('a', E(() => nb.lb));"),
                ("b.js", "import * as na from './a.js';
                    print('in', 'la' in na, Reflect.has(na, 'la'), Object.prototype.hasOwnProperty.call(na, 'nope'));
                    print(E(() => na.la), E(() => Object.getOwnPropertyDescriptor(na, 'la')), E(() => Object.keys(na)), E(() => JSON.stringify(na)));
                    print(Reflect.ownKeys(na).length);
                    export let lb = 2;"),
            ],
            "a.js"
        ),
        "in true true false\n\
         ReferenceError: Failed to read the 'la' property from 'Module': Cannot access 'la' before initialization ReferenceError: la is not defined ReferenceError: Cannot access 'la' before initialization ReferenceError: Cannot access 'la' before initialization\n\
         2\n\
         a 2"
    );
}

#[test]
fn star_exports_and_ambiguity() {
    let sources = [
        (
            "s1.js",
            "export const s = 1; export const only1 = 'o1'; export default 'd1';",
        ),
        ("s2.js", "export const s = 2; export const only2 = 'o2';"),
        ("same.js", "export * from './s1.js';"),
        (
            "star.js",
            "export * from './s1.js'; export * from './s2.js'; export * from './same.js';",
        ),
        (
            "ok.js",
            "import * as ns from './star.js'; import { only1, only2 } from './star.js';
            print(Object.keys(ns).join(), only1, only2, 'default' in ns);",
        ),
        ("bad.js", "\nimport {s} from './star.js';print('w')"),
        (
            "own.js",
            "export * from './s1.js'; export const s = 'own'; export * from './s2.js';",
        ),
        ("own-user.js", "import { s } from './own.js'; print(s);"),
    ];
    assert_eq!(run_graph(&sources, "ok.js"), "only1,only2 o1 o2 false");
    // Chromium: `x.js:1:9` style, the imported name in the importer.
    assert_eq!(
        run_graph(&sources, "bad.js"),
        "link: SyntaxError: The requested module './star.js' contains conflicting star exports for name 's' @bad.js:2:9"
    );
    assert_eq!(run_graph(&sources, "own-user.js"), "own");
}

#[test]
fn missing_exports_are_link_errors() {
    // `tools/probes/web-module-graph.json`, case `link-error-external`.
    let sources = [
        ("x.js", "import {zz} from './dep.js';print('x')"),
        ("dep.js", "export const a=1;print('dep')"),
        ("re.js", "export { nope as y } from './dep.js';"),
        ("re-user.js", "  import { y } from './re.js';"),
        ("default.js", "import d from './dep.js';"),
    ];
    assert_eq!(
        run_graph(&sources, "x.js"),
        "link: SyntaxError: The requested module './dep.js' does not provide an export named 'zz' @x.js:1:9"
    );
    // The indirect export is checked in its own module.
    assert_eq!(
        run_graph(&sources, "re-user.js"),
        "link: SyntaxError: The requested module './dep.js' does not provide an export named 'nope' @re.js:1:10"
    );
    assert_eq!(
        run_graph(&sources, "default.js"),
        "link: SyntaxError: The requested module './dep.js' does not provide an export named 'default' @default.js:1:8"
    );
}

#[test]
fn a_failed_link_can_be_retried_after_nothing_changed() {
    on_big_stack(|| {
        let mut rt = runtime_with(
            &[
                ("x.js", "import {zz} from './dep.js';"),
                ("dep.js", "export const a=1;"),
            ],
            true,
            false,
        );
        let module = module_of(&mut rt, "x.js").unwrap();
        rt.load_requested_modules(module).unwrap();
        assert!(rt.link(module).is_err());
        // The modules are `unlinked` again: the same error.
        let error = rt.link(module).unwrap_err();
        assert!(error.to_string().contains("'zz'"), "{error}");
    });
}

#[test]
fn link_errors_have_a_one_line_stack_and_a_location() {
    // Chromium 148: `stack` is the first line only; the host still reports
    // the error at the import (`x.js:1:9`).
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with(
                &[
                    ("x.js", "import {zz} from './dep.js';"),
                    ("dep.js", "export const a=1;"),
                ],
                stress,
                false,
            );
            let module = module_of(&mut rt, "x.js").unwrap();
            rt.load_requested_modules(module).unwrap();
            assert!(rt.link(module).is_err());
            let error = rt.last_value();
            assert_eq!(
                rt.error_report(error),
                "SyntaxError: The requested module './dep.js' does not provide an export named 'zz'"
            );
            let location = rt.error_location(error).unwrap();
            assert_eq!(
                (location.name.as_str(), location.line, location.column),
                ("x.js", 1, 9)
            );
        }
    });
}

/// Runs the rows (script, expected result) as `print(i + '=' + E(...))` in
/// `y.js`, which `x.js` imports and which imports `x.js` (so `x.js` is in
/// its dead zone); returns the printed lines and the expected ones.
fn dead_zone_rows(x: &str, rows: &[(&str, &str)]) -> (String, String) {
    let mut y = String::from("import * as nx from './x.js';");
    let mut expected = Vec::new();
    for (i, (code, result)) in rows.iter().enumerate() {
        write!(y, "print('{i}='+E(function(){{{code}}}));").unwrap();
        expected.push(format!("{i}={result}"));
    }
    let actual = run_graph(
        &[("main.js", "import './x.js';"), ("x.js", x), ("y.js", &y)],
        "main.js",
    );
    (actual, expected.join("\n"))
}

#[test]
fn namespace_bindings_in_the_dead_zone_have_chromiums_messages() {
    // Chromium 148, cases `msg-namespace-tdz`, `msg-namespace-tdz-2` and
    // `msg-namespace-tdz-callers` of `tools/probes/web-module-graph.json`:
    // `[[Get]]` has its own text, `[[GetOwnProperty]]` and
    // `[[DefineOwnProperty]]` say "v is not defined", and the key
    // enumeration of the namespace object itself says "Cannot access".
    let get = "ReferenceError: Failed to read the 'v' property from 'Module': Cannot access 'v' before initialization";
    let own = "ReferenceError: v is not defined";
    let list = "ReferenceError: Cannot access 'v' before initialization";
    let (actual, expected) = dead_zone_rows(
        "import * as ny from './y.js';export let v=1;export function f(){};",
        &[
            ("return Reflect.get(nx,'v')", get),
            ("var k='v';return nx[k]", get),
            ("var {v}=nx;return v", get),
            ("return nx?.v", get),
            ("return nx.v", get),
            ("return Reflect.getOwnPropertyDescriptor(nx,'v')", own),
            ("return Object.getOwnPropertyDescriptors(nx)", own),
            ("return Object.values(nx)", own),
            ("return Object.entries(nx)", own),
            ("return Reflect.has(nx,'v')", "true"),
            ("return Object.hasOwn(nx,'v')", own),
            ("return Object.prototype.hasOwnProperty.call(nx,'v')", own),
            (
                "return Object.prototype.propertyIsEnumerable.call(nx,'v')",
                own,
            ),
            ("return {...nx}", own),
            ("return Object.assign({},nx)", own),
            ("return Object.defineProperty(nx,'v',{value:1})", own),
            ("return Reflect.defineProperty(nx,'v',{value:1})", own),
            (
                "return Object.freeze(nx)",
                "TypeError: Cannot redefine property: f",
            ),
            ("return Object.isFrozen(nx)", "false"),
            ("return new Proxy(nx,{}).v", get),
            ("return Object.create(nx).v", get),
            ("return Reflect.get(nx,'v',{})", get),
            ("return nx.w", "undefined"),
            (
                "return Object.getOwnPropertyDescriptor(nx,'w')",
                "undefined",
            ),
            (
                "return Object.getOwnPropertyDescriptor(nx,'f').value.name",
                "f",
            ),
            ("return Object.keys(nx).length", list),
            ("var s='';for(var k in nx)s+=k;return s", list),
            ("return JSON.stringify(nx)", list),
            ("return JSON.stringify({o:nx})", list),
        ],
    );
    assert_eq!(actual, expected);
    let (actual, expected) = dead_zone_rows(
        "import * as ny from './y.js';export let v=1;",
        &[
            ("return Object.keys(new Proxy(nx,{}))", own),
            ("return Object.freeze(nx)", own),
            ("return Object.isFrozen(nx)", own),
            ("return Object.isSealed(nx)", own),
            ("return Object.getOwnPropertyNames(nx).join()", "v"),
            (
                "var a=[];for(var k in new Proxy(nx,{}))a.push(k);return a.join()",
                own,
            ),
            ("return JSON.stringify(new Proxy(nx,{}))", own),
            ("return Object.keys(Object.create(nx))", ""),
            (
                "var a=[];for(var k in Object.create(nx))a.push(k);return a.join()",
                list,
            ),
            ("return Object.entries(nx).length", own),
        ],
    );
    assert_eq!(actual, expected);
}

#[test]
fn namespace_define_messages() {
    // Chromium 148, case `msg-namespace-ops-2`: a key that the module does not export
    // is "Cannot redefine property"; a symbol keeps the generic text;
    // `Reflect.defineProperty` returns false.
    let (actual, expected) = {
        let rows = [
            ("return Reflect.defineProperty(ns,'q',{value:1})", "false"),
            (
                "return Reflect.defineProperty(ns,'a',{writable:false})",
                "false",
            ),
            (
                "return Object.defineProperty(ns,Symbol('s'),{value:1})",
                "TypeError: Cannot define property Symbol(s), object is not extensible",
            ),
            (
                "return Object.defineProperty(ns,'q',{value:1})",
                "TypeError: Cannot redefine property: q",
            ),
            (
                "return Object.defineProperties(ns,{q:{value:1}})",
                "TypeError: Cannot redefine property: q",
            ),
            (
                "return Object.defineProperty(ns,'1',{value:1})",
                "TypeError: Cannot redefine property: 1",
            ),
            (
                "return Object.defineProperty(ns,1,{value:1})",
                "TypeError: Cannot redefine property: 1",
            ),
            (
                "return Object.defineProperty(ns,'a',{get:function(){}})",
                "TypeError: Cannot redefine property: a",
            ),
            (
                "return Object.defineProperty(ns,'a',{value:1})===ns",
                "true",
            ),
        ];
        let mut x = String::from("import * as ns from './m.js';");
        let mut expected = Vec::new();
        for (i, (code, result)) in rows.iter().enumerate() {
            write!(x, "print('{i}='+E(function(){{{code}}}));").unwrap();
            expected.push(format!("{i}={result}"));
        }
        (
            run_graph(&[("x.js", &x), ("m.js", "export var a=1")], "x.js"),
            expected.join("\n"),
        )
    };
    assert_eq!(actual, expected);
}

#[test]
fn default_exports() {
    assert_eq!(
        run_graph(
            &[
                ("main.js", "import f from './f.js'; import c from './c.js'; import e from './e.js'; import a from './a.js'; import n from './n.js'; import r from './r.js';
                    print(f.name, f(), c.name, e, a.name, n.name, r);"),
                ("f.js", "export default function () { return 'called' }"),
                ("c.js", "export default class {}"),
                ("e.js", "export default 6 * 7;"),
                ("a.js", "export default (function () {});"),
                ("n.js", "export default function named() {}"),
                ("r.js", "const v = 'renamed'; export { v as default };"),
            ],
            "main.js"
        ),
        "default called default 42 default named renamed"
    );
}

#[test]
fn default_function_exists_before_its_module_runs() {
    assert_eq!(
        run_graph(
            &[
                (
                    "a.js",
                    "import b from './b.js'; export default function () { return 'from a' } print('a');"
                ),
                (
                    "b.js",
                    "import fa from './a.js'; print(fa()); export default 1;"
                ),
            ],
            "a.js"
        ),
        "from a\na"
    );
}

#[test]
fn namespace_reexports() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { ns, alias } from './re.js'; import * as top from './re.js';
                    print(ns.x, alias.x, ns === alias, top.ns === ns, Object.keys(top).join());"
                ),
                (
                    "re.js",
                    "export * as ns from './x.js'; import * as inner from './x.js'; export { inner as alias };"
                ),
                ("x.js", "export const x = 'X';"),
            ],
            "main.js"
        ),
        "X X true true alias,ns"
    );
}

#[test]
fn star_as_namespace_cycles() {
    assert_eq!(
        run_graph(
            &[
                (
                    "a.js",
                    "export * as b from './b.js'; export const va = 'a'; import * as self from './a.js'; print(self.b.a.va, self.b.a.b.vb);"
                ),
                (
                    "b.js",
                    "export * as a from './a.js'; export const vb = 'b';"
                ),
            ],
            "a.js"
        ),
        "a b"
    );
}

#[test]
fn evaluation_errors_are_cached() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with(
                &[
                    ("bad.js", "print('bad runs'); throw new TypeError('boom');"),
                    ("r1.js", "import './bad.js'; print('r1');"),
                    ("r2.js", "import './bad.js'; print('r2');"),
                ],
                stress,
                false,
            );
            assert_eq!(
                run_graph_in(&mut rt, "r1.js"),
                "bad runs\nerror: TypeError: boom @bad.js:1:26"
            );
            // The module runs once; the second root gets the same error.
            assert_eq!(
                run_graph_in(&mut rt, "r2.js"),
                "error: TypeError: boom @bad.js:1:26"
            );
            // The same error object, and Evaluate gives the same promise.
            let bad = loader(&mut rt).modules["bad.js"];
            let r1 = loader(&mut rt).modules["r1.js"];
            let first = rt.evaluate(r1).unwrap();
            let again = rt.evaluate(r1).unwrap();
            assert_eq!(first, again);
            let p = rt.evaluate(bad).unwrap();
            let (_, e1) = rt.promise_settlement(first).unwrap();
            let (_, e2) = rt.promise_settlement(p).unwrap();
            assert_eq!(e1, e2);
            // The rejection is handled: nothing for the rejection tracker.
            assert_eq!(rt.take_unhandled_rejections().len(), 0);
        }
    });
}

#[test]
fn modules_run_once_in_order() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import './a.js'; import './b.js'; import './a.js'; print('main');"
                ),
                ("a.js", "import './c.js'; print('a');"),
                ("b.js", "import './c.js'; import './a.js'; print('b');"),
                ("c.js", "print('c');"),
            ],
            "main.js"
        ),
        "c\na\nb\nmain"
    );
}

#[test]
fn load_errors_reject_the_load() {
    assert_eq!(
        run_graph(&[("main.js", "import './missing.js';")], "main.js"),
        "load: TypeError: Cannot find module 'missing.js'"
    );
    assert_eq!(
        run_graph(
            &[("main.js", "import './bad.js';"), ("bad.js", "let x = ;")],
            "main.js"
        ),
        "load: SyntaxError: Unexpected token ';'"
    );
    assert_eq!(
        run_graph(
            &[
                ("main.js", "import x from './a.js' with { other: 'x' };"),
                ("a.js", "")
            ],
            "main.js"
        ),
        "load: SyntaxError: Invalid attribute key \"other\"."
    );
    // The key error comes before any other error of the attributes, also
    // for `export ... from`.
    for main in [
        "import x from './a.js' with { type: 'json', k: 'v' };",
        "export { x } from './a.js' with { k2: 'v' };",
    ] {
        let key = if main.contains("k2") { "k2" } else { "k" };
        assert_eq!(
            run_graph(&[("main.js", main), ("a.js", "")], "main.js"),
            format!("load: SyntaxError: Invalid attribute key \"{key}\".")
        );
    }
}

#[test]
fn the_hook_sees_each_request_once_per_load() {
    on_big_stack(|| {
        let mut rt = runtime_with(
            &[
                ("main.js", "import './a.js'; import './b.js';"),
                ("a.js", "import './b.js';"),
                ("b.js", "import './a.js';"),
            ],
            false,
            false,
        );
        assert_eq!(run_graph_in(&mut rt, "main.js"), "");
        assert_eq!(loader(&mut rt).requests, ["a.js", "b.js", "a.js", "b.js"]);
        // A second load of a linked graph asks nothing.
        let main = loader(&mut rt).modules["main.js"];
        let promise = rt.load_requested_modules(main).unwrap();
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ));
        assert_eq!(loader(&mut rt).requests.len(), 4);
    });
}

#[test]
fn later_answers_can_come_in_any_order() {
    on_big_stack(|| {
        let mut rt = runtime_with(
            &[
                (
                    "main.js",
                    "import { a } from './a.js'; import { b } from './b.js'; print(a + b);",
                ),
                ("a.js", "export const a = 'a';"),
                ("b.js", "export const b = 'b';"),
            ],
            true,
            true,
        );
        let main = module_of(&mut rt, "main.js").unwrap();
        let promise = rt.load_requested_modules(main).unwrap();
        let waiting: Vec<(u32, String)> = loader(&mut rt).waiting.drain(..).collect();
        assert_eq!(waiting.len(), 2);
        // An unknown token does nothing.
        rt.finish_loading_imported_module(9999, Err(Value::Undefined))
            .unwrap();
        for (token, name) in waiting.into_iter().rev() {
            assert!(matches!(
                rt.promise_settlement(promise),
                Some((PromiseStatus::Pending, _))
            ));
            let result = module_of(&mut rt, &name);
            rt.collect();
            rt.finish_loading_imported_module(token, result).unwrap();
        }
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ));
        rt.link(main).unwrap();
        rt.evaluate(main).unwrap();
        assert_eq!(loader(&mut rt).printed, ["ab"]);
        // A second answer for a token that was answered does nothing.
    });
}

#[test]
fn a_failed_later_answer_rejects_once() {
    on_big_stack(|| {
        let mut rt = runtime_with(
            &[("main.js", "import './a.js'; import './b.js';")],
            false,
            true,
        );
        let main = module_of(&mut rt, "main.js").unwrap();
        let promise = rt.load_requested_modules(main).unwrap();
        let waiting: Vec<(u32, String)> = loader(&mut rt).waiting.drain(..).collect();
        let error = rt.new_error(ThrowKind::TypeError, "first").unwrap();
        rt.finish_loading_imported_module(waiting[0].0, Err(error.into()))
            .unwrap();
        let other = rt.new_error(ThrowKind::TypeError, "second").unwrap();
        rt.finish_loading_imported_module(waiting[1].0, Err(other.into()))
            .unwrap();
        let (status, reason) = rt.promise_settlement(promise).unwrap();
        assert_eq!(status, PromiseStatus::Rejected);
        assert_eq!(reason, Value::Object(error));
    });
}

/// Case `msg-assign-import` of Chromium 148: every write form on an import
/// binding, in the dead zone (`t`) and after the exporter ran (`a`). A pure
/// write throws the `TypeError` without a dead-zone check; a form that reads
/// first throws the `ReferenceError` in the dead zone.
#[test]
fn writes_to_import_bindings() {
    // (code, kind in the dead zone, result after): `T` the TypeError, `R`
    // the ReferenceError, `U` undefined.
    let forms: [(&str, char, char); 23] = [
        ("late = 1", 'T', 'T'),
        ("late++", 'R', 'T'),
        ("--late", 'R', 'T'),
        ("late += 1", 'R', 'T'),
        ("late ??= 1", 'R', 'U'),
        ("late ||= 1", 'R', 'U'),
        ("late &&= 1", 'R', 'T'),
        ("[late] = [1]", 'T', 'T'),
        ("({late} = {late: 1})", 'T', 'T'),
        ("({x: late} = {x: 1})", 'T', 'T'),
        ("for (late of [1]);", 'T', 'T'),
        ("for (late in {a: 1});", 'T', 'T'),
        ("(function(){ late = 1 })()", 'T', 'T'),
        ("(() => late++)()", 'R', 'T'),
        ("eval('late = 1')", 'T', 'T'),
        ("eval('late++')", 'R', 'T'),
        ("lc = 1", 'T', 'T'),
        ("Kl = 1", 'T', 'T'),
        ("def = 1", 'T', 'T'),
        ("typeof late", 'R', 'U'),
        ("late", 'R', 'U'),
        ("[...[late]] = [1]", 'T', 'T'),
        ("({...late} = {})", 'T', 'T'),
    ];
    let text = |k: char| match k {
        'T' => "TypeError: Assignment to constant variable.",
        'R' => "ReferenceError: Cannot access 'late' before initialization",
        _ => "undefined",
    };
    let row = |prefix: &str, i: usize, code: &str| {
        format!("print('{prefix}{i}='+E(function(){{{code}}}));")
    };
    let mut dead = String::new();
    let mut live = String::new();
    let mut expected = Vec::new();
    for (i, (code, _, _)) in forms.iter().enumerate() {
        dead.push_str(&row("t", i, code));
        live.push_str(&row("a", i, code));
    }
    for (i, (_, k, _)) in forms.iter().enumerate() {
        expected.push(format!("t{i}={}", text(*k)));
    }
    for (i, (_, _, k)) in forms.iter().enumerate() {
        expected.push(format!("a{i}={}", text(*k)));
    }
    let dep = format!(
        "import {{late, lc, Kl, default as def}} from './tmain.js';{dead}export function after(){{{live}}}"
    );
    assert_eq!(
        run_graph(
            &[
                (
                    "tmain.js",
                    "import './tdep.js';export let late=3;export const lc=4;export class Kl{};export default function(){};import {after} from './tdep.js';after()"
                ),
                ("tdep.js", &dep),
            ],
            "tmain.js"
        ),
        expected.join("\n")
    );
}

#[test]
fn imports_are_immutable_and_live_in_closures() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { n, bump } from './n.js'; const read = () => n;
                    bump(); print(read()); bump(); print(read(), n);
                    print(E(() => { n++ }), E(() => { n = 1 }), typeof n);"
                ),
                (
                    "n.js",
                    "export let n = 0; export function bump() { n += 1 }"
                ),
            ],
            "main.js"
        ),
        "1\n2 2\nTypeError: Assignment to constant variable. TypeError: Assignment to constant variable. number"
    );
}

#[test]
fn direct_eval_sees_the_imports() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { b } from './b.js'; import * as ns from './b.js'; export let x = 1;
                    print(eval('b'), eval('ns.b'), eval('x'), (() => eval('b + x'))());
                    function f() { return eval('typeof b') } print(f());
                    print(E(() => eval('b = 2')), E(() => (() => eval('b = 3'))()));
                    eval('x = 5'); print(x, new Function('return typeof b')());"
                ),
                ("b.js", "export const b = 'B';"),
            ],
            "main.js"
        ),
        "B B 1 B1\nstring\nTypeError: Assignment to constant variable. TypeError: Assignment to constant variable.\n5 undefined"
    );
}

#[test]
fn module_code_is_strict_with_this_undefined() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { t } from './t.js'; print(this === undefined, t, typeof globalThis.v);"
                ),
                ("t.js", "export const t = this === undefined; var v = 1;"),
            ],
            "main.js"
        ),
        "true true undefined"
    );
}

#[test]
fn errors_in_bodies_have_stack_traces() {
    assert_eq!(
        run_graph(
            &[
                (
                    "main.js",
                    "import { f } from './f.js';\nprint(new Error('x').stack); f();"
                ),
                (
                    "f.js",
                    "export function f() {\n  throw new RangeError('in f');\n}"
                ),
            ],
            "main.js"
        ),
        "Error: x\n    at main.js:2:7\nerror: RangeError: in f @f.js:2:9"
    );
}

// --- Robustness (ADR 0034 section 6) ---

/// Runs `f` in a runtime with a deadline; returns the result and the time.
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let value = f();
    (value, started.elapsed())
}

/// A runtime without stress and with a deadline of `seconds`.
fn hostile_runtime(sources: &[(String, String)], seconds: u64) -> Runtime {
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let mut rt = runtime_with(&refs, false, false);
    rt.set_deadline(Some(Instant::now() + Duration::from_secs(seconds)));
    rt
}

#[test]
fn a_chain_of_10000_modules() {
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
                "export const v = 0;".to_owned()
            };
            sources.push((format!("m{i}.js"), text));
        }
        sources.push((
            "main.js".to_owned(),
            "import { v } from './m0.js'; print(v);".to_owned(),
        ));
        let mut rt = hostile_runtime(&sources, 20);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "9999");
        assert!(time < Duration::from_secs(10), "{time:?}");
        assert_eq!(rt.stack_depths(), (0, 0));
    });
}

#[test]
fn a_chain_of_10000_reexports() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let text = if i + 1 < count {
                format!(
                    "export {{ v }} from './m{}.js'; export * from './m{}.js';",
                    i + 1,
                    i + 1
                )
            } else {
                "export const v = 'end';".to_owned()
            };
            sources.push((format!("m{i}.js"), text));
        }
        sources.push((
            "main.js".to_owned(),
            "import { v } from './m0.js'; import * as ns from './m0.js'; print(v, Object.keys(ns).join());".to_owned(),
        ));
        let mut rt = hostile_runtime(&sources, 20);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "end v");
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn a_ring_cycle_of_10000_modules() {
    on_big_stack(|| {
        let count = 10_000;
        let mut sources = Vec::new();
        for i in 0..count {
            let next = (i + 1) % count;
            sources.push((
                format!("m{i}.js"),
                format!("import {{ f{next} }} from './m{next}.js'; export function f{i}() {{ return {i} }} globalThis.n = (globalThis.n || 0) + 1;"),
            ));
        }
        sources.push((
            "main.js".to_owned(),
            "import { f0 } from './m0.js'; print(f0(), globalThis.n);".to_owned(),
        ));
        let mut rt = hostile_runtime(&sources, 20);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "0 10000");
        assert!(time < Duration::from_secs(10), "{time:?}");
    });
}

#[test]
fn an_export_star_mesh() {
    on_big_stack(|| {
        // 200 modules, each `export *` from all the others: 40,000 edges.
        let count = 200;
        let mut sources = Vec::new();
        for i in 0..count {
            let mut text = format!("export const v{i} = {i};");
            for j in 0..count {
                if j != i {
                    let _ = write!(text, "export * from './m{j}.js';");
                }
            }
            sources.push((format!("m{i}.js"), text));
        }
        sources.push((
            "main.js".to_owned(),
            "import * as ns from './m0.js'; import { v199 } from './m0.js'; print(Object.keys(ns).length, v199);"
                .to_owned(),
        ));
        let mut rt = hostile_runtime(&sources, 30);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "200 199");
        assert!(time < Duration::from_secs(20), "{time:?}");
    });
}

#[test]
fn export_star_cycles() {
    assert_eq!(
        run_graph(
            &[
                ("a.js", "export * from './b.js'; export const a = 'a';"),
                (
                    "b.js",
                    "export * from './c.js'; export * from './a.js'; export const b = 'b';"
                ),
                ("c.js", "export * from './a.js'; export const c = 'c';"),
                (
                    "main.js",
                    "import * as ns from './a.js'; import { c } from './b.js'; print(Object.keys(ns).join(), c);"
                ),
                ("missing.js", "import { nope } from './a.js';"),
            ],
            "main.js"
        ),
        "a,b,c c"
    );
}

#[test]
fn export_star_cycles_without_the_name() {
    assert_eq!(
        run_graph(
            &[
                ("a.js", "export * from './b.js';"),
                ("b.js", "export * from './a.js';"),
                ("main.js", "import { nope } from './a.js';"),
            ],
            "main.js"
        ),
        "link: SyntaxError: The requested module './a.js' does not provide an export named 'nope' @main.js:1:10"
    );
}

#[test]
fn a_module_with_100000_export_names() {
    on_big_stack(|| {
        let count = 100_000;
        let mut text = String::from("const v = 1; export {");
        for i in 0..count {
            let _ = write!(text, "v as e{i},");
        }
        text.push_str("};");
        let sources = vec![
            ("big.js".to_owned(), text),
            ("star.js".to_owned(), "export * from './big.js';".to_owned()),
            (
                "main.js".to_owned(),
                "import * as ns from './star.js'; import { e99999 } from './star.js'; print(Object.keys(ns).length, e99999);"
                    .to_owned(),
            ),
        ];
        let mut rt = hostile_runtime(&sources, 30);
        let (output, time) = timed(|| run_graph_in(&mut rt, "main.js"));
        assert_eq!(output, "100000 1");
        assert!(time < Duration::from_secs(20), "{time:?}");
    });
}

#[test]
fn a_hostile_graph_ends_at_the_deadline() {
    on_big_stack(|| {
        // A mesh too big for the deadline of 200 ms: the link or the
        // namespace ends with a termination, not a hang.
        let count = 600;
        let mut sources = Vec::new();
        for i in 0..count {
            let mut text = format!("export const v{i} = {i};");
            for j in 0..count {
                if j != i {
                    let _ = write!(text, "export * from './m{j}.js';");
                }
            }
            sources.push((format!("m{i}.js"), text));
        }
        sources.push((
            "main.js".to_owned(),
            "import * as ns from './m0.js'; print(Object.keys(ns).length);".to_owned(),
        ));
        let refs: Vec<(&str, &str)> = sources
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let mut rt = runtime_with(&refs, false, false);
        // Parse and load first (the parse of 360,000 requests takes a while).
        let main = module_of(&mut rt, "main.js").unwrap();
        rt.load_requested_modules(main).unwrap();
        rt.set_deadline(Some(Instant::now() + Duration::from_millis(200)));
        let (result, time) = timed(|| rt.link(main).and_then(|()| rt.evaluate(main).map(drop)));
        assert!(
            matches!(result, Err(ScriptError::Terminated(_))),
            "{result:?}"
        );
        assert!(time < Duration::from_secs(5), "{time:?}");
        assert_eq!(rt.stack_depths(), (0, 0));
    });
}

#[test]
fn modules_die_with_their_last_reference() {
    on_big_stack(|| {
        let mut rt = runtime_with(
            &[
                (
                    "main.js",
                    "import * as ns from './dep.js'; globalThis.keep = ns;",
                ),
                (
                    "dep.js",
                    "export const big = new Array(1000).fill('x').join('');",
                ),
            ],
            false,
            false,
        );
        assert_eq!(run_graph_in(&mut rt, "main.js"), "");
        loader(&mut rt).modules.clear();
        rt.collect();
        // The namespace keeps its module alive.
        assert_eq!(
            rt.eval("keep.big.length").unwrap().as_number(),
            Some(1000.0)
        );
        let before = rt.heap().counts().objects;
        rt.eval("delete globalThis.keep").unwrap();
        rt.collect();
        assert!(rt.heap().counts().objects < before);
    });
}

#[test]
fn hooks_are_optional() {
    on_big_stack(|| {
        let mut rt = Runtime::new(RuntimeConfig::default()).unwrap();
        let module = rt
            .parse_module(0, "import './a.js';", "m.js", (0, 0), 0)
            .unwrap();
        // Nothing collects between the parse and the load.
        let promise = rt.load_requested_modules(module).unwrap();
        let (status, reason) = rt.promise_settlement(promise).unwrap();
        assert_eq!(status, PromiseStatus::Rejected);
        assert_eq!(rt.constructor_name(reason), "TypeError");
    });
}

// Top-level `await` (sub-feature 3a-2).
#[path = "module_graph/tla.rs"]
mod tla;

// `import()` and `import.meta` (sub-feature 3a-3).
#[path = "module_graph/dynamic.rs"]
mod dynamic;
