// console.log of typed arrays; the expected output (typed_console.expected)
// is that of Node.js 22 (`node typed_console.js`). Node.js groups the
// numbers of an array of more than six elements into columns and swb does
// not (console.rs), so the long arrays here are printed one level down,
// where Node.js prints them on one line too when they fit.
console.log(new Uint8Array([1, 2, 3]));
console.log(new Uint8Array(0));
console.log(new Int8Array([-1, 127, -128]));
console.log(new Uint8ClampedArray([300, -5]));
console.log(new Int16Array([1, -1]), new Uint16Array([65535]));
console.log(new Int32Array([-2147483648]), new Uint32Array([4294967295]));
console.log(new Float32Array([0.1, -0]));
console.log(new Float64Array([1.5, -0, NaN, Infinity]));
console.log(new BigInt64Array([1n, -2n]));
console.log(new BigUint64Array([2n ** 64n - 1n]));
console.log(new BigInt64Array(0));
console.log([new Uint8Array(2)]);
console.log({ a: new Int16Array([1, -1]) });
console.log({ a: { b: { c: new Uint8Array(2) } } });
console.log({ a: { b: { c: { d: new Uint8Array(2) } } } });
var t = new Uint8Array([1, 2]); t.x = 5; console.log(t);
var d = new Uint8Array(4); d.buffer.transfer(); console.log(d);
var b = new ArrayBuffer(4, { maxByteLength: 8 }); var o = new Uint8Array(b, 2); b.resize(1); console.log(o);
class X extends Uint8Array {}
console.log(new X(2));
var p = new Uint8Array(2); Object.setPrototypeOf(p, null); console.log(p);
console.log(new Proxy(new Uint8Array(2), {}));
console.log(Uint8Array.prototype);
console.log(Object.getPrototypeOf(Int8Array).prototype);
console.log(Uint8Array, Object.getPrototypeOf(Int8Array));
var q = new Float64Array(2); q[Symbol.toStringTag] = 'Q'; console.log(q);
var r = new Uint8Array(2); Object.defineProperty(r, 'y', { value: 1, enumerable: false }); console.log(r);
var s = new Uint8Array(2); Object.defineProperty(s, 'g', { get() { return 1; }, enumerable: true }); console.log(s);
var sy = new Uint8Array(1); sy[Symbol('k')] = 2; console.log(sy);
var rb = new ArrayBuffer(2, { maxByteLength: 8 }); var tr = new Uint8Array(rb); console.log(tr); rb.resize(3); console.log(tr);
console.log(new Uint8Array(new ArrayBuffer(8), 2, 3));
console.log(new Uint8Array(2).buffer);
var w = new Uint8Array(3); w.foo = new Uint8Array(1); console.log(w);
console.log(Object.create(new Uint8Array(2)));
console.log({ long: new Uint8Array(150) }.long.length, [new Uint16Array(5)]);
console.log({ k: new Float64Array([0.5, 0.25, 0.125, 1e21, 1e-7, -1.5]) });
console.log(new DataView(new Uint8Array([1, 2]).buffer));
console.log(String(Object.prototype.toString.call(new Uint8Array(1))));
