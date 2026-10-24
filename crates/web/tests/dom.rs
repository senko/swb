//! The runtime of a document with the DOM bindings of feature 4c2
//! (`ScriptRuntime` with a test document): the cases of the Chromium probe
//! `tools/probes/web-dom-tree.json` (prototype chains, property
//! descriptors, brand checks, `instanceof`, wrapper identity, expandos,
//! the tree accessors, `document.title`, the window and its named
//! properties, the console, inline scripts), each with and without the GC
//! stress mode; and expandos and identity across collections.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "dom/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};
use swb_web::{ConsoleLevel, Evaluation, JsonForm, ScriptRuntime};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "chains",
        7,
        r#""[object Element] [object Node] [object EventTarget] [object Object]""#,
        "no SVG interfaces",
    ),
    (
        "chains",
        8,
        r#""[object Element] [object Node] [object EventTarget] [object Object]""#,
        "no SVG interfaces",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn expandos_and_identity_survive_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load("<!DOCTYPE html><body><div>x<p>y</p></div>", stress);
            let set = "{ const d = document.body.firstChild; d.expando = { n: 42 }; d.lastChild.list = d.childNodes; d.childNodes.mark = 'm'; window.kept = d.firstChild; [new WeakRef(d) && 1]; }";
            assert_eq!(answer(&mut rt, set), expected("[1]"));
            rt.collect();
            let check = "{ const d = document.body.firstChild; [d.expando.n, d.lastChild.list === d.childNodes, d.childNodes.mark, window.kept === d.firstChild, d === document.body.firstChild]; }";
            assert_eq!(
                answer(&mut rt, check),
                expected("[42, true, \"m\", true, true]"),
                "stress {stress}"
            );
        }
    });
}

#[test]
fn child_nodes_is_live() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><head></head><body>", false);
        let value = answer(
            &mut rt,
            "const c = document.head.childNodes; const n = c.length; document.title = 'A'; const m = c.length; document.title = 'B'; [n, m, c.length, c[0].nodeName, document.title]",
        );
        assert_eq!(value, expected(r#"[0, 1, 1, "TITLE", "B"]"#));
    });
}

#[test]
fn console_messages_and_errors() {
    on_big_stack(|| {
        let rt = load(
            "<!DOCTYPE html><script>console.log('a', 1, [2]); console.warn('w'); console.error('e'); console.info('i'); console.debug('d'); null.x;</script><script>Promise.reject(new Error('later'));</script>",
            false,
        );
        let lines: Vec<(ConsoleLevel, String)> = rt
            .console_messages()
            .into_iter()
            .map(|m| (m.level, m.text))
            .collect();
        assert_eq!(
            lines,
            [
                (ConsoleLevel::Log, "a 1 [ 2 ]".to_owned()),
                (ConsoleLevel::Warn, "w".to_owned()),
                (ConsoleLevel::Error, "e".to_owned()),
                (ConsoleLevel::Info, "i".to_owned()),
                (ConsoleLevel::Debug, "d".to_owned()),
                (
                    ConsoleLevel::Error,
                    "Uncaught TypeError: Cannot read properties of null (reading 'x')".to_owned()
                ),
                (
                    ConsoleLevel::Error,
                    "Uncaught (in promise) Error: later".to_owned()
                ),
            ]
        );
    });
}

#[test]
fn evaluation_forms_and_promises() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><title>T</title>", false);
        let probe = |rt: &mut ScriptRuntime, source: &str| rt.evaluate(source, JsonForm::Probe);
        assert_eq!(
            probe(
                &mut rt,
                "[NaN, -0, 1e21, Infinity, -Infinity, undefined, 'x\"', {b: 1, a: [null]}]"
            ),
            Evaluation::Value(
                r#"[NaN,-0.0,1e+21,Infinity,-Infinity,null,"x\"",{"b":1,"a":[null]}]"#.to_owned()
            )
        );
        assert_eq!(
            rt.evaluate("[NaN, -0]", JsonForm::Strict),
            Evaluation::Value("[null,0]".to_owned())
        );
        assert_eq!(
            probe(&mut rt, "Promise.resolve(document.title)"),
            Evaluation::Value(r#""T""#.to_owned())
        );
        assert_eq!(
            probe(&mut rt, "Promise.reject(new RangeError('r'))"),
            Evaluation::Error("RangeError: r".to_owned())
        );
        assert_eq!(
            probe(&mut rt, "x.y"),
            Evaluation::Error("ReferenceError: x is not defined".to_owned())
        );
        assert_eq!(
            probe(&mut rt, "JSON.stringify = null; Array.isArray = null; [1]"),
            Evaluation::Value("[1]".to_owned())
        );
        assert!(matches!(
            probe(&mut rt, "new Promise(() => {})"),
            Evaluation::Error(e) if e.contains("did not settle")
        ));
        let cyclic = probe(&mut rt, "const o = {}; o.o = o; o");
        assert_eq!(cyclic, Evaluation::Value(r#"{"o":null}"#.to_owned()));
        // Strict JSON has no lone surrogates; the probe form keeps them.
        assert_eq!(
            rt.evaluate("['\\ud800', { '\\udc00': 1 }]", JsonForm::Strict),
            Evaluation::Value("[\"\u{fffd}\",{\"\u{fffd}\":1}]".to_owned())
        );
        assert_eq!(
            probe(&mut rt, "'\\ud800'"),
            Evaluation::Value(r#""\ud800""#.to_owned())
        );
        // A `DOMException` has accessors for `name` and `message`.
        assert!(matches!(
            probe(&mut rt, "document.body = document.head"),
            Evaluation::Error(e) if e.starts_with("HierarchyRequestError: Failed to set the 'body' property")
        ));
        assert!(
            rt.run_classic_script("document.body = document.head", "a.js")
                .is_err()
        );
        let messages = rt.console_messages();
        let last = messages.last().map(|m| m.text.as_str()).unwrap_or_default();
        assert!(
            last.starts_with("Uncaught HierarchyRequestError: "),
            "{last}"
        );
    });
}

#[test]
fn tasks_end_at_the_time_limit_and_three_stop_the_scripts() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html>", false);
        let limit = std::time::Duration::from_millis(200);
        rt.set_time_limit(limit);
        for round in 0..3 {
            let started = std::time::Instant::now();
            let error = rt.run_classic_script("for (;;) {}", "loop.js").unwrap_err();
            assert!(
                matches!(error, swb_web::ScriptError::Terminated(_)),
                "{error}"
            );
            assert!(started.elapsed() < limit * 5);
            if round < 2 {
                // The runtime runs the next task.
                assert_eq!(answer(&mut rt, "1 + 1"), expected("2"));
                assert!(!rt.stopped());
            }
        }
        assert!(rt.stopped());
        assert!(rt.run_classic_script("1", "after.js").is_err());
        assert_eq!(
            answer(&mut rt, "1"),
            serde_json::Value::String(
                "error: terminated: a native function ended the script: the scripts of the document were stopped"
                    .to_owned()
            )
        );
    });
}

/// The prelude of the interface cases: `t(f)` gives the value or
/// `Name: message`.
const PRELUDE: &str = "function t(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.name + ': ' + e.message; } }";

/// Runs (expression, expected string) cases in a new runtime each, with
/// and without the GC stress mode. The expected values come from Chromium
/// 148 (`tools/probes/web-bindings.json`, `web-legacy.json`,
/// `web-dom-tree.json`) unless a comment says otherwise.
fn check(cases: &[(&str, &str)]) {
    let cases: Vec<(String, String)> = cases
        .iter()
        .map(|&(s, e)| (s.to_owned(), e.to_owned()))
        .collect();
    let failures = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let mut failures = Vec::new();
            for (source, want) in &cases {
                for stress in [false, true] {
                    let mut rt = load("<!DOCTYPE html><body><div>a<p>b</p></div>", stress);
                    rt.run_classic_script(PRELUDE, "prelude").unwrap();
                    let got = answer(&mut rt, source);
                    if got != serde_json::Value::String(want.clone()) {
                        failures.push(format!(
                            "{source} (stress {stress})\n  expected: {want}\n  actual:   {got}"
                        ));
                    }
                }
            }
            failures
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn node_and_event_target_interfaces() {
    check(&[
        // Chromium's list (Node.prototype, from `web-dom-mutation.json`).
        (
            "JSON.stringify(Object.getOwnPropertyNames(Node.prototype))",
            "[\"nodeType\",\"nodeName\",\"baseURI\",\"isConnected\",\"ownerDocument\",\"parentNode\",\"parentElement\",\"childNodes\",\"firstChild\",\"lastChild\",\"previousSibling\",\"nextSibling\",\"nodeValue\",\"textContent\",\"ELEMENT_NODE\",\"ATTRIBUTE_NODE\",\"TEXT_NODE\",\"CDATA_SECTION_NODE\",\"ENTITY_REFERENCE_NODE\",\"ENTITY_NODE\",\"PROCESSING_INSTRUCTION_NODE\",\"COMMENT_NODE\",\"DOCUMENT_NODE\",\"DOCUMENT_TYPE_NODE\",\"DOCUMENT_FRAGMENT_NODE\",\"NOTATION_NODE\",\"DOCUMENT_POSITION_DISCONNECTED\",\"DOCUMENT_POSITION_PRECEDING\",\"DOCUMENT_POSITION_FOLLOWING\",\"DOCUMENT_POSITION_CONTAINS\",\"DOCUMENT_POSITION_CONTAINED_BY\",\"DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC\",\"appendChild\",\"cloneNode\",\"compareDocumentPosition\",\"contains\",\"getRootNode\",\"hasChildNodes\",\"insertBefore\",\"isDefaultNamespace\",\"isEqualNode\",\"isSameNode\",\"lookupNamespaceURI\",\"lookupPrefix\",\"normalize\",\"removeChild\",\"replaceChild\",\"constructor\"]",
        ),
        (
            "JSON.stringify(Object.getOwnPropertyNames(Node))",
            "[\"length\",\"name\",\"prototype\",\"ELEMENT_NODE\",\"ATTRIBUTE_NODE\",\"TEXT_NODE\",\"CDATA_SECTION_NODE\",\"ENTITY_REFERENCE_NODE\",\"ENTITY_NODE\",\"PROCESSING_INSTRUCTION_NODE\",\"COMMENT_NODE\",\"DOCUMENT_NODE\",\"DOCUMENT_TYPE_NODE\",\"DOCUMENT_FRAGMENT_NODE\",\"NOTATION_NODE\",\"DOCUMENT_POSITION_DISCONNECTED\",\"DOCUMENT_POSITION_PRECEDING\",\"DOCUMENT_POSITION_FOLLOWING\",\"DOCUMENT_POSITION_CONTAINS\",\"DOCUMENT_POSITION_CONTAINED_BY\",\"DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC\"]",
        ),
        // Chromium also has `when` (Observable API).
        (
            "JSON.stringify(Object.getOwnPropertyNames(EventTarget.prototype)) + JSON.stringify(Object.getOwnPropertyNames(EventTarget))",
            "[\"addEventListener\",\"dispatchEvent\",\"removeEventListener\",\"constructor\"][\"length\",\"name\",\"prototype\"]",
        ),
        (
            "['addEventListener', 'removeEventListener', 'dispatchEvent'].map((n) => n + ' ' + EventTarget.prototype[n].length + ' ' + EventTarget.prototype[n].name).join()",
            "addEventListener 2 addEventListener,removeEventListener 2 removeEventListener,dispatchEvent 1 dispatchEvent",
        ),
        (
            "(() => { const d = Object.getOwnPropertyDescriptor(Node.prototype, 'ELEMENT_NODE'); const e = Object.getOwnPropertyDescriptor(Node, 'DOCUMENT_POSITION_CONTAINED_BY'); return [d.value, d.writable, d.enumerable, d.configurable, e.value, e.writable, e.enumerable, e.configurable].join(); })()",
            "1,false,true,false,16,false,true,false",
        ),
        (
            "[EventTarget.length, typeof new EventTarget(), Object.prototype.toString.call(new EventTarget()), Object.getPrototypeOf(Node.prototype) === EventTarget.prototype, Object.getPrototypeOf(Node) === EventTarget, Object.getPrototypeOf(EventTarget.prototype) === Object.prototype, Object.getPrototypeOf(EventTarget) === Function.prototype].join()",
            "0,object,[object EventTarget],true,true,true,true",
        ),
        (
            "(() => { const d = Object.getOwnPropertyDescriptor(Node.prototype, 'contains'); return [d.writable, d.enumerable, d.configurable, String(d.value), Object.prototype.hasOwnProperty.call(d.value, 'prototype'), t(() => new d.value())].join('|'); })()",
            "true|true|true|function contains() { [native code] }|false|TypeError: d.value is not a constructor",
        ),
        (
            "[JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, 'Node')), t(() => new Node()), t(() => Node()), t(() => EventTarget()), String(Node)].join('|')",
            "{\"writable\":true,\"enumerable\":false,\"configurable\":true}|TypeError: Failed to construct 'Node': Illegal constructor|TypeError: Illegal constructor|TypeError: Failed to construct 'EventTarget': Please use the 'new' operator, this DOM object constructor cannot be called as a function.|function Node() { [native code] }",
        ),
        (
            "class T extends EventTarget {} const x = new T(); [x instanceof T, x instanceof EventTarget, typeof x.addEventListener].join()",
            "true,true,function",
        ),
    ]);
}

/// `NodeList` (DOM §4.2.10.1) as Chromium 148 shows it
/// (`tools/probes/web-legacy.json`).
#[test]
fn node_list_interface() {
    check(&[
        (
            "[Object.getOwnPropertyNames(NodeList.prototype), Reflect.ownKeys(NodeList.prototype).map(String)].join(' | ')",
            "entries,keys,values,forEach,length,item,constructor | entries,keys,values,forEach,length,item,constructor,Symbol(Symbol.toStringTag),Symbol(Symbol.iterator)",
        ),
        (
            "const p = NodeList.prototype; [p[Symbol.iterator] === Array.prototype.values, p.forEach === Array.prototype.forEach, p.entries === Array.prototype.entries, p.keys === Array.prototype.keys, p.values === Array.prototype.values, p.item.length, t(() => new NodeList())].join()",
            "true,true,true,true,true,1,TypeError: Failed to construct 'NodeList': Illegal constructor",
        ),
        (
            "const d = document.body.firstChild; const l = d.childNodes; [Object.keys(l), l[0] === d.firstChild, l.item(1) === d.lastChild, l.length, l[2], [...l].length, 1 in l, Object.prototype.toString.call(l)].join(' | ')",
            "0,1 | true | true | 2 |  | 2 | true | [object NodeList]",
        ),
        (
            "const l = document.body.childNodes; [t(() => { 'use strict'; l[0] = 1; }), t(() => { 'use strict'; delete l[0]; }), t(() => Object.defineProperty(l, 0, { value: 1 })), t(() => Object.freeze(l))].join(' | ')",
            "TypeError: Failed to set an indexed property [0] on 'NodeList': Indexed property setter is not supported. | TypeError: Failed to delete an indexed property [0] from 'NodeList': Index property deleter is not supported. | TypeError: Failed to set an indexed property [0] on 'NodeList': Index property setter is not supported. | TypeError: Cannot freeze",
        ),
    ]);
}

#[test]
fn glue_checks_of_node_and_event_target() {
    check(&[
        (
            "const n = document.body; [t(() => Node.prototype.isSameNode.call(n)), t(() => n.contains()), t(() => n.contains(undefined)), t(() => n.contains(5)), n.contains(n)].join('|')",
            "TypeError: Failed to execute 'isSameNode' on 'Node': 1 argument required, but only 0 present.|TypeError: Failed to execute 'contains' on 'Node': 1 argument required, but only 0 present.|false|TypeError: Failed to execute 'contains' on 'Node': parameter 1 is not of type 'Node'.|true",
        ),
        (
            "const n = document.body; [t(() => n.addEventListener('x', 5)), t(() => n.addEventListener('x', 'str')), t(() => n.addEventListener('x', null)), t(() => n.addEventListener('x')), t(() => n.addEventListener('x', {})), t(() => n.addEventListener()), t(() => n.addEventListener('x', () => 1, 7))].join('|')",
            "TypeError: Failed to execute 'addEventListener' on 'EventTarget': parameter 2 is not of type 'Object'.|TypeError: Failed to execute 'addEventListener' on 'EventTarget': parameter 2 is not of type 'Object'.|undefined|TypeError: Failed to execute 'addEventListener' on 'EventTarget': 2 arguments required, but only 1 present.|undefined|TypeError: Failed to execute 'addEventListener' on 'EventTarget': 2 arguments required, but only 0 present.|undefined",
        ),
        (
            "const f = Object.getOwnPropertyDescriptor(Node.prototype, 'nodeType').get; [t(() => f.call(5)), t(() => f.call('s')), t(() => Node.prototype.contains.call(5, document.body)), t(() => f.call(new EventTarget())), f.call(document.body)].join('|')",
            "TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Illegal invocation|1",
        ),
        (
            "[t(() => new EventTarget().dispatchEvent()), t(() => new EventTarget().dispatchEvent({})), new EventTarget().dispatchEvent(new Event('x'))].join('|')",
            "TypeError: Failed to execute 'dispatchEvent' on 'EventTarget': 1 argument required, but only 0 present.|TypeError: Failed to execute 'dispatchEvent' on 'EventTarget': parameter 1 is not of type 'Event'.|true",
        ),
    ]);
}

#[test]
fn event_interface() {
    check(&[
        (
            "[t(() => new Event('x', 5)), t(() => new Event('x', null).bubbles), t(() => new Event('x', undefined).bubbles), t(() => new Event('x', { bubbles: 1 }).bubbles), t(() => new Event('x', 'str')), t(() => new Event('x', () => 1).bubbles), t(() => new Event())].join('|')",
            "TypeError: Failed to construct 'Event': The provided value is not of type 'EventInit'.|false|false|true|TypeError: Failed to construct 'Event': The provided value is not of type 'EventInit'.|false|TypeError: Failed to construct 'Event': 1 argument required, but only 0 present.",
        ),
        (
            "const order = []; const init = {}; for (const k of ['composed', 'cancelable', 'bubbles']) Object.defineProperty(init, k, { get() { order.push(k); return 1; } }); const e = new Event('x', init); [order.join(), e.type, e.bubbles, e.cancelable, e.composed].join('|')",
            "bubbles,cancelable,composed|x|true|true|true",
        ),
        (
            "t(() => new Event('x', { get bubbles() { throw new SyntaxError('in getter'); } }))",
            "SyntaxError: in getter",
        ),
        (
            "(() => { const e = new Event('x'); const d = Object.getOwnPropertyDescriptor(e, 'isTrusted'); const o = Object.getOwnPropertyDescriptor(new Event('y'), 'isTrusted'); return [JSON.stringify(Object.getOwnPropertyNames(e)), typeof d.get, d.enumerable, d.configurable, d.get.name, Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted') === undefined, Event.length, d.get === o.get].join('|'); })()",
            "[\"isTrusted\"]|function|true|false|get isTrusted|true|1|true",
        ),
        // Spec order of the attributes (Chromium lists its own IDL order).
        (
            "JSON.stringify(Object.getOwnPropertyNames(Event.prototype))",
            "[\"type\",\"target\",\"srcElement\",\"currentTarget\",\"eventPhase\",\"cancelBubble\",\"bubbles\",\"cancelable\",\"returnValue\",\"defaultPrevented\",\"composed\",\"timeStamp\",\"NONE\",\"CAPTURING_PHASE\",\"AT_TARGET\",\"BUBBLING_PHASE\",\"composedPath\",\"initEvent\",\"preventDefault\",\"stopImmediatePropagation\",\"stopPropagation\",\"constructor\"]",
        ),
        (
            "class C extends Event {} const c = new C('q'); [c instanceof C, c.type, Object.getOwnPropertyNames(c).join()].join()",
            "true,q,isTrusted",
        ),
        ("JSON.stringify(new Event('x').composedPath())", "[]"),
    ]);
}

#[test]
fn dom_exception() {
    check(&[
        (
            "(() => { const e = new DOMException(); return [e.name, e.message, e.code, String(e), Object.prototype.toString.call(e)].join('|'); })()",
            "Error||0|Error|[object DOMException]",
        ),
        (
            "(() => { const e = new DOMException('m', 'SyntaxError'); return [e.name, e.message, e.code, String(e), new DOMException('m', 'Bogus').code, new DOMException(undefined, undefined).name, new DOMException(null, null).message, new DOMException(1, 2).name].join('|'); })()",
            "SyntaxError|m|12|SyntaxError: m|0|Error|null|2",
        ),
        (
            "(() => { const e = new DOMException('m'); return [e instanceof Error, e instanceof DOMException, Object.getPrototypeOf(DOMException.prototype) === Error.prototype, Object.getPrototypeOf(DOMException) === Function.prototype, DOMException.length, DOMException.name, Error.prototype.isPrototypeOf(e)].join(); })()",
            "true,true,true,true,0,DOMException,true",
        ),
        (
            "[t(() => DOMException()), t(() => Object.getOwnPropertyDescriptor(DOMException.prototype, 'code').get.call({})), t(() => Object.getOwnPropertyDescriptor(DOMException.prototype, 'message').get.call(new Error('x'))), t(() => new DOMException(Symbol()))].join('|')",
            "TypeError: Failed to construct 'DOMException': Please use the 'new' operator, this DOM object constructor cannot be called as a function.|TypeError: Illegal invocation|TypeError: Illegal invocation|TypeError: Failed to construct 'DOMException': Cannot convert a Symbol value to a string",
        ),
        (
            "(() => { try { document.body = document.head; } catch (e) { return [e instanceof DOMException, e.name, e.code, e.message].join('|'); } })()",
            "true|HierarchyRequestError|3|Failed to set the 'body' property on 'Document': The new body element is of type 'HEAD'. It must be either a 'BODY' or 'FRAMESET' element.",
        ),
    ]);
}
