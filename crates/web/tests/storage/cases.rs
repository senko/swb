// The cases of tools/probes/web-storage.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "storage-structure",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"[typeof Storage, Storage.length, Storage.name, Object.prototype.toString.call(localStorage), Object.prototype.toString.call(sessionStorage), localStorage instanceof Storage, sessionStorage instanceof Storage, localStorage === sessionStorage, localStorage.constructor === Storage]",
                r#"["function", 0, "Storage", "[object Storage]", "[object Storage]", true, true, false, true]"#,
            ),
            (
                r"[localStorage === localStorage, window.localStorage === localStorage, sessionStorage === sessionStorage, window.sessionStorage === sessionStorage, globalThis.localStorage === localStorage]",
                r"[true, true, true, true, true]",
            ),
            (
                r"E(() => new Storage())",
                r#""TypeError: Failed to construct 'Storage': Illegal constructor""#,
            ),
            (r"E(() => Storage())", r#""TypeError: Illegal constructor""#),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(window, 'localStorage'); return [d.enumerable, d.configurable, typeof d.get, typeof d.set, d.get.name]; })()",
                r#"[true, true, "function", "undefined", "get localStorage"]"#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(window, 'sessionStorage'); return [d.enumerable, d.configurable, typeof d.get, typeof d.set, d.get.name]; })()",
                r#"[true, true, "function", "undefined", "get sessionStorage"]"#,
            ),
            (
                r"Object.getOwnPropertyNames(Storage.prototype).sort().join()",
                r#""clear,constructor,getItem,key,length,removeItem,setItem""#,
            ),
            (
                r"['length','key','getItem','setItem','removeItem','clear'].map(n => { const d = Object.getOwnPropertyDescriptor(Storage.prototype, n); return n + ':' + (d.get ? 'get' : typeof d.value) + ':' + d.enumerable + ':' + d.configurable + ':' + (d.value ? d.value.length : '') }).join()",
                r#""length:get:true:true:,key:function:true:true:1,getItem:function:true:true:1,setItem:function:true:true:2,removeItem:function:true:true:1,clear:function:true:true:0""#,
            ),
            (
                r"Object.getOwnPropertyDescriptor(Storage.prototype, Symbol.toStringTag).value",
                r#""Storage""#,
            ),
            (r"Object.getOwnPropertyNames(localStorage).length", r"0"),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Storage.prototype, 'length').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Storage.prototype.getItem.call({}, 'a'))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Storage.prototype.setItem.call(sessionStorage, 'a', 'b'))",
                r#""undefined""#,
            ),
            (
                r"E(() => localStorage.getItem())",
                r#""TypeError: Failed to execute 'getItem' on 'Storage': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => localStorage.setItem('a'))",
                r#""TypeError: Failed to execute 'setItem' on 'Storage': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => localStorage.setItem())",
                r#""TypeError: Failed to execute 'setItem' on 'Storage': 2 arguments required, but only 0 present.""#,
            ),
            (
                r"E(() => localStorage.removeItem())",
                r#""TypeError: Failed to execute 'removeItem' on 'Storage': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => localStorage.key())",
                r#""TypeError: Failed to execute 'key' on 'Storage': 1 argument required, but only 0 present.""#,
            ),
            (r"E(() => localStorage.clear(1, 2))", r#""undefined""#),
            (
                r"E(() => localStorage.getItem.call(sessionStorage, 'a'))",
                r#""b""#,
            ),
            (r"E(() => localStorage.length = 3)", r"3"),
            (
                r"E(() => { 'use strict'; window.localStorage = 1; return typeof window.localStorage; })",
                r#""TypeError: Cannot set property localStorage of #<Window> which has only a getter""#,
            ),
            (
                r"(() => { window.localStorage = 5; return typeof window.localStorage; })()",
                r#""object""#,
            ),
        ],
    },
    Case {
        name: "storage-methods",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"[localStorage.length, localStorage.getItem('a'), localStorage.key(0), localStorage.setItem('a', 'b'), localStorage.length, localStorage.getItem('a'), localStorage.key(0), localStorage.key(1)]",
                r#"[0, null, null, null, 1, "b", "a", null]"#,
            ),
            (
                r"[localStorage.setItem(1, 2), localStorage.getItem('1'), localStorage.getItem(1), typeof localStorage.getItem('1')]",
                r#"[null, "2", "2", "string"]"#,
            ),
            (
                r"[localStorage.setItem(null, undefined), localStorage.getItem('null'), localStorage.getItem(null), localStorage.getItem('undefined')]",
                r#"[null, "undefined", "undefined", null]"#,
            ),
            (
                r"[localStorage.setItem(undefined, null), localStorage.getItem('undefined'), localStorage.getItem()]",
                r"error: TypeError: Failed to execute 'getItem' on 'Storage': 1 argument required, but only 0 present.",
            ),
            (
                r"[localStorage.setItem('o', {}), localStorage.getItem('o'), localStorage.setItem('arr', [1, 2]), localStorage.getItem('arr'), localStorage.setItem('f', 1.5), localStorage.getItem('f'), localStorage.setItem('t', true), localStorage.getItem('t')]",
                r#"[null, "[object Object]", null, "1,2", null, "1.5", null, "true"]"#,
            ),
            (
                r"[localStorage.setItem('', ''), localStorage.getItem(''), localStorage.length, localStorage.removeItem(''), localStorage.length]",
                r#"[null, "", 9, null, 8]"#,
            ),
            (
                r"[localStorage.getItem('missing'), localStorage.removeItem('missing'), localStorage.removeItem('a'), localStorage.getItem('a'), localStorage.length]",
                r"[null, null, null, null, 7]",
            ),
            (
                r"(() => { reset(); localStorage.setItem('only', '1'); return [localStorage.key(0), localStorage.key(-1), localStorage.key(1e10), localStorage.key(NaN), localStorage.key('0'), localStorage.key('x'), localStorage.key(0.9), localStorage.key(1.9), localStorage.key(4294967296), localStorage.key(4294967297), localStorage.key(null), localStorage.key(undefined), localStorage.key(2 ** 32 - 1), localStorage.key(Infinity)]; })()",
                r#"["only", null, null, "only", "only", "only", "only", null, "only", null, "only", "only", null, "only"]"#,
            ),
            (
                r"E(() => localStorage.setItem({ toString() { throw new Error('boom'); } }, 'v'))",
                r#""Error: boom""#,
            ),
            (
                r"E(() => localStorage.setItem('k', { toString() { throw new Error('boom'); } }))",
                r#""Error: boom""#,
            ),
            (
                r"E(() => localStorage.setItem(Symbol('s'), 'v'))",
                r#""TypeError: Failed to execute 'setItem' on 'Storage': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"[localStorage.clear(), localStorage.length, localStorage.key(0)]",
                r"[null, 0, null]",
            ),
            (
                r"(() => { reset(); localStorage.setItem('l', 'L'); sessionStorage.setItem('s', 'S'); return [localStorage.length, sessionStorage.length, localStorage.getItem('s'), sessionStorage.getItem('l'), localStorage.getItem('l'), sessionStorage.getItem('s')]; })()",
                r#"[1, 1, null, null, "L", "S"]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('x', '1'); sessionStorage.clear(); return [localStorage.length, sessionStorage.length]; })()",
                r"[1, 0]",
            ),
            (
                r"(() => { reset(); const a = []; for (let i = 0; i < 20; i++) localStorage.setItem('k' + i, String(i)); localStorage.setItem('k3', 'again'); localStorage.removeItem('k5'); localStorage.setItem('k5', 'back'); return [lens(localStorage).join(), localStorage.getItem('k3'), localStorage.getItem('k5'), localStorage.length]; })()",
                r#"["k0,k1,k10,k11,k12,k13,k14,k15,k16,k17,k18,k19,k2,k3,k4,k5,k6,k7,k8,k9", "again", "back", 20]"#,
            ),
            (
                r"(() => { reset(); for (const k of ['b', 'a', 'c', '10', '2', '', 'B', 'ab', 'é', '😀', 'z']) localStorage.setItem(k, '1'); return [lens(localStorage), K(localStorage)]; })()",
                r#"[["", "10", "2", "B", "a", "ab", "b", "c", "z", "é", "😀"], ["", "10", "2", "B", "a", "ab", "b", "c", "z", "é", "😀"]]"#,
            ),
            (
                r"(() => { reset(); for (const k of ['b', 'a', 'c', '10', '2', '', 'B', 'ab', 'é', 'z']) sessionStorage.setItem(k, '1'); return [lens(sessionStorage), K(sessionStorage)]; })()",
                r#"[["", "10", "2", "B", "a", "ab", "b", "c", "z", "é"], ["", "10", "2", "B", "a", "ab", "b", "c", "z", "é"]]"#,
            ),
            (
                r"(() => { reset(); const keys = []; for (let i = 0; i < 300; i++) keys.push('key' + ((i * 7919) % 300)); for (const k of keys) localStorage.setItem(k, '1'); const got = lens(localStorage); return [got.length, got.join() === [...keys].sort().join(), K(localStorage).join() === got.join(), Object.getOwnPropertyNames(localStorage).length]; })()",
                r"[300, true, true, 300]",
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', '1'); localStorage.setItem('b', '2'); localStorage.setItem('a', '3'); return [lens(localStorage), localStorage.getItem('a')]; })()",
                r#"[["a", "b"], "3"]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', '1'); localStorage.setItem('b', '2'); return [localStorage.key(0) !== localStorage.key(1), [localStorage.key(0), localStorage.key(1)].sort(), K(localStorage)]; })()",
                r#"[true, ["a", "b"], ["a", "b"]]"#,
            ),
        ],
    },
    Case {
        name: "storage-named",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"[localStorage.x = 1, localStorage.x, localStorage.getItem('x'), localStorage.length, typeof localStorage.x, localStorage['y'] = {}, localStorage.y, localStorage.length]",
                r#"[1, "1", "1", 1, "string", {}, "[object Object]", 2]"#,
            ),
            (
                r"['x' in localStorage, 'nope' in localStorage, localStorage.hasOwnProperty('x'), localStorage.hasOwnProperty('nope'), Object.prototype.hasOwnProperty.call(localStorage, 'y'), 'getItem' in localStorage, localStorage.hasOwnProperty('getItem'), 'length' in localStorage, localStorage.hasOwnProperty('length')]",
                r"[true, false, true, false, true, true, false, true, false]",
            ),
            (
                r"[K(localStorage), Object.getOwnPropertyNames(localStorage).sort(), JSON.stringify(Object.fromEntries(Object.entries(localStorage).sort())), Object.entries(localStorage).length]",
                r#"[["x", "y"], ["x", "y"], "{\"x\":\"1\",\"y\":\"[object Object]\"}", 2]"#,
            ),
            (
                r"(() => { const o = []; for (const k in localStorage) o.push(k); return [o.slice(0, 2).sort(), o.slice(2)]; })()",
                r#"[["x", "y"], ["length", "clear", "getItem", "key", "removeItem", "setItem"]]"#,
            ),
            (
                r"Object.getOwnPropertyDescriptor(localStorage, 'x')",
                r#"{"value": "1", "writable": true, "enumerable": true, "configurable": true}"#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(localStorage, 'y'); return [d.value, d.writable, d.enumerable, d.configurable]; })()",
                r#"["[object Object]", true, true, true]"#,
            ),
            (
                r"Object.getOwnPropertyDescriptor(localStorage, 'nope')",
                r"null",
            ),
            (
                r"[delete localStorage.x, localStorage.getItem('x'), localStorage.length, delete localStorage.nope, delete localStorage.length]",
                r"[true, null, 1, true, true]",
            ),
            (
                r"(() => { reset(); localStorage.setItem('getItem', 'v'); return [typeof localStorage.getItem, localStorage.getItem('getItem'), localStorage.length, K(localStorage), localStorage.hasOwnProperty('getItem'), Object.getOwnPropertyDescriptor(localStorage, 'getItem')]; })()",
                r#"["function", "v", 1, ["getItem"], false, null]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('key', 'v'); localStorage.setItem('length', 'v2'); return [typeof localStorage.key, localStorage.length, localStorage.getItem('length'), typeof localStorage.length, K(localStorage), localStorage.hasOwnProperty('length'), Object.getOwnPropertyDescriptor(localStorage, 'length')]; })()",
                r#"["function", 2, "v2", "number", ["key"], false, null]"#,
            ),
            (
                r"(() => { reset(); localStorage[0] = 'zero'; localStorage[1.5] = 'x'; localStorage[-1] = 'neg'; return [localStorage[0], localStorage['0'], localStorage[1.5], localStorage.getItem('0'), K(localStorage), localStorage.length, localStorage[2]]; })()",
                r#"["zero", "zero", "x", "zero", ["-1", "0", "1.5"], 3, null]"#,
            ),
            (
                r"(() => { reset(); const s = Symbol('s'); localStorage[s] = 'sym'; return [localStorage[s], localStorage.length, Object.getOwnPropertySymbols(localStorage).length, s in localStorage, delete localStorage[s], localStorage[s]]; })()",
                r#"["sym", 0, 1, true, true, null]"#,
            ),
            (
                r"(() => { reset(); localStorage.a = 'x'; localStorage.b = 'y'; return [Object.entries(localStorage).sort(), Object.values(localStorage).sort(), JSON.stringify(Object.fromEntries(Object.entries(localStorage).sort())), JSON.stringify(Object.assign({}, localStorage)).length, Object.keys({ ...localStorage }).length]; })()",
                r#"[[["a", "x"], ["b", "y"]], ["x", "y"], "{\"a\":\"x\",\"b\":\"y\"}", 17, 2]"#,
            ),
            (
                r"(() => { reset(); localStorage.n = null; localStorage.u = undefined; localStorage.num = 12; localStorage.o = { a: 1 }; return [localStorage.n, localStorage.u, localStorage.num, localStorage.o]; })()",
                r#"["null", "undefined", "12", "[object Object]"]"#,
            ),
            (
                r"(() => { reset(); localStorage.a = '1'; Object.defineProperty(localStorage, 'b', { value: 'dv', configurable: true, enumerable: true, writable: true }); return [localStorage.b, localStorage.getItem('b'), localStorage.length]; })()",
                r#"["dv", "dv", 2]"#,
            ),
            (
                r"E(() => { reset(); Object.defineProperty(localStorage, 'c', { get() { return 1; }, configurable: true }); return [localStorage.c, localStorage.length]; })",
                r#""TypeError: Failed to set a named property 'c' on 'Storage': Accessor properties are not allowed.""#,
            ),
            (
                r"E(() => { 'use strict'; reset(); Object.freeze(localStorage); return Object.isFrozen(localStorage); })",
                r#""TypeError: Cannot freeze""#,
            ),
            (
                r"E(() => { reset(); Object.preventExtensions(localStorage); localStorage.zz = '1'; return [Object.isExtensible(localStorage), localStorage.zz, localStorage.length]; })",
                r#""TypeError: Cannot prevent extensions""#,
            ),
            (
                r"(() => { reset(); localStorage.a = '1'; const p = new Proxy(localStorage, {}); return E(() => [p.a, p.length, Object.keys(p)]); })()",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"(() => { reset(); localStorage.a = '1'; const o = Object.create(localStorage); o.b = '2'; return [o.a, localStorage.length, Object.keys(o).sort(), localStorage.getItem('b'), o.hasOwnProperty('b'), 'a' in o]; })()",
                r#"["1", 1, ["b"], null, true, true]"#,
            ),
            (
                r"(() => { reset(); sessionStorage.s = '1'; return [sessionStorage.s, localStorage.s, K(sessionStorage), K(localStorage)]; })()",
                r#"["1", null, ["s"], []]"#,
            ),
        ],
    },
    Case {
        name: "storage-proto-names",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"(() => { try { reset(); localStorage.setItem('getItem', 'v'); return [typeof localStorage.getItem, localStorage.getItem('getItem'), localStorage.length, K(localStorage)]; } finally { clean('getItem'); } })()",
                r#"["function", "v", 1, ["getItem"]]"#,
            ),
            (
                r"(() => { try { reset(); localStorage.setItem('length', 'v2'); localStorage.setItem('key', 'v'); return [typeof localStorage.key, localStorage.length, localStorage.getItem('length'), typeof localStorage.length, K(localStorage), localStorage.hasOwnProperty('length'), Object.getOwnPropertyDescriptor(localStorage, 'length')]; } finally { clean('length'); } })()",
                r#"["function", 2, "v2", "number", ["key"], false, null]"#,
            ),
            (
                r"(() => { try { reset(); localStorage.getItem = 'mine'; return [typeof localStorage.getItem, localStorage.length, K(localStorage)]; } finally { clean('getItem'); } })()",
                r#"["string", 0, ["getItem"]]"#,
            ),
            (
                r"(() => { try { reset(); localStorage.length = 7; return [typeof localStorage.length, localStorage.length, K(localStorage)]; } finally { clean('length'); } })()",
                r#"["number", 0, []]"#,
            ),
            (
                r"(() => { try { reset(); localStorage.toString = 'ts'; return [typeof localStorage.toString, localStorage.length, K(localStorage)]; } finally { clean('toString'); } })()",
                r#"["string", 0, ["toString"]]"#,
            ),
            (
                r"(() => { try { reset(); localStorage.setItem('toString', 'ts'); return [typeof localStorage.toString, localStorage.length, K(localStorage), String(localStorage)]; } finally { clean('toString'); } })()",
                r#"["function", 1, ["toString"], "[object Storage]"]"#,
            ),
            (
                r"(() => { try { reset(); localStorage.setItem('constructor', 'cs'); return [localStorage.constructor === Storage, localStorage.length, K(localStorage)]; } finally { clean('constructor'); } })()",
                r#"[true, 1, ["constructor"]]"#,
            ),
            (
                r"(() => { reset(); return [typeof localStorage.getItem, typeof localStorage.length, K(localStorage)]; })()",
                r#"["function", "number", []]"#,
            ),
        ],
    },
    Case {
        name: "storage-quota",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"(() => { reset(); try { localStorage.setItem('b', 'y'.repeat(5242880 - 1)); } catch (e) { return ['err1', e.message]; } try { localStorage.setItem('a', 'x'); } catch (e) { return ['err', e.name, e.message, e.code, e.constructor.name, e instanceof DOMException, e instanceof Error, Object.prototype.toString.call(e), localStorage.length, localStorage.getItem('a')]; } return ['ok', localStorage.length]; })()",
                r#"["err", "QuotaExceededError", "Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.", 22, "QuotaExceededError", true, true, "[object QuotaExceededError]", 1, null]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', 'x'.repeat(5242879)); return [localStorage.length, localStorage.getItem('a').length]; })()",
                r"[1, 5242879]",
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('a', 'x'.repeat(5242880)); } catch (e) { return ['err', e.name, e.message, e.code, localStorage.length]; } return ['ok', localStorage.length]; })()",
                r#"["err", "QuotaExceededError", "Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.", 22, 0]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('', 'x'.repeat(5242880)); } catch (e) { return ['err', e.name, localStorage.length]; } return ['ok', localStorage.length]; })()",
                r#"["ok", 1]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('', 'x'.repeat(5242881)); } catch (e) { return ['err', e.name, e.message, localStorage.length]; } return ['ok', localStorage.length]; })()",
                r#"["err", "QuotaExceededError", "Failed to execute 'setItem' on 'Storage': Setting the value of '' exceeded the quota.", 0]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('k'.repeat(5242880), ''); } catch (e) { return ['err', e.name, localStorage.length]; } return ['ok', localStorage.length]; })()",
                r#"["ok", 1]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('k'.repeat(5242881), ''); } catch (e) { return ['err', e.name, e.message.length, localStorage.length]; } return ['ok', localStorage.length]; })()",
                r#"["err", "QuotaExceededError", 5242966, 0]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('a', '😀'.repeat(2621439)); } catch (e) { return ['err', e.name]; } try { localStorage.setItem('b', ''); } catch (e) { return ['err b', e.name]; } try { localStorage.setItem('c', ''); } catch (e) { return ['err c', e.name]; } return ['ok', localStorage.length]; })()",
                r#"["err c", "QuotaExceededError"]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.setItem('a', '€'.repeat(5242879)); } catch (e) { return ['err', e.name]; } return ['ok', localStorage.length]; })()",
                r#"["ok", 1]"#,
            ),
            (
                r"(() => { reset(); const big = 'x'.repeat(3000000); localStorage.setItem('a', big); const r = []; try { localStorage.setItem('b', big); r.push('ok'); } catch (e) { r.push(e.name); } try { localStorage.setItem('a', big + 'y'); r.push('ok'); } catch (e) { r.push(e.name); } try { localStorage.setItem('a', 'small'); r.push('ok'); } catch (e) { r.push(e.name); } try { localStorage.setItem('b', big); r.push('ok'); } catch (e) { r.push(e.name); } return [r, localStorage.length, localStorage.getItem('a')]; })()",
                r#"[["QuotaExceededError", "ok", "ok", "ok"], 2, "small"]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', 'x'.repeat(5242000)); const r = []; try { localStorage.setItem('a', 'y'.repeat(5242879)); r.push('ok'); } catch (e) { r.push(e.name); } try { localStorage.setItem('a', 'y'.repeat(5242880)); r.push('ok'); } catch (e) { r.push(e.name); } return [r, localStorage.getItem('a').length, localStorage.getItem('a')[0]]; })()",
                r#"[["ok", "QuotaExceededError"], 5242879, "y"]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', 'x'.repeat(5242879)); const r = []; try { localStorage.setItem('a', 'x'.repeat(5242879)); r.push('same ok'); } catch (e) { r.push(e.name); } try { localStorage.removeItem('a'); localStorage.setItem('b', 'x'.repeat(5242879)); r.push('after remove ok'); } catch (e) { r.push(e.name); } return [r, localStorage.length]; })()",
                r#"[["same ok", "after remove ok"], 1]"#,
            ),
            (
                r"(() => { reset(); const r = []; for (let i = 0; i < 10; i++) { try { localStorage.setItem('k' + i, 'x'.repeat(1000000)); r.push(1); } catch (e) { r.push(0); } } return [r.join(''), localStorage.length]; })()",
                r#"["1111100000", 5]"#,
            ),
            (
                r"(() => { reset(); const r = []; for (let i = 0; i < 6; i++) { try { sessionStorage.setItem('k' + i, 'x'.repeat(1000000)); r.push(1); } catch (e) { r.push(0); } } try { localStorage.setItem('l', 'x'.repeat(5000000)); r.push(1); } catch (e) { r.push(0); } return [r.join(''), sessionStorage.length, localStorage.length]; })()",
                r#"["1111101", 5, 1]"#,
            ),
            (
                r"(() => { reset(); try { localStorage.x = 'y'.repeat(5242880); } catch (e) { return ['err', e.name, e.message, e.code]; } return ['ok']; })()",
                r#"["err", "QuotaExceededError", "Failed to set a named property 'x' on 'Storage': Setting the value of 'x' exceeded the quota.", 22]"#,
            ),
            (
                r"(() => { reset(); try { sessionStorage.setItem('a', 'y'.repeat(5242880)); } catch (e) { return ['err', e.name, e.message, e.code]; } return ['ok']; })()",
                r#"["err", "QuotaExceededError", "Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.", 22]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', 'x'.repeat(5242879)); try { localStorage.setItem('b', ''); return 'ok'; } catch (e) { return e.name; } })()",
                r#""QuotaExceededError""#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a', 'x'.repeat(5242879)); try { localStorage.setItem('', ''); return ['ok', localStorage.length]; } catch (e) { return e.name; } })()",
                r#"["ok", 2]"#,
            ),
            (
                r"(() => { reset(); const e = (() => { try { localStorage.setItem('a', 'x'.repeat(5242881)); } catch (e) { return e; } })(); return [e.name, e.message, e.code, Object.getOwnPropertyNames(e), String(e), typeof e.quota, typeof e.requested, DOMException.QUOTA_ERR]; })()",
                r#"["QuotaExceededError", "Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.", 22, ["stack"], "QuotaExceededError: Failed to execute 'setItem' on 'Storage': Setting the value of 'a' exceeded the quota.", "object", "object", null]"#,
            ),
            (r"reset()", r"null"),
        ],
    },
    Case {
        name: "storage-values",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"(() => { reset(); localStorage.setItem('k', '\ud800'); const v = localStorage.getItem('k'); return [v.length, v.charCodeAt(0), v === '\ud800', v === '�']; })()",
                r"[1, 55296, true, false]",
            ),
            (
                r"(() => { reset(); localStorage.setItem('\ud800', 'v'); const k = localStorage.key(0); return [localStorage.length, k.length, k.charCodeAt(0), localStorage.getItem('\ud800'), localStorage.getItem('�')]; })()",
                r#"[1, 1, 55296, "v", null]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a\ud800b', 'x\udc00y'); return [localStorage.getItem('a\ud800b'), localStorage.getItem('a�b'), localStorage.getItem('a\ud800b').length, localStorage.getItem('a\ud800b').charCodeAt(1)]; })()",
                r#"["x�y", null, 3, 56320]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('\ud800', '1'); localStorage.setItem('�', '2'); return [localStorage.length, localStorage.getItem('\ud800'), localStorage.getItem('�')]; })()",
                r#"[2, "1", "2"]"#,
            ),
            (
                r"(() => { reset(); localStorage['\ud800x'] = 'lone'; return [Object.keys(localStorage).map(k => k.charCodeAt(0)), localStorage['\ud800x'], localStorage.length]; })()",
                r#"[[55296], "lone", 1]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('é', 'é€😀'); return [localStorage.getItem('é'), localStorage.key(0).length, localStorage.getItem('é').length]; })()",
                r#"["é€😀", 1, 4]"#,
            ),
            (
                r"(() => { reset(); localStorage.setItem('a\u0000b', 'v\u0000w'); return [localStorage.getItem('a\u0000b').length, localStorage.getItem('a'), localStorage.key(0).length, localStorage.getItem('a\u0000b').charCodeAt(1)]; })()",
                r"[3, null, 3, 0]",
            ),
            (
                r"(() => { reset(); localStorage.setItem('big', 'x'.repeat(100000)); return [localStorage.getItem('big').length, localStorage.length]; })()",
                r"[100000, 1]",
            ),
        ],
    },
    Case {
        name: "storage-events",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function reset() { localStorage.clear(); sessionStorage.clear(); }
function lens(st) { const o = []; for (let i = 0; i < st.length; i++) o.push(st.key(i)); return o.sort(); }
function K(st) { return Object.keys(st).sort(); }
function clean(...names) { for (const n of names) { delete localStorage[n]; } reset(); }
</script></body></html>",
        queries: &[
            (r"reset()", r"null"),
            (
                r"(() => { reset(); window.EV = []; window.addEventListener('storage', (e) => EV.push(e.type)); window.onstorage = (e) => EV.push('on'); localStorage.setItem('ev', '1'); sessionStorage.setItem('ev', '1'); localStorage.removeItem('ev'); localStorage.clear(); return EV.length; })()",
                r"0",
            ),
            (r"wait(60).then(() => EV.length)", r"0"),
            (
                r"[typeof StorageEvent, StorageEvent.length, new StorageEvent('storage').key, new StorageEvent('storage').storageArea, new StorageEvent('storage').url, new StorageEvent('storage').oldValue]",
                r#"["function", 1, null, null, "", null]"#,
            ),
            (
                r"(() => { const e = new StorageEvent('storage', { key: 'k', oldValue: 'o', newValue: 'n', url: 'http://a.test/', storageArea: localStorage }); return [e.key, e.oldValue, e.newValue, e.url, e.storageArea === localStorage, e.storageArea === sessionStorage, e.bubbles, e.cancelable, e.constructor.name]; })()",
                r#"["k", "o", "n", "http://a.test/", true, false, false, false, "StorageEvent"]"#,
            ),
            (
                r"E(() => new StorageEvent('storage', { storageArea: {} }))",
                r#""TypeError: Failed to construct 'StorageEvent': Failed to read the 'storageArea' property from 'StorageEventInit': Failed to convert value to 'Storage'.""#,
            ),
            (
                r"E(() => new StorageEvent('storage', { storageArea: 1 }))",
                r#""TypeError: Failed to construct 'StorageEvent': Failed to read the 'storageArea' property from 'StorageEventInit': Failed to convert value to 'Storage'.""#,
            ),
            (
                r"E(() => new StorageEvent('storage', { storageArea: null }).storageArea)",
                r"null",
            ),
            (
                r"E(() => new StorageEvent('storage', { storageArea: undefined }).storageArea)",
                r"null",
            ),
            (
                r"E(() => { const e = new StorageEvent('storage'); e.initStorageEvent('storage', true, true, 'a', null, 'b', 'http://x.test/', sessionStorage); return [e.key, e.oldValue, e.newValue, e.url, e.storageArea === sessionStorage, e.bubbles, e.cancelable]; })",
                r#"["a", null, "b", "http://x.test/", true, true, true]"#,
            ),
            (
                r"E(() => new StorageEvent('storage').initStorageEvent('storage', false, false, null, null, null, '', {}))",
                r#""TypeError: Failed to execute 'initStorageEvent' on 'StorageEvent': parameter 8 is not of type 'Storage'.""#,
            ),
            (
                r"E(() => { const e = new StorageEvent('storage', { storageArea: localStorage }); localStorage.setItem('x', 'y'); return [e.storageArea.getItem('x'), e.storageArea === window.localStorage]; })",
                r#"["y", true]"#,
            ),
            (
                r"E(() => { let n = 0; window.addEventListener('storage', () => n++); window.dispatchEvent(new StorageEvent('storage', { key: 'z' })); return n; })",
                r"1",
            ),
            (
                r"(() => { reset(); window.onstorage = () => {}; return [typeof window.onstorage, 'onstorage' in window, typeof document.body.onstorage]; })()",
                r#"["function", true, "function"]"#,
            ),
        ],
    },
];
