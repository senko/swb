// The cases of tools/probes/web-idle.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "idle-structure",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof requestIdleCallback, requestIdleCallback.length, cancelIdleCallback.length, requestIdleCallback.name, typeof IdleDeadline, Object.getOwnPropertyNames(IdleDeadline.prototype).sort().join(), Object.getOwnPropertyDescriptor(window, 'requestIdleCallback').enumerable, Object.getOwnPropertyDescriptor(window, 'cancelIdleCallback').writable]",
                r#"["function", 1, 1, "requestIdleCallback", "function", "constructor,didTimeout,timeRemaining", true, true]"#,
            ),
            (
                r"E(() => requestIdleCallback())",
                r#""TypeError: Failed to execute 'requestIdleCallback' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => requestIdleCallback(5))",
                r#""TypeError: Failed to execute 'requestIdleCallback' on 'Window': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => requestIdleCallback({}))",
                r#""TypeError: Failed to execute 'requestIdleCallback' on 'Window': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => requestIdleCallback(() => {}, 5))",
                r#""TypeError: Failed to execute 'requestIdleCallback' on 'Window': The provided value is not of type 'IdleRequestOptions'.""#,
            ),
            (
                r"E(() => typeof requestIdleCallback(() => {}, { timeout: -1 }))",
                r#""number""#,
            ),
            (
                r"E(() => typeof requestIdleCallback(() => {}, { timeout: 'x' }))",
                r#""number""#,
            ),
            (
                r"E(() => typeof requestIdleCallback(() => {}, null))",
                r#""number""#,
            ),
            (
                r"E(() => new IdleDeadline())",
                r#""TypeError: Failed to construct 'IdleDeadline': Illegal constructor""#,
            ),
            (
                r"E(() => cancelIdleCallback())",
                r#""TypeError: Failed to execute 'cancelIdleCallback' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (r"E(() => cancelIdleCallback(12345))", r#""undefined""#),
            (r"E(() => cancelIdleCallback('x'))", r#""undefined""#),
            (r"E(() => cancelIdleCallback(-1))", r#""undefined""#),
            (
                r"E(() => { const a = requestIdleCallback(() => {}); const b = requestIdleCallback(() => {}); cancelIdleCallback(a); cancelIdleCallback(b); return [typeof a, b - a, requestIdleCallback(() => {}) - b, a > 0]; })",
                r#"["number", 1, 1, true]"#,
            ),
            (
                r"E(() => requestIdleCallback.call({}, () => {}) > 0)",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(IdleDeadline.prototype, 'didTimeout').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
        ],
    },
    Case {
        name: "idle-run",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"(async () => { L.length = 0; requestIdleCallback((d) => { log(['idle', d.constructor.name, d.didTimeout, typeof d.timeRemaining(), d.timeRemaining() <= 50, d.timeRemaining() > 0, Object.getOwnPropertyNames(d).length, Object.prototype.toString.call(d)]); }); log('sync'); await wait(250); return JSON.stringify(L); })()",
                r#""[\"sync\",[\"idle\",\"IdleDeadline\",false,\"number\",true,true,0,\"[object IdleDeadline]\"]]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback(() => log('idle')); setTimeout(() => log('t0'), 0); Promise.resolve().then(() => log('micro')); await wait(250); return JSON.stringify(L); })()",
                r#""[\"micro\",\"t0\",\"idle\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const h = requestIdleCallback(() => log('idle')); cancelIdleCallback(h); await wait(250); return JSON.stringify(L); })()",
                r#""[]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback(() => log('a')); requestIdleCallback(() => log('b')); requestIdleCallback(() => { log('c'); requestIdleCallback(() => log('d')); }); await wait(300); return JSON.stringify(L); })()",
                r#""[\"a\",\"b\",\"c\",\"d\"]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback((d) => { log(['tr', d.timeRemaining() > 0, d.timeRemaining() <= 50, d.didTimeout]); }, { timeout: 20 }); await wait(250); return JSON.stringify(L); })()",
                r#""[[\"tr\",true,true,false]]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback(function () { log(['this', String(this === window), arguments.length]); }); await wait(250); return JSON.stringify(L); })()",
                r#""[[\"this\",\"true\",1]]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback(function () { 'use strict'; log(['strict this', String(this)]); }); await wait(250); return JSON.stringify(L); })()",
                r#""[[\"strict this\",\"undefined\"]]""#,
            ),
            (
                r"(async () => { L.length = 0; const errors = []; const h = (e) => errors.push(e.message); addEventListener('error', h); requestIdleCallback(() => { throw new Error('boom'); }); requestIdleCallback(() => log('after')); await wait(250); removeEventListener('error', h); return JSON.stringify([errors, L]); })()",
                r#""[[\"Uncaught Error: boom\"],[\"after\"]]""#,
            ),
            (
                r"(async () => { L.length = 0; const hs = []; for (let i = 0; i < 3; i++) hs.push(requestIdleCallback(() => log('cb' + i))); cancelIdleCallback(hs[1]); await wait(250); return JSON.stringify(L); })()",
                r#""[\"cb0\",\"cb2\"]""#,
            ),
            (
                r"(async () => { L.length = 0; const hs = []; hs.push(requestIdleCallback(() => { log('first'); cancelIdleCallback(hs[1]); })); hs.push(requestIdleCallback(() => log('second'))); await wait(250); return JSON.stringify(L); })()",
                r#""[\"first\"]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback((d) => { let n = 0; while (d.timeRemaining() > 0 && n < 1e7) n++; log(['spin', n < 1e7]); }); await wait(300); return JSON.stringify(L); })()",
                r#""[[\"spin\",true]]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback((d) => { const a = d.timeRemaining(); const b = d.timeRemaining(); log([a >= b, a <= 50, b >= 0]); }); await wait(250); return JSON.stringify(L); })()",
                r#""[[true,true,true]]""#,
            ),
            (
                r"(async () => { L.length = 0; let n = 0; let stop = false; function f() { n++; if (!stop) requestIdleCallback(f); } f(); await wait(1000); stop = true; return [n > 5, n <= 100]; })()",
                r"[true, true]",
            ),
            (
                r"(async () => { L.length = 0; let n = 0; function f(d) { n++; log(Math.round(d.timeRemaining())); if (n < 4) requestIdleCallback(f); } requestIdleCallback(f); await wait(500); return JSON.stringify(L.map((x) => (x > 40 ? 'long' : x > 0 ? 'short' : 'zero'))); })()",
                r#""[\"long\",\"long\",\"long\",\"long\"]""#,
            ),
            (
                r"(async () => { L.length = 0; let f = 0; let stop = false; function loop() { f++; if (!stop) requestAnimationFrame(loop); } loop(); let ic = 0; function idle(d) { ic++; log(Math.round(d.timeRemaining())); if (!stop) requestIdleCallback(idle); } requestIdleCallback(idle); await wait(500); stop = true; await wait(100); return [f > 20, ic > 5, L.length > 5, L.every((x) => x >= 0 && x <= 50)]; })()",
                r"[true, true, true, true]",
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback((d) => { log(['to', d.didTimeout]); }, { timeout: 5000 }); await wait(250); return JSON.stringify(L); })()",
                r#""[[\"to\",false]]""#,
            ),
            (
                r"(async () => { L.length = 0; requestIdleCallback((d) => { log(['t0', d.didTimeout]); }, { timeout: 0 }); await wait(250); return JSON.stringify(L); })()",
                r#""[[\"t0\",false]]""#,
            ),
        ],
    },
];
