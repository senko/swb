// The cases of tools/probes/web-structured-clone.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "api",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"[typeof structuredClone, structuredClone.length, structuredClone.name, Object.getOwnPropertyDescriptor(window, 'structuredClone').enumerable, Object.getOwnPropertyDescriptor(window, 'structuredClone').writable, Object.getOwnPropertyDescriptor(window, 'structuredClone').configurable]",
                r#"["function", 1, "structuredClone", true, true, true]"#,
            ),
            (
                r"E(() => structuredClone())",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => structuredClone(1, 5))",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': The provided value is not of type 'StructuredSerializeOptions'.""#,
            ),
            (r"E(() => structuredClone(1, null))", r"1"),
            (
                r"E(() => structuredClone(1, {transfer: 5}))",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': The provided value cannot be converted to a sequence.""#,
            ),
            (
                r"E(() => structuredClone(1, {transfer: [1]}))",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': Failed to convert value to 'object'.""#,
            ),
            (r"E(() => structuredClone(1, {transfer: undefined}))", r"1"),
            (r"E(() => structuredClone.call(null, 1))", r"1"),
            (
                r"E(() => structuredClone.call({}, 1))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => structuredClone(1, {transfer: [], extra: 1}))",
                r"1",
            ),
            (
                r"E(() => new structuredClone(1))",
                r#""TypeError: structuredClone is not a constructor""#,
            ),
        ],
    },
    Case {
        name: "primitives",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"[C(1), C('a'), C(null), C(undefined), C(true), C(false), C(2 ** 53), C(-1.5), C('').length]",
                r#"[1, "a", null, null, true, false, 9007199254740992, -1.5, 0]"#,
            ),
            (
                r"[same(C(NaN), NaN), same(C(-0), -0), same(C(0), 0), same(C(Infinity), Infinity)]",
                r"[true, true, true, true]",
            ),
            (
                r"[typeof C(10n), String(C(10n)), String(C(-(2n ** 100n)))]",
                r#"["bigint", "10", "-1267650600228229401496703205376"]"#,
            ),
            (
                r"E(() => C(Symbol('a')))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Symbol(a) could not be cloned.""#,
            ),
            (
                r"E(() => C(Symbol.iterator))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Symbol(Symbol.iterator) could not be cloned.""#,
            ),
            (r"C('a\ud800b').charCodeAt(1)", r"55296"),
            (
                r"E(() => C(() => 1))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': () => 1 could not be cloned.""#,
            ),
            (
                r"E(() => C(function f() { return 1; }))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': function f() { return 1; } could not be cloned.""#,
            ),
            (
                r"E(() => C(class A {}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': class A {} could not be cloned.""#,
            ),
            (
                r"E(() => C([() => 1]))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': () => 1 could not be cloned.""#,
            ),
            (
                r"E(() => C({a: {b: Symbol('x')}}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Symbol(x) could not be cloned.""#,
            ),
            (
                r"E(() => C(Math.sin))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': function sin() { [native code] } could not be cloned.""#,
            ),
            (
                r"E(() => C(async () => {}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': async () => {} could not be cloned.""#,
            ),
            (
                r"E(() => C(function* g() {}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': function* g() {} could not be cloned.""#,
            ),
        ],
    },
    Case {
        name: "wrappers",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const c = C(new Number(1)); return [T(c), typeof c, c.valueOf(), c instanceof Number]; })()",
                r#"["[object Number]", "object", 1, true]"#,
            ),
            (
                r"(() => { const c = C(new String('ab')); return [T(c), typeof c, c.valueOf(), c.length, c instanceof String, Object.keys(c)]; })()",
                r#"["[object String]", "object", "ab", 2, true, ["0", "1"]]"#,
            ),
            (
                r"(() => { const c = C(new Boolean(false)); return [T(c), typeof c, c.valueOf()]; })()",
                r#"["[object Boolean]", "object", false]"#,
            ),
            (
                r"(() => { const c = C(Object(5n)); return [T(c), typeof c, String(c.valueOf())]; })()",
                r#"["[object BigInt]", "object", "5"]"#,
            ),
            (
                r"E(() => C(Object(Symbol())))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': [object Symbol] could not be cloned.""#,
            ),
            (
                r"(() => { const n = new Number(3); n.x = 1; const c = C(n); return [c.x, c.valueOf()]; })()",
                r"[null, 3]",
            ),
            (
                r"(() => { const s = new String('ab'); s.x = 1; const c = C(s); return [c.x, Object.keys(c)]; })()",
                r#"[null, ["0", "1"]]"#,
            ),
        ],
    },
    Case {
        name: "objects",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const c = C({b: 1, a: 2, 2: 'x', 1: 'y'}); return [Object.keys(c), JSON.stringify(c)]; })()",
                r#"[["1", "2", "b", "a"], "{\"1\":\"y\",\"2\":\"x\",\"b\":1,\"a\":2}"]"#,
            ),
            (
                r"(() => { const o = {a: 1}; Object.defineProperty(o, 'h', {value: 2, enumerable: false}); o[Symbol('s')] = 3; const c = C(o); return [Object.getOwnPropertyNames(c), Object.getOwnPropertySymbols(c).length]; })()",
                r#"[["a"], 0]"#,
            ),
            (
                r"(() => { const c = C(Object.create({inherited: 1}, {own: {value: 2, enumerable: true}})); return [Object.getPrototypeOf(c) === Object.prototype, c.inherited, c.own]; })()",
                r"[true, null, 2]",
            ),
            (
                r"(() => { class A { constructor() { this.x = 1; } get y() { return 2; } m() {} } const c = C(new A()); return [Object.getPrototypeOf(c) === Object.prototype, c.constructor === Object, JSON.stringify(c), typeof c.m]; })()",
                r#"[true, true, "{\"x\":1}", "undefined"]"#,
            ),
            (
                r"(() => { let n = 0; const o = {get a() { n++; return 5; }, b: 1}; const c = C(o); const d = Object.getOwnPropertyDescriptor(c, 'a'); return [n, d.value, d.writable, d.enumerable, d.configurable, 'get' in d]; })()",
                r"[1, 5, true, true, true, false]",
            ),
            (
                r"E(() => C({get a() { throw new RangeError('boom'); }}))",
                r#""RangeError: boom""#,
            ),
            (
                r"(() => { const log = []; const o = {get a() { log.push('a'); return {get x() { log.push('x'); return 1; }}; }, get b() { log.push('b'); return 2; }}; C(o); return log.join(); })()",
                r#""a,x,b""#,
            ),
            (
                r"(() => { const c = C(Object.create(null, {a: {value: 1, enumerable: true}})); return [Object.getPrototypeOf(c) === Object.prototype, c.a]; })()",
                r"[true, 1]",
            ),
            (
                r"(() => { const o = {}; Object.freeze(o); const c = C(o); return [Object.isFrozen(c), Object.isExtensible(c)]; })()",
                r"[false, true]",
            ),
            (
                r"(() => { const o = {a: {b: {c: [1, 2, {d: 'deep'}]}}}; const c = C(o); return [c.a.b.c[2].d, c !== o, c.a !== o.a]; })()",
                r#"["deep", true, true]"#,
            ),
            (
                r"(() => { const o = {a: 1}; o.self = o; const c = C(o); return [c.self === c, c !== o]; })()",
                r"[true, true]",
            ),
            (
                r"(() => { const s = {x: 1}; const o = {a: s, b: s, c: [s]}; const c = C(o); return [c.a === c.b, c.a === c.c[0], c.a !== s]; })()",
                r"[true, true, true]",
            ),
            (
                r"(() => { const a = {}; const b = {a}; a.b = b; const c = C([a, b]); return [c[0].b === c[1], c[1].a === c[0]]; })()",
                r"[true, true]",
            ),
            (
                r"(() => { const o = {'__proto__x': 1}; Object.defineProperty(o, '__proto__', {value: 5, enumerable: true, configurable: true, writable: true}); const c = C(o); return [Object.getPrototypeOf(c) === Object.prototype, Object.getOwnPropertyDescriptor(c, '__proto__') !== undefined]; })()",
                r"[true, true]",
            ),
            (
                r"E(() => C(new Proxy({}, {})))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<Object> could not be cloned.""#,
            ),
            (
                r"E(() => C(new Proxy([], {})))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': [object Array] could not be cloned.""#,
            ),
            (
                r"E(() => C(Promise.resolve(1)))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<Promise> could not be cloned.""#,
            ),
            (
                r"E(() => C(new WeakMap()))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<WeakMap> could not be cloned.""#,
            ),
            (
                r"E(() => C(new WeakSet()))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<WeakSet> could not be cloned.""#,
            ),
            (
                r"E(() => C(new WeakRef({})))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<WeakRef> could not be cloned.""#,
            ),
            (
                r"E(() => C((function () { return arguments; })(1)))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<Object> could not be cloned.""#,
            ),
            (
                r"E(() => C({a: 1, f() {}}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': f() {} could not be cloned.""#,
            ),
            (
                r"E(() => C(Symbol))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': function Symbol() { [native code] } could not be cloned.""#,
            ),
            (
                r"E(() => C(globalThis))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<Window> could not be cloned.""#,
            ),
            (
                r"E(() => C(window))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': #<Window> could not be cloned.""#,
            ),
            (
                r"E(() => C(document))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': HTMLDocument object could not be cloned.""#,
            ),
            (
                r"E(() => C(document.body))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': HTMLBodyElement object could not be cloned.""#,
            ),
            (
                r"E(() => C([document.body]))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': HTMLBodyElement object could not be cloned.""#,
            ),
            (
                r"E(() => C(new Event('x')))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Event object could not be cloned.""#,
            ),
            (
                r"E(() => C(location))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Location object could not be cloned.""#,
            ),
            (
                r"E(() => C(new Map().entries()))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': [object Map Iterator] could not be cloned.""#,
            ),
            (
                r"E(() => C([].values()))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': [object Array Iterator] could not be cloned.""#,
            ),
            (r"E(() => C(new (class extends Array {})()))", r"[]"),
            (r"E(() => C(JSON))", r"{}"),
            (r"E(() => C(Math))", r"{}"),
        ],
    },
    Case {
        name: "arrays",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const c = C([1, 'a', null, undefined]); return [Array.isArray(c), c.length, JSON.stringify(c), 3 in c]; })()",
                r#"[true, 4, "[1,\"a\",null,null]", true]"#,
            ),
            (
                r"(() => { const c = C([1, , 3]); return [c.length, 1 in c, JSON.stringify(c)]; })()",
                r#"[3, false, "[1,null,3]"]"#,
            ),
            (
                r"(() => { const a = []; a[5] = 1; a[100] = 2; const c = C(a); return [c.length, Object.keys(c)]; })()",
                r#"[101, ["5", "100"]]"#,
            ),
            (
                r"(() => { const a = [1, 2]; a.extra = 'e'; a[-1] = 'n'; const c = C(a); return [c.length, c.extra, c[-1], Object.keys(c)]; })()",
                r#"[2, "e", "n", ["0", "1", "extra", "-1"]]"#,
            ),
            (
                r"(() => { const a = new Array(5); const c = C(a); return [c.length, Object.keys(c)]; })()",
                r"[5, []]",
            ),
            (
                r"(() => { const a = [1, 2, 3]; a.length = 10; const c = C(a); return [c.length, Object.keys(c)]; })()",
                r#"[10, ["0", "1", "2"]]"#,
            ),
            (
                r"(() => { const a = [1]; a.push(a); const c = C(a); return [c[1] === c, c.length]; })()",
                r"[true, 2]",
            ),
            (
                r"(() => { const a = [1, 2, 3]; Object.defineProperty(a, 1, {get() { return 'g'; }, enumerable: true}); const c = C(a); return [JSON.stringify(c), Object.getOwnPropertyDescriptor(c, 1).value]; })()",
                r#"["[1,\"g\",3]", "g"]"#,
            ),
            (
                r"(() => { const c = C(new Array(2 ** 32 - 1)); return c.length; })()",
                r"4294967295",
            ),
            (
                r"(() => { const a = [1,2,3]; Object.defineProperty(a, 'length', {writable: false}); const c = C(a); return [Object.getOwnPropertyDescriptor(c, 'length').writable, c.length]; })()",
                r"[true, 3]",
            ),
            (
                r"(() => { const c = C(Object.assign([1], {length: 1, x: 1})); return Object.keys(c); })()",
                r#"["0", "x"]"#,
            ),
        ],
    },
    Case {
        name: "date-regexp",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const d = new Date(1234567890123); const c = C(d); return [T(c), c.getTime(), c !== d, c instanceof Date, Object.getPrototypeOf(c) === Date.prototype]; })()",
                r#"["[object Date]", 1234567890123, true, true, true]"#,
            ),
            (
                r"(() => { const c = C(new Date(NaN)); return [T(c), String(c.getTime())]; })()",
                r#"["[object Date]", "NaN"]"#,
            ),
            (
                r"(() => { const d = new Date(5); d.x = 1; const c = C(d); return [c.x, Object.keys(c)]; })()",
                r"[null, []]",
            ),
            (
                r"(() => { const r = /a(b)+/gi; r.lastIndex = 3; const c = C(r); return [T(c), c.source, c.flags, c.lastIndex, c !== r, c instanceof RegExp]; })()",
                r#"["[object RegExp]", "a(b)+", "gi", 0, true, true]"#,
            ),
            (
                r"(() => { const r = /x/; r.extra = 1; const c = C(r); return [c.extra, Object.keys(c), Object.getOwnPropertyNames(c)]; })()",
                r#"[null, [], ["lastIndex"]]"#,
            ),
            (
                r"(() => { const c = C(/./dgimsuy); return c.flags; })()",
                r#""dgimsuy""#,
            ),
            (
                r"(() => { const c = C(/[\p{L}--[a-z]]/v); return [c.flags, c.source]; })()",
                r#"["v", "[\\p{L}--[a-z]]"]"#,
            ),
            (
                r"(() => { const c = C(new RegExp('\ud800')); return [c.source.length]; })()",
                r"[1]",
            ),
            (
                r"(() => { const c = C(/(?<n>x)/); return [c.source, c.exec('x').groups.n]; })()",
                r#"["(?<n>x)", "x"]"#,
            ),
            (
                r"(() => { class D extends Date {} const c = C(new D(7)); return [Object.getPrototypeOf(c) === Date.prototype, c.getTime()]; })()",
                r"[true, 7]",
            ),
            (
                r"(() => { class R extends RegExp {} const c = C(new R('x', 'g')); return [Object.getPrototypeOf(c) === RegExp.prototype, c.flags]; })()",
                r#"[true, "g"]"#,
            ),
        ],
    },
    Case {
        name: "map-set",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const m = new Map([[1, 'a'], ['k', {x: 1}], [NaN, 2]]); const c = C(m); return [T(c), c.size, [...c.keys()].map(String), c.get('k').x, c !== m, c instanceof Map]; })()",
                r#"["[object Map]", 3, ["1", "k", "NaN"], 1, true, true]"#,
            ),
            (
                r"(() => { const s = new Set([1, 'a', {y: 2}, 1]); const c = C(s); return [T(c), c.size, [...c].length, [...c][2].y]; })()",
                r#"["[object Set]", 3, 3, 2]"#,
            ),
            (
                r"(() => { const m = new Map(); m.set(m, m); const c = C(m); return [c.get(c) === c, c.size]; })()",
                r"[true, 1]",
            ),
            (
                r"(() => { const s = new Set(); s.add(s); const c = C(s); return [c.has(c), c.size]; })()",
                r"[true, 1]",
            ),
            (
                r"(() => { const k = {}; const m = new Map([[k, 1]]); const o = {k, m}; const c = C(o); return [c.m.keys().next().value === c.k]; })()",
                r"[true]",
            ),
            (
                r"(() => { const m = new Map([[1, 1]]); m.extra = 5; const c = C(m); return [c.extra, Object.keys(c)]; })()",
                r"[null, []]",
            ),
            (
                r"(() => { const m = new Map([[1, 1], [2, 2], [3, 3]]); m.delete(2); m.set(2, 'again'); const c = C(m); return [...c.entries()].map(String); })()",
                r#"["1,1", "3,3", "2,again"]"#,
            ),
            (
                r"(() => { class M extends Map {} const c = C(new M([[1, 2]])); return [Object.getPrototypeOf(c) === Map.prototype, c.size]; })()",
                r"[true, 1]",
            ),
            (
                r"E(() => C(new Map([[1, () => 1]])))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': () => 1 could not be cloned.""#,
            ),
            (
                r"E(() => C(new Set([Symbol()])))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Symbol() could not be cloned.""#,
            ),
            (
                r"(() => { const m = new Map(); m.set(-0, 1); const c = C(m); return [Object.is([...c.keys()][0], 0)]; })()",
                r"[true]",
            ),
            (
                r"(() => { const m = new Map([[1, 2]]); m.get = () => { throw 1; }; const c = C(m); return c.size; })()",
                r"1",
            ),
            (
                r"(() => { const m = new Map(); for (let i = 0; i < 1000; i++) m.set(i, {i}); const c = C(m); return [c.size, c.get(999).i]; })()",
                r"[1000, 999]",
            ),
        ],
    },
    Case {
        name: "buffers",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const b = new ArrayBuffer(8); new Uint8Array(b).set([1, 2, 3]); const c = C(b); return [T(c), c.byteLength, [...new Uint8Array(c)].join(), c !== b, c instanceof ArrayBuffer]; })()",
                r#"["[object ArrayBuffer]", 8, "1,2,3,0,0,0,0,0", true, true]"#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const c = C(b); new Uint8Array(c)[0] = 9; return new Uint8Array(b)[0]; })()",
                r"0",
            ),
            (
                r"(() => { const u = new Uint8Array([1, 2, 3]); const c = C(u); return [T(c), c.length, [...c].join(), c.buffer !== u.buffer, c.byteOffset, c instanceof Uint8Array]; })()",
                r#"["[object Uint8Array]", 3, "1,2,3", true, 0, true]"#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(16); const u = new Int16Array(b, 4, 3); u.set([1, -2, 3]); const c = C(u); return [c.length, c.byteOffset, c.buffer.byteLength, [...c].join()]; })()",
                r#"[3, 4, 16, "1,-2,3"]"#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const o = {u: new Uint8Array(b, 0, 4), v: new Uint8Array(b, 4), w: b}; const c = C(o); return [c.u.buffer === c.v.buffer, c.u.buffer === c.w, c.w !== b, c.u.length, c.v.length, c.v.byteOffset]; })()",
                r"[true, true, true, 4, 4, 4]",
            ),
            (
                r"(() => { const out = []; for (const T of [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array]) { const c = C(new T(2)); out.push(T.name + ':' + (c instanceof T) + ':' + c.byteLength); } return out; })()",
                r#"["Int8Array:true:2", "Uint8Array:true:2", "Uint8ClampedArray:true:2", "Int16Array:true:4", "Uint16Array:true:4", "Int32Array:true:8", "Uint32Array:true:8", "Float32Array:true:8", "Float64Array:true:16", "BigInt64Array:true:16", "BigUint64Array:true:16"]"#,
            ),
            (
                r"(() => { const t = typeof Float16Array === 'undefined' ? 'none' : (() => { try { const c = C(new Float16Array([1.5])); return T(c) + c[0]; } catch (e) { return e.message; } })(); return t; })()",
                r#""[object Float16Array]1.5""#,
            ),
            (
                r"(() => { const d = new DataView(new ArrayBuffer(8), 2, 4); d.setInt16(0, -5); const c = C(d); return [T(c), c.byteLength, c.byteOffset, c.getInt16(0), c.buffer.byteLength, c instanceof DataView]; })()",
                r#"["[object DataView]", 4, 2, -5, 8, true]"#,
            ),
            (
                r"(() => { const u = new Float64Array([NaN, -0, 1.5]); const c = C(u); return [Number.isNaN(c[0]), Object.is(c[1], -0), c[2]]; })()",
                r"[true, true, 1.5]",
            ),
            (
                r"(() => { const u = new Uint8Array(2); u.extra = 1; const c = C(u); return [c.extra, Object.keys(c)]; })()",
                r#"[null, ["0", "1"]]"#,
            ),
            (
                r"(() => { class U extends Uint8Array {} const c = C(new U(2)); return [Object.getPrototypeOf(c) === Uint8Array.prototype]; })()",
                r"[true]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(4); b.extra = 1; const c = C(b); return [c.extra, Object.keys(c)]; })()",
                r"[null, []]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(4, {maxByteLength: 16}); const c = C(b); return [c.resizable, c.maxByteLength, c.byteLength]; })()",
                r"[true, 16, 4]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(4, {maxByteLength: 16}); const u = new Uint8Array(b); const c = C(u); b.resize(8); return [u.length, c.length, c.buffer.resizable, c.buffer.byteLength]; })()",
                r"[8, 4, true, 4]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(4, {maxByteLength: 16}); const u = new Uint8Array(b); const c = C(u); c.buffer.resize(12); return [c.length]; })()",
                r"[12]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const d = b.transfer(); return E(() => C(b)); })()",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': An ArrayBuffer is detached and could not be cloned.""#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const u = new Uint8Array(b); b.transfer(); return E(() => C(u)); })()",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': An ArrayBuffer is detached and could not be cloned.""#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const o = {u: new Uint8Array(b)}; b.transfer(); return E(() => C(o)); })()",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': An ArrayBuffer is detached and could not be cloned.""#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(0); const c = C(b); return [c.byteLength]; })()",
                r"[0]",
            ),
            (
                r"(() => { const s = typeof SharedArrayBuffer; return s; })()",
                r#""undefined""#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const u = new Uint8Array(b, 2, 4); const c = C(u); return [c.buffer.byteLength, c.byteOffset]; })()",
                r"[8, 2]",
            ),
            (
                r"(() => { const c = C(new Uint8Array(1e6)); return c.length; })()",
                r"1000000",
            ),
        ],
    },
    Case {
        name: "transfer",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const b = new ArrayBuffer(8); new Uint8Array(b)[0] = 7; const c = C(b, {transfer: [b]}); return [b.byteLength, c.byteLength, new Uint8Array(c)[0], b.detached, c !== b]; })()",
                r"[0, 8, 7, true, true]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const u = new Uint8Array(b); const c = C({u}, {transfer: [b]}); return [u.length, u.byteLength, c.u.length, c.u.buffer.byteLength, b.byteLength]; })()",
                r"[0, 0, 8, 8, 0]",
            ),
            (
                r"E(() => { const b = new ArrayBuffer(8); return C(b, {transfer: [b, b]}); })",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': ArrayBuffer at index 1 is a duplicate of an earlier ArrayBuffer.""#,
            ),
            (
                r"E(() => { const b = new ArrayBuffer(8); const c = C(1, {transfer: [b]}); return [b.byteLength, c]; })",
                r"[0, 1]",
            ),
            (
                r"E(() => C(1, {transfer: [{}]}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Value at index 0 does not have a transferable type.""#,
            ),
            (
                r"E(() => C(1, {transfer: [new Uint8Array(1)]}))",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Value at index 0 does not have a transferable type.""#,
            ),
            (
                r"E(() => C(1, {transfer: [new Uint8Array(1).buffer, 1]}))",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': Failed to convert value to 'object'.""#,
            ),
            (
                r"E(() => { const b = new ArrayBuffer(8); b.transfer(); return C(1, {transfer: [b]}); })",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': ArrayBuffer at index 0 is already detached.""#,
            ),
            (
                r"E(() => C(1, {transfer: new Set([new ArrayBuffer(1)])}))",
                r"1",
            ),
            (
                r"E(() => C(1, {transfer: {length: 1, 0: new ArrayBuffer(1)}}))",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': The object must have a callable @@iterator property.""#,
            ),
            (
                r"E(() => { const b = new ArrayBuffer(8); return C(() => 1, {transfer: [b]}); })",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': () => 1 could not be cloned.""#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); try { C(() => 1, {transfer: [b]}); } catch (e) {} return b.byteLength; })()",
                r"8",
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); try { C({f: () => 1, b}, {transfer: [b]}); } catch (e) {} return b.byteLength; })()",
                r"8",
            ),
            (
                r"E(() => { const b = new ArrayBuffer(8); return C(1, {transfer: [new Proxy(b, {})]}); })",
                r#""DOMException DataCloneError [25]: Failed to execute 'structuredClone' on 'Window': Value at index 0 does not have a transferable type.""#,
            ),
            (
                r"(() => { const b = new ArrayBuffer(4, {maxByteLength: 8}); const c = C(b, {transfer: [b]}); return [c.resizable, c.maxByteLength, c.byteLength, b.detached]; })()",
                r"[true, 8, 4, true]",
            ),
            (
                r"(() => { const b = new ArrayBuffer(8); const dv = new DataView(b, 4); const c = C(dv, {transfer: [b]}); return [dv.byteLength, c.byteLength, c.byteOffset, b.byteLength]; })()",
                r"error: TypeError: Cannot perform get DataView.prototype.byteLength on a detached or out-of-bounds ArrayBuffer",
            ),
            (
                r"E(() => { let n = 0; const t = {get length() { n++; return 0; }}; C(1, {transfer: t}); return n; })",
                r#""TypeError: Failed to execute 'structuredClone' on 'Window': Failed to read the 'transfer' property from 'StructuredSerializeOptions': The object must have a callable @@iterator property.""#,
            ),
            (
                r"E(() => C(1, {get transfer() { return [new ArrayBuffer(1)]; }}))",
                r"1",
            ),
            (
                r"E(() => C(1, {transfer: [new ArrayBuffer(1)], other: 1}))",
                r"1",
            ),
            (r"E(() => C(1, function () {}))", r"1"),
            (r"E(() => C(1, []))", r"1"),
        ],
    },
    Case {
        name: "errors",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function C(v, o) { return structuredClone(v, o); }
function T(v) { return Object.prototype.toString.call(v); }
function same(a, b) { return Object.is(a, b); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const out = []; for (const T of [Error, TypeError, RangeError, SyntaxError, ReferenceError, EvalError, URIError]) { const c = C(new T('m')); out.push(c.name + ':' + c.message + ':' + (Object.getPrototypeOf(c) === T.prototype) + ':' + (c instanceof T)); } return out; })()",
                r#"["Error:m:true:true", "TypeError:m:true:true", "RangeError:m:true:true", "SyntaxError:m:true:true", "ReferenceError:m:true:true", "EvalError:m:true:true", "URIError:m:true:true"]"#,
            ),
            (
                r"(() => { const e = new Error('hello'); const c = C(e); return [T(c), c !== e, c instanceof Error, c.message, c.name, Object.getOwnPropertyNames(c).join(), typeof c.stack, c.stack === e.stack]; })()",
                r#"["[object Error]", true, true, "hello", "Error", "stack,message", "string", true]"#,
            ),
            (
                r"(() => { const e = new Error('x'); e.name = 'Custom'; const c = C(e); return [c.name, Object.getPrototypeOf(c) === Error.prototype, c.stack === e.stack]; })()",
                r#"["Error", true, true]"#,
            ),
            (
                r"(() => { class MyErr extends TypeError {} const e = new MyErr('q'); const c = C(e); return [c.name, Object.getPrototypeOf(c) === TypeError.prototype, c.message]; })()",
                r#"["TypeError", true, "q"]"#,
            ),
            (
                r"(() => { const e = new Error('x', {cause: {a: 1}}); const c = C(e); return [Object.getOwnPropertyNames(c).join(), c.cause.a, c.cause !== e.cause, Object.getOwnPropertyDescriptor(c, 'cause').enumerable]; })()",
                r#"["stack,message,cause", 1, true, false]"#,
            ),
            (
                r"(() => { const e = new Error('x', {cause: 'str'}); const c = C(e); return [c.cause, 'cause' in c]; })()",
                r#"["str", true]"#,
            ),
            (
                r"(() => { const e = new Error('x'); e.extra = 1; const c = C(e); return [c.extra, Object.getOwnPropertyNames(c).join()]; })()",
                r#"[null, "stack,message"]"#,
            ),
            (
                r"(() => { const e = new Error(); const c = C(e); return [c.message, Object.getOwnPropertyNames(c).join(), Object.prototype.hasOwnProperty.call(c, 'message')]; })()",
                r#"["", "stack", false]"#,
            ),
            (
                r"(() => { const e = new Error('x'); Object.defineProperty(e, 'message', {value: 5}); const c = C(e); return [typeof c.message, c.message]; })()",
                r#"["string", "5"]"#,
            ),
            (
                r"(() => { const e = new Error('x'); e.stack = 'custom stack'; const c = C(e); return [c.stack]; })()",
                r#"["custom stack"]"#,
            ),
            (
                r"(() => { const e = new Error('x'); delete e.stack; const c = C(e); return [typeof c.stack, Object.getOwnPropertyNames(c).join()]; })()",
                r#"["undefined", "stack,message"]"#,
            ),
            (
                r"(() => { const e = new Error('x'); e.stack = 5; const c = C(e); return [typeof c.stack]; })()",
                r#"["undefined"]"#,
            ),
            (
                r"(() => { const e = new Error('x'); const d = Object.getOwnPropertyDescriptor(e, 'stack'); const c = C(e); const dc = Object.getOwnPropertyDescriptor(c, 'stack'); return [!!d, !!dc, dc && dc.enumerable, dc && dc.writable, dc && dc.configurable, dc && ('value' in dc)]; })()",
                r"[true, true, false, null, true, false]",
            ),
            (
                r"(() => { const e = new Error('x'); const c = C(e); return c.stack.split('\n')[0]; })()",
                r#""Error: x""#,
            ),
            (
                r"(() => { const e = new Error('x'); e.name = 'Alt'; const c = C(e); return c.stack.split('\n')[0]; })()",
                r#""Alt: x""#,
            ),
            (
                r"(() => { const e = new AggregateError([1], 'agg'); const c = C(e); return [c.name, Object.getPrototypeOf(c) === Error.prototype, c.message, 'errors' in c]; })()",
                r#"["Error", true, "agg", false]"#,
            ),
            (
                r"(() => { const e = new DOMException('dm', 'NotFoundError'); const c = C(e); return [T(c), c.name, c.message, c.code, c instanceof DOMException]; })()",
                r#"["[object DOMException]", "NotFoundError", "dm", 8, true]"#,
            ),
            (
                r"(() => { const e = new Error('x'); e.name = 'TypeError'; const c = C(e); return [Object.getPrototypeOf(c) === TypeError.prototype, c.name]; })()",
                r#"[true, "TypeError"]"#,
            ),
            (
                r"(() => { const e = new TypeError('x'); Object.setPrototypeOf(e, null); try { const c = C(e); return [c.name]; } catch (err) { return err.constructor.name + ':' + err.message; } })()",
                r#"["Error"]"#,
            ),
            (
                r"(() => { const e = new Error('x'); const o = {e, again: e}; const c = C(o); return c.e === c.again; })()",
                r"true",
            ),
            (
                r"(() => { const e = new Error('x'); e.cause = e; const c = C(e); return [c.cause === c]; })()",
                r"[true]",
            ),
            (
                r"(() => { const e = new Error('x', {cause: new TypeError('inner')}); const c = C(e); return [c.cause.name, c.cause.message, c.cause instanceof TypeError]; })()",
                r#"["TypeError", "inner", true]"#,
            ),
            (
                r"(() => { const e = new Error('x'); Object.defineProperty(e, 'name', {get() { return 'RangeError'; }}); const c = C(e); return [c.name, Object.getPrototypeOf(c) === RangeError.prototype]; })()",
                r#"["RangeError", true]"#,
            ),
            (
                r"(() => { const e = new Error('x'); Object.defineProperty(e, 'cause', {get() { return 7; }, enumerable: false}); const c = C(e); return [c.cause]; })()",
                r"[null]",
            ),
            (
                r"(() => { const e = new Error('x'); e.name = undefined; const c = C(e); return [c.name]; })()",
                r#"["Error"]"#,
            ),
            (
                r"(() => { const e = new Error('x'); e.name = 'rangeerror'; const c = C(e); return [c.name]; })()",
                r#"["Error"]"#,
            ),
        ],
    },
];
