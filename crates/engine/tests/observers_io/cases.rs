// The cases of tools/probes/web-intersection-observer.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "io-structure",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}.b{position:absolute;left:10px;width:100px;height:100px;background:#ccc}</style><div class=b id=t1 style="top:0"></div><div class=b id=t2 style="top:150px"></div><div class=b id=t3 style="top:400px"></div><div class=b id=t4 style="top:200px"></div><div class=b id=t5 style="display:none"></div><div id=spacer style="height:2000px"></div><script>
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

const rf = (r) => r ? [r.x, r.y, r.width, r.height].map((n) => +n.toFixed(2)).join(',') : 'null';
const ent = (e) => [e.target.id || e.target.localName, e.isIntersecting, +e.intersectionRatio.toFixed(3), rf(e.boundingClientRect), rf(e.intersectionRect), rf(e.rootBounds), typeof e.time].join(' ');
// make(opts): an observer that logs its batches into L.
const OBS = [];
const ORIG = {};
const ELS = {};
for (const id of ['t1', 't2', 't3', 't4', 't5']) { ORIG[id] = document.getElementById(id).style.cssText; ELS[id] = document.getElementById(id); }
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  window.scrollTo(0, 0);
  for (const id in ORIG) { if (!ELS[id].isConnected) document.body.insertBefore(ELS[id], spacer); ELS[id].style.cssText = ORIG[id]; }
  window.onerror = null;
}
function make(opts, name) {
  const o = new IntersectionObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); }, opts);
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(120);
</script></body></html>"#,
        queries: &[
            (
                r"[typeof IntersectionObserver, IntersectionObserver.length, IntersectionObserver.name, typeof IntersectionObserverEntry, IntersectionObserverEntry.length]",
                r#"["function", 1, "IntersectionObserver", "function", 0]"#,
            ),
            (
                r"Object.getOwnPropertyNames(IntersectionObserver.prototype).sort().join()",
                r#""constructor,delay,disconnect,observe,root,rootMargin,scrollMargin,takeRecords,thresholds,trackVisibility,unobserve""#,
            ),
            (
                r"Object.getOwnPropertyNames(IntersectionObserverEntry.prototype).sort().join()",
                r#""boundingClientRect,constructor,intersectionRatio,intersectionRect,isIntersecting,isVisible,rootBounds,target,time""#,
            ),
            (
                r"['observe', 'unobserve', 'disconnect', 'takeRecords'].map((n) => IntersectionObserver.prototype[n].length).join()",
                r#""1,1,0,0""#,
            ),
            (
                r"E(() => new IntersectionObserver())",
                r#""TypeError: Failed to construct 'IntersectionObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new IntersectionObserver(1))",
                r#""TypeError: Failed to construct 'IntersectionObserver': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => IntersectionObserver(() => {}))",
                r#""TypeError: Failed to construct 'IntersectionObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new IntersectionObserverEntry())",
                r#""TypeError: Failed to construct 'IntersectionObserverEntry': Illegal constructor""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {root: 1}))",
                r#""TypeError: Failed to construct 'IntersectionObserver': Failed to read the 'root' property from 'IntersectionObserverInit': The provided value is not of type '(Document or Element)'.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {root: document.body}).root === document.body)",
                r"true",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {root: document}).root === document)",
                r"true",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {root: null}).root)",
                r"null",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {root: document.createTextNode('x')}))",
                r#""TypeError: Failed to construct 'IntersectionObserver': Failed to read the 'root' property from 'IntersectionObserverInit': The provided value is not of type '(Document or Element)'.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: 'abc'}))",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '10'}))",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '10px 20px 30px 40px 50px'}))",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': Extra text found at the end of rootMargin.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '10em'}))",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: ''}).rootMargin)",
                r#""0px 0px 0px 0px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '10px'}).rootMargin)",
                r#""10px 10px 10px 10px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '10px 20%'}).rootMargin)",
                r#""10px 20% 10px 20%""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '1px 2px 3px'}).rootMargin)",
                r#""1px 2px 3px 2px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: ' 5px  -6px '}).rootMargin)",
                r#""5px -6px 5px -6px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '0'}).rootMargin)",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '1.5px'}).rootMargin)",
                r#""1px 1px 1px 1px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '+5px'}).rootMargin)",
                r#""5px 5px 5px 5px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: '5PX'}).rootMargin)",
                r#""5px 5px 5px 5px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {rootMargin: 5}).rootMargin)",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).rootMargin)",
                r#""0px 0px 0px 0px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).thresholds)",
                r"[0]",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: 0.5}).thresholds)",
                r"[0.5]",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: [1, 0.5, 0, 0.5]}).thresholds)",
                r"[0, 0.5, 0.5, 1]",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: []}).thresholds)",
                r"[0]",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: -0.1}))",
                r#""RangeError: Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: 1.1}))",
                r#""RangeError: Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: [0, 2]}))",
                r#""RangeError: Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: NaN}))",
                r#""TypeError: Failed to construct 'IntersectionObserver': Failed to read the 'threshold' property from 'IntersectionObserverInit': The provided double value is non-finite.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: Infinity}))",
                r#""TypeError: Failed to construct 'IntersectionObserver': Failed to read the 'threshold' property from 'IntersectionObserverInit': The provided double value is non-finite.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: 'x'}))",
                r#""TypeError: Failed to construct 'IntersectionObserver': Failed to read the 'threshold' property from 'IntersectionObserverInit': The provided double value is non-finite.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {threshold: '0.5'}).thresholds)",
                r"[0.5]",
            ),
            (
                r"E(() => { const o = new IntersectionObserver(() => {}); return o.thresholds === o.thresholds; })",
                r"false",
            ),
            (
                r"E(() => { const o = new IntersectionObserver(() => {}, {threshold: [0.5, 0]}); return Object.isFrozen(o.thresholds); })",
                r"true",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, 5))",
                r#""TypeError: Failed to construct 'IntersectionObserver': The provided value is not of type 'IntersectionObserverInit'.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, null).thresholds)",
                r"[0]",
            ),
            (
                r"E(() => { const o = new IntersectionObserver(() => {}); return [o.delay, o.trackVisibility, 'scrollMargin' in o ? o.scrollMargin : 'none']; })",
                r#"[0, false, "0px 0px 0px 0px"]"#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {delay: 100, trackVisibility: true}))",
                r"{}",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {delay: 100, trackVisibility: true}).delay)",
                r"100",
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {trackVisibility: true, delay: 50}).delay)",
                r#""DOMException NotSupportedError: Failed to construct 'IntersectionObserver': To enable the 'trackVisibility' option, you must also use a 'delay' option with a value of at least 100. Visibility is more expensive to compute than the basic intersection; enabling this option may negatively affect your page's performance. Please make sure you *really* need visibility tracking before enabling the 'trackVisibility' option.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {scrollMargin: '10px'}).scrollMargin)",
                r#""10px 10px 10px 10px""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}, {scrollMargin: 'x'}))",
                r#""DOMException SyntaxError: Failed to construct 'IntersectionObserver': scrollMargin must be specified in pixels or percent.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).observe())",
                r#""TypeError: Failed to execute 'observe' on 'IntersectionObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).observe(1))",
                r#""TypeError: Failed to execute 'observe' on 'IntersectionObserver': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).observe(document))",
                r#""TypeError: Failed to execute 'observe' on 'IntersectionObserver': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).observe(document.createTextNode('x')))",
                r#""TypeError: Failed to execute 'observe' on 'IntersectionObserver': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).unobserve(t1))",
                r#""undefined""#,
            ),
            (
                r"E(() => new IntersectionObserver(() => {}).unobserve())",
                r#""TypeError: Failed to execute 'unobserve' on 'IntersectionObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => { const o = new IntersectionObserver(() => {}); return [o.observe(t1), o.unobserve(t1), o.disconnect(), o.takeRecords()]; })",
                r"[null, null, null, []]",
            ),
            (
                r"E(() => IntersectionObserver.prototype.observe.call({}, t1))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => IntersectionObserver.prototype.takeRecords.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(IntersectionObserver.prototype, 'root').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"Object.getOwnPropertyNames(IntersectionObserver.prototype).filter((n) => { const d = Object.getOwnPropertyDescriptor(IntersectionObserver.prototype, n); return d.get; }).join()",
                r#""root,rootMargin,scrollMargin,thresholds,delay,trackVisibility""#,
            ),
            (
                r"Object.getOwnPropertyNames(IntersectionObserverEntry.prototype).filter((n) => Object.getOwnPropertyDescriptor(IntersectionObserverEntry.prototype, n).set).join() + '|' + Object.getOwnPropertyNames(IntersectionObserverEntry.prototype).length",
                r#""|9""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(IntersectionObserverEntry.prototype, 'isIntersecting'); return [typeof d.get, d.set, d.enumerable, d.configurable]; })",
                r#"["function", null, true, true]"#,
            ),
            (
                r"E(() => Object.prototype.toString.call(new IntersectionObserver(() => {})))",
                r#""[object IntersectionObserver]""#,
            ),
        ],
    },
    Case {
        name: "io-basic",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}.b{position:absolute;left:10px;width:100px;height:100px;background:#ccc}</style><div class=b id=t1 style="top:0"></div><div class=b id=t2 style="top:150px"></div><div class=b id=t3 style="top:400px"></div><div class=b id=t4 style="top:200px"></div><div class=b id=t5 style="display:none"></div><div id=spacer style="height:2000px"></div><script>
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

const rf = (r) => r ? [r.x, r.y, r.width, r.height].map((n) => +n.toFixed(2)).join(',') : 'null';
const ent = (e) => [e.target.id || e.target.localName, e.isIntersecting, +e.intersectionRatio.toFixed(3), rf(e.boundingClientRect), rf(e.intersectionRect), rf(e.rootBounds), typeof e.time].join(' ');
// make(opts): an observer that logs its batches into L.
const OBS = [];
const ORIG = {};
const ELS = {};
for (const id of ['t1', 't2', 't3', 't4', 't5']) { ORIG[id] = document.getElementById(id).style.cssText; ELS[id] = document.getElementById(id); }
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  window.scrollTo(0, 0);
  for (const id in ORIG) { if (!ELS[id].isConnected) document.body.insertBefore(ELS[id], spacer); ELS[id].style.cssText = ORIG[id]; }
  window.onerror = null;
}
function make(opts, name) {
  const o = new IntersectionObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); }, opts);
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(120);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); [t1, t2, t3, t4, t5].forEach((t) => o.observe(t)); await settle(); reset(); return L; })()",
                r#"["cb 5", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "t3 false 0 10,400,100,100 0,0,0,0 0,0,400,200 number", "t4 true 0 10,200,100,100 10,200,100,0 0,0,400,200 number", "t5 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({threshold: [0, 0.25, 0.5, 0.75, 1]}); [t1, t2, t3, t4, t5, spacer].forEach((t) => o.observe(t)); await settle(); reset(); return L; })()",
                r#"["cb 6", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "t3 false 0 10,400,100,100 0,0,0,0 0,0,400,200 number", "t4 true 0 10,200,100,100 10,200,100,0 0,0,400,200 number", "t5 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number", "spacer true 0.1 0,0,400,2000 0,0,400,200 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t2); await settle(); L.push('--'); window.scrollTo(0, 100); await settle(); L.push('--'); window.scrollTo(0, 100); await settle(); L.push('--'); window.scrollTo(0, 300); await settle(); L.push('--'); window.scrollTo(0, 0); await settle(); reset(); return L; })()",
                r#"["cb 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "--", "--", "--", "cb 1", "t2 false 0 10,-150,100,100 0,0,0,0 0,0,400,200 number", "--", "cb 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({threshold: [0, 0.25, 0.5, 0.75, 1]}); o.observe(t2); await settle(); for (const y of [25, 50, 75, 100, 125, 150]) { L.push('-- scroll ' + y); window.scrollTo(0, y); await settle(); } for (const y of [100, 50, 0]) { L.push('-- scroll ' + y); window.scrollTo(0, y); await settle(); } reset(); return L; })()",
                r#"["cb 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "-- scroll 25", "cb 1", "t2 true 0.75 10,125,100,100 10,125,100,75 0,0,400,200 number", "-- scroll 50", "cb 1", "t2 true 1 10,100,100,100 10,100,100,100 0,0,400,200 number", "-- scroll 75", "-- scroll 100", "-- scroll 125", "-- scroll 150", "-- scroll 100", "-- scroll 50", "-- scroll 0", "cb 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({threshold: [0.5]}); o.observe(t2); await settle(); L.push('-- 60'); window.scrollTo(0, 60); await settle(); L.push('-- 40'); window.scrollTo(0, 40); await settle(); L.push('-- 49'); window.scrollTo(0, 49); await settle(); reset(); return L; })()",
                r#"["cb 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "-- 60", "-- 40", "-- 49"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({rootMargin: '50px'}); [t3, t4, t1].forEach((t) => o.observe(t)); await settle(); L.push('-- scroll'); window.scrollTo(0, 200); await settle(); reset(); return L; })()",
                r#"["cb 3", "t3 false 0 10,400,100,100 0,0,0,0 -50,-50,500,300 number", "t4 true 0.5 10,200,100,100 10,200,100,50 -50,-50,500,300 number", "t1 true 1 10,0,100,100 10,0,100,100 -50,-50,500,300 number", "-- scroll", "cb 2", "t3 true 0.5 10,200,100,100 10,200,100,50 -50,-50,500,300 number", "t1 false 0 10,-200,100,100 0,0,0,0 -50,-50,500,300 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({rootMargin: '-20px 10px'}); [t1, t2, t3].forEach((t) => o.observe(t)); await settle(); reset(); return L; })()",
                r#"["cb 3", "t1 true 0.8 10,0,100,100 10,20,100,80 -10,20,420,160 number", "t2 true 0.3 10,150,100,100 10,150,100,30 -10,20,420,160 number", "t3 false 0 10,400,100,100 0,0,0,0 -10,20,420,160 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({rootMargin: '10% 20%'}); [t1, t2, t3, t4].forEach((t) => o.observe(t)); await settle(); reset(); return L; })()",
                r#"["cb 4", "t1 true 1 10,0,100,100 10,0,100,100 -80,-20,560,240 number", "t2 true 0.7 10,150,100,100 10,150,100,70 -80,-20,560,240 number", "t3 false 0 10,400,100,100 0,0,0,0 -80,-20,560,240 number", "t4 true 0.2 10,200,100,100 10,200,100,20 -80,-20,560,240 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({rootMargin: '100px 0px 0px 0px'}); [t3, t4].forEach((t) => o.observe(t)); await settle(); const p = make({rootMargin: '0px 0px -100px 0px'}, 'p'); [t1, t2].forEach((t) => p.observe(t)); await settle(); reset(); return L; })()",
                r#"["cb 2", "t3 false 0 10,400,100,100 0,0,0,0 0,-100,400,300 number", "t4 true 0 10,200,100,100 10,200,100,0 0,-100,400,300 number", "p 2", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,100 number", "t2 false 0 10,150,100,100 0,0,0,0 0,0,400,100 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t4); o.observe(t4); o.observe(t4); await settle(); reset(); return L; })()",
                r#"["cb 1", "t4 true 0 10,200,100,100 10,200,100,0 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); await settle(); o.unobserve(t1); L.push('-- unobserved'); window.scrollTo(0, 500); await settle(); window.scrollTo(0, 0); await settle(); L.push('-- observe again'); o.observe(t1); await settle(); reset(); return L; })()",
                r#"["cb 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "-- unobserved", "-- observe again", "cb 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); o.observe(t2); await settle(); o.disconnect(); window.scrollTo(0, 500); await settle(); L.push('taken ' + o.takeRecords().length); o.observe(t3); await settle(); reset(); return L; })()",
                r#"["cb 2", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "taken 0", "cb 1", "t3 true 0 10,-100,100,100 10,0,100,0 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); const r = o.takeRecords(); L.push('taken ' + r.length + ' ' + (r[0] ? ent(r[0]) : '')); L.push('again ' + o.takeRecords().length); await settle(); L.push('after'); reset(); return L; })()",
                r#"["taken 0 ", "again 0", "cb 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "after"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); o.unobserve(t1); await settle(); L.push('end'); reset(); return L; })()",
                r#"["end"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); o.observe(t2); o.unobserve(t1); await settle(); L.push('end'); reset(); return L; })()",
                r#"["cb 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number", "end"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); const d = document.createElement('div'); d.id = 'detached'; o.observe(d); await settle(); document.body.appendChild(d); await settle(); d.style.cssText = 'height:50px;width:50px'; await settle(); d.remove(); await settle(); reset(); return L; })()",
                r#"["cb 1", "detached false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t5); await settle(); t5.style.display = 'block'; await settle(); t5.style.display = 'none'; await settle(); reset(); return L; })()",
                r#"["cb 1", "t5 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number", "cb 1", "t5 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "cb 1", "t5 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); await settle(); t1.style.top = '300px'; await settle(); t1.style.top = '0'; await settle(); t1.style.visibility = 'hidden'; await settle(); t1.style.opacity = '0'; await settle(); t1.style.height = '0'; await settle(); reset(); return L; })()",
                r#"["cb 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "cb 1", "t1 false 0 10,300,100,100 0,0,0,0 0,0,400,200 number", "cb 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}); o.observe(t1); await settle(); t1.style.width = '0px'; t1.style.height = '0px'; await settle(); L.push('-- zero at edge'); t1.style.top = '200px'; await settle(); L.push('-- zero outside'); t1.style.top = '250px'; await settle(); reset(); return L; })()",
                r#"["cb 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "-- zero at edge", "-- zero outside", "cb 1", "t1 false 0 10,250,0,0 0,0,0,0 0,0,400,200 number"]"#,
            ),
        ],
    },
    Case {
        name: "io-clip",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}.b{position:absolute;left:10px;width:100px;height:100px;background:#ccc}</style><div class=b id=t1 style="top:0"></div><div class=b id=t2 style="top:150px"></div><div class=b id=t3 style="top:400px"></div><div class=b id=t4 style="top:200px"></div><div class=b id=t5 style="display:none"></div><div id=spacer style="height:2000px"></div><script>
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

const rf = (r) => r ? [r.x, r.y, r.width, r.height].map((n) => +n.toFixed(2)).join(',') : 'null';
const ent = (e) => [e.target.id || e.target.localName, e.isIntersecting, +e.intersectionRatio.toFixed(3), rf(e.boundingClientRect), rf(e.intersectionRect), rf(e.rootBounds), typeof e.time].join(' ');
// make(opts): an observer that logs its batches into L.
const OBS = [];
const ORIG = {};
const ELS = {};
for (const id of ['t1', 't2', 't3', 't4', 't5']) { ORIG[id] = document.getElementById(id).style.cssText; ELS[id] = document.getElementById(id); }
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  window.scrollTo(0, 0);
  for (const id in ORIG) { if (!ELS[id].isConnected) document.body.insertBefore(ELS[id], spacer); ELS[id].style.cssText = ORIG[id]; }
  window.onerror = null;
}
function make(opts, name) {
  const o = new IntersectionObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); }, opts);
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(120);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:300px;width:60px;height:50px;overflow:hidden'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'width:200px;height:200px;background:red'; w.appendChild(c); document.body.appendChild(w); window.scrollTo(0, 250); const o = make({threshold: [0, 0.1, 0.5, 1]}); o.observe(c); await settle(); L.push('-- scroll inside'); w.scrollTop = 100; await settle(); L.push('-- viewport'); window.scrollTo(0, 100); await settle(); w.remove(); window.scrollTo(0, 0); reset(); return L; })()",
                r#"["cb 1", "c true 0.075 0,50,200,200 0,50,60,50 0,0,400,200 number", "-- scroll inside", "-- viewport"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:50px;width:60px;height:50px;overflow:hidden'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'position:absolute;left:0;top:0;width:200px;height:200px;background:red'; w.appendChild(c); document.body.appendChild(w); const o = make({}); o.observe(c); await settle(); w.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 0.075 0,50,200,200 0,50,60,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:50px;width:60px;height:50px;overflow:hidden;position:relative'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'position:absolute;left:0;top:0;width:200px;height:200px;background:red'; w.appendChild(c); document.body.appendChild(w); const o = make({}); o.observe(c); await settle(); w.remove(); reset(); return L; })()",
                r#"["cb 1", "c false 0 0,2050,200,200 0,0,0,0 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:50px;width:60px;height:50px;overflow:hidden'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'position:fixed;left:0;top:10px;width:30px;height:30px;background:red'; w.appendChild(c); document.body.appendChild(w); const o = make({}); o.observe(c); await settle(); w.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 1 0,10,30,30 0,10,30,30 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:50px;width:60px;height:50px;overflow:clip'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'width:200px;height:200px;background:red'; w.appendChild(c); document.body.appendChild(w); const o = make({}); o.observe(c); await settle(); w.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 0.075 0,50,200,200 0,50,60,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:50px;width:60px;height:50px;overflow:auto'; w.style.padding = '10px'; w.style.border = '5px solid'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'width:200px;height:200px;background:red'; w.appendChild(c); document.body.appendChild(w); const o = make({}); o.observe(c); await settle(); w.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 0.105 15,65,200,200 15,65,70,60 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const w = document.createElement('div'); w.id = 'w'; w.style.cssText = 'position:absolute;left:0;top:50px;width:60px;height:50px;overflow:hidden;transform:translate(20px, 10px)'; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'width:200px;height:200px;background:red'; w.appendChild(c); document.body.appendChild(w); const o = make({}); o.observe(c); await settle(); w.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 0.075 20,60,200,200 20,60,60,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'position:absolute;left:10px;top:20px;width:100px;height:100px;transform:scale(0.5);transform-origin:0 0'; document.body.appendChild(c); const o = make({}); o.observe(c); await settle(); c.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 1 10,20,50,50 10,20,50,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'position:absolute;left:10px;top:20px;width:100px;height:100px;clip-path:inset(0 50px 0 0)'; document.body.appendChild(c); const o = make({}); o.observe(c); await settle(); c.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 0.5 10,20,100,100 10,20,50,100 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const c = document.createElement('div'); c.id = 'c'; c.style.cssText = 'position:absolute;left:380px;top:20px;width:100px;height:100px;'; document.body.appendChild(c); const o = make({}); o.observe(c); await settle(); c.remove(); reset(); return L; })()",
                r#"["cb 1", "c true 0.2 380,20,100,100 380,20,20,100 0,0,400,200 number"]"#,
            ),
        ],
    },
    Case {
        name: "io-root",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}.b{position:absolute;left:10px;width:100px;height:100px;background:#ccc}</style><div class=b id=t1 style="top:0"></div><div class=b id=t2 style="top:150px"></div><div class=b id=t3 style="top:400px"></div><div class=b id=t4 style="top:200px"></div><div class=b id=t5 style="display:none"></div><div id=spacer style="height:2000px"></div><script>
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

const rf = (r) => r ? [r.x, r.y, r.width, r.height].map((n) => +n.toFixed(2)).join(',') : 'null';
const ent = (e) => [e.target.id || e.target.localName, e.isIntersecting, +e.intersectionRatio.toFixed(3), rf(e.boundingClientRect), rf(e.intersectionRect), rf(e.rootBounds), typeof e.time].join(' ');
// make(opts): an observer that logs its batches into L.
const OBS = [];
const ORIG = {};
const ELS = {};
for (const id of ['t1', 't2', 't3', 't4', 't5']) { ORIG[id] = document.getElementById(id).style.cssText; ELS[id] = document.getElementById(id); }
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  window.scrollTo(0, 0);
  for (const id in ORIG) { if (!ELS[id].isConnected) document.body.insertBefore(ELS[id], spacer); ELS[id].style.cssText = ORIG[id]; }
  window.onerror = null;
}
function make(opts, name) {
  const o = new IntersectionObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); }, opts);
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(120);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; const r = document.createElement('div'); r.id = 'r'; r.style.cssText = 'position:absolute;left:20px;top:300px;width:100px;height:80px;overflow:auto;border:5px solid;padding:7px'; const k = document.createElement('div'); k.id = 'k'; k.style.cssText = 'height:300px;width:50px'; const i = document.createElement('div'); i.id = 'i'; i.style.cssText = 'height:30px;width:40px;margin-top:20px'; k.appendChild(i); const j = document.createElement('div'); j.id = 'j'; j.style.cssText = 'height:30px;width:40px;margin-top:120px'; k.appendChild(j); r.appendChild(k); document.body.appendChild(r); window.scrollTo(0, 250); const o = make({root: r, threshold: [0, 0.5, 1]}); o.observe(i); o.observe(j); await settle(); L.push('-- scroll root'); r.scrollTop = 100; await settle(); L.push('-- scroll root'); r.scrollTop = 150; await settle(); L.push('-- viewport'); window.scrollTo(0, 0); await settle(); L.push('-- root removed'); r.remove(); await settle(); window.scrollTo(0, 0); reset(); return L; })()",
                r#"["cb 2", "i true 1 32,82,40,30 32,82,40,30 25,55,114,94 number", "j false 0 32,232,40,30 0,0,0,0 25,55,114,94 number", "-- scroll root", "cb 2", "i false 0 32,-18,40,30 0,0,0,0 25,55,114,94 number", "j true 0.567 32,132,40,30 32,132,40,17 25,55,114,94 number", "-- scroll root", "cb 1", "j true 1 32,82,40,30 32,82,40,30 25,55,114,94 number", "-- viewport", "-- root removed", "cb 1", "j false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const r = document.createElement('div'); r.id = 'r'; r.style.cssText = 'position:absolute;left:20px;top:20px;width:100px;height:80px;overflow:auto'; const k = document.createElement('div'); k.id = 'k'; k.style.cssText = 'height:30px;width:40px'; r.appendChild(k); document.body.appendChild(r); const other = document.createElement('div'); other.id = 'other'; other.style.cssText = 'position:absolute;left:20px;top:150px;width:10px;height:10px'; document.body.appendChild(other); const o = make({root: r}); o.observe(k); o.observe(other); o.observe(t1); await settle(); r.remove(); other.remove(); reset(); return L; })()",
                r#"["cb 3", "k true 1 20,20,40,30 20,20,40,30 20,20,100,80 number", "other false 0 0,0,0,0 0,0,0,0 0,0,0,0 number", "t1 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({root: document.documentElement}); o.observe(t1); o.observe(t2); o.observe(t3); await settle(); reset(); return L; })()",
                r#"["cb 3", "t1 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number", "t2 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number", "t3 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({root: document.body}); o.observe(t1); o.observe(t2); await settle(); reset(); return L; })()",
                r#"["cb 2", "t1 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number", "t2 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({root: document, rootMargin: '10px'}); o.observe(t1); o.observe(t3); await settle(); reset(); return L; })()",
                r#"["cb 2", "t1 true 1 10,0,100,100 10,0,100,100 -10,-10,420,220 number", "t3 false 0 10,400,100,100 0,0,0,0 -10,-10,420,220 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const r = document.createElement('div'); r.id = 'r'; r.style.cssText = 'position:absolute;left:20px;top:20px;width:100px;height:80px;overflow:hidden'; const k = document.createElement('div'); k.id = 'k'; k.style.cssText = 'height:30px;width:40px;margin-left:90px'; r.appendChild(k); document.body.appendChild(r); const o = make({root: r, rootMargin: '10px'}); o.observe(k); await settle(); r.remove(); reset(); return L; })()",
                r#"["cb 1", "k true 0.5 110,20,40,30 110,20,20,30 10,10,120,100 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const r = document.createElement('div'); r.id = 'r'; r.style.cssText = 'position:absolute;left:20px;top:20px;width:100px;height:80px;'; document.body.appendChild(r); const o = make({root: r}); o.observe(t1); await settle(); r.remove(); reset(); return L; })()",
                r#"["cb 1", "t1 false 0 0,0,0,0 0,0,0,0 0,0,0,0 number"]"#,
            ),
        ],
    },
    Case {
        name: "io-order",
        html: r#"<!DOCTYPE html><html><head></head><body><style>html,body{margin:0}.b{position:absolute;left:10px;width:100px;height:100px;background:#ccc}</style><div class=b id=t1 style="top:0"></div><div class=b id=t2 style="top:150px"></div><div class=b id=t3 style="top:400px"></div><div class=b id=t4 style="top:200px"></div><div class=b id=t5 style="display:none"></div><div id=spacer style="height:2000px"></div><script>
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

const rf = (r) => r ? [r.x, r.y, r.width, r.height].map((n) => +n.toFixed(2)).join(',') : 'null';
const ent = (e) => [e.target.id || e.target.localName, e.isIntersecting, +e.intersectionRatio.toFixed(3), rf(e.boundingClientRect), rf(e.intersectionRect), rf(e.rootBounds), typeof e.time].join(' ');
// make(opts): an observer that logs its batches into L.
const OBS = [];
const ORIG = {};
const ELS = {};
for (const id of ['t1', 't2', 't3', 't4', 't5']) { ORIG[id] = document.getElementById(id).style.cssText; ELS[id] = document.getElementById(id); }
function reset() {
  OBS.splice(0).forEach((o) => o.disconnect());
  window.scrollTo(0, 0);
  for (const id in ORIG) { if (!ELS[id].isConnected) document.body.insertBefore(ELS[id], spacer); ELS[id].style.cssText = ORIG[id]; }
  window.onerror = null;
}
function make(opts, name) {
  const o = new IntersectionObserver(function (es, ob) { L.push((name || 'cb') + ' ' + es.length + (ob === o ? '' : ' wrong-observer') + (this === o ? '' : ' wrong-this')); es.forEach((e) => L.push(ent(e))); }, opts);
  OBS.push(o);
  return o;
}
function frames(n) { return new Promise((res) => { let i = 0; const step = () => { if (++i >= n) res(); else requestAnimationFrame(step); }; requestAnimationFrame(step); }); }
const settle = () => wait(120);
</script></body></html>"#,
        queries: &[
            (
                r"(async () => { reset(); await settle(); L.length = 0; make({}, 'io').observe(t1); requestAnimationFrame(() => L.push('raf1')); setTimeout(() => L.push('timeout0'), 0); Promise.resolve().then(() => L.push('micro')); await settle(); reset(); return L; })()",
                r#"["micro", "timeout0", "raf1", "io 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; make({}, 'io').observe(t1); await frames(1); L.push('frame1'); await frames(1); L.push('frame2'); await settle(); reset(); return L; })()",
                r#"["frame1", "io 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "frame2"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; make({}, 'io').observe(t1); const ro = new ResizeObserver(() => L.push('ro')); OBS.push(ro); ro.observe(t1); (() => { const m = new MutationObserver(() => L.push('mo')); OBS.push(m); m.observe(t1, {attributes: true}); })(); t1.title = 'x'; requestAnimationFrame(() => L.push('raf1')); requestAnimationFrame(() => requestAnimationFrame(() => L.push('raf2'))); await settle(); reset(); return L; })()",
                r#"["mo", "raf1", "ro", "io 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "raf2"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'io'); o.observe(t1); await settle(); L.push('-- change'); t1.style.top = '300px'; setTimeout(() => L.push('timeout0'), 0); requestAnimationFrame(() => L.push('raf')); await settle(); reset(); return L; })()",
                r#"["io 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "-- change", "timeout0", "raf", "io 1", "t1 false 0 10,300,100,100 0,0,0,0 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'io'); o.observe(t1); await settle(); L.push('-- change'); o.observe(t2); L.push('sync'); await settle(); reset(); return L; })()",
                r#"["io 1", "t1 true 1 10,0,100,100 10,0,100,100 0,0,400,200 number", "-- change", "sync", "io 1", "t2 true 0.5 10,150,100,100 10,150,100,50 0,0,400,200 number"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = new IntersectionObserver(() => { L.push('cb1'); throw new Error('boom'); }); OBS.push(o); window.onerror = (m) => { L.push('onerror ' + m); return true; }; o.observe(t1); await settle(); t1.style.top = '300px'; await settle(); window.onerror = null; reset(); return L; })()",
                r#"["cb1", "onerror Uncaught Error: boom", "cb1", "onerror Uncaught Error: boom"]"#,
            ),
            (
                r"(async () => { reset(); await settle(); L.length = 0; const o = make({}, 'io'); o.observe(t1); await settle(); L.push('-- removing'); t1.remove(); await settle(); document.body.appendChild(t1); await settle(); reset(); return L; })()",
                r"error: ReferenceError: t1 is not defined",
            ),
        ],
    },
    Case {
        name: "io-extra",
        html: r"<!DOCTYPE html><html><head><style>html,body{margin:0}</style></head><body></body><script>
window.L=[];
const rf=(r)=>r?[r.x,r.y,r.width,r.height].map(n=>+n.toFixed(2)).join(','):'null';
const ent=(e)=>[e.target.id,e.isIntersecting,+e.intersectionRatio.toFixed(3),rf(e.boundingClientRect),rf(e.intersectionRect),rf(e.rootBounds)].join(' ');
function wait(ms){return new Promise(r=>setTimeout(r,ms));}
async function run(html, setup, opts, targets, after){
  document.body.innerHTML=html; window.scrollTo(0,0); L.length=0;
  await wait(50);
  const o=new IntersectionObserver((es)=>{L.push('cb '+es.length); es.forEach(e=>L.push(ent(e)));}, opts||{});
  if(setup) setup();
  targets.forEach(t=>o.observe(document.getElementById(t)));
  await wait(120);
  if(after){ await after(o); await wait(120);}
  o.disconnect(); return L.slice();
}
</script></html>",
        queries: &[
            (
                r#"(async()=>run('<div id=t style="position:absolute;left:10px;top:10px;width:100px;height:100px;transform:translate(150px,20px) scale(.5)"></div>',null,{threshold:[0,.5,1]},['t']))()"#,
                r#"["cb 1", "t true 1 185,55,50,50 185,55,50,50 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="position:absolute;left:0;top:0;transform:translateX(250px);width:100px;height:100px"><div id=t style="width:80px;height:50px"></div></div>',null,{},['t']))()"#,
                r#"["cb 1", "t true 1 250,0,80,50 250,0,80,50 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="height:2000px"><div id=t style="position:sticky;top:10px;height:30px;margin-top:300px"></div></div>',null,{},['t'],async()=>{scrollTo(0,500);await wait(100);}))()"#,
                r#"["cb 1", "t false 0 0,300,400,30 0,0,0,0 0,0,400,200", "cb 1", "t true 1 0,10,400,30 0,10,400,30 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div id=s style="width:100px;height:60px;overflow:auto;margin:20px"><div style="height:40px"></div><div id=t style="height:40px"></div><div style="height:300px"></div></div>',null,{threshold:[0,.5,1]},['t'],async()=>{document.getElementById('s').scrollTop=30;await wait(100);document.getElementById('s').scrollTop=50;}))()"#,
                r#"["cb 1", "t true 0.5 20,60,100,40 20,60,100,20 0,0,400,200", "cb 1", "t true 1 20,30,100,40 20,30,100,40 0,0,400,200", "cb 1", "t true 0.75 20,10,100,40 20,20,100,30 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div id=r style="width:100px;height:80px;overflow:auto;margin:30px;border:5px solid;padding:7px"><div style="height:100px"></div><div id=t style="height:40px;width:200px"></div><div style="height:300px"></div></div>',null,{root:null},['t']))()"#,
                r#"["cb 1", "t false 0 42,142,200,40 0,0,0,0 0,0,400,200"]"#,
            ),
            (
                r#"(()=>{document.body.innerHTML='<div id=r style="width:100px;height:80px;overflow:auto;margin:30px;border:5px solid;padding:7px"><div style="height:100px"></div><div id=t style="height:40px;width:200px"></div><div style="height:300px"></div></div>';return wait(50).then(()=>{L.length=0;const o=new IntersectionObserver(es=>{L.push('cb '+es.length);es.forEach(e=>L.push(ent(e)))},{root:document.getElementById('r'),rootMargin:'10px 20% -5px 3px',threshold:[0,.5,1]});o.observe(document.getElementById('t'));return wait(120).then(()=>{document.getElementById('r').scrollTop=60;return wait(120)}).then(()=>{o.disconnect();return L.slice()})})})()"#,
                r#"["cb 1", "t false 0 42,142,200,40 0,0,0,0 32,25,139,99", "cb 1", "t true 0.645 42,82,200,40 42,82,129,40 32,25,139,99"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="position:fixed;left:10px;top:150px;width:50px;height:100px"></div><div style="height:3000px"></div>',null,{threshold:[0,.5,1]},['t']))()"#,
                r#"["cb 1", "t true 0.5 10,150,50,100 10,150,50,50 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:0;height:0;margin-top:50px"></div><div id=u style="width:100px;height:0;margin-top:500px"></div>',null,{},['t','u']))()"#,
                r#"["cb 2", "t false 0 0,500,0,0 0,0,0,0 0,0,400,200", "u false 0 0,500,100,0 0,0,0,0 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="height:2000px"></div><div id=t style="height:50px;margin-top:300px"></div>',null,{rootMargin:'50%'},['t']))()"#,
                r#"["cb 1", "t false 0 0,2300,400,50 0,0,0,0 -200,-100,800,400"]"#,
            ),
            (
                r#"(async()=>run('<div id=s style="width:100px;height:50px;margin:10px;overflow:hidden"><div id=t style="width:300px;height:300px;position:relative"></div></div>',null,{threshold:[0,.1,.5]},['t']))()"#,
                r#"["cb 1", "t true 0.056 10,10,300,300 10,10,100,50 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="overflow:hidden;width:50px;height:50px;position:relative"><div id=t style="position:absolute;left:0;top:0;width:100px;height:100px"></div></div>',null,{},['t']))()"#,
                r#"["cb 1", "t true 0.25 0,0,100,100 0,0,50,50 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="overflow:hidden;width:50px;height:50px"><div id=t style="position:absolute;left:0;top:0;width:100px;height:100px"></div></div>',null,{},['t']))()"#,
                r#"["cb 1", "t true 1 0,0,100,100 0,0,100,100 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="height:2000px"></div><div id=t style="height:100px;margin-top:0"></div>',null,{threshold:[0,1/3,.5,2/3,1]},['t'],async()=>{for(const y of [1950,1967,1983,2000,2050]){scrollTo(0,y);await wait(100);}}))()"#,
                r#"["cb 1", "t false 0 0,2000,400,100 0,0,0,0 0,0,400,200", "cb 1", "t true 1 0,100,400,100 0,100,400,100 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div id=t style="width:20px;height:20px;visibility:hidden"></div><div id=u style="width:20px;height:20px;opacity:0"></div>',null,{},['t','u']))()"#,
                r#"["cb 2", "t true 1 0,0,20,20 0,0,20,20 0,0,400,200", "u true 1 0,20,20,20 0,20,20,20 0,0,400,200"]"#,
            ),
            (
                r#"(async()=>run('<div style="width:3000px;height:50px"><div id=t style="margin-left:1000px;width:50px;height:50px"></div></div>',null,{},['t'],async()=>{scrollTo(900,0);await wait(100);}))()"#,
                r#"["cb 1", "t false 0 1000,0,50,50 0,0,0,0 0,0,400,200", "cb 1", "t true 1 100,0,50,50 100,0,50,50 0,0,400,200"]"#,
            ),
        ],
    },
];
