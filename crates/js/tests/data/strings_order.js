var log = [];
function L(name, v) { return { toString() { log.push(name); return v; }, valueOf() { log.push(name + '.valueOf'); return typeof v === 'number' ? v : 1; } }; }
function N(name, v) { return { valueOf() { log.push(name); return v; }, toString() { log.push(name + '.toString'); return 'x'; } }; }
function t(f) { log = []; var r; try { r = f(); } catch (e) { r = e.constructor.name + ': ' + e.message; } print(String(r) + ' | ' + log.join(',')); }
t(function () { return String.prototype.padStart.call(L('this', 'ab'), N('max', 5), L('fill', '12')); });
t(function () { return String.prototype.padEnd.call(L('this', 'ab'), N('max', 1), L('fill', '12')); });
t(function () { return String.prototype.slice.call(L('this', 'abcdef'), N('a', 1), N('b', 4)); });
t(function () { return String.prototype.substring.call(L('this', 'abcdef'), N('a', 4), N('b', 1)); });
t(function () { return String.prototype.substr.call(L('this', 'abcdef'), N('a', 1), N('b', 2)); });
t(function () { return String.prototype.indexOf.call(L('this', 'abcdef'), L('s', 'c'), N('p', 1)); });
t(function () { return String.prototype.lastIndexOf.call(L('this', 'abcabc'), L('s', 'c'), N('p', 4)); });
t(function () { return String.prototype.includes.call(L('this', 'abcdef'), L('s', 'c'), N('p', 1)); });
t(function () { return String.prototype.startsWith.call(L('this', 'abcdef'), L('s', 'c'), N('p', 2)); });
t(function () { return String.prototype.endsWith.call(L('this', 'abcdef'), L('s', 'c'), N('p', 3)); });
t(function () { return String.prototype.split.call(L('this', 'a,b,c'), L('sep', ','), N('lim', 2)); });
t(function () { return String.prototype.replace.call(L('this', 'abc'), L('s', 'b'), L('r', 'X')); });
t(function () { return String.prototype.replaceAll.call(L('this', 'abcb'), L('s', 'b'), L('r', 'X')); });
t(function () { return String.prototype.repeat.call(L('this', 'ab'), N('n', 2)); });
t(function () { return String.prototype.at.call(L('this', 'ab'), N('i', 1)); });
t(function () { return String.prototype.concat.call(L('this', 'ab'), L('a', 'c'), L('b', 'd')); });
t(function () { return String.prototype.charCodeAt.call(L('this', 'ab'), N('i', 1)); });
t(function () { return String.prototype.codePointAt.call(L('this', 'ab'), N('i', 1)); });
t(function () { return String.prototype.anchor.call(L('this', 'ab'), L('v', 'q')); });
t(function () { return String.fromCharCode(N('a', 65), N('b', 66)); });
t(function () { return String.fromCodePoint(N('a', 65), N('b', 66)); });
t(function () { return String.raw({ raw: { length: L('len', 2), get 0() { log.push('get0'); return L('l0', 'a'); }, get 1() { log.push('get1'); return L('l1', 'b'); } } }, L('s0', 'X'), L('s1', 'Y')); });
t(function () { return encodeURIComponent(L('s', 'a b')); });
t(function () { return decodeURI(L('s', '%41')); });
t(function () { return escape(L('s', 'a b')); });
t(function () { return unescape(L('s', '%41')); });
// Symbol.replace / split dispatch.
t(function () { var o = { get [Symbol.replace]() { log.push('get'); return function (a, b) { log.push('call:' + (this === o) + ':' + a + ':' + b); return 'R'; }; } }; return 'abc'.replace(o, 'Z'); });
t(function () { var o = { [Symbol.replace]: function (a, b) { return 'R:' + a + ':' + b; } }; return 'abc'.replaceAll(o, 'Z'); });
t(function () { var o = { [Symbol.split]: function (a, b) { return 'S:' + a + ':' + b; } }; return 'abc'.split(o, 3); });
t(function () { var o = { [Symbol.match]: true, flags: 'gi', [Symbol.replace]: function (a, b) { return 'RA'; } }; return 'abc'.replaceAll(o, 'Z'); });
t(function () { var o = { [Symbol.match]: true, flags: 'i', [Symbol.replace]: function (a, b) { return 'RA'; } }; return 'abc'.replaceAll(o, 'Z'); });
t(function () { var o = { [Symbol.match]: true, flags: undefined }; return 'abc'.replaceAll(o, 'Z'); });
t(function () { var o = { [Symbol.match]: true, flags: 'g', toString() { return 'b'; } }; return 'abc'.replaceAll(o, 'Z'); });
t(function () { var o = { [Symbol.match]: true, toString() { return '/b/'; } }; return 'a/b/c'.startsWith(o); });
t(function () { return 'abc'.replace('b', function () { return { toString() { log.push('cb'); return 'Z'; } }; }); });
t(function () { return 'abcabc'.replaceAll('b', function (m, p, s) { log.push(p); return arguments.length + m + s; }); });
t(function () { return 'abc'.split('', 2).length + ',' + 'abc'.split('', 0).length + ',' + 'abc'.split('', 9).length; });
t(function () { var it = 'a😀'[Symbol.iterator](); var r = it.next(); var s = it.next(); var d = it.next(); return r.value.length + ',' + s.value.length + ',' + s.done + ',' + d.done + ',' + typeof d.value; });
t(function () { var a = [], s = 'x\uD800y\uDC00z😀'; for (var c of s) a.push(c.length); return a.join(); });
t(function () { var [a, b, ...r] = 'hello'; return a + b + r.join('-'); });
t(function () { var s = new String('ab'); return s.padStart(4, 'x') + s.at(-1) + s.split('').length; });
