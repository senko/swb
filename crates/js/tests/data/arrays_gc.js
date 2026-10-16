// Array methods with collections inside callbacks, comparators, getters and
// iterators, sparse arrays of length 2^32 - 1 and mapped arguments (M7 6a).
// The expected output (arrays_gc.expected) is that of Node.js 22, except for
// the sparse cases of length 2^32 - 1 and 2^53 - 1: V8 loops over the length
// there (minutes), so their lines were checked by hand against the steps of
// the specification.
var gc = typeof gc === 'function' ? gc : function () {};
function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
// Comparators that allocate and collect: the values being sorted stay alive.
t(function () { var a = []; for (var i = 0; i < 300; i++) a.push({ k: (i * 7919) % 300, s: 'v' + i }); a.sort(function (x, y) { junk(3); if (x.k % 50 === 0) gc(); return x.k - y.k; }); return a.map(function (o) { return o.s; }).slice(0, 5).join() + ' ' + a[299].k; });
t(function () { var a = []; for (var i = 0; i < 200; i++) a.push('s' + ((i * 31) % 200)); var r = a.toSorted(function (x, y) { gc(); return x < y ? -1 : x > y ? 1 : 0; }); return r.slice(0, 4).join() + ' ' + (a[0] === 's0'); });
t(function () { var a = []; for (var i = 0; i < 100; i++) a.push({ toString: function () { junk(2); gc(); return String.fromCharCode(97 + (this.n % 26)); }, n: (i * 13) % 100 }); a.sort(); return a.slice(0, 3).map(function (o) { return o.n; }).join(); });
t(function () { var a = [5, 1, 4, 2, 3]; return a.sort(function (x, y) { a.length = 0; gc(); return x - y; }).join() + ' ' + a.length; });
t(function () { var a = [3, 1, 2]; a.sort(function (x, y) { a.push(junk(1)); gc(); return x - y; }); return a.slice(0, 3).join() + " " + (a.length > 3); });
// Callbacks that allocate and collect.
t(function () { var a = []; for (var i = 0; i < 100; i++) a.push('e' + i); var m = a.map(function (x, i) { if (i % 10 === 0) gc(); return x + '!' + junk(2); }); return m[0] + m[99]; });
t(function () { var a = []; for (var i = 0; i < 100; i++) a.push({ v: i }); var f = a.filter(function (o) { gc(); return o.v % 3 === 0; }); return f.length + ' ' + f[33].v; });
t(function () { var a = []; for (var i = 0; i < 50; i++) a.push('r' + i); return a.reduce(function (acc, x) { gc(); return acc + x.length; }, 0) + ' ' + a.reduceRight(function (acc, x) { junk(1); return acc + x.charAt(1); }, ''); });
t(function () { var a = [[1, [2, [3]]], 'x', [[['y']]]]; return a.flatMap(function (x) { gc(); return [x, 'm']; }).length + ' ' + a.flat(Infinity).join(); });
t(function () { var src = { length: 4 }; Object.defineProperty(src, 1, { get: function () { gc(); return ['g', junk(1)]; } }); return Array.prototype.flat.call(src).join('|'); });
t(function () { var it = { i: 0 }; it[Symbol.iterator] = function () { return this; }; it.next = function () { gc(); return this.i < 5 ? { value: 'v' + this.i++, done: false } : { done: true }; }; return Array.from(it, function (v) { junk(1); return v + v; }).join(); });
t(function () { var a = []; for (var i = 0; i < 64; i++) a.push('c' + i); var b = a.concat(a, { length: 2, 0: 'q', 1: 'w', [Symbol.isConcatSpreadable]: true }, 'z'); gc(); return b.length + ' ' + b[64] + b[128] + b[130]; });
t(function () { var s = 'a,bb,ccc,dddd'; var r = []; for (var i = 0; i < 20; i++) { var p = s.split(','); gc(); r.push(p.length + p[3]); p[0] = 'changed'; } return r[19] + ' ' + s.split(',')[0]; });
t(function () { var a = [1, , 3, , 5]; var log = []; a.forEach(function (x, i) { gc(); log.push(i + ':' + x); if (i === 0) a[1] = 'added'; }); return log.join(); });
t(function () { var a = []; a[2 ** 32 - 2] = 'end'; a[10] = 'ten'; var log = []; a.forEach(function (x, i) { gc(); log.push(i); }); return log.join() + ' ' + a.indexOf('end') + ' ' + a.lastIndexOf('ten') + ' ' + a.includes(undefined); });
t(function () { var a = []; a[2 ** 32 - 2] = 1; a[3] = 2; a.reverse(); return Object.keys(a).join(); });
t(function () { var a = []; a[2 ** 32 - 2] = 1; a[3] = 2; a.shift(); a.unshift('u'); return Object.keys(a).join() + ' ' + a.length; });
t(function () { var a = []; a.length = 2 ** 32 - 1; a[7] = 'q'; a.splice(2, 3, 'x'); return Object.keys(a).join() + ' ' + a.length; });
t(function () { var a = []; a.length = 2 ** 32 - 1; a[100] = 'p'; a[5] = 'q'; a.sort(); return Object.keys(a).join() + ' ' + a.length; });
t(function () { var a = []; a.length = 2 ** 32 - 1; a[100] = 'p'; a.copyWithin(0, 90, 110); return Object.keys(a).join(); });
t(function () { var o = { length: 2 ** 53 - 1 }; o[2 ** 53 - 2] = 'last'; o[5] = 'five'; return [Array.prototype.lastIndexOf.call(o, 'five'), Array.prototype.indexOf.call(o, 'last'), Array.prototype.at.call(o, -1)].join(); });
// Mapped arguments: the methods write through to the parameters.
t(function () { return (function (a, b, c) { Array.prototype.sort.call(arguments); gc(); return [a, b, c].join(); })('z', 'x', 'y'); });
t(function () { return (function (a, b, c) { Array.prototype.fill.call(arguments, 'f', 1); return [a, b, c].join(); })(1, 2, 3); });
t(function () { return (function (a, b, c) { Array.prototype.splice.call(arguments, 0, 2, 'p'); return [a, b, c, arguments.length].join(); })(1, 2, 3); });
t(function () { return (function (a, b, c) { Array.prototype.reverse.call(arguments); a = 'A'; return [arguments[0], b, c].join(); })(1, 2, 3); });
// Results that are copies.
t(function () { var a = [3, 1, 2]; var b = a.toSorted(); var c = a.toReversed(); var d = a.with(0, 9); var e = a.toSpliced(1, 1); return [a, b, c, d, e].join(' '); });
t(function () { var a = [1, 2, 3]; var r = a.slice(); r.push(4); return a.length + ' ' + r.length; });
