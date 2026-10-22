//! The inline caches of the property sites, the global cache and the
//! shapes of object literals (ADR 0026 section 15, M7 13a): each case
//! fills a cache, changes what the entry depends on, and reads or writes
//! again through the same site. Every case runs normally and in the GC
//! stress mode. The expected outputs come from Node.js 22 (a black box),
//! with `print` for `console.log` and an empty `gc`.

mod common;

use common::{check_cases, on_big_stack, run_in, runtime};

#[test]
fn own_and_prototype_reads() {
    check_cases(READS);
}

#[test]
fn writes_and_adds() {
    check_cases(WRITES);
}

#[test]
fn exotic_objects_at_a_cached_site() {
    check_cases(EXOTIC);
}

#[test]
fn global_bindings() {
    check_cases(GLOBALS);
}

#[test]
fn object_literals_with_inline_caches() {
    check_cases(LITERALS);
}

#[test]
fn reused_shape_slots() {
    check_cases(REUSED_SHAPES);
}

/// A later script declares a global `let` that shadows a property of the
/// global object that a site of an earlier script cached.
#[test]
fn later_script_shadows_a_global_property() {
    for stress in [false, true] {
        let out = on_big_stack(move || {
            let mut rt = runtime(stress);
            let first = run_in(
                &mut rt,
                "globalThis.g = 1; function rg() { return g; } \
                 function wg(v) { g = v; return g; } \
                 print(rg(), rg(), wg(5), wg(6));",
            );
            let second = run_in(
                &mut rt,
                "let g = 2; print(rg(), wg(7), rg(), globalThis.g); \
                 const k = 3; print(typeof k);",
            );
            format!("{first}\n{second}")
        });
        assert_eq!(out, "1 1 5 6\n2 7 7 6\nnumber", "stress: {stress}");
    }
}

const READS: &[(&str, &str)] = &[
    // A shape change after the cache filled: a new key, a deletion
    // (dictionary mode), the key added again.
    (
        "function rd(o) { return o.x; }
         var o = {x: 1}; print(rd(o), rd(o));
         o.y = 2; print(rd(o));
         delete o.x; print(rd(o));
         o.x = 3; print(rd(o), rd({x: 4}));",
        "1 1\n1\nundefined\n3 4",
    ),
    // `Object.setPrototypeOf` on an object of the chain, with and without
    // own properties (the dictionary branch).
    (
        "function rd(o) { return o.m; }
         var a = {m: 'a'}, b = {m: 'b'};
         var p = Object.create(a); var c = Object.create(p);
         print(rd(c), rd(c));
         Object.setPrototypeOf(p, b); print(rd(c));
         Object.setPrototypeOf(p, null); print(rd(c));
         var q = Object.create(a); q.own = 1; var d = Object.create(q);
         print(rd(d), rd(d));
         Object.setPrototypeOf(q, b); print(rd(d));
         Object.setPrototypeOf(d, {m: 'direct'}); print(rd(d));",
        "a a\nb\nundefined\na a\nb\ndirect",
    ),
    // `defineProperty` on the prototype: data to accessor, the getter
    // replaced in place, back to data, non-writable.
    (
        "function P() {} P.prototype.v = 1;
         var o = new P();
         function rd(q) { return q.v; }
         print(rd(o), rd(o));
         Object.defineProperty(P.prototype, 'v', { get() { return 'g1'; }, configurable: true });
         print(rd(o), rd(o));
         Object.defineProperty(P.prototype, 'v', { get() { return 'g2'; }, configurable: true });
         print(rd(o));
         Object.defineProperty(P.prototype, 'v', { value: 'data', writable: false, configurable: true });
         print(rd(o));
         P.prototype.w = 1; print(rd(o));
         delete P.prototype.v; print(rd(o));
         Object.prototype.v = 'top'; print(rd(o));
         delete Object.prototype.v; print(rd(o));",
        "1 1\ng1 g1\ng2\ndata\ndata\nundefined\ntop\nundefined",
    ),
    // A value that changes in place on the prototype.
    (
        "var proto = {n: 0}; var o = Object.create(proto);
         function rd(q) { return q.n; } var s = '';
         for (var i = 0; i < 4; i++) { proto.n = i; s += rd(o); }
         print(s);",
        "0123",
    ),
    // Missing keys: absent, then added at each level.
    (
        "var base = {}; var mid = Object.create(base); var o = Object.create(mid);
         function rd(q) { return q.k; }
         print(rd(o), rd(o));
         base.k = 'base'; print(rd(o));
         mid.k = 'mid'; print(rd(o));
         o.k = 'own'; print(rd(o));",
        "undefined undefined\nbase\nmid\nown",
    ),
    // Own accessors, and an accessor without a getter.
    (
        "var o = { get a() { return this.b * 2; }, b: 1, set c(v) {} };
         function ra(q) { return q.a; } function rc(q) { return q.c; }
         print(ra(o), ra(o), rc(o), rc(o));
         o.b = 5; print(ra(o));",
        "2 2 undefined undefined\n10",
    ),
    // Dictionary mode: a large object, a prototype that leaves the shared
    // shapes, and a site that sees both kinds.
    (
        "var big = {}; for (var i = 0; i < 200; i++) big['k' + i] = i;
         function rd(q) { return q.k7; }
         print(rd(big), rd({k7: 'small'}), rd(big));
         var proto = {k7: 'p', z: 1}; var o = Object.create(proto);
         print(rd(o), rd(o));
         delete proto.z; print(rd(o));
         proto.k7 = 'p2'; print(rd(o));
         delete proto.k7; print(rd(o));
         proto.k7 = 'p3'; print(rd(o));",
        "7 small 7\np p\np\np2\nundefined\np3",
    ),
    // `freeze`, `seal` and `preventExtensions` of the prototype do not
    // change what a read finds.
    (
        "var proto = {x: 1}; var o = Object.create(proto);
         function rd(q) { return q.x; }
         print(rd(o)); Object.freeze(proto); print(rd(o));
         var p2 = {y: 2}; var o2 = Object.create(p2);
         function ry(q) { return q.y; }
         print(ry(o2)); Object.seal(p2); Object.preventExtensions(p2); print(ry(o2));",
        "1\n1\n2\n2",
    ),
    // Inherited methods of built-in prototypes.
    (
        "function f(q) { return q.toString(); }
         print(f({}), f([1, 2]), f({}));
         Object.prototype.toString = function () { return 'patched'; };
         print(f({}), f([3]));
         Array.prototype.toString = function () { return 'arr'; };
         print(f([3]));",
        "[object Object] 1,2 [object Object]\npatched 3\narr",
    ),
];

const WRITES: &[(&str, &str)] = &[
    // Own writes and adds, then the object made non-extensible, sealed
    // and frozen.
    (
        "function add(q) { q.z = 1; return q; }
         add({}); add({});
         var n = Object.preventExtensions({}); add(n);
         print(n.z, add({}).z, Object.keys(add({a: 1})).join());
         function wr(q, v) { q.a = v; return q.a; }
         var o = {a: 0}; print(wr(o, 1), wr(o, 2));
         Object.seal(o); print(wr(o, 3));
         Object.freeze(o); print(wr(o, 4));
         var s = Object.seal({}); add(s); print(s.z);",
        "undefined 1 a,z\n1 2\n3\n3\nundefined",
    ),
    // A strict write to a non-extensible or frozen object throws through
    // a cached site.
    (
        "'use strict';
         function add(q) { q.z = 1; }
         add({}); add({});
         try { add(Object.freeze({})); } catch (e) { print(e.name); }
         function wr(q) { q.a = 2; }
         var o = {a: 1}; wr(o); wr(o); Object.freeze(o);
         try { wr(o); } catch (e) { print(e.name, o.a); }",
        "TypeError\nTypeError 2",
    ),
    // `defineProperty` of an own property to non-writable and to an
    // accessor after the write was cached.
    (
        "function wr(q, v) { q.a = v; return q.a; }
         var o = {a: 0}; print(wr(o, 1), wr(o, 2));
         Object.defineProperty(o, 'a', { writable: false });
         print(wr(o, 3));
         var log = [];
         var p = {a: 0}; wr(p, 1); wr(p, 2);
         Object.defineProperty(p, 'a', { get() { return 'get'; }, set(v) { log.push(v); }, configurable: true });
         print(wr(p, 9), log.join());",
        "1 2\n2\nget 9",
    ),
    // A setter or a read-only property defined on the prototype after an
    // add was cached.
    (
        "function P() {} function init(q) { q.k = 1; return q; }
         init(new P()); init(new P());
         var log = [];
         Object.defineProperty(P.prototype, 'k', { set(v) { log.push(v); }, configurable: true });
         var z = init(new P());
         print(log.join(), Object.keys(z).length);
         Object.defineProperty(P.prototype, 'k', { value: 'ro', writable: false, configurable: true });
         z = init(new P());
         print(z.k, Object.keys(z).length);
         delete P.prototype.k; z = init(new P());
         print(z.k, Object.keys(z).length);
         Object.prototype.k = 'inherited writable'; z = init({});
         print(z.k, Object.keys(z).length);
         delete Object.prototype.k; void 0;",
        "1 0\nro 0\n1 1\n1 1",
    ),
    // Setters on the chain: cached, replaced in place, removed.
    (
        "var log = [];
         var proto = { set s(v) { log.push('a' + v); } };
         var o = Object.create(proto);
         function ws(q, v) { q.s = v; }
         ws(o, 1); ws(o, 2);
         Object.defineProperty(proto, 's', { set(v) { log.push('b' + v); }, configurable: true });
         ws(o, 3);
         Object.defineProperty(proto, 's', { get() { return 1; }, set: undefined, configurable: true });
         ws(o, 4);
         print(log.join(), Object.keys(o).length);",
        "a1,a2,b3 0",
    ),
    // `length` of arrays and of other objects through one site.
    (
        "function sl(q, n) { q.length = n; return q.length; }
         var a = [1, 2, 3];
         print(sl(a, 1), sl(a, 5), a.length, a[0]);
         var o = {}; print(sl(o, 2), sl(o, 3));
         print(sl([1, 2], '1'), sl(a, 0));
         try { sl([], -1); } catch (e) { print(e.name); }
         var f = Object.freeze([1, 2]); print(sl(f, 0));",
        "1 5 5 1\n2 3\n1 0\nRangeError\n2",
    ),
    // `__proto__` through the setter of `Object.prototype`.
    (
        "function sp(q, p) { q.__proto__ = p; return Object.getPrototypeOf(q) === p; }
         var x = {a: 1};
         print(sp({}, x), sp({}, x), sp({}, null), sp({}, Array.prototype));",
        "true true true true",
    ),
    // Compound assignments and updates through one key.
    (
        "var o = {n: 1}; var proto = {m: 10}; var c = Object.create(proto);
         for (var i = 0; i < 3; i++) { o.n += 2; o.n++; c.m += 1; }
         print(o.n, c.m, proto.m, Object.keys(c).join());",
        "10 13 10 m",
    ),
];

const EXOTIC: &[(&str, &str)] = &[
    // A proxy, an array and an inherited value at a site that cached an
    // ordinary object.
    (
        "function rd(q) { return q.x; }
         var list = [{x: 1}, Object.create({x: 2}), {}, new Proxy({x: 4}, {}),
                     new Proxy({}, { get() { return 5; } }), Object.assign([], {x: 6})];
         var out = [];
         for (var r = 0; r < 2; r++) for (var e of list) out.push(rd(e));
         print(out.join());",
        "1,2,,4,5,6,1,2,,4,5,6",
    ),
    // A proxy with the shape of a cached empty object without a prototype.
    (
        "function rd(q) { return q.y; }
         var bare = Object.create(null);
         print(rd(bare), rd(bare), rd(new Proxy({y: 'trap'}, {})));
         function wr(q) { q.y = 1; return q.y; }
         wr(Object.create(null)); wr(Object.create(null));
         var target = {}; var p = new Proxy(target, { set(t, k, v) { t[k] = v * 10; return true; } });
         print(wr(p), target.y);",
        "undefined undefined trap\n10 10",
    ),
    // `length`: an array and an object with an array prototype share the
    // root shape.
    (
        "function len(q) { return q.length; }
         var fake = Object.create(Array.prototype);
         print(len(fake), len([1, 2]), len(fake), len([1]), len({length: 7}), len([1, 2, 3]));
         var withProto = Object.create({length: 'proto'});
         var arr = Object.setPrototypeOf([1, 2], Object.getPrototypeOf(withProto));
         print(len(withProto), len(arr), len(withProto), len(arr));
         print(len(new String('abc')), len({length: 5}), len(new String('ab')));
         print(len(function (a, b) {}), (function () { return len(arguments); })(9));",
        "0 2 0 1 7 3\nproto 2 proto 2\n3 5 2\n2 1",
    ),
    // Typed arrays: `length` without the walk, after a detach and a
    // resize, and a numeric key that an ordinary object with the same
    // prototype inherits.
    (
        "function len(q) { return q.length; }
         var ab = new ArrayBuffer(4, { maxByteLength: 16 });
         var t = new Uint8Array(ab);
         print(len(t), len(t));
         ab.resize(12); print(len(t));
         var fixed = new Uint16Array(new ArrayBuffer(8), 2, 2);
         print(len(fixed));
         ab.transfer(); print(len(t), len(fixed));
         var fake = Object.create(Uint8Array.prototype);
         try { len(fake); } catch (e) { print(e.name); }
         function inf(q) { return q.Infinity; }
         Uint8Array.prototype.Infinity = 'inherited';
         print(inf(fake), inf(fake), inf(new Uint8Array(2)), inf(new Uint8Array(2)));
         function sub(q) { return q.subarray(1).length; }
         print(sub(new Uint8Array(3)), sub(new Uint8Array(5)));
         Object.defineProperty(Object.getPrototypeOf(Uint8Array.prototype), 'length',
             { get() { return 'patched'; }, configurable: true });
         print(len(new Uint8Array(3)));",
        "4 4\n12\n2\n0 2\nTypeError\ninherited inherited undefined undefined\n2 4\npatched",
    ),
    // String objects and arguments objects at a site.
    (
        "function rd(q) { return q.foo; }
         String.prototype.foo = 'sp';
         print(rd(new String('x')), rd(new String('y')), rd('prim'));
         function args() { return rd(arguments); }
         Object.prototype.foo = 'op';
         print(args(), args());
         delete Object.prototype.foo; delete String.prototype.foo;
         print(rd(new String('x')), args());",
        "sp sp sp\nop op\nundefined undefined",
    ),
];

const GLOBALS: &[(&str, &str)] = &[
    // A global var, a property of the global object's prototype, a
    // sloppy assignment that creates an own property, `delete`.
    (
        "var v = 1; function rv() { return v; } function wv(x) { v = x; }
         print(rv(), rv()); wv(2); wv(3); print(rv());
         Object.prototype.zz = 'proto';
         function rz() { return zz; }
         print(rz(), rz());
         zz = 'own'; print(rz());
         delete globalThis.zz; print(rz());
         delete Object.prototype.zz;
         try { rz(); } catch (e) { print(e.name); }",
        "1 1\n3\nproto proto\nown\nproto\nReferenceError",
    ),
    // Global lexical bindings: the dead zone, `const`, `let`.
    (
        "function early() { return T; }
         try { early(); } catch (e) { print(e.name); }
         let T = 1; print(early(), early());
         T = 2; print(early());
         const C = 1; function sc() { C = 2; }
         for (var i = 0; i < 2; i++) { try { sc(); } catch (e) { print(e.name, C); } }
         function wt(x) { T = x; return T; }
         print(wt(5), wt(6));",
        "ReferenceError\n1 1\n2\nTypeError 1\nTypeError 1\n5 6",
    ),
    // `with` and direct `eval` that declares a `var`.
    (
        "var w = 'global';
         function rw(o) { with (o) { return w; } }
         print(rw({}), rw({w: 'with'}), rw({}));
         function rev() { return typeof ev === 'undefined' ? 'none' : ev; }
         function rev2() { return ev; }
         try { rev2(); } catch (e) { print(e.name); }
         eval('var ev = 5'); print(rev(), rev2(), rev2());
         var gv = 'g';
         function f() { var r = gv; eval('var gv = \"local\"'); return r + gv; }
         print(f(), f(), gv);",
        "global with global\nReferenceError\n5 5 5\nglocal glocal g",
    ),
    // The global object in dictionary mode, an accessor on it, and a
    // global replaced by `defineProperty`.
    (
        "var a0 = 'first'; function ra() { return a0; } function wa(x) { a0 = x; }
         print(ra(), ra());
         for (var i = 0; i < 300; i++) globalThis['many' + i] = i;
         print(ra()); wa('second'); print(ra(), many299);
         delete globalThis.many5; print(ra());
         Object.defineProperty(globalThis, 'acc', { get() { return 'A'; }, configurable: true });
         function racc() { return acc; }
         print(racc(), racc());
         Object.defineProperty(globalThis, 'acc', { value: 'D', writable: true, configurable: true });
         print(racc());
         Object.defineProperty(globalThis, 'a0', { value: 'ro', writable: false });
         wa('third'); print(ra());",
        "first first\nfirst\nsecond 299\nsecond\nA A\nD\nro",
    ),
    // Function declarations at script level and their redefinition.
    (
        "function fn() { return 1; }
         function call() { return fn(); }
         print(call(), call());
         fn = function () { return 2; }; print(call());
         globalThis.fn = function () { return 3; }; print(call());",
        "1 1\n2\n3",
    ),
];

const LITERALS: &[(&str, &str)] = &[
    // Duplicate keys keep the first position and the last value; methods
    // have the literal as their home object.
    (
        "var o = {a: 1, b: 2, a: 3};
         print(JSON.stringify(o), Object.keys(o).join());
         var m = { x: 1, f() { return super.hasOwnProperty === Object.prototype.hasOwnProperty; } };
         print(m.f(), Object.keys(m).join());
         var named = { fn: function () {}, arrow: () => 0 };
         print(named.fn.name, named.arrow.name);
         var d = Object.getOwnPropertyDescriptor({k: 1}, 'k');
         print(d.writable, d.enumerable, d.configurable);",
        "{\"a\":3,\"b\":2} a,b\ntrue x,f\nfn arrow\ntrue true true",
    ),
    // The general path: `__proto__`, computed and numeric keys, spread,
    // accessors, and more keys than a shared shape holds.
    (
        "var p = {inherited: 1};
         var a = {__proto__: p, own: 2};
         print(a.inherited, Object.keys(a).join());
         var k = 'dyn'; var b = {[k]: 1, 2: 'two', x: 3, ...{s: 4}, get g() { return 5; }};
         print(Object.keys(b).join(), b.g);
         var src = 'var big = {'; for (var i = 0; i < 140; i++) src += 'k' + i + ': ' + i + ', ';
         eval(src + '};');
         print(Object.keys(big).length, big.k139);
         var sh = {__proto__}; print(Object.keys(sh).join(), Object.getPrototypeOf(sh) === Object.prototype);",
        "1 own\n2,dyn,x,s,g 5\n140 139\n__proto__ true",
    ),
    // A literal in a generator that suspends between its values, and a
    // literal whose value throws.
    (
        "function* g() { var o = {p: yield 1, q: yield 2}; return o; }
         var it = g(); it.next(); it.next('A');
         print(JSON.stringify(it.next('B').value));
         function mk(x) { return {a: x, b: x.y.z, c: 1}; }
         try { mk({}); } catch (e) { print(e.name); }
         print(JSON.stringify(mk({y: {z: 2}})));",
        "{\"p\":\"A\",\"q\":\"B\"}\nTypeError\n{\"a\":{\"y\":{\"z\":2}},\"b\":2,\"c\":1}",
    ),
    // Many objects from one literal, with collections between them, and
    // `Object.prototype` changed between evaluations.
    (
        "function mk(i) { return {a: i, b: 'x' + i, c: {d: i}}; }
         var keep = []; var sum = 0;
         for (var i = 0; i < 2000; i++) { var o = mk(i); sum += o.c.d; if (i % 100 == 0) keep.push(o); }
         gc();
         print(sum, keep.length, keep[19].b, Object.keys(mk(1)).join());
         Object.prototype.extra = 1; print(mk(2).extra, Object.keys({}).length);
         delete Object.prototype.extra; void 0;",
        "1999000 20 x1900 a,b,c\n1 0",
    ),
];

const REUSED_SHAPES: &[(&str, &str)] = &[
    // Objects with one-off shapes die and their shape slots are reused
    // by shapes with other layouts; the sites must not mistake them.
    (
        "function rd(q) { return q.a; } function wr(q) { q.a = 'w'; return q; }
         var total = 0;
         for (var i = 0; i < 300; i++) {
           var o = {}; o['k' + i] = 1; o.a = i; total += rd(o); wr(o);
           if (i % 50 == 0) gc();
           var p = {}; p.a = 1; p['m' + i] = 2; p.b = 3;
           total += rd(p);
           var q = {}; q['n' + i] = 4;
           if (rd(q) !== undefined) total = -1e9;
           var r = {}; r['r' + i] = 1; wr(r);
           if (Object.keys(r).join() !== 'r' + i + ',a') total = -1e9;
         }
         print(total);",
        "45150",
    ),
];
