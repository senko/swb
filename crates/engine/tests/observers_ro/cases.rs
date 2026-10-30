// The cases of tools/probes/web-resize-observer.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "ro-structure",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}</style><div id=e1 style="width:100px;height:50px"></div><div id=e0 style="width:0;height:0"></div><div id=e2 style="display:none;width:30px;height:30px"></div><div id=e3 style="width:100px;height:50px;padding:5px 7px;border:3px solid;margin:4px"></div><div id=e4 style="width:33.333px;height:20.5px"></div><span id=e5>inline text</span><svg id=sv width=200 height=100><rect id=r1 x=10 y=10 width=50 height=30></rect><g id=g1><rect width=20 height=20></rect></g></svg><div id=e6 style="width:100px;height:50px;box-sizing:border-box;padding:10px;border:2px solid"></div><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function d(n) { if (!n) return 'null'; if (n.nodeType == 1) return n.localName + (n.id ? '#' + n.id : ''); if (n.nodeType == 3) return '#text(' + n.data + ')'; if (n.nodeType == 8) return '#comment(' + n.data + ')'; if (n.nodeType == 9) return '#document'; if (n.nodeType == 11) return '#fragment'; return n.nodeName; }
function dl(l) { return Array.from(l).map(d).join('|'); }
function R(r) { return [r.type, d(r.target), dl(r.addedNodes), dl(r.removedNodes), d(r.previousSibling), d(r.nextSibling), r.attributeName, r.attributeNamespace, r.oldValue].join(' ; '); }
function RS(recs) { return recs.map(R); }
const ALL = {childList: true, subtree: true, attributes: true, attributeOldValue: true, characterData: true, characterDataOldValue: true};
var mo = new MutationObserver(function (recs, o) { L.push('cb ' + recs.length + ' ' + (this === o)); recs.forEach(r => L.push(R(r))); });

const sz = (s) => s ? (Array.isArray(s) ? s.map((x) => x.inlineSize + 'x' + x.blockSize).join('|') : 'notarray') : 'null';
const cr = (r) => [r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left].map((n) => +n.toFixed(2)).join(',');
const ent = (e) => [e.target.id || e.target.localName, 'cr=' + cr(e.contentRect), 'bb=' + sz(e.borderBoxSize), 'cb=' + sz(e.contentBoxSize), 'dp=' + sz(e.devicePixelContentBoxSize)].join(' ');
const OBS = [];
const SAVED = {};
for (const el of document.querySelectorAll('[id]')) SAVED[el.id] = [el, el.style.cssText];
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  for (const id in SAVED) { const [el, css] = SAVED[id]; el.style.cssText = css; if (!el.isConnected) document.body.appendChild(el); }
  window.onerror = null;
}
function make(opts, name) {
  const o = new ResizeObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); });
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(100);
</script></body></html>"#,
        queries: &[
            (
                r"[typeof ResizeObserver, ResizeObserver.length, ResizeObserver.name, typeof ResizeObserverEntry, typeof ResizeObserverSize]",
                r#"["function", 1, "ResizeObserver", "function", "function"]"#,
            ),
            (
                r"Object.getOwnPropertyNames(ResizeObserver.prototype).sort().join()",
                r#""constructor,disconnect,observe,unobserve""#,
            ),
            (
                r"Object.getOwnPropertyNames(ResizeObserverEntry.prototype).sort().join()",
                r#""borderBoxSize,constructor,contentBoxSize,contentRect,devicePixelContentBoxSize,target""#,
            ),
            (
                r"Object.getOwnPropertyNames(ResizeObserverSize.prototype).sort().join()",
                r#""blockSize,constructor,inlineSize""#,
            ),
            (
                r"['observe', 'unobserve', 'disconnect'].map((n) => ResizeObserver.prototype[n].length).join()",
                r#""1,1,0""#,
            ),
            (
                r"E(() => new ResizeObserver())",
                r#""TypeError: Failed to construct 'ResizeObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new ResizeObserver(1))",
                r#""TypeError: Failed to construct 'ResizeObserver': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => ResizeObserver(() => {}))",
                r#""TypeError: Failed to construct 'ResizeObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new ResizeObserverEntry())",
                r#""TypeError: Failed to construct 'ResizeObserverEntry': Illegal constructor""#,
            ),
            (
                r"E(() => new ResizeObserverSize())",
                r#""TypeError: Failed to construct 'ResizeObserverSize': Illegal constructor""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe())",
                r#""TypeError: Failed to execute 'observe' on 'ResizeObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(1))",
                r#""TypeError: Failed to execute 'observe' on 'ResizeObserver': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(document))",
                r#""TypeError: Failed to execute 'observe' on 'ResizeObserver': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(e1, {box: 'x'}))",
                r#""TypeError: Failed to execute 'observe' on 'ResizeObserver': Failed to read the 'box' property from 'ResizeObserverOptions': The provided value 'x' is not a valid enum value of type ResizeObserverBoxOptions.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(e1, {box: 'padding-box'}))",
                r#""TypeError: Failed to execute 'observe' on 'ResizeObserver': Failed to read the 'box' property from 'ResizeObserverOptions': The provided value 'padding-box' is not a valid enum value of type ResizeObserverBoxOptions.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(e1, {box: 'device-pixel-content-box'}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(e1, 1))",
                r#""TypeError: Failed to execute 'observe' on 'ResizeObserver': The provided value is not of type 'ResizeObserverOptions'.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(e1, null))",
                r#""undefined""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).observe(e1, {}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).unobserve())",
                r#""TypeError: Failed to execute 'unobserve' on 'ResizeObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).unobserve(e1))",
                r#""undefined""#,
            ),
            (
                r"E(() => new ResizeObserver(() => {}).unobserve(1))",
                r#""TypeError: Failed to execute 'unobserve' on 'ResizeObserver': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => { const o = new ResizeObserver(() => {}); return [o.observe(e1), o.unobserve(e1), o.disconnect()]; })",
                r"[null, null, null]",
            ),
            (
                r"E(() => ResizeObserver.prototype.observe.call({}, e1))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.prototype.toString.call(new ResizeObserver(() => {})))",
                r#""[object ResizeObserver]""#,
            ),
            (
                r"E(() => [Object.prototype.toString.call(ResizeObserverEntry.prototype), Object.prototype.toString.call(ResizeObserverSize.prototype)])",
                r#"["[object ResizeObserverEntry]", "[object ResizeObserverSize]"]"#,
            ),
            (
                r"Object.getOwnPropertyNames(ResizeObserverEntry.prototype).filter((n) => Object.getOwnPropertyDescriptor(ResizeObserverEntry.prototype, n).get).join()",
                r#""target,contentRect,contentBoxSize,borderBoxSize,devicePixelContentBoxSize""#,
            ),
        ],
    },
    Case {
        name: "ro-basic",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}</style><div id=e1 style="width:100px;height:50px"></div><div id=e0 style="width:0;height:0"></div><div id=e2 style="display:none;width:30px;height:30px"></div><div id=e3 style="width:100px;height:50px;padding:5px 7px;border:3px solid;margin:4px"></div><div id=e4 style="width:33.333px;height:20.5px"></div><span id=e5>inline text</span><svg id=sv width=200 height=100><rect id=r1 x=10 y=10 width=50 height=30></rect><g id=g1><rect width=20 height=20></rect></g></svg><div id=e6 style="width:100px;height:50px;box-sizing:border-box;padding:10px;border:2px solid"></div><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function d(n) { if (!n) return 'null'; if (n.nodeType == 1) return n.localName + (n.id ? '#' + n.id : ''); if (n.nodeType == 3) return '#text(' + n.data + ')'; if (n.nodeType == 8) return '#comment(' + n.data + ')'; if (n.nodeType == 9) return '#document'; if (n.nodeType == 11) return '#fragment'; return n.nodeName; }
function dl(l) { return Array.from(l).map(d).join('|'); }
function R(r) { return [r.type, d(r.target), dl(r.addedNodes), dl(r.removedNodes), d(r.previousSibling), d(r.nextSibling), r.attributeName, r.attributeNamespace, r.oldValue].join(' ; '); }
function RS(recs) { return recs.map(R); }
const ALL = {childList: true, subtree: true, attributes: true, attributeOldValue: true, characterData: true, characterDataOldValue: true};
var mo = new MutationObserver(function (recs, o) { L.push('cb ' + recs.length + ' ' + (this === o)); recs.forEach(r => L.push(R(r))); });

const sz = (s) => s ? (Array.isArray(s) ? s.map((x) => x.inlineSize + 'x' + x.blockSize).join('|') : 'notarray') : 'null';
const cr = (r) => [r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left].map((n) => +n.toFixed(2)).join(',');
const ent = (e) => [e.target.id || e.target.localName, 'cr=' + cr(e.contentRect), 'bb=' + sz(e.borderBoxSize), 'cb=' + sz(e.contentBoxSize), 'dp=' + sz(e.devicePixelContentBoxSize)].join(' ');
const OBS = [];
const SAVED = {};
for (const el of document.querySelectorAll('[id]')) SAVED[el.id] = [el, el.style.cssText];
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  for (const id in SAVED) { const [el, css] = SAVED[id]; el.style.cssText = css; if (!el.isConnected) document.body.appendChild(el); }
  window.onerror = null;
}
function make(opts, name) {
  const o = new ResizeObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); });
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(100);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); [e1, e0, e2, e3, e4, e5, e6].forEach((t) => o.observe(t)); await settle(); reset(); return L; })()",
                r#"["cb 7", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "e0 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "e2 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "e3 cr=7,5,100,50,5,107,55,7 bb=120x66 cb=100x50 dp=100x50", "e4 cr=0,0,33.33,20.5,0,33.33,20.5,0 bb=33.328125x20.5 cb=33.328125x20.5 dp=33x21", "e5 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "e6 cr=10,10,76,26,10,86,36,10 bb=100x50 cb=76x26 dp=76x26"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); o.observe(e1); o.observe(e1); await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e3, {box: 'border-box'}); o.observe(e4, {box: 'border-box'}); o.observe(e6, {box: 'border-box'}); await settle(); reset(); return L; })()",
                r#"["cb 3", "e3 cr=7,5,100,50,5,107,55,7 bb=120x66 cb=100x50 dp=100x50", "e4 cr=0,0,33.33,20.5,0,33.33,20.5,0 bb=33.328125x20.5 cb=33.328125x20.5 dp=33x21", "e6 cr=10,10,76,26,10,86,36,10 bb=100x50 cb=76x26 dp=76x26"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e3, {box: 'device-pixel-content-box'}); o.observe(e4, {box: 'device-pixel-content-box'}); await settle(); reset(); return L; })()",
                r#"["cb 2", "e3 cr=7,5,100,50,5,107,55,7 bb=120x66 cb=100x50 dp=100x50", "e4 cr=0,0,33.33,20.5,0,33.33,20.5,0 bb=33.328125x20.5 cb=33.328125x20.5 dp=33x21"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(sv); o.observe(r1); o.observe(g1); await settle(); reset(); return L; })()",
                r#"["cb 3", "sv cr=0,0,200,100,0,200,100,0 bb=200x100 cb=200x100 dp=200x100", "r1 cr=0,0,50,30,0,50,30,0 bb=50x30 cb=50x30 dp=50x30", "g1 cr=0,0,20,20,0,20,20,0 bb=20x20 cb=20x20 dp=20x20"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e0); o.observe(e2); await settle(); L.push('-- show'); e2.style.display = 'block'; await settle(); L.push('-- hide'); e2.style.display = 'none'; await settle(); L.push('-- grow'); e0.style.width = '10px'; await settle(); reset(); return L; })()",
                r#"["cb 2", "e0 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "e2 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "-- show", "cb 1", "e2 cr=0,0,30,30,0,30,30,0 bb=30x30 cb=30x30 dp=30x30", "-- hide", "cb 1", "e2 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "-- grow", "cb 1", "e0 cr=0,0,10,0,0,10,0,0 bb=10x0 cb=10x0 dp=10x0"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; let got = null; const o = new ResizeObserver((es) => { got = es[0]; }); OBS.push(o); o.observe(e3); await settle(); L.push([got.contentBoxSize === got.contentBoxSize, Array.isArray(got.contentBoxSize), Object.isFrozen(got.contentBoxSize), got.contentBoxSize.length, got.contentBoxSize[0] instanceof ResizeObserverSize, Object.isFrozen(got.contentBoxSize[0]), got.contentRect instanceof DOMRectReadOnly, got.contentRect === got.contentRect, typeof got.borderBoxSize[0].inlineSize, got.target === e3].join()); reset(); return L; })()",
                r#"["true,true,true,1,true,false,true,true,number,true"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); await settle(); L.push('-- width'); e1.style.width = '120px'; await settle(); L.push('-- same'); e1.style.width = '120px'; await settle(); L.push('-- height'); e1.style.height = '60px'; await settle(); L.push('-- both back'); e1.style.cssText = 'width:100px;height:50px'; await settle(); L.push('-- background only'); e1.style.background = 'red'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- width", "cb 1", "e1 cr=0,0,120,50,0,120,50,0 bb=120x50 cb=120x50 dp=120x50", "-- same", "-- height", "cb 1", "e1 cr=0,0,120,60,0,120,60,0 bb=120x60 cb=120x60 dp=120x60", "-- both back", "cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- background only"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e3); await settle(); L.push('-- border'); e3.style.borderWidth = '6px'; await settle(); L.push('-- padding'); e3.style.padding = '8px 9px'; await settle(); L.push('-- margin'); e3.style.margin = '20px'; await settle(); L.push('-- transform'); e3.style.transform = 'scale(2)'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e3 cr=7,5,100,50,5,107,55,7 bb=120x66 cb=100x50 dp=100x50", "-- border", "-- padding", "-- margin", "-- transform"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e3, {box: 'border-box'}); await settle(); L.push('-- padding'); e3.style.padding = '8px 9px'; await settle(); L.push('-- width'); e3.style.width = '90px'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e3 cr=7,5,100,50,5,107,55,7 bb=120x66 cb=100x50 dp=100x50", "-- padding", "cb 1", "e3 cr=9,8,100,50,8,109,58,9 bb=124x72 cb=100x50 dp=100x50", "-- width", "cb 1", "e3 cr=9,8,90,50,8,99,58,9 bb=114x72 cb=90x50 dp=90x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e3, {box: 'content-box'}); await settle(); L.push('-- padding same box'); e3.style.padding = '8px 9px'; await settle(); L.push('-- border-box too'); o.observe(e3, {box: 'border-box'}); await settle(); L.push('-- width'); e3.style.width = '90px'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e3 cr=7,5,100,50,5,107,55,7 bb=120x66 cb=100x50 dp=100x50", "-- padding same box", "-- border-box too", "cb 1", "e3 cr=9,8,100,50,8,109,58,9 bb=124x72 cb=100x50 dp=100x50", "-- width", "cb 1", "e3 cr=9,8,90,50,8,99,58,9 bb=114x72 cb=90x50 dp=90x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); o.observe(e0); await settle(); L.push('-- unobserve'); o.unobserve(e1); e1.style.width = '10px'; e0.style.width = '10px'; await settle(); L.push('-- disconnect'); o.disconnect(); e0.style.width = '20px'; await settle(); L.push('-- observe'); o.observe(e0); await settle(); reset(); return L; })()",
                r#"["cb 2", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "e0 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "-- unobserve", "cb 1", "e0 cr=0,0,10,0,0,10,0,0 bb=10x0 cb=10x0 dp=10x0", "-- disconnect", "-- observe", "cb 1", "e0 cr=0,0,20,0,0,20,0,0 bb=20x0 cb=20x0 dp=20x0"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); await settle(); o.unobserve(e1); o.observe(e1); await settle(); L.push('-- again'); o.disconnect(); o.observe(e1); await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- again", "cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); const d = document.createElement('div'); d.style.cssText = 'width:10px;height:10px'; o.observe(d); await settle(); document.body.appendChild(d); await settle(); d.remove(); await settle(); L.push('-- reinsert'); document.body.appendChild(d); await settle(); d.remove(); reset(); return L; })()",
                r#"["cb 1", "div cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "cb 1", "div cr=0,0,10,10,0,10,10,0 bb=10x10 cb=10x10 dp=10x10", "cb 1", "div cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "-- reinsert", "cb 1", "div cr=0,0,10,10,0,10,10,0 bb=10x10 cb=10x10 dp=10x10"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e5); await settle(); L.push('-- more text'); e5.textContent = 'a much longer inline text'; await settle(); L.push('-- block'); e5.style.display = 'block'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e5 cr=0,0,0,0,0,0,0,0 bb=0x0 cb=0x0 dp=0x0", "-- more text", "-- block", "cb 1", "e5 cr=0,0,400,18,0,400,18,0 bb=400x18 cb=400x18 dp=400x18"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e4); await settle(); e4.style.width = '33.3334px'; await settle(); L.push('-- tiny'); e4.style.width = '33.33401px'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e4 cr=0,0,33.33,20.5,0,33.33,20.5,0 bb=33.328125x20.5 cb=33.328125x20.5 dp=33x21", "-- tiny"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); await settle(); L.push('-- writing mode'); e1.style.writingMode = 'vertical-rl'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- writing mode", "cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=50x100 cb=50x100 dp=50x100"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); await settle(); L.push('-- zoom'); e1.style.zoom = '2'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- zoom"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); await settle(); L.push('-- viewport'); e1.style.width = '50%'; await settle(); L.push('-- same size'); e1.style.width = '200px'; await settle(); L.push('-- fraction'); e1.style.width = '50.5%'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- viewport", "cb 1", "e1 cr=0,0,200,50,0,200,50,0 bb=200x50 cb=200x50 dp=200x50", "-- same size", "-- fraction", "cb 1", "e1 cr=0,0,202,50,0,202,50,0 bb=202x50 cb=202x50 dp=202x50"]"#,
            ),
        ],
    },
    Case {
        name: "ro-nest",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}</style><div id=e1 style="width:100px;height:50px"></div><div id=e0 style="width:0;height:0"></div><div id=e2 style="display:none;width:30px;height:30px"></div><div id=e3 style="width:100px;height:50px;padding:5px 7px;border:3px solid;margin:4px"></div><div id=e4 style="width:33.333px;height:20.5px"></div><span id=e5>inline text</span><svg id=sv width=200 height=100><rect id=r1 x=10 y=10 width=50 height=30></rect><g id=g1><rect width=20 height=20></rect></g></svg><div id=e6 style="width:100px;height:50px;box-sizing:border-box;padding:10px;border:2px solid"></div><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function d(n) { if (!n) return 'null'; if (n.nodeType == 1) return n.localName + (n.id ? '#' + n.id : ''); if (n.nodeType == 3) return '#text(' + n.data + ')'; if (n.nodeType == 8) return '#comment(' + n.data + ')'; if (n.nodeType == 9) return '#document'; if (n.nodeType == 11) return '#fragment'; return n.nodeName; }
function dl(l) { return Array.from(l).map(d).join('|'); }
function R(r) { return [r.type, d(r.target), dl(r.addedNodes), dl(r.removedNodes), d(r.previousSibling), d(r.nextSibling), r.attributeName, r.attributeNamespace, r.oldValue].join(' ; '); }
function RS(recs) { return recs.map(R); }
const ALL = {childList: true, subtree: true, attributes: true, attributeOldValue: true, characterData: true, characterDataOldValue: true};
var mo = new MutationObserver(function (recs, o) { L.push('cb ' + recs.length + ' ' + (this === o)); recs.forEach(r => L.push(R(r))); });

const sz = (s) => s ? (Array.isArray(s) ? s.map((x) => x.inlineSize + 'x' + x.blockSize).join('|') : 'notarray') : 'null';
const cr = (r) => [r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left].map((n) => +n.toFixed(2)).join(',');
const ent = (e) => [e.target.id || e.target.localName, 'cr=' + cr(e.contentRect), 'bb=' + sz(e.borderBoxSize), 'cb=' + sz(e.contentBoxSize), 'dp=' + sz(e.devicePixelContentBoxSize)].join(' ');
const OBS = [];
const SAVED = {};
for (const el of document.querySelectorAll('[id]')) SAVED[el.id] = [el, el.style.cssText];
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  for (const id in SAVED) { const [el, css] = SAVED[id]; el.style.cssText = css; if (!el.isConnected) document.body.appendChild(el); }
  window.onerror = null;
}
function make(opts, name) {
  const o = new ResizeObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); });
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(100);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; const p = document.createElement('div'); p.id = 'p'; p.style.cssText = 'width:100px;height:100px'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'width:100%;height:50%'; p.appendChild(c); document.body.appendChild(p); const o = make({}); o.observe(c); o.observe(p); await settle(); L.push('-- resize parent'); p.style.width = '200px'; await settle(); p.remove(); reset(); return L; })()",
                r#"["cb 2", "c cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "p cr=0,0,100,100,0,100,100,0 bb=100x100 cb=100x100 dp=100x100", "-- resize parent", "cb 2", "c cr=0,0,200,50,0,200,50,0 bb=200x50 cb=200x50 dp=200x50", "p cr=0,0,200,100,0,200,100,0 bb=200x100 cb=200x100 dp=200x100"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'a'); const q = make({}, 'b'); o.observe(e1); q.observe(e1); await settle(); L.push('-- resize'); e1.style.width = '70px'; await settle(); reset(); return L; })()",
                r#"["a 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "b 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- resize", "a 1", "e1 cr=0,0,70,50,0,70,50,0 bb=70x50 cb=70x50 dp=70x50", "b 1", "e1 cr=0,0,70,50,0,70,50,0 bb=70x50 cb=70x50 dp=70x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = new ResizeObserver(() => { L.push('cb'); e1.style.width = '70px'; e0.style.width = '30px'; }); OBS.push(o); o.observe(e1); o.observe(e0); await settle(); reset(); return L; })()",
                r#"["cb", "cb"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; let cbs = 0, errs = 0; const msgs = new Set(); const o = new ResizeObserver(() => { cbs++; e1.style.width = (e1.offsetWidth + 1) + 'px'; }); OBS.push(o); window.addEventListener('error', function h(e) { errs++; msgs.add(['error', e.message, e instanceof ErrorEvent, e.error, e.filename === '' ? 'nofile' : 'file', e.lineno, e.colno, e.bubbles, e.cancelable, e.isTrusted, e.target === window].join()); }); o.observe(e1); await frames(6); await wait(50); L.push('cbs>=4 ' + (cbs >= 4), 'errs>=3 ' + (errs >= 3), 'balanced ' + (Math.abs(cbs - errs) <= 1)); L.push(...msgs); reset(); return L; })()",
                r#"["cbs>=4 true", "errs>=3 true", "balanced true", "error,ResizeObserver loop completed with undelivered notifications.,true,,file,0,0,false,true,true,true"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; let cbs = 0; const msgs = new Set(); window.onerror = (m, f, l, c, err) => { msgs.add('onerror ' + m + ' | ' + typeof err + ' ' + l + ' ' + c); return true; }; const o = new ResizeObserver(() => { cbs++; e1.style.width = (e1.offsetWidth + 1) + 'px'; }); OBS.push(o); o.observe(e1); await frames(5); await wait(50); o.disconnect(); const n = cbs; await frames(3); L.push('cbs>=3 ' + (cbs >= 3), 'stopped ' + (cbs === n)); L.push(...msgs); reset(); return L; })()",
                r#"["cbs>=3 true", "stopped true", "onerror ResizeObserver loop completed with undelivered notifications. | object 0 0"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; let n = 0; const o = new ResizeObserver(() => { n++; e1.style.width = (e1.offsetWidth + 1) + 'px'; }); OBS.push(o); window.onerror = () => true; o.observe(e1); await frames(5); await wait(50); L.push('n>=3 ' + (n >= 3)); const n1 = n; await frames(4); L.push('goes on ' + (n > n1)); o.disconnect(); reset(); return L; })()",
                r#"["n>=3 true", "goes on true"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const p = document.createElement('div'); p.id = 'p'; p.style.cssText = 'width:100px;height:100px'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'width:50px;height:20px'; p.appendChild(c); document.body.appendChild(p); const o = new ResizeObserver((es) => { L.push('parent ' + es.length); c.style.width = (c.offsetWidth + 1) + 'px'; }); const q = new ResizeObserver((es) => { L.push('child ' + es.length); }); OBS.push(o, q); window.onerror = (m) => { L.push('onerror ' + m); return true; }; q.observe(c); await settle(); L.push('-- observe parent'); o.observe(p); await settle(); L.push('-- resize'); p.style.width = '150px'; await settle(); p.remove(); reset(); return L; })()",
                r#"["child 1", "-- observe parent", "parent 1", "child 1", "-- resize", "parent 1", "child 1"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = new ResizeObserver(() => { L.push('cb'); throw new Error('boom'); }); OBS.push(o); window.onerror = (m) => { L.push('onerror ' + m); return true; }; o.observe(e1); await settle(); e1.style.width = '20px'; await settle(); reset(); return L; })()",
                r#"["cb", "onerror Uncaught Error: boom", "cb", "onerror Uncaught Error: boom"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(e1); await settle(); L.push('-- resize in microtask'); e1.style.width = '20px'; await Promise.resolve(); e1.style.width = '30px'; await settle(); reset(); return L; })()",
                r#"["cb 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- resize in microtask", "cb 1", "e1 cr=0,0,30,50,0,30,50,0 bb=30x50 cb=30x50 dp=30x50"]"#,
            ),
        ],
    },
    Case {
        name: "ro-order",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}</style><div id=e1 style="width:100px;height:50px"></div><div id=e0 style="width:0;height:0"></div><div id=e2 style="display:none;width:30px;height:30px"></div><div id=e3 style="width:100px;height:50px;padding:5px 7px;border:3px solid;margin:4px"></div><div id=e4 style="width:33.333px;height:20.5px"></div><span id=e5>inline text</span><svg id=sv width=200 height=100><rect id=r1 x=10 y=10 width=50 height=30></rect><g id=g1><rect width=20 height=20></rect></g></svg><div id=e6 style="width:100px;height:50px;box-sizing:border-box;padding:10px;border:2px solid"></div><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
function d(n) { if (!n) return 'null'; if (n.nodeType == 1) return n.localName + (n.id ? '#' + n.id : ''); if (n.nodeType == 3) return '#text(' + n.data + ')'; if (n.nodeType == 8) return '#comment(' + n.data + ')'; if (n.nodeType == 9) return '#document'; if (n.nodeType == 11) return '#fragment'; return n.nodeName; }
function dl(l) { return Array.from(l).map(d).join('|'); }
function R(r) { return [r.type, d(r.target), dl(r.addedNodes), dl(r.removedNodes), d(r.previousSibling), d(r.nextSibling), r.attributeName, r.attributeNamespace, r.oldValue].join(' ; '); }
function RS(recs) { return recs.map(R); }
const ALL = {childList: true, subtree: true, attributes: true, attributeOldValue: true, characterData: true, characterDataOldValue: true};
var mo = new MutationObserver(function (recs, o) { L.push('cb ' + recs.length + ' ' + (this === o)); recs.forEach(r => L.push(R(r))); });

const sz = (s) => s ? (Array.isArray(s) ? s.map((x) => x.inlineSize + 'x' + x.blockSize).join('|') : 'notarray') : 'null';
const cr = (r) => [r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left].map((n) => +n.toFixed(2)).join(',');
const ent = (e) => [e.target.id || e.target.localName, 'cr=' + cr(e.contentRect), 'bb=' + sz(e.borderBoxSize), 'cb=' + sz(e.contentBoxSize), 'dp=' + sz(e.devicePixelContentBoxSize)].join(' ');
const OBS = [];
const SAVED = {};
for (const el of document.querySelectorAll('[id]')) SAVED[el.id] = [el, el.style.cssText];
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  for (const id in SAVED) { const [el, css] = SAVED[id]; el.style.cssText = css; if (!el.isConnected) document.body.appendChild(el); }
  window.onerror = null;
}
function make(opts, name) {
  const o = new ResizeObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); });
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(100);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); requestAnimationFrame(() => L.push('raf1')); setTimeout(() => L.push('timeout0'), 0); Promise.resolve().then(() => L.push('micro')); await settle(); reset(); return L; })()",
                r#"["micro", "timeout0", "raf1", "ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); await frames(1); L.push('frame1'); await frames(1); L.push('frame2'); await settle(); reset(); return L; })()",
                r#"["frame1", "ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "frame2"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); requestAnimationFrame(() => { L.push('raf1'); e1.style.width = '70px'; }); await settle(); reset(); return L; })()",
                r#"["raf1", "ro 1", "e1 cr=0,0,70,50,0,70,50,0 bb=70x50 cb=70x50 dp=70x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); await settle(); L.push('-- change'); e1.style.width = '70px'; setTimeout(() => L.push('timeout0'), 0); requestAnimationFrame(() => L.push('raf')); await settle(); reset(); return L; })()",
                r#"["ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- change", "timeout0", "raf", "ro 1", "e1 cr=0,0,70,50,0,70,50,0 bb=70x50 cb=70x50 dp=70x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); await settle(); L.push('-- change in task'); setTimeout(() => { e1.style.width = '70px'; L.push('changed'); }, 0); await settle(); reset(); return L; })()",
                r#"["ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- change in task", "changed", "ro 1", "e1 cr=0,0,70,50,0,70,50,0 bb=70x50 cb=70x50 dp=70x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); const i = new IntersectionObserver((es) => L.push('io ' + es.length)); OBS.push(i); i.observe(e1); o.observe(e1); await settle(); reset(); return L; })()",
                r#"["ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "io 1"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); await settle(); L.push('-- change'); e1.style.width = '70px'; L.push('sync offsetWidth ' + e1.offsetWidth); await settle(); reset(); return L; })()",
                r#"["ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- change", "sync offsetWidth 70", "ro 1", "e1 cr=0,0,70,50,0,70,50,0 bb=70x50 cb=70x50 dp=70x50"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'ro'); o.observe(e1); await settle(); L.push('-- via getBoundingClientRect'); e1.style.width = '71px'; e1.getBoundingClientRect(); L.push('queried'); await settle(); reset(); return L; })()",
                r#"["ro 1", "e1 cr=0,0,100,50,0,100,50,0 bb=100x50 cb=100x50 dp=100x50", "-- via getBoundingClientRect", "queried", "ro 1", "e1 cr=0,0,71,50,0,71,50,0 bb=71x50 cb=71x50 dp=71x50"]"#,
            ),
        ],
    },
    Case {
        name: "ro-extra",
        html: r"<!DOCTYPE html><html><head><style>html,body{margin:0}</style></head><body></body><script>
window.L=[];
const rf=(r)=>r?[r.x,r.y,r.width,r.height].map(n=>+n.toFixed(3)).join(','):'null';
const sz=(a)=>a.map(s=>s.inlineSize+'x'+s.blockSize).join('|');
const ent=(e)=>[e.target.id,rf(e.contentRect),'b:'+sz(e.borderBoxSize),'c:'+sz(e.contentBoxSize),'d:'+sz(e.devicePixelContentBoxSize)].join(' ');
function wait(ms){return new Promise(r=>setTimeout(r,ms));}
async function run(html, opts, targets, after){
  document.body.innerHTML=html; L.length=0;
  await wait(50);
  const o=new ResizeObserver((es)=>{L.push('cb '+es.length); es.forEach(e=>L.push(ent(e)));});
  targets.forEach(t=>o.observe(document.getElementById(t),opts));
  await wait(100);
  if(after){ await after(o); await wait(100);}
  o.disconnect(); return L.slice();
}
const $=(i)=>document.getElementById(i);
</script></html>",
        queries: &[
            (
                r#"(async()=>run('<div id=t style="width:100.4px;height:33.33px;padding:3.3px;border:1.7px solid"></div>',{box:'border-box'},['t']))()"#,
                r#"["cb 1", "t 3.297,3.297,100.391,33.328 b:108.984375x41.921875 c:100.390625x33.328125 d:100x33"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100.4px;height:33.33px;padding:3.3px;border:1.7px solid"></div>',{box:'device-pixel-content-box'},['t']))()"#,
                r#"["cb 1", "t 3.297,3.297,100.391,33.328 b:108.984375x41.921875 c:100.390625x33.328125 d:100x33"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100px;height:50px;overflow:scroll"><div style="height:500px"></div></div>',{},['t']))()"#,
                r#"["cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="display:contents"><span>a</span></div><span id=u>abc</span><div id=v style="display:table-cell;width:30px;height:20px"></div>',{},['t','u','v']))()"#,
                r#"["cb 3", "t 0,0,0,0 b:0x0 c:0x0 d:0x0", "u 0,0,0,0 b:0x0 c:0x0 d:0x0", "v 0,0,30,20 b:30x20 c:30x20 d:30x20"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100px;height:50px;transform:scale(2)"></div>',{},['t'],async()=>{$('t').style.transform='scale(3)';}))()"#,
                r#"["cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100px;height:50px"></div>',{},['t'],async()=>{$('t').style.height='50.001px';await wait(50);$('t').style.height='50.01px';}))()"#,
                r#"["cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100px;height:50px"></div>',{box:'border-box'},['t'],async()=>{$('t').style.padding='5px';await wait(50);$('t').style.border='2px solid'; }))()"#,
                r#"["cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50", "cb 1", "t 5,5,100,50 b:110x60 c:100x50 d:100x50", "cb 1", "t 5,5,100,50 b:114x64 c:100x50 d:100x50"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100px;height:50px"></div>',{},['t'],async(o)=>{o.observe($('t'),{box:'border-box'});await wait(50);$('t').style.width='101px'}))()"#,
                r#"["cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50", "cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50", "cb 1", "t 0,0,101,50 b:101x50 c:101x50 d:101x50"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:100px;height:50px"></div>',{},['t'],async(o)=>{$('t').style.display='none';await wait(50);$('t').style.display='block';}))()"#,
                r#"["cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50", "cb 1", "t 0,0,0,0 b:0x0 c:0x0 d:0x0", "cb 1", "t 0,0,100,50 b:100x50 c:100x50 d:100x50"]"#,
            ),
            (
                r"(async()=>run('<input id=t value=abc><textarea id=u></textarea><img id=v width=30 height=20><button id=w>b</button>',{},['t','u','v','w']))()",
                r#"["cb 4", "t 2,1,177,15 b:185x21 c:177x15 d:177x15", "u 2,2,175,30 b:181x36 c:175x30 d:175x30", "v 0,0,30,20 b:30x20 c:30x20 d:30x20", "w 6,1,7.422,15 b:23.421875x21 c:7.421875x15 d:7x15"]"#,
            ),
            (
                r"(async()=>run('<svg id=t width=100 height=50><rect id=u width=30 height=20 /></svg>',{},['t','u']))()",
                r#"["cb 2", "t 0,0,100,50 b:100x50 c:100x50 d:100x50", "u 0,0,30,20 b:30x20 c:30x20 d:30x20"]"#,
            ),
            (
                r#"(async()=>run('<div id=a style="width:100px"><div id=b style="width:50px"><div id=c style="width:20px"></div></div></div>',{},['a','b','c'],async()=>{}))()"#,
                r#"["cb 3", "a 0,0,100,0 b:100x0 c:100x0 d:100x0", "b 0,0,50,0 b:50x0 c:50x0 d:50x0", "c 0,0,20,0 b:20x0 c:20x0 d:20x0"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:50%;height:10vh;font-size:20px;line-height:2"></div>',{},['t'],async()=>{window.resizeBy&&0}))()"#,
                r#"["cb 1", "t 0,0,200,20 b:200x20 c:200x20 d:200x20"]"#,
            ),
        ],
    },
];
