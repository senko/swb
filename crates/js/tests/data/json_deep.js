// Deep and wide JSON structures (small enough for the GC stress mode; the
// large ones are in tests/language_hostile.rs). The output is the same in
// Node.js 22.
var out = [];
function depthOfArrays(v) { var n = 0; while (Array.isArray(v) && v.length) { v = v[0]; n++; } return n; }
function depthOfObjects(v) { var n = 0; while (v && typeof v === 'object' && 'a' in v) { v = v.a; n++; } return n; }

// Nested arrays and objects.
var text = '['.repeat(1500) + '1' + ']'.repeat(1500);
var v = JSON.parse(text);
out.push(depthOfArrays(v), JSON.stringify(v) === text);
text = '{"a":'.repeat(1200) + 'null' + '}'.repeat(1200);
v = JSON.parse(text);
out.push(depthOfObjects(v), JSON.stringify(v) === text);
out.push(JSON.stringify(v, null, 1).length, JSON.stringify(JSON.parse(text, function (k, x) { return x; })) === text);

// The reviver sees every level, innermost first.
var seen = 0;
v = JSON.parse('['.repeat(300) + '5' + ']'.repeat(300), function (k, x) { seen++; return x; });
out.push(seen, depthOfArrays(v));

// Wide arrays and objects.
var items = [];
for (var i = 0; i < 2000; i++) items.push(i % 7 === 0 ? '"s' + i + '"' : String(i * 1.5));
text = '[' + items.join(',') + ']';
v = JSON.parse(text);
out.push(v.length, v[1], v[7], v[1999], JSON.stringify(v) === text);
var members = [];
for (i = 0; i < 2000; i++) members.push('"k' + i + '":' + i);
v = JSON.parse('{' + members.join(',') + '}');
out.push(Object.keys(v).length, v.k0, v.k1999, JSON.stringify(v).length);
v = JSON.parse('{"1":1,"0":0,"b":2,"a":3,"01":4,"__proto__":5,"b":6}');
out.push(Object.keys(v).join(), JSON.stringify(v), Object.getPrototypeOf(v) === Object.prototype);

// Long strings with escapes and unpaired surrogates.
var chunk = 'abé中😀\u0001"\\/\n\ud800';
var long = chunk.repeat(500);
var enc = JSON.stringify(long);
out.push(enc.length, JSON.parse(enc) === long, JSON.stringify(JSON.parse(JSON.stringify([long, long]))[1]) === enc);
out.push(JSON.stringify('𐀀\udc00\ud800'));

// Indentation of a nested structure.
v = { a: [1, { b: [[], {}, [2]] }], c: 'd' };
out.push(JSON.stringify(v, null, 3));
out.push(JSON.stringify(v, null, '\t').split('\n').length);

// Numbers.
var numbers = ['0', '-0', '1', '-1', '0.5', '1e3', '1E-3', '123456789012345678', '1.7976931348623157e308', '5e-324', '2.2250738585072014e-308',
  '0.1', '0.7', '1e21', '1e22', '1e23', '9007199254740993', '4.35', '0.000001', '0.0000001', '123.456e-5', '1e400', '-1e400', '1e-400'];
out.push(numbers.map(function (n) { return String(JSON.parse(n)); }).join(' '));
out.push(JSON.stringify(numbers.map(function (n) { return JSON.parse(n); })));
console.log(out.join('\n'));
