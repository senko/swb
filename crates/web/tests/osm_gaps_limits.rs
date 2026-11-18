//! The hostile cases of M12 feature 2 (docs/testing.md, "Hostile-page set"):
//! `ImageData` of huge sizes, thousands of `Worker` stand-ins that get large
//! messages, and an element with thousands of attributes read through
//! `Attr` as a `Node`. Each must end with an exception or a value, without a
//! panic, in bounded time and with a bounded heap. One test checks
//! `getContext` and the event of a failed WebGL context.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::{Duration, Instant};

use common::{answer, load, on_big_stack};
use serde_json::json;

#[test]
fn huge_image_data_fails_without_allocating() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let sizes = answer(
            &mut rt,
            "(() => { const out = []; \
             for (const [w, h] of [[50000, 50000], [1e6, 1e6], [2 ** 31, 1], [2 ** 30, 1], \
             [2 ** 29, 1], [30000, 30000], [16384, 16384], [9000, 9000], [0, 5], [5, 0], \
             [NaN, 5], [-1, 5]]) { \
             try { new ImageData(w, h); out.push('ok'); } catch (e) { out.push(e.name); } } \
             return out; })()",
        );
        assert_eq!(
            sizes,
            json!([
                "IndexSizeError",
                "IndexSizeError",
                "IndexSizeError",
                "IndexSizeError",
                "RangeError",
                "RangeError",
                "RangeError",
                "RangeError",
                "IndexSizeError",
                "IndexSizeError",
                "IndexSizeError",
                "IndexSizeError"
            ])
        );
        // A loop of failing allocations does not grow the heap, and a
        // small image still works afterwards.
        let started = Instant::now();
        let after = answer(
            &mut rt,
            "(() => { for (let i = 0; i < 500; i++) { try { new ImageData(16384, 16384); } catch (e) {} } \
             const small = new ImageData(256, 256); return [small.data.length, small.width]; })()",
        );
        assert_eq!(after, json!([262_144, 256]));
        assert!(started.elapsed() < Duration::from_secs(10));
        rt.collect();
        assert!(rt.heap_size() < (64 << 20), "{} bytes", rt.heap_size());
    });
}

#[test]
fn image_data_with_a_data_array_checks_the_array() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let out = answer(
            &mut rt,
            "(() => { const e = (f) => { try { return f(); } catch (x) { return x.name; } }; \
             return [e(() => new ImageData(new Uint8ClampedArray(0), 1)), \
             e(() => new ImageData(new Uint8ClampedArray(3), 1)), \
             e(() => new ImageData(new Uint8ClampedArray(12), 5)), \
             e(() => new ImageData(new Uint8ClampedArray(8), 2, 3)), \
             e(() => new ImageData(new Uint8ClampedArray(8), 0)), \
             new ImageData(new Uint8ClampedArray(12), 3).height]; })()",
        );
        assert_eq!(
            out,
            json!([
                "InvalidStateError",
                "InvalidStateError",
                "IndexSizeError",
                "IndexSizeError",
                "IndexSizeError",
                1
            ])
        );
    });
}

/// The length of the data array comes from the array itself, not from a
/// `length` property that the page defines (review of feature 2).
#[test]
fn image_data_ignores_a_redefined_length() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let out = answer(
            &mut rt,
            "(() => { const u = new Uint8ClampedArray(8); \
             Object.defineProperty(u, 'length', {value: 2 ** 40}); \
             let big; try { new ImageData(u, 2 ** 19, 2 ** 19); big = 'ok'; } catch (e) { big = e.name; } \
             const d = new ImageData(u, 2); \
             const r = new ArrayBuffer(8, {maxByteLength: 16}); \
             const t = new ImageData(new Uint8ClampedArray(r, 4), 1); \
             return [big, d.width, d.height, t.height]; })()",
        );
        assert_eq!(out, json!(["IndexSizeError", 2, 1, 1]));
    });
}

#[test]
fn thousands_of_workers_with_large_messages_stay_bounded() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let started = Instant::now();
        let made = answer(
            &mut rt,
            "(() => { const big = {a: Array.from({length: 300}, (_, i) => ({i, s: 'x'.repeat(40)}))}; \
             const keep = []; \
             for (let i = 0; i < 10000; i++) { \
             const w = new Worker('data:text/javascript,1'); w.postMessage(big); \
             if (i % 100 === 0) keep.push(w); } \
             return keep.length; })()",
        );
        assert_eq!(made, json!(100));
        assert!(started.elapsed() < Duration::from_secs(30));
        // The messages are dropped when they are made: nothing grows.
        rt.collect();
        assert!(rt.heap_size() < (64 << 20), "{} bytes", rt.heap_size());
    });
}

#[test]
fn a_worker_takes_the_transfer_list_and_drops_the_message() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let out = answer(
            &mut rt,
            "(() => { const w = new Worker('data:text/javascript,1'); \
             const e = (f) => { try { f(); return 'ok'; } catch (x) { return x.name; } }; \
             const b = new ArrayBuffer(16); \
             return [e(() => w.postMessage(() => 1)), e(() => w.postMessage(b, [b])), b.byteLength, \
             e(() => w.postMessage(b, [b])), e(() => w.terminate()), e(() => w.postMessage(1)), \
             w instanceof EventTarget, typeof SharedWorker]; })()",
        );
        assert_eq!(
            out,
            json!([
                "DataCloneError",
                "ok",
                0,
                "DataCloneError",
                "ok",
                "ok",
                true,
                "undefined"
            ])
        );
    });
}

/// `getContext` is null for every type; a WebGL type fires the creation
/// error at the canvas first, which `MapLibre` turns into the JSON message that
/// OpenStreetMap's code parses.
#[test]
fn get_context_is_null_and_webgl_fires_the_creation_error() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let out = answer(
            &mut rt,
            "(() => { const c = document.createElement('canvas'); const seen = []; \
             c.addEventListener('webglcontextcreationerror', (e) => \
             seen.push([e.type, e.cancelable, e.bubbles, e.isTrusted])); \
             const r = ['webgl2', 'webgl', 'experimental-webgl', '2d', 'bitmaprenderer', 'x'] \
             .map((t) => c.getContext(t, {alpha: false})); \
             let noarg; try { c.getContext(); } catch (e) { noarg = e.name; } \
             return [r, seen, noarg]; })()",
        );
        let event = json!(["webglcontextcreationerror", true, false, true]);
        assert_eq!(
            out,
            json!([
                [null, null, null, null, null, null],
                [event, event, event],
                "TypeError"
            ])
        );
    });
}

#[test]
fn an_element_with_many_attributes_reads_as_nodes() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let started = Instant::now();
        let out = answer(
            &mut rt,
            "(() => { const el = document.createElement('div'); \
             for (let i = 0; i < 20000; i++) el.setAttribute('a' + i, String(i)); \
             const all = [].concat(...el.attributes); \
             let sum = 0; for (const a of all) { if (a.nodeType === 2 && a.nodeName === 'a' + a.nodeValue) sum++; } \
             return [all.length, sum]; })()",
        );
        assert_eq!(out, json!([20000, 20000]));
        assert!(started.elapsed() < Duration::from_secs(20));
    });
}

/// The `Node` operations of an `Attr` check their argument count as for
/// any node (review of feature 2).
#[test]
fn attr_node_operations_require_their_arguments() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><p>x", false);
        let out = answer(
            &mut rt,
            "(() => { const a = document.createAttribute('x'); \
             return ['contains', 'isEqualNode', 'isSameNode', 'lookupPrefix', 'appendChild', \
             'removeChild', 'compareDocumentPosition', 'insertBefore'].map((m) => { \
             try { a[m](); return 'ok'; } catch (e) { return e.message; } }) \
             .concat([a.cloneNode().nodeName, a.hasChildNodes()]); })()",
        );
        let one = "1 argument required, but only 0 present.";
        let two = "2 arguments required, but only 0 present.";
        let prefixed =
            |member: &str, text: &str| format!("Failed to execute '{member}' on 'Node': {text}");
        assert_eq!(
            out,
            json!([
                prefixed("contains", one),
                prefixed("isEqualNode", one),
                prefixed("isSameNode", one),
                prefixed("lookupPrefix", one),
                prefixed("appendChild", one),
                prefixed("removeChild", one),
                prefixed("compareDocumentPosition", one),
                prefixed("insertBefore", two),
                "x",
                false
            ])
        );
    });
}

/// `compareDocumentPosition` of two attributes: by their elements in one
/// tree, by the attribute list on one element, and a consistent order
/// when they are disconnected (DOM, the `attr1`/`attr2` steps; review of
/// feature 2).
#[test]
fn attributes_compare_by_their_elements() {
    on_big_stack(|| {
        let mut rt = load(
            "<!DOCTYPE html><div id=d a=1><p id=p z=2></p></div><span id=s b=3></span>",
            false,
        );
        let out = answer(
            &mut rt,
            "(() => { const at = (id, n) => document.getElementById(id).getAttributeNode(n); \
             const [da, dId, pz, sb] = [at('d', 'a'), at('d', 'id'), at('p', 'z'), at('s', 'b')]; \
             const x = document.createAttribute('x'), y = document.createAttribute('y'); \
             const c = (m, o) => m.compareDocumentPosition(o); \
             const xy = c(x, y), yx = c(y, x); \
             return [c(da, pz), c(pz, da), c(da, sb), c(sb, da), c(pz, sb), \
             c(dId, da) & 0x06, c(da, dId) & 0x06, \
             (xy & 0x21) === 0x21 && (yx & 0x21) === 0x21 && (xy & 0x06) !== (yx & 0x06) \
             && (xy & 0x06) !== 0 && (yx & 0x06) !== 0]; })()",
        );
        // `id` comes before `a` in the attribute list of the `div`.
        assert_eq!(out, json!([4, 2, 4, 2, 4, 4, 2, true]));
    });
}
