// The cases of `tools/probes/web-content-editable.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-content-editable.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::common::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}</style></head><body><div id=a contenteditable><p id=p>x<span id=s>y</span></p><div id=f contenteditable=false><b id=b>z</b><i id=i contenteditable=true>w</i></div></div><div id=h></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "ce-getter",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const r = []; for (const v of [null, '', 'true', 'false', 'plaintext-only', 'inherit', 'TRUE', 'False', 'PlainText-Only', 'INHERIT', 'x', ' true', 'true ', 'events', 'caret', 'plaintext-only ']) { const e = document.createElement('div'); if (v !== null) e.setAttribute('contenteditable', v); r.push(S(v) + '>' + e.contentEditable + '/' + e.isContentEditable); } return r.join(' '); })",
                r#""null>inherit/false \"\">true/true \"true\">true/true \"false\">false/false \"plaintext-only\">plaintext-only/true \"inherit\">inherit/false \"TRUE\">true/true \"False\">false/false \"PlainText-Only\">plaintext-only/true \"INHERIT\">inherit/false \"x\">inherit/false \" true\">inherit/false \"true \">inherit/false \"events\">inherit/false \"caret\">inherit/false \"plaintext-only \">inherit/false""#,
            ),
            (
                r"E(() => [document.getElementById('a').contentEditable, document.getElementById('p').contentEditable, document.getElementById('f').contentEditable, document.getElementById('i').contentEditable].join())",
                r#""true,inherit,false,true""#,
            ),
            (
                r"E(() => [document.getElementById('a').isContentEditable, document.getElementById('p').isContentEditable, document.getElementById('s').isContentEditable, document.getElementById('f').isContentEditable, document.getElementById('b').isContentEditable, document.getElementById('i').isContentEditable, document.getElementById('h').isContentEditable].join())",
                r#""true,true,true,false,false,true,false""#,
            ),
            (
                r"E(() => typeof Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'contentEditable').set + ' ' + typeof Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'isContentEditable').get + ' ' + Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'isContentEditable').set + ' ' + ('contentEditable' in document.createElement('div')) + ' ' + ('contentEditable' in document.createElementNS('http://www.w3.org/2000/svg', 'g')) + ' ' + ('isContentEditable' in Element.prototype) + ' ' + Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'contentEditable').enumerable + Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'contentEditable').configurable)",
                r#""function function undefined true false false truetrue""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); try { d.isContentEditable = true; } catch (e) { return 'threw ' + e.constructor.name; } return d.isContentEditable; })",
                r"false",
            ),
            (
                r"E(() => { const d = document.createElement('div'); d.setAttribute('contenteditable', 'true'); return [document.getElementById('a').attributes.length, d.getAttribute('contenteditable'), d.matches('[contenteditable]'), d.matches('[contenteditable=true]')]; })",
                r#""[2,\"true\",true,true]""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLElement.prototype).filter((n) => /ditable|designMode/i.test(n)).join())",
                r#""contentEditable,isContentEditable""#,
            ),
        ],
    },
    Case {
        name: "ce-setter",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const r = []; for (const v of [true, false, 'true', 'false', 'plaintext-only', 'inherit', 'TRUE', 'False', 'PlainText-Only', 'INHERIT', '', 'x', ' true', 'events', 'caret', 'plaintext-only ', 'tru\u0045', 5, null, undefined, {toString() { return 'True'; }}]) { const e = document.createElement('div'); e.setAttribute('contenteditable', 'false'); let res; try { e.contentEditable = v; res = e.contentEditable + '|' + e.getAttribute('contenteditable') + '|' + e.isContentEditable; } catch (x) { res = x.constructor.name + ':' + x.name + ':' + x.message + '|' + e.getAttribute('contenteditable') + '|' + x.code; } r.push(S(v) + '>' + res); } return r.join('\n'); })",
                r#""true>true|true|true\nfalse>false|false|false\n\"true\">true|true|true\n\"false\">false|false|false\n\"plaintext-only\">plaintext-only|plaintext-only|true\n\"inherit\">inherit|null|false\n\"TRUE\">true|true|true\n\"False\">false|false|false\n\"PlainText-Only\">plaintext-only|plaintext-only|true\n\"INHERIT\">inherit|null|false\n\"\">DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\n\"x\">DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('x') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\n\" true\">DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided (' true') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\n\"events\">DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('events') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\n\"caret\">DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('caret') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\n\"plaintext-only \">DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('plaintext-only ') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\n\"truE\">true|true|true\n5>DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('5') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\nnull>DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('null') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\nundefined>DOMException:SyntaxError:Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('undefined') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.|false|12\nTrue>true|true|true""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.contentEditable = 'inherit'; return [e.hasAttribute('contenteditable'), e.contentEditable]; })",
                r#""[false,\"inherit\"]""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); try { e.contentEditable = 'x'; } catch (x) { return [x instanceof DOMException, x.code, x.name, x.message, String(x)]; } })",
                r#""[true,12,\"SyntaxError\",\"Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('x') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.\",\"SyntaxError: Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('x') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.\"]""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.contentEditable = 'true'; return [e.outerHTML]; })",
                r#""[\"<div contenteditable=\\\"true\\\"></div>\"]""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.contentEditable = 'PLAINTEXT-ONLY'; return [e.outerHTML]; })",
                r#""[\"<div contenteditable=\\\"plaintext-only\\\"></div>\"]""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); e.contentEditable = true; return [e.outerHTML]; })",
                r#""[\"<div contenteditable=\\\"true\\\"></div>\"]""#,
            ),
        ],
    },
    Case {
        name: "ce-inherit",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); const c = w.appendChild(document.createElement('div')); const g = c.appendChild(document.createElement('span')); return [w.isContentEditable, c.isContentEditable, g.isContentEditable, c.contentEditable, g.contentEditable]; })",
                r#""[true,true,true,\"inherit\",\"inherit\"]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); const c = w.appendChild(document.createElement('div')); c.setAttribute('contenteditable', 'inherit'); const d = w.appendChild(document.createElement('div')); d.setAttribute('contenteditable', 'x'); const f = w.appendChild(document.createElement('div')); f.setAttribute('contenteditable', 'false'); const ff = f.appendChild(document.createElement('div')); return [c.isContentEditable, d.isContentEditable, f.isContentEditable, ff.isContentEditable]; })",
                r#""[true,true,false,false]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'plaintext-only'); const c = w.appendChild(document.createElement('div')); const f = c.appendChild(document.createElement('div')); f.setAttribute('contenteditable', 'true'); return [w.isContentEditable, c.isContentEditable, c.contentEditable, f.isContentEditable]; })",
                r#""[true,true,\"inherit\",true]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); const s = w.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'svg')); const g = s.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'g')); return [s.isContentEditable, g.isContentEditable, 'contentEditable' in s]; })",
                r#""[null,null,false]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); w.style.display = 'none'; document.body.appendChild(w); const c = w.appendChild(document.createElement('p')); const r = [w.isContentEditable, c.isContentEditable]; w.remove(); return r; })",
                r#""[true,true]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); const r1 = w.isContentEditable; w.setAttribute('contenteditable', 'false'); const r2 = w.isContentEditable; w.removeAttribute('contenteditable'); return [r1, r2, w.isContentEditable]; })",
                r#""[true,false,false]""#,
            ),
            (
                r"E(() => { document.body.setAttribute('contenteditable', 'true'); const r = [document.body.isContentEditable, document.getElementById('h').isContentEditable, document.documentElement.isContentEditable, document.getElementById('f').isContentEditable]; document.body.removeAttribute('contenteditable'); return r; })",
                r#""[true,true,false,false]""#,
            ),
            (
                r"E(() => { document.documentElement.setAttribute('contenteditable', 'true'); const r = [document.body.isContentEditable, document.head.isContentEditable]; document.documentElement.removeAttribute('contenteditable'); return r; })",
                r#""[true,true]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); const t = w.appendChild(document.createElement('template')); const i = w.appendChild(document.createElement('input')); const ta = w.appendChild(document.createElement('textarea')); const b = w.appendChild(document.createElement('button')); const sel = w.appendChild(document.createElement('select')); return [t.isContentEditable, i.isContentEditable, ta.isContentEditable, b.isContentEditable, sel.isContentEditable]; })",
                r#""[true,true,true,true,true]""#,
            ),
            (
                r"E(() => { const i = document.createElement('i'); i.setAttribute('contenteditable', 'true'); const f = document.createDocumentFragment(); f.appendChild(i); const d = document.implementation.createHTMLDocument('x'); const e = d.createElement('div'); e.setAttribute('contenteditable', 'true'); d.body.appendChild(e); return [i.isContentEditable, e.isContentEditable]; })",
                r#""[true,true]""#,
            ),
        ],
    },
    Case {
        name: "ce-selectors",
        html: PAGE_1,
        queries: &[
            (
                r"E(() => { const r = []; for (const v of [null, '', 'true', 'false', 'plaintext-only', 'inherit', 'x', 'TRUE']) { const e = document.createElement('div'); if (v !== null) e.setAttribute('contenteditable', v); const c = e.appendChild(document.createElement('span')); r.push(S(v) + '>' + e.matches(':read-write') + e.matches(':read-only') + ' ' + c.matches(':read-write') + c.matches(':read-only')); } return r.join(' '); })",
                r#""null>falsetrue falsetrue \"\">truefalse truefalse \"true\">truefalse truefalse \"false\">falsetrue falsetrue \"plaintext-only\">truefalse truefalse \"inherit\">falsetrue falsetrue \"x\">falsetrue falsetrue \"TRUE\">truefalse truefalse""#,
            ),
            (
                r"E(() => [...document.querySelectorAll(':read-write')].map((e) => e.id || e.localName).join())",
                r#""a,p,s,i""#,
            ),
            (
                r"E(() => [...document.querySelectorAll('body :read-only')].map((e) => e.id || e.localName).join())",
                r#""f,b,h,script""#,
            ),
            (
                r"E(() => { const r = []; for (const t of ['input', 'textarea', 'select', 'button', 'a', 'img', 'option', 'details', 'p']) { const e = document.createElement(t); const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); w.appendChild(e); r.push(t + ':' + e.matches(':read-write') + e.matches(':read-only')); } return r.join(' '); })",
                r#""input:truefalse textarea:truefalse select:truefalse button:truefalse a:truefalse img:truefalse option:truefalse details:truefalse p:truefalse""#,
            ),
            (
                r"E(() => { const r = []; for (const t of ['input', 'textarea']) { const e = document.createElement(t); e.setAttribute('contenteditable', 'true'); e.setAttribute('readonly', ''); r.push(t + ':' + e.matches(':read-write') + e.matches(':read-only')); } return r.join(' '); })",
                r#""input:falsetrue textarea:falsetrue""#,
            ),
            (
                r"E(() => { const r = []; for (const h of ['<input readonly>', '<input disabled>', '<input type=checkbox>', '<input type=hidden>', '<textarea readonly></textarea>', '<textarea disabled></textarea>', '<fieldset disabled><input></fieldset>', '<input type=text>', '<button disabled>b</button>', '<select disabled></select>']) { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); w.innerHTML = h; const e = w.firstChild; const o = document.createElement('div'); o.innerHTML = h; const f = o.firstChild; r.push(h + ' in=' + e.matches(':read-write') + e.matches(':read-only') + ' out=' + f.matches(':read-write') + f.matches(':read-only')); } return r.join('\n'); })",
                r#""<input readonly> in=falsetrue out=falsetrue\n<input disabled> in=falsetrue out=falsetrue\n<input type=checkbox> in=falsetrue out=falsetrue\n<input type=hidden> in=falsetrue out=falsetrue\n<textarea readonly></textarea> in=falsetrue out=falsetrue\n<textarea disabled></textarea> in=falsetrue out=falsetrue\n<fieldset disabled><input></fieldset> in=truefalse out=falsetrue\n<input type=text> in=truefalse out=truefalse\n<button disabled>b</button> in=truefalse out=falsetrue\n<select disabled></select> in=truefalse out=falsetrue""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); w.innerHTML = '<div contenteditable=false><input></div><input readonly><span contenteditable=false>x</span>'; return [w.querySelector('div > input').matches(':read-write'), w.querySelector('div > input').matches(':read-only'), w.querySelector('span').matches(':read-write'), w.querySelector('input[readonly]').matches(':read-write')]; })",
                r#""[true,false,false,false]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); w.innerHTML = '<p><b>x</b></p>'; document.body.appendChild(w); const r = [document.querySelectorAll('div:read-write').length, document.querySelectorAll(':not(:read-only)').length > 0, w.querySelector('b').matches(':is(:read-write)'), w.querySelector('b').matches(':not(:read-only)')]; w.remove(); return r; })",
                r#""[2,true,true,true]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); const s = w.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'svg')); const g = s.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'g')); const m = w.appendChild(document.createElementNS('http://www.w3.org/1998/Math/MathML', 'math')); return [s.matches(':read-write'), g.matches(':read-write'), m.matches(':read-write'), s.matches(':read-only')]; })",
                r#""[false,false,false,false]""#,
            ),
            (
                r"E(() => { const w = document.createElement('div'); w.setAttribute('contenteditable', 'true'); w.innerHTML = '<p>x</p>'; const p = w.firstChild; const r = []; document.body.appendChild(w); r.push(p.matches(':read-write')); w.setAttribute('contenteditable', 'false'); r.push(p.matches(':read-write')); w.remove(); return r; })",
                r#""[true,false]""#,
            ),
        ],
    },
    Case {
        name: "ce-tabindex",
        html: PAGE_1,
        queries: &[(
            r"E(() => { const e = document.getElementById('a'); e.setAttribute('contenteditable', 'false'); const r = [e.tabIndex, e.isContentEditable]; e.setAttribute('contenteditable', 'true'); return r; })",
            r#""[-1,false]""#,
        )],
    },
];
