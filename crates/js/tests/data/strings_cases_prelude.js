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
