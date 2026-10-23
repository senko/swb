//! The other embedding hooks of ADR 0028 section 6: termination requested
//! by a native function (item 5; section 7), event handler functions with
//! a scope chain of object environments (item 6), host data that the
//! collector traces (item 4) and the heap limit setting (item 8). Each test
//! runs with and without the GC stress mode.

#![allow(clippy::unwrap_used)]

mod common;

use common::host::{TestHost, host_runtime, run_host};
use swb_js::{EventHandler, Gc, HostData, Object, ScriptError, Termination, ThrowKind, Value};

/// Runs `f` with a host runtime in each stress mode, on a big stack.
fn each_mode(f: fn(&mut swb_js::Runtime)) {
    common::on_big_stack(move || {
        for stress in [false, true] {
            let mut rt = host_runtime(stress);
            f(&mut rt);
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
            common::assert_clean(&rt);
        }
    });
}

/// The object that a script evaluates to.
fn object(rt: &mut swb_js::Runtime, source: &str) -> Gc<Object> {
    match rt.eval(source) {
        Ok(Value::Object(object)) => object,
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn a_native_termination_skips_catch_and_finally() {
    each_mode(|rt| {
        let result = rt.eval(
            "try { terminateNow(); print('after'); } catch (e) { print('caught'); } finally { print('finally'); }",
        );
        assert_eq!(
            result,
            Err(ScriptError::Terminated(Termination::Native("test limit")))
        );
        assert_eq!(
            rt.host_data_mut::<TestHost>().unwrap().lines,
            Vec::<String>::new()
        );
        common::assert_clean(rt);
        // The next task runs.
        assert_eq!(run_host(rt, "print('next'); 1 + 1"), "next\n=> 2");
    });
}

#[test]
fn a_termination_ends_nested_runs_and_promise_jobs() {
    each_mode(|rt| {
        // From a function that a native calls (a re-entry), from a getter
        // and from a hook.
        for source in [
            "[1, 2].map(function () { terminateNow(); }); print('no')",
            "var o = { get x() { terminateNow(); } }; try { o.x } finally { print('no') }",
            "var m = makeMap(); try { m.stop = 1; } catch (e) { print('no') }",
            "class X { constructor() { terminateNow(); } } try { new X() } catch (e) { print('no') }",
        ] {
            let result = rt.eval(source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::Native(_)))),
                "{source}: {result:?}"
            );
            assert_eq!(
                rt.host_data_mut::<TestHost>().unwrap().lines,
                Vec::<String>::new(),
                "{source}"
            );
        }
        // A promise job: never a rejection; the queue keeps the others.
        rt.eval(
            "Promise.resolve().then(() => { terminateNow(); }).catch(() => print('rejected')); Promise.resolve().then(() => print('second'))",
        )
        .unwrap();
        assert_eq!(
            rt.run_jobs(),
            Err(ScriptError::Terminated(Termination::Native("test limit")))
        );
        // The other job waits for the next checkpoint.
        assert_eq!(run_host(rt, "print('after')"), "after\nsecond");
    });
}

#[test]
fn a_dropped_termination_still_ends_the_task() {
    each_mode(|rt| {
        // `swallow` drops the error; the next time check ends the task
        // anyway, without a deadline.
        let result = rt.eval("swallow(terminateNow); for (;;) {}");
        assert_eq!(
            result,
            Err(ScriptError::Terminated(Termination::Native("test limit")))
        );
        assert_eq!(
            run_host(rt, "var n = 0; for (var i = 0; i < 100000; i++) n++; n"),
            "=> 100000"
        );
        // A short script has no further time check: the end of the task
        // reports the termination.
        for source in [
            "swallow(()=>{terminateNow()}); 5",
            "swallow(terminateNow); print('x'); 1",
        ] {
            assert_eq!(
                rt.eval(source),
                Err(ScriptError::Terminated(Termination::Native("test limit"))),
                "{source}"
            );
            assert!(run_host(rt, "1 + 1").ends_with("=> 2"));
        }
        // A job that drops it ends the job run; the next task is usable.
        rt.eval("Promise.resolve().then(() => { swallow(terminateNow); })")
            .unwrap();
        assert_eq!(
            rt.run_jobs(),
            Err(ScriptError::Terminated(Termination::Native("test limit")))
        );
        assert_eq!(rt.run_jobs(), Ok(()));
        assert_eq!(run_host(rt, "1 + 1"), "=> 2");
    });
}

#[test]
fn host_data_is_a_root() {
    each_mode(|rt| {
        // An object that only the host state holds survives collections.
        let kept = object(rt, "var o = { tag: 'kept' }; keep(o); o");
        rt.eval("o = null; gc(); gc();").unwrap();
        rt.collect();
        assert!(rt.heap().is_live_object(kept));
        assert_eq!(run_host(rt, "1"), "=> 1");
        // Dropped by the host, it dies.
        rt.eval("forget()").unwrap();
        rt.collect();
        rt.eval("undefined").unwrap();
        rt.collect();
        assert!(!rt.heap().is_live_object(kept));
    });
}

#[test]
fn generic_host_data_traces_its_values() {
    struct Held(Vec<Value>);
    impl swb_js::GenericData for Held {
        fn trace(&self, tracer: &mut swb_js::Tracer<'_>) {
            for value in &self.0 {
                tracer.value(*value);
            }
        }
        fn heap_size(&self) -> usize {
            self.0.capacity() * size_of::<Value>()
        }
    }
    each_mode(|rt| {
        let inner = object(rt, "({ inner: true })");
        let classes = rt.host_data_mut::<TestHost>().unwrap().classes;
        let data = rt.alloc_host_data(Held(vec![inner.into()])).unwrap();
        let wrapper = rt.new_host_object(0, classes.a, data).unwrap();
        rt.host_data_mut::<TestHost>()
            .unwrap()
            .kept
            .push(wrapper.into());
        rt.collect();
        assert!(rt.heap().is_live_object(inner));
        assert_eq!(
            rt.host_object(wrapper.into()).map(|(class, _)| class),
            Some(classes.a)
        );
        let HostData::Generic(generic) = data else {
            panic!("generic data");
        };
        assert_eq!(rt.heap().generic::<Held>(generic).unwrap().0.len(), 1);
        // The host changes the data: a value that it adds is traced too.
        let added = object(rt, "({ added: true })");
        rt.heap_mut()
            .generic_mut::<Held>(generic)
            .unwrap()
            .0
            .push(added.into());
        rt.eval("undefined").unwrap();
        rt.collect();
        assert!(rt.heap().is_live_object(added));
        rt.host_data_mut::<TestHost>().unwrap().kept.clear();
        rt.eval("undefined").unwrap();
        rt.collect();
        assert!(!rt.heap().is_live_object(wrapper));
        assert!(!rt.heap().is_live_object(inner));
        assert!(rt.heap().generic::<Held>(generic).is_err());
    });
}

/// The scope objects of the handler tests: a document, a form and an
/// element (ordinary objects with properties that overlap).
const SCOPE: &str = "var documentObject = { docProp: 'd', shared: 'document', docOnly: 1 }; \
    var formObject = { formProp: 'f', shared: 'form' }; \
    var elementObject = { elProp: 'e', shared: 'element', id: 'el' }; \
    var globalProp = 'g'; var shared = 'global'; \
    [documentObject, formObject, elementObject]";

/// Compiles a handler with the scope of [`SCOPE`] and calls it with the
/// element as `this` and `event` as its argument.
fn handler(rt: &mut swb_js::Runtime, body: &str) -> Result<Gc<Object>, ScriptError> {
    rt.eval(SCOPE).unwrap();
    let scope: Vec<Gc<Object>> = ["documentObject", "formObject", "elementObject"]
        .iter()
        .map(|name| object(rt, name))
        .collect();
    let function = rt.compile_event_handler(&EventHandler {
        realm: 0,
        name: "onclick",
        params: &["event"],
        body,
        scope: &scope,
        script_name: "https://example.test/page.html",
    })?;
    // The host keeps the function in its traced state.
    rt.host_data_mut::<TestHost>()
        .unwrap()
        .kept
        .push(function.into());
    Ok(function)
}

/// Calls `function` with `this` = the element and `event` = "ev".
fn call_handler(rt: &mut swb_js::Runtime, function: Gc<Object>) -> String {
    let element = object(rt, "elementObject");
    let event = rt.eval("'ev'").unwrap();
    match rt.call_function(function.into(), element.into(), &[event]) {
        Ok(value) => rt.display(value),
        Err(error) => error.to_string(),
    }
}

#[test]
fn event_handlers_look_up_the_element_the_form_the_document_then_the_global() {
    each_mode(|rt| {
        let function = handler(
            rt,
            "return [elProp, formProp, docProp, globalProp, shared, this.id, event, typeof onclick].join()",
        )
        .unwrap();
        rt.collect();
        assert_eq!(
            call_handler(rt, function),
            "e,f,d,g,element,el,ev,undefined"
        );
        // An assignment writes the object that has the name; a new name
        // is a global.
        let function = handler(rt, "formProp = 'F'; docOnly = 2; fresh = 3; return 1").unwrap();
        call_handler(rt, function);
        assert_eq!(
            run_host(
                rt,
                "[formObject.formProp, documentObject.docOnly, fresh, 'fresh' in elementObject].join()"
            ),
            "=> F,2,3,false"
        );
        // `@@unscopables` of the element hides a name (a with environment).
        let function = handler(rt, "return shared").unwrap();
        rt.eval("elementObject[Symbol.unscopables] = { shared: true }")
            .unwrap();
        assert_eq!(call_handler(rt, function), "form");
        // Local declarations and the parameter shadow the scope.
        let function = handler(
            rt,
            "var elProp = 'local'; let shared = event; return elProp + shared",
        )
        .unwrap();
        assert_eq!(call_handler(rt, function), "localev");
    });
}

#[test]
fn direct_eval_in_a_handler_sees_the_scope_objects() {
    each_mode(|rt| {
        for (body, expected) in [
            ("return eval('elProp')", "e"),
            ("return eval('docProp')", "d"),
            ("return eval('typeof formProp')", "string"),
            ("return eval('shared')", "element"),
            ("return eval('event')", "ev"),
            ("return eval('typeof nothingHere')", "undefined"),
            ("return (() => eval('elProp'))()", "e"),
            ("return (function () { return eval('docProp') })()", "d"),
            ("return eval('(function () { return formProp })')()", "f"),
            ("eval('docOnly = 5'); return eval('docOnly')", "5"),
        ] {
            let function = handler(rt, body).unwrap();
            assert_eq!(call_handler(rt, function), expected, "{body}");
        }
        let function = handler(rt, "eval('docOnly = 7'); return 1").unwrap();
        call_handler(rt, function);
        assert_eq!(run_host(rt, "documentObject.docOnly"), "=> 7");
    });
}

#[test]
fn event_handlers_are_functions_like_chromium_makes_them() {
    each_mode(|rt| {
        let function = handler(rt, "return 1").unwrap();
        rt.eval("var keptHandler;").unwrap();
        let global = rt.global(0).unwrap();
        let setter = rt.eval("(function (f) { keptHandler = f; })").unwrap();
        rt.call_function(setter, global.into(), &[function.into()])
            .unwrap();
        assert_eq!(
            run_host(
                rt,
                "[keptHandler.toString(), keptHandler.name, keptHandler.length, typeof keptHandler.prototype].join('|')"
            ),
            "=> function onclick(event) {\nreturn 1\n}|onclick|1|object"
        );
        // Sloppy: `this` is the global object for `undefined`; strict: not.
        let sloppy = handler(rt, "return this === globalThis").unwrap();
        assert_eq!(
            rt.call_function(sloppy.into(), Value::Undefined, &[])
                .map(|v| rt.display(v)),
            Ok("true".to_owned())
        );
        let strict = handler(rt, "'use strict'; return typeof this").unwrap();
        assert_eq!(
            rt.call_function(strict.into(), Value::Undefined, &[])
                .map(|v| rt.display(v)),
            Ok("undefined".to_owned())
        );
        // The handler's own name is the element's property, not a binding.
        let named = handler(rt, "return onclick").unwrap();
        rt.eval("elementObject.onclick = 'property'").unwrap();
        assert_eq!(call_handler(rt, named), "property");
    });
}

#[test]
fn event_handlers_have_the_early_errors_of_a_function_body() {
    each_mode(|rt| {
        for (body, message, offset) in [
            // The `}` after the body is unexpected: at the end of the body.
            ("}", "Unexpected token '}'", 1),
            ("return )", "Unexpected token ')'", 7),
            (
                "let event",
                "Identifier 'event' has already been declared",
                4,
            ),
            (
                "}); print('injected'); ({",
                "Single function literal required",
                0,
            ),
            (
                "'use strict'; with (x) {}",
                "Strict mode code may not include a with statement",
                14,
            ),
        ] {
            match handler(rt, body) {
                Err(ScriptError::Compile {
                    kind,
                    message: actual,
                    offset: at,
                }) => {
                    assert_eq!(kind, ThrowKind::SyntaxError, "{body}");
                    assert_eq!(actual, message, "{body}");
                    assert_eq!(at, offset, "{body}");
                }
                other => panic!("{body}: {other:?}"),
            }
        }
        assert_eq!(
            rt.host_data_mut::<TestHost>().unwrap().lines,
            Vec::<String>::new()
        );
        // `yield` and `await` are identifiers in a sloppy body.
        let function = handler(rt, "var yield = 1, await = 2; return yield + await").unwrap();
        assert_eq!(call_handler(rt, function), "3");
    });
}

#[test]
fn an_event_handler_without_scope_objects_reads_globals() {
    common::on_big_stack(|| {
        let mut rt = host_runtime(false);
        rt.eval("var g = 'global'").unwrap();
        let function = rt
            .compile_event_handler(&EventHandler {
                realm: 0,
                name: "onload",
                params: &["event"],
                body: "return g",
                scope: &[],
                script_name: "",
            })
            .unwrap();
        let value = rt
            .call_function(function.into(), Value::Undefined, &[])
            .unwrap();
        assert_eq!(rt.display(value), "global");
    });
}

#[test]
fn the_heap_limit_can_change_between_tasks() {
    common::on_big_stack(|| {
        let mut rt = host_runtime(false);
        rt.set_heap_limit(8 << 20);
        let result = rt.eval("var a = []; for (;;) a.push({ x: [1, 2, 3] });");
        assert_eq!(result, Err(ScriptError::Terminated(Termination::HeapLimit)));
        rt.eval("a = null").unwrap();
        rt.collect();
        rt.set_heap_limit(64 << 20);
        assert_eq!(
            run_host(
                &mut rt,
                "var b = []; for (var i = 0; i < 100000; i++) b.push(i); b.length"
            ),
            "=> 100000"
        );
    });
}
