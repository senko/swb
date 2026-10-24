//! The event core of feature 5a (DOM §2, ADR 0028 section 11): the cases
//! of the Chromium probe `tools/probes/web-events.json` (phases and order,
//! `once`, `passive`, duplicates, changes during a dispatch, propagation,
//! the dispatch flag, `composedPath`, `window.event`, the event
//! interfaces), each with and without the GC stress mode; the console
//! lines of exceptions in listeners; the engine's entry for trusted
//! events; listeners across collections; nested dispatches.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "events/cases.rs"]
mod cases;
mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};
use swb_dom::NodeId;
use swb_web::{ConsoleLevel, DispatchTarget, EventInterface, ScriptRuntime, TrustedEvent};

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    (
        "once-passive",
        7,
        r#""capture,once,passive | capture""#,
        "no `signal` option (no AbortSignal yet)",
    ),
    (
        "event-attributes",
        3,
        r#"[true, "r", null, "TypeError: Failed to construct 'PromiseRejectionEvent': 2 arguments required, but only 1 present.", "TypeError: Failed to construct 'PromiseRejectionEvent': Failed to read the 'promise' property from 'PromiseRejectionEventInit': Required member is undefined.", "TypeError: Failed to construct 'PromiseRejectionEvent': Failed to read the 'promise' property from 'PromiseRejectionEventInit': Failed to convert value to 'object'.", 2, "[\"promise\",\"reason\",\"constructor\"]"]"#,
        "`promise` is an `object` in HTML (Chromium: `Promise<any>`, which takes any value)",
    ),
];

#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The error lines of the console.
fn errors(rt: &ScriptRuntime) -> Vec<String> {
    rt.console_messages()
        .into_iter()
        .filter(|m| m.level == ConsoleLevel::Error)
        .map(|m| m.text)
        .collect()
}

/// The page of the dispatch tests.
const TREE: &str = "<!DOCTYPE html><body><div id=a><p id=b><span id=c>t</span></p></div>";

#[test]
fn exceptions_in_listeners_are_reported_and_the_dispatch_goes_on() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(TREE, stress);
            let source = "const log = []; c.addEventListener('x', () => { log.push(1); throw new TypeError('first'); }); c.addEventListener('x', () => { log.push(2); null.f(); }); c.addEventListener('x', () => { throw 'a string'; }); c.addEventListener('x', () => { throw { custom: 1 }; }); c.addEventListener('x', () => { throw new DOMException('dom message', 'NotFoundError'); }); c.addEventListener('x', {}); c.addEventListener('x', { get handleEvent() { throw new RangeError('getter'); } }); c.addEventListener('x', () => log.push(3)); b.addEventListener('x', () => log.push('b')); [c.dispatchEvent(new Event('x', { bubbles: true })), log.join()]";
            assert_eq!(answer(&mut rt, source), expected(r#"[true, "1,2,3,b"]"#));
            // Chromium's messages (the probe's `exceptions` case).
            assert_eq!(
                errors(&rt),
                [
                    "Uncaught TypeError: first",
                    "Uncaught TypeError: Cannot read properties of null (reading 'f')",
                    "Uncaught a string",
                    "Uncaught [object Object]",
                    "Uncaught NotFoundError: dom message",
                    "Uncaught TypeError: The provided callback is not callable.",
                    "Uncaught RangeError: getter",
                ],
                "stress {stress}"
            );
        }
    });
}

#[test]
fn the_engine_fires_trusted_events_as_tasks() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(TREE, stress);
            let setup = "window.log = []; const name = (x) => x === window ? 'window' : x === document ? 'document' : x.localName; for (const x of [window, document, c]) x.addEventListener('ping', (e) => { log.push([name(e.currentTarget), e.eventPhase, e.isTrusted, name(e.target), e instanceof PageTransitionEvent && e.persisted].join()); Promise.resolve().then(() => log.push('microtask')); }); c.addEventListener('ping', (e) => e.preventDefault()); window.addEventListener('load', (e) => log.push('load ' + name(e.target) + ' ' + e.composedPath().length)); 1";
            assert_eq!(answer(&mut rt, setup), expected("1"));
            let c = node_by_name(&mut rt, "span");
            let mut ping = TrustedEvent::new("ping");
            ping.bubbles = true;
            ping.cancelable = true;
            assert_eq!(rt.dispatch_event(DispatchTarget::Node(c), &ping), Ok(false));
            ping.interface = EventInterface::PageTransition { persisted: true };
            ping.cancelable = false;
            assert_eq!(rt.dispatch_event(DispatchTarget::Window, &ping), Ok(true));
            let mut load = TrustedEvent::new("load");
            load.legacy_target_override = true;
            assert_eq!(rt.dispatch_event(DispatchTarget::Window, &load), Ok(true));
            assert_eq!(
                answer(&mut rt, "log"),
                expected(
                    // A checkpoint after each listener (HTML "clean up
                    // after running a callback", feature 5b).
                    r#"["span,2,true,span,false", "microtask", "document,3,true,span,false", "microtask", "window,3,true,span,false", "microtask", "window,2,true,window,true", "microtask", "load document 1"]"#
                ),
                "stress {stress}"
            );
        }
    });
}

#[test]
fn an_endless_listener_ends_at_the_time_limit() {
    on_big_stack(|| {
        let mut rt = load(TREE, false);
        rt.set_time_limit(std::time::Duration::from_millis(200));
        let setup = "window.addEventListener('spin', () => { for (;;) {} }); window.addEventListener('spin', () => { window.ran = true; }); 1";
        assert_eq!(answer(&mut rt, setup), expected("1"));
        let started = std::time::Instant::now();
        let result = rt.dispatch_event(DispatchTarget::Window, &TrustedEvent::new("spin"));
        assert!(
            matches!(result, Err(swb_web::ScriptError::Terminated(_))),
            "{result:?}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        // The next task runs, and the event's state was reset.
        assert_eq!(
            answer(&mut rt, "[window.ran, window.event]"),
            expected("[null, null]")
        );
    });
}

/// The first element named `name` in tree order. The wrapper has no API
/// for its node id, so the test counts the nodes in tree order, the order
/// in which the parser numbered them.
fn node_by_name(rt: &mut ScriptRuntime, name: &str) -> NodeId {
    let index = answer(
        rt,
        &format!(
            "(() => {{ let n = 0; const walk = (x) => {{ if (x.localName === '{name}') return n; n++; for (let c = x.firstChild; c; c = c.nextSibling) {{ const r = walk(c); if (r !== undefined) return r; }} }}; return walk(document); }})()"
        ),
    );
    let index = index.as_u64().unwrap() as usize;
    NodeId::from_index(index).unwrap()
}

#[test]
fn listeners_and_events_survive_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(TREE, stress);
            let setup = "window.log = []; c.addEventListener('k', function (e) { log.push('c ' + e.detail.v); }); const t = new EventTarget(); t.addEventListener('k', { handleEvent(e) { log.push('t ' + e.detail.v); } }); window.t = t; window.e = new CustomEvent('k', { detail: { v: 1 } }); 1";
            assert_eq!(answer(&mut rt, setup), expected("1"));
            rt.collect();
            let run = "c.dispatchEvent(e); t.dispatchEvent(new CustomEvent('k', { detail: { v: 2 } })); [log.join(), e.target === c, e.detail.v]";
            assert_eq!(answer(&mut rt, run), expected(r#"["c 1,t 2", true, 1]"#));
        }
    });
}

#[test]
fn nested_dispatches_end_with_a_range_error() {
    on_big_stack(|| {
        let mut rt = load(TREE, false);
        let source = "let depth = 0; const t = new EventTarget(); t.addEventListener('r', () => { depth++; t.dispatchEvent(new Event('r')); }); t.dispatchEvent(new Event('r')); depth > 10";
        assert_eq!(answer(&mut rt, source), expected("true"));
        let errors = errors(&rt);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with("Uncaught RangeError: "), "{errors:?}");
    });
}

#[test]
fn many_listeners_on_one_target() {
    on_big_stack(|| {
        let mut rt = load(TREE, false);
        let source = "let n = 0; const fs = []; for (let i = 0; i < 20000; i++) { const f = () => n++; fs.push(f); c.addEventListener('m', f); c.addEventListener('m', f); } c.dispatchEvent(new Event('m')); for (let i = 0; i < 20000; i += 2) c.removeEventListener('m', fs[i]); c.dispatchEvent(new Event('m')); n";
        let started = std::time::Instant::now();
        assert_eq!(answer(&mut rt, source), expected("30000"));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    });
}
