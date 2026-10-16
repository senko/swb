// The `show` function of tools/probes/js-strings.json without JSON.
function q(s) {
  var o = '"';
  for (var i = 0; i < s.length; i++) {
    var c = s.charCodeAt(i);
    var ch = s.charAt(i);
    var lone = c >= 0xD800 && c <= 0xDFFF &&
      !(c < 0xDC00 && i + 1 < s.length && s.charCodeAt(i + 1) >= 0xDC00 && s.charCodeAt(i + 1) < 0xE000) &&
      !(c >= 0xDC00 && i > 0 && s.charCodeAt(i - 1) >= 0xD800 && s.charCodeAt(i - 1) < 0xDC00);
    if (ch === '"') o += '\\"';
    else if (ch === '\\') o += '\\\\';
    else if (c === 10) o += '\\n';
    else if (c === 13) o += '\\r';
    else if (c === 9) o += '\\t';
    else if (c === 8) o += '\\b';
    else if (c === 12) o += '\\f';
    else if (c < 32 || lone) { var h = c.toString(16); o += '\\u' + '0000'.slice(h.length) + h; }
    else o += ch;
  }
  return o + '"';
}
function show(v) {
  if (typeof v === 'string') return q(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'function') return 'function ' + v.name;
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (v && typeof v === 'object') {
    if (Array.isArray(v)) {
      var parts = [];
      for (var i = 0; i < v.length; i++) parts.push(show2(v[i]));
      return '[' + parts.join(',') + ']';
    }
    var keys = Object.keys(v);
    var p = [];
    for (var k of keys) p.push(q(k) + ':' + show2(v[k]));
    return '{' + p.join(',') + '}';
  }
  return String(v);
}
function show2(v) {
  if (v === undefined) return 'null';
  if (typeof v === 'symbol') return 'null';
  if (typeof v === 'number' && !isFinite(v)) return 'null';
  return show(v);
}
var SNIPPETS = ["String.prototype.toLowerCase.call(null)",
"String.prototype.toUpperCase.call(undefined)",
"String.prototype.toLocaleLowerCase.call(null)",
"String.prototype.toLocaleUpperCase.call(undefined)",
"String.prototype.normalize.call(null)",
"String.prototype.localeCompare.call(undefined, 'a')",
"String.prototype.localeCompare.call('a', Symbol())",
"String.prototype.normalize.call('a', Symbol())",
"'a'.normalize('x')",
"'a'.normalize('nfc')",
"'a'.normalize('NFC ')",
"'a'.normalize(null)",
"'a'.normalize(undefined)",
"'a'.normalize({ toString() { return 'NFD'; } })",
"'a'.normalize('')",
"'\\u00e9'.normalize('NFD').length",
"'e\\u0301'.normalize().length",
"'e\\u0301'.normalize('NFC') === '\\u00e9'",
"'\\u1e9b\\u0323'.normalize('NFC').length + ' ' + '\\u1e9b\\u0323'.normalize('NFD').length + ' ' + '\\u1e9b\\u0323'.normalize('NFKC').length + ' ' + '\\u1e9b\\u0323'.normalize('NFKD').length",
"'\\ufdfa'.normalize('NFKD').length",
"'\\ufdfa'.normalize('NFKC').length",
"'\\ufdfa'.normalize('NFC').length",
"'\\u0958\\u212b\\u2126\\u0344\\u0f73'.normalize('NFC').length",
"(() => { const r = []; for (const c of '\\ud800a\\udc00\\ud83d\\ude00'.normalize('NFD')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of '\\ud800a\\udc00\\ud83d\\ude00'.normalize('NFKC')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of '\\uac01\\u1100\\u1161\\u11a8\\ud7a3'.normalize('NFD')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of '\\u1100\\u1161\\u11a8\\u1112\\u1175\\u11c2'.normalize('NFC')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of 'a\\u0323\\u0307\\u0301\\u0300\\u0315\\u031b\\u0316'.normalize('NFC')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of 'a\\u0323\\u0307\\u0301\\u0300\\u0315\\u031b\\u0316'.normalize('NFD')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of '\\u01d5\\u1e0b\\u0323\\u0b47\\u0b3e\\u0d46\\u0d3e\\u{1d15e}\\u{1d1bb}'.normalize('NFC')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of '\\ud834\\udd5e\\ud834\\uddbb\\ud834\\uddbc'.normalize('NFC')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const r = []; for (const c of '\\ud834\\udd5e\\ud834\\uddbb\\ud834\\uddbc'.normalize('NFD')) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"'\\u00c5'.normalize('NFD') === 'A\\u030a'",
"'\\u00a0\\u00b2\\u2160\\ufb01\\u00bd'.normalize('NFKC')",
"'abc'.normalize('NFD') === 'abc'",
"'\\u00e9'.normalize === String.prototype.normalize",
"'ABC'.toLowerCase()",
"'abc'.toUpperCase()",
"'\\u00c0\\u00de\\u00d7\\u00df\\u00b5\\u00ff'.toLowerCase()",
"'\\u00e0\\u00fe\\u00f7\\u00df\\u00b5\\u00ff'.toUpperCase()",
"'\\u0130'.toLowerCase().length",
"'i\\u0307'.toUpperCase()",
"'\\u0131'.toUpperCase()",
"'\\u017f'.toUpperCase()",
"'\\u212a'.toLowerCase()",
"'\\u212a'.toUpperCase()",
"'\\u1e9e'.toLowerCase()",
"'\\u0390\\u03b0\\u0149\\u01f0\\u1e96\\u1fb3\\u1f88'.toUpperCase().length",
"(() => { const r = []; for (const c of '\\u0390\\u03b0\\u0149\\u01f0\\u1e96\\u1fb3\\u1f88\\u1f80\\u1ffc\\u1fb7'.toUpperCase()) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"'\\ufb00\\ufb03\\ufb06\\u0587\\ufb13'.toUpperCase()",
"'\\u01c5\\u01c8\\u01cb\\u01f2'.toUpperCase() + '\\u01c5\\u01c8\\u01cb\\u01f2'.toLowerCase()",
"'\\u10d0\\u1c90\\u2c30\\u2c00\\ua64a\\ua64b'.toUpperCase() + '\\u10d0\\u1c90\\u2c30\\u2c00'.toLowerCase()",
"'\\u{10400}\\u{10428}\\u{1e900}\\u{1e922}\\u{10d50}\\u{10d70}\\u{16ea0}\\u{16ebb}'.toLowerCase().length",
"(() => { const r = []; for (const c of '\\u{10400}\\u{10428}\\u{1e900}\\u{1e922}\\u{10d50}\\u{10d70}\\u{16ea0}\\u{16ebb}'.toUpperCase()) r.push(c.codePointAt(0).toString(16)); return r.join(); })()",
"(() => { const s = '\\ud801\\ud801\\udc28\\udc28a\\ud801'.toUpperCase(); const r = []; for (let i = 0; i < s.length; i++) r.push(s.charCodeAt(i).toString(16)); return r.join(); })()",
"(() => { const s = '\\ud801\\ud801\\udc00\\udc28a\\ud801'.toLowerCase(); const r = []; for (let i = 0; i < s.length; i++) r.push(s.charCodeAt(i).toString(16)); return r.join(); })()",
"'\\u0391\\u03a3'.toLowerCase() === '\\u03b1\\u03c2'",
"'\\u03a3'.toLowerCase() === '\\u03c3'",
"'\\u0391\\u03a3\\u0392'.toLowerCase() === '\\u03b1\\u03c3\\u03b2'",
"'A.\\u03a3'.toLowerCase() === 'a.\\u03c2'",
"\"A'\\u03a3\".toLowerCase() === \"a'\\u03c2\"",
"\"A\\u03a3'B\".toLowerCase() === \"a\\u03c3'b\"",
"\"A\\u03a3'.\".toLowerCase() === \"a\\u03c2'.\"",
"'A\\u03a3\\u0301'.toLowerCase() === 'a\\u03c2\\u0301'",
"'A\\u03a3\\u0301B'.toLowerCase() === 'a\\u03c3\\u0301b'",
"'1\\u03a3'.toLowerCase() === '1\\u03c3'",
"' \\u03a3'.toLowerCase() === ' \\u03c3'",
"'A \\u03a3'.toLowerCase() === 'a \\u03c3'",
"'A\\u03a3\\u03a3'.toLowerCase() === 'a\\u03c3\\u03c2'",
"'\\u03a3\\u03a3'.toLowerCase() === '\\u03c3\\u03c3'",
"'A\\u00ad\\u03a3'.toLowerCase() === 'a\\u00ad\\u03c2'",
"'A\\u200b\\u03a3'.toLowerCase() === 'a\\u200b\\u03c3'",
"'A\\u200c\\u03a3'.toLowerCase() === 'a\\u200c\\u03c2'",
"'A\\ud800\\u03a3'.toLowerCase() === 'a\\ud800\\u03c3'",
"'\\u{10400}\\u03a3'.toLowerCase() === '\\u{10428}\\u03c2'",
"'\\u03a3\\u{10400}'.toLowerCase() === '\\u03c3\\u{10428}'",
"'A\\u02b0\\u03a3'.toLowerCase() === 'a\\u02b0\\u03c2'",
"'A\\u2019\\u03a3'.toLowerCase() === 'a\\u2019\\u03c2'",
"'A\\u00b7\\u03a3'.toLowerCase() === 'a\\u00b7\\u03c2'",
"'A\\u0345\\u03a3'.toLowerCase() === 'a\\u0345\\u03c2'",
"'\\u0345\\u03a3'.toLowerCase() === '\\u0345\\u03c3'",
"'\\u2160\\u24b6'.toLowerCase() + '\\u2170\\u24d0'.toUpperCase()",
"'\\u0345\\u1fbe'.toUpperCase().length",
"'\\u1fbe\\u0345'.toUpperCase().length",
"'\\ud800\\udfff\\u0378\\uffff'.toUpperCase().length",
"'\\u0069\\u0307\\u0049\\u0307'.toLowerCase().length",
"'a'.toLocaleUpperCase()",
"'A'.toLocaleLowerCase()",
"'ss'.localeCompare('\\u00df')",
"'a'.localeCompare('b')",
"'b'.localeCompare('a')",
"'a'.localeCompare('a')",
"'a'.localeCompare('A')",
"'A'.localeCompare('a')",
"'a'.localeCompare('B')",
"'A'.localeCompare('b')",
"'e\\u0301'.localeCompare('\\u00e9')",
"'\\u00e9'.localeCompare('e\\u0301')",
"'\\u1e69'.localeCompare('s\\u0323\\u0307')",
"'\\u212b'.localeCompare('\\u00c5')",
"'\\u0958'.localeCompare('\\u0915\\u093c')",
"'\\uac01'.localeCompare('\\u1100\\u1161\\u11a8')",
"''.localeCompare('')",
"''.localeCompare('a')",
"'a'.localeCompare('')",
"'a'.localeCompare()",
"'undefined'.localeCompare()",
"'null'.localeCompare(null)",
"'1'.localeCompare(1)",
"'a'.localeCompare('b', 'de')",
"'a'.localeCompare('b', undefined, { sensitivity: 'base' })",
"'\\ud800'.localeCompare('\\ud800')",
"'\\ud800'.localeCompare('\\udc00')",
"(() => { const a = ['b', 'a', 'B', 'A', '\\u00e4', 'z', 'Z', '_', '1', '10', '2', ' ', '-']; for (let i = 1; i < a.length; i++) { const x = a[i]; let j = i - 1; while (j >= 0 && a[j].localeCompare(x) > 0) { a[j + 1] = a[j]; j--; } a[j + 1] = x; } return a.join('|'); })()",
"(() => { const a = ['\\u00e9', '\\u00e8', '\\u00ea', '\\u00eb', 'e', 'f', 'E', '\\u00c9']; for (let i = 1; i < a.length; i++) { const x = a[i]; let j = i - 1; while (j >= 0 && a[j].localeCompare(x) > 0) { a[j + 1] = a[j]; j--; } a[j + 1] = x; } return a.join('|'); })()",
"(() => { const a = ['\\u03b1', '\\u03b2', '\\u0391', '\\u03c3', '\\u03c2', '\\u03a3', '\\u03ac', '\\u0430', '\\u0431', '\\u0410', '\\u0439', '\\u0438', '\\u0451', '\\u0435']; for (let i = 1; i < a.length; i++) { const x = a[i]; let j = i - 1; while (j >= 0 && a[j].localeCompare(x) > 0) { a[j + 1] = a[j]; j--; } a[j + 1] = x; } return a.join('|'); })()",
"String.prototype.localeCompare.length + ' ' + String.prototype.normalize.length + ' ' + String.prototype.toLowerCase.length + ' ' + String.prototype.toLocaleUpperCase.length",
"String.prototype.toLowerCase.name + String.prototype.toLocaleUpperCase.name + String.prototype.normalize.name + String.prototype.localeCompare.name",
"(() => { const want = ['link', 'localeCompare', 'normalize', 'padEnd', 'trimRight', 'toLocaleLowerCase', 'toLocaleUpperCase', 'toLowerCase', 'toUpperCase', 'valueOf']; const r = []; for (const k of Object.getOwnPropertyNames(String.prototype)) if (want.indexOf(k) >= 0) r.push(k); return r.join(); })()",
"Object.getOwnPropertyDescriptor(String.prototype, 'normalize').enumerable",
"new String('Ab').toLowerCase()",
"String.prototype.toUpperCase.call(123.5)",
"String.prototype.toLowerCase.call({ toString() { return 'XY'; } })",
"String.prototype.normalize.call({ toString() { return '\\u0041\\u030a'; } })",
"('a'.repeat(100) + 'B').toLowerCase() === 'a'.repeat(100) + 'b'",
"('\\u00e9'.repeat(20000) + 'E\\u0301').normalize('NFC').length",
"'\\u00df'.repeat(30000).toUpperCase().length",
"'\\u0390'.repeat(30000).toUpperCase().length",
"'\\u0130'.repeat(30000).toLowerCase().length",
"'A\\u03a3'.repeat(30000).toLowerCase().length",
"'\\ufdfa'.repeat(30000).normalize('NFKD').length",
"('a' + '\\u0301\\u0323'.repeat(30000)).normalize('NFC').length",
"('a' + '\\u0301\\u0323'.repeat(30000)).normalize('NFD').charCodeAt(1)",
"'\\u{1d15e}'.repeat(20000).normalize('NFC').length"];
for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }
