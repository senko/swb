// The cases of `tools/probes/web-dom-query.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-dom-query.json`, three
// runs, all equal); where swb departs on purpose, `tests/dom_query.rs`
// overrides the value (SWB_VALUES). Generated; the page of most cases is `PAGE`.

use crate::common::Case;

/// The page of most cases.
const PAGE: &str = r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=a class='k1 k2'><p id=p class=k1>x</p><span id=s name=nm>y</span></div><div id=b></div><!--c--><template id=t><i>q</i></template><form id=f><input id=i1 name=nm><a id=l href=x>l</a></form><img id=im><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message + (e instanceof DOMException ? ' [' + e.code + ']' : ''); } }
function T(n) { return n && n.tagName; }
function I(l) { return Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "selectors-errors-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => document.querySelector('p['))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector(''))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': The provided selector is empty. [12]""#,
            ),
            (
                r"E(() => document.querySelector())",
                r#""TypeError: Failed to execute 'querySelector' on 'Document': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => document.querySelector('  '))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': '  ' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('a:foo'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'a:foo' is not a valid selector. [12]""#,
            ),
            (r"E(() => document.querySelector('a::before'))", r"null"),
            (
                r"E(() => document.querySelector('a:hover') + '|' + document.querySelectorAll('a:hover').length)",
                r#""null|0""#,
            ),
            (
                r"E(() => document.querySelector('1a'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': '1a' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('a,'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'a,' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('ns|a'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'ns|a' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('svg|p'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'svg|p' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('p >'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'p >' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('#'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': '#' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('.'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': '.' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector(':not()'))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': ':not()' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelector('p:nth-child('))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': 'p:nth-child(' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.createElement('div').matches('p['))",
                r#""DOMException SyntaxError: Failed to execute 'matches' on 'Element': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.createElement('div').matches(''))",
                r#""DOMException SyntaxError: Failed to execute 'matches' on 'Element': The provided selector is empty. [12]""#,
            ),
            (
                r"E(() => document.createElement('div').closest('p['))",
                r#""DOMException SyntaxError: Failed to execute 'closest' on 'Element': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.createElement('div').closest())",
                r#""TypeError: Failed to execute 'closest' on 'Element': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => document.querySelectorAll('p[').length)",
                r#""DOMException SyntaxError: Failed to execute 'querySelectorAll' on 'Document': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.querySelectorAll(''))",
                r#""DOMException SyntaxError: Failed to execute 'querySelectorAll' on 'Document': The provided selector is empty. [12]""#,
            ),
            (
                r"E(() => document.createDocumentFragment().querySelector('p['))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'DocumentFragment': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.createDocumentFragment().querySelectorAll('p['))",
                r#""DOMException SyntaxError: Failed to execute 'querySelectorAll' on 'DocumentFragment': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.createElement('div').querySelector('p['))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Element': 'p[' is not a valid selector. [12]""#,
            ),
            (
                r"E(() => document.createElement('div').webkitMatchesSelector('p['))",
                r#""DOMException SyntaxError: Failed to execute 'webkitMatchesSelector' on 'Element': 'p[' is not a valid selector. [12]""#,
            ),
            (r"E(() => document.querySelector(null))", r"null"),
            (r"E(() => document.querySelector(undefined))", r"null"),
            (
                r"E(() => document.querySelector(5))",
                r#""DOMException SyntaxError: Failed to execute 'querySelector' on 'Document': '5' is not a valid selector. [12]""#,
            ),
            (r"E(() => document.querySelector('*|p').id)", r#""p""#),
            (r"E(() => document.querySelector('p::first-line'))", r"null"),
            (
                r"E(() => document.querySelectorAll('p::first-line').length + ',' + document.querySelectorAll('p:before').length)",
                r#""0,0""#,
            ),
        ],
    },
    Case {
        name: "selectors-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => document.getElementById('a').querySelector(':scope > p').id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementById('a').querySelectorAll(':scope p').length)",
                r"1",
            ),
            (
                r"E(() => document.getElementById('a').querySelectorAll('div p').length)",
                r"1",
            ),
            (
                r"E(() => document.getElementById('a').querySelectorAll('body p').length)",
                r"1",
            ),
            (
                r"E(() => document.getElementById('a').querySelector('div'))",
                r"null",
            ),
            (
                r"E(() => document.getElementById('a').querySelector('*').id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementById('a').querySelectorAll('*').length)",
                r"2",
            ),
            (
                r"E(() => document.getElementById('a').querySelector(':scope'))",
                r"null",
            ),
            (
                r"E(() => document.querySelector(':scope').nodeName)",
                r#""HTML""#,
            ),
            (
                r"E(() => document.createDocumentFragment().querySelector(':scope'))",
                r"null",
            ),
            (r"E(() => document.querySelectorAll(':scope').length)", r"1"),
            (
                r"E(() => document.getElementById('a').matches(':scope'))",
                r"true",
            ),
            (
                r"E(() => document.getElementById('a').matches(':scope > p'))",
                r"false",
            ),
            (
                r"E(() => document.getElementById('p').matches(':scope'))",
                r"true",
            ),
            (
                r"E(() => document.getElementById('a').closest(':scope').id)",
                r#""a""#,
            ),
            (
                r"E(() => document.getElementById('p').closest(':scope').id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementById('p').closest('div').id)",
                r#""a""#,
            ),
            (
                r"E(() => document.getElementById('p').closest('p').id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementById('p').closest('#nope'))",
                r"null",
            ),
            (
                r"E(() => document.getElementById('p').closest('div, body').id)",
                r#""a""#,
            ),
            (
                r"E(() => document.getElementById('p').closest('html').nodeName)",
                r#""HTML""#,
            ),
            (
                r"E(() => document.getElementById('p').matches('div > p'))",
                r"true",
            ),
            (
                r"E(() => document.getElementById('p').matches('p, #zz'))",
                r"true",
            ),
            (
                r"E(() => document.getElementById('p').matches('span, #zz'))",
                r"false",
            ),
            (
                r"E(() => document.getElementById('p').matches('p::before'))",
                r"false",
            ),
            (
                r"E(() => document.getElementById('p').webkitMatchesSelector('div > p'))",
                r"true",
            ),
            (
                r"E(() => document.getElementById('p').matchesSelector)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementById('p').msMatchesSelector)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementById('p').mozMatchesSelector)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementById('p').oMatchesSelector)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.querySelectorAll('p') instanceof NodeList)",
                r"true",
            ),
            (
                r"E(() => Object.prototype.toString.call(document.querySelectorAll('p')))",
                r#""[object NodeList]""#,
            ),
            (
                r"E(() => document.querySelectorAll('p') === document.querySelectorAll('p'))",
                r"false",
            ),
            (
                r"E(() => document.querySelector('p') === document.getElementById('p'))",
                r"true",
            ),
            (
                r"E(() => { const f = document.createElement('div'); f.innerHTML = '<p id=q1></p><div><p id=q2></p></div>'; return [f.querySelector('div > p').id, f.querySelector('p ~ div').tagName, f.querySelectorAll('p').length, f.querySelector('p:first-child').id, f.querySelectorAll('div p, #q1').length]; })",
                r#""q1,DIV,2,q1,2""#,
            ),
            (
                r"E(() => { const f = document.createElement('div'); f.innerHTML = '<p id=q1></p><div><p id=q2></p></div>'; return I(f.querySelectorAll('div p, #q1')) })",
                r#""q1,q2""#,
            ),
            (
                r"E(() => { const f = document.createElement('div'); f.innerHTML = '<p id=q1></p><div><p id=q2></p></div>'; return I(f.querySelectorAll('#q2, #q1, p')) })",
                r#""q1,q2""#,
            ),
            (
                r"E(() => { const f = document.createElement('div'); f.innerHTML = '<p id=q1></p><div><p id=q2></p></div>'; return [f.querySelector('p').closest('div') === f, f.querySelector('p').matches('div > p'), f.matches('div'), f.querySelector('div').closest('div') === f.querySelector('div')]; })",
                r#""true,true,true,true""#,
            ),
            (
                r"E(() => { const f = document.createDocumentFragment(); f.appendChild(document.createElement('p')).id = 'qq'; return [f.querySelector('p').id, f.querySelectorAll('p').length, f.querySelector('#qq').tagName, f.querySelector('div p')]; })",
                r#""qq,1,P,""#,
            ),
            (
                r"E(() => { const f = document.createDocumentFragment(); const d = f.appendChild(document.createElement('div')); d.appendChild(document.createElement('p')).id = 'qq'; return [f.querySelector('div p').id, f.querySelector('div > p').id, f.querySelector(':root'), d.querySelector(':root'), d.matches(':root'), f.querySelectorAll('*').length]; })",
                r#""qq,qq,,,false,2""#,
            ),
        ],
    },
    Case {
        name: "selectors-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<p id=q1></p>'; return [T(d.querySelector('div p')), d.querySelector('body p'), d.querySelector(':scope p').id, d.querySelector(':root p')]; })",
                r#""P,,q1,""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); return [t.querySelector('i'), t.content.querySelector('i').tagName, document.querySelector('template i'), document.querySelectorAll('i').length]; })",
                r#"",I,,0""#,
            ),
            (
                r#"E(() => document.querySelectorAll('[name=nm]').length + ',' + document.querySelectorAll('[NAME=nm]').length + ',' + document.querySelectorAll('[name="NM"]').length + ',' + document.querySelectorAll('[name="NM" i]').length)"#,
                r#""2,2,0,2""#,
            ),
            (
                r"E(() => document.querySelectorAll('P').length + ',' + document.querySelectorAll('#A').length + ',' + document.querySelectorAll('.K1').length)",
                r#""1,0,0""#,
            ),
            (
                r"E(() => document.querySelectorAll(':is(p, span):not(.k1)').length + ',' + document.querySelectorAll('div:has(> p)').length + ',' + document.querySelectorAll(':where(p)').length + ',' + document.querySelectorAll('p:nth-child(1)').length)",
                r#""1,1,1,1""#,
            ),
            (
                r"E(() => document.querySelectorAll('div:has(p)').length)",
                r"1",
            ),
            (
                r"E(() => document.querySelector('body > *:last-child').tagName)",
                r#""SCRIPT""#,
            ),
            (
                r#"E(() => document.querySelector('a:not([href^="y"])').id)"#,
                r#""l""#,
            ),
            (
                r"E(() => I(document.querySelectorAll('p, span, form, #b, [name]')))",
                r#""p,s,b,f,i1""#,
            ),
            (
                r"E(() => I(document.querySelectorAll('input:checked, input:disabled, input:enabled, a:link, a:any-link, a:visited, p:empty, :root, :focus, :target, input:required, input:optional')))",
                r#""HTML,i1,l""#,
            ),
            (
                r"E(() => T(document.querySelector('input:read-write')) + '|' + document.querySelector('input:placeholder-shown'))",
                r#""INPUT|null""#,
            ),
            (
                r"E(() => { const l = document.getElementById('l'); return [l.matches(':link'), l.matches(':visited'), l.matches(':any-link'), l.matches(':hover'), l.matches(':active')]; })",
                r#""true,false,true,false,false""#,
            ),
            (
                r"E(() => { const i = document.createElement('input'); const r = [i.matches(':disabled'), i.matches(':enabled'), i.matches(':read-write'), i.matches(':optional')]; return r; })",
                r#""false,true,true,true""#,
            ),
            (
                r"E(() => { const i = document.createElement('input'); i.setAttribute('checked', ''); i.setAttribute('type', 'checkbox'); return [i.matches(':checked'), i.matches('[type=checkbox]:checked')]; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { const l = document.querySelectorAll('div'); return [typeof l.forEach, typeof l.entries, l.length, [...l.keys()].join(), l.item(1) === l[1], l[100], l.item(100), l['1'] === l[1], 'x' in l, typeof l[Symbol.iterator]]; })",
                r#""function,function,2,0,1,true,,,true,false,function""#,
            ),
            (
                r"E(() => { const out = []; document.querySelectorAll('div').forEach((e, i, l) => out.push(e.id + i + l.length)); return out.join(); })",
                r#""a02,b12""#,
            ),
        ],
    },
    Case {
        name: "collections-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => document.getElementsByTagName('p') instanceof HTMLCollection)",
                r"true",
            ),
            (
                r"E(() => Object.prototype.toString.call(document.getElementsByName('nm')))",
                r#""[object NodeList]""#,
            ),
            (
                r"E(() => Object.prototype.toString.call(document.scripts))",
                r#""[object HTMLCollection]""#,
            ),
            (
                r"E(() => Object.prototype.toString.call(document.forms))",
                r#""[object HTMLCollection]""#,
            ),
            (
                r"E(() => Object.prototype.toString.call(document.getElementsByClassName('k1')))",
                r#""[object HTMLCollection]""#,
            ),
            (
                r"E(() => document.getElementsByClassName('k1') === document.getElementsByClassName('k1'))",
                r"true",
            ),
            (r"E(() => document.forms === document.forms)", r"true"),
            (
                r"E(() => document.getElementsByTagName('p').constructor.name)",
                r#""HTMLCollection""#,
            ),
            (
                r"E(() => document.scripts.length + ',' + document.forms.length + ',' + document.images.length + ',' + document.links.length + ',' + document.anchors.length + ',' + document.embeds.length + ',' + document.plugins.length + ',' + document.applets.length)",
                r#""1,1,1,1,0,0,0,0""#,
            ),
            (
                r"E(() => I(document.getElementsByTagName('*')))",
                r#""HTML,HEAD,TITLE,BODY,a,p,s,b,t,f,i1,l,im,SCRIPT""#,
            ),
            (r"E(() => I(document.getElementsByTagName('P')))", r#""p""#),
            (r"E(() => I(document.getElementsByTagName('')))", r#""""#),
            (
                r"E(() => I(document.getElementsByTagName()))",
                r#""TypeError: Failed to execute 'getElementsByTagName' on 'Document': 1 argument required, but only 0 present.""#,
            ),
            (r"E(() => I(document.getElementsByTagName(null)))", r#""""#),
            (
                r"E(() => I(document.getElementsByTagNameNS('http://www.w3.org/1999/xhtml', 'p')))",
                r#""p""#,
            ),
            (
                r"E(() => I(document.getElementsByTagNameNS('*', 'P')))",
                r#""""#,
            ),
            (
                r"E(() => I(document.getElementsByTagNameNS('*', 'p')))",
                r#""p""#,
            ),
            (
                r"E(() => I(document.getElementsByTagNameNS('', 'p')))",
                r#""""#,
            ),
            (
                r"E(() => I(document.getElementsByTagNameNS(null, 'p')))",
                r#""""#,
            ),
            (
                r"E(() => document.getElementsByTagNameNS('http://www.w3.org/1999/xhtml', '*').length + ',' + document.getElementsByTagName('*').length + ',' + document.getElementsByTagNameNS('*', '*').length + ',' + document.getElementsByTagNameNS(null, '*').length)",
                r#""14,14,14,0""#,
            ),
            (
                r"E(() => document.getElementsByTagNameNS('http://www.w3.org/1999/xhtml'))",
                r#""TypeError: Failed to execute 'getElementsByTagNameNS' on 'Document': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => I(document.getElementsByClassName('k1 k2')))",
                r#""a""#,
            ),
            (
                r"E(() => I(document.getElementsByClassName('  k1   ')))",
                r#""a,p""#,
            ),
            (
                r"E(() => I(document.getElementsByClassName('k2 k1 k2')))",
                r#""a""#,
            ),
            (r"E(() => I(document.getElementsByClassName('')))", r#""""#),
            (r"E(() => I(document.getElementsByClassName(' ')))", r#""""#),
            (
                r"E(() => I(document.getElementsByClassName('K1')))",
                r#""""#,
            ),
            (
                r"E(() => I(document.getElementsByClassName('k1 nope')))",
                r#""""#,
            ),
            (
                r"E(() => I(document.getElementsByClassName(null)))",
                r#""""#,
            ),
            (r"E(() => I(document.getElementsByName('nm')))", r#""s,i1""#),
            (r"E(() => I(document.getElementsByName('')))", r#""""#),
            (r"E(() => I(document.getElementsByName('NM')))", r#""""#),
            (
                r"E(() => I(document.getElementsByName()))",
                r#""TypeError: Failed to execute 'getElementsByName' on 'Document': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => document.getElementsByName('nm').namedItem)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementById('a').getElementsByName)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p').namedItem('p').id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p').p.id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p')[0].id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p').item(0).id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p').item(5))",
                r"null",
            ),
        ],
    },
    Case {
        name: "collections-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => document.getElementsByTagName('p')[5])",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p').nope)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.getElementsByTagName('p').namedItem('nope'))",
                r"null",
            ),
            (
                r"E(() => document.getElementsByTagName('*').namedItem('nm').id)",
                r#""s""#,
            ),
            (
                r"E(() => document.getElementsByTagName('*').namedItem('i1').id)",
                r#""i1""#,
            ),
            (
                r"E(() => document.getElementsByTagName('*').nm.id)",
                r#""s""#,
            ),
            (
                r"E(() => document.getElementById('a').getElementsByTagName('*').length)",
                r"2",
            ),
            (
                r"E(() => document.getElementById('a').getElementsByTagName('div').length)",
                r"0",
            ),
            (
                r"E(() => document.getElementById('a').getElementsByTagName('span')[0].id)",
                r#""s""#,
            ),
            (
                r"E(() => document.getElementById('a').getElementsByClassName('k1')[0].id)",
                r#""p""#,
            ),
            (
                r"E(() => document.getElementById('a').getElementsByClassName('k1').length)",
                r"1",
            ),
            (
                r"E(() => document.getElementById('a').getElementsByTagNameNS('*', 'p').length)",
                r"1",
            ),
            (
                r"E(() => document.createElement('div').getElementsByTagName('*').length)",
                r"0",
            ),
            (
                r"E(() => document.createDocumentFragment().getElementsByTagName)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.createDocumentFragment().getElementsByClassName)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.createDocumentFragment().querySelectorAll('*').length)",
                r"0",
            ),
            (
                r"E(() => { const l = document.getElementsByTagName('div'); return [typeof l[Symbol.iterator], l.length, [...l].length, Array.from(l).length, Array.prototype.slice.call(l).length, typeof l.forEach, typeof l.entries, typeof l.keys]; })",
                r#""function,2,2,2,2,undefined,undefined,undefined""#,
            ),
            (
                r"E(() => { const l = document.getElementsByName('nm'); return [typeof l.forEach, typeof l.entries, l.length, 'nm' in l, l.nm, Object.keys(l).join()]; })",
                r#""function,function,2,false,,0,1""#,
            ),
            (
                r"E(() => { const l = document.getElementsByClassName('k1'); return [l.length, l.namedItem('p').id, l.p && l.p.id, Object.keys(l).join(), Object.getOwnPropertyNames(l).join(), 'p' in l, 'k1' in l]; })",
                r#""2,p,p,0,1,0,1,a,p,true,false""#,
            ),
            (
                r"E(() => { const l = document.getElementsByTagName('div'); l.expando = 1; return [Object.keys(l).join(), l.expando]; })",
                r#""0,1,expando,1""#,
            ),
            (
                r"E(() => document.getElementsByTagName('div')[Symbol.toStringTag])",
                r#""HTMLCollection""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<b>1</b><i><b>2</b></i>'; const l = e.getElementsByTagName('b'); return [l.length, e.getElementsByTagName('B').length, e.getElementsByTagName('div').length]; })",
                r#""2,2,0""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<svg><g></g><circle></circle><foreignObject><p></p></foreignObject></svg><math><mi></mi></math>'; return [d.getElementsByTagName('G').length, d.getElementsByTagName('g').length, d.getElementsByTagName('foreignObject').length, d.getElementsByTagName('foreignobject').length, d.getElementsByTagName('P').length, d.getElementsByTagName('mi').length, d.getElementsByTagNameNS('http://www.w3.org/2000/svg', 'g').length, d.getElementsByTagNameNS('http://www.w3.org/2000/svg', 'G').length]; })",
                r#""0,1,1,0,1,1,1,0""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 's:g')); d.appendChild(document.createElementNS('urn:x', 'X:y')); return [d.getElementsByTagName('s:g').length, d.getElementsByTagName('g').length, d.getElementsByTagName('X:y').length, d.getElementsByTagName('x:y').length, d.getElementsByTagNameNS('urn:x', 'y').length, d.getElementsByTagNameNS('urn:x', 'Y').length, d.getElementsByTagNameNS('*', '*').length, d.getElementsByTagNameNS('urn:x', '*').length]; })",
                r#""1,0,1,0,1,0,2,1""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = '<p class="A b">1</p><p class="a">2</p>'; return [d.getElementsByClassName('a').length, d.getElementsByClassName('A').length, d.getElementsByClassName('b A').length, d.getElementsByClassName('A\tb').length, d.getElementsByClassName('A\nb').length]; })"#,
                r#""1,1,1,1,1""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = '<p class="a  b">1</p>'; return [d.getElementsByClassName('b a').length, d.getElementsByClassName('a a').length, d.getElementsByClassName('a x').length, d.getElementsByClassName(undefined).length]; })"#,
                r#""1,1,0,0""#,
            ),
        ],
    },
    Case {
        name: "serialization-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = '<p class="a&quot;b" title=\'x&amp;y<z>\'>t &amp; &lt; &gt; &nbsp;<br><img src=a.png><input value=v></p>'; return d.innerHTML; })"#,
                r#""<p class=\"a&quot;b\" title=\"x&amp;y&lt;z&gt;\">t &amp; &lt; &gt; &nbsp;<br><img src=\"a.png\"><input value=\"v\"></p>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<pre>\nx</pre><textarea>\ny</textarea><listing>\nz</listing>'; return d.innerHTML; })",
                r#""<pre>x</pre><textarea>y</textarea><listing>z</listing>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<pre>\n\nx</pre>'; return d.innerHTML; })",
                r#""<pre>\nx</pre>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.appendChild(document.createElement('pre')).appendChild(document.createTextNode('\nx')); return d.innerHTML; })",
                r#""<pre>\nx</pre>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<style>a > b { }</style><script>if (a < b && c > d) {}</script><xmp><b></xmp><iframe><i></iframe><noembed><i></noembed><noframes><i></noframes>'; return d.innerHTML; })",
                r#""<style>a > b { }</style><script>if (a < b && c > d) {}</script><xmp><b></xmp><iframe><i></iframe><noembed><i></noembed><noframes><i></noframes>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<noscript><p>a &amp; b</p></noscript>'; return d.innerHTML + '|' + d.firstChild.childNodes.length + '|' + d.firstChild.firstChild.nodeType; })",
                r#""<noscript><p>a &amp; b</p></noscript>|1|3""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<template><p>t</p></template>'; return d.innerHTML + '|' + d.firstChild.childNodes.length + '|' + d.firstChild.content.childNodes.length; })",
                r#""<template><p>t</p></template>|0|1""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<template id=x><p>t</p></template>'; return d.firstChild.innerHTML + '|' + d.firstChild.outerHTML; })",
                r#""<p>t</p>|<template id=\"x\"><p>t</p></template>""#,
            ),
            (
                r##"E(() => { const d = document.createElement('div'); d.innerHTML = '<svg viewBox="0 0 1 1" xlink:href="#a" xml:lang=en xmlns:xlink="http://www.w3.org/1999/xlink"><foreignObject><p>x</p></foreignObject><circle r=1 /></svg>'; return d.innerHTML; })"##,
                r##""<svg viewBox=\"0 0 1 1\" xlink:href=\"#a\" xml:lang=\"en\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><foreignObject><p>x</p></foreignObject><circle r=\"1\"></circle></svg>""##,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = '<math><mi>x</mi><annotation-xml encoding="text/html"><p>y</p></annotation-xml></math>'; return d.innerHTML; })"#,
                r#""<math><mi>x</mi><annotation-xml encoding=\"text/html\"><p>y</p></annotation-xml></math>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<svg><title>t</title><style>a > b</style><script>a<b</script><desc><p>x</desc></svg>'; return d.innerHTML; })",
                r#""<svg><title>t</title><style>a &gt; b</style><script>a<b< script=\"\"><desc><p>x</p></desc></b<></script></svg>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.appendChild(document.createElementNS('http://example.com/ns', 'x:y')).setAttributeNS('http://example.com/ns', 'x:z', 'v'); return d.innerHTML; })",
                r#""<x:y x:z=\"v\"></x:y>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); const e = d.appendChild(document.createElementNS('http://www.w3.org/1999/xhtml', 'x:y')); e.setAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns:q', 'v'); e.setAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns', 'w'); e.setAttributeNS('http://www.w3.org/XML/1998/namespace', 'xml:lang', 'en'); e.setAttributeNS('http://www.w3.org/1999/xlink', 'foo:href', 'h'); e.setAttributeNS('urn:x', 'p:a', 'b'); e.setAttributeNS('urn:x', 'c', 'd'); return d.innerHTML; })",
                r#""<x:y xmlns:q=\"v\" xmlns=\"w\" xml:lang=\"en\" xlink:href=\"h\" p:a=\"b\" c=\"d\"></x:y>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 's:g')); d.appendChild(document.createElementNS('http://www.w3.org/1998/Math/MathML', 'm:mi')); d.appendChild(document.createElementNS('http://www.w3.org/1999/xhtml', 'H:P')); d.appendChild(document.createElementNS('http://www.w3.org/1999/xhtml', 'ABC')); d.appendChild(document.createElementNS(null, 'Foo')); return d.innerHTML; })",
                r#""<s:g></s:g><m:mi></m:mi><H:P></H:P><ABC></ABC><Foo></Foo>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); const e = d.appendChild(document.createElement('i')); e.setAttribute('A:B', 'x'); e.setAttributeNS(null, 'Cd', 'y'); return d.innerHTML; })",
                r#""<i a:b=\"x\" Cd=\"y\"></i>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<!--c--><?pi data?><b>x</b>'; return d.innerHTML + '|' + d.childNodes.length + '|' + d.childNodes[1].nodeType; })",
                r#""<!--c--><!--?pi data?--><b>x</b>|3|8""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.appendChild(document.createComment('a--b')); d.appendChild(document.createTextNode('<&>" ')); return d.innerHTML; })"#,
                r#""<!--a--b-->&lt;&amp;&gt;\"&nbsp;""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.setAttribute('title', '<>&" \''); const c = d.cloneNode(); return c.outerHTML; })"#,
                r#""<div title=\"&lt;&gt;&amp;&quot;&nbsp;'\"></div>""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = '<div id=\'q\' hidden class=""></div>'; return d.innerHTML; })"#,
                r#""<div id=\"q\" hidden=\"\" class=\"\"></div>""#,
            ),
            (
                r"E(() => document.documentElement.outerHTML.slice(0, 60))",
                r#""<html><head><title>t</title></head><body><div id=\"a\" class=\"""#,
            ),
            (r"E(() => document.head.innerHTML)", r#""<title>t</title>""#),
            (
                r"E(() => document.getElementById('f').outerHTML)",
                r#""<form id=\"f\"><input id=\"i1\" name=\"nm\"><a id=\"l\" href=\"x\">l</a></form>""#,
            ),
            (
                r"E(() => document.getElementById('im').outerHTML)",
                r#""<img id=\"im\">""#,
            ),
            (
                r"E(() => document.createElement('br').outerHTML + document.createElement('br').innerHTML + '|')",
                r#""<br>|""#,
            ),
            (
                r"E(() => document.createElement('div').innerHTML = null)",
                r"null",
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = 'x'; d.innerHTML = null; return d.innerHTML + '|' + d.childNodes.length; })",
                r#""|0""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = 'x'; d.innerHTML = undefined; return d.innerHTML + '|' + d.childNodes.length; })",
                r#""undefined|1""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = 'x'; d.innerHTML = 12; return d.innerHTML; })",
                r#""12""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = ''; return d.childNodes.length; })",
                r"0",
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = 'x<b>y</b>'; const b = d.lastChild; d.innerHTML = 'x<b>y</b>'; return [b === d.lastChild, d.childNodes.length]; })",
                r#""false,2""#,
            ),
            (
                r"E(() => { const t = document.createElement('table'); t.innerHTML = '<tr><td>1</td></tr>'; return t.innerHTML; })",
                r#""<tbody><tr><td>1</td></tr></tbody>""#,
            ),
            (
                r"E(() => { const t = document.createElement('tr'); t.innerHTML = '<td>1</td><td>2'; return t.innerHTML; })",
                r#""<td>1</td><td>2</td>""#,
            ),
            (
                r"E(() => { const t = document.createElement('select'); t.innerHTML = '<option>a<option>b<div>c'; return t.innerHTML; })",
                r#""<option>a</option><option>b<div>c</div></option>""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.innerHTML = '<td>1</td>'; return t.innerHTML; })",
                r#""1""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.innerHTML = '<html><head><title>a</title></head><body class=c><p>x</p></body></html>'; return t.innerHTML; })",
                r#""<title>a</title><p>x</p>""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.innerHTML = '<!DOCTYPE html><p>x</p>'; return t.innerHTML; })",
                r#""<p>x</p>""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.innerHTML = '<p>a<p>b</div><b>x'; return t.innerHTML; })",
                r#""<p>a</p><p>b<b>x</b></p>""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.innerHTML = '<form><form id=g>'; return t.innerHTML; })",
                r#""<form></form>""#,
            ),
            (
                r"E(() => { const f = document.createElement('form'); f.innerHTML = '<form id=g>'; return f.innerHTML; })",
                r#""""#,
            ),
            (
                r#"E(() => { const t = document.createElement('div'); t.innerHTML = '<script>window.ran = 1</script><img src=x onerror="window.ran2=1">'; return [typeof window.ran, t.firstChild.tagName]; })"#,
                r#""undefined,SCRIPT""#,
            ),
        ],
    },
    Case {
        name: "serialization-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<p>x</p>'; return [e.firstChild.parentNode === e, e.firstChild.ownerDocument === document, e.firstChild.isConnected]; })",
                r#""true,true,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<p>x</p>'; const p = e.firstChild; e.innerHTML = '<p>y</p>'; return [p.parentNode, p.isConnected, e.firstChild === p]; })",
                r#"",false,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = 'a<b></b>c'; const n = e.firstChild; e.innerHTML = 'a'; return [n.parentNode]; })",
                r#""""#,
            ),
        ],
    },
    Case {
        name: "parse-context-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); return e.innerHTML; })",
                r#""""#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'g'); e.innerHTML = '<circle r=1></circle><p>x</p>'; return e.innerHTML + '|' + e.firstChild.namespaceURI + '|' + e.lastChild.namespaceURI; })",
                r#""<circle r=\"1\"></circle><p>x</p>|http://www.w3.org/2000/svg|http://www.w3.org/1999/xhtml""#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'foreignObject'); e.innerHTML = '<p>x</p>'; return e.innerHTML + '|' + e.firstChild.namespaceURI; })",
                r#""<p>x</p>|http://www.w3.org/1999/xhtml""#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); e.innerHTML = '<p>x</p><b>y'; return e.innerHTML + '|' + e.firstChild.namespaceURI; })",
                r#""<p>x</p><b>y</b>|http://www.w3.org/1999/xhtml""#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://example.com', 'foo'); e.innerHTML = '<p>x</p>'; return e.innerHTML + '|' + e.firstChild.namespaceURI + '|' + e.firstChild.tagName; })",
                r#""<p>x</p>|http://www.w3.org/1999/xhtml|P""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<P ID=A>x</P>'; return e.innerHTML + '|' + e.firstChild.tagName; })",
                r#""<p id=\"A\">x</p>|P""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<x-y a=1></x-y><custom>t</custom>'; return e.innerHTML + '|' + e.firstChild.constructor.name; })",
                r#""<x-y a=\"1\"></x-y><custom>t</custom>|HTMLElement""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '&amp;&lt;&#65;&notit;&copy'; return e.innerHTML; })",
                r#""&amp;&lt;A¬it;©""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '\u0000a\ud800b'; return JSON.stringify(e.innerHTML); })",
                r#""\"a\\ud800b\"""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '\r\nx\ry'; return JSON.stringify(e.innerHTML); })",
                r#""\"\\nx\\ny\"""#,
            ),
            (
                r"E(() => { const e = document.createElement('p'); e.innerHTML = '<div>x</div>'; return e.innerHTML + '|' + e.parentNode; })",
                r#""<div>x</div>|null""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<a href=x><a href=y>'; return e.innerHTML; })",
                r#""<a href=\"x\"></a><a href=\"y\"></a>""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.innerHTML = '<table><td>x'; return e.innerHTML; })",
                r#""<table><tbody><tr><td>x</td></tr></tbody></table>""#,
            ),
            (
                r"E(() => { const e = document.createElement('body'); e.innerHTML = '<p>x'; return e.innerHTML; })",
                r#""<p>x</p>""#,
            ),
            (
                r"E(() => { const e = document.createElement('html'); e.innerHTML = '<p>x'; return e.innerHTML; })",
                r#""<head></head><body><p>x</p></body>""#,
            ),
            (
                r"E(() => { const e = document.createElement('head'); e.innerHTML = '<p>x<title>y'; return e.innerHTML; })",
                r#""<p>x<title>y</title></p>""#,
            ),
            (
                r"E(() => { const e = document.createElement('script'); e.innerHTML = '<p>x'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""<p>x|1""#,
            ),
            (
                r"E(() => { const e = document.createElement('style'); e.innerHTML = '<p>x</p>'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""<p>x</p>|1""#,
            ),
            (
                r"E(() => { const e = document.createElement('textarea'); e.innerHTML = '<p>x</p>&amp;'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""&lt;p&gt;x&lt;/p&gt;&amp;|1""#,
            ),
            (
                r"E(() => { const e = document.createElement('title'); e.innerHTML = '<p>x</p>&amp;'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""&lt;p&gt;x&lt;/p&gt;&amp;|1""#,
            ),
            (
                r"E(() => { const e = document.createElement('iframe'); e.innerHTML = '<p>x</p>'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""<p>x</p>|1""#,
            ),
            (
                r"E(() => { const e = document.createElement('noscript'); e.innerHTML = '<p>x</p>'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""<p>x</p>|1""#,
            ),
            (
                r"E(() => { const e = document.createElement('plaintext'); e.innerHTML = '<p>x</p>'; return e.innerHTML + '|' + e.childNodes.length; })",
                r#""<p>x</p>|1""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<noscript><b>x</b></noscript>'; return [d.firstChild.firstChild.nodeType, d.innerHTML]; })",
                r#""3,<noscript><b>x</b></noscript>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.appendChild(document.createElement('noscript')).appendChild(document.createTextNode('<&>')); return d.innerHTML; })",
                r#""<noscript><&></noscript>""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.appendChild(document.createElement('noscript')).appendChild(document.createElement('b')).textContent = '<&>'; return d.innerHTML; })",
                r#""<noscript><b>&lt;&amp;&gt;</b></noscript>""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.innerHTML = '<noscript><b>x</b></noscript>'; return [t.content.firstChild.firstChild.nodeType, t.innerHTML]; })",
                r#""1,<noscript><b>x</b></noscript>""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.content.appendChild(document.createElement('noscript')).appendChild(document.createTextNode('<&>')); return t.innerHTML; })",
                r#""<noscript>&lt;&amp;&gt;</noscript>""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); const n = d.createElement('div'); n.innerHTML = '<noscript><b>x</b></noscript>'; return [n.firstChild.firstChild.nodeType, n.innerHTML]; })",
                r#""1,<noscript><b>x</b></noscript>""#,
            ),
        ],
    },
    Case {
        name: "insert-adjacent-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('afterbegin', '<b>x</b>'); t.insertAdjacentHTML('BeforeEnd', '<i>y</i>'); return t.innerHTML; })",
                r#""<b>x</b><i>y</i>""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('beforebegin', '<b>x</b>'); })",
                r#""DOMException NoModificationAllowedError: Failed to execute 'insertAdjacentHTML' on 'Element': The element has no parent. [7]""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('afterend', '<b>x</b>'); })",
                r#""DOMException NoModificationAllowedError: Failed to execute 'insertAdjacentHTML' on 'Element': The element has no parent. [7]""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('middle', '<b>x</b>'); })",
                r#""DOMException SyntaxError: Failed to execute 'insertAdjacentHTML' on 'Element': The value provided ('middle') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'. [12]""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('', '<b>x</b>'); })",
                r#""DOMException SyntaxError: Failed to execute 'insertAdjacentHTML' on 'Element': The value provided ('') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'. [12]""#,
            ),
            (
                r"E(() => { const t = document.documentElement; t.insertAdjacentHTML('beforebegin', '<b>x</b>'); })",
                r#""DOMException NoModificationAllowedError: Failed to execute 'insertAdjacentHTML' on 'Element': The element has no parent. [7]""#,
            ),
            (
                r"E(() => { const t = document.documentElement; t.insertAdjacentHTML('afterend', '<b>x</b>'); })",
                r#""DOMException NoModificationAllowedError: Failed to execute 'insertAdjacentHTML' on 'Element': The element has no parent. [7]""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); const t = p.appendChild(document.createElement('span')); t.insertAdjacentHTML('beforebegin', '<b>x</b>'); t.insertAdjacentHTML('afterend', '<i>y</i>'); return p.innerHTML; })",
                r#""<b>x</b><span></span><i>y</i>""#,
            ),
            (
                r"E(() => { const f = document.createDocumentFragment(); const t = f.appendChild(document.createElement('span')); t.insertAdjacentHTML('beforebegin', '<b>x</b>'); return f.childNodes.length; })",
                r"2",
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('beforeend'); })",
                r#""TypeError: Failed to execute 'insertAdjacentHTML' on 'Element': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('beforeend', null); return t.innerHTML; })",
                r#""null""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentHTML('beforeend', ''); return t.innerHTML; })",
                r#""""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); const t = p.appendChild(document.createElement('tr')); t.insertAdjacentHTML('beforeend', '<td>1'); return p.innerHTML; })",
                r#""<tr><td>1</td></tr>""#,
            ),
            (
                r"E(() => { const p = document.createElement('table'); const t = p.appendChild(document.createElement('tbody')); const r = t.appendChild(document.createElement('tr')); r.insertAdjacentHTML('afterend', '<tr><td>2'); return p.innerHTML; })",
                r#""<tbody><tr></tr><tr><td>2</td></tr></tbody>""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.insertAdjacentHTML('afterbegin', '<b>x</b>'); return [t.childNodes.length, t.content.childNodes.length]; })",
                r#""1,0""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); const r = t.insertAdjacentElement('afterbegin', document.createElement('b')); return [r.tagName, t.innerHTML]; })",
                r#""B,<b></b>""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); return t.insertAdjacentElement('beforebegin', document.createElement('b')); })",
                r"null",
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentElement('middle', document.createElement('b')); })",
                r#""DOMException SyntaxError: Failed to execute 'insertAdjacentElement' on 'Element': The value provided ('middle') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'. [12]""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentElement('afterbegin', 'x'); })",
                r#""TypeError: Failed to execute 'insertAdjacentElement' on 'Element': parameter 2 is not of type 'Element'.""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentElement('afterbegin', document.createTextNode('x')); })",
                r#""TypeError: Failed to execute 'insertAdjacentElement' on 'Element': parameter 2 is not of type 'Element'.""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentElement('afterbegin', null); })",
                r#""TypeError: Failed to execute 'insertAdjacentElement' on 'Element': parameter 2 is not of type 'Element'.""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentElement('afterbegin'); })",
                r#""TypeError: Failed to execute 'insertAdjacentElement' on 'Element': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentElement('beforeend', t); })",
                r#""DOMException HierarchyRequestError: Failed to execute 'insertAdjacentElement' on 'Element': The new child element contains the parent. [3]""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); const t = p.appendChild(document.createElement('span')); t.insertAdjacentElement('beforebegin', document.createElement('b')); t.insertAdjacentElement('afterend', document.createElement('i')); return p.innerHTML; })",
                r#""<b></b><span></span><i></i>""#,
            ),
            (
                r"E(() => { document.documentElement.insertAdjacentElement('afterend', document.createElement('b')); })",
                r#""DOMException HierarchyRequestError: Failed to execute 'insertAdjacentElement' on 'Element': Only one element on document allowed. [3]""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentText('afterbegin', '<b>x'); t.insertAdjacentText('BEFOREEND', 'y'); return t.innerHTML; })",
                r#""&lt;b&gt;xy""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentText('beforebegin', 'x'); })",
                r#""undefined""#,
            ),
            (
                r"E(() => { const t = document.createElement('div'); t.insertAdjacentText('foo', 'x'); })",
                r#""DOMException SyntaxError: Failed to execute 'insertAdjacentText' on 'Element': The value provided ('foo') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'. [12]""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); const t = p.appendChild(document.createElement('span')); t.insertAdjacentText('beforebegin', 'a'); t.insertAdjacentText('afterend', 'b'); return p.innerHTML; })",
                r#""a<span></span>b""#,
            ),
            (
                r"E(() => { document.documentElement.insertAdjacentText('afterend', 'b'); })",
                r#""DOMException HierarchyRequestError: Failed to execute 'insertAdjacentText' on 'Element': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
            ),
        ],
    },
    Case {
        name: "outer-html-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { return Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML').set.length + ',' + Object.getOwnPropertyDescriptor(Element.prototype, 'outerHTML').get.name })",
                r#""1,get outerHTML""#,
            ),
            (
                r"E(() => document.createDocumentFragment().innerHTML)",
                r#""undefined""#,
            ),
            (r"E(() => document.innerHTML)", r#""undefined""#),
            (
                r"E(() => document.createTextNode('x').innerHTML)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.createTextNode('x').outerHTML)",
                r#""undefined""#,
            ),
            (
                r"E(() => document.createElementNS('http://www.w3.org/2000/svg', 'svg').innerHTML)",
                r#""""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); const q = p.appendChild(document.createElement('div')); q.innerHTML = '<style>b{}</style><link rel=stylesheet href=x.css><img src=y.png>'; return q.children.length; })",
                r"3",
            ),
        ],
    },
    Case {
        name: "template-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const d = document.createElement('div'); d.innerHTML = '<template><template><b>x</b></template>y</template>'; return d.innerHTML + '|' + d.firstChild.content.firstChild.content.firstChild.tagName; })",
                r#""<template><template><b>x</b></template>y</template>|B""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.innerHTML = '<td>1</td><tr><td>2</td></tr>'; return t.innerHTML + '|' + t.content.childNodes.length; })",
                r#""<td>1</td><td>2</td>|2""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.innerHTML = '<b>x</b>'; return [t.content.nodeType, t.content.firstChild.tagName, t.content.ownerDocument === document, t.content.ownerDocument.defaultView, t.content === t.content, t.childNodes.length]; })",
                r#""11,B,false,,true,0""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.innerHTML = '<b>x</b>'; return t.content.ownerDocument.contentType + t.content.ownerDocument.URL + t.content.ownerDocument.compatMode; })",
                r#""text/htmlabout:blankCSS1Compat""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.appendChild(document.createElement('i')); return [t.childNodes.length, t.content.childNodes.length]; })",
                r#""1,0""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); return [t.content.childNodes.length, t.content.firstChild.tagName, t.content.firstChild.ownerDocument === document, t.innerHTML, t.textContent, t.querySelector('i'), t.content.querySelector('i').tagName, t.content.firstChild.isConnected]; })",
                r#""1,I,false,<i>q</i>,,,I,false""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); const c = t.cloneNode(true); return [c.content.childNodes.length, c.content !== t.content, c.innerHTML]; })",
                r#""1,true,<i>q</i>""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); const c = t.cloneNode(false); return [c.content.childNodes.length, c.innerHTML]; })",
                r#""0,""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); const c = document.importNode(t.content, true); return [c.nodeType, c.firstChild.ownerDocument === document, c.childNodes.length]; })",
                r#""11,true,1""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); const c = t.content.cloneNode(true); return [c.firstChild.ownerDocument === document, c.firstChild.ownerDocument === t.content.ownerDocument]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.content.appendChild(document.createElement('b')); return t.innerHTML + '|' + t.outerHTML; })",
                r#""<b></b>|<template><b></b></template>""#,
            ),
            (
                r"E(() => { const s = document.createElementNS('http://www.w3.org/2000/svg', 'template'); return String(s.content); })",
                r#""undefined""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.content = 1; return typeof t.content; })",
                r#""object""#,
            ),
            (
                r"E(() => document.getElementById('t').constructor.name)",
                r#""HTMLTemplateElement""#,
            ),
            (
                r"E(() => document.getElementById('t') instanceof HTMLTemplateElement)",
                r"true",
            ),
            (
                r"E(() => Object.prototype.toString.call(document.getElementById('t').content))",
                r#""[object DocumentFragment]""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); const f = t.content.cloneNode(true); const d = document.createElement('div'); d.appendChild(f); return d.innerHTML; })",
                r#""<i>q</i>""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.innerHTML = '<p id=in-t2>x</p>'; return [document.getElementById('in-t2'), t.content.getElementById('in-t2').tagName, t.content.querySelector('#in-t2').tagName]; })",
                r#"",P,P""#,
            ),
            (
                r"E(() => { const c = document.getElementById('t').cloneNode(true); return c.content.firstChild.outerHTML + '|' + document.getElementById('t').cloneNode(false).content.childNodes.length; })",
                r#""<i>q</i>|0""#,
            ),
            (
                r"E(() => document.getElementById('t').content.firstChild.isConnected + '|' + (document.getElementById('t').content.firstChild.getRootNode() === document.getElementById('t').content))",
                r#""false|true""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); return [t.content.getElementById('x'), t.content.children.length, t.content.childElementCount, t.content.firstElementChild.tagName].join('|'); })",
                r#""|1|1|I""#,
            ),
            (
                r"E(() => { const t = document.getElementById('t'); return [t.textContent, t.content.textContent].join('|'); })",
                r#""|q""#,
            ),
        ],
    },
    Case {
        name: "dom-parser-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p class=a>x</p><script>window.ranp=1</script><noscript><i>n</i></noscript>', 'text/html'); return [d.nodeType, d.compatMode, d.contentType, d.URL === document.URL, d.characterSet, d.documentElement.outerHTML, d.body.querySelector('noscript').firstChild.nodeType, typeof window.ranp, d.defaultView, d.doctype]; })",
                r#""9,BackCompat,text/html,true,UTF-8,<html><head></head><body><p class=\"a\">x</p><script>window.ranp=1</script><noscript><i>n</i></noscript></body></html>,1,undefined,,""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<!DOCTYPE html><p class=A>x</p>', 'text/html'); return [d.compatMode, d.doctype.name, d.querySelectorAll('.a').length, d.querySelectorAll('.A').length, d.documentElement.outerHTML]; })",
                r#""CSS1Compat,html,0,1,<html><head></head><body><p class=\"A\">x</p></body></html>""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p class=A>x</p>', 'text/html'); return [d.compatMode, d.querySelectorAll('.a').length, d.getElementsByClassName('a').length, d.querySelectorAll('#X').length]; })",
                r#""BackCompat,1,1,0""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('', 'text/html'); return d.documentElement.outerHTML + '|' + d.compatMode; })",
                r#""<html><head></head><body></body></html>|BackCompat""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<title>x</title>', 'text/html'); return [d.title, d.head.innerHTML, d.body.innerHTML, d.documentElement.childNodes.length]; })",
                r#""x,<title>x</title>,,2""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p id=zz>x</p>', 'text/html'); return [d.getElementById('zz').tagName, document.getElementById('zz'), d.getElementById('zz').ownerDocument === d, d.getElementById('zz').isConnected, d.getElementById('zz').parentNode.tagName]; })",
                r#""P,,true,true,BODY""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p id=zz>x</p>', 'text/html'); const p = d.getElementById('zz'); const q = document.importNode(p, true); return [q.ownerDocument === document, p.ownerDocument === d, q.isConnected]; })",
                r#""true,true,false""#,
            ),
            (
                r"E(() => new DOMParser().parseFromString('<a/>', 'text/plain'))",
                r#""TypeError: Failed to execute 'parseFromString' on 'DOMParser': The provided value 'text/plain' is not a valid enum value of type SupportedType.""#,
            ),
            (
                r"E(() => new DOMParser().parseFromString('<a/>', 'TEXT/HTML'))",
                r#""TypeError: Failed to execute 'parseFromString' on 'DOMParser': The provided value 'TEXT/HTML' is not a valid enum value of type SupportedType.""#,
            ),
            (
                r"E(() => new DOMParser().parseFromString('<a/>'))",
                r#""TypeError: Failed to execute 'parseFromString' on 'DOMParser': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => new DOMParser().parseFromString())",
                r#""TypeError: Failed to execute 'parseFromString' on 'DOMParser': 2 arguments required, but only 0 present.""#,
            ),
            (
                r"E(() => new DOMParser().parseFromString(null, 'text/html').documentElement.outerHTML)",
                r#""<html><head></head><body>null</body></html>""#,
            ),
            (
                r"E(() => DOMParser())",
                r#""TypeError: Failed to construct 'DOMParser': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { const p = new DOMParser(); return [Object.prototype.toString.call(p), p instanceof DOMParser, typeof p.parseFromString, p.parseFromString.length, DOMParser.length]; })",
                r#""[object DOMParser],true,function,2,0""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<a/>', 'text/html'); return [d instanceof Document, d.body.constructor.name === 'HTMLBodyElement' || d.body.constructor.name === 'HTMLElement']; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<a/>', 'text/html'); return [d.createElement('P').tagName, d.createElement('p').ownerDocument === d]; })",
                r#""P,true""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p>a</p><p>b</p><form></form><img><a href=x></a><script></script>', 'text/html'); return [d.getElementsByTagName('p').length, d.scripts.length, d.forms.length, d.images.length, d.links.length, document.getElementsByTagName('p').length, document.forms.length, document.scripts.length]; })",
                r#""2,1,1,1,1,1,1,1""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p id=pp>a</p>', 'text/html'); return d.querySelector('p').matches(':root p') + ',' + d.querySelector('html').matches(':root') + ',' + document.querySelector('html').matches(':root') + ',' + d.querySelectorAll(':root').length + ',' + d.querySelector(':scope').nodeName; })",
                r#""true,true,true,1,HTML""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<p id=pp>a</p>', 'text/html'); const l = d.getElementsByTagName('p'); l[0].id; d.body.appendChild(d.createElement('p')); return l.length; })",
                r"2",
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<div id=dd><template><b>x</b></template></div>', 'text/html'); const t = d.querySelector('template'); return [t.content.firstChild.tagName, d.getElementById('dd').innerHTML, t.content.ownerDocument === d]; })",
                r#""B,<template><b>x</b></template>,false""#,
            ),
            (
                r"E(() => { const d = new DOMParser().parseFromString('<div id=dd><p>a</p></div>', 'text/html'); const n = d.getElementById('dd'); n.innerHTML = '<b>q</b>'; n.insertAdjacentHTML('beforeend', '<i>r</i>'); return [d.body.innerHTML, d.documentElement.outerHTML.length]; })",
                r#""<div id=\"dd\"><b>q</b><i>r</i></div>,74""#,
            ),
        ],
    },
    Case {
        name: "implementation-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => document.implementation.constructor.name)",
                r#""DOMImplementation""#,
            ),
            (
                r"E(() => document.implementation === document.implementation)",
                r"true",
            ),
            (
                r"E(() => Object.prototype.toString.call(document.implementation))",
                r#""[object DOMImplementation]""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('ttl'); return [d.title, d.documentElement.outerHTML, d.compatMode, d.contentType, d.URL, d.doctype.name, d.doctype.publicId === '', d.defaultView, d.characterSet]; })",
                r#""ttl,<html><head><title>ttl</title></head><body></body></html>,CSS1Compat,text/html,about:blank,html,true,,UTF-8""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument(); return [d.title, d.documentElement.outerHTML, d.head.childNodes.length]; })",
                r#"",<html><head></head><body></body></html>,0""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument(''); return [d.title, d.documentElement.outerHTML]; })",
                r#"",<html><head><title></title></head><body></body></html>""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument(null); return [d.title, d.documentElement.outerHTML]; })",
                r#""null,<html><head><title>null</title></head><body></body></html>""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument(undefined); return [d.title, d.documentElement.outerHTML]; })",
                r#"",<html><head></head><body></body></html>""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('<b>&'); return [d.title, d.documentElement.outerHTML]; })",
                r#""<b>&,<html><head><title>&lt;b&gt;&amp;</title></head><body></body></html>""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); const b = d.body; b.innerHTML = '<p id=hh class=A>x</p><script>window.rans=1</script>'; return [d.getElementById('hh').tagName, typeof window.rans, d.querySelectorAll('.A').length, d.querySelectorAll('.a').length]; })",
                r#""P,undefined,1,0""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); const s = d.createElement('script'); s.textContent = 'window.rant = 1'; d.body.appendChild(s); return typeof window.rant; })",
                r#""undefined""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); const s = d.createElement('img'); s.src = 'x.png'; d.body.appendChild(s); return d.images.length + ',' + document.images.length; })",
                r#""1,1""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); return d.defaultView === null && d.body.ownerDocument === d && document.adoptNode(d.body).ownerDocument === document; })",
                r"true",
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); return [d instanceof Document, d.nodeName, d.children.length, d.childNodes.length] })",
                r#""true,#document,1,2""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('x'); d.body.appendChild(d.createElement('p')).id = 'pz'; return [d.getElementById('pz').tagName, document.getElementById('pz'), d.querySelector('#pz').tagName, d.getElementsByTagName('p').length, document.getElementsByTagName('p').length]; })",
                r#""P,,P,1,1""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('html', 'pub', 'sys').nodeName + document.implementation.createDocumentType('html', 'pub', 'sys').publicId + document.implementation.createDocumentType('html', 'pub', 'sys').systemId)",
                r#""htmlpubsys""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('a b', '', ''))",
                r#""DOMException InvalidCharacterError: Failed to execute 'createDocumentType' on 'DOMImplementation': The provided doctype name ('a b') contains an invalid character. [5]""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('', '', '').name === '')",
                r"true",
            ),
            (
                r"E(() => document.implementation.createDocumentType('x'))",
                r#""TypeError: Failed to execute 'createDocumentType' on 'DOMImplementation': 3 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('p:q', '', '').name)",
                r#""p:q""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('1', '', '').name)",
                r#""1""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('a>b', '', ''))",
                r#""DOMException InvalidCharacterError: Failed to execute 'createDocumentType' on 'DOMImplementation': The provided doctype name ('a>b') contains an invalid character. [5]""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('a/b', '', ''))",
                r#""[object DocumentType]""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('a<b', '', '').name)",
                r#""a<b""#,
            ),
            (
                r#"E(() => document.implementation.createDocumentType('a"b', '', '').name)"#,
                r#""a\"b""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('é', '', '').name)",
                r#""é""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('a\0b', '', ''))",
                r#""DOMException InvalidCharacterError: Failed to execute 'createDocumentType' on 'DOMImplementation': The provided doctype name ('a\u0000b') contains an invalid character. [5]""#,
            ),
            (
                r"E(() => document.implementation.createDocumentType('a\tb', '', ''))",
                r#""DOMException InvalidCharacterError: Failed to execute 'createDocumentType' on 'DOMImplementation': The provided doctype name ('a\tb') contains an invalid character. [5]""#,
            ),
            (
                r"E(() => { const d = document.implementation.createDocumentType('html', 'p', 's'); return [d.ownerDocument === document, d.parentNode, d.nodeType]; })",
                r#""true,,10""#,
            ),
            (
                r"E(() => document.implementation.hasFeature('x', 'y') + ',' + document.implementation.hasFeature())",
                r#""true,true""#,
            ),
            (r"E(() => document.implementation.hasFeature.length)", r"0"),
        ],
    },
    Case {
        name: "merge-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].insertAdjacentHTML('afterend', 'X'); return [p.childNodes.length, p.lastChild.data]; })",
                r#""4,b""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].insertAdjacentHTML('beforebegin', 'X'); return [p.childNodes.length, p.firstChild.data]; })",
                r#""4,a""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].insertAdjacentText('beforebegin', 'X'); return [p.childNodes.length, p.firstChild.data]; })",
                r#""4,a""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a'; p.insertAdjacentHTML('afterbegin', 'X'); return [p.childNodes.length, p.firstChild.data]; })",
                r#""2,X""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a'; p.insertAdjacentText('beforeend', 'X'); return [p.childNodes.length, p.firstChild.data]; })",
                r#""2,a""#,
            ),
        ],
    },
    Case {
        name: "selectors-03",
        html: PAGE,
        queries: &[(
            r"E(() => { const i = document.createElement('input'); i.setAttribute('type', 'checkbox'); i.setAttribute('checked', ''); document.body.appendChild(i); const r = [document.querySelectorAll('input:checked').length, i.matches(':checked'), i.matches(':default')]; i.removeAttribute('checked'); r.push(i.matches(':checked'), i.matches(':default'), document.querySelectorAll('input:checked').length); i.remove(); return r; })",
            r#""1,true,true,false,false,0""#,
        )],
    },
    Case {
        name: "selectors-04",
        html: PAGE,
        queries: &[(
            r"E(() => { const i = document.createElement('input'); i.setAttribute('disabled', ''); document.body.appendChild(i); const r = [i.matches(':disabled'), i.matches(':enabled')]; i.remove(); return r; })",
            r#""true,false""#,
        )],
    },
    Case {
        name: "selectors-05",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.createElement('div'); d.innerHTML = '<fieldset disabled><legend><input id=n1></legend><input id=n2></fieldset>'; document.body.appendChild(d); return [document.getElementById('n1').matches(':disabled'), document.getElementById('n2').matches(':disabled'), document.querySelectorAll('input:disabled').length]; })",
            r#""false,true,1""#,
        )],
    },
    Case {
        name: "selectors-06",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.createElement('div'); d.innerHTML = '<p id=Kc class=Ac>'; document.body.appendChild(d); return [document.querySelectorAll('#kc').length, document.querySelectorAll('.ac').length, document.getElementsByClassName('ac').length, document.getElementById('kc')]; })",
            r#""0,0,0,""#,
        )],
    },
    Case {
        name: "selectors-07",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.querySelectorAll('div'); const n = l.length; document.body.appendChild(document.createElement('div')); return [n, l.length]; })",
            r#""2,2""#,
        )],
    },
    Case {
        name: "selectors-08",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.querySelectorAll('div'); const f = l[0]; f.remove(); return [l.length, l[0] === f, f.parentNode]; })",
            r#""2,true,""#,
        )],
    },
    Case {
        name: "selectors-09",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.querySelectorAll('u'); let n = 0; document.body.appendChild(document.createElement('u')); for (const e of l) { n++; document.body.appendChild(document.createElement('u')); if (n > 50) break; } return [n, l.length]; })",
            r#""0,0""#,
        )],
    },
    Case {
        name: "collections-03",
        html: PAGE,
        queries: &[(
            r#"E(() => { const d = document.createElement('div'); d.innerHTML = '<p name=a></p><p name="a b"></p><p name="A"></p><svg><a name=a></a></svg>'; document.body.appendChild(d); return [document.getElementsByName('a').length, document.getElementsByName('A').length, document.getElementsByName('a b').length, document.getElementsByName(null).length, document.getElementsByName('a')[0].tagName]; })"#,
            r#""1,1,1,0,P""#,
        )],
    },
    Case {
        name: "collections-04",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementsByTagName('div'); const n = l.length; document.body.appendChild(document.createElement('div')); return [n, l.length]; })",
            r#""2,3""#,
        )],
    },
    Case {
        name: "collections-05",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementsByClassName('zz'); const n = l.length; document.getElementById('b').className = 'zz'; const m = l.length; document.getElementById('b').className = ''; return [n, m, l.length]; })",
            r#""0,1,0""#,
        )],
    },
    Case {
        name: "collections-06",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementsByName('qq'); const n = l.length; document.getElementById('b').setAttribute('name', 'qq'); const m = l.length; document.getElementById('b').removeAttribute('name'); return [n, m, l.length]; })",
            r#""0,1,0""#,
        )],
    },
    Case {
        name: "collections-07",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementsByTagName('p'); const n = l.length; document.getElementById('p').remove(); return [n, l.length, l[0]]; })",
            r#""1,0,""#,
        )],
    },
    Case {
        name: "collections-08",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementsByTagName('div'); const out = []; for (let i = 0; i < l.length && i < 10; i++) { out.push(l[i].id); document.body.appendChild(document.createElement('div')); } return [out.join(), l.length]; })",
            r#""a,b,,,,,,,,,12""#,
        )],
    },
    Case {
        name: "collections-09",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementsByTagName('*'); let n = 0; for (const e of l) { n++; if (n < 5) document.body.appendChild(document.createElement('u')); } return n; })",
            r"18",
        )],
    },
    Case {
        name: "collections-10",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.getElementById('a'); const l = e.getElementsByTagName('p'); const m = e.getElementsByClassName('k1'); e.remove(); const n = [l.length, m.length]; document.body.appendChild(e); return [n.join(), l.length, m.length]; })",
            r#""1,1,1,1""#,
        )],
    },
    Case {
        name: "collections-11",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); const l = e.getElementsByTagName('i'); const n = l.length; e.innerHTML = '<i></i><b><i></i></b>'; return [n, l.length]; })",
            r#""0,2""#,
        )],
    },
    Case {
        name: "collections-12",
        html: PAGE,
        queries: &[(
            r#"E(() => { const e = document.createElement('div'); const l = e.getElementsByClassName('x'); const n = l.length; e.innerHTML = '<i class="x y"></i>'; const m = l.length; e.firstChild.className = 'y'; return [n, m, l.length]; })"#,
            r#""0,1,0""#,
        )],
    },
    Case {
        name: "collections-13",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElementNS('http://www.w3.org/2000/svg', 'script'); document.body.appendChild(s); const n = document.scripts.length; s.remove(); return [n, document.scripts.length]; })",
            r#""1,1""#,
        )],
    },
    Case {
        name: "collections-14",
        html: PAGE,
        queries: &[(
            r"E(() => { const i = document.createElement('img'); document.body.appendChild(i); const n = document.images.length; i.remove(); return [n, document.images.length]; })",
            r#""2,1""#,
        )],
    },
    Case {
        name: "collections-15",
        html: PAGE,
        queries: &[(
            r"E(() => { const i = document.createElement('a'); i.setAttribute('href', ''); document.body.appendChild(i); const n = document.links.length; i.remove(); return [n, document.links.length]; })",
            r#""2,1""#,
        )],
    },
    Case {
        name: "collections-16",
        html: PAGE,
        queries: &[(
            r"E(() => { const i = document.createElementNS('http://www.w3.org/2000/svg', 'a'); i.setAttribute('href', 'x'); document.body.appendChild(i); const n = document.links.length; i.remove(); return n; })",
            r"1",
        )],
    },
    Case {
        name: "collections-17",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<svg><a name=q></a></svg><a name=anc1>x</a><a>y</a><a href=z>w</a><area href=ar><area><embed><object></object><applet></applet>'); return [document.anchors.length, document.links.length, document.embeds.length, document.plugins.length, document.applets.length, I(document.links), I(document.anchors), document.getElementsByName('q').length]; })",
            r#""1,3,1,1,0,l,A,AREA,A,0""#,
        )],
    },
    Case {
        name: "collections-18",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<form id=ff><input name=ii></form>'); const f = document.getElementById('ff'); return [document.forms.ff === f, document.forms[1] === f, document.forms.namedItem('ff') === f, document.forms.length, document.forms.f.id]; })",
            r#""true,true,true,2,f""#,
        )],
    },
    Case {
        name: "collections-19",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.createElement('div'); d.innerHTML = '<form></form>'; const n = document.forms.length; document.body.appendChild(d); return [n, document.forms.length]; })",
            r#""1,2""#,
        )],
    },
    Case {
        name: "window-names-01",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<p id=named1 name=nn1></p><i id=named1 name=nn1></i>'); return [window.named1 instanceof HTMLCollection, window.named1.length, I(window.named1), window.nn1 === undefined]; })",
            r#""true,2,named1,named1,true""#,
        )],
    },
    Case {
        name: "window-names-02",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<img name=wnm><img id=wnm>'); return [Object.prototype.toString.call(window.wnm), window.wnm.length, I(window.wnm)]; })",
            r#""[object HTMLCollection],2,IMG,wnm""#,
        )],
    },
    Case {
        name: "window-names-03",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<img id=wnx name=wnx><div id=wnx2>'); return [window.wnx.tagName, window.wnx2.tagName, window.wnx2 === document.getElementById('wnx2')]; })",
            r#""IMG,DIV,true""#,
        )],
    },
    Case {
        name: "window-names-04",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<p id=wny><p id=wny>'); const c = window.wny; const n = c.length; document.getElementById('wny').remove(); return [n, c.length, T(window.wny)]; })",
            r#""2,1,P""#,
        )],
    },
    Case {
        name: "window-names-05",
        html: PAGE,
        queries: &[(
            r"E(() => { document.body.insertAdjacentHTML('beforeend', '<img name=wz><form name=wz></form><embed name=wz>'); return [window.wz.length, I(window.wz), window.wz.namedItem('wz') && window.wz.namedItem('wz').tagName]; })",
            r#""3,IMG,FORM,EMBED,IMG""#,
        )],
    },
    Case {
        name: "serialization-03",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); t.innerHTML = '<script>window.ran3 = 1</script>'; document.body.appendChild(t); const r = typeof window.ran3; t.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "serialization-04",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); document.body.appendChild(t); t.innerHTML = '<script>window.ran4 = 1</script>'; const r = typeof window.ran4; t.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "serialization-05",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); document.body.appendChild(t); t.innerHTML = '<script>window.ran5 = 1</script>'; const s = t.firstChild; const c = s.cloneNode(true); document.body.appendChild(c); const r = typeof window.ran5; t.remove(); c.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "serialization-06",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); document.body.appendChild(t); t.innerHTML = '<script>window.ran6 = 1</script>'; const s = document.createElement('div'); s.appendChild(t.firstChild); document.body.appendChild(s); const r = typeof window.ran6; t.remove(); s.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "serialization-07",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); document.body.appendChild(t); t.insertAdjacentHTML('beforeend', '<script>window.ran7 = 1</script>'); const r = typeof window.ran7; t.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "serialization-08",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); document.body.appendChild(t); t.outerHTML = '<script>window.ran8 = 1</script>'; return typeof window.ran8; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "outer-html-02",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = '<span id=sp></span>'; const t = p.firstChild; t.outerHTML = '<b>1</b><i>2</i>'; return p.innerHTML; })",
            r#""<b>1</b><i>2</i>""#,
        )],
    },
    Case {
        name: "outer-html-03",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); t.outerHTML = '<b>1</b>'; return t.outerHTML; })",
            r#""DOMException NoModificationAllowedError: Failed to set the 'outerHTML' property on 'Element': This element has no parent node. [7]""#,
        )],
    },
    Case {
        name: "outer-html-04",
        html: PAGE,
        queries: &[(
            r"E(() => { document.documentElement.outerHTML = '<b>1</b>'; })",
            r#""DOMException NoModificationAllowedError: Failed to set the 'outerHTML' property on 'Element': This element's parent is of type '#document', which is not an element node. [7]""#,
        )],
    },
    Case {
        name: "outer-html-05",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); const t = f.appendChild(document.createElement('span')); t.outerHTML = '<b>1</b>x'; return f.childNodes.length + f.firstChild.tagName; })",
            r#""DOMException NoModificationAllowedError: Failed to set the 'outerHTML' property on 'Element': This element's parent is of type '#document-fragment', which is not an element node. [7]""#,
        )],
    },
    Case {
        name: "outer-html-06",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = '<span></span>'; const t = p.firstChild; t.outerHTML = null; return p.innerHTML + '|' + p.childNodes.length; })",
            r#""|0""#,
        )],
    },
    Case {
        name: "outer-html-07",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = '<span></span>'; const t = p.firstChild; t.outerHTML = '<td>x</td>'; return p.innerHTML + '|' + t.parentNode; })",
            r#""x|null""#,
        )],
    },
    Case {
        name: "outer-html-08",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('table'); p.innerHTML = '<tbody><tr><td>x</td></tr></tbody>'; const t = p.querySelector('td'); t.outerHTML = '<th>y</th>'; return p.innerHTML; })",
            r#""<tbody><tr><th>y</th></tr></tbody>""#,
        )],
    },
    Case {
        name: "outer-html-09",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = '<span id=sp></span>'; const t = p.firstChild; t.outerHTML = '<b>1</b>'; return [t.tagName, t.parentNode, p.innerHTML]; })",
            r#""SPAN,,<b>1</b>""#,
        )],
    },
    Case {
        name: "outer-html-10",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = 'X'; return p.childNodes.length + p.innerHTML; })",
            r#""1aXb""#,
        )],
    },
    Case {
        name: "outer-html-11",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = '<i></i><span></span><u></u>'; p.childNodes[1].outerHTML = ''; return p.innerHTML; })",
            r#""<i></i><u></u>""#,
        )],
    },
    Case {
        name: "outer-html-12",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('div'); return t.outerHTML = 5 })",
            r#""DOMException NoModificationAllowedError: Failed to set the 'outerHTML' property on 'Element': This element has no parent node. [7]""#,
        )],
    },
    Case {
        name: "outer-html-13",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = '<span></span>'; const t = p.firstChild; const l = p.children; t.outerHTML = '<b></b>'; return [l.length, l[0].tagName]; })",
            r#""1,B""#,
        )],
    },
    Case {
        name: "template-02",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createElement('template'); t.innerHTML = '<script>window.rant2 = 1</script><p id=in-template>x</p>'; document.body.appendChild(t.content.cloneNode(true)); const r = [typeof window.rant2, document.getElementById('in-template') && document.getElementById('in-template').tagName]; return r; })",
            r#""undefined,P""#,
        )],
    },
    Case {
        name: "template-03",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild(document.getElementById('t').content))",
            r#""[object DocumentFragment]""#,
        )],
    },
    Case {
        name: "template-04",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.getElementById('t'); const a = document.getElementById('a'); a.appendChild(t.content); return [a.lastChild.tagName, t.content.childNodes.length, a.lastChild.ownerDocument === document]; })",
            r#""I,0,true""#,
        )],
    },
    Case {
        name: "template-05",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('t').content.appendChild(document.createElement('z')).nodeName)",
            r#""Z""#,
        )],
    },
    Case {
        name: "dom-parser-02",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = new DOMParser().parseFromString('<p id=zz>x</p>', 'text/html'); const p = d.getElementById('zz'); document.body.appendChild(p); const r = [p.ownerDocument === document, document.getElementById('zz') === p, d.getElementById('zz')]; p.remove(); return r; })",
            r#""true,true,""#,
        )],
    },
    Case {
        name: "dom-parser-03",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = new DOMParser().parseFromString('<script>window.ranq=1</script>', 'text/html'); const s = d.querySelector('script'); document.body.appendChild(s); const r = typeof window.ranq; s.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "dom-parser-04",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = new DOMParser().parseFromString('<script>window.ranr=1</script>', 'text/html'); const s = document.importNode(d.querySelector('script'), true); document.body.appendChild(s); const r = typeof window.ranr; s.remove(); return r; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "dom-parser-05",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = new DOMParser().parseFromString('<div><p>a</p></div>', 'text/html'); document.body.appendChild(d.body.firstChild); const r = document.body.lastChild.innerHTML; document.body.lastChild.remove(); return r; })",
            r#""<p>a</p>""#,
        )],
    },
    Case {
        name: "merge-02",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = 'X'; return [p.childNodes.length, p.firstChild.data]; })",
            r#""1,aXb""#,
        )],
    },
    Case {
        name: "merge-03",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = '<i>1</i>X'; return [p.childNodes.length, p.lastChild.data]; })",
            r#""3,Xb""#,
        )],
    },
    Case {
        name: "merge-04",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = 'X<i>1</i>'; return [p.childNodes.length, p.firstChild.data]; })",
            r#""3,aX""#,
        )],
    },
    Case {
        name: "merge-05",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = 'X<i>1</i>Y'; return [p.childNodes.length, p.firstChild.data, p.lastChild.data]; })",
            r#""3,aX,Yb""#,
        )],
    },
    Case {
        name: "merge-06",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span><i></i>'; p.childNodes[1].outerHTML = 'X'; return [p.childNodes.length, p.firstChild.data]; })",
            r#""2,aX""#,
        )],
    },
    Case {
        name: "merge-07",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; const n = p.lastChild; p.childNodes[1].outerHTML = 'X'; return [n.parentNode === p, n.data]; })",
            r#""false,b""#,
        )],
    },
    Case {
        name: "merge-08",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; const n = p.firstChild; p.childNodes[1].outerHTML = 'X'; return [n.parentNode === p, n.data]; })",
            r#""true,aXb""#,
        )],
    },
    Case {
        name: "merge-09",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = ''; return [p.childNodes.length]; })",
            r#""1""#,
        )],
    },
    Case {
        name: "merge-10",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('div'); p.innerHTML = 'a<span></span>b'; p.childNodes[1].outerHTML = '<!--c-->'; return [p.childNodes.length]; })",
            r#""3""#,
        )],
    },
];
