// The cases of `tools/probes/web-input-handlers.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-input-handlers.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::Case;

/// A page of several cases.
pub(crate) const PAGE0: &str = r##"<!DOCTYPE html><html><head><title>t</title></head><body><input id=a><input id=b><button id=c>c</button><div id=d>x</div><div id=e tabindex=0>e</div><div id=f tabindex=-1>f</div><a id=g href="#">g</a><a id=h>h</a><input id=hid type=hidden><button id=dis disabled>z</button><div id=nd style="display:none"><input id=ndi></div><input id=vh style="visibility:hidden"><svg width=50 height=50><rect id=svgr tabindex=0 width=10 height=10></rect><a id=sa href="#"><text id=st y=20>t</text></a></svg><textarea id=ta></textarea><select id=sel><option>o</option></select><details id=dt><summary id=sm>s</summary>c</details><fieldset id=fs disabled><input id=fsi></fieldset><div id=ce contenteditable>c</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { if (x === null) return 'null'; if (x === undefined) return 'undefined'; if (x === window) return 'window'; if (x === document) return 'document'; if (x.nodeType === 1) return x.localName + (x.id ? '#' + x.id : ''); if (x.nodeType) return '#' + x.nodeType; return 'target'; }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
function P(o, names) { return names.map((n) => n + '=' + (typeof o[n] === 'function' ? 'fn' : String(o[n]))).join(' '); }
</script><script>
const log = [];
function rec(e) { log.push(e.type + ':' + N(e.target) + (e.relatedTarget === undefined ? '' : '<' + N(e.relatedTarget)) + '@' + N(document.activeElement)); }
for (const t of ['focus', 'blur', 'focusin', 'focusout']) { document.addEventListener(t, rec, true); }
function take() { const r = log.join(' '); log.length = 0; return r; }
function W(f) {
  const sig = {}; const rm = []; const orig = EventTarget.prototype.addEventListener;
  EventTarget.prototype.addEventListener = function (type, fn, o) {
    if (o && o.signal === sig) { const opt = {capture: !!o.capture}; orig.call(this, type, fn, opt); rm.push(() => this.removeEventListener(type, fn, opt)); } else { orig.call(this, type, fn, o); }
  };
  try { return f(sig); } finally { EventTarget.prototype.addEventListener = orig; while (rm.length) rm.pop()(); }
}
</script></body></html>"##;

/// A page of several cases.
pub(crate) const PAGE1: &str = r##"<!DOCTYPE html><html><head><title>t</title></head><body><form id=fm action="x" method=get><input id=cb type=checkbox><input id=r1 type=radio name=r><input id=r2 type=radio name=r checked><input id=txt value=v><input id=dcb type=checkbox disabled><button id=sb type=submit name=n value=bv>s</button><button id=rb type=reset>r</button><button id=pb type=button>p</button><input id=ims type=image src=x.png name=im><input id=sub type=submit value=sv><input id=rst type=reset><label id=lb for=cb>l</label><label id=lb2><input id=lbi type=checkbox>text</label><label id=lb3 for=txt>t3</label><label id=lbn for=nothing>n</label><label id=lbd for=dcb>ld</label></form><a id=ah href="#frag">a</a><a id=an>a</a><div id=d>d</div><span id=sp><b id=bb>b</b></span><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { if (x === null) return 'null'; if (x === undefined) return 'undefined'; if (x === window) return 'window'; if (x === document) return 'document'; if (x.nodeType === 1) return x.localName + (x.id ? '#' + x.id : ''); if (x.nodeType) return '#' + x.nodeType; return 'target'; }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
function P(o, names) { return names.map((n) => n + '=' + (typeof o[n] === 'function' ? 'fn' : String(o[n]))).join(' '); }
</script><script>
const log = [];
function rec(e) { log.push(e.type + ':' + N(e.target) + (e.target === e.currentTarget ? '' : '/' + N(e.currentTarget)) + (e.target && e.target.checked !== undefined ? '[' + e.target.checked + ']' : '') + (e.defaultPrevented ? '!' : '')); }
for (const t of ['click', 'input', 'change', 'submit', 'reset', 'invalid', 'focus', 'blur']) { document.addEventListener(t, rec, true); }
function take() { const r = log.join(' '); log.length = 0; return r; }
function W(f) {
  const sig = {}; const rm = []; const orig = EventTarget.prototype.addEventListener;
  EventTarget.prototype.addEventListener = function (type, fn, o) {
    if (o && o.signal === sig) { const opt = {capture: !!o.capture}; orig.call(this, type, fn, opt); rm.push(() => this.removeEventListener(type, fn, opt)); } else { orig.call(this, type, fn, o); }
  };
  try { return f(sig); } finally { EventTarget.prototype.addEventListener = orig; while (rm.length) rm.pop()(); }
}
</script></body></html>"##;

/// A page of several cases.
pub(crate) const PAGE2: &str = r#"<!DOCTYPE html><html><head><title>t</title></head><body onload="window.bodyload = (window.bodyload || 0) + 1"><div id=d onclick="log.push('attr:' + event.type + ':' + this.id + ':' + typeof event)">d</div><form id=fm name=fname action="x"><input id=i name=iname value=VV onclick="log.push([value, form === fm, typeof elements, typeof submit, this.id, name].join())"><button type=button id=bt name=btname onclick="log.push([typeof value, name, typeof reset].join())">b</button><div id=inform onclick="log.push([typeof value, typeof elements, typeof submit, typeof name].join())">x</div></form><svg id=sv width=20 height=20><rect id=sr width=10 height=10 onclick="log.push('svg:' + typeof evt + ':' + typeof event + ':' + arguments.length)"></rect></svg><p id=p onclick="syntax error here(">p</p><p id=q onclick="">q</p><p id=r onclick="return 5">r</p><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { if (x === null) return 'null'; if (x === undefined) return 'undefined'; if (x === window) return 'window'; if (x === document) return 'document'; if (x.nodeType === 1) return x.localName + (x.id ? '#' + x.id : ''); if (x.nodeType) return '#' + x.nodeType; return 'target'; }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
function P(o, names) { return names.map((n) => n + '=' + (typeof o[n] === 'function' ? 'fn' : String(o[n]))).join(' '); }
</script><script>const log = []; function take() { const r = log.join(' | '); log.length = 0; return r; } function fire(el, type) { el.dispatchEvent(new MouseEvent(type || 'click', {bubbles: true, cancelable: true})); }</script></body></html>"#;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "focus-basic",
        html: PAGE0,
        queries: &[
            (
                r"E(() => [N(document.activeElement), document.hasFocus()].join())",
                r#""body,true""#,
            ),
            (
                r"E(() => { a.focus(); return take() + ' => ' + N(document.activeElement); })",
                r#""focus:input#a<null@input#a focusin:input#a<null@input#a => input#a""#,
            ),
            (
                r"E(() => { b.focus(); return take() + ' => ' + N(document.activeElement); })",
                r#""blur:input#a<input#b@body focusout:input#a<input#b@body focus:input#b<input#a@input#b focusin:input#b<input#a@input#b => input#b""#,
            ),
            (
                r"E(() => { b.focus(); return take() + ' => ' + N(document.activeElement); })",
                r#"" => input#b""#,
            ),
            (
                r"E(() => { b.blur(); return take() + ' => ' + N(document.activeElement); })",
                r#""blur:input#b<null@body focusout:input#b<null@body => body""#,
            ),
            (
                r"E(() => { b.blur(); a.blur(); return take() + ' => ' + N(document.activeElement); })",
                r#"" => body""#,
            ),
            (
                r"E(() => { c.focus(); a.blur(); return take() + ' => ' + N(document.activeElement); })",
                r#""focus:button#c<null@button#c focusin:button#c<null@button#c => button#c""#,
            ),
            (
                r"E(() => { c.blur(); return take() + ' => ' + N(document.activeElement); })",
                r#""blur:button#c<null@body focusout:button#c<null@body => body""#,
            ),
            (
                r"E(() => ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'hid', 'dis', 'ndi', 'vh', 'svgr', 'sa', 'st', 'ta', 'sel', 'dt', 'sm', 'fsi', 'ce'].map((id) => { document.activeElement && document.activeElement.blur(); const el = document.getElementById(id); el.focus(); return id + '=' + N(document.activeElement); }).join(' '))",
                r#""a=input#a b=input#b c=button#c d=body e=div#e f=div#f g=a#g h=body hid=body dis=body ndi=body vh=body svgr=rect#svgr sa=a#sa st=body ta=textarea#ta sel=select#sel dt=body sm=summary#sm fsi=body ce=div#ce""#,
            ),
            (
                r"E(() => { document.activeElement.blur(); document.body.focus(); return N(document.activeElement); })",
                r#""body""#,
            ),
            (
                r"E(() => { document.activeElement.blur(); document.documentElement.focus(); return N(document.activeElement); })",
                r#""body""#,
            ),
            (
                r"E(() => { a.focus(); take(); const x = document.createElement('input'); x.focus(); return N(document.activeElement) + ' ' + take(); })",
                r#""input#a ""#,
            ),
            (
                r"E(() => { const A = a; a.focus(); take(); a.remove(); const r = N(document.activeElement) + ' ' + take(); document.body.insertBefore(A, document.body.firstChild); return r; })",
                r#""body blur:input#a<null@body focusout:input#a<null@body""#,
            ),
            (
                r"E(() => { a.focus(); take(); a.setAttribute('style', 'display:none'); const r = N(document.activeElement); a.removeAttribute('style'); return r; })",
                r#""input#a""#,
            ),
            (
                r"E(() => { a.focus(); take(); a.disabled = true; const r = N(document.activeElement) + ' ' + take(); a.disabled = false; return r; })",
                r#""body blur:input#a<null@body focusout:input#a<null@body""#,
            ),
            (
                r"E(() => { a.focus(); a.hidden = true; const r = N(document.activeElement); a.hidden = false; return r; })",
                r#""body""#,
            ),
        ],
    },
    Case {
        name: "focus-events",
        html: PAGE0,
        queries: &[
            (
                r"E(() => W((s) => { a.focus(); take(); const seen = []; b.addEventListener('focus', (e) => seen.push([e.constructor.name, e.bubbles, e.cancelable, e.composed, e.view === window, e.detail, e.isTrusted, e.eventPhase, N(e.relatedTarget), e.which].join()), {signal: s}); b.addEventListener('focusin', (e) => seen.push([e.constructor.name, e.bubbles, e.cancelable, e.composed, e.view === window, e.detail, e.isTrusted, N(e.relatedTarget)].join()), {signal: s}); a.addEventListener('blur', (e) => seen.push('blur ' + [e.constructor.name, e.bubbles, e.cancelable, e.composed, N(e.relatedTarget)].join()), {signal: s}); a.addEventListener('focusout', (e) => seen.push('focusout ' + [e.bubbles, N(e.relatedTarget)].join()), {signal: s}); b.focus(); return seen.join(' | '); }))",
                r#""blur FocusEvent,false,false,true,input#b | focusout true,input#b | FocusEvent,false,false,true,true,0,true,2,input#a,0 | FocusEvent,true,false,true,true,0,true,input#a""#,
            ),
            (
                r"E(() => W((s) => { const r = []; document.addEventListener('focus', (e) => r.push('doc-bubble'), {signal: s}); window.addEventListener('focus', (e) => r.push('win-bubble'), {signal: s}); window.addEventListener('focus', (e) => r.push('win-capture:' + N(e.target)), {capture: true, signal: s}); document.addEventListener('focusin', (e) => r.push('doc-focusin'), {signal: s}); c.focus(); return r.join(); }))",
                r#""win-capture:button#c,doc-focusin""#,
            ),
            (
                r"E(() => W((s) => { const r = []; a.focus(); take(); a.addEventListener('blur', () => { b.focus(); r.push('in blur: ' + N(document.activeElement)); }, {signal: s}); c.focus(); r.push(N(document.activeElement)); r.push(take()); return r.join(' | '); }))",
                r#""in blur: input#b | input#b | blur:input#a<button#c@body focus:input#b<null@input#b focusin:input#b<null@input#b focusout:input#a<null@input#b""#,
            ),
            (
                r"E(() => W((s) => { const r = []; b.focus(); take(); c.addEventListener('focus', () => { a.focus(); r.push('in focus: ' + N(document.activeElement)); }, {signal: s}); c.focus(); r.push(N(document.activeElement)); r.push(take()); return r.join(' | '); }))",
                r#""in focus: input#a | input#a | blur:input#b<button#c@body focusout:input#b<button#c@body focus:button#c<input#b@button#c blur:button#c<input#a@body focusout:button#c<input#a@body focus:input#a<button#c@input#a focusin:input#a<button#c@input#a""#,
            ),
            (
                r"E(() => W((s) => { const r = []; a.focus(); take(); b.addEventListener('focusin', () => { c.focus(); }, {signal: s}); b.focus(); r.push(N(document.activeElement)); r.push(take()); return r.join(' | '); }))",
                r#""button#c | blur:input#a<input#b@body focusout:input#a<input#b@body focus:input#b<input#a@input#b focusin:input#b<input#a@input#b blur:input#b<button#c@body focusout:input#b<button#c@body focus:button#c<input#b@button#c focusin:button#c<input#b@button#c""#,
            ),
            (
                r"E(() => W((s) => { a.focus(); take(); b.addEventListener('focus', (e) => e.preventDefault(), {signal: s}); b.focus(); return N(document.activeElement); }))",
                r#""input#b""#,
            ),
            (
                r"E(() => W((s) => { a.focus(); take(); a.addEventListener('blur', () => { throw new Error('boom'); }, {signal: s}); b.focus(); return N(document.activeElement) + ' ' + take(); }))",
                r#""input#b blur:input#a<input#b@body focusout:input#a<input#b@body focus:input#b<input#a@input#b focusin:input#b<input#a@input#b""#,
            ),
            (
                r"E(() => W((s) => { const B = b; a.focus(); take(); a.addEventListener('blur', () => { B.remove(); }, {signal: s}); B.focus(); const r = N(document.activeElement) + ' ' + take(); document.body.insertBefore(B, c); return r; }))",
                r#""body blur:input#a<input#b@body focusout:input#a<input#b@body""#,
            ),
            (
                r"E(() => W((s) => { b.focus(); take(); b.addEventListener('blur', () => { a.focus(); }, {signal: s}); a.blur(); return N(document.activeElement) + ' ' + take(); }))",
                r#""input#b ""#,
            ),
            (
                r"E(() => W((s) => { a.blur(); a.focus(); b.addEventListener('blur', () => { c.focus(); }, {signal: s}); b.focus(); take(); a.focus(); return N(document.activeElement) + ' ' + take(); }))",
                r#""button#c blur:input#b<input#a@body focus:button#c<null@button#c focusin:button#c<null@button#c focusout:input#b<null@button#c""#,
            ),
            (
                r"E(() => { a.focus(); a.focus({preventScroll: true}); a.focus({focusVisible: false}); a.focus(5); })",
                r#""TypeError: Failed to execute 'focus' on 'HTMLElement': The provided value is not of type 'FocusOptions'.""#,
            ),
            (
                r"E(() => { a.focus(null); a.focus(undefined); a.focus({}); return 1; })",
                r"1",
            ),
            (
                r"E(() => { a.focus('x'); })",
                r#""TypeError: Failed to execute 'focus' on 'HTMLElement': The provided value is not of type 'FocusOptions'.""#,
            ),
            (
                r"E(() => { a.focus({focusVisible: false}); return [a.matches(':focus'), a.matches(':focus-visible'), a.matches(':focus-within'), document.body.matches(':focus-within')].join(); })",
                r#""true,true,true,true""#,
            ),
            (
                r"E(() => { a.blur(); a.focus({focusVisible: true}); return [a.matches(':focus'), a.matches(':focus-visible')].join(); })",
                r#""true,true""#,
            ),
            (
                r"E(() => { a.blur(); a.focus(); const r = [a.matches(':focus'), a.matches(':focus-visible')].join(); a.blur(); return r; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { document.body.setAttribute('tabindex', '-1'); document.body.focus(); const r = N(document.activeElement); take(); document.body.removeAttribute('tabindex'); return r; })",
                r#""body""#,
            ),
            (
                r"E(() => { sa.focus(); const r = N(document.activeElement); sa.blur(); svgr.focus(); const q = N(document.activeElement); svgr.blur(); return r + ' ' + q + ' ' + typeof svgr.focus + ' ' + typeof st.focus; })",
                r#""a#sa rect#svgr function function""#,
            ),
            (
                r"E(() => W((s) => { ta.focus(); take(); const r = []; window.addEventListener('blur', () => r.push('win-blur'), {signal: s}); window.addEventListener('focus', () => r.push('win-focus'), {signal: s}); document.activeElement.blur(); return r.join(); }))",
                r#""""#,
            ),
            (
                r"E(() => [HTMLElement.prototype.focus.length, HTMLElement.prototype.blur.length, SVGElement.prototype.focus.length, typeof Element.prototype.focus, typeof document.focus].join())",
                r#""0,0,0,undefined,undefined""#,
            ),
            (
                r"E(() => HTMLElement.prototype.focus.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => HTMLElement.prototype.focus.call(document.createElementNS('http://www.w3.org/2000/svg', 'g')))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => document.createElement('div').hasFocus)",
                r#""undefined""#,
            ),
            (
                r"E(() => { const d2 = document.implementation.createHTMLDocument('x'); const i2 = d2.createElement('input'); d2.body.appendChild(i2); i2.focus(); return [d2.activeElement && d2.activeElement.localName, d2.hasFocus(), N(document.activeElement)].join(); })",
                r#""body,false,body""#,
            ),
            (
                r"E(() => { const d2 = new DOMParser().parseFromString('<input id=z>', 'text/html'); d2.getElementById('z').focus(); return [d2.activeElement && d2.activeElement.localName, N(document.activeElement)].join(); })",
                r#""body,body""#,
            ),
            (
                r"E(() => { a.focus(); const x = document.createElement('div'); x.tabIndex = 0; x.addEventListener('focus', () => {}); document.body.appendChild(x); x.focus(); const r = N(document.activeElement); x.remove(); return r + ' ' + N(document.activeElement); })",
                r#""div body""#,
            ),
            (
                r"E(() => { a.focus(); take(); const t = document.createElement('div'); t.id = 'tt'; t.tabIndex = 0; document.body.appendChild(t); t.focus(); take(); t.remove(); return N(document.activeElement) + ' [' + take() + ']'; })",
                r#""body [blur:div#tt<null@body focusout:div#tt<null@body]""#,
            ),
            (
                r"E(() => { a.focus(); take(); const R = a; a.setAttribute('style', 'visibility:hidden'); const r = N(document.activeElement); a.removeAttribute('style'); return r; })",
                r#""input#a""#,
            ),
            (
                r"E(() => { ta.focus(); take(); ta.disabled = true; const r = N(document.activeElement) + ' [' + take() + ']'; ta.disabled = false; return r; })",
                r#""body [blur:textarea#ta<null@body focusout:textarea#ta<null@body]""#,
            ),
            (
                r"E(() => { const x = document.createElement('input'); x.id = 'late'; document.body.appendChild(x); x.focus(); take(); x.hidden = true; x.setAttribute('style', 'display:none'); const r = N(document.activeElement); x.remove(); return r; })",
                r#""body""#,
            ),
            (
                r"E(() => document.activeElement === document.body)",
                r"true",
            ),
            (
                r"E(() => { const B = document.body; const H = B.parentNode; H.removeChild(B); const r = N(document.activeElement); H.appendChild(B); return r; })",
                r#""null""#,
            ),
        ],
    },
    Case {
        name: "click-basic",
        html: PAGE1,
        queries: &[
            (
                r"E(() => { d.click(); return take(); })",
                r#""click:div#d/document""#,
            ),
            (
                r"E(() => W((s) => { const seen = []; d.addEventListener('click', (e) => seen.push([e.constructor.name, e.isTrusted, e.bubbles, e.cancelable, e.composed, e.detail, e.view === window, e.button, e.buttons, e.which, e.clientX, e.screenX, e.pageX, e.relatedTarget, e.ctrlKey, e.eventPhase, e.pointerType, e.pointerId, e.width, e.pressure, e.isPrimary].join()), {signal: s}); d.click(); take(); return seen.join('|'); }))",
                r#""PointerEvent,false,true,true,true,0,true,0,0,1,0,0,0,,false,2,,-1,1,0,false""#,
            ),
            (
                r"E(() => { bb.click(); return take(); })",
                r#""click:b#bb/document""#,
            ),
            (
                r"E(() => { sp.click(); return take(); })",
                r#""click:span#sp/document""#,
            ),
            (
                r"E(() => W((s) => { const seen = []; d.addEventListener('click', () => { seen.push('a'); d.click(); seen.push('b'); }, {signal: s}); d.click(); take(); return seen.join(); }))",
                r#""a,b""#,
            ),
            (
                r"E(() => { const seen = []; const x = document.createElement('div'); x.onclick = () => seen.push('x'); x.click(); return seen.join(); })",
                r#""x""#,
            ),
            (r"E(() => HTMLElement.prototype.click.length)", r"0"),
            (
                r"E(() => HTMLElement.prototype.click.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => { const seen = []; const x = document.createElementNS('http://www.w3.org/2000/svg', 'rect'); x.onclick = () => seen.push('x'); return typeof x.click + ' ' + seen.join(); })",
                r#""undefined ""#,
            ),
            (
                r"E(() => { cb.click(); const r = take() + ' => ' + cb.checked; cb.click(); return r + ' | ' + take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[true] input:input#cb/document[true] change:input#cb/document[true] => true | click:input#cb/document[false] input:input#cb/document[false] change:input#cb/document[false] => false""#,
            ),
            (
                r"E(() => { const h = (e) => e.preventDefault(); cb.addEventListener('click', h); cb.click(); cb.removeEventListener('click', h); return take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[true] => false""#,
            ),
            (
                r"E(() => { const h = (e) => e.preventDefault(); cb.addEventListener('click', h); cb.indeterminate = true; cb.click(); cb.removeEventListener('click', h); const r = cb.indeterminate + ' ' + cb.checked; cb.indeterminate = false; return r; })",
                r#""true false""#,
            ),
            (
                r"E(() => { cb.indeterminate = true; cb.click(); const r = cb.indeterminate + ' ' + cb.checked; cb.checked = false; cb.indeterminate = false; take(); return r; })",
                r#""false true""#,
            ),
            (
                r"E(() => { r1.click(); const r = take() + ' => ' + r1.checked + r2.checked; r2.click(); return r + ' | ' + take() + ' => ' + r1.checked + r2.checked; })",
                r#""click:input#r1/document[true] input:input#r1/document[true] change:input#r1/document[true] => truefalse | click:input#r2/document[true] input:input#r2/document[true] change:input#r2/document[true] => falsetrue""#,
            ),
            (
                r"E(() => { const h = (e) => e.preventDefault(); r1.addEventListener('click', h); r1.click(); r1.removeEventListener('click', h); return take() + ' => ' + r1.checked + r2.checked; })",
                r#""click:input#r1/document[true] => falsetrue""#,
            ),
            (
                r"E(() => { r2.click(); return take() + ' => ' + r1.checked + r2.checked; })",
                r#""click:input#r2/document[true] => falsetrue""#,
            ),
            (
                r"E(() => { r2.checked = true; const h = (e) => e.preventDefault(); r1.addEventListener('click', h); r1.click(); r1.removeEventListener('click', h); const x = r1.checked + ' ' + r2.checked; r2.click(); take(); return x; })",
                r#""false true""#,
            ),
            (
                r"E(() => { dcb.click(); return take() + ' => ' + dcb.checked; })",
                r#"" => false""#,
            ),
            (
                r"E(() => { const seen = []; dcb.addEventListener('click', () => seen.push('direct')); dcb.dispatchEvent(new MouseEvent('click', {bubbles: true})); return seen.join() + ' ' + take() + ' => ' + dcb.checked; })",
                r#""direct click:input#dcb/document[true] input:input#dcb/document[true] change:input#dcb/document[true] => true""#,
            ),
            (
                r"E(() => { const seen = []; pb.disabled = true; pb.addEventListener('click', () => seen.push('direct')); pb.click(); pb.dispatchEvent(new MouseEvent('click', {bubbles: true})); pb.disabled = false; return seen.join() + ' ' + take(); })",
                r#""direct click:button#pb/document""#,
            ),
            (
                r"E(() => { cb.checked = false; cb.dispatchEvent(new MouseEvent('click', {bubbles: true, cancelable: true})); return take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[true] input:input#cb/document[true] change:input#cb/document[true] => true""#,
            ),
            (
                r"E(() => { cb.checked = false; cb.dispatchEvent(new Event('click', {bubbles: true, cancelable: true})); return take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[false] => false""#,
            ),
            (
                r"E(() => { cb.checked = false; cb.dispatchEvent(new PointerEvent('click', {bubbles: true, cancelable: true})); return take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[true] input:input#cb/document[true] change:input#cb/document[true] => true""#,
            ),
            (
                r"E(() => { cb.checked = false; cb.dispatchEvent(new MouseEvent('mousedown', {bubbles: true})); return take() + ' => ' + cb.checked; })",
                r#"" => false""#,
            ),
            (
                r"E(() => { cb.checked = false; cb.dispatchEvent(new MouseEvent('click', {bubbles: false})); return take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[true] input:input#cb/document[true] change:input#cb/document[true] => true""#,
            ),
            (
                r"E(() => { cb.checked = false; sp.dispatchEvent(new MouseEvent('click', {bubbles: true})); return take() + ' => ' + cb.checked; })",
                r#""click:span#sp/document => false""#,
            ),
            (
                r"E(() => { cb.checked = false; const x = document.createElement('input'); x.type = 'checkbox'; x.dispatchEvent(new MouseEvent('click', {bubbles: true})); const r = x.checked; const y = document.createElement('input'); y.type = 'checkbox'; y.click(); return r + ' ' + y.checked + ' ' + take(); })",
                r#""true true ""#,
            ),
            (
                r"E(() => { const seen = []; cb.checked = false; cb.addEventListener('click', () => seen.push('click:' + cb.checked)); cb.addEventListener('input', () => seen.push('input:' + cb.checked)); cb.addEventListener('change', () => seen.push('change:' + cb.checked)); cb.click(); take(); return seen.join(); })",
                r#""click:true,input:true,change:true""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => seen.push([e.type, e.constructor.name, e.bubbles, e.cancelable, e.composed, e.isTrusted, e.inputType, e.isComposing].join()); document.addEventListener('input', h); document.addEventListener('change', h); cb.click(); document.removeEventListener('input', h); document.removeEventListener('change', h); take(); return seen.join('|'); })",
                r#""input,Event,true,false,true,true,,|change,Event,true,false,false,true,,""#,
            ),
            (
                r"E(() => { cb.checked = false; const h = () => { cb.checked = true; }; cb.addEventListener('click', h); cb.click(); cb.removeEventListener('click', h); return take() + ' => ' + cb.checked; })",
                r#""click:input#cb/document[true] input:input#cb/document[true] change:input#cb/document[true] => true""#,
            ),
            (
                r"E(() => { cb.checked = false; const x = document.createElement('input'); x.type = 'checkbox'; const seen = []; x.addEventListener('input', () => seen.push('input')); x.addEventListener('change', () => seen.push('change')); x.click(); return seen.join() + ' ' + x.checked; })",
                r#"" true""#,
            ),
            (
                r"E(() => W((s) => { cb.checked = false; cb.addEventListener('click', () => { cb.type = 'text'; }, {signal: s}); cb.click(); const r = cb.type + ' ' + take(); cb.type = 'checkbox'; return r; }))",
                r#""text click:input#cb/document[true]""#,
            ),
            (
                r"E(() => { cb.checked = false; take(); lb.click(); return take() + ' => ' + cb.checked + ' ' + N(document.activeElement); })",
                r#""click:label#lb/document focus:input#cb/document[false] click:input#cb/document[true] input:input#cb/document[true] change:input#cb/document[true] => true input#cb""#,
            ),
            (
                r"E(() => { lbi.checked = false; take(); lb2.click(); return take() + ' => ' + lbi.checked + ' ' + N(document.activeElement); })",
                r#""click:label#lb2/document blur:input#cb/document[true] focus:input#lbi/document[false] click:input#lbi/document[true] input:input#lbi/document[true] change:input#lbi/document[true] => true input#lbi""#,
            ),
            (
                r"E(() => { take(); lb3.click(); return take() + ' => ' + N(document.activeElement); })",
                r#""click:label#lb3/document blur:input#lbi/document[true] focus:input#txt/document[false] click:input#txt/document[false] => input#txt""#,
            ),
            (
                r"E(() => { take(); lbn.click(); return take(); })",
                r#""click:label#lbn/document""#,
            ),
            (
                r"E(() => { dcb.checked = false; take(); lbd.click(); return take() + ' => ' + dcb.checked; })",
                r#""click:label#lbd/document => false""#,
            ),
            (
                r"E(() => { cb.checked = false; take(); const h = (e) => e.preventDefault(); lb.addEventListener('click', h); lb.click(); lb.removeEventListener('click', h); return take() + ' => ' + cb.checked; })",
                r#""click:label#lb/document => false""#,
            ),
            (
                r"E(() => { lbi.checked = false; take(); lbi.click(); return take() + ' => ' + lbi.checked; })",
                r#""click:input#lbi/document[true] input:input#lbi/document[true] change:input#lbi/document[true] => true""#,
            ),
            (
                r"E(() => { take(); lb3.click(); lb.click(); cb.checked = false; return N(document.activeElement); })",
                r#""input#cb""#,
            ),
        ],
    },
    Case {
        name: "forms-methods",
        html: PAGE1,
        queries: &[
            (
                r"E(() => { fm.addEventListener('submit', (e) => e.preventDefault(), {once: true}); fm.requestSubmit(); return take(); })",
                r#""submit:form#fm/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push([e.constructor.name, e.type, e.bubbles, e.cancelable, e.composed, e.isTrusted, N(e.submitter), N(e.target)].join()); }; fm.addEventListener('submit', h); fm.requestSubmit(); fm.requestSubmit(sb); fm.requestSubmit(sub); fm.requestSubmit(ims); fm.removeEventListener('submit', h); take(); return seen.join(' | '); })",
                r#""SubmitEvent,submit,true,true,false,true,null,form#fm | SubmitEvent,submit,true,true,false,true,button#sb,form#fm | SubmitEvent,submit,true,true,false,true,input#sub,form#fm | SubmitEvent,submit,true,true,false,true,input#ims,form#fm""#,
            ),
            (
                r"E(() => { const h = (e) => e.preventDefault(); fm.addEventListener('submit', h); const r = []; r.push(E(() => fm.requestSubmit(pb))); r.push(E(() => fm.requestSubmit(rb))); r.push(E(() => fm.requestSubmit(txt))); r.push(E(() => fm.requestSubmit(d))); r.push(E(() => fm.requestSubmit(null))); r.push(E(() => fm.requestSubmit(undefined))); r.push(E(() => fm.requestSubmit(5))); fm.removeEventListener('submit', h); take(); return r.join(' | '); })",
                r#""TypeError: Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not a submit button. | TypeError: Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not a submit button. | TypeError: Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not a submit button. | TypeError: Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not a submit button. | undefined | undefined | TypeError: Failed to execute 'requestSubmit' on 'HTMLFormElement': parameter 1 is not of type 'HTMLElement'.""#,
            ),
            (
                r"E(() => { const h = (e) => e.preventDefault(); fm.addEventListener('submit', h); const g = document.createElement('form'); const gb = document.createElement('button'); g.appendChild(gb); const r = E(() => fm.requestSubmit(gb)); const s2 = document.createElement('button'); s2.setAttribute('form', 'fm'); document.body.appendChild(s2); const q = E(() => fm.requestSubmit(s2)); s2.remove(); fm.removeEventListener('submit', h); take(); return r + ' | ' + q; })",
                r#""DOMException NotFoundError: Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not owned by this form element. | undefined""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push('submit'); }; fm.addEventListener('submit', h); const g = document.createElement('form'); g.addEventListener('submit', h); g.requestSubmit(); g.submit(); fm.removeEventListener('submit', h); return seen.join() + ' ' + take(); })",
                r#"" ""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push('n'); }; fm.addEventListener('submit', h); txt.required = true; txt.value = ''; fm.requestSubmit(); const r = take(); txt.required = false; txt.value = 'v'; fm.removeEventListener('submit', h); return seen.join() + ' ' + r; })",
                r#"" invalid:input#txt/document[false] focus:input#txt/document[false]""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push('n'); }; fm.addEventListener('submit', h); fm.noValidate = true; txt.required = true; txt.value = ''; fm.requestSubmit(); const r = take(); txt.required = false; txt.value = 'v'; fm.noValidate = false; fm.removeEventListener('submit', h); return seen.join() + ' ' + r; })",
                r#""n submit:form#fm/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push('n'); }; fm.addEventListener('submit', h); sb.formNoValidate = true; txt.required = true; txt.value = ''; fm.requestSubmit(sb); const r = take(); txt.required = false; txt.value = 'v'; sb.formNoValidate = false; fm.removeEventListener('submit', h); return seen.join() + ' ' + r; })",
                r#""n submit:form#fm/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push('n'); }; fm.addEventListener('submit', h); txt.required = true; txt.value = ''; txt.addEventListener('invalid', (e) => seen.push('inv:' + [e.type, e.bubbles, e.cancelable, e.constructor.name].join('/'))); fm.requestSubmit(); const r = take() + ' ' + N(document.activeElement); txt.required = false; txt.value = 'v'; fm.removeEventListener('submit', h); return seen.join() + ' ' + r; })",
                r#""inv:invalid/false/true/Event invalid:input#txt/document[false] input#txt""#,
            ),
            (
                r"E(() => { const seen = []; const h = () => { seen.push('submit'); fm.requestSubmit(); seen.push('after'); }; const g = (e) => e.preventDefault(); fm.addEventListener('submit', h); fm.addEventListener('submit', g); fm.requestSubmit(); fm.removeEventListener('submit', h); fm.removeEventListener('submit', g); return seen.join() + ' ' + take(); })",
                r#""submit,after submit:form#fm/document""#,
            ),
            (
                r"E(() => { txt.value = 'changed'; cb.checked = true; r1.checked = true; fm.reset(); return [txt.value, cb.checked, r1.checked, r2.checked, take()].join(); })",
                r#""v,false,false,true,reset:form#fm/document""#,
            ),
            (
                r"E(() => { txt.value = 'changed'; const h = (e) => e.preventDefault(); fm.addEventListener('reset', h); fm.reset(); fm.removeEventListener('reset', h); const r = txt.value + ' ' + take(); fm.reset(); return r; })",
                r#""changed reset:form#fm/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => seen.push([e.constructor.name, e.type, e.bubbles, e.cancelable, e.composed, e.isTrusted, N(e.target)].join()); fm.addEventListener('reset', h); fm.reset(); fm.removeEventListener('reset', h); take(); return seen.join(' | '); })",
                r#""Event,reset,true,true,false,true,form#fm""#,
            ),
            (
                r"E(() => { const seen = []; const h = () => { seen.push('reset'); fm.reset(); seen.push('after'); }; fm.addEventListener('reset', h); fm.reset(); fm.removeEventListener('reset', h); take(); return seen.join(); })",
                r#""reset,after""#,
            ),
            (
                r"E(() => { txt.value = 'changed'; rb.click(); const r = txt.value + ' ' + take(); txt.value = 'v'; return r; })",
                r#""v click:button#rb/document reset:form#fm/document""#,
            ),
            (
                r"E(() => { txt.value = 'changed'; rst.click(); const r = txt.value + ' ' + take(); return r; })",
                r#""v click:input#rst/document[false] reset:form#fm/document""#,
            ),
            (
                r"E(() => { txt.value = 'changed'; const h = (e) => e.preventDefault(); rb.addEventListener('click', h); rb.click(); rb.removeEventListener('click', h); return txt.value + ' ' + take(); })",
                r#""changed click:button#rb/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push(N(e.submitter) + ':' + e.type); }; fm.addEventListener('submit', h); sb.click(); sub.click(); ims.click(); pb.click(); fm.removeEventListener('submit', h); return seen.join() + ' ' + take(); })",
                r#""button#sb:submit,input#sub:submit,input#ims:submit click:button#sb/document submit:form#fm/document click:input#sub/document[false] submit:form#fm/document click:input#ims/document[false] submit:form#fm/document click:button#pb/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push(N(e.submitter) + ':' + e.type); }; fm.addEventListener('submit', h); const h2 = (e) => e.preventDefault(); sb.addEventListener('click', h2); sb.click(); sb.removeEventListener('click', h2); fm.removeEventListener('submit', h); return seen.join() + ' ' + take(); })",
                r#"" click:button#sb/document""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push(N(e.submitter) + ':' + e.type); }; fm.addEventListener('submit', h); sb.disabled = true; sb.click(); sb.disabled = false; fm.removeEventListener('submit', h); return seen.join() + ' ' + take(); })",
                r#"" ""#,
            ),
            (
                r"E(() => { const seen = []; const h = (e) => { e.preventDefault(); seen.push(N(e.submitter) + ':' + e.type); }; fm.addEventListener('submit', h); const lone = document.createElement('button'); lone.type = 'submit'; document.body.appendChild(lone); lone.click(); lone.remove(); fm.removeEventListener('submit', h); return seen.join() + ' ' + take(); })",
                r#"" click:button/document""#,
            ),
            (
                r"E(() => [HTMLFormElement.prototype.submit.length, HTMLFormElement.prototype.requestSubmit.length, HTMLFormElement.prototype.reset.length].join())",
                r#""0,0,0""#,
            ),
            (
                r"E(() => { const g = document.createElement('form'); const seen = []; g.addEventListener('reset', () => seen.push('reset')); g.reset(); return seen.join(); })",
                r#""reset""#,
            ),
        ],
    },
    Case {
        name: "handler-attrs",
        html: PAGE2,
        queries: &[
            (
                r"E(() => { fire(d); return take(); })",
                r#""attr:click:d:object""#,
            ),
            (
                r"E(() => [typeof d.onclick, d.onclick === d.onclick, d.onclick.name, d.onclick.length, d.hasOwnProperty('onclick'), 'onclick' in d, d.getAttribute('onclick').length].join())",
                r#""function,true,onclick,1,false,true,67""#,
            ),
            (
                r"E(() => d.onclick.toString())",
                r#""function onclick(event) {\nlog.push('attr:' + event.type + ':' + this.id + ':' + typeof event)\n}""#,
            ),
            (
                r"E(() => i.onclick.toString())",
                r#""function onclick(event) {\nlog.push([value, form === fm, typeof elements, typeof submit, this.id, name].join())\n}""#,
            ),
            (
                r"E(() => sr.onclick.toString())",
                r#""function onclick(evt) {\nlog.push('svg:' + typeof evt + ':' + typeof event + ':' + arguments.length)\n}""#,
            ),
            (
                r"E(() => [typeof i.onclick, typeof q.onclick, q.onclick.toString(), typeof r.onclick].join('|'))",
                r#""function|function|function onclick() { [native code] }|function""#,
            ),
            (
                r"E(() => { fire(i); return take(); })",
                r#""VV,true,object,function,i,iname""#,
            ),
            (
                r"E(() => { fire(bt); return take(); })",
                r#""string,btname,function""#,
            ),
            (
                r"E(() => { fire(inform); return take(); })",
                r#""undefined,undefined,undefined,string""#,
            ),
            (
                r"E(() => { fire(sr); return take(); })",
                r#""svg:object:object:1""#,
            ),
            (
                r"E(() => { sr.dispatchEvent(new Event('click')); return take(); })",
                r#""svg:object:object:1""#,
            ),
            (
                r"E(() => { const r = [typeof p.onclick]; fire(p); r.push(take()); r.push(typeof p.onclick); return r.join(); })",
                r#""object,,object""#,
            ),
            (
                r"E(() => { fire(r); return take() + ' ' + (r.dispatchEvent(new MouseEvent('click', {cancelable: true}))); })",
                r#"" true""#,
            ),
            (
                r"E(() => { const f = () => log.push('prop'); d.onclick = f; const r = [d.onclick === f, d.getAttribute('onclick') !== null]; fire(d); r.push(take()); return r.join(); })",
                r#""true,true,prop""#,
            ),
            (
                r#"E(() => { d.removeAttribute('onclick'); const r = [d.onclick]; fire(d); r.push(take()); d.setAttribute('onclick', 'log.push("set")'); r.push(typeof d.onclick); fire(d); r.push(take()); return r.join(); })"#,
                r#"",,function,set""#,
            ),
            (
                r#"E(() => { d.onclick = () => log.push('prop'); d.setAttribute('onclick', 'log.push("again")'); fire(d); return take() + ' ' + d.onclick.name; })"#,
                r#""again onclick""#,
            ),
            (
                r"E(() => { const r = []; for (const v of [{}, 5, 'str', null, undefined, true, [], Symbol.iterator, class {}, Math.max, function () {}, () => {}, async () => {}, new Proxy(function () {}, {}), {handleEvent() {}}]) { d.onclick = v; r.push(typeof d.onclick + (d.onclick === v ? '=' : '')); } return r.join(); })",
                r#""object=,object,object,object=,object,object,object=,object,function=,function=,function=,function=,function=,function=,object=""#,
            ),
            (
                r"E(() => { d.onclick = null; fire(d); const r = take(); d.onclick = 'x'; return r + typeof d.onclick; })",
                r#""object""#,
            ),
            (
                r"E(() => { d.onclick = () => log.push('A'); d.addEventListener('click', () => log.push('L1')); d.onclick = () => log.push('B'); d.addEventListener('click', () => log.push('L2')); fire(d); return take(); })",
                r#""B | L1 | L2""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.addEventListener('click', () => log.push('L0')); x.onclick = () => log.push('H'); x.addEventListener('click', () => log.push('L1')); x.onclick = null; x.onclick = () => log.push('H2'); fire(x); return take(); })",
                r#""L0 | L1 | H2""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push("attr")'); x.addEventListener('click', () => log.push('L1')); fire(x); return take(); })"#,
                r#""attr | L1""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.addEventListener('click', () => log.push('L1')); x.setAttribute('onclick', 'log.push("attr")'); x.addEventListener('click', () => log.push('L2')); fire(x); return take(); })"#,
                r#""L1 | attr | L2""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.innerHTML = '<p onclick="log.push(1)" onmouseover="log.push(2)" ONFOCUS="log.push(3)" onclick2="x">p</p>'; const p2 = x.firstChild; return [typeof p2.onclick, typeof p2.onmouseover, typeof p2.onfocus, typeof p2.onclick2].join(); })"#,
                r#""function,function,function,undefined""#,
            ),
            (
                r"E(() => { const x = d.cloneNode(true); fire(x); const r = [typeof x.onclick, take()]; return r.join(); })",
                r#""function,again""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push("v1")'); const f1 = x.onclick; x.setAttribute('onclick', 'log.push("v2")'); const f2 = x.onclick; fire(x); return [f1 === f2, take()].join(); })"#,
                r#""false,v2""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push("v1")'); x.setAttribute('onclick', 'log.push("v2")'); fire(x); return take(); })"#,
                r#""v2""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(this.id)'); x.setAttribute('id', 'xx'); fire(x); return take(); })",
                r#""xx""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(window.event === event, event.currentTarget === this)'); fire(x); return take(); })",
                r#""true | true""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(typeof arguments, arguments.length, typeof onclick, this.onclick === onclick)'); fire(x); return take(); })",
                r#""object | 1 | function | true""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', '"use strict"; log.push(this === undefined, typeof arguments)'); fire(x); return take(); })"#,
                r#""false | object""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'return false'); return x.dispatchEvent(new MouseEvent('click', {cancelable: true})); })",
                r"false",
            ),
            (
                r#"E(() => { const rs = []; for (const body of ['return false', 'return true', 'return 0', 'return null', 'return undefined', 'return ""', 'return NaN', 'return new Boolean(false)', 'return 1', '']) { const x = document.createElement('div'); x.setAttribute('onclick', body); rs.push(x.dispatchEvent(new MouseEvent('click', {cancelable: true}))); } return rs.join(); })"#,
                r#""false,true,true,true,true,true,true,true,true,true""#,
            ),
            (
                r"E(() => { const rs = []; for (const f of [() => false, () => true, () => 0, () => null, () => undefined, () => '', () => new Boolean(false)]) { const x = document.createElement('div'); x.onclick = f; rs.push(x.dispatchEvent(new MouseEvent('click', {cancelable: true}))); } return rs.join(); })",
                r#""false,true,true,true,true,true,true""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.onclick = () => false; return x.dispatchEvent(new MouseEvent('click', {cancelable: false})); })",
                r"true",
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.addEventListener('click', (e) => e.preventDefault()); x.onclick = () => true; return x.dispatchEvent(new MouseEvent('click', {cancelable: true})); })",
                r"false",
            ),
            (
                r"E(() => { const x = document.createElement('div'); const seen = []; x.onclick = function () { seen.push(this === x, arguments.length, window.event === arguments[0]); }; fire(x); return seen.join(); })",
                r#""true,1,true""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); const seen = []; x.onclick = function () { throw new Error('boom'); }; x.addEventListener('click', () => seen.push('L')); const errs = []; const eh = (e) => errs.push(e.message); window.addEventListener('error', eh); fire(x); window.removeEventListener('error', eh); return seen.join() + ' ' + errs.join(); })",
                r#""L Uncaught Error: boom""#,
            ),
            (
                r#"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'throw new Error("attrboom")'); const errs = []; const eh = (e) => errs.push([e.message, e.lineno, e.colno, e.error && e.error.message].join('/')); window.addEventListener('error', eh); fire(x); window.removeEventListener('error', eh); return errs.join(); })"#,
                r#""Uncaught Error: attrboom/1/7/attrboom""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'syntax error here('); const errs = []; const eh = (e) => errs.push([e.message, e.error && e.error.name, e.lineno, e.colno].join('/')); window.addEventListener('error', eh); const f = x.onclick; window.removeEventListener('error', eh); return errs.join() + ' ' + f; })",
                r#""Uncaught SyntaxError: Failed to read the 'onclick' property from 'HTMLElement': Unexpected identifier 'error'/SyntaxError/1/8 null""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'var 1x = 2'); const errs = []; const eh = (e) => errs.push([e.message, e.lineno, e.colno].join('/')); window.addEventListener('error', eh); fire(x); fire(x); window.removeEventListener('error', eh); return errs.join() + ' ' + typeof x.onclick; })",
                r#""Uncaught SyntaxError: Failed to execute 'dispatchEvent' on 'EventTarget': Invalid or unexpected token/1/5 object""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'return'); const f = x.onclick; return typeof f + ' ' + (f ? f.toString() : ''); })",
                r#""function function onclick(event) {\nreturn\n}""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', '}), (function () { log.push(1)'); const errs = []; const eh = (e) => errs.push(e.message); window.addEventListener('error', eh); const f = x.onclick; window.removeEventListener('error', eh); return errs.join() + ' ' + typeof f; })",
                r#""Uncaught SyntaxError: Failed to read the 'onclick' property from 'HTMLElement': Unexpected token '}' object""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(typeof zzz); var zzz = 1'); fire(x); return take(); })",
                r#""undefined""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'var zq = 5'); fire(x); return typeof zq; })",
                r#""undefined""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(typeof fname, typeof iname, typeof fm)'); fire(x); return take(); })",
                r#""object | undefined | object""#,
            ),
            (
                r"E(() => { i.setAttribute('onclick', 'log.push(typeof createElement, typeof getElementById, typeof body, typeof title)'); fire(i); return take(); })",
                r#""function | function | object | string""#,
            ),
            (
                r"E(() => { fm.setAttribute('onclick', 'log.push([typeof submit, typeof elements, typeof action, typeof getElementById].join())'); fire(fm); return take(); })",
                r#""function,object,string,function""#,
            ),
            (
                r"E(() => { document.body.setAttribute('onclick', 'log.push([typeof getElementById, typeof body].join())'); fire(document.body); document.body.removeAttribute('onclick'); return take(); })",
                r#""function,object""#,
            ),
            (
                r"E(() => { i.value = 'NEW'; i.setAttribute('onclick', 'log.push(value, name)'); fire(i); return take(); })",
                r#""NEW | iname | function,object,string,function""#,
            ),
            (
                r"E(() => { const o = document.createElement('input'); o.value = 'ORPHAN'; o.setAttribute('onclick', 'log.push(value, typeof form)'); fire(o); return take(); })",
                r#""ORPHAN | object""#,
            ),
            (
                r"E(() => { const o = document.createElement('input'); const g = document.createElement('form'); g.name = 'gg'; g.appendChild(o); o.setAttribute('onclick', 'log.push(typeof elements, name)'); fire(o); return take(); })",
                r#""object | ""#,
            ),
            (
                r"E(() => { const o = document.createElement('div'); o.setAttribute('onclick', 'log.push(typeof elements)'); fm.appendChild(o); fire(o); return take(); })",
                r#""undefined | function,object,string,function""#,
            ),
            (
                r"E(() => { const o = document.createElement('input'); o.setAttribute('form', 'fm'); o.setAttribute('onclick', 'log.push(typeof elements)'); document.body.appendChild(o); fire(o); o.remove(); return take(); })",
                r#""object""#,
            ),
            (
                r"E(() => { const r = document.createElement('div'); r.id = 'sh'; r.setAttribute('onclick', 'log.push(sh === this)'); document.body.appendChild(r); fire(r); r.remove(); return take(); })",
                r#""true""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(typeof children, typeof id, typeof className, typeof title)'); fire(x); return take(); })",
                r#""object | string | string | string""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onclick', 'log.push(onclick === undefined, typeof onclick)'); fire(x); return take(); })",
                r#""false | function""#,
            ),
        ],
    },
    Case {
        name: "handler-order",
        html: r#"<!DOCTYPE html><html><head><title>t</title><script>window.order = []; window.addEventListener("load", () => order.push("head-listener")); window.onerror = (m) => { order.push("onerror"); return true; }; document.addEventListener("DOMContentLoaded", () => order.push("dcl"));</script></head><body onload="order.push('body-attr')"><script>window.addEventListener("load", () => order.push("body-listener")); order.push("body-onload-is-" + typeof window.onload);</script><div id=d onclick="order.push('attr')"></div><script>d.addEventListener("click", () => order.push("l1")); d.onclick = () => order.push("prop"); d.addEventListener("click", () => order.push("l2"));</script><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(x) { if (x === null) return 'null'; if (x === undefined) return 'undefined'; if (x === window) return 'window'; if (x === document) return 'document'; if (x.nodeType === 1) return x.localName + (x.id ? '#' + x.id : ''); if (x.nodeType) return '#' + x.nodeType; return 'target'; }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
function P(o, names) { return names.map((n) => n + '=' + (typeof o[n] === 'function' ? 'fn' : String(o[n]))).join(' '); }
</script></body></html>"#,
        queries: &[
            (
                r"window.order.join()",
                r#""body-onload-is-function,dcl,head-listener,body-attr,body-listener""#,
            ),
            (
                r"(() => { window.order.length = 0; d.dispatchEvent(new MouseEvent('click')); return window.order.join(); })()",
                r#""prop,l1,l2""#,
            ),
            (
                r"(() => { window.order.length = 0; window.dispatchEvent(new Event('load')); return window.order.join(); })()",
                r#""head-listener,body-attr,body-listener""#,
            ),
            (
                r#"(() => { window.order.length = 0; document.body.setAttribute('onload', 'order.push("second")'); window.dispatchEvent(new Event('load')); return window.order.join(); })()"#,
                r#""head-listener,second,body-listener""#,
            ),
            (
                r"(() => { window.order.length = 0; window.onload = () => order.push('prop-load'); window.dispatchEvent(new Event('load')); return window.order.join(); })()",
                r#""head-listener,prop-load,body-listener""#,
            ),
        ],
    },
    Case {
        name: "handler-window",
        html: PAGE2,
        queries: &[
            (
                r"E(() => [typeof window.onload, typeof document.body.onload, window.onload === document.body.onload, window.bodyload].join())",
                r#""function,function,true,1""#,
            ),
            (
                r"E(() => window.onload && window.onload.toString())",
                r#""function onload(event) {\nwindow.bodyload = (window.bodyload || 0) + 1\n}""#,
            ),
            (
                r"E(() => { const f = () => 1; document.body.onload = f; const r = [window.onload === f, document.body.getAttribute('onload') !== null].join(); return r; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { const f = () => 2; window.onload = f; return document.body.onload === f; })",
                r"true",
            ),
            (
                r"E(() => { document.body.removeAttribute('onload'); return String(window.onload === null) + typeof window.onload; })",
                r#""trueobject""#,
            ),
            (
                r"E(() => { document.body.setAttribute('onload', 'window.zz = 1'); return typeof window.onload + ' ' + window.onload.name; })",
                r#""function onload""#,
            ),
            (
                r"E(() => { const r = []; for (const n of ['onblur', 'onerror', 'onfocus', 'onload', 'onresize', 'onscroll', 'onhashchange', 'onmessage', 'onpagehide', 'onpageshow', 'onunload', 'onbeforeunload', 'onclick', 'onkeydown', 'onunhandledrejection', 'onrejectionhandled', 'onpopstate', 'onstorage']) { const f = function () {}; document.body[n] = f; r.push(n + ':' + (window[n] === f) + (document.documentElement[n] === f ? '!' : '')); document.body[n] = null; window[n] = null; } return r.join(' '); })",
                r#""onblur:true onerror:true onfocus:true onload:true onresize:true onscroll:true onhashchange:true onmessage:true onpagehide:true onpageshow:true onunload:true onbeforeunload:true onclick:false onkeydown:false onunhandledrejection:true onrejectionhandled:true onpopstate:true onstorage:true""#,
            ),
            (
                r"E(() => { const fs = document.createElement('frameset'); fs.onload = () => 1; return typeof window.onload; })",
                r#""function""#,
            ),
            (
                r"E(() => { const args = []; window.onerror = function () { args.push(arguments.length, ...[].map.call(arguments, (a) => typeof a)); return true; }; window.dispatchEvent(new ErrorEvent('error', {message: 'm', filename: 'f', lineno: 3, colno: 4, error: 5})); const r = args.join(); window.onerror = null; return r; })",
                r#""5,string,string,number,number,number""#,
            ),
            (
                r"E(() => { const args = []; window.onerror = function (m, f, l, c, e) { args.push(m, f, l, c, e); return false; }; window.dispatchEvent(new ErrorEvent('error', {message: 'm', filename: 'f', lineno: 3, colno: 4, error: 5})); window.onerror = null; return args.join(); })",
                r#""m,f,3,4,5""#,
            ),
            (
                r"E(() => { const rs = []; for (const f of [() => true, () => false, () => undefined, () => 1, () => 'x']) { window.onerror = f; rs.push(window.dispatchEvent(new ErrorEvent('error', {cancelable: true}))); } window.onerror = null; return rs.join(); })",
                r#""false,true,true,true,true""#,
            ),
            (
                r"E(() => { window.onerror = () => true; const r = window.dispatchEvent(new Event('error', {cancelable: true})); window.onerror = () => { return true; }; const q = window.dispatchEvent(new CustomEvent('error', {cancelable: true})); window.onerror = null; return r + ' ' + q; })",
                r#""true true""#,
            ),
            (
                r"E(() => { const args = []; window.onerror = function () { args.push(arguments.length, typeof arguments[0], typeof arguments[1]); }; window.dispatchEvent(new Event('error')); window.onerror = null; return args.join(); })",
                r#""1,object,undefined""#,
            ),
            (
                r"E(() => { window.onerror = null; document.body.setAttribute('onerror', 'window.zz3 = [arguments.length, typeof event, typeof source, typeof lineno, typeof colno, typeof error].join()'); window.dispatchEvent(new ErrorEvent('error', {message: 'm'})); const r = window.zz3; document.body.removeAttribute('onerror'); return r + ' ' + window.onerror; })",
                r#""5,string,string,number,number,undefined null""#,
            ),
            (
                r"E(() => { const x = document.createElement('div'); x.setAttribute('onerror', 'window.zz4 = arguments.length + typeof event + typeof source'); x.dispatchEvent(new ErrorEvent('error')); return window.zz4; })",
                r#""1objectundefined""#,
            ),
            (
                r"E(() => { const x = document.createElementNS('http://www.w3.org/2000/svg', 'rect'); x.setAttribute('onerror', 'window.zz5 = arguments.length + typeof evt + typeof event'); x.dispatchEvent(new ErrorEvent('error')); return window.zz5; })",
                r#""1objectobject""#,
            ),
            (
                r"E(() => { const x = document.createElementNS('http://www.w3.org/2000/svg', 'rect'); return ['onclick', 'onload', 'focus', 'blur', 'oncopy'].map((n) => n + '=' + typeof x[n]).join(); })",
                r#""onclick=object,onload=object,focus=function,blur=function,oncopy=object""#,
            ),
            (
                r"E(() => { const order = []; window.addEventListener('error', () => order.push('listener1')); window.onerror = () => { order.push('onerror'); }; window.addEventListener('error', () => order.push('listener2')); window.dispatchEvent(new Event('error')); window.onerror = null; return order.join(); })",
                r#""listener1,onerror,listener2""#,
            ),
            (
                r"E(() => { const order = []; document.onreadystatechange = () => order.push('rsc'); document.dispatchEvent(new Event('readystatechange')); document.onreadystatechange = null; return order.join(); })",
                r#""rsc""#,
            ),
            (
                r"E(() => { window.onbeforeunload = () => 'x'; const r = typeof window.onbeforeunload; window.onbeforeunload = null; return r; })",
                r#""function""#,
            ),
            (
                r"E(() => { const r = []; for (const n of ['onclick', 'onkeydown', 'onmousewheel', 'onwheel', 'onpointerdown', 'ontouchstart', 'onselectstart', 'onanimationend', 'onwebkitanimationend', 'ontransitionend', 'oncopy', 'onreadystatechange', 'onvisibilitychange', 'onhashchange', 'onfocusin', 'onfocusout', 'onmouseenter']) { r.push(n + '=' + [n in window, n in document, n in document.body, n in document.createElement('div')].map(Number).join('')); } return r.join(' '); })",
                r#""onclick=1111 onkeydown=1111 onmousewheel=1111 onwheel=1111 onpointerdown=1111 ontouchstart=0000 onselectstart=1111 onanimationend=1111 onwebkitanimationend=1111 ontransitionend=1111 oncopy=0111 onreadystatechange=0100 onvisibilitychange=0100 onhashchange=1010 onfocusin=0000 onfocusout=0000 onmouseenter=1111""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'onclick'); return [typeof d.get, typeof d.set, d.get.name, d.set.name, d.enumerable, d.configurable, d.get.length, d.set.length].join(); })",
                r#""function,function,get onclick,set onclick,true,true,0,1""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(Window.prototype, 'onclick'); const e = Object.getOwnPropertyDescriptor(window, 'onclick'); return [d === undefined, e === undefined].join(); })",
                r#""true,false""#,
            ),
            (
                r"E(() => HTMLElement.prototype.__lookupGetter__('onclick').call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'onclick').set.call(document.createElement('p'), () => 1))",
                r#""undefined""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'onclick').set.call(document.createElement('p')))",
                r#""undefined""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Document.prototype, 'onclick').get.call(document.createElement('p')))",
                r#""TypeError: Illegal invocation""#,
            ),
        ],
    },
    Case {
        name: "label-recursion",
        html: r"<!DOCTYPE html><html><body><label id=l>t<input id=i></label><label id=l2 for=i2><span>x</span></label><input id=i2 type=checkbox><label id=l3>t3<input id=i3 type=checkbox><label id=l4 for=i3>in</label></label><script>window.n=0;document.addEventListener('click',()=>n++,true);</script></body></html>",
        queries: &[
            (
                r"(() => { n=0; l.click(); return n + ' ' + document.activeElement.id })()",
                r#""2 i""#,
            ),
            (
                r"(() => { n=0; l2.click(); return n + ' ' + i2.checked })()",
                r#""2 true""#,
            ),
            (
                r"(() => { n=0; l3.click(); return n + ' ' + i3.checked })()",
                r#""2 true""#,
            ),
            (
                r"(() => { n=0; l4.click(); return n + ' ' + i3.checked })()",
                r#""2 false""#,
            ),
            (
                r"(() => { n=0; i.dispatchEvent(new MouseEvent('click',{bubbles:true})); return n + ' ' + document.activeElement.id })()",
                r#""1 i3""#,
            ),
            (
                r"(() => { const a = document.createElement('label'); const b = document.createElement('label'); const x = document.createElement('input'); const y = document.createElement('input'); a.appendChild(b); b.appendChild(y); a.appendChild(x); document.body.appendChild(a); n=0; a.click(); const r1 = n; n=0; b.click(); const r2 = n; n=0; y.click(); return r1 + ' ' + r2 + ' ' + n })()",
                r#""2 2 1""#,
            ),
            (
                r"(() => { const b = document.createElement('button'); b.id = 'bb'; const l = document.createElement('label'); l.htmlFor = 'bb'; b.appendChild(l); document.body.appendChild(b); n=0; l.click(); return n })()",
                r"1",
            ),
        ],
    },
    Case {
        name: "handler-window-scope",
        html: r#"<!DOCTYPE html><html><body onload="window.zz=[typeof createElement, typeof tagName, typeof getElementById, this===window, typeof event].join()"><form id=fm onsubmit="window.zs=[typeof action, typeof createElement].join()"><input id=i onclick="window.zi=[typeof action, typeof createElement,typeof submit].join()"></form><script>window.addEventListener('load',()=>{window.zr=window.zz});</script></body></html>"#,
        queries: &[
            (
                r"window.zr",
                r#""undefined,undefined,undefined,true,object""#,
            ),
            (
                r"(() => {fm.dispatchEvent(new Event('submit')); return window.zs})()",
                r#""string,function""#,
            ),
            (
                r"(() => {i.dispatchEvent(new MouseEvent('click')); return window.zi})()",
                r#""string,function,function""#,
            ),
            (
                r"(() => { const f = document.createElement('form'); f.setAttribute('onsubmit','window.zq=typeof action'); f.dispatchEvent(new Event('submit')); return window.zq })()",
                r#""string""#,
            ),
            (
                r"(() => { const d = document.createElement('div'); document.body.appendChild(d); d.onclick; d.setAttribute('onclick','window.zd=typeof id+typeof submit'); d.dispatchEvent(new MouseEvent('click')); return window.zd })()",
                r#""stringundefined""#,
            ),
        ],
    },
    Case {
        name: "focus-fixup-moves",
        html: r"<!DOCTYPE html><html><body><div id=w><input id=i></div><div id=z></div><script>var log=[];for (const t of ['blur','focusout']) document.addEventListener(t,e=>log.push(t+':'+(document.contains(e.target))+':'+document.activeElement.tagName),true);function R(f){log.length=0;document.body.innerHTML='<div id=w><input id=i></div><div id=z></div>';const w=document.getElementById('w');const i=document.getElementById('i');i.focus();log.length=0;f(w,i);return log.join()+' => '+document.activeElement.tagName}</script></body></html>",
        queries: &[
            (
                r"R((w,i)=>{w.textContent=''})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.innerHTML=''})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.outerHTML='<p></p>'})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.replaceChildren()})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{i.replaceWith('x')})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.replaceChild(document.createElement('b'),i)})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.removeChild(i)})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{document.getElementById('z').appendChild(i)})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.insertAdjacentHTML('afterbegin','<b></b>')})",
                r#"" => INPUT""#,
            ),
            (
                r"R((w,i)=>{w.remove()})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{document.getElementById('z').appendChild(w)})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (r"R((w,i)=>{w.hidden=true})", r#"" => INPUT""#),
            (r"R((w,i)=>{w.setAttribute('hidden','')})", r#"" => INPUT""#),
            (
                r"R((w,i)=>{i.setAttribute('disabled','')})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{const fs=document.createElement('fieldset');w.replaceChildren(fs);fs.appendChild(i);i.focus();log.length=0;fs.disabled=true})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{w.innerHTML='<input id=i disabled>'})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (r"R((w,i)=>{i.type='hidden'})", r#"" => INPUT""#),
            (r"R((w,i)=>{i.type='hidden'})", r#"" => INPUT""#),
            (
                r"R((w,i)=>{i.hidden=true})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{i.setAttribute('hidden','until-found')})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{document.body.insertBefore(w,document.getElementById('z'))})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{document.getElementById('z').insertBefore(i,null)})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
            (
                r"R((w,i)=>{document.getElementById('z').append(w)})",
                r#""blur:true:BODY,focusout:true:BODY => BODY""#,
            ),
        ],
    },
];
