// The cases of `tools/probes/web-event-loop.json` with the values that
// Chromium 148 gave (`just probe --json tools/probes/web-event-loop.json`);
// where swb departs on purpose, `tests/event_loop.rs` overrides the value
// (SWB_VALUES).

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "order",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script>",
        queries: &[
            (
                r"(async () => { const log = []; setTimeout(() => log.push('t0'), 0); setTimeout(() => log.push('t1'), 1); setTimeout(() => log.push('t0b'), 0); Promise.resolve().then(() => log.push('p1')); queueMicrotask(() => log.push('m1')); Promise.resolve().then(() => log.push('p2')); log.push('sync'); await wait(60); return log.join(' '); })()",
                r#""sync p1 m1 p2 t0 t0b t1""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => { log.push('a'); Promise.resolve().then(() => log.push('a-p')); queueMicrotask(() => log.push('a-m')); }, 0); setTimeout(() => log.push('b'), 0); await wait(60); return log.join(' '); })()",
                r#""a a-p a-m b""#,
            ),
            (
                r"(async () => { const log = []; queueMicrotask(() => setTimeout(() => log.push('from-m'), 0)); setTimeout(() => { log.push('t'); setTimeout(() => log.push('from-t'), 0); }, 0); setTimeout(() => log.push('t2'), 0); await wait(60); return log.join(' '); })()",
                r#""t t2 from-m from-t""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => log.push('20'), 20); setTimeout(() => log.push('5'), 5); setTimeout(() => log.push('10'), 10); setTimeout(() => log.push('5b'), 5); await wait(60); return log.join(' '); })()",
                r#""5 5b 10 20""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => log.push('1'), 1); setTimeout(() => log.push('0'), 0); await wait(60); return log.join(' '); })()",
                r#""0 1""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => log.push('2'), 2); setTimeout(() => log.push('1'), 1); setTimeout(() => log.push('0'), 0); setTimeout(() => log.push('-5'), -5); await wait(60); return log.join(' '); })()",
                r#""0 -5 1 2""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => log.push('big'), 2 ** 32 + 30); setTimeout(() => log.push('nan'), NaN); setTimeout(() => log.push('str'), '15'); setTimeout(() => log.push('ten'), 10); await wait(60); return log.join(' '); })()",
                r#""nan ten str big""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => log.push('t'), 0); let n = 0; function step() { if (++n < 1000) queueMicrotask(step); else log.push('chain ' + n); } queueMicrotask(step); await wait(60); return log.join(' '); })()",
                r#""chain 1000 t""#,
            ),
            (
                r"(async () => { const log = []; let n = 0; const id = setInterval(() => { log.push('i' + n); if (++n === 3) clearInterval(id); }, 5); setTimeout(() => log.push('t12'), 12); await wait(80); return log.join(' '); })()",
                r#""i0 i1 t12 i2""#,
            ),
        ],
    },
    Case {
        name: "nesting",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script>",
        queries: &[
            (
                r"(async () => { const runs = []; for (let k = 0; k < 3; k++) { const t = []; await new Promise((r) => { function f() { t.push(performance.now()); if (t.length < 10) setTimeout(f, 0); else r(); } setTimeout(f, 0); }); const gaps = []; for (let i = 1; i < t.length; i++) gaps.push(t[i] - t[i - 1] >= 3.5 ? 1 : 0); runs.push(gaps.join('')); } return runs; })()",
                r#"["000001111", "111111111", "111111111"]"#,
            ),
            (
                r"(async () => { const t = []; await new Promise((r) => { const id = setInterval(() => { t.push(performance.now()); if (t.length === 10) { clearInterval(id); r(); } }, 0); }); const gaps = []; for (let i = 1; i < t.length; i++) gaps.push(t[i] - t[i - 1] >= 3.5 ? 1 : 0); return gaps.join(''); })()",
                r#""000001111""#,
            ),
            (
                r"(async () => { const t = []; await new Promise((r) => { function f() { t.push(performance.now()); if (t.length < 10) setTimeout(f, 2); else r(); } setTimeout(f, 2); }); const gaps = []; for (let i = 1; i < t.length; i++) gaps.push(t[i] - t[i - 1] >= 3.5 ? 1 : 0); return gaps.join(''); })()",
                r#""000001111""#,
            ),
            (
                r"(async () => { const t = []; await new Promise((r) => { function f() { t.push(performance.now()); if (t.length < 10) queueMicrotask(() => setTimeout(f, 0)); else r(); } setTimeout(f, 0); }); const gaps = []; for (let i = 1; i < t.length; i++) gaps.push(t[i] - t[i - 1] >= 3.5 ? 1 : 0); return gaps.join(''); })()",
                r#""000001111""#,
            ),
        ],
    },
    Case {
        name: "arguments",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script>",
        queries: &[
            (
                r#"(async () => { const log = []; window.slog = log; setTimeout("slog.push('string')", 0); setTimeout((a, b, c) => log.push([a, b, c].join('+')), 0, 'x', 2); setTimeout(function () { log.push(this === window, arguments.length); }, 0); setTimeout(function () { 'use strict'; log.push(this === window); }, 0); const iv = setInterval(function () { log.push('int-args ' + arguments.length); clearInterval(iv); }, 0, 1, 2, 3); setTimeout({ toString() { return "slog.push('object')"; } }, 0); setTimeout(5, 0); setTimeout(undefined, 0); setTimeout(null, 0); await wait(60); return log.join(' '); })()"#,
                r#""string x+2+ true 0 true int-args 3 object""#,
            ),
            (
                r#"(async () => { const log = []; window.slog = log; setTimeout("var declaredByTimer = 7; slog.push(typeof declaredByTimer, this === window)", 0); await wait(60); return log.join(' '); })()"#,
                r#""number true""#,
            ),
            (
                r#"(async () => { const log = []; window.slog = log; setTimeout("slog.push(typeof log)", 0); await wait(60); return log.join(' '); })()"#,
                r#""undefined""#,
            ),
            (
                r"(() => { const r = []; for (const f of [() => setTimeout(), () => setInterval(), () => queueMicrotask(), () => queueMicrotask(5), () => queueMicrotask('x'), () => clearTimeout(), () => clearInterval('a'), () => setTimeout({ toString() { throw new Error('ts'); } })]) { try { r.push(String(f())); } catch (e) { r.push(e.name + ': ' + e.message); } } return r; })()",
                r#"["TypeError: Failed to execute 'setTimeout' on 'Window': 1 argument required, but only 0 present.", "TypeError: Failed to execute 'setInterval' on 'Window': 1 argument required, but only 0 present.", "TypeError: Failed to execute 'queueMicrotask' on 'Window': 1 argument required, but only 0 present.", "TypeError: Failed to execute 'queueMicrotask' on 'Window': parameter 1 is not of type 'Function'.", "TypeError: Failed to execute 'queueMicrotask' on 'Window': parameter 1 is not of type 'Function'.", "undefined", "undefined", "Error: ts"]"#,
            ),
            (
                r"[setTimeout.length, setInterval.length, clearTimeout.length, clearInterval.length, queueMicrotask.length, setTimeout.name, typeof window.reportError, reportError.length]",
                r#"[1, 1, 0, 0, 1, "setTimeout", "function", 1]"#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(window, 'setTimeout'); return [d.writable, d.enumerable, d.configurable, typeof d.value, Object.getOwnPropertyNames(window).includes('queueMicrotask')]; })()",
                r#"[true, true, true, "function", true]"#,
            ),
        ],
    },
    Case {
        name: "ids",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script>",
        queries: &[
            (
                r"(() => { const a = setTimeout(() => {}, 1000); const b = setInterval(() => {}, 1000); const c = setTimeout(() => {}, 1000); clearTimeout(a); clearTimeout(b); clearInterval(c); const d = setTimeout(() => {}, 1000); clearTimeout(d); return [a > 0, Number.isInteger(a), b - a, c - b, d - c]; })()",
                r"[true, true, 1, 1, 1]",
            ),
            (
                r"(async () => { const log = []; let b; setTimeout(() => { log.push('a'); clearInterval(b); }, 0); b = setTimeout(() => log.push('b'), 0); clearTimeout(123456); clearTimeout(-1); clearTimeout('x'); setTimeout(() => log.push('c'), 0); await wait(60); return log.join(' '); })()",
                r#""a c""#,
            ),
            (
                r"(async () => { const log = []; const id = setInterval(() => { log.push('once'); clearInterval(id); }, 0); await wait(60); return log.join(' '); })()",
                r#""once""#,
            ),
        ],
    },
    Case {
        name: "errors",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script><script>window.inlineMark = E.length; throw new TypeError('inline');</script><script>window.inlineAfter = E.slice(inlineMark);</script>",
        queries: &[
            (
                r"window.inlineAfter",
                r#"[["Uncaught TypeError: inline", "TypeError", true, 1, 756]]"#,
            ),
            (
                r"(async () => { const n = E.length; setTimeout(() => { throw new RangeError('in timer'); }, 0); await wait(30); return E.slice(n); })()",
                r#"[["Uncaught RangeError: in timer", "RangeError", false, 1, 61]]"#,
            ),
            (
                r"(async () => { const n = E.length; setTimeout(() => { throw 'a string'; }, 0); setTimeout(() => { throw { x: 1 }; }, 0); setTimeout(() => { throw null; }, 0); await wait(30); return E.slice(n); })()",
                r#"[["Uncaught a string", "string", false, 1, 55], ["Uncaught [object Object]", "Object", false, 1, 99], ["Uncaught null", "null", false, 1, 141]]"#,
            ),
            (
                r#"(async () => { const n = E.length; setTimeout('}', 0); setTimeout('throw new Error("str")', 0); await wait(30); return E.slice(n).map((x) => [x[1], x[3] > 0, x[4] > 0]); })()"#,
                r#"[["SyntaxError", true, true], ["Error", true, true]]"#,
            ),
            (
                r"(async () => { const n = E.length; queueMicrotask(() => { throw new Error('in microtask'); }); queueMicrotask(() => E.push('next microtask')); await wait(30); return E.slice(n).map((x) => Array.isArray(x) ? x.slice(0, 2) : x); })()",
                r#"[["Uncaught Error: in microtask", "Error"], "next microtask"]"#,
            ),
            (
                r"(() => { const n = E.length; const log = []; p.addEventListener('x1', () => { throw new Error('in listener'); }); p.addEventListener('x1', () => log.push('second')); window.addEventListener('error', () => log.push('error event'), { once: true }); p.dispatchEvent(new Event('x1')); log.push('returned'); return [log.join(' '), E.slice(n).map((x) => x[0])]; })()",
                r#"["error event second returned", ["Uncaught Error: in listener"]]"#,
            ),
            (
                r"(() => { const n = E.length; reportError(new SyntaxError('reported')); reportError(5); return E.slice(n); })()",
                r#"[["Uncaught SyntaxError: reported", "SyntaxError", false, 1, 42], ["Uncaught 5", "number", false, 1, 72]]"#,
            ),
            (
                r"(async () => { let r; window.addEventListener('error', (e) => { r = [e instanceof ErrorEvent, e.type, e.cancelable, e.bubbles, e.isTrusted, e.eventPhase, e.target === window, e.defaultPrevented]; }, { once: true }); setTimeout(() => { throw new Error('event'); }, 0); await wait(30); return r; })()",
                r#"[true, "error", true, false, true, 2, true, false]"#,
            ),
            (
                r"(async () => { const n = E.length; const f = () => { throw new Error('in error listener'); }; window.addEventListener('error', f); setTimeout(() => { throw new Error('first'); }, 0); await wait(30); window.removeEventListener('error', f); return E.slice(n).map((x) => x[0]); })()",
                r#"["Uncaught Error: first"]"#,
            ),
            (
                r"(async () => { const n = E.length; window.cancelErrors = true; let seen; window.addEventListener('error', (e) => { seen = e.defaultPrevented; }, { once: true }); setTimeout(() => { throw new Error('canceled'); }, 0); await wait(30); window.cancelErrors = false; return [seen, E.length - n]; })()",
                r"[true, 1]",
            ),
        ],
    },
    Case {
        name: "rejections",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script><script>Promise.reject(new Error('inline rejection'));</script>",
        queries: &[
            (
                r"(async () => { await wait(20); return U.filter((x) => x[0] === 'inline rejection'); })()",
                r#"[["inline rejection", true, true, true]]"#,
            ),
            (
                r"(async () => { const n = U.length; Promise.reject(new Error('r1')); Promise.reject(7); const p = Promise.reject(new Error('handled')); p.catch(() => {}); await wait(30); return U.slice(n); })()",
                r#"[["r1", true, true, true], ["7", true, true, true]]"#,
            ),
            (
                r"(async () => { const n = U.length; const log = []; const p = Promise.reject(new Error('late')); queueMicrotask(() => queueMicrotask(() => p.catch(() => log.push('caught')))); await wait(30); return [log.join(), U.length - n]; })()",
                r#"["caught", 0]"#,
            ),
            (
                r"(async () => { const n = U.length; const log = []; window.addEventListener('unhandledrejection', () => log.push('event'), { once: true }); setTimeout(() => log.push('timer before'), 0); const p = Promise.reject(new Error('timer')); setTimeout(() => { log.push('timer after'); p.catch(() => log.push('caught')); }, 0); await wait(30); return [log.join(' '), U.slice(n).map((x) => x[0])]; })()",
                r#"["timer before timer after caught", []]"#,
            ),
            (
                r"(async () => { const log = []; window.addEventListener('unhandledrejection', () => log.push('event'), { once: true }); setTimeout(() => { Promise.reject(1); queueMicrotask(() => log.push('microtask')); log.push('task'); }, 0); await wait(30); return log.join(' '); })()",
                r#""task microtask event""#,
            ),
            (
                r"(async () => { const n = U.length; let k = 0; const f = () => { if (k++ < 1) Promise.reject(new Error('nested')); }; window.addEventListener('unhandledrejection', f); Promise.reject(new Error('outer')); await wait(40); window.removeEventListener('unhandledrejection', f); return U.slice(n).map((x) => x[0]); })()",
                r#"["outer", "nested"]"#,
            ),
        ],
    },
    Case {
        name: "callback-checkpoint",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script>",
        queries: &[
            (
                r"(async () => { const log = []; const a = () => { log.push('l1'); queueMicrotask(() => log.push('m1')); }; const b = () => { log.push('l2'); queueMicrotask(() => log.push('m2')); }; window.addEventListener('error', a); window.addEventListener('error', b); setTimeout(() => { throw new Error('x'); }, 0); await wait(30); window.removeEventListener('error', a); window.removeEventListener('error', b); return log.join(' '); })()",
                r#""l1 l2 m1 m2""#,
            ),
            (
                r"(() => { const log = []; p.addEventListener('c1', () => { log.push('l1'); queueMicrotask(() => log.push('m1')); }); p.addEventListener('c1', () => log.push('l2')); p.dispatchEvent(new Event('c1')); log.push('after'); return log.join(' '); })()",
                r#""l1 l2 after""#,
            ),
            (
                r"(async () => { const log = []; setTimeout(() => { log.push('t1'); queueMicrotask(() => log.push('m1')); }, 5); setTimeout(() => { log.push('t2'); }, 5); await wait(60); return log.join(' '); })()",
                r#""t1 m1 t2""#,
            ),
        ],
    },
    Case {
        name: "clock",
        html: r"<!DOCTYPE html><html><head></head><body><p id=p>t</p><script>function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }const E = []; const U = [];function info(e) { const x = e.error; return [e.message, x === null ? 'null' : x === undefined ? 'undefined' : typeof x === 'object' ? x.constructor.name : typeof x, e.filename === document.URL, e.lineno, e.colno]; }window.addEventListener('error', (e) => { E.push(info(e)); if (window.cancelErrors) e.preventDefault(); });window.addEventListener('unhandledrejection', (e) => { U.push([String(e.reason && e.reason.message || e.reason), e.promise instanceof Promise, e.cancelable, e.isTrusted]); if (window.cancelRejections) e.preventDefault(); });</script>",
        queries: &[
            (
                r"[typeof performance, String(performance), performance instanceof Performance, Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype, typeof performance.now(), typeof performance.timeOrigin]",
                r#"["object", "[object Performance]", true, true, "number", "number"]"#,
            ),
            (
                r"(() => { const d = Date.now(), p = performance.timeOrigin + performance.now(); return Math.abs(d - p) < 20; })()",
                r"true",
            ),
            (
                r"(() => { const a = performance.now(); const e = new Event('x'); const b = performance.now(); return [a >= 0, e.timeStamp >= a - 0.2, e.timeStamp <= b + 0.2]; })()",
                r"[true, true, true]",
            ),
            (
                r"(async () => { const a = performance.now(), d = Date.now(); await wait(50); const b = performance.now(), e = Date.now(); return [b - a >= 49, b - a < 500, e - d >= 49, e - d < 500]; })()",
                r"[true, true, true, true]",
            ),
            (
                r"(async () => { let s; window.addEventListener('error', (e) => { s = e.timeStamp; }, { once: true }); const a = performance.now(); setTimeout(() => { throw 1; }, 0); await wait(30); return [s >= a, s - a < 500]; })()",
                r"[true, true]",
            ),
            (
                r"(() => { const t = performance.now(); return Math.round(t * 1000) % 5 === 0 || Math.round(t * 10000) % 1000 !== 0; })()",
                r"true",
            ),
            (
                r"[Object.getOwnPropertyNames(Performance.prototype).includes('now'), Object.getOwnPropertyNames(Performance.prototype).includes('timeOrigin'), typeof Object.getOwnPropertyDescriptor(window, 'performance').get, Object.getOwnPropertyDescriptor(Performance.prototype, 'timeOrigin').enumerable]",
                r#"[true, true, "function", true]"#,
            ),
        ],
    },
];
