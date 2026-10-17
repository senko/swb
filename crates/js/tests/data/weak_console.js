// console.log of the weak objects (the expected output is that of
// Node.js 22, `node weak_console.js`).
const o = {};
const wm = new WeakMap([[o, 1]]);
console.log(wm);
console.log(new WeakSet([o]));
console.log(new WeakRef(o));
console.log(new FinalizationRegistry(() => {}));
console.log({ a: wm, b: new WeakSet() });
console.log([new WeakRef(o)]);
class M extends WeakMap {}
console.log(new M());
wm.x = 1;
console.log(wm);
console.log(new WeakRef({ a: 1, b: { c: { d: 1 } } }));
console.log(new WeakRef(Symbol('q')));
console.log({ d: { e: { f: new WeakMap() } } });
const fr = new FinalizationRegistry(() => {});
fr.y = 2;
console.log(fr);
class F extends FinalizationRegistry {}
console.log(new F(() => {}));
console.log(Object.create(WeakMap.prototype));
console.log({ d: { e: { f: new M(), g: new WeakRef({}), h: new FinalizationRegistry(() => {}), i: new WeakSet() } } });
const w2 = new WeakMap();
Object.setPrototypeOf(w2, null);
console.log(w2);
const w3 = new WeakSet();
Object.setPrototypeOf(w3, null);
console.log(w3);
const r = new WeakRef({});
Object.setPrototypeOf(r, null);
console.log(r);
const w4 = new WeakMap();
w4.a = { b: { c: 1 } };
console.log({ x: w4 });
const r2 = new WeakRef({});
r2.x = 1;
console.log(r2);
const s = new WeakSet();
Object.defineProperty(s, 'hidden', { value: 1 });
s[Symbol('q')] = 2;
console.log(s);
console.log({ a: { b: { c: { d: new WeakRef({}) } } } });
console.log({ a: { b: { c: new FinalizationRegistry(() => {}) } } });
console.log(new Proxy(new WeakMap(), {}));
Object.defineProperty(WeakMap.prototype, Symbol.toStringTag, { value: 'X' });
console.log(new WeakMap());
