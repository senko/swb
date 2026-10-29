// The cases of tools/probes/web-navigator.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "navigator-structure",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof Navigator, Navigator.length, Navigator.name, Object.prototype.toString.call(navigator), navigator === navigator, window.navigator === navigator, navigator instanceof Navigator, navigator.constructor === Navigator]",
                r#"["function", 0, "Navigator", "[object Navigator]", true, true, true, true]"#,
            ),
            (
                r"E(() => new Navigator())",
                r#""TypeError: Failed to construct 'Navigator': Illegal constructor""#,
            ),
            (
                r"E(() => Navigator())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(window, 'navigator'); return [d.enumerable, d.configurable, typeof d.get, typeof d.set, d.get.name]; })()",
                r#"[true, true, "function", "undefined", "get navigator"]"#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgent'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name]; })()",
                r#"["function", "undefined", true, true, "get userAgent"]"#,
            ),
            (r"Object.getOwnPropertyNames(navigator).length", r"0"),
            (
                r"['appCodeName','appName','appVersion','platform','product','productSub','userAgent','vendor','vendorSub','language','languages','onLine','cookieEnabled','hardwareConcurrency','maxTouchPoints','webdriver','doNotTrack','javaEnabled','pdfViewerEnabled'].map(n => n in Navigator.prototype).join()",
                r#""true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgent').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => navigator.javaEnabled.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"[navigator.javaEnabled.name, navigator.javaEnabled.length, navigator.javaEnabled(), typeof navigator.javaEnabled]",
                r#"["javaEnabled", 0, false, "function"]"#,
            ),
            (
                r"(() => { navigator.userAgent = 'x'; navigator.foo = 1; return [navigator.userAgent === 'x', navigator.foo]; })()",
                r"[false, 1]",
            ),
            (
                r"(() => { 'use strict'; try { navigator.userAgent = 'x'; return 'no error'; } catch (e) { return e.name + ': ' + e.message; } })()",
                r#""TypeError: Cannot set property userAgent of #<Navigator> which has only a getter""#,
            ),
            (
                r"Object.getOwnPropertyDescriptor(Navigator.prototype, Symbol.toStringTag).value",
                r#""Navigator""#,
            ),
        ],
    },
    Case {
        name: "navigator-values",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[navigator.appCodeName, navigator.appName, navigator.product, navigator.productSub, navigator.vendorSub]",
                r#"["Mozilla", "Netscape", "Gecko", "20030107", ""]"#,
            ),
            (
                r"[navigator.language, navigator.languages, navigator.onLine, navigator.cookieEnabled, navigator.maxTouchPoints, navigator.doNotTrack, navigator.pdfViewerEnabled]",
                r#"["en-US", ["en-US"], true, true, 0, null, false]"#,
            ),
            (
                r"[Object.isFrozen(navigator.languages), navigator.languages === navigator.languages, Array.isArray(navigator.languages), navigator.languages[0] === navigator.language]",
                r"[true, false, true, true]",
            ),
            (
                r"[navigator.userAgent.startsWith('Mozilla/5.0 ('), navigator.appVersion === navigator.userAgent.replace('Mozilla/', ''), typeof navigator.platform, navigator.platform.length > 0, typeof navigator.vendor]",
                r#"[true, true, "string", true, "string"]"#,
            ),
            (
                r"[typeof navigator.hardwareConcurrency, Number.isInteger(navigator.hardwareConcurrency), navigator.hardwareConcurrency >= 1]",
                r#"["number", true, true]"#,
            ),
            (
                r"[typeof navigator.userAgent, typeof navigator.webdriver, typeof navigator.maxTouchPoints, typeof navigator.onLine]",
                r#"["string", "boolean", "number", "boolean"]"#,
            ),
        ],
    },
    Case {
        name: "navigator-departures",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (r"navigator.vendor", r#""Google Inc.""#),
            (r"navigator.webdriver", r"true"),
            (r"navigator.userAgent.includes('Chrome')", r"true"),
            (r"'userAgentData' in navigator", r"true"),
            (r"typeof navigator.sendBeacon", r#""function""#),
            (
                r"[typeof navigator.plugins, typeof navigator.mimeTypes, typeof navigator.mediaDevices, typeof navigator.serviceWorker, typeof navigator.permissions]",
                r#"["object", "object", "object", "object", "object"]"#,
            ),
            (r"window.clientInformation === navigator", r"true"),
        ],
    },
    Case {
        name: "visibility",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof document.hidden, document.hidden, document.visibilityState, 'hidden' in Document.prototype, 'visibilityState' in Document.prototype, Object.getOwnPropertyDescriptor(Document.prototype, 'hidden').get.name, typeof Object.getOwnPropertyDescriptor(Document.prototype, 'hidden').set, Object.getOwnPropertyDescriptor(Document.prototype, 'visibilityState').get.name, typeof Object.getOwnPropertyDescriptor(Document.prototype, 'visibilityState').set]",
                r#"["boolean", false, "visible", true, true, "get hidden", "undefined", "get visibilityState", "undefined"]"#,
            ),
            (
                r"[document.onvisibilitychange, 'onvisibilitychange' in document, 'onvisibilitychange' in window]",
                r"[null, true, false]",
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Document.prototype, 'hidden').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"(() => { const d = document.implementation.createHTMLDocument(''); return [d.hidden, d.visibilityState]; })()",
                r#"[true, "hidden"]"#,
            ),
            (
                r"(() => { const d = new DOMParser().parseFromString('<p>x', 'text/html'); return [d.hidden, d.visibilityState]; })()",
                r#"[true, "hidden"]"#,
            ),
            (
                r"(async () => { L.length = 0; document.addEventListener('visibilitychange', () => log('event')); await wait(150); return JSON.stringify(L); })()",
                r#""[]""#,
            ),
            (
                r"(() => { L.length = 0; document.onvisibilitychange = (e) => log([e.type, e.bubbles, e.cancelable, e.target === document]); document.dispatchEvent(new Event('visibilitychange')); document.onvisibilitychange = null; return JSON.stringify(L); })()",
                r#""[\"event\",[\"visibilitychange\",false,false,true]]""#,
            ),
            (
                r"(() => { document.hidden = true; return [document.hidden, document.visibilityState]; })()",
                r#"[false, "visible"]"#,
            ),
            (r"document.webkitHidden", r"false"),
        ],
    },
];
