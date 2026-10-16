var seed = 12345;
function rnd() { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; }
function ri(n) { return Math.floor(rnd() * n); }
var alphabets = ['ab', 'abc', 'a', 'abĀ', 'aĀ😀', 'xyz012'];
function rstr(alpha, n) { var s = ''; for (var i = 0; i < n; i++) s += alpha[ri(alpha.length)]; return s; }
var h = 0;
function mix(x) { var s = String(x); for (var i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 1000000007; }
var out = [];
for (var iter = 0; iter < 300; iter++) {
  var alpha = alphabets[ri(alphabets.length)];
  var hay = rstr(alpha, ri(120));
  var needleLen = ri(4) === 0 ? 14 + ri(30) : ri(6);
  var needle;
  if (ri(2) === 0 && hay.length > 0) { var a = ri(hay.length); needle = hay.slice(a, a + needleLen); } else needle = rstr(alpha, needleLen);
  var pos = ri(hay.length + 6) - 3;
  var r = [hay.indexOf(needle), hay.indexOf(needle, pos), hay.lastIndexOf(needle), hay.lastIndexOf(needle, pos), hay.includes(needle, pos), hay.startsWith(needle, pos), hay.endsWith(needle, pos)];
  mix(r.join());
  var sp = hay.split(needle);
  mix(sp.length + ':' + sp.join('|'));
  mix(hay.split(needle, ri(5)).join('|'));
  mix(hay.replaceAll(needle, '[$&]'));
  mix(hay.replace(needle, "<$`|$'>"));
  mix(hay.replaceAll(needle, function (m, p) { return p; }));
  if (iter % 100 === 0) out.push(iter + ' ' + h);
}
out.push('final ' + h);
print(out.join('\n'));
// slice/substring/substr/pad/trim/at/codePointAt
for (var iter = 0; iter < 200; iter++) {
  var s = rstr(' \t\na b 😀Ā', ri(20));
  var a = ri(30) - 10, b = ri(30) - 10;
  mix([s.slice(a, b), s.substring(a, b), s.substr(a, b), s.slice(a), s.substring(a), s.substr(a), s.padStart(a + 10, 'Ā-'), s.padEnd(b + 10, 'xy'), s.trim(), s.trimStart(), s.trimEnd(), s.at(a), s.charAt(a), s.charCodeAt(a), s.codePointAt(a), s.concat(a, b), s.repeat(ri(4)), s.isWellFormed(), s.toWellFormed(), [...s].length].join('/'));
}
print('final2 ' + h);
