//! Lazy interfaces (ADR 0028 section 5, "Lazy interfaces notes"): only
//! `Window` and its ancestors are created with the runtime; every other
//! interface at its first use. Scripts cannot tell: the global object has
//! the same own properties in the same order with the same attributes as
//! with the eager creation (`lazy_interfaces/globals.txt`, recorded from
//! the build before the change, a543e9d; regenerate it when the IDL files
//! change), each interface has one interface object whatever path creates
//! it, and a change of a global property before the first use acts on the
//! property only.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use common::{answer, load, on_big_stack};

/// The page of the tests (no script, so the global object has only the
/// properties of the runtime).
const PAGE: &str = "<!doctype html><title>x</title><body><p>x</p>";

/// The own keys of the global object with their attributes (`d` data, `a`
/// accessor, `w` writable, `e` enumerable, `c` configurable), one per line.
const KEYS: &str = "Reflect.ownKeys(window).map(k => typeof k === 'symbol' ? k.toString() : (d => k + ':' + ('value' in d ? 'd' + (d.writable ? 'w' : '') : 'a') + (d.enumerable ? 'e' : '') + (d.configurable ? 'c' : ''))(Object.getOwnPropertyDescriptor(window, k))).join('\\n')";

/// Runs `queries` in one runtime per mode (with and without the GC stress
/// mode) and returns the differences from the expected values.
fn failures(queries: &'static [(&'static str, &'static str)]) -> Vec<String> {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for stress in [false, true] {
            let mut rt = load(PAGE, stress);
            for &(query, want) in queries {
                let got = answer(&mut rt, query);
                if got != common::expected(want) {
                    failures.push(format!(
                        "(stress {stress}) {query}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        failures
    })
}

/// Runs each query in a new runtime (the first use decides the order of
/// creation).
fn fresh_failures(queries: &'static [(&'static str, &'static str)]) -> Vec<String> {
    let mut all = Vec::new();
    for query in queries {
        all.extend(failures(std::slice::from_ref(query)));
    }
    all
}

#[test]
fn the_global_object_has_the_keys_of_the_eager_creation() {
    on_big_stack(|| {
        let base = include_str!("lazy_interfaces/globals.txt").trim_end();
        let mut rt = load(PAGE, false);
        // The names alone create nothing.
        let before = rt.heap_size();
        let names = answer(&mut rt, "Object.getOwnPropertyNames(window).join('\\n')");
        let grown = rt.heap_size().saturating_sub(before);
        assert!(
            grown < 100_000,
            "listing the names grew the heap by {grown} bytes"
        );
        let names = names.as_str().unwrap().to_owned();
        let base_names: Vec<&str> = base
            .lines()
            .map(|line| line.rsplit_once(':').map_or(line, |(name, _)| name))
            .collect();
        assert_eq!(names, base_names.join("\n"));
        // The attributes (this reads every value, so every interface is
        // created; the order stays).
        let keys = answer(&mut rt, KEYS);
        assert_eq!(keys.as_str().unwrap(), base);
    });
}

#[test]
fn the_runtime_starts_small() {
    on_big_stack(|| {
        let config = swb_js::RuntimeConfig::default();
        let clock = swb_web::PageClock::new(swb_web::ClockMode::Virtual, None);
        let rt = swb_web::ScriptRuntime::with_config(config, clock).unwrap();
        // The goal of the lazy creation: about 500 KB (1,197,385 bytes
        // before it).
        let size = rt.heap_size();
        assert!(size < 520_000, "a new runtime has {size} bytes of heap");
    });
}

#[test]
fn one_interface_object_per_class_whatever_path_comes_first() {
    let failures = fresh_failures(&[
        ("document.body.constructor === HTMLBodyElement", "true"),
        (
            "var C = HTMLBodyElement; [document.body.constructor === C, document.body instanceof C, document.body instanceof HTMLElement].join()",
            "\"true,true,true\"",
        ),
        (
            "var e = document.createElement('div'); [e.constructor === HTMLDivElement, Object.getPrototypeOf(HTMLDivElement) === HTMLElement, Object.getPrototypeOf(HTMLElement.prototype) === Element.prototype].join()",
            "\"true,true,true\"",
        ),
        (
            "class X extends HTMLElement {} [Object.getPrototypeOf(X) === HTMLElement, X.prototype instanceof Node].join()",
            "\"true,true\"",
        ),
        (
            "var i = new Image(); [i instanceof HTMLImageElement, Image.prototype === HTMLImageElement.prototype, i.constructor === HTMLImageElement].join()",
            "\"true,true,true\"",
        ),
        (
            "var o = new Option('a', 'b'); [o.constructor === HTMLOptionElement, Option.prototype === HTMLOptionElement.prototype].join()",
            "\"true,true\"",
        ),
        (
            "try { document.createElement('1') } catch (e) { [e instanceof DOMException, e.constructor === DOMException, e.name, e instanceof Error].join() }",
            "\"true,true,InvalidCharacterError,true\"",
        ),
        (
            "var ev = new CustomEvent('x'); [ev instanceof Event, Object.getPrototypeOf(CustomEvent) === Event, typeof ev.isTrusted].join()",
            "\"true,true,boolean\"",
        ),
        (
            "var l = document.querySelectorAll('p'); [l instanceof NodeList, typeof l.forEach, l[Symbol.iterator] === [][Symbol.iterator]].join()",
            "\"true,function,true\"",
        ),
        (
            "[typeof getComputedStyle(document.body), getComputedStyle(document.body) instanceof CSSStyleDeclaration].join()",
            "\"object,true\"",
        ),
        (
            "Object.prototype.toString.call(document) + ' ' + (document instanceof HTMLDocument) + ' ' + (document instanceof Document)",
            "\"[object HTMLDocument] true true\"",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn changes_before_the_first_use_act_on_the_property_only() {
    let failures = fresh_failures(&[
        (
            "delete window.Node; var p = Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(document.body)))); [typeof Node, 'Node' in window, p.constructor.name, p === Object.getPrototypeOf(Element.prototype), document.body instanceof EventTarget].join()",
            "\"undefined,false,Node,true,true\"",
        ),
        (
            "window.Node = 1; var N = Object.getPrototypeOf(Element); [Node, N.name, N.prototype === Object.getPrototypeOf(Element.prototype), Object.getOwnPropertyDescriptor(window, 'Node').value].join()",
            "\"1,Node,true,1\"",
        ),
        (
            "Object.defineProperty(window, 'HTMLElement', { value: 5 }); var d = document.createElement('div'); [HTMLElement, d instanceof HTMLDivElement, Object.getPrototypeOf(HTMLDivElement).name].join()",
            "\"5,true,HTMLElement\"",
        ),
        (
            "Object.defineProperty(window, 'Text', { enumerable: true }); [Object.keys(window).includes('Text'), typeof Text, document.createTextNode('a') instanceof Text].join()",
            "\"true,function,true\"",
        ),
        (
            "Image = null; var i = document.createElement('img'); [Image, i instanceof HTMLImageElement].join()",
            "\",true\"",
        ),
        // The built-in objects that interfaces use are the realm's own,
        // even when a script replaced the global ones first.
        (
            "window.Error = function () {}; var e = new DOMException('m'); [Object.getPrototypeOf(DOMException.prototype) === Object.getPrototypeOf(TypeError.prototype), e instanceof Object.getPrototypeOf(TypeError)].join()",
            "\"true,true\"",
        ),
        (
            "var v = Array.prototype.values, f = Array.prototype.forEach; Array.prototype.values = function () {}; Array.prototype.forEach = null; [NodeList.prototype.values === v, NodeList.prototype.forEach === f, NodeList.prototype[Symbol.iterator] === v].join()",
            "\"true,true,true\"",
        ),
        (
            "let HTMLDivElement = 3; [HTMLDivElement, typeof window.HTMLDivElement, document.createElement('div').constructor === window.HTMLDivElement].join()",
            "\"3,function,true\"",
        ),
        (
            "function Comment() { return 1; } [Comment(), document.createComment('x') instanceof Comment].join()",
            "\"1,false\"",
        ),
        (
            "Object.freeze(window); [Object.isFrozen(window), typeof HTMLSpanElement, Object.getOwnPropertyDescriptor(window, 'HTMLSpanElement').writable].join()",
            "\"true,function,false\"",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_name_reads_as_an_interface_object() {
    // Each global name of an interface reads as a function whose `name` is
    // the name, through the identifier, `window.X` and `globalThis[X]`, in
    // a loop that the caches serve after the first round.
    let failures = failures(&[(
        "var names = Object.getOwnPropertyNames(window).filter(n => /^[A-Z]/.test(n) && typeof window[n] === 'function' && window[n].prototype && n !== 'Image' && n !== 'Option' && n !== 'WebKitCSSMatrix'); var bad = []; for (var round = 0; round < 3; round++) for (var n of names) { var v = new Function('return ' + n)(); if (v !== window[n] || v !== globalThis[n] || v.name !== n) bad.push(n); } [names.length > 150, bad.join()].join()",
        "\"true,\"",
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_heap_limit_during_a_creation_ends_the_task() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(PAGE, stress);
            rt.set_heap_limit(rt.heap_size() + 8192);
            // HTMLElement alone needs more than the room left.
            let result = answer(&mut rt, "typeof HTMLElement");
            let text = result.as_str().unwrap_or_default().to_owned();
            assert!(text.contains("heap"), "{text}");
            assert!(rt.failed(), "the runtime is marked as failed");
            assert!(rt.stopped());
        }
    });
}
