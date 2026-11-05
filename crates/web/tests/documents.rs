//! Several documents in one runtime (M9 feature 12a, ADR 0031 section 5): two
//! documents in two realms stay apart in their wrappers, `[SameObject]`
//! objects, listeners, timers and `document`; a function of one realm acts on
//! its own document when the other realm calls it; a native follows its
//! `this`, and the document stays right across re-entry. Each test runs with
//! and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::Duration;

use common::{TestDocument, answer, expected, load_with, on_big_stack};
use swb_web::{ClockMode, DocumentKey, ScriptRuntime};

const FIRST: &str = "<!DOCTYPE html><html><head><title>first</title></head>\
    <body id=body-a><p id=a>text of a</p><form id=form-a></form></body></html>";
const SECOND: &str = "<!DOCTYPE html><html><head><title>second</title></head>\
    <body id=body-b><p id=b>text of b</p><form id=form-b></form></body></html>";

/// A runtime with two parsed documents that know each other as the global
/// `other`.
fn two_documents(stress: bool) -> (ScriptRuntime, DocumentKey) {
    let mut rt = load_with(TestDocument::parsing(FIRST), stress, ClockMode::Virtual);
    let second = rt.add_document(1).unwrap();
    rt.lend_document(second, Box::new(TestDocument::parsing(SECOND)));
    rt.with_document(second, |rt| {
        rt.start_document();
        rt.fire_load_event();
        rt.run_due_tasks(Duration::from_secs(5));
    })
    .unwrap();
    rt.expose_window(DocumentKey::FIRST, "other", second)
        .unwrap();
    rt.expose_window(second, "other", DocumentKey::FIRST)
        .unwrap();
    (rt, second)
}

/// Runs `test` with and without the GC stress mode, on a big stack.
fn both_modes(test: impl Fn(bool) + Send + Sync + Copy + 'static) {
    on_big_stack(move || {
        test(false);
        test(true);
    });
}

/// The value of an expression in the first document.
fn first(rt: &mut ScriptRuntime, source: &str) -> serde_json::Value {
    answer(rt, source)
}

/// The value of an expression in the document `key`.
fn second(rt: &mut ScriptRuntime, key: DocumentKey, source: &str) -> serde_json::Value {
    rt.with_document(key, |rt| answer(rt, source)).unwrap()
}

#[test]
fn each_document_has_its_own_window_and_document() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        assert_eq!(first(&mut rt, "document.title"), expected(r#""first""#));
        assert_eq!(
            second(&mut rt, key, "document.title"),
            expected(r#""second""#)
        );
        assert_eq!(
            first(
                &mut rt,
                "[window !== other, other.window === other, other.self === other, \
                  other.document !== document, other.document.defaultView === other, \
                  other.other === window, other.Array !== Array, other.globalThis === other, \
                  other.document.title, other.location !== location, \
                  other.navigator !== navigator, other.history !== history, \
                  other.performance !== performance, other.screen !== screen, \
                  other.document === other.document, other.location === other.location, \
                  other.Node === other.Node, other.document instanceof other.Document, \
                  other.document instanceof Document]"
            ),
            expected(
                r#"[true, true, true, true, true, true, true, true, "second", true, true, true,
                    true, true, true, true, true, true, false]"#
            )
        );
    });
}

#[test]
fn wrappers_and_same_objects_stay_apart() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        // The two `body` elements have the same index in their arenas.
        assert_eq!(
            first(
                &mut rt,
                "document.body.expando = 'a';
                 const aBody = document.body, bBody = other.document.body;
                 [aBody === bBody, aBody.id, bBody.id, String(bBody.expando),
                  aBody.classList === aBody.classList, bBody.classList === bBody.classList,
                  aBody.classList === bBody.classList, aBody.children === bBody.children,
                  bBody.children === bBody.children, aBody.childNodes === aBody.childNodes,
                  aBody.style === bBody.style, bBody.style === bBody.style,
                  aBody.attributes === bBody.attributes,
                  bBody.getAttribute('id'), bBody.attributes.id.value, bBody.children[0].id,
                  bBody.firstChild.id, bBody instanceof HTMLElement,
                  bBody instanceof other.HTMLElement, bBody.ownerDocument === other.document,
                  other.document.documentElement.lastChild === bBody,
                  other.document.getElementById('a'), other.document.getElementById('b').id,
                  document.getElementById('b')]"
            ),
            expected(
                r#"[false, "body-a", "body-b", "undefined", true, true, false, false, true, true,
                    false, true, false, "body-b", "body-b", "b", "b", false, true, true, true,
                    null, "b", null]"#
            )
        );
        assert_eq!(
            second(
                &mut rt,
                key,
                "[document.body.id, String(document.body.expando)]"
            ),
            expected(r#"["body-b", "undefined"]"#)
        );
    });
}

#[test]
fn listeners_stay_with_their_document() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        assert_eq!(
            first(
                &mut rt,
                "var log = [];
                 document.body.addEventListener('x', () => log.push('a-body'));
                 other.document.body.addEventListener('x', e => log.push('b-body', e.target.id));
                 addEventListener('x', function () { log.push('a-window', this === window); });
                 other.addEventListener('x', function () { log.push('b-window', this === other); });
                 document.body.dispatchEvent(new Event('x', {bubbles: true}));
                 log.push('|');
                 other.document.body.dispatchEvent(new other.Event('x', {bubbles: true}));
                 log.push('|');
                 other.dispatchEvent(new Event('x'));
                 log"
            ),
            expected(
                r#"["a-body", "a-window", true, "|", "b-body", "body-b", "b-window", true, "|",
                    "b-window", true]"#
            )
        );
        // A handler attribute's scope and a handler's `this`.
        assert_eq!(
            second(
                &mut rt,
                key,
                "document.body.setAttribute('onclick', 'window.seen = [document.title, this.id]');
                 document.body.click();
                 window.seen"
            ),
            expected(r#"["second", "body-b"]"#)
        );
    });
}

#[test]
fn timers_belong_to_the_window_whose_method_was_called() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        assert_eq!(
            first(
                &mut rt,
                "var log = [];
                 var idA = setTimeout(function () { log.push('a', this === window, document.title); }, 0);
                 setTimeout(() => {}, 0);
                 var idB = other.setTimeout(function () { log.push('b', this === other); }, 0);
                 other.setTimeout('window.fromString = [document.title, this === window]', 0);
                 [typeof idA, typeof idB, idA, idB]"
            ),
            expected(r#"["number", "number", 1, 1]"#)
        );
        rt.run_due_tasks(Duration::from_secs(5));
        assert_eq!(
            first(&mut rt, "log.slice(0, 5)"),
            expected(r#"["a", true, "first", "b", true]"#)
        );
        assert_eq!(
            second(&mut rt, key, "window.fromString"),
            expected(r#"["second", true]"#)
        );
        // The ids are per window: a clear of the number of a timer of the
        // other window leaves it.
        assert_eq!(
            second(
                &mut rt,
                key,
                "window.ran = false;
                 window.id = setTimeout(() => { window.ran = true; }, 0);
                 window.id"
            ),
            expected("3")
        );
        assert_eq!(
            first(
                &mut rt,
                "window.ran = false;
                 window.id = setTimeout(() => { window.ran = true; }, 0);
                 other.clearTimeout(window.id + 1000);
                 [window.id, other.id]"
            ),
            expected("[3, 3]")
        );
        first(&mut rt, "clearTimeout(other.id); 0");
        rt.run_due_tasks(Duration::from_secs(5));
        assert_eq!(second(&mut rt, key, "window.ran"), expected("true"));
        assert_eq!(first(&mut rt, "window.ran"), expected("false"));
    });
}

#[test]
fn a_function_of_one_realm_acts_on_its_own_document() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        second(
            &mut rt,
            key,
            "window.title = function () { return document.title; };
             window.make = function () { return document.createElement('div'); };
             window.me = function () { return this; };
             0",
        );
        assert_eq!(
            first(
                &mut rt,
                "const div = other.make();
                 [other.title(), div.ownerDocument === other.document, div instanceof HTMLElement,
                  div instanceof other.HTMLElement, (0, other.me)() === other,
                  other.me.call(undefined) === other, document.title]"
            ),
            expected(r#"["second", true, false, true, true, true, "first"]"#)
        );
    });
}

#[test]
fn a_native_follows_its_this() {
    both_modes(|stress| {
        let (mut rt, _) = two_documents(stress);
        assert_eq!(
            first(
                &mut rt,
                "const titleGet = Object.getOwnPropertyDescriptor(Document.prototype, 'title').get;
                 const docGet = Object.getOwnPropertyDescriptor(window, 'document').get;
                 const made = document.createElement.call(other.document, 'span');
                 const f = new other.Event('y');
                 [titleGet.call(other.document), titleGet.call(document),
                  docGet.call(other) === other.document, docGet.call(undefined) === document,
                  document.getElementById.call(other.document, 'b').id,
                  made.ownerDocument === other.document, made instanceof other.HTMLElement,
                  Node.prototype.contains.call(other.document.body, other.document.body.firstChild),
                  HTMLCollection.prototype.item.call(other.document.body.children, 0).id,
                  Element.prototype.getAttribute.call(other.document.body, 'id'),
                  EventTarget.prototype.dispatchEvent.call(other, f),
                  f.target === other, f.currentTarget, new Event('y').view]"
            ),
            expected(
                r#"["second", "first", true, true, "b", true, true, true, "b", "body-b", true,
                    true, null, null]"#
            )
        );
    });
}

#[test]
fn the_document_stays_right_across_re_entry() {
    both_modes(|stress| {
        let (mut rt, _) = two_documents(stress);
        assert_eq!(
            first(
                &mut rt,
                "var log = [];
                 other.document.body.addEventListener('deep', () => {
                   log.push(other.document.title, document.title, other.document.body.id);
                   document.body.dispatchEvent(new Event('deeper'));
                   log.push(other.document.title, document.title);
                 });
                 document.body.addEventListener('deeper', () => {
                   log.push('deeper', document.body.id, other.document.body.id);
                 });
                 document.body.addEventListener('start', () => {
                   log.push(document.title);
                   other.document.body.dispatchEvent(new other.Event('deep'));
                   log.push(document.title, document.body.id);
                 });
                 document.body.dispatchEvent(new Event('start'));
                 log.push(document.title);
                 log"
            ),
            expected(
                r#"["first", "second", "first", "body-b", "deeper", "body-a", "body-b", "second",
                    "first", "first", "body-a", "first"]"#
            )
        );
        // An exception through two documents leaves the first one current.
        // `active_timers` reads the current document without a native, which
        // would switch to its own document first.
        first(&mut rt, "setTimeout(() => {}, 1000000); 0");
        assert_eq!(rt.active_timers(), 1);
        assert_eq!(
            first(
                &mut rt,
                "var thrown = 'none';
                 try {
                   other.document.body.querySelector.call(
                     other.document.body, {toString() { throw new RangeError('y'); }});
                 } catch (e) { thrown = e.name; }
                 [thrown, document.title, document.body.id]"
            ),
            expected(r#"["RangeError", "first", "body-a"]"#)
        );
        assert_eq!(rt.active_timers(), 1, "after a throw from a native");
        assert_eq!(
            first(
                &mut rt,
                "var thrown = 'none';
                 try { other.document.body.dataset['a-b'] = 1; } catch (e) { thrown = e.name; }
                 thrown"
            ),
            expected(r#""SyntaxError""#)
        );
        assert_eq!(rt.active_timers(), 1, "after a throw from a hook");
        first(
            &mut rt,
            "other.document.body.addEventListener('throw', () => { throw new Error('x'); });
             other.document.body.dispatchEvent(new other.Event('throw'));
             0",
        );
        assert_eq!(rt.active_timers(), 1, "after a throw from a listener");
    });
}

#[test]
fn a_record_from_a_hook_reaches_its_observer_after_a_native_of_the_other_document() {
    both_modes(|stress| {
        let (mut rt, _) = two_documents(stress);
        // A write to `dataset` runs in a hook, which ends without a look at
        // the request of the registry. The next native belongs to the first
        // document, and the request cell is shared.
        assert_eq!(
            first(
                &mut rt,
                "new Promise(resolve => {
                   const log = [];
                   const body = other.document.body;
                   new other.MutationObserver(records => log.push(records.length))
                     .observe(body, {attributes: true});
                   body.dataset.k = 'v';
                   log.push(document.title);
                   setTimeout(() => resolve(log), 0);
                 })"
            ),
            expected(r#"["first", 1]"#)
        );
    });
}

#[test]
fn a_termination_leaves_the_observers_of_the_other_documents_working() {
    both_modes(|stress| {
        let (mut rt, _) = two_documents(stress);
        first(
            &mut rt,
            "window.count = 0;
             new other.MutationObserver(records => { count += records.length; })
               .observe(other.document.body, {attributes: true});
             0",
        );
        // The job queue of the agent goes with the termination, also the
        // microtask of the second document.
        rt.set_time_limit(Duration::from_secs(1));
        first(
            &mut rt,
            "queueMicrotask(() => { while (true) {} });
             other.document.body.setAttribute('t', '1');
             0",
        );
        rt.set_time_limit(Duration::from_secs(5));
        assert_eq!(
            first(
                &mut rt,
                "new Promise(resolve => {
                   other.document.body.setAttribute('t', '2');
                   setTimeout(() => resolve(count), 0);
                 })"
            ),
            expected("2")
        );
    });
}

#[test]
fn hooks_of_platform_objects_read_their_own_document() {
    both_modes(|stress| {
        let (mut rt, _) = two_documents(stress);
        assert_eq!(
            first(
                &mut rt,
                "const children = other.document.body.children;
                 const forms = other.document.forms;
                 const map = other.document.body.attributes;
                 const data = other.document.body.dataset;
                 const style = other.document.body.style;
                 data.k = 'v';
                 style.color = 'red';
                 [children.length, children[0].id, children['form-b'].id, 'b' in children,
                  Object.keys(children).join(), forms.length, forms['form-b'].id, map.id.value,
                  data.k, other.document.body.getAttribute('data-k'), style.color,
                  other.document.body.getAttribute('style'), document.body.getAttribute('style'),
                  other['form-b'].id, other.b.id, String(window.b), window.a.id, String(other.a),
                  'b' in other, 'b' in window, document.body.dataset.k === undefined]"
            ),
            expected(
                r#"[2, "b", "form-b", true, "0,1", 1, "form-b", "body-b", "v", "v", "red",
                    "color: red;", null, "form-b", "b", "undefined", "a", "undefined", true,
                    false, true]"#
            )
        );
    });
}

#[test]
fn a_node_of_another_document_fails_as_an_argument() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        // Feature 12b adopts the node. Here the conversion refuses it, and
        // no arena sees a `NodeId` of another one.
        assert_eq!(
            first(
                &mut rt,
                "const E = g => { try { return String(g()); } catch (e) { return e.name + ': ' + e.message; } };
                 [E(() => document.body.appendChild(other.document.body)),
                  E(() => document.body.contains(other.document.body)),
                  E(() => other.document.body.contains(document.body)),
                  E(() => document.body.insertBefore(other.document.createElement('p'), null)),
                  E(() => getComputedStyle(other.document.body)),
                  other.document.body.parentNode === other.document.documentElement,
                  document.body.childNodes.length, other.document.body.childNodes.length]"
            ),
            expected(
                r#"["TypeError: a node of another document", "TypeError: a node of another document",
                    "TypeError: a node of another document", "TypeError: a node of another document",
                    "TypeError: a node of another document", true, 2, 2]"#
            )
        );
        assert_eq!(
            second(&mut rt, key, "document.body.childNodes.length"),
            expected("2")
        );
    });
}

#[test]
fn observers_and_callbacks_run_in_their_document() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        assert_eq!(
            first(
                &mut rt,
                "new Promise(resolve => {
                   const log = [];
                   const target = other.document.body;
                   new other.MutationObserver(function (records, observer) {
                     log.push(records.length, records[0].target === target, this === observer);
                   }).observe(target, {attributes: true});
                   target.setAttribute('x', '1');
                   new MutationObserver(() => log.push('wrong')).observe(document.body, {attributes: true});
                   other.queueMicrotask(() => log.push('microtask'));
                   other.setTimeout(() => {
                     log.push(other.document.title);
                     resolve(log);
                   }, 0);
                 })"
            ),
            expected(r#"[1, true, true, "microtask", "second"]"#)
        );
        assert_eq!(
            second(&mut rt, key, "document.body.getAttribute('x')"),
            expected(r#""1""#)
        );
        assert_eq!(
            first(&mut rt, "document.body.getAttribute('x')"),
            expected("null")
        );
    });
}

#[test]
fn events_of_the_engine_go_to_the_document() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        second(
            &mut rt,
            key,
            "window.seen = [];
             addEventListener('from-engine', e => seen.push(e.target === window, this === window, e.isTrusted));
             0",
        );
        first(
            &mut rt,
            "window.seen = []; addEventListener('from-engine', () => seen.push('wrong')); 0",
        );
        rt.with_document(key, |rt| {
            rt.dispatch_event(
                swb_web::DispatchTarget::Window,
                &swb_web::TrustedEvent::new("from-engine"),
            )
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            second(&mut rt, key, "window.seen"),
            expected("[true, true, true]")
        );
        assert_eq!(first(&mut rt, "window.seen"), expected("[]"));
    });
}

#[test]
fn a_new_document_of_a_known_frame_takes_over_its_proxy() {
    both_modes(|stress| {
        let (mut rt, key) = two_documents(stress);
        first(
            &mut rt,
            "window.old = other; other.expando = 1; window.ids = new WeakMap([[other, 'kept']]); 0",
        );
        // A navigation of the frame 1 commits: a new realm and a new document.
        let next = rt.add_document(1).unwrap();
        rt.lend_document(next, Box::new(TestDocument::parsing(SECOND)));
        rt.with_document(next, |rt| {
            rt.start_document();
            rt.fire_load_event();
        })
        .unwrap();
        assert_eq!(
            first(
                &mut rt,
                "[old === other, ids.get(other), String(other.expando), other.document.title]"
            ),
            expected(r#"[true, "kept", "undefined", "second"]"#)
        );
        assert_eq!(
            second(&mut rt, next, "[window === globalThis, this === window]"),
            expected("[true, true]")
        );
        // The old `Window` keeps its state for code that still runs there:
        // its global names; `window` is the proxy, with the new target.
        assert_eq!(
            second(
                &mut rt,
                key,
                "[expando, String(window.expando), window.document.title, document.title]"
            ),
            expected(r#"[1, "undefined", "second", "second"]"#)
        );
    });
}
