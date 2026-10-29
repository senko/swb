// The cases of `tools/probes/web-inner-text.json`, `web-match-media.json` and
// `web-render-update.json` with the values that Chromium 148 gave (`just
// probe --json FILE`, three runs, all equal); where swb departs on purpose,
// the test overrides the value.
// Generated.

use crate::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>  Hello   <b>bold</b>
 world <p>para</p><p>two</p> tail</div><div id=b><div>one</div><div><div>two</div></div>three<span> four </span></div><div id=c><p>x</p></div><div id=d>y</div><div id=e> <span> a </span> <span> b </span> </div><div id=f>a&nbsp;&nbsp;b &nbsp; c</div><div id=g><h1>Head</h1><hr><ul><li>i1</li><li>i2</li></ul><ol><li>o1</li></ol><blockquote>q</blockquote></div><div id=h><dl><dt>t</dt><dd>d</dd></dl><address>ad</address><figure><figcaption>fc</figcaption>f</figure></div><div id=i>a<p></p>b<p> </p>c<div></div>d</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 2 of the cases.
pub(crate) const PAGE_2: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>a<br>b<br><br>c<br></div><div id=b><br>x</div><div id=c>x<br> y</div><div id=d><span>a<br></span>b</div><p id=e>a<br></p><p id=f><br></p><div id=g>a<wbr>b</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 3 of the cases.
pub(crate) const PAGE_3: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>a<span style='display:none'>X</span>b</div><div id=b style='display:none'>hid<b>den</b> <p>p</p></div><div id=c>a<span style='visibility:hidden'>X</span>b</div><div id=d style='visibility:hidden'>x<span style='visibility:visible'>V</span>y</div><div id=e>a<span hidden>X</span>b</div><div id=f>a<span style='opacity:0'>O</span>b</div><div id=g style='height:0;overflow:hidden'>clipped</div><div id=h>a<span style='display:contents'>C<b>D</b></span>b</div><span id=i style='display:contents'>c1<b>c2</b></span><div id=j>a<script>var s = 1;</script><style>.x{}</style>b<template>T</template><noscript>N</noscript></div><div id=k style='visibility:hidden'><p>p1</p><p>p2</p></div><div id=l>a<div style='visibility:hidden'>h</div>b</div><div id=m style='content-visibility:hidden'>cv</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 4 of the cases.
pub(crate) const PAGE_4: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><table id=a><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table><table id=b><caption>cap</caption><thead><tr><th>h1</th><th>h2</th></tr></thead><tbody><tr><td> x </td><td></td><td>z</td></tr></tbody><tfoot><tr><td>f</td></tr></tfoot></table><div id=c>before<table><tr><td>t</td></tr></table>after</div><table id=d><tr><td><p>p</p></td><td><div>dv</div></td></tr></table><div id=e style='display:table'><div style='display:table-row'><div style='display:table-cell'>r1c1</div><div style='display:table-cell'>r1c2</div></div><div style='display:table-row'><div style='display:table-cell'>r2c1</div></div></div><table id=f><tr><td>a</td><td style='display:none'>b</td><td>c</td></tr></table><table id=g><tr><td>a</td></tr><tr style='display:none'><td>b</td></tr><tr><td>c</td></tr></table><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 5 of the cases.
pub(crate) const PAGE_5: &str = "<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><pre id=a>  a  b

  c	 d  
</pre><div id=b style='white-space:pre-wrap'>  a  b 
 c  </div><div id=c style='white-space:pre-line'>  a  b 
  c  </div><div id=d style='white-space:nowrap'>  a   b 
 c  </div><div id=e>x<pre>
 p </pre>y</div><textarea id=f>  ta
 x</textarea><div id=g style='width:30px'>soft wrapped words here</div><pre id=h>\r
r\rn</pre><div id=i>a<span style='white-space:pre'>  b  </span>c</div><div id=j> x </div><div id=k>a\u{200b}b 　c</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 6 of the cases.
pub(crate) const PAGE_6: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a style='text-transform:uppercase'>hello World ß</div><div id=b style='text-transform:capitalize'>hello wORLD a-b o'k 1st</div><div id=c style='text-transform:lowercase'>HeLLo İ Σ</div><div id=d>a<span style='text-transform:uppercase'>bc</span>d</div><div id=e style='text-transform:full-width'>ab1</div><div id=f style='font-variant:small-caps'>small</div><div id=g style='letter-spacing:5px;word-spacing:9px'>a b</div><div id=h style='text-transform:capitalize'><span>ab</span><span>cd</span> ef</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 7 of the cases.
pub(crate) const PAGE_7: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>a<span style='display:inline-block'>ib</span>b</div><div id=b>a<div style='display:inline-block'><p>ib1</p><p>ib2</p></div>b</div><div id=c>a<span style='float:left'>fl</span>b</div><div id=d>a<span style='position:absolute'>abs</span>b</div><div id=e style='display:flex'><span>f1</span><span>f2</span>f3</div><div id=f style='display:grid'><span>g1</span><span>g2</span></div><div id=g>a<span style='display:block'>blk</span>b</div><div id=h style='display:inline'>x<div>y</div>z</div><div id=i>a<li>li</li>b</div><div id=j style='columns:2'>c1 c2</div><div id=k>a<div style='display:inline-flex'><span>if1</span><span>if2</span></div>b</div><div id=l>a<span style='position:relative;top:5px'>rel</span>b</div><div id=m>a<div style='display:flow-root'>fr</div>b</div><div id=n>a<span style='display:table-cell'>tc</span>b</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 8 of the cases.
pub(crate) const PAGE_8: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } .x::before { content: 'B' } .x::after { content: 'A' } .y::before { content: 'blk'; display: block }</style></head><body><div id=a class=x>mid</div><div id=b class=y>mid</div><ol id=c><li>one</li><li>two</li></ol><div id=d><q>quoted</q></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 9 of the cases.
pub(crate) const PAGE_9: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>a<img alt=ALT>b</div><div id=b>a<input value=val>b<button>btn</button>c</div><div id=c><select><option>o1</option><option>o2</option></select></div><div id=d><select size=3><option>s1</option><optgroup label=G><option>s2</option></optgroup></select></div><div id=e>a<canvas>fallback</canvas>b</div><div id=f>a<video>vid</video>b<audio>aud</audio>c</div><div id=g>a<iframe>if</iframe>b<object>ob</object>c</div><div id=h>a<svg width=10 height=10><text>svgt</text></svg>b</div><div id=i>a<math><mi>x</mi></math>b</div><div id=j><details><summary>sum</summary>body</details><details open><summary>s2</summary>b2</details></div><div id=k>a<ruby>r<rt>t</rt></ruby>b</div><div id=l><label>lab<input></label><fieldset><legend>leg</legend>fs</fieldset></div><div id=m><progress>pr</progress><meter>me</meter><output>ou</output></div><div id=n><button>b<span>c</span></button><input type=submit><input type=button value=v></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 10 of the cases.
pub(crate) const PAGE_10: &str = r"<html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>a<p>b</p>c</div><table id=b><tr><td>x</td><td>y</td></tr></table><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 11 of the cases.
pub(crate) const PAGE_11: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><div id=a>old <b>bold</b></div><div id=b></div><pre id=c></pre><div id=d><span id=s>s</span></div><p id=o>x<span id=os>mid</span>y</p><div id=w></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 12 of the cases.
pub(crate) const PAGE_12: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } </style></head><body><p id=o>x<span id=os>mid</span>y</p><div id=w><span id=ws>z</span></div><div id=v>q<i id=vi>i</i></div><div id=u><b id=ub>b</b></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function T(id) { return E(() => document.getElementById(id).innerText); }
function K(n) { return [...n.childNodes].map(c => c.nodeType === 3 ? JSON.stringify(c.data) : c.nodeName).join(' '); }
</script></body></html>";

/// Page 13 of the cases.
pub(crate) const PAGE_13: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function M(q) { return E(() => { const m = matchMedia(q); return [m.media, m.matches]; }); }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
</script></body></html>";

/// Page 14 of the cases.
pub(crate) const PAGE_14: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function M(q) { return E(() => { const m = matchMedia(q); return [m.media, m.matches]; }); }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }

var mqs = {};
function watch(q) {
  const m = matchMedia(q);
  mqs[q] = m;
  m.addListener(e => log.push('L ' + e.media + ' ' + e.matches + ' ' + (e instanceof MediaQueryListEvent) + ' ' + e.isTrusted + ' ' + e.bubbles + ' ' + e.cancelable + ' ' + (e.target === m) + ' ' + m.matches));
  return m;
}

var a = watch('(min-width: 600px)');
var b = watch('(max-width: 599px)');
var c = watch('(orientation: portrait)');
var d = watch('(min-width: 2000px)');
b.onchange = e => log.push('H ' + e.media);
a.addEventListener('change', e => log.push('E ' + e.media));
addEventListener('resize', e => { log.push('resize ' + innerWidth + ' ' + e.isTrusted + ' ' + e.bubbles + ' ' + e.cancelable + ' ' + (e.target === window) + ' ' + a.matches); requestAnimationFrame(() => log.push('raf-from-resize')); });
document.addEventListener('resize', () => log.push('resize at document'));
onresize = () => log.push('onresize');
function tick() { requestAnimationFrame(() => { if (log.length && log[log.length - 1] !== 'raf') log.push('raf'); tick(); }); }
tick();
</script></body></html>";

/// Page 15 of the cases.
pub(crate) const PAGE_15: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function M(q) { return E(() => { const m = matchMedia(q); return [m.media, m.matches]; }); }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }

var mqs = {};
function watch(q) {
  const m = matchMedia(q);
  mqs[q] = m;
  m.addListener(e => log.push('L ' + e.media + ' ' + e.matches + ' ' + (e instanceof MediaQueryListEvent) + ' ' + e.isTrusted + ' ' + e.bubbles + ' ' + e.cancelable + ' ' + (e.target === m) + ' ' + m.matches));
  return m;
}
var h = watch('(min-height: 500px)'); var w = watch('(min-width: 600px)'); addEventListener('resize', () => log.push('resize ' + innerHeight));</script></body></html>";

/// Page 16 of the cases.
pub(crate) const PAGE_16: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function M(q) { return E(() => { const m = matchMedia(q); return [m.media, m.matches]; }); }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }

var mqs = {};
function watch(q) {
  const m = matchMedia(q);
  mqs[q] = m;
  m.addListener(e => log.push('L ' + e.media + ' ' + e.matches + ' ' + (e instanceof MediaQueryListEvent) + ' ' + e.isTrusted + ' ' + e.bubbles + ' ' + e.cancelable + ' ' + (e.target === m) + ' ' + m.matches));
  return m;
}
var k = watch('(prefers-color-scheme: dark)'); var l = watch('(prefers-color-scheme: light)'); addEventListener('resize', () => log.push('resize'));</script></body></html>";

/// Page 17 of the cases.
pub(crate) const PAGE_17: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function M(q) { return E(() => { const m = matchMedia(q); return [m.media, m.matches]; }); }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }

var mqs = {};
function watch(q) {
  const m = matchMedia(q);
  mqs[q] = m;
  m.addListener(e => log.push('L ' + e.media + ' ' + e.matches + ' ' + (e instanceof MediaQueryListEvent) + ' ' + e.isTrusted + ' ' + e.bubbles + ' ' + e.cancelable + ' ' + (e.target === m) + ' ' + m.matches));
  return m;
}
var x = matchMedia('(min-width: 600px)'); var y = matchMedia('(min-width: 700px)'); y.addListener(() => log.push('y')); x.addListener(() => log.push('x')); var z = matchMedia('(min-width: 650px)'); z.addListener(() => { log.push('z'); Promise.resolve().then(() => log.push('z-micro')); }); var unwatched = matchMedia('(min-width: 600px)');</script></body></html>";

/// Page 18 of the cases.
pub(crate) const PAGE_18: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
function frame() { return new Promise(r => requestAnimationFrame(r)); }
function frames(n) { return new Promise(r => { const ts = []; function f(t) { ts.push(t); if (ts.length === n) r(ts); else requestAnimationFrame(f); } requestAnimationFrame(f); }); }
</script></body></html>";

/// Page 19 of the cases.
pub(crate) const PAGE_19: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=sc style='height:100px;overflow:auto'><div style='height:500px'></div></div><div style='height:3000px'></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
function frame() { return new Promise(r => requestAnimationFrame(r)); }
function frames(n) { return new Promise(r => { const ts = []; function f(t) { ts.push(t); if (ts.length === n) r(ts); else requestAnimationFrame(f); } requestAnimationFrame(f); }); }
addEventListener('scroll', () => log.push('scroll')); sc.addEventListener('scroll', () => log.push('sc-scroll')); addEventListener('resize', () => log.push('resize'));</script></body></html>";

/// Page 20 of the cases.
pub(crate) const PAGE_20: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d style='width:100px'>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
function frame() { return new Promise(r => requestAnimationFrame(r)); }
function frames(n) { return new Promise(r => { const ts = []; function f(t) { ts.push(t); if (ts.length === n) r(ts); else requestAnimationFrame(f); } requestAnimationFrame(f); }); }
var widths = []; function loop(n) { requestAnimationFrame(() => { d.style.width = (100 + n) + 'px'; widths.push(d.offsetWidth); if (n < 5) loop(n + 1); }); } loop(1);</script></body></html>";

/// Page 21 of the cases.
pub(crate) const PAGE_21: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } </style></head><body><div id=d></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
var log = [];
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
function frame() { return new Promise(r => requestAnimationFrame(r)); }
function frames(n) { return new Promise(r => { const ts = []; function f(t) { ts.push(t); if (ts.length === n) r(ts); else requestAnimationFrame(f); } requestAnimationFrame(f); }); }
addEventListener('resize', () => log.push('resize ' + innerWidth)); function tick() { requestAnimationFrame(() => { log.push('raf ' + innerWidth); if (log.length < 40) tick(); }); }</script></body></html>";

/// Page 22 of the cases.
pub(crate) const PAGE_22: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif }</style></head><body><svg id=s width=300 height=200 viewBox='0 0 150 100'><rect id=r x=10 y=20 width=30 height=40 transform='translate(5 5)' /><rect id=rr x=0 y=0 width=20 height=10 rx=3 /><circle id=c cx=50 cy=50 r=10 /><ellipse id=e cx=80 cy=20 rx=10 ry=5 /><line id=l x1=0 y1=0 x2=30 y2=40 stroke=black /><polyline id=pl points='0,0 10,0 10,10' /><polygon id=pg points='0,0 10,0 10,10' /><path id=p d='M0 0 L30 40' /><path id=pc d='M0 0 C 10 0 20 10 20 20' /><path id=pq d='M0 0 Q 10 10 20 0 Z' /><path id=pm d='M0 0 L10 0 M20 0 L20 10' /><path id=pa d='M0 0 A 10 10 0 0 1 20 0' /><g id=g transform='translate(100 0)'><rect id=gr x=1 y=2 width=3 height=4 /><circle id=gc cx=10 cy=10 r=2 transform='scale(2)' /></g><g id=ge></g><text id=t x=5 y=90>text</text><rect id=hid x=1 y=1 width=5 height=5 style='display:none' /><path id=pe d='' /></svg><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function R(r) { return [r.x, r.y, r.width, r.height]; }
function B(id) { return E(() => R(document.getElementById(id).getBBox())); }
function L(id) { return E(() => Math.round(document.getElementById(id).getTotalLength() * 1000) / 1000); }
</script></body></html>";

/// The cases of `web-inner-text.json`.
pub(crate) const INNER_TEXT: &[Case] = &[
    Case {
        name: "blocks",
        html: PAGE_1,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""Hello bold world\n\npara\n\ntwo\n\ntail""#),
            (r#"T("b")"#, r#""one\ntwo\nthree four""#),
            (r#"T("c")"#, r#""x""#),
            (r#"T("d")"#, r#""y""#),
            (r#"T("e")"#, r#""a b""#),
            (r#"T("f")"#, r#""a  b   c""#),
            (r#"T("g")"#, r#""Head\ni1\ni2\no1\nq""#),
            (r#"T("h")"#, r#""t\nd\nad\nfc\nf""#),
            (r#"T("i")"#, r#""a\n\nb\n\nc\nd""#),
            (
                r"JSON.stringify(document.body.innerText)",
                r#""\"Hello bold world\\n\\npara\\n\\ntwo\\n\\ntail\\none\\ntwo\\nthree four\\n\\nx\\n\\ny\\na b\\na  b   c\\nHead\\ni1\\ni2\\no1\\nq\\nt\\nd\\nad\\nfc\\nf\\na\\n\\nb\\n\\nc\\nd\"""#,
            ),
            (
                r"JSON.stringify(document.documentElement.innerText)",
                r#""\"Hello bold world\\n\\npara\\n\\ntwo\\n\\ntail\\none\\ntwo\\nthree four\\n\\nx\\n\\ny\\na b\\na  b   c\\nHead\\ni1\\ni2\\no1\\nq\\nt\\nd\\nad\\nfc\\nf\\na\\n\\nb\\n\\nc\\nd\"""#,
            ),
        ],
    },
    Case {
        name: "breaks",
        html: PAGE_2,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""a\nb\n\nc\n""#),
            (r#"T("b")"#, r#""\nx""#),
            (r#"T("c")"#, r#""x\ny""#),
            (r#"T("d")"#, r#""a\nb""#),
            (r#"T("e")"#, r#""a\n""#),
            (r#"T("f")"#, r#""\n""#),
            (r#"T("g")"#, r#""ab""#),
            (r"T('a').length", r"7"),
        ],
    },
    Case {
        name: "hidden",
        html: PAGE_3,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""ab""#),
            (r#"T("b")"#, r#""hidden p""#),
            (r#"T("c")"#, r#""ab""#),
            (r#"T("d")"#, r#""V""#),
            (r#"T("e")"#, r#""ab""#),
            (r#"T("f")"#, r#""aOb""#),
            (r#"T("g")"#, r#""clipped""#),
            (r#"T("h")"#, r#""aCDb""#),
            (r#"T("i")"#, r#""c1c2""#),
            (r#"T("j")"#, r#""ab""#),
            (r#"T("k")"#, r#""""#),
            (r#"T("l")"#, r#""ab""#),
            (r#"T("m")"#, r#""""#),
            (
                r"E(() => document.querySelector('#j script').innerText)",
                r#""var s = 1;""#,
            ),
            (
                r"E(() => document.querySelector('#j style').innerText)",
                r#"".x{}""#,
            ),
            (r"E(() => document.createElement('div').innerText)", r#""""#),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = ' a  <p>b</p> c '; return d.innerText; })",
                r#"" a  b c ""#,
            ),
            (
                r"E(() => document.querySelector('template').innerText)",
                r#""""#,
            ),
            (
                r"E(() => document.head.innerText)",
                r#""tbody { font: 16px/20px sans-serif } ""#,
            ),
            (
                r"E(() => document.querySelector('title').innerText)",
                r#""t""#,
            ),
        ],
    },
    Case {
        name: "tables",
        html: PAGE_4,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""a\tb\nc\td""#),
            (r#"T("b")"#, r#""cap\nh1\th2\nx\t\tz\nf""#),
            (r#"T("c")"#, r#""before\nt\nafter""#),
            (r#"T("d")"#, r#""p\n\n\t\ndv""#),
            (r#"T("e")"#, r#""r1c1\tr1c2\nr2c1""#),
            (r#"T("f")"#, r#""a\tc""#),
            (r#"T("g")"#, r#""a\nc""#),
            (
                r"E(() => document.querySelector('#a tr').innerText)",
                r#""a\tb""#,
            ),
            (
                r"E(() => document.querySelector('#a td').innerText)",
                r#""a""#,
            ),
            (
                r"E(() => document.querySelector('#b caption').innerText)",
                r#""cap""#,
            ),
        ],
    },
    Case {
        name: "white-space",
        html: PAGE_5,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""  a  b\n\n  c\t d  \n""#),
            (r#"T("b")"#, r#""  a  b \n c  ""#),
            (r#"T("c")"#, r#""a b\nc""#),
            (r#"T("d")"#, r#""a b c""#),
            (r#"T("e")"#, r#""x\n p \ny""#),
            (r#"T("f")"#, r#""""#),
            (r#"T("g")"#, r#""soft wrapped words here""#),
            (r#"T("h")"#, r#""r\nn""#),
            (r#"T("i")"#, r#""a  b  c""#),
            (r#"T("j")"#, r#"" x ""#),
            (r#"T("k")"#, "\"a\u{200b}b 　c\""),
        ],
    },
    Case {
        name: "transform",
        html: PAGE_6,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""HELLO WORLD SS""#),
            (r#"T("b")"#, r#""Hello WORLD A-B O'k 1st""#),
            (r#"T("c")"#, r#""hello i̇ σ""#),
            (r#"T("d")"#, r#""aBCd""#),
            (r#"T("e")"#, r#""ab1""#),
            (r#"T("f")"#, r#""small""#),
            (r#"T("g")"#, r#""a b""#),
            (r#"T("h")"#, r#""Abcd Ef""#),
        ],
    },
    Case {
        name: "layout-kinds",
        html: PAGE_7,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""aibb""#),
            (r#"T("b")"#, r#""a\n\nib1\n\nib2\n\nb""#),
            (r#"T("c")"#, r#""a\nfl\nb""#),
            (r#"T("d")"#, r#""a\nabs\nb""#),
            (r#"T("e")"#, r#""f1\nf2\nf3""#),
            (r#"T("f")"#, r#""g1\ng2""#),
            (r#"T("g")"#, r#""a\nblk\nb""#),
            (r#"T("h")"#, r#""x\ny\nz""#),
            (r#"T("i")"#, r#""a\nli\nb""#),
            (r#"T("j")"#, r#""c1 c2""#),
            (r#"T("k")"#, r#""a\nif1\nif2\nb""#),
            (r#"T("l")"#, r#""arelb""#),
            (r#"T("m")"#, r#""a\nfr\nb""#),
            (r#"T("n")"#, r#""atcb""#),
        ],
    },
    Case {
        name: "generated",
        html: PAGE_8,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""mid""#),
            (r#"T("b")"#, r#""mid""#),
            (r#"T("c")"#, r#""one\ntwo""#),
            (r#"T("d")"#, r#""quoted""#),
        ],
    },
    Case {
        name: "replaced",
        html: PAGE_9,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""ab""#),
            (r#"T("b")"#, r#""abbtnc""#),
            (r#"T("c")"#, r#""o1\no2""#),
            (r#"T("d")"#, r#""s1\ns2""#),
            (r#"T("e")"#, r#""ab""#),
            (r#"T("f")"#, r#""abc""#),
            (r#"T("g")"#, r#""abobc""#),
            (r#"T("h")"#, r#""a\nsvgt\nb""#),
            (r#"T("i")"#, r#""a\n𝑥\nb""#),
            (r#"T("j")"#, r#""sum\ns2\nb2""#),
            (r#"T("k")"#, r#""artb""#),
            (r#"T("l")"#, r#""lab\nleg\nfs""#),
            (r#"T("m")"#, r#""ou""#),
            (r#"T("n")"#, r#""bc""#),
            (
                r"E(() => document.querySelector('select').innerText)",
                r#""o1\no2""#,
            ),
            (
                r"E(() => document.querySelector('option').innerText)",
                r#""o1""#,
            ),
            (
                r"E(() => document.querySelector('textarea, button').innerText)",
                r#""btn""#,
            ),
            (
                r"E(() => document.querySelector('summary').innerText)",
                r#""sum""#,
            ),
            (
                r"E(() => document.querySelector('details').innerText)",
                r#""sum""#,
            ),
            (
                r"['innerText' in document.querySelector('svg'), 'innerText' in document.querySelector('text'), 'outerText' in document.body, 'innerText' in HTMLElement.prototype, 'innerText' in Element.prototype]",
                r"[false, false, true, true, false]",
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'innerText').set.length)",
                r"1",
            ),
        ],
    },
    Case {
        name: "quirks",
        html: PAGE_10,
        steps: &[],
        queries: &[
            (r#"T("a")"#, r#""a\n\nb\n\nc""#),
            (r#"T("b")"#, r#""x\ty""#),
            (
                r"JSON.stringify(document.body.innerText)",
                r#""\"a\\n\\nb\\n\\nc\\nx\\ty\"""#,
            ),
        ],
    },
    Case {
        name: "setter",
        html: PAGE_11,
        steps: &[],
        queries: &[
            (
                r"E(() => { const b = a.querySelector('b'); a.innerText = 'x'; return [K(a), b.parentNode === null] })",
                r#"["\"x\"", true]"#,
            ),
            (
                r"E(() => { b.innerText = 'a\nb'; return [K(b), b.innerHTML] })",
                r#"["\"a\" BR \"b\"", "a<br>b"]"#,
            ),
            (
                r"E(() => { b.innerText = 'a\r\nb\rc\n\nd\n'; return [K(b), b.innerHTML] })",
                r#"["\"a\" BR \"b\" BR \"c\" BR BR \"d\" BR", "a<br>b<br>c<br><br>d<br>"]"#,
            ),
            (
                r"E(() => { b.innerText = ''; return [K(b), b.childNodes.length] })",
                r#"["", 0]"#,
            ),
            (
                r"E(() => { b.innerText = null; return [K(b), b.childNodes.length] })",
                r#"["", 0]"#,
            ),
            (
                r"E(() => { b.innerText = undefined; return K(b) })",
                r#""\"undefined\"""#,
            ),
            (r"E(() => { b.innerText = 42; return K(b) })", r#""\"42\"""#),
            (
                r"E(() => { b.innerText = '\n'; return [K(b), b.innerText] })",
                r#"["BR", "\n"]"#,
            ),
            (
                r"E(() => { b.innerText = ' a  b '; return [K(b), JSON.stringify(b.innerText)] })",
                r#"["\" a  b \"", "\"a b\""]"#,
            ),
            (
                r"E(() => { b.innerText = '<i>x</i> &amp;'; return [K(b), b.innerHTML] })",
                r#"["\"<i>x</i> &amp;\"", "&lt;i&gt;x&lt;/i&gt; &amp;amp;"]"#,
            ),
            (
                r"E(() => { c.innerText = 'p\nq'; return [K(c), c.innerHTML, JSON.stringify(c.innerText)] })",
                r#"["\"p\" BR \"q\"", "p<br>q", "\"p\\nq\""]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerText = 'u\nv'; return [K(e), e.innerText] })",
                r#"["\"u\" BR \"v\"", "uv"]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerText = 'u\nv'; return e.innerHTML })",
                r#""u<br>v""#,
            ),
            (
                r"E(() => { b.innerText = 'x'; const t = b.firstChild; b.innerText = 'y'; return [t.parentNode === null, K(b)] })",
                r#"[true, "\"y\""]"#,
            ),
            (
                r"E(() => { d.innerText = 'n'; return [K(d), s.parentNode === null] })",
                r#""ReferenceError: s is not defined""#,
            ),
            (
                r"E(() => { const l = []; const m = new MutationObserver(r => {}); m.observe(b, {childList: true}); b.innerText = 'a\nb'; const r = m.takeRecords(); return [r.length, r[0] && r[0].addedNodes.length, r[0] && r[0].removedNodes.length] })",
                r"[1, 3, 1]",
            ),
            (
                r"E(() => { b.innerText = 'a\u2028b\u0085c'; return K(b) })",
                r#""\"a bc\"""#,
            ),
            (
                r"E(() => { b.innerText = 'x'.repeat(5) + '\n'.repeat(3); return [b.childNodes.length, K(b)] })",
                r#"[4, "\"xxxxx\" BR BR BR"]"#,
            ),
        ],
    },
    Case {
        name: "outer-text",
        html: PAGE_12,
        steps: &[],
        queries: &[
            (r"E(() => os.outerText)", r#""mid""#),
            (r"E(() => o.outerText)", r#""xmidy""#),
            (
                r"E(() => { os.outerText = 'A\nB'; return [K(o), o.innerHTML, o.childNodes.length] })",
                r#"["\"xA\" BR \"By\"", "xA<br>By", 3]"#,
            ),
            (
                r"E(() => { ws.outerText = 'w'; return [K(w), w.childNodes.length] })",
                r#"["\"w\"", 1]"#,
            ),
            (
                r"E(() => { vi.outerText = ''; return [K(v), v.childNodes.length] })",
                r#"["\"q\"", 1]"#,
            ),
            (
                r"E(() => { const e = document.createElement('span'); e.outerText = 'x'; return 'no throw' })",
                r#""DOMException NoModificationAllowedError: Failed to set the 'outerText' property on 'HTMLElement': The element has no parent.""#,
            ),
            (
                r"E(() => { ub.outerText = null; return [K(u), u.childNodes.length] })",
                r#"["\"\"", 1]"#,
            ),
            (
                r"E(() => { document.documentElement.outerText = 'x'; return 'no throw' })",
                r#""DOMException HierarchyRequestError: Failed to set the 'outerText' property on 'HTMLElement': Nodes of type '#text' may not be inserted inside nodes of type '#document'.""#,
            ),
        ],
    },
];

/// The cases of `web-match-media.json`.
pub(crate) const MATCH_MEDIA: &[Case] = &[
    Case {
        name: "parse",
        html: PAGE_13,
        steps: &[],
        queries: &[
            (r#"M("")"#, r#"["", true]"#),
            (r#"M(" ")"#, r#"["", true]"#),
            (r#"M("all")"#, r#"["all", true]"#),
            (r#"M("ALL")"#, r#"["all", true]"#),
            (r#"M("screen")"#, r#"["screen", true]"#),
            (r#"M("SCREEN")"#, r#"["screen", true]"#),
            (r#"M("print")"#, r#"["print", false]"#),
            (r#"M("not print")"#, r#"["not print", true]"#),
            (r#"M("not screen")"#, r#"["not screen", false]"#),
            (r#"M("only screen")"#, r#"["only screen", true]"#),
            (r#"M("only print")"#, r#"["only print", false]"#),
            (
                r#"M("screen and (color)")"#,
                r#"["screen and (color)", true]"#,
            ),
            (
                r#"M("not screen and (color)")"#,
                r#"["not screen and (color)", false]"#,
            ),
            (
                r#"M("only screen and (max-width: 600px)")"#,
                r#"["only screen and (max-width: 600px)", false]"#,
            ),
            (r#"M("(color)")"#, r#"["(color)", true]"#),
            (r#"M("(monochrome)")"#, r#"["(monochrome)", false]"#),
            (
                r#"M("(min-width: 500px)")"#,
                r#"["(min-width: 500px)", true]"#,
            ),
            (
                r#"M("(MIN-WIDTH:500PX)")"#,
                r#"["(min-width: 500px)", true]"#,
            ),
            (
                r#"M("(min-width:500px) and (max-width:1000px)")"#,
                r#"["(min-width: 500px) and (max-width: 1000px)", true]"#,
            ),
            (
                r#"M("(max-width: 500px)")"#,
                r#"["(max-width: 500px)", false]"#,
            ),
            (r#"M("(width >= 500px)")"#, r#"["(width >= 500px)", true]"#),
            (
                r#"M("(400px < width < 1000px)")"#,
                r#"["(400px < width < 1000px)", true]"#,
            ),
            (
                r#"M("(width > 799.5px)")"#,
                r#"["(width > 799.5px)", true]"#,
            ),
            (r#"M("(width: 800px)")"#, r#"["(width: 800px)", true]"#),
            (r#"M("(height: 600px)")"#, r#"["(height: 600px)", true]"#),
            (
                r#"M("(min-width: 30em)")"#,
                r#"["(min-width: 30em)", true]"#,
            ),
            (
                r#"M("(min-width: 50.5em)")"#,
                r#"["(min-width: 50.5em)", false]"#,
            ),
            (
                r#"M("(min-width: 49.9999em)")"#,
                r#"["(min-width: 49.9999em)", true]"#,
            ),
            (r#"M("(max-width: 0)")"#, r#"["(max-width: 0)", false]"#),
            (r#"M("(min-width: 0)")"#, r#"["(min-width: 0)", true]"#),
            (
                r#"M("(min-width: 100.25px)")"#,
                r#"["(min-width: 100.25px)", true]"#,
            ),
            (
                r#"M("(min-width: 1e2px)")"#,
                r#"["(min-width: 100px)", true]"#,
            ),
            (r#"M("(min-width: 10Q)")"#, r#"["(min-width: 10q)", true]"#),
            (r#"M("(min-width: 2in)")"#, r#"["(min-width: 2in)", true]"#),
            (r#"M("(width < 50vw)")"#, r#"["(width < 50vw)", false]"#),
            (
                r#"M("(min-width: calc(100px + 1em))")"#,
                r#"["(min-width: calc(1em + 100px))", true]"#,
            ),
            (
                r#"M("(orientation: landscape)")"#,
                r#"["(orientation: landscape)", true]"#,
            ),
            (
                r#"M("(orientation: portrait)")"#,
                r#"["(orientation: portrait)", false]"#,
            ),
            (
                r#"M("(prefers-color-scheme: dark)")"#,
                r#"["(prefers-color-scheme: dark)", false]"#,
            ),
            (
                r#"M("(prefers-color-scheme: light)")"#,
                r#"["(prefers-color-scheme: light)", true]"#,
            ),
            (
                r#"M("(prefers-color-scheme)")"#,
                r#"["(prefers-color-scheme)", true]"#,
            ),
            (
                r#"M("(prefers-reduced-motion: reduce)")"#,
                r#"["(prefers-reduced-motion: reduce)", false]"#,
            ),
            (
                r#"M("(prefers-reduced-motion: no-preference)")"#,
                r#"["(prefers-reduced-motion: no-preference)", true]"#,
            ),
            (r#"M("(hover: hover)")"#, r#"["(hover: hover)", true]"#),
            (r#"M("(hover)")"#, r#"["(hover)", true]"#),
            (
                r#"M("(any-hover: hover)")"#,
                r#"["(any-hover: hover)", true]"#,
            ),
            (r#"M("(pointer: fine)")"#, r#"["(pointer: fine)", true]"#),
            (
                r#"M("(pointer: coarse)")"#,
                r#"["(pointer: coarse)", false]"#,
            ),
            (
                r#"M("(any-pointer: fine)")"#,
                r#"["(any-pointer: fine)", true]"#,
            ),
            (
                r#"M("(scripting: enabled)")"#,
                r#"["(scripting: enabled)", true]"#,
            ),
            (
                r#"M("(scripting: none)")"#,
                r#"["(scripting: none)", false]"#,
            ),
            (
                r#"M("(-webkit-min-device-pixel-ratio: 2)")"#,
                r#"["(-webkit-min-device-pixel-ratio: 2)", false]"#,
            ),
            (
                r#"M("(-webkit-min-device-pixel-ratio: 1)")"#,
                r#"["(-webkit-min-device-pixel-ratio: 1)", true]"#,
            ),
            (
                r#"M("(-webkit-device-pixel-ratio: 1)")"#,
                r#"["(-webkit-device-pixel-ratio: 1)", true]"#,
            ),
            (
                r#"M("(min-resolution: 2dppx)")"#,
                r#"["(min-resolution: 2dppx)", false]"#,
            ),
            (
                r#"M("(resolution: 1dppx)")"#,
                r#"["(resolution: 1dppx)", true]"#,
            ),
            (r#"M("(resolution: 1x)")"#, r#"["(resolution: 1x)", true]"#),
            (
                r#"M("(min-resolution: 96dpi)")"#,
                r#"["(min-resolution: 96dpi)", true]"#,
            ),
            (
                r#"M("(min-resolution: 1.5dppx)")"#,
                r#"["(min-resolution: 1.5dppx)", false]"#,
            ),
            (
                r#"M("(aspect-ratio: 4/3)")"#,
                r#"["(aspect-ratio: 4 / 3)", true]"#,
            ),
            (
                r#"M("(aspect-ratio: 4 / 3)")"#,
                r#"["(aspect-ratio: 4 / 3)", true]"#,
            ),
            (
                r#"M("(min-aspect-ratio: 16/9)")"#,
                r#"["(min-aspect-ratio: 16 / 9)", false]"#,
            ),
            (
                r#"M("(max-aspect-ratio: 2)")"#,
                r#"["(max-aspect-ratio: 2 / 1)", true]"#,
            ),
            (
                r#"M("(device-width: 800px)")"#,
                r#"["(device-width: 800px)", true]"#,
            ),
            (
                r#"M("(min-device-width: 1px)")"#,
                r#"["(min-device-width: 1px)", true]"#,
            ),
            (r#"M("(foo)")"#, r#"["(foo)", false]"#),
            (r#"M("(foo: bar)")"#, r#"["(foo: bar)", false]"#),
            (r#"M("garbage!!")"#, r#"["not all", false]"#),
            (r#"M("screen, print")"#, r#"["screen, print", true]"#),
            (r#"M("screen,print")"#, r#"["screen, print", true]"#),
            (
                r#"M("screen , (min-width: 1px)")"#,
                r#"["screen, (min-width: 1px)", true]"#,
            ),
            (
                r#"M("(display-mode: browser)")"#,
                r#"["(display-mode: browser)", true]"#,
            ),
            (
                r#"M("(forced-colors: none)")"#,
                r#"["(forced-colors: none)", true]"#,
            ),
            (
                r#"M("(prefers-contrast: no-preference)")"#,
                r#"["(prefers-contrast: no-preference)", true]"#,
            ),
            (
                r#"M("(color-gamut: srgb)")"#,
                r#"["(color-gamut: srgb)", true]"#,
            ),
            (
                r#"M("(dynamic-range: standard)")"#,
                r#"["(dynamic-range: standard)", true]"#,
            ),
            (r#"M("(update: fast)")"#, r#"["(update: fast)", true]"#),
            (r#"M("(min-color: 8)")"#, r#"["(min-color: 8)", true]"#),
            (r#"M("(grid)")"#, r#"["(grid)", false]"#),
            (r#"M("not (color)")"#, r#"["not (color)", false]"#),
            (
                r#"M("(color) or (monochrome)")"#,
                r#"["(color) or (monochrome)", true]"#,
            ),
            (r#"M("((color))")"#, r#"["((color))", true]"#),
            (r#"M("(not (color))")"#, r#"["(not (color))", false]"#),
            (
                r#"M("screen and (min-width: 100px) and (max-width: 2000px)")"#,
                r#"["screen and (min-width: 100px) and (max-width: 2000px)", true]"#,
            ),
            (r#"M("tv")"#, r#"["tv", false]"#),
            (r#"M("not tv")"#, r#"["not tv", true]"#),
            (r#"M("@media screen")"#, r#"["not all", false]"#),
            (
                r#"M("(min-width: 100px")"#,
                r#"["(min-width: 100px)", true]"#,
            ),
            (r#"M("screen and")"#, r#"["not all", false]"#),
            (r#"M("and")"#, r#"["not all", false]"#),
            (r#"M("not")"#, r#"["not all", false]"#),
            (r#"M("only")"#, r#"["not all", false]"#),
            (r#"M("(min-width: 100)")"#, r#"["(min-width: 100)", false]"#),
            (r#"M("(width: 800.0px)")"#, r#"["(width: 800px)", true]"#),
            (r#"M("(width: +800px)")"#, r#"["(width: 800px)", true]"#),
            (
                r#"M("(min-width: -1px)")"#,
                r#"["(min-width: -1px)", true]"#,
            ),
            (r#"M("speech")"#, r#"["speech", false]"#),
            (r#"M("handheld")"#, r#"["handheld", false]"#),
            (r#"M("(color: 8)")"#, r#"["(color: 8)", true]"#),
            (r#"M("(color-index)")"#, r#"["(color-index)", false]"#),
            (
                r#"M("(overflow-block: scroll)")"#,
                r#"["(overflow-block: scroll)", true]"#,
            ),
            (
                r#"M("(inverted-colors: none)")"#,
                r#"["(inverted-colors: none)", false]"#,
            ),
            (
                r#"M("(scan: progressive)")"#,
                r#"["(scan: progressive)", false]"#,
            ),
            (
                r#"M("(min-height: 37.5em)")"#,
                r#"["(min-height: 37.5em)", true]"#,
            ),
            (
                r#"M("screen and (orientation: landscape), print and (color)")"#,
                r#"["screen and (orientation: landscape), print and (color)", true]"#,
            ),
            (
                r#"M("(min-width:500px)and (max-width:900px)")"#,
                r#"["(min-width: 500px) and (max-width: 900px)", true]"#,
            ),
            (r#"M("screen and(color)")"#, r#"["not all", false]"#),
            (
                r#"M("(MAX-width: 1000PX) AND (Orientation: Landscape)")"#,
                r#"["(max-width: 1000px) and (orientation: landscape)", true]"#,
            ),
            (r#"M("not all")"#, r#"["not all", false]"#),
            (
                r#"M("(height > 500px) and (width > 500px)")"#,
                r#"["(height > 500px) and (width > 500px)", true]"#,
            ),
        ],
    },
    Case {
        name: "api",
        html: PAGE_13,
        steps: &[],
        queries: &[
            (
                r"[typeof matchMedia, matchMedia.length, typeof MediaQueryList, typeof MediaQueryListEvent]",
                r#"["function", 1, "function", "function"]"#,
            ),
            (
                r"E(() => matchMedia())",
                r#""TypeError: Failed to execute 'matchMedia' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (r"M(null)", r#"["null", false]"#),
            (r"M(undefined)", r#"["undefined", false]"#),
            (r"M(5)", r#"["not all", false]"#),
            (
                r"E(() => String(matchMedia('')))",
                r#""[object MediaQueryList]""#,
            ),
            (
                r"E(() => matchMedia('(color)') === matchMedia('(color)'))",
                r"false",
            ),
            (
                r"E(() => Object.getPrototypeOf(MediaQueryList.prototype) === EventTarget.prototype)",
                r"true",
            ),
            (
                r"E(() => Object.getOwnPropertyNames(MediaQueryList.prototype).sort())",
                r#"["addListener", "constructor", "matches", "media", "onchange", "removeListener"]"#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(MediaQueryListEvent.prototype).sort())",
                r#"["constructor", "matches", "media"]"#,
            ),
            (
                r"E(() => { const e = new MediaQueryListEvent('change', {media: 'x', matches: true}); return [e.type, e.media, e.matches, e.bubbles, e.cancelable, e.isTrusted]; })",
                r#"["change", "x", true, false, false, false]"#,
            ),
            (
                r"E(() => { const e = new MediaQueryListEvent('change'); return [e.media, e.matches]; })",
                r#"["", false]"#,
            ),
            (
                r"E(() => { const e = new MediaQueryListEvent('x', {media: null}); return [e.media, e.matches]; })",
                r#"["null", false]"#,
            ),
            (
                r"E(() => new MediaQueryListEvent())",
                r#""TypeError: Failed to construct 'MediaQueryListEvent': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => MediaQueryListEvent('change'))",
                r#""TypeError: Failed to construct 'MediaQueryListEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); return [m.onchange, m.addListener(null), m.removeListener(null), m.addListener.length, m.removeListener.length]; })",
                r"[null, null, null, 1, 1]",
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); return m.addListener(5); })",
                r#""TypeError: Failed to execute 'addListener' on 'MediaQueryList': parameter 1 is not of type 'Object'.""#,
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); return m.addListener(); })",
                r#""TypeError: Failed to execute 'addListener' on 'MediaQueryList': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); const f = () => {}; m.onchange = f; return [m.onchange === f, typeof m.onchange]; })",
                r#"[true, "function"]"#,
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); m.onchange = 5; return m.onchange; })",
                r"null",
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); let n = 0; m.addEventListener('change', () => n++); m.dispatchEvent(new Event('change')); m.onchange = () => n += 10; m.dispatchEvent(new Event('change')); return n; })",
                r"12",
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); let n = 0; const f = () => n++; m.addListener(f); m.addListener(f); m.addEventListener('change', f); m.dispatchEvent(new Event('change')); m.removeListener(f); m.dispatchEvent(new Event('change')); return n; })",
                r"1",
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); let n = 0; const f = () => n++; m.addEventListener('change', f, true); m.addListener(f); m.dispatchEvent(new Event('change')); m.removeListener(f); m.dispatchEvent(new Event('change')); return n; })",
                r"3",
            ),
            (
                r"E(() => { const m = matchMedia('(color)'); let t; m.addListener(function (e) { t = [this === m, e.target === m, e.currentTarget === m]; }); m.dispatchEvent(new Event('change')); return t; })",
                r"[true, true, true]",
            ),
            (
                r"E(() => Object.prototype.toString.call(matchMedia('x')))",
                r#""[object MediaQueryList]""#,
            ),
            (
                r"E(() => { const m = matchMedia('(min-width: 1px)'); return [m.media, m.matches, Object.keys(m).length]; })",
                r#"["(min-width: 1px)", true, 0]"#,
            ),
            (
                r"E(() => MediaQueryList.prototype.matches)",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => matchMedia.call(null, '(color)').matches)",
                r"true",
            ),
        ],
    },
    Case {
        name: "change-resize",
        html: PAGE_14,
        steps: &[
            (r"wait", r"100"),
            (r"eval", r#""log = []""#),
            (r"viewport", r"[500, 600]"),
            (r"wait", r"200"),
        ],
        queries: &[
            (
                r"log.filter(x => x !== 'raf' || true)",
                r#"["resize 500 true false false true false", "onresize", "L (min-width: 600px) false true true false false true false", "E (min-width: 600px)", "L (max-width: 599px) true true true false false true true", "H (max-width: 599px)", "L (orientation: portrait) true true true false false true true", "raf", "raf-from-resize", "raf"]"#,
            ),
            (
                r"[a.matches, b.matches, c.matches, d.matches, innerWidth, innerHeight]",
                r"[false, true, true, false, 500, 600]",
            ),
        ],
    },
    Case {
        name: "change-resize-back",
        html: PAGE_14,
        steps: &[
            (r"wait", r"100"),
            (r"eval", r#""log = []""#),
            (r"viewport", r"[500, 600]"),
            (r"wait", r"200"),
            (r"eval", r#""log.push('--')""#),
            (r"viewport", r"[800, 700]"),
            (r"wait", r"200"),
        ],
        queries: &[
            (
                r"log",
                r#"["resize 500 true false false true false", "onresize", "L (min-width: 600px) false true true false false true false", "E (min-width: 600px)", "L (max-width: 599px) true true true false false true true", "H (max-width: 599px)", "L (orientation: portrait) true true true false false true true", "raf", "raf-from-resize", "raf", "--", "resize 800 true false false true true", "onresize", "L (min-width: 600px) true true true false false true true", "E (min-width: 600px)", "L (max-width: 599px) false true true false false true false", "H (max-width: 599px)", "L (orientation: portrait) false true true false false true false", "raf", "raf-from-resize", "raf"]"#,
            ),
            (r"[a.matches, b.matches, innerWidth]", r"[true, false, 800]"),
        ],
    },
    Case {
        name: "change-height",
        html: PAGE_15,
        steps: &[
            (r"wait", r"100"),
            (r"viewport", r"[800, 400]"),
            (r"wait", r"200"),
        ],
        queries: &[(
            r"log",
            r#"["resize 400", "L (min-height: 500px) false true true false false true false"]"#,
        )],
    },
    Case {
        name: "change-color",
        html: PAGE_16,
        steps: &[
            (r"wait", r"100"),
            (r"colorScheme", r#""dark""#),
            (r"wait", r"200"),
        ],
        queries: &[
            (
                r"log",
                r#"["L (prefers-color-scheme: dark) true true true false false true true", "L (prefers-color-scheme: light) false true true false false true false"]"#,
            ),
            (
                r"[k.matches, l.matches, matchMedia('(prefers-color-scheme: dark)').matches]",
                r"[true, false, true]",
            ),
            (r"getComputedStyle(d).color", r#""rgb(0, 0, 0)""#),
        ],
    },
    Case {
        name: "change-order",
        html: PAGE_17,
        steps: &[
            (r"wait", r"100"),
            (r"viewport", r"[500, 600]"),
            (r"wait", r"200"),
        ],
        queries: &[
            (r"log", r#"["x", "y", "z", "z-micro"]"#),
            (r"unwatched.matches", r"false"),
        ],
    },
];

/// The cases of `web-render-update.json`.
pub(crate) const RENDER_UPDATE: &[Case] = &[
    Case {
        name: "ids",
        html: PAGE_18,
        steps: &[],
        queries: &[
            (
                r"[requestAnimationFrame(() => {}), requestAnimationFrame(() => {}), requestAnimationFrame(() => {})]",
                r"[1, 2, 3]",
            ),
            (
                r"(cancelAnimationFrame(1), [requestAnimationFrame(() => {}), typeof requestAnimationFrame(() => {})])",
                r#"[4, "number"]"#,
            ),
        ],
    },
    Case {
        name: "api",
        html: PAGE_18,
        steps: &[],
        queries: &[
            (
                r"[typeof requestAnimationFrame, requestAnimationFrame.length, typeof cancelAnimationFrame, cancelAnimationFrame.length]",
                r#"["function", 1, "function", 1]"#,
            ),
            (
                r"E(() => requestAnimationFrame())",
                r#""TypeError: Failed to execute 'requestAnimationFrame' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => requestAnimationFrame(5))",
                r#""TypeError: Failed to execute 'requestAnimationFrame' on 'Window': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => requestAnimationFrame({handleEvent() {}}))",
                r#""TypeError: Failed to execute 'requestAnimationFrame' on 'Window': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => cancelAnimationFrame())",
                r#""TypeError: Failed to execute 'cancelAnimationFrame' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (r"E(() => cancelAnimationFrame(999))", r#""undefined""#),
            (r"E(() => cancelAnimationFrame('x'))", r#""undefined""#),
            (r"E(() => cancelAnimationFrame(-1))", r#""undefined""#),
            (
                r"E(() => requestAnimationFrame.call(null, () => {}) > 0)",
                r"true",
            ),
            (
                r"E(() => requestAnimationFrame.call({}, () => {}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"new Promise(r => requestAnimationFrame(function (t) { r([typeof t, arguments.length, this === window]); }))",
                r#"["number", 1, true]"#,
            ),
            (
                r"new Promise(r => requestAnimationFrame(function (t) { 'use strict'; r(this === undefined); }))",
                r"true",
            ),
            (
                r"new Promise(r => { const p = performance.now(); requestAnimationFrame(t => r([t >= p - 20, performance.now() >= t, t - p < 100])); })",
                r"[true, true, true]",
            ),
            (
                r"new Promise(r => { let a; requestAnimationFrame(t => { a = t; }); requestAnimationFrame(t => r(a === t)); })",
                r"true",
            ),
            (r"frames(2).then(ts => ts[1] > ts[0])", r"true"),
            (
                r"new Promise(r => { const l = []; requestAnimationFrame(t => { l.push('a'); requestAnimationFrame(u => { l.push('inner ' + (u > t)); r(l); }); }); requestAnimationFrame(() => l.push('b')); })",
                r#"["a", "b", "inner true"]"#,
            ),
            (
                r"new Promise(r => { const l = []; let id; requestAnimationFrame(() => { l.push('a'); cancelAnimationFrame(id); }); id = requestAnimationFrame(() => l.push('b')); requestAnimationFrame(() => { l.push('c'); r(l); }); })",
                r#"["a", "c"]"#,
            ),
            (
                r"new Promise(r => { const l = []; requestAnimationFrame(() => { l.push('a'); Promise.resolve().then(() => l.push('micro')); }); requestAnimationFrame(() => { l.push('b'); r(l); }); })",
                r#"["a", "micro", "b"]"#,
            ),
            (
                r"new Promise(r => { const l = []; window.onerror = m => { l.push('onerror ' + m); }; requestAnimationFrame(() => { l.push('a'); throw new Error('boom'); }); requestAnimationFrame(() => { l.push('b'); setTimeout(() => { window.onerror = null; r(l); }, 0); }); })",
                r#"["a", "onerror Uncaught Error: boom", "b"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const f = () => l.push('f'); requestAnimationFrame(f); requestAnimationFrame(f); requestAnimationFrame(() => r(l)); })",
                r#"["f", "f"]"#,
            ),
            (
                r"new Promise(r => { const id = requestAnimationFrame(() => r('ran')); cancelAnimationFrame(id); setTimeout(() => r('canceled'), 100); })",
                r#""canceled""#,
            ),
        ],
    },
    Case {
        name: "interval",
        html: PAGE_18,
        steps: &[],
        queries: &[(
            r"frames(21).then(ts => { const d = ts.slice(1).map((t, i) => t - ts[i]); const mean = (ts[20] - ts[0]) / 20; return [Math.round(mean * 10) / 10, d.every(x => x > 10 && x < 25)]; })",
            r"[16.7, true]",
        )],
    },
    Case {
        name: "order",
        html: PAGE_19,
        steps: &[],
        queries: &[
            (
                r"after(50).then(() => { log = []; scrollTo(0, 100); sc.scrollTop = 20; requestAnimationFrame(() => log.push('raf')); return after(100); }).then(() => log)",
                r#"["scroll", "sc-scroll", "raf"]"#,
            ),
            (
                r"after(50).then(() => { log = []; requestAnimationFrame(() => { log.push('raf1'); scrollTo(0, 200); requestAnimationFrame(() => log.push('raf2')); }); return after(150); }).then(() => log)",
                r#"["raf1", "scroll", "raf2"]"#,
            ),
            (
                r"after(50).then(() => { log = []; requestAnimationFrame(() => { log.push('raf'); setTimeout(() => log.push('timeout0'), 0); Promise.resolve().then(() => log.push('micro')); }); return after(100); }).then(() => log)",
                r#"["raf", "micro", "timeout0"]"#,
            ),
        ],
    },
    Case {
        name: "layout",
        html: PAGE_20,
        steps: &[],
        queries: &[
            (
                r"after(300).then(() => widths)",
                r"[101, 102, 103, 104, 105]",
            ),
            (
                r"frame().then(() => { d.style.width = '50px'; return frame(); }).then(() => getComputedStyle(d).width)",
                r#""50px""#,
            ),
        ],
    },
    Case {
        name: "resize-raf",
        html: PAGE_21,
        steps: &[
            (r"wait", r"100"),
            (r"eval", r#""log = []; tick()""#),
            (r"wait", r"50"),
            (r"viewport", r"[500, 600]"),
            (r"wait", r"100"),
        ],
        queries: &[(
            r"log.filter((x, i) => i === 0 || x !== log[i - 1]).slice(0, 6)",
            r#"["raf 800", "resize 500", "raf 500"]"#,
        )],
    },
];

/// The cases of `web-svg-geometry.json`.
pub(crate) const SVG_GEOMETRY: &[Case] = &[
    Case {
        name: "bbox",
        html: PAGE_22,
        steps: &[],
        queries: &[
            (r#"B("r")"#, r"[10, 20, 30, 40]"),
            (r#"B("rr")"#, r"[0, 0, 20, 10]"),
            (r#"B("c")"#, r"[40, 40, 20, 20]"),
            (r#"B("e")"#, r"[70, 15, 20, 10]"),
            (r#"B("l")"#, r"[0, 0, 30, 40]"),
            (r#"B("pl")"#, r"[0, 0, 10, 10]"),
            (r#"B("pg")"#, r"[0, 0, 10, 10]"),
            (r#"B("p")"#, r"[0, 0, 30, 40]"),
            (r#"B("pc")"#, r"[0, 0, 20, 20]"),
            (r#"B("pq")"#, r"[0, 0, 20, 5]"),
            (r#"B("pm")"#, r"[0, 0, 20, 10]"),
            (r#"B("pa")"#, r"[0, -10, 20, 10]"),
            (r#"B("g")"#, r"[1, 2, 23, 22]"),
            (r#"B("gr")"#, r"[1, 2, 3, 4]"),
            (r#"B("gc")"#, r"[8, 8, 4, 4]"),
            (r#"B("ge")"#, r"[0, 0, 0, 0]"),
            (r#"B("s")"#, r"[0, -10, 124, 103.5]"),
            (r#"B("t")"#, r"[5, 75.5, 25.84375, 18]"),
            (r#"B("hid")"#, r"[0, 0, 0, 0]"),
            (r#"B("pe")"#, r"[0, 0, 0, 0]"),
            (
                r"E(() => { const r = document.createElementNS('http://www.w3.org/2000/svg', 'rect'); r.setAttribute('width', 5); return R(r.getBBox()); })",
                r"[0, 0, 0, 0]",
            ),
            (
                r"E(() => [s.getBBox() instanceof DOMRect, s.getBBox() === s.getBBox(), typeof r.getBBox, r.getBBox.length])",
                r#"[false, false, "function", 0]"#,
            ),
            (
                r"E(() => R(r.getBBox({stroke: true})))",
                r"[10, 20, 30, 40]",
            ),
            (
                r"E(() => (r.setAttribute('width', 60), R(r.getBBox())))",
                r"[10, 20, 60, 40]",
            ),
        ],
    },
    Case {
        name: "length",
        html: PAGE_22,
        steps: &[],
        queries: &[
            (r#"L("r")"#, r"140"),
            (r#"L("rr")"#, r"54.369"),
            (r#"L("c")"#, r"62.427"),
            (r#"L("e")"#, r"47.958"),
            (r#"L("l")"#, r"50"),
            (r#"L("pl")"#, r"20"),
            (r#"L("pg")"#, r"34.142"),
            (r#"L("p")"#, r"50"),
            (r#"L("pc")"#, r"30.977"),
            (r#"L("pq")"#, r"42.956"),
            (r#"L("pm")"#, r"20"),
            (r#"L("pa")"#, r"31.42"),
            (r#"L("hid")"#, r"20"),
            (r#"L("pe")"#, r"0"),
            (
                r"E(() => [typeof p.getTotalLength, p.getTotalLength.length, 'getTotalLength' in g, 'getTotalLength' in t, 'getBBox' in g, 'getBBox' in s])",
                r#"["function", 0, false, false, true, true]"#,
            ),
            (
                r"E(() => { const p = document.createElementNS('http://www.w3.org/2000/svg', 'path'); p.setAttribute('d', 'M0 0 L3 4'); return p.getTotalLength(); })",
                r"5",
            ),
            (
                r"E(() => (p.setAttribute('d', 'M0 0 L6 8'), p.getTotalLength()))",
                r"10",
            ),
        ],
    },
    Case {
        name: "factories",
        html: PAGE_22,
        steps: &[],
        queries: &[
            (
                r"E(() => { const r = s.createSVGRect(); return [R(r), r instanceof DOMRect, String(r), r === s.createSVGRect()]; })",
                r#"[[0, 0, 0, 0], false, "[object SVGRect]", false]"#,
            ),
            (
                r"E(() => ['createSVGRect' in s, s.createSVGRect.length, 'createSVGRect' in document.createElementNS('http://www.w3.org/2000/svg', 'svg'), 'createSVGRect' in r])",
                r"[true, 0, true, false]",
            ),
            (
                r"E(() => { const r = s.createSVGRect(); r.x = 5; r.width = 7; return R(r); })",
                r"[5, 0, 7, 0]",
            ),
            (
                r"E(() => SVGSVGElement.prototype.createSVGRect.call(r))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => [typeof SVGRect, s.createSVGRect() instanceof SVGRect, s.getBBox() instanceof SVGRect, Object.getOwnPropertyNames(SVGRect.prototype).sort(), Object.getPrototypeOf(SVGRect.prototype) === Object.prototype])",
                r#"["function", true, true, ["constructor", "height", "width", "x", "y"], true]"#,
            ),
            (
                r"E(() => { const r = s.createSVGRect(); r.x = 'a'; r.y = 1.5; r.height = -3; return R(r); })",
                r#""TypeError: Failed to set the 'x' property on 'SVGRect': The provided float value is non-finite.""#,
            ),
            (
                r"E(() => new SVGRect())",
                r#""TypeError: Failed to construct 'SVGRect': Illegal constructor""#,
            ),
            (r"E(() => JSON.stringify(s.createSVGRect()))", r#""{}""#),
            (
                r"E(() => SVGGraphicsElement.prototype.getBBox.call(document.body))",
                r#""TypeError: Illegal invocation""#,
            ),
        ],
    },
];
