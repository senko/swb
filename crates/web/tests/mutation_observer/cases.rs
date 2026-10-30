// The cases of tools/probes/web-mutation-observer.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "mo-structure",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"[typeof MutationObserver, MutationObserver.length, MutationObserver.name, Object.prototype.toString.call(new MutationObserver(() => {})), typeof MutationRecord, MutationRecord.length, Object.prototype.toString.call(MutationRecord.prototype)]",
                r#"["function", 1, "MutationObserver", "[object MutationObserver]", "function", 0, "[object MutationRecord]"]"#,
            ),
            (
                r"Object.getOwnPropertyNames(MutationObserver.prototype).sort().join()",
                r#""constructor,disconnect,observe,takeRecords""#,
            ),
            (
                r"Object.getOwnPropertyNames(MutationRecord.prototype).sort().join()",
                r#""addedNodes,attributeName,attributeNamespace,constructor,nextSibling,oldValue,previousSibling,removedNodes,target,type""#,
            ),
            (
                r"['observe','disconnect','takeRecords'].map(n => [MutationObserver.prototype[n].length, MutationObserver.prototype[n].name]).join('|')",
                r#""1,observe|0,disconnect|0,takeRecords""#,
            ),
            (
                r"E(() => new MutationObserver())",
                r#""TypeError: Failed to construct 'MutationObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new MutationObserver(1))",
                r#""TypeError: Failed to construct 'MutationObserver': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => new MutationObserver({}))",
                r#""TypeError: Failed to construct 'MutationObserver': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"E(() => MutationObserver(() => {}))",
                r#""TypeError: Failed to construct 'MutationObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new MutationRecord())",
                r#""TypeError: Failed to construct 'MutationRecord': Illegal constructor""#,
            ),
            (
                r"E(() => MutationRecord())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe())",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(1, {childList: true}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe({}, {childList: true}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, 1))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The provided value is not of type 'MutationObserverInit'.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, null))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, undefined))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributes: false}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {childList: false, attributeOldValue: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributes: false, attributeOldValue: true}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeOldValue' to true when 'attributes' is true or not present.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {characterData: false, characterDataOldValue: true}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object may only set 'characterDataOldValue' to true when 'characterData' is true or not present.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {childList: true, characterDataOldValue: true, characterData: false}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object may only set 'characterDataOldValue' to true when 'characterData' is true or not present.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributes: false, attributeFilter: ['id']}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeFilter' when 'attributes' is true or not present.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributeFilter: ['id']}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributeFilter: 5}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': Failed to read the 'attributeFilter' property from 'MutationObserverInit': The provided value cannot be converted to a sequence.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributeFilter: [1, {toString() { return 'x'; }}]}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {childList: true, subtree: 'yes'}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {subtree: true}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributeOldValue: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {characterDataOldValue: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {attributes: undefined, childList: undefined, characterData: undefined}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(a, {childList: 1}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(document, {childList: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(document.createDocumentFragment(), {childList: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(document.createTextNode('t'), {characterData: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(document.createAttribute('x'), {attributes: true}))",
                r#""undefined""#,
            ),
            (
                r"E(() => new MutationObserver(() => {}).observe(window, {childList: true}))",
                r#""TypeError: Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.""#,
            ),
            (
                r"E(() => { const o = new MutationObserver(() => {}); return [o.observe(a, CL), o.disconnect(), o.takeRecords()]; })",
                r"[null, null, []]",
            ),
            (
                r"(() => { const o = new MutationObserver(() => {}); return [o.constructor === MutationObserver, Object.getPrototypeOf(o) === MutationObserver.prototype, Object.keys(o).length, o.takeRecords().length, Array.isArray(o.takeRecords())]; })()",
                r"[true, true, 0, 0, true]",
            ),
            (
                r"E(() => MutationObserver.prototype.observe.call({}, a, CL))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => MutationObserver.prototype.takeRecords.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => MutationObserver.prototype.disconnect.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"(() => { const o = new MutationObserver(() => {}); o.observe(a, CL); a.appendChild(document.createElement('i')); const r = o.takeRecords(); o.disconnect(); a.lastChild.remove(); const x = r[0]; return [x instanceof MutationRecord, Object.prototype.toString.call(x), Object.keys(x).length, x.addedNodes instanceof NodeList, x.addedNodes === x.addedNodes, x.removedNodes.length, Object.isFrozen(r), x.type, x.attributeName, x.attributeNamespace, x.oldValue, x.previousSibling === null]; })()",
                r#"[true, "[object MutationRecord]", 0, true, true, 0, false, "childList", null, null, null, false]"#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(MutationRecord.prototype, 'type'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, d.get.name]; })()",
                r#"["function", "undefined", true, true, "get type"]"#,
            ),
        ],
    },
    Case {
        name: "mo-options",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributeOldValue: true}, () => { P().id = 'q'; P().setAttribute('id', 'r'); })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributeFilter: ['id']}, () => { P().id = 'q'; P().title = 'x'; P().setAttribute('ID', 'z'); P().className = 'c' })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributeFilter: ['id', 'Title'], attributeOldValue: true}, () => { P().title = 'x'; P().setAttribute('title', 'y'); P().setAttribute('Title', 'z'); })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributeFilter: []}, () => { P().id = 'q'; })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributes: true, attributeFilter: ['id']}, () => { P().id = 'q'; P().title = 'x' })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {characterDataOldValue: true}, () => { P().firstChild.data = 'n'; P().appendChild(document.createTextNode('t')) })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {characterDataOldValue: true, subtree: true}, () => { P().firstChild.data = 'n'; P().appendChild(document.createTextNode('t')); P().lastChild.data = 'tt'; })",
                r#"["characterData ; #text(n) ;  ;  ; null ; null ;  ;  ; x", "characterData ; #text(tt) ;  ;  ; null ; null ;  ;  ; t"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {characterData: true}, () => { P().firstChild.data = 'n'; })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {characterData: true, subtree: true}, () => { P().firstChild.data = 'n'; P().firstChild.appendData('m'); P().firstChild.insertData(0, '>'); P().firstChild.deleteData(0, 1); P().firstChild.replaceData(0, 1, 'zz'); P().firstChild.nodeValue = 'v'; P().firstChild.textContent = 'w'; P().firstChild.data = 'w' })",
                r#"["characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; ", "characterData ; #text(w) ;  ;  ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {childList: true}, () => { P().appendChild(document.createElement('i')); a.appendChild(document.createElement('i')); P().id='q' })",
                r#"["childList ; div#a ; i ;  ; span#s1 ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {childList: true, subtree: true}, () => { P().appendChild(document.createElement('i')); a.appendChild(document.createElement('i')); P().id='q' })",
                r#"["childList ; p#q ; i ;  ; #text(x) ; null ;  ;  ; ", "childList ; div#a ; i ;  ; span#s1 ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributes: true, subtree: true}, () => { P().appendChild(document.createElement('i')); a.title='q'; P().id='q'; s1.title='t' })",
                r#"["attributes ; div#a ;  ;  ; null ; null ; title ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; id ;  ; ", "attributes ; span#s1 ;  ;  ; null ; null ; title ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', {attributes: true}, () => { P().id='q'; a.title='z'; a.title='z'; a.setAttribute('title', 'z') })",
                r#"["attributes ; div#a ;  ;  ; null ; null ; title ;  ; ", "attributes ; div#a ;  ;  ; null ; null ; title ;  ; ", "attributes ; div#a ;  ;  ; null ; null ; title ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {childList: true}); o.observe(a, {attributes: true}); P().id = 'q'; a.appendChild(document.createElement('i')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r"[]",
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {childList: true}); o.observe(P(), {attributes: true}); P().id = 'q'; a.appendChild(document.createElement('i')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["attributes ; p#q ;  ;  ; null ; null ; id ;  ; ", "childList ; div#a ; i ;  ; span#s1 ; null ;  ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {childList: true, subtree: true}); o.observe(P(), {attributes: true}); P().id = 'q'; P().appendChild(document.createElement('i')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["attributes ; p#q ;  ;  ; null ; null ; id ;  ; ", "childList ; p#q ; i ;  ; #text(x) ; null ;  ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, subtree: true}); o.observe(P(), {attributes: true, attributeOldValue: true}); P().id = 'q'; P().id = 'r'; const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["attributes ; p#r ;  ;  ; null ; null ; id ;  ; p1", "attributes ; p#r ;  ;  ; null ; null ; id ;  ; q"]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, attributeOldValue: true, subtree: true}); o.observe(P(), {attributes: true}); P().id = 'q'; P().id = 'r'; const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["attributes ; p#r ;  ;  ; null ; null ; id ;  ; p1", "attributes ; p#r ;  ;  ; null ; null ; id ;  ; q"]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, attributeFilter: ['id'], subtree: true}); o.observe(P(), {attributes: true, attributeFilter: ['title'], attributeOldValue: true}); P().id = 'q'; P().title = 'r'; const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["attributes ; p#q ;  ;  ; null ; null ; id ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; title ;  ; "]"#,
            ),
        ],
    },
    Case {
        name: "mo-operations",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().setAttribute('x', '1'); P().setAttribute('x', '2'); P().removeAttribute('x'); P().removeAttribute('x'); P().toggleAttribute('y'); P().toggleAttribute('y'); P().toggleAttribute('y', true); P().toggleAttribute('y', true); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; x ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; x ;  ; 1", "attributes ; p#p1 ;  ;  ; null ; null ; x ;  ; 2", "attributes ; p#p1 ;  ;  ; null ; null ; y ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; y ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; y ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', 'u'); P().setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', 'v'); P().removeAttributeNS('http://www.w3.org/1999/xlink', 'href'); P().setAttributeNS('urn:x', 'a:b', '1'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; href ; http://www.w3.org/1999/xlink ; ", "attributes ; p#p1 ;  ;  ; null ; null ; href ; http://www.w3.org/1999/xlink ; u", "attributes ; p#p1 ;  ;  ; null ; null ; href ; http://www.w3.org/1999/xlink ; v", "attributes ; p#p1 ;  ;  ; null ; null ; b ; urn:x ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().classList.add('a'); P().classList.add('a'); P().classList.add('b'); P().classList.remove('a'); P().classList.toggle('c'); P().classList.replace('b', 'd'); P().classList.remove('zz'); P().className = 'k'; P().classList.value = 'm'; })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; a", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; a", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; a b", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; b", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; b c", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; d c", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; d c", "attributes ; p#p1 ;  ;  ; null ; null ; class ;  ; k"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().dataset.foo = 'x'; P().dataset.foo = 'y'; delete P().dataset.foo; delete P().dataset.foo; })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; data-foo ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; data-foo ;  ; x", "attributes ; p#p1 ;  ;  ; null ; null ; data-foo ;  ; y"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { const t = document.createAttribute('t'); t.value = 'v'; P().setAttributeNode(t); t.value = 'w'; P().removeAttributeNode(t); P().attributes.setNamedItem(t); P().attributes.removeNamedItem('t'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; t ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; t ;  ; v", "attributes ; p#p1 ;  ;  ; null ; null ; t ;  ; w", "attributes ; p#p1 ;  ;  ; null ; null ; t ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; t ;  ; w"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().id = 'q'; P().id = 'q'; P().hidden = true; P().hidden = true; P().hidden = false; P().title = 't'; P().lang = 'en'; P().tabIndex = 2; P().draggable = true; P().dir = 'rtl'; P().accessKey = 'k'; P().translate = false; })",
                r#"["attributes ; p#q ;  ;  ; null ; null ; id ;  ; p1", "attributes ; p#q ;  ;  ; null ; null ; id ;  ; q", "attributes ; p#q ;  ;  ; null ; null ; hidden ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; hidden ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; hidden ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; title ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; lang ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; tabindex ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; draggable ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; dir ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; accesskey ;  ; ", "attributes ; p#q ;  ;  ; null ; null ; translate ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().style.color = 'red'; P().style.color = 'blue'; P().style.color = 'blue'; P().style.setProperty('margin', '1px'); P().style.removeProperty('margin'); P().style.removeProperty('margin'); P().style.cssText = 'top: 1px'; P().style.cssText = 'top: 1px'; P().setAttribute('style', 'left: 2px'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; color: red;", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; color: blue;", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; color: blue; margin: 1px;", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; color: blue;", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; top: 1px;", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; top: 1px;"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().style.color = 'red'; P().style.color = ''; P().style.cssText = ''; P().style.width = '1px'; P().style.removeProperty('width'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; color: red;", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; width: 1px;"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { s1.style.color = 'red'; const x = s1.getAttribute('style'); s1.style.color = 'blue'; s1.style.color = 'green'; s1.removeAttribute('style'); })",
                r#"["attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; color: red;", "attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; color: blue;", "attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; color: blue;"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', CL, () => { P().style.color = 'red'; P().setAttribute('style', 'top: 1px') })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { const i = document.createElement('i'); a.appendChild(i); a.appendChild(i); a.insertBefore(i, P()); a.insertBefore(i, i); a.insertBefore(i, null); })",
                r#"["childList ; div#a ; i ;  ; span#s1 ; null ;  ;  ; ", "childList ; div#a ;  ; i ; span#s1 ; null ;  ;  ; ", "childList ; div#a ; i ;  ; span#s1 ; null ;  ;  ; ", "childList ; div#a ;  ; i ; span#s1 ; null ;  ;  ; ", "childList ; div#a ; i ;  ; null ; p#p1 ;  ;  ; ", "childList ; div#a ;  ; i ; null ; p#p1 ;  ;  ; ", "childList ; div#a ; i ;  ; null ; p#p1 ;  ;  ; ", "childList ; div#a ;  ; i ; null ; p#p1 ;  ;  ; ", "childList ; div#a ; i ;  ; span#s1 ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { a.removeChild(P()); a.appendChild(P()); a.replaceChild(document.createElement('u'), s1); })",
                r#"["childList ; div#a ;  ; p#p1 ; null ; span#s1 ;  ;  ; ", "childList ; div#a ;  ; span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ; span#s1 ;  ; null ; null ;  ;  ; ", "childList ; div#a ; u ; span#s1 ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { const f = document.createDocumentFragment(); f.append(document.createElement('i'), 'txt', document.createElement('u')); a.insertBefore(f, s1); a.appendChild(f); a.appendChild(document.createDocumentFragment()); })",
                r#"["childList ; div#a ; i|#text(txt)|u ;  ; p#p1 ; span#s1 ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { a.append('x', 'y'); a.prepend('p', document.createElement('i')); a.append(); a.prepend(); a.append('');  })",
                r#"["childList ; div#a ; #text(x)|#text(y) ;  ; span#s1 ; null ;  ;  ; ", "childList ; div#a ; #text(p)|i ;  ; null ; p#p1 ;  ;  ; ", "childList ; div#a ; #text() ;  ; #text(y) ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().before('b'); P().after('a'); s1.replaceWith('r', document.createElement('i')); a.firstChild.remove(); a.firstChild.remove(); const m = document.createElement('m'); m.remove(); })",
                r#"["childList ; div#a ; #text(b) ;  ; null ; p#p1 ;  ;  ; ", "childList ; div#a ; #text(a) ;  ; p#p1 ; span#s1 ;  ;  ; ", "childList ; div#a ; #text(r)|i ; span#s1 ; #text(a) ; null ;  ;  ; ", "childList ; div#a ;  ; #text(b) ; null ; p#p1 ;  ;  ; ", "childList ; div#a ;  ; p#p1 ; null ; #text(a) ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { s1.replaceWith(); P().replaceWith(P()); })",
                r#"["childList ; div#a ;  ; span#s1 ; p#p1 ; null ;  ;  ; ", "childList ; div#a ;  ; p#p1 ; null ; null ;  ;  ; ", "childList ; div#a ; p#p1 ;  ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { a.replaceChildren(); a.replaceChildren(); a.replaceChildren('x', document.createElement('i')); a.replaceChildren(a.firstChild); })",
                r#"["childList ; div#a ;  ; p#p1|span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ; #text(x)|i ;  ; null ; null ;  ;  ; ", "childList ; div#a ; #text(x) ; #text(x)|i ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { a.textContent = 'abc'; a.textContent = ''; a.textContent = ''; a.textContent = 'def'; a.textContent = 'def' })",
                r#"["childList ; div#a ; #text(abc) ; p#p1|span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ;  ; #text(abc) ; null ; null ;  ;  ; ", "childList ; div#a ; #text(def) ;  ; null ; null ;  ;  ; ", "childList ; div#a ; #text(def) ; #text(def) ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { a.innerHTML = '<i>1</i><i>2</i>'; a.innerHTML = ''; a.innerHTML = ''; a.innerHTML = 'x'; a.innerHTML = 'x' })",
                r#"["childList ; div#a ; i|i ; p#p1|span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ;  ; i|i ; null ; null ;  ;  ; ", "childList ; div#a ; #text(x) ;  ; null ; null ;  ;  ; ", "childList ; div#a ; #text(x) ; #text(x) ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().innerHTML = ''; P().innerHTML = '<b>1</b>'; s1.outerHTML = '<u>1</u><u>2</u>'; a.firstChild.outerHTML = 'plain'; })",
                r#"["childList ; p#p1 ;  ; #text(x) ; null ; null ;  ;  ; ", "childList ; p#p1 ; b ;  ; null ; null ;  ;  ; ", "childList ; div#a ; u|u ; span#s1 ; p#p1 ; null ;  ;  ; ", "childList ; div#a ; #text(plain) ; p#p1 ; null ; u ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { P().insertAdjacentHTML('beforebegin', '<i>1</i>'); P().insertAdjacentHTML('afterbegin', '<i>2</i>'); P().insertAdjacentHTML('beforeend', 'txt'); P().insertAdjacentHTML('afterend', '<i>3</i><i>4</i>'); P().insertAdjacentElement('afterend', document.createElement('e')); P().insertAdjacentText('afterbegin', 'tt'); P().insertAdjacentText('beforebegin', '') })",
                r#"["childList ; div#a ; i ;  ; null ; p#p1 ;  ;  ; ", "childList ; i ; i ;  ; null ; #text(1) ;  ;  ; ", "childList ; i ; #text(txt) ;  ; #text(1) ; null ;  ;  ; ", "childList ; div#a ; i|i ;  ; i ; p#p1 ;  ;  ; ", "childList ; div#a ; e ;  ; i ; i ;  ;  ; ", "childList ; i ; #text(tt) ;  ; null ; i ;  ;  ; ", "childList ; div#a ; #text() ;  ; null ; i ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', ALL, () => { a.innerText = 'a\nb'; a.innerText = ''; a.innerText = 'c'; a.innerText = 'c'; P().innerText = 'q' })",
                r#"["childList ; div#a ; #text(a)|br|#text(b) ; p#p1|span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ;  ; #text(a)|br|#text(b) ; null ; null ;  ;  ; ", "childList ; div#a ; #text(c) ;  ; null ; null ;  ;  ; ", "childList ; div#a ; #text(c) ; #text(c) ; null ; null ;  ;  ; ", "threw TypeError: Cannot set properties of null (setting 'innerText')"]"#,
            ),
            (
                r"T('<p id=p1>x</p><span id=s1></span>', SUB, () => { a.innerText = 'a\nb'; a.innerText = ''; a.innerText = 'c'; a.innerText = 'c'; s1.innerText = 'q'; s1.outerText = 'w' })",
                r#"["childList ; div#a ; #text(a)|br|#text(b) ; p#p1|span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ;  ; #text(a)|br|#text(b) ; null ; null ;  ;  ; ", "childList ; div#a ; #text(c) ;  ; null ; null ;  ;  ; ", "childList ; div#a ; #text(c) ; #text(c) ; null ; null ;  ;  ; ", "threw ReferenceError: s1 is not defined"]"#,
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', ALL, () => { P().normalize(); a.normalize(); P().firstChild.after('tt', 'uu'); P().normalize(); P().lastChild.before(document.createTextNode('')); P().normalize(); })",
                r#"["childList ; p#p1 ; #text(tt)|#text(uu) ;  ; #text(xttuu) ; b#b1 ;  ;  ; ", "characterData ; #text(xttuu) ;  ;  ; null ; null ;  ;  ; x", "childList ; p#p1 ;  ; #text(tt) ; #text(xttuu) ; #text(uu) ;  ;  ; ", "characterData ; #text(xttuu) ;  ;  ; null ; null ;  ;  ; xtt", "childList ; p#p1 ;  ; #text(uu) ; #text(xttuu) ; b#b1 ;  ;  ; ", "childList ; p#p1 ; #text() ;  ; b#b1 ; #text(z) ;  ;  ; ", "childList ; p#p1 ;  ; #text() ; b#b1 ; #text(z) ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', ALL, () => { P().firstChild.splitText(0); P().firstChild.splitText(1); b1.firstChild.splitText(1); P().lastChild.splitText(1); })",
                r#"["characterData ; #text() ;  ;  ; null ; null ;  ;  ; x", "childList ; p#p1 ; #text(x) ;  ; #text() ; b#b1 ;  ;  ; ", "threw IndexSizeError: Failed to execute 'splitText' on 'Text': The offset 1 is larger than the Text node's length."]"#,
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', ALL, () => { const t = P().firstChild; const u = t.cloneNode(true); P().cloneNode(true); document.importNode(P(), true); a.cloneNode(true); document.createElement('i').appendChild(t.cloneNode());  })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', ALL, () => { const t = P().firstChild; document.adoptNode(P()); a.appendChild(P()); document.adoptNode(t); P().appendChild(t); document.adoptNode(document.createElement('z')); })",
                r#"["childList ; div#a ;  ; p#p1 ; null ; span#s1 ;  ;  ; ", "childList ; div#a ;  ; span#s1 ; null ; null ;  ;  ; ", "childList ; div#a ; span#s1 ;  ; null ; null ;  ;  ; ", "childList ; p#p1 ;  ; #text(x) ; null ; b#b1 ;  ;  ; ", "childList ; span#s1 ; #text(x) ;  ; #text(q) ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', SUB, () => { const d2 = document.implementation.createHTMLDocument(''); const n = d2.adoptNode(P()); d2.body.appendChild(n); n.id = 'moved'; const m = d2.importNode(s1, true); m.id = 'imp'; })",
                r#"["childList ; div#a ;  ; p#moved ; null ; span#s1 ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', ALL, () => { const c = document.createComment('c'); a.appendChild(c); c.data = 'd'; c.appendData('e'); c.textContent = 'f'; c.nodeValue = 'g'; c.remove(); })",
                r#"["childList ; div#a ; #comment(g) ;  ; span#s1 ; null ;  ;  ; ", "characterData ; #comment(g) ;  ;  ; null ; null ;  ;  ; c", "characterData ; #comment(g) ;  ;  ; null ; null ;  ;  ; d", "characterData ; #comment(g) ;  ;  ; null ; null ;  ;  ; de", "characterData ; #comment(g) ;  ;  ; null ; null ;  ;  ; f", "childList ; div#a ;  ; #comment(g) ; span#s1 ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x<b id=b1>y</b>z</p><span id=s1>q</span>', ALL, () => { b1.firstChild.data = 'Y'; b1.firstChild.data = 'Y'; b1.firstChild.replaceData(0, 0, ''); b1.firstChild.appendData(''); b1.firstChild.insertData(1, ''); b1.firstChild.deleteData(0, 0); b1.firstChild.substringData(0, 1) })",
                r#"["characterData ; #text(Y) ;  ;  ; null ; null ;  ;  ; y", "characterData ; #text(Y) ;  ;  ; null ; null ;  ;  ; Y", "characterData ; #text(Y) ;  ;  ; null ; null ;  ;  ; Y", "characterData ; #text(Y) ;  ;  ; null ; null ;  ;  ; Y", "characterData ; #text(Y) ;  ;  ; null ; null ;  ;  ; Y", "characterData ; #text(Y) ;  ;  ; null ; null ;  ;  ; Y"]"#,
            ),
        ],
    },
    Case {
        name: "mo-forms",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"T('<input id=i1 value=v><input id=i2 type=checkbox><textarea id=t1>tx</textarea><select id=sel><option id=o1 value=1>one<option id=o2 value=2 selected>two</select>', ALL, () => { i1.value = 'w'; i1.defaultValue = 'dv'; i1.checked = true; i2.checked = true; i2.defaultChecked = true; i2.indeterminate = true; i1.type = 'text'; i1.name = 'n'; i1.disabled = true; i1.readOnly = true; i1.required = true; i1.placeholder = 'p'; i1.maxLength = 3 })",
                r#"["attributes ; input#i1 ;  ;  ; null ; null ; value ;  ; v", "attributes ; input#i2 ;  ;  ; null ; null ; checked ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; type ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; name ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; disabled ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; readonly ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; required ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; placeholder ;  ; ", "attributes ; input#i1 ;  ;  ; null ; null ; maxlength ;  ; "]"#,
            ),
            (
                r"T('<input id=i1 value=v><input id=i2 type=checkbox><textarea id=t1>tx</textarea><select id=sel><option id=o1 value=1>one<option id=o2 value=2 selected>two</select>', ALL, () => { t1.value = 'new'; t1.defaultValue = 'dflt'; t1.rows = 3; })",
                r#"["childList ; textarea#t1 ; #text(dflt) ; #text(tx) ; null ; null ;  ;  ; ", "attributes ; textarea#t1 ;  ;  ; null ; null ; rows ;  ; "]"#,
            ),
            (
                r"T('<input id=i1 value=v><input id=i2 type=checkbox><textarea id=t1>tx</textarea><select id=sel><option id=o1 value=1>one<option id=o2 value=2 selected>two</select>', ALL, () => { sel.value = '1'; sel.selectedIndex = 1; o1.selected = true; o2.selected = false; o1.defaultSelected = true; o1.value = 'x'; o1.text = 'uno'; o1.label = 'lab'; o1.disabled = true; })",
                r#"["attributes ; option#o1 ;  ;  ; null ; null ; selected ;  ; ", "attributes ; option#o1 ;  ;  ; null ; null ; value ;  ; 1", "childList ; option#o1 ; #text(uno) ; #text(one) ; null ; null ;  ;  ; ", "attributes ; option#o1 ;  ;  ; null ; null ; label ;  ; ", "attributes ; option#o1 ;  ;  ; null ; null ; disabled ;  ; "]"#,
            ),
            (
                r"T('<input id=i1 value=v><input id=i2 type=checkbox><textarea id=t1>tx</textarea><select id=sel><option id=o1 value=1>one<option id=o2 value=2 selected>two</select>', ALL, () => { sel.add(new Option('three', '3')); sel.remove(0); sel.options[0].remove(); sel.options.length = 0; sel.length = 2; sel.options.add(new Option('x')); })",
                r#"["childList ; select#sel ; option ;  ; option#o2 ; null ;  ;  ; ", "childList ; select#sel ;  ; option#o1 ; null ; option#o2 ;  ;  ; ", "childList ; select#sel ;  ; option#o2 ; null ; option ;  ;  ; ", "childList ; select#sel ;  ; option ; null ; null ;  ;  ; ", "childList ; select#sel ; option ;  ; null ; null ;  ;  ; ", "childList ; select#sel ; option ;  ; option ; null ;  ;  ; ", "childList ; select#sel ; option ;  ; option ; null ;  ;  ; "]"#,
            ),
            (
                r"T('<form id=f1><input id=i1 value=v><input id=i2 type=checkbox></form>', ALL, () => { f1.reset(); i1.value = 'z'; f1.reset(); i2.click(); i2.click(); i1.focus(); i1.blur(); f1.action = 'x'; f1.method = 'post' })",
                r#"["attributes ; form#f1 ;  ;  ; null ; null ; action ;  ; ", "attributes ; form#f1 ;  ;  ; null ; null ; method ;  ; "]"#,
            ),
            (
                r"T('<table id=tb><tbody><tr><td>c</td></tr></tbody></table>', ALL, () => { tb.insertRow(); tb.rows[0].insertCell(); tb.deleteRow(0); tb.createCaption(); tb.createTHead(); tb.deleteCaption(); tb.tBodies[0].insertRow(0).insertCell(0).colSpan = 2; })",
                r#"["childList ; tbody ; tr ;  ; tr ; null ;  ;  ; ", "childList ; tr ; td ;  ; td ; null ;  ;  ; ", "childList ; tbody ;  ; tr ; null ; tr ;  ;  ; ", "childList ; table#tb ; caption ;  ; null ; tbody ;  ;  ; ", "childList ; table#tb ; thead ;  ; caption ; tbody ;  ;  ; ", "childList ; table#tb ;  ; caption ; null ; thead ;  ;  ; ", "childList ; tbody ; tr ;  ; null ; tr ;  ;  ; ", "childList ; tr ; td ;  ; null ; null ;  ;  ; ", "attributes ; td ;  ;  ; null ; null ; colspan ;  ; "]"#,
            ),
            (
                r"T('<img id=im src=a.png><a id=an href=x>t</a><details id=dt><summary>s</summary>d</details>', ALL, () => { im.src = 'b.png'; im.alt = 'x'; im.width = 5; im.setAttribute('src', 'b.png'); an.href = 'y'; an.text = 'newtext'; an.target = '_blank'; an.rel = 'noopener'; an.relList.add('x'); dt.open = true; dt.open = true; dt.open = false; })",
                r#"["attributes ; img#im ;  ;  ; null ; null ; src ;  ; a.png", "attributes ; img#im ;  ;  ; null ; null ; alt ;  ; ", "attributes ; img#im ;  ;  ; null ; null ; width ;  ; ", "attributes ; img#im ;  ;  ; null ; null ; src ;  ; b.png", "attributes ; a#an ;  ;  ; null ; null ; href ;  ; x", "childList ; a#an ; #text(newtext) ; #text(t) ; null ; null ;  ;  ; ", "attributes ; a#an ;  ;  ; null ; null ; target ;  ; ", "attributes ; a#an ;  ;  ; null ; null ; rel ;  ; ", "attributes ; a#an ;  ;  ; null ; null ; rel ;  ; noopener", "attributes ; details#dt ;  ;  ; null ; null ; open ;  ; ", "attributes ; details#dt ;  ;  ; null ; null ; open ;  ; ", "attributes ; details#dt ;  ;  ; null ; null ; open ;  ; "]"#,
            ),
            (
                r"T('<script id=sc></script><div id=d1 tabindex=0></div>', ALL, () => { sc.text = '1'; sc.textContent = '2'; sc.src = 'x.js'; sc.async = true; sc.type = 'text/plain'; d1.focus(); d1.blur(); d1.scrollTop = 5; d1.scrollIntoView(); d1.click(); d1.getBoundingClientRect(); window.getComputedStyle(d1).color; d1.offsetWidth; })",
                r#"["childList ; script#sc ; #text(1) ;  ; null ; null ;  ;  ; ", "childList ; script#sc ; #text(2) ; #text(1) ; null ; null ;  ;  ; ", "attributes ; script#sc ;  ;  ; null ; null ; src ;  ; ", "attributes ; script#sc ;  ;  ; null ; null ; async ;  ; ", "attributes ; script#sc ;  ;  ; null ; null ; type ;  ; "]"#,
            ),
            (
                r"T('<canvas id=cv></canvas><video id=vd></video><dialog id=dg></dialog>', ALL, () => { cv.width = 10; cv.height = 20; vd.controls = true; vd.muted = true; dg.open = true; dg.setAttribute('open', ''); dg.removeAttribute('open') })",
                r#"["attributes ; canvas#cv ;  ;  ; null ; null ; width ;  ; ", "attributes ; canvas#cv ;  ;  ; null ; null ; height ;  ; ", "attributes ; video#vd ;  ;  ; null ; null ; controls ;  ; ", "attributes ; dialog#dg ;  ;  ; null ; null ; open ;  ; ", "attributes ; dialog#dg ;  ;  ; null ; null ; open ;  ; ", "attributes ; dialog#dg ;  ;  ; null ; null ; open ;  ; "]"#,
            ),
            (
                r"T('<p id=p1 contenteditable=true>x</p>', ALL, () => { p1.spellcheck = false; p1.inert = true; p1.autofocus = true; p1.enterKeyHint = 'go'; p1.inputMode = 'numeric'; p1.autocapitalize = 'words' })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; spellcheck ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; inert ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; autofocus ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; enterkeyhint ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; inputmode ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; autocapitalize ;  ; "]"#,
            ),
        ],
    },
    Case {
        name: "mo-transient",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const out = []; const o = new MutationObserver(() => {}); o.observe(a, SUB); const e = document.createElement('i'); a.appendChild(e); a.removeChild(e); e.id = 'after'; e.appendChild(document.createElement('k')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["childList ; div#a ; i#after ;  ; span#s1 ; null ;  ;  ; ", "childList ; div#a ;  ; i#after ; span#s1 ; null ;  ;  ; ", "childList ; i#after ; k ;  ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, subtree: true, attributeOldValue: true}); const p = P(); a.removeChild(p); p.id = 'q1'; p.firstChild.data = 'zz'; const r1 = o.takeRecords().map(R); p.id = 'q2'; const r2 = o.takeRecords().map(R); o.disconnect(); return [r1, r2]; })()",
                r#"[["attributes ; p#q1 ;  ;  ; null ; null ; id ;  ; p1"], ["attributes ; p#q2 ;  ;  ; null ; null ; id ;  ; q1"]]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, subtree: true, attributeOldValue: true}); const p = P(); a.removeChild(p); p.id = 'q1'; const r1 = o.takeRecords().map(R); return new Promise(res => queueMicrotask(() => { p.id = 'q2'; res([r1, o.takeRecords().map(R)]); })); })()",
                r#"[["attributes ; p#q1 ;  ;  ; null ; null ; id ;  ; p1"], []]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, subtree: true}); const p = P(); a.removeChild(p); a.appendChild(p); p.id = 'q1'; const r1 = o.takeRecords().map(R); o.disconnect(); return r1; })()",
                r#"["attributes ; p#q1 ;  ;  ; null ; null ; id ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const log = []; const o = new MutationObserver(r => { log.push('delivered ' + r.length); }); o.observe(a, {attributes: true, subtree: true}); const p = P(); a.removeChild(p); p.id = 'q1'; return new Promise(res => setTimeout(() => { p.id = 'q2'; setTimeout(() => { o.disconnect(); res(log); }, 10); }, 10)); })()",
                r#"["delivered 1"]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const log = []; const o = new MutationObserver(r => { log.push('delivered ' + r.length + R(r[0])); }); o.observe(a, {attributes: true, subtree: true}); const p = P(); const q = document.createElement('q'); p.appendChild(q); a.removeChild(p); q.id = 'deep'; return new Promise(res => setTimeout(() => { o.disconnect(); res(log); }, 10)); })()",
                r#"["delivered 1attributes ; q#deep ;  ;  ; null ; null ; id ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const log = []; const o = new MutationObserver(r => { log.push('delivered ' + r.length); }); o.observe(P(), {attributes: true}); const p = P(); a.removeChild(p); p.id = 'q1'; return new Promise(res => setTimeout(() => { o.disconnect(); res(log); }, 10)); })()",
                r#"["delivered 1"]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {attributes: true, subtree: true, attributeOldValue: true}); const o2 = new MutationObserver(() => {}); o2.observe(P(), {attributes: true}); const p = P(); a.removeChild(p); p.id = 'q1'; const r = [o.takeRecords().map(R), o2.takeRecords().map(R)]; o.disconnect(); o2.disconnect(); return r; })()",
                r#"[["attributes ; p#q1 ;  ;  ; null ; null ; id ;  ; p1"], ["attributes ; p#q1 ;  ;  ; null ; null ; id ;  ; "]]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {childList: true, subtree: true}); const p = P(); const q = document.createElement('q'); a.removeChild(p); p.appendChild(q); a.appendChild(document.createElement('z')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["childList ; div#a ;  ; p#p1 ; null ; span#s1 ;  ;  ; ", "childList ; p#p1 ; q ;  ; #text(x) ; null ;  ;  ; ", "childList ; div#a ; z ;  ; span#s1 ; null ;  ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(a, {childList: true, subtree: true}); const p = P(); const s = s1; a.replaceChildren(); p.appendChild(document.createElement('q')); s.appendChild(document.createElement('w')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["childList ; div#a ;  ; p#p1|span#s1 ; null ; null ;  ;  ; ", "childList ; p#p1 ; q ;  ; #text(x) ; null ;  ;  ; ", "childList ; span#s1 ; w ;  ; null ; null ;  ;  ; "]"#,
            ),
            (
                r"(() => { a.innerHTML = '<p id=p1>x</p><span id=s1></span>'; const o = new MutationObserver(() => {}); o.observe(document.body, {childList: true, subtree: true}); const d2 = document.createElement('div'); d2.innerHTML = '<p></p>'; d2.firstChild.appendChild(document.createElement('s')); a.appendChild(d2); d2.firstChild.appendChild(document.createElement('t')); const r = o.takeRecords().map(R); o.disconnect(); return r; })()",
                r#"["childList ; div#a ; div ;  ; span#s1 ; null ;  ;  ; ", "childList ; p ; t ;  ; s ; null ;  ;  ; "]"#,
            ),
        ],
    },
    Case {
        name: "mo-delivery",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(function (r, ob) { L.push(['cb', r.length, this === o, ob === o, arguments.length, r instanceof Array, Object.isFrozen(r), r[0] instanceof MutationRecord].join()); }); o.observe(a, CL); a.appendChild(document.createElement('i')); L.push('sync'); await wait(5); o.disconnect(); return L; })()",
                r#"["sync", "cb,1,true,true,2,true,false,true"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); Promise.resolve().then(() => L.push('p1')); a.appendChild(document.createElement('i')); Promise.resolve().then(() => L.push('p2')); queueMicrotask(() => L.push('q1')); setTimeout(() => L.push('t0'), 0); await wait(5); o.disconnect(); return L; })()",
                r#"["p1", "mo 1", "p2", "q1", "t0"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); Promise.resolve().then(() => { L.push('p1'); a.appendChild(document.createElement('i')); Promise.resolve().then(() => L.push('p3')); }); Promise.resolve().then(() => L.push('p2')); await wait(5); o.disconnect(); return L; })()",
                r#"["p1", "p2", "mo 1", "p3"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o1 = new MutationObserver(r => L.push('o1 ' + r.length)); const o2 = new MutationObserver(r => L.push('o2 ' + r.length)); const o3 = new MutationObserver(r => L.push('o3 ' + r.length)); o3.observe(a, CL); o2.observe(a, CL); o1.observe(a, CL); a.appendChild(document.createElement('i')); await wait(5); o1.disconnect(); o2.disconnect(); o3.disconnect(); return L; })()",
                r#"["o1 1", "o2 1", "o3 1"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => { L.push('mo ' + r.length); if (L.length < 6) a.appendChild(document.createElement('i')); }); o.observe(a, CL); a.appendChild(document.createElement('i')); Promise.resolve().then(() => L.push('p')); await wait(5); o.disconnect(); return L; })()",
                r#"["mo 1", "p", "mo 1", "mo 1", "mo 1", "mo 1"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o1 = new MutationObserver(r => { L.push('o1 ' + r.length); if (L.length < 6) a.appendChild(document.createElement('i')); }); const o2 = new MutationObserver(r => L.push('o2 ' + r.length)); o1.observe(a, CL); o2.observe(a, CL); a.appendChild(document.createElement('i')); await wait(5); L.push('end'); o1.disconnect(); o2.disconnect(); return L; })()",
                r#"["o1 1", "o2 2", "o1 1", "o2 1", "o1 1", "o2 1", "o1 1", "end"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => { L.push('mo ' + r.length); throw new Error('boom'); }); window.onerror = (m) => { L.push('onerror ' + m); return true; }; o.observe(a, CL); a.appendChild(document.createElement('i')); await wait(5); a.appendChild(document.createElement('i')); await wait(5); window.onerror = null; o.disconnect(); return L; })()",
                r#"["mo 1", "onerror Uncaught Error: boom", "mo 1", "onerror Uncaught Error: boom"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); a.appendChild(document.createElement('i')); const t = o.takeRecords(); L.push('taken ' + t.length); L.push('again ' + o.takeRecords().length); await wait(5); o.disconnect(); return L; })()",
                r#"["taken 1", "again 0"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); a.appendChild(document.createElement('i')); o.disconnect(); a.appendChild(document.createElement('i')); await wait(5); L.push('end'); o.observe(a, CL); a.appendChild(document.createElement('i')); await wait(5); o.disconnect(); return L; })()",
                r#"["end", "mo 1"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); a.appendChild(document.createElement('i')); o.observe(a, {attributes: true}); a.appendChild(document.createElement('i')); a.title = 'zz'; await wait(5); o.disconnect(); return L; })()",
                r#"["mo 2"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => { L.push('mo ' + r.length); o.takeRecords(); o.disconnect(); }); o.observe(a, CL); a.appendChild(document.createElement('i')); a.appendChild(document.createElement('i')); await wait(5); return L; })()",
                r#"["mo 2"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); a.appendChild(document.createElement('i')); await Promise.resolve(); await Promise.resolve(); L.push('after awaits'); await wait(5); o.disconnect(); return L; })()",
                r#"["mo 1", "after awaits"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); const b = document.createElement('div'); o.observe(b, CL); b.appendChild(document.createElement('i')); await wait(5); o.disconnect(); return L; })()",
                r#"["mo 1"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => L.push('mo ' + r.length)); o.observe(a, CL); const ev = []; a.addEventListener('click', () => { ev.push('l1'); a.appendChild(document.createElement('i')); Promise.resolve().then(() => ev.push('p')); }); a.addEventListener('click', () => ev.push('l2')); a.click(); L.push(...ev); await wait(5); o.disconnect(); return L; })()",
                r#"["l1", "l2", "mo 1"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => { L.push('mo ' + r.length); }); o.observe(document, {childList: true, subtree: true, attributes: true, characterData: true}); document.title = 'newtitle'; document.documentElement.lang = 'x'; await wait(5); o.disconnect(); return L; })()",
                r#"["mo 3"]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => { r.forEach(x => L.push(R(x))); }); o.observe(document, {childList: true, subtree: true, attributes: true, characterData: true}); document.body.className = 'k'; document.title = 'newtitle'; const s = document.createElement('style'); s.textContent = 'p{color:red}'; document.head.appendChild(s); s.remove(); await wait(5); o.disconnect(); return L; })()",
                r#"["attributes ; body ;  ;  ; null ; null ; class ;  ; ", "childList ; title ; #text(newtitle) ; #text(newtitle) ; null ; null ;  ;  ; ", "childList ; head ; style ;  ; title ; null ;  ;  ; ", "childList ; head ;  ; style ; title ; null ;  ;  ; "]"#,
            ),
            (
                r"(async () => { L.length = 0; const o = new MutationObserver(r => { r.forEach(x => L.push(R(x))); }); o.observe(a, {childList: true}); const c = a.cloneNode(true); c.removeAttribute('id'); a.appendChild(c); const A = a; document.body.appendChild(A); document.body.appendChild(A); await wait(5); o.disconnect(); return L; })()",
                r#"["childList ; div#a ; div ;  ; i ; null ;  ;  ; "]"#,
            ),
        ],
    },
    Case {
        name: "parser-document",
        html: r"<!DOCTYPE html><html><head></head><body><p id=pre>before</p><script>
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

window.PL = []; window.PLC = 0;
var po = new MutationObserver(function (recs) { PLC++; recs.forEach(r => PL.push(R(r))); }); po.observe(document, {childList: true, subtree: true, attributes: true, characterData: true, attributeOldValue: true, characterDataOldValue: true});</script><p id=p2 class=c>after <b id=b2>bold</b> text</p><div id=d3><span>s</span></div><!-- cmt --><script>PL.push('script2 ' + document.getElementById('d3'));</script><ul><li>one<li>two</ul><table><tr><td>cell</table></body></html>",
        queries: &[
            (
                r"(async () => { await wait(20); return window.PL; })()",
                r#"["childList ; body ; p#p2 ;  ; script ; null ;  ;  ; ", "childList ; p#p2 ; #text(after ) ;  ; null ; null ;  ;  ; ", "childList ; p#p2 ; b#b2 ;  ; #text(after ) ; null ;  ;  ; ", "childList ; b#b2 ; #text(bold) ;  ; null ; null ;  ;  ; ", "childList ; p#p2 ; #text( text) ;  ; b#b2 ; null ;  ;  ; ", "childList ; body ; div#d3 ;  ; p#p2 ; null ;  ;  ; ", "childList ; div#d3 ; span ;  ; null ; null ;  ;  ; ", "childList ; span ; #text(s) ;  ; null ; null ;  ;  ; ", "childList ; body ; #comment( cmt ) ;  ; div#d3 ; null ;  ;  ; ", "childList ; body ; script ;  ; #comment( cmt ) ; null ;  ;  ; ", "childList ; script ; #text(PL.push('script2 ' + document.getElementById('d3'));) ;  ; null ; null ;  ;  ; ", "script2 [object HTMLDivElement]", "childList ; body ; ul ;  ; script ; null ;  ;  ; ", "childList ; ul ; li ;  ; null ; null ;  ;  ; ", "childList ; li ; #text(one) ;  ; null ; null ;  ;  ; ", "childList ; ul ; li ;  ; li ; null ;  ;  ; ", "childList ; li ; #text(two) ;  ; null ; null ;  ;  ; ", "childList ; body ; table ;  ; ul ; null ;  ;  ; ", "childList ; table ; tbody ;  ; null ; null ;  ;  ; ", "childList ; tbody ; tr ;  ; null ; null ;  ;  ; ", "childList ; tr ; td ;  ; null ; null ;  ;  ; ", "childList ; td ; #text(cell) ;  ; null ; null ;  ;  ; "]"#,
            ),
            (r"window.PLC", r"2"),
        ],
    },
    Case {
        name: "parser-body-only",
        html: r"<!DOCTYPE html><html><head></head><body><div id=host><script>
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

window.PL = []; window.PLC = 0;
var po = new MutationObserver(function (recs) { PLC++; recs.forEach(r => PL.push(R(r))); }); po.observe(document.getElementById('host'), {childList: true, subtree: true});</script><i>x</i>text<b>y</b></div></body></html>",
        queries: &[
            (
                r"(async () => { await wait(20); return window.PL; })()",
                r#"["childList ; div#host ; i ;  ; script ; null ;  ;  ; ", "childList ; i ; #text(x) ;  ; null ; null ;  ;  ; ", "childList ; div#host ; #text(text) ;  ; i ; null ;  ;  ; ", "childList ; div#host ; b ;  ; #text(text) ; null ;  ;  ; ", "childList ; b ; #text(y) ;  ; null ; null ;  ;  ; "]"#,
            ),
            (r"window.PLC", r"1"),
        ],
    },
    Case {
        name: "parser-microtask",
        html: r"<!DOCTYPE html><html><head></head><body><div id=host><script>
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

window.PL = []; window.PLC = 0;
var po = new MutationObserver(function (recs) { PL.push('mo ' + recs.length); }); po.observe(document.getElementById('host'), {childList: true, subtree: true});</script><i>x</i><script>PL.push('s2 ' + po.takeRecords().length); Promise.resolve().then(() => PL.push('p'));</script><b>y</b><script>PL.push('s3');</script></div></body></html>",
        queries: &[
            (
                r"(async () => { await wait(20); return window.PL; })()",
                r#"["mo 4", "s2 0", "p", "mo 4", "s3"]"#,
            ),
            (r"window.PLC", r"0"),
        ],
    },
    Case {
        name: "parser-load-event",
        html: r"<!DOCTYPE html><html><head></head><body><div id=host><script>
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

window.PL = []; window.PLC = 0;
var po = new MutationObserver(function (recs) { PLC++; recs.forEach(r => PL.push(R(r))); }); po.observe(document, {childList: true, subtree: true, attributes: true, characterData: true});</script></div><script>document.addEventListener('DOMContentLoaded', () => PL.push('dcl')); window.addEventListener('load', () => PL.push('load'));</script></body></html>",
        queries: &[
            (
                r"(async () => { await wait(20); return window.PL; })()",
                r#"["childList ; body ; script ;  ; div#host ; null ;  ;  ; ", "childList ; script ; #text(document.addEventListener('DOMContentLoaded', () => PL.push('dcl')); window.addEventListener('load', () => PL.push('load'));) ;  ; null ; null ;  ;  ; ", "dcl", "load"]"#,
            ),
            (r"window.PLC", r"1"),
        ],
    },
    Case {
        name: "mo-style",
        html: r"<!DOCTYPE html><html><head></head><body><div id=a><p id=p1>x</p><span id=s1></span></div><script>
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

// T: sets a's content to html, observes (the target is a unless root() gives another), runs ops,
// and returns the records as strings (taken by takeRecords).
function T(html, opts, ops, root) {
  a.innerHTML = html;
  const o = new MutationObserver(() => {});
  o.observe(root ? root() : a, opts);
  let thrown = [];
  try { ops(); } catch (e) { thrown = ['threw ' + e.name + ': ' + e.message]; }
  const recs = o.takeRecords();
  o.disconnect();
  return recs.map(R).concat(thrown);
}
const CL = {childList: true};
const SUB = {childList: true, subtree: true};
const AT = {attributes: true, attributeOldValue: true};
const CD = {characterData: true, characterDataOldValue: true, subtree: true};
const P = () => a.firstElementChild;
</script></body></html>",
        queries: &[
            (
                r"T('<p id=p1>x</p>', ALL, () => { P().style.setProperty('x','1'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p>', ALL, () => { P().setAttribute('style','a:b'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; "]"#,
            ),
            (
                r"T('<p id=p1>x</p>', ALL, () => { P().style.color='red'; P().setAttribute('style','a:b'); })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; color: red;"]"#,
            ),
            (
                r"T('<p id=p1>x</p>', ALL, () => { P().style.removeProperty('color'); })",
                r"[]",
            ),
            (
                r"T('<p id=p1>x</p>', ALL, () => { P().style.color=''; P().style.cssText=''; })",
                r#"["attributes ; p#p1 ;  ;  ; null ; null ; style ;  ; "]"#,
            ),
        ],
    },
];
