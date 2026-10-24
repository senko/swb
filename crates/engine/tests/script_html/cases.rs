// The cases of `tools/probes/web-html-elements.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-html-elements.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::Case;

/// The page of the cases.
pub(crate) const PAGE: &str = r"<!DOCTYPE html><html><head><base href='http://x.test/d/e.html'><title>t</title></head><body><div id=a class='k1 k2'><p id=p class=k1>x</p><span id=s name=nm>y</span></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : (v === undefined ? 'undefined' : (typeof v === 'number' && v !== v ? 'NaN' : (Object.is(v, -0) ? '-0' : String(v)))); }
function K(h) { const d = document.createElement('div'); d.innerHTML = h; document.body.appendChild(d); return d; }
function ids(l) { return Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
const VALUES = {
  str: ['', 'x', ' Mixed Case ', 'a b'],
  enum: [''],
  bool: ['', 'x', 'false', 'true', 'FALSE'],
  url: ['', 'x', ' x ', '/p?q#f', '//h.test/p', 'http://a.test/c d', 'javascript:void(0)', 'mailto:a@b.test', '#f', '?q', 'x\ty', 'http://[bad', 'ftp://u:p@h.test:21/p'],
  long: ['5', '-5', '+5', '5x', ' 5', '', 'x', '2147483647', '2147483648', '-2147483648', '-2147483649', '1e3', '0', '00', '5.9', '-0'],
};
const WRITES = {
  str: ['', 'x', null, undefined, 5, true, {toString() { return 'obj'; }}],
  enum: ['', 'x', null, 'X'],
  bool: [true, false, 0, 1, '', 'x', null, undefined, NaN, {}],
  url: ['', 'x', '/p', null, 5],
  long: [0, 1, -1, 5.9, -5.9, 2147483647, 2147483648, -2147483648, -2147483649, 4294967295, 4294967296, 1e20, NaN, Infinity, '7', 'x', null, undefined],
};
// kind: str, enum (the keywords in `kw`), bool, url, long (signed), ulong, nonneg.
function P(tag, prop, attr, kind, kw) {
  const out = [];
  const make = () => document.createElement(tag);
  let e = make();
  out.push('d=' + S(e[prop]));
  let values = VALUES[kind] || VALUES.long;
  if (kind === 'enum' || kind === 'enumnull') {
    values = VALUES.enum.concat(kw, kw.map((k) => k.toUpperCase()), kw.map((k) => ' ' + k), ['junk']);
  }
  for (const v of values) { e = make(); e.setAttribute(attr, v); out.push(S(v) + '>' + S(e[prop])); }
  const k = kind === 'enumnull' ? 'enum' : (kind === 'ulong' || kind === 'nonneg' ? 'long' : kind);
  let writes = WRITES[k];
  if (kind === 'enum' || kind === 'enumnull') writes = WRITES.enum.concat(kw.slice(0, 3));
  for (const v of writes) {
    e = make();
    try { e[prop] = v; out.push(S(v) + '<' + S(e.getAttribute(attr))); } catch (x) { out.push(S(v) + '<!' + x.constructor.name + ':' + x.message); }
  }
  return out.join(' | ');
}
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "forms-input-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const d = K('<input id=i1 value=a><input id=i2><input id=i3 type=number value=x><input id=i4 type=checkbox><input id=i5 type=hidden value=h><input id=i6 type=file><input id=i7 type=color value=red><input id=i8 type=submit>'); return ['i1','i2','i3','i4','i5','i6','i7','i8'].map((i) => JSON.stringify(document.getElementById(i).value)).join(); })",
                r##""\"a\",\"\",\"\",\"on\",\"h\",\"\",\"#ff0000\",\"\"""##,
            ),
            (
                r"E(() => { const d = K('<input id=w1 value=a>'); const e = document.getElementById('w1'); const r = [e.value, e.defaultValue]; e.setAttribute('value', 'b'); r.push(e.value, e.defaultValue); e.value = 'c'; r.push(e.value, e.defaultValue, e.getAttribute('value')); e.setAttribute('value', 'd'); r.push(e.value, e.defaultValue); e.defaultValue = 'e'; r.push(e.value, e.getAttribute('value')); return r; })",
                r#""a,a,b,b,c,b,b,c,d,c,e""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.value = 'x'; const r = [e.value, e.getAttribute('value')]; document.body.appendChild(e); r.push(e.value); e.setAttribute('value', 'y'); r.push(e.value); return r; })",
                r#""x,,x,x""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.value = 'x'; K('<p>q</p>'); const r = [e.value]; document.body.appendChild(e); K('<b></b>'); r.push(e.value, e.defaultValue === ''); e.remove(); r.push(e.value); document.body.appendChild(e); r.push(e.value); return r; })",
                r#""x,x,true,x,x""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.value = 'a\nb'; const r = [e.value]; e.type = 'number'; r.push(e.value); e.value = '12'; r.push(e.value); e.value = 'abc'; r.push(e.value); e.type = 'color'; r.push(e.value); e.value = '#ABCDEF'; r.push(e.value); e.type = 'url'; e.value = '  http://x  '; r.push(e.value); return r; })",
                r#""ab,,12,,#000000,#abcdef,http://x""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.type = 'checkbox'; const r = [e.value]; e.value = 'v'; r.push(e.value, e.getAttribute('value')); e.type = 'hidden'; r.push(e.value); e.type = 'submit'; r.push(e.value); e.type = 'file'; r.push(e.value); e.value = ''; r.push(e.value); try { e.value = 'x'; } catch (x) { r.push(x.name + ': ' + x.message); } return r; })",
                r#""on,v,v,v,v,,,InvalidStateError: Failed to set the 'value' property on 'HTMLInputElement': This input element accepts a filename, which may only be programmatically set to the empty string.""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.value = null; const r = [e.value]; e.value = undefined; r.push(e.value); e.value = 5; r.push(e.value); e.value = {toString() { return 'obj'; }}; r.push(e.value); return r; })",
                r#"",undefined,5,obj""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.value = 'x'; e.type = 'text'; e.setAttribute('type', 'password'); return [e.value, e.type]; })",
                r#""x,password""#,
            ),
            (
                r"E(() => { const d = K('<input id=m1 value=a>'); const e = document.getElementById('m1'); e.value = 'zz'; document.body.appendChild(K('<div id=m2></div>')); return [e.value, e.defaultValue, document.querySelectorAll(':invalid').length]; })",
                r#""zz,a,0""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.value = 'q'.repeat(5); e.maxLength = 2; return [e.value, e.maxLength, e.getAttribute('maxlength')]; })",
                r#""qqqqq,2,2""#,
            ),
            (
                r"E(() => { const d = K('<input id=pl placeholder=ph>'); const e = document.getElementById('pl'); const a = document.querySelectorAll(':placeholder-shown').length; e.value = 'x'; const b = document.querySelectorAll(':placeholder-shown').length; e.value = ''; return [a, b, document.querySelectorAll(':placeholder-shown').length]; })",
                r#""1,0,1""#,
            ),
            (
                r"E(() => { const d = K('<input id=rq required>'); const e = document.getElementById('rq'); const a = document.querySelectorAll('#rq:invalid').length; e.value = 'x'; const b = document.querySelectorAll('#rq:invalid').length; const c = document.querySelectorAll('#rq:valid').length; e.value = ''; return [a, b, c, document.querySelectorAll('#rq:invalid').length]; })",
                r#""1,0,1,1""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); return [e.indeterminate, (e.indeterminate = true, e.indeterminate), (e.indeterminate = 0, e.indeterminate)]; })",
                r#""false,true,false""#,
            ),
            (
                r"E(() => { const d = K('<input id=dl list=dlx><datalist id=dlx><option value=a></datalist><input id=nl list=nope>'); return [document.getElementById('dl').list.id, document.getElementById('nl').list, document.getElementById('dl').list === document.getElementById('dlx')]; })",
                r#""dlx,,true""#,
            ),
            (
                r"E(() => { const d = K('<form id=ff><input id=fi></form><input id=fo form=ff><input id=fn form=nope>'); return [document.getElementById('fi').form.id, document.getElementById('fo').form.id, document.getElementById('fn').form, document.createElement('input').form]; })",
                r#""ff,ff,,""#,
            ),
        ],
    },
    Case {
        name: "forms-checked-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const d = K('<input id=c1 type=checkbox checked><input id=c2 type=checkbox><input id=c3 type=text checked>'); const a = document.getElementById('c1'); const b = document.getElementById('c2'); return [a.checked, a.defaultChecked, b.checked, document.getElementById('c3').checked, document.querySelectorAll(':checked').length]; })",
                r#""true,true,false,true,1""#,
            ),
            (
                r"E(() => { const e = document.getElementById('c2'); e.checked = true; const r = [e.checked, e.defaultChecked, e.hasAttribute('checked'), document.querySelectorAll(':checked').length]; e.setAttribute('checked', ''); e.removeAttribute('checked'); r.push(e.checked); e.checked = false; r.push(e.checked, document.querySelectorAll(':checked').length); return r; })",
                r#""true,false,false,2,true,false,1""#,
            ),
            (
                r"E(() => { const e = document.getElementById('c1'); e.checked = false; e.defaultChecked = false; e.defaultChecked = true; return [e.checked, e.defaultChecked, document.querySelectorAll(':checked').length]; })",
                r#""false,true,0""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.type = 'checkbox'; e.checked = true; const r = [e.checked]; document.body.appendChild(e); r.push(e.checked, document.querySelectorAll('input:checked').length); e.remove(); r.push(e.checked); return r; })",
                r#""true,true,1,true""#,
            ),
            (
                r"E(() => { const d = K('<input type=radio name=g id=ra><input type=radio name=g id=rb checked><input type=radio name=g id=rc><input type=radio name=h id=rd checked>'); const c = (i) => document.getElementById(i).checked; const r = [c('ra'), c('rb'), c('rc'), c('rd')]; document.getElementById('ra').checked = true; r.push(c('ra'), c('rb'), c('rc'), c('rd')); document.getElementById('rc').checked = true; r.push(c('ra'), c('rb'), c('rc')); document.getElementById('rc').checked = false; r.push(c('ra'), c('rb'), c('rc')); return r; })",
                r#""false,true,false,true,true,false,false,true,false,false,true,false,false,false""#,
            ),
            (
                r"E(() => { const d = K('<form><input type=radio name=g id=sa checked><input type=radio name=g id=sb></form><form><input type=radio name=g id=sc checked></form>'); const c = (i) => document.getElementById(i).checked; document.getElementById('sb').checked = true; return [c('sa'), c('sb'), c('sc')]; })",
                r#""false,true,true""#,
            ),
            (
                r"E(() => { const a = document.createElement('input'), b = document.createElement('input'); a.type = b.type = 'radio'; a.name = b.name = 'zz'; a.checked = true; b.checked = true; const d = document.createElement('div'); d.append(a, b); document.body.appendChild(d); return [a.checked, b.checked]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const a = document.createElement('input'), b = document.createElement('input'); a.type = b.type = 'radio'; a.name = b.name = 'yy'; b.checked = true; a.checked = true; const d = document.createElement('div'); d.append(a, b); document.body.appendChild(d); return [a.checked, b.checked]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const d = K('<input type=radio name=q id=qa checked><input type=radio name=q id=qb checked>'); return [document.getElementById('qa').checked, document.getElementById('qb').checked]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const d = K('<input type=radio name=u id=ua checked><input type=radio name=u id=ub>'); document.getElementById('ub').checked = true; const x = document.createElement('div'); x.appendChild(K('<input type=radio name=u id=uc checked>')); return [document.getElementById('ua').checked, document.getElementById('ub').checked, document.getElementById('uc').checked]; })",
                r#""TypeError: Cannot read properties of null (reading 'checked')""#,
            ),
        ],
    },
    Case {
        name: "forms-textarea-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const d = K('<textarea id=t1>\nline1\r\nline2</textarea><textarea id=t2></textarea>'); const e = document.getElementById('t1'); const r = [e.value, e.defaultValue, e.textLength, e.type]; e.value = 'x\r\ny'; r.push(e.value, e.defaultValue, e.textLength); e.defaultValue = 'new'; r.push(e.value, e.textContent); return r; })",
                r#""line1\nline2,line1\nline2,11,textarea,x\ny,line1\nline2,3,x\ny,new""#,
            ),
            (
                r"E(() => { const e = document.getElementById('t2'); e.textContent = 'a'; const r = [e.value]; e.value = 'b'; e.textContent = 'c'; r.push(e.value, e.defaultValue); return r; })",
                r#""a,b,c""#,
            ),
            (
                r"E(() => { const e = document.createElement('textarea'); e.value = 'detached'; document.body.appendChild(e); return [e.value, e.textContent, e.defaultValue]; })",
                r#""detached,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('textarea'); return [e.cols, e.rows, e.wrap, e.maxLength, e.minLength]; })",
                r#""20,2,,-1,-1""#,
            ),
        ],
    },
    Case {
        name: "forms-select-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => { const d = K("<select id=s1><option id=o1 value=a>A</option><option id=o2 selected>B B</option><optgroup id=g1 label=G><option id=o3 value=c>C</option><option id=o4 disabled>D</option></optgroup></select><select id=s2 multiple><option id=m1 selected>1</option><option id=m2>2</option><option id=m3 selected>3</option></select><select id=s3></select><select id=s4><option disabled id=d1>x</option><option id=d2>y</option></select>"); const s = document.getElementById('s1'); const m = document.getElementById('s2'); return [s.selectedIndex, s.value, s.length, s.options.length, ids(s.options), s.type, m.type, m.selectedIndex, m.value, ids(m.selectedOptions), ids(s.selectedOptions), document.getElementById('s3').selectedIndex, document.getElementById('s3').value, document.getElementById('s4').selectedIndex]; })"#,
                r#""1,B B,4,4,o1,o2,o3,o4,select-one,select-multiple,0,1,m1,m3,o2,-1,,1""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s1'); s.selectedIndex = 0; const r = [s.value, s.selectedIndex, document.getElementById('o2').selected, document.getElementById('o1').selected, ids(s.selectedOptions), document.querySelectorAll('option:checked').length]; s.selectedIndex = 2; r.push(s.value, s.selectedIndex); s.selectedIndex = -1; r.push(s.value, s.selectedIndex, s.options.selectedIndex); s.selectedIndex = 9; r.push(s.selectedIndex); return r; })",
                r#""a,0,false,true,o1,4,c,2,,-1,-1,-1""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s1'); s.value = 'c'; const r = [s.selectedIndex, s.value]; s.value = 'nope'; r.push(s.selectedIndex, s.value); s.value = 'B B'; r.push(s.selectedIndex); s.value = 'A'; r.push(s.selectedIndex); return r; })",
                r#""2,c,-1,,1,-1""#,
            ),
            (
                r"E(() => { const o = document.getElementById('o3'); o.selected = true; const s = document.getElementById('s1'); const r = [s.selectedIndex, document.getElementById('o2').selected, o.selected]; o.selected = false; r.push(s.selectedIndex, o.selected, document.getElementById('o1').selected); return r; })",
                r#""2,false,true,0,false,true""#,
            ),
            (
                r"E(() => { const m = document.getElementById('s2'); document.getElementById('m2').selected = true; const r = [ids(m.selectedOptions), m.selectedIndex]; document.getElementById('m1').selected = false; r.push(ids(m.selectedOptions), m.selectedIndex); m.selectedIndex = 1; r.push(ids(m.selectedOptions)); return r; })",
                r#""m1,m2,m3,0,m2,m3,1,m2""#,
            ),
            (
                r"E(() => { const o = document.getElementById('o1'); return [o.defaultSelected, document.getElementById('o2').defaultSelected, o.selected, o.index, document.getElementById('o3').index, document.getElementById('o4').index, o.value, document.getElementById('o2').value, document.getElementById('o3').text, document.getElementById('o2').text, o.label, document.getElementById('o4').disabled, o.form]; })",
                r#""false,true,true,0,2,3,a,B B,C,B B,A,true,""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s1'); const r = [s.options.constructor.name, s.options === s.options, s.options.item(1).id, s.item(2).id, s.namedItem('o3').id, s.options.namedItem('o1').id, s[0].id, s[3].id, s[4], s.options.length]; return r; })",
                r#""HTMLOptionsCollection,true,o2,o3,o3,o1,o1,o4,,4""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); s.length = 3; const r = [s.length, s.options.length, s.innerHTML]; s.length = 1; r.push(s.length, s.innerHTML); s.length = 100001; r.push(s.length); s.options.length = 0; r.push(s.length, s.innerHTML); return r; })",
                r#""3,3,<option></option><option></option><option></option>,1,<option></option>,1,0,""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); const o1 = document.createElement('option'); o1.text = 'one'; o1.value = '1'; s.add(o1); const o2 = document.createElement('option'); o2.text = 'two'; s.add(o2, 0); const o3 = document.createElement('option'); o3.text = 'three'; s.add(o3, o1); const o4 = document.createElement('option'); o4.text = 'four'; s.options.add(o4, 7); const r = [s.innerHTML, s.selectedIndex]; s.remove(1); r.push(s.innerHTML); s.options.remove(0); r.push(s.innerHTML); s.remove(10); s.remove(-1); r.push(s.length); return r; })",
                r#""<option>two</option><option>three</option><option value=\"1\">one</option><option>four</option>,2,<option>two</option><option value=\"1\">one</option><option>four</option>,<option value=\"1\">one</option><option>four</option>,2""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); s.innerHTML = ''; const g = document.createElement('optgroup'); s.add(g); const o = document.createElement('option'); s.add(o, g); return [s.innerHTML]; })",
                r#""<option></option><optgroup></optgroup>""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); s.innerHTML = ''; const o = document.createElement('option'); try { s.add(o, document.body); } catch (e) { return e.name + ': ' + e.message; } })",
                r#""NotFoundError: Failed to execute 'add' on 'HTMLSelectElement': The node before which the new node is to be inserted is not a child of this node.""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); try { s.add(s); } catch (e) { return e.name + ': ' + e.message; } })",
                r#""TypeError: Failed to execute 'add' on 'HTMLSelectElement': The provided value is not of type '(HTMLOptGroupElement or HTMLOptionElement)'.""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); s.innerHTML = ''; const o = document.createElement('option'); s.add(o, o); return s.innerHTML; })",
                r#""DOMException NotFoundError: Failed to execute 'add' on 'HTMLSelectElement': The node before which the new node is to be inserted is not a child of this node.""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); s.innerHTML = '<option>a<option>b'; const o = document.createElement('option'); o.text = 'z'; s[1] = o; const r = [s.innerHTML]; s[5] = document.createElement('option'); r.push(s.length, s.innerHTML); s[0] = null; r.push(s.length); s.options[2] = document.createElement('option'); r.push(s.length); return r; })",
                r#""<option>a</option><option>z</option>,6,<option>a</option><option>z</option><option></option><option></option><option></option><option></option>,5,5""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s3'); s.innerHTML = '<option>a<option>b<option>c'; const r = [s.selectedIndex]; s.remove(0); r.push(s.selectedIndex, s.value); s.options[0].remove(); r.push(s.selectedIndex, s.value); s.firstChild.remove(); r.push(s.selectedIndex, s.value, s.length); return r; })",
                r#""0,0,b,0,c,-1,,0""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); const r = [s.selectedIndex]; const o = document.createElement('option'); o.value = 'v'; s.appendChild(o); r.push(s.selectedIndex, s.value, o.selected); const p = document.createElement('option'); p.selected = true; s.appendChild(p); r.push(s.selectedIndex, o.selected, p.selected); return r; })",
                r#""-1,0,v,true,1,false,true""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.selected = true; const r = [o.selected, o.defaultSelected]; const s = document.createElement('select'); s.appendChild(document.createElement('option')); s.appendChild(o); r.push(s.selectedIndex); return r; })",
                r#""true,false,1""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s.multiple = true; s.innerHTML = '<option>a<option>b'; const r = [s.selectedIndex, s.type, s.size]; s.options[1].selected = true; r.push(s.selectedIndex, ids(s.selectedOptions)); s.multiple = false; r.push(s.selectedIndex); return r; })",
                r#""-1,select-multiple,0,1,OPTION,1""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s.size = 3; s.innerHTML = '<option>a<option>b'; return [s.selectedIndex, s.size, s.value]; })",
                r#""-1,3,""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s1'); const r = [s.selectedOptions.constructor.name, s.selectedOptions === s.selectedOptions]; const l = s.selectedOptions; s.selectedIndex = 2; r.push(ids(l)); return r; })",
                r#""HTMLCollection,true,o3""#,
            ),
            (
                r"E(() => { const d = K('<form id=sf><select id=s9><option>x</option></select></form>'); return [document.getElementById('s9').form.id, document.getElementById('s9').options[0].form.id, document.createElement('option').form]; })",
                r#""sf,sf,""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.text = ' a  b '; return [o.text, o.value, o.label, o.innerHTML]; })",
                r#""a b,a b,a b, a  b ""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.setAttribute('label', ''); o.text = 'x'; return [o.label]; })",
                r#""""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.label = 'L'; o.value = 'V'; return [o.getAttribute('label'), o.getAttribute('value'), o.label, o.value]; })",
                r#""L,V,L,V""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.text = 'T'; o.text = '<b>x</b>'; return [o.innerHTML, o.childNodes.length]; })",
                r#""&lt;b&gt;x&lt;/b&gt;,1""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.disabled = true; return [o.disabled, o.getAttribute('disabled'), o.index]; })",
                r#""true,,0""#,
            ),
            (
                r"E(() => { const g = document.createElement('optgroup'); g.label = 'x'; g.disabled = true; return [g.label, g.disabled, g.getAttribute('label')]; })",
                r#""x,true,x""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s1'); const r = []; for (let i = 0; i < 3; i++) { s.selectedIndex = i; r.push(s.selectedIndex); } return r; })",
                r#""0,1,2""#,
            ),
            (
                r"E(() => { const s = document.getElementById('s4'); const r = [s.selectedIndex]; document.getElementById('d1').disabled = false; r.push(s.selectedIndex); s.innerHTML = '<option disabled>a<option disabled>b'; r.push(s.selectedIndex); return r; })",
                r#""1,1,-1""#,
            ),
        ],
    },
    Case {
        name: "forms-form-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => { const d = K("<form id=F><input id=a1 name=n1><input id=a2 name=n2><input type=radio name=rr id=r1 value=x><input type=radio name=rr id=r2 value=y checked><input type=radio name=rr id=r3><select id=sel name=sl><option>o</select><textarea id=ta name=n1></textarea><button id=bt name=bb></button><fieldset id=fs name=fz><input id=ina name=inner></fieldset><object id=ob name=oo></object><output id=ou name=ouu></output><input type=image id=im name=imn><img id=img name=pic><div><input id=nest></div></form><input id=ext form=F name=extn><input id=other>"); const f = document.getElementById('F'); return [f.length, f.elements.length, ids(f.elements), f.elements.constructor.name, f.elements === f.elements, f.elements[0].id, f[0].id, f[3].id, f[100], f.elements.item(1).id, f.elements.item(100)]; })"#,
                r#""14,14,a1,a2,r1,r2,r3,sel,ta,bt,fs,ina,ob,ou,nest,ext,HTMLFormControlsCollection,true,a1,a1,r2,,a2,""#,
            ),
            (
                r"E(() => { const f = document.getElementById('F'); const r = [f.n2.id, f.sl.id, f.elements.n2.id, f.elements['n2'].id, f.elements.sl.id, f.pic, f.imn, f.elements.imn, f.elements.pic, f.fz.id, f.ouu.id, f.bb.id, f.oo.id, f.nest.id, f.extn.id]; return r; })",
                r#""a2,sel,a2,a2,sel,[object HTMLImageElement],,,,fs,ou,bt,ob,nest,ext""#,
            ),
            (
                r"E(() => { const f = document.getElementById('F'); const n1 = f.n1; const rr = f.rr; return [n1.constructor.name, n1.length, ids(n1), rr.constructor.name, rr.length, ids(rr), rr.value, f.elements.rr.value, f.elements.n1.length, f.elements.namedItem('n1').constructor.name, f.elements.namedItem('n2').id, f.elements.namedItem('nope'), f.elements.namedItem('')]; })",
                r#""RadioNodeList,2,a1,ta,RadioNodeList,3,r1,r2,r3,y,y,2,RadioNodeList,a2,,""#,
            ),
            (
                r"E(() => { const f = document.getElementById('F'); const rr = f.rr; rr.value = 'x'; const r = [rr.value, document.getElementById('r1').checked, document.getElementById('r2').checked]; rr.value = 'zz'; r.push(rr.value, document.getElementById('r1').checked); document.getElementById('r3').checked = true; r.push(rr.value); document.getElementById('r3').value = 'v3'; r.push(rr.value); return r; })",
                r#""x,true,false,x,true,on,v3""#,
            ),
            (
                r"E(() => { const f = document.getElementById('F'); const r = [f.a1.id, 'n1' in f, 'nope' in f, 'a1' in f, 'imn' in f, Object.keys(f).join(), f.elements.a1.id, Object.keys(f.elements).join()]; return r; })",
                r#""a1,true,false,true,false,0,1,2,3,4,5,6,7,8,9,10,11,12,13,a1,0,1,2,3,4,5,6,7,8,9,10,11,12,13,a1,n1,a2,n2,r1,rr,r2,r3,sel,sl,ta,bt,bb,fs,fz,ina,inner,ob,oo,ou,ouu,nest,ext,extn""#,
            ),
            (
                r"E(() => { const f = document.getElementById('F'); const r = []; const el = f.elements; r.push(el.length); f.appendChild(document.createElement('input')); r.push(el.length, f.length); document.getElementById('a1').remove(); r.push(el.length); document.getElementById('ext').removeAttribute('form'); r.push(el.length); return r; })",
                r#""14,15,15,14,13""#,
            ),
            (
                r"E(() => { const f = document.getElementById('F'); const i = document.createElement('input'); i.name = 'late'; f.appendChild(i); return [f.late === i, f.elements.late === i, i.form === f]; })",
                r#""true,true,true""#,
            ),
            (
                r"E(() => { const f = document.createElement('form'); f.innerHTML = '<input name=a><input name=a><input id=a>'; return [f.length, f.a.length, ids(f.a), f.elements.a.length]; })",
                r#""3,3,INPUT,INPUT,a,3""#,
            ),
            (
                r"E(() => { const f = document.createElement('form'); const i = document.createElement('input'); i.id = 'zq'; i.setAttribute('form', 'zqf'); f.id = 'zqf'; const r = [i.form]; document.body.append(f, i); r.push(i.form === f, f.length, f.elements[0] === i); return r; })",
                r#"",true,1,true""#,
            ),
            (
                r"E(() => { const f = document.createElement('form'); f.innerHTML = '<table><tr><td><input id=tin></td></tr></table>'; return [f.elements.length, f.querySelector('#tin').form === f]; })",
                r#""1,true""#,
            ),
            (
                r"E(() => { const f = document.createElement('form'); f.innerHTML = '<input name=length><input name=item>'; return [typeof f.length, typeof f.item, typeof f.elements.length, typeof f.elements.item]; })",
                r#""object,object,number,function""#,
            ),
            (
                r"E(() => { const f = document.createElement('form'); f.innerHTML = '<input name=action><input name=submit>'; return [typeof f.action, f.action.constructor.name, f.getAttribute('action')]; })",
                r#""object,HTMLInputElement,""#,
            ),
            (
                r"E(() => { const l = K('<label id=lb for=a2>x</label><label id=lb2><b>y</b><input id=lbi></label><label id=lb3 for=nope></label><label id=lb4 for=ex>q</label><input id=ex type=hidden>'); const g = (i) => document.getElementById(i); return [g('lb').control.id, g('lb2').control.id, g('lb3').control, g('lb4').control, g('lb').form.id, g('lb2').form, g('lb').htmlFor, g('lb3').htmlFor]; })",
                r#""a2,lbi,,,F,,a2,nope""#,
            ),
            (
                r"E(() => { const l = K('<form id=lf><fieldset id=lfs><legend id=lg>l</legend></fieldset></form><legend id=lg2></legend>'); const g = (i) => document.getElementById(i); return [g('lg').form.id, g('lg2').form, g('lfs').form.id, g('lfs').type, g('lfs').elements.length]; })",
                r#""lf,,lf,fieldset,0""#,
            ),
            (
                r"E(() => { const l = K('<form id=bf><button id=b1></button><button id=b2 type=button></button><button id=b3 type=RESET></button><button id=b4 type=x></button></form>'); const g = (i) => document.getElementById(i); return [g('b1').type, g('b2').type, g('b3').type, g('b4').type, g('b1').form.id, g('b1').value, g('b1').name, g('b1').disabled]; })",
                r#""submit,button,reset,submit,bf,,,false""#,
            ),
            (
                r#"E(() => { const l = K('<output id=op for="a b" name=nn>text</output>'); const o = document.getElementById('op'); const r = [o.value, o.defaultValue, o.type, o.name, o.htmlFor.length]; o.value = 'new'; r.push(o.textContent, o.value); return r; })"#,
                r#""text,text,output,nn,2,new,new""#,
            ),
            (
                r"E(() => { const l = K('<object id=obj data=x.swf></object>'); const o = document.getElementById('obj'); return [o.data, o.form, o.type, o.name]; })",
                r#""http://x.test/d/x.swf,,,""#,
            ),
        ],
    },
    Case {
        name: "forms-fix-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; s.selectedIndex = 1; s.remove(); return [s.selectedIndex, s.value, s.options[1].selected]; })",
                r#""1,b,true""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const o = K('<select><option>x<option>y</select>').firstChild; s.selectedIndex = 1; s.remove(); o.selectedIndex; return [s.selectedIndex, s.value]; })",
                r#""1,b""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const t = document.createElement('template'); s.selectedIndex = 2; t.content.appendChild(s); return [s.selectedIndex, s.value, s.options[2].selected]; })",
                r#""2,c,true""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const d = document.implementation.createHTMLDocument('x'); s.selectedIndex = 1; d.body.appendChild(d.adoptNode(s)); return [s.selectedIndex, s.value, d.body.firstChild === s]; })",
                r#""1,b,true""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; s.selectedIndex = 1; s.remove(); s.selectedIndex = 2; document.body.appendChild(s); return [s.selectedIndex, s.value]; })",
                r#""2,c""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const o = s.options[0]; s.removeChild(o); const r = [o.selected, o.index]; o.selected = true; r.push(o.selected); s.appendChild(o); r.push(s.selectedIndex, s.options[1].selected); return r; })",
                r#""true,0,true,2,false""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const o = s.options[1]; s.selectedIndex = 1; o.remove(); s.selectedIndex; return [o.selected, s.selectedIndex, s.value]; })",
                r#""true,0,a""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const o = s.options[1]; s.selectedIndex = 1; o.remove(); return [o.selected, s.selectedIndex]; })",
                r#""true,0""#,
            ),
            (
                r"E(() => { const s = K('<select id=sd><option>a<option>b<option>c</select>').firstChild; const o = s.options[2]; o.selected = true; o.remove(); const r = [o.selected, s.selectedIndex]; s.append(o); r.push(s.selectedIndex, o.selected); return r; })",
                r#""true,0,2,true""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s.innerHTML = '<option>a<option>b'; s.selectedIndex = 1; document.body.appendChild(s); const r = [s.selectedIndex]; s.remove(); r.push(s.selectedIndex); return r; })",
                r#""1,1""#,
            ),
            (
                r"E(() => { const s = K('<select multiple><option>a<option selected>b<option>c</select>').firstChild; s.options[2].selected = true; s.remove(); return [s.selectedIndex, s.selectedOptions.length, s.options[2].selected]; })",
                r#""1,2,true""#,
            ),
            (
                r"E(() => { const i = K('<input value=a>').firstChild; const c = i.cloneNode(); i.value = 'z'; return [c.value, i.cloneNode().value, i.cloneNode().getAttribute('value'), i.cloneNode().defaultValue]; })",
                r#""a,z,a,a""#,
            ),
            (
                r"E(() => { const i = K('<input type=checkbox checked>').firstChild; i.checked = false; const c = i.cloneNode(); const d = K('<input type=checkbox>').firstChild; d.checked = true; return [c.checked, d.cloneNode().checked, c.defaultChecked, d.cloneNode().defaultChecked]; })",
                r#""false,true,true,false""#,
            ),
            (
                r"E(() => { const t = K('<textarea>x</textarea>').firstChild; const c = t.cloneNode(true); t.value = 'n'; const d = t.cloneNode(true); return [c.value, d.value, d.defaultValue, d.textContent]; })",
                r#""x,n,x,x""#,
            ),
            (
                r"E(() => { const t = document.createElement('template'); t.innerHTML = '<input value=q><textarea>w</textarea><input type=checkbox>'; const c = t.content.children; c[0].value = 'in'; c[1].value = 'tt'; c[2].checked = true; const f = document.importNode(t.content, true); return [f.children[0].value, f.children[1].value, f.children[2].checked, c[0].value]; })",
                r#""in,tt,true,in""#,
            ),
            (
                r"E(() => { const f = K('<form id=cf><input id=ci value=a><textarea>t</textarea><input type=checkbox></form>').firstChild; f.elements[0].value = 'q'; f.elements[1].value = 'w'; f.elements[2].checked = true; const c = f.cloneNode(true); return [c.elements[0].value, c.elements[1].value, c.elements[2].checked, f.cloneNode(false).elements.length]; })",
                r#""q,w,true,0""#,
            ),
            (
                r"E(() => { const i = K('<input value=a>').firstChild; i.value = 'z'; const d = document.implementation.createHTMLDocument('x'); const n = d.importNode(i, true); return [n.value, n.ownerDocument === d]; })",
                r#""z,true""#,
            ),
            (
                r"E(() => { const i = K('<input type=radio checked>').firstChild; i.checked = false; const c = i.cloneNode(); return [c.checked, c.defaultChecked]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const i = document.createElement('input'); i.value = 'det'; const c = i.cloneNode(); const j = document.createElement('input'); j.type = 'checkbox'; j.checked = true; return [c.value, j.cloneNode().checked]; })",
                r#""det,true""#,
            ),
            (
                r"E(() => { const i = K('<input value=a>').firstChild; i.value = 'z'; const c = i.cloneNode(); c.setAttribute('value', 'q'); return [c.value, i.value]; })",
                r#""z,z""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s.innerHTML = '<option>a  b\n c<script>x</script></option>'; const o = s.options[0]; return [o.text, o.value, o.label]; })",
                r#""a b c,a b c,a b c""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); const o = document.createElement('option'); o.innerHTML = 'a<b>b<script>q</script>c</b><svg><script>y</script><title>t</title></svg> d'; s.appendChild(o); return [o.text, o.value, o.label, s.value]; })",
                r#""abct d,abct d,abct d,abct d""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); const sc = document.createElement('script'); sc.textContent = 'zz'; o.append('x', sc, 'y'); return [o.text, o.value, o.label, o.textContent]; })",
                r#""xy,xy,xy,xzzy""#,
            ),
            (
                r"E(() => { const o = document.createElement('option'); o.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'script')).textContent = 'sv'; o.append(' ok'); return [o.text, o.value]; })",
                r#""ok,ok""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; r2.checked = true; const n = mk(); n.checked = true; r1.before(n); return [r1.checked, r2.checked, n.checked]; })",
                r#""false,false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; r1.checked = true; const n = mk(); n.checked = true; f.appendChild(n); return [r1.checked, r2.checked, n.checked]; })",
                r#""false,false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; const n = mk(); n.checked = true; r2.checked = true; f.appendChild(n); return [r1.checked, r2.checked, n.checked]; })",
                r#""false,false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; r2.checked = true; const c = r2.cloneNode(); r1.checked = true; f.appendChild(c); return [r1.checked, r2.checked, c.checked]; })",
                r#""false,false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; const a = mk(), b = mk(); a.checked = true; b.checked = true; f.append(a, b); return [r1.checked, r2.checked, a.checked, b.checked]; })",
                r#""false,false,false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; const a = mk(), b = mk(); f.append(a, b); a.checked = true; b.checked = true; return [a.checked, b.checked]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; r2.checked = true; r2.remove(); r1.checked = true; f.appendChild(r2); return [r1.checked, r2.checked]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const f = K('<form><input type=radio name=g id=fxr1><input type=radio name=g id=fxr2></form>').firstChild; const r1 = f.elements[0], r2 = f.elements[1]; const mk = () => { const n = document.createElement('input'); n.type = 'radio'; n.name = 'g'; return n; }; const n = mk(); n.checked = true; n.checked = false; f.appendChild(n); r1.checked = true; return [r1.checked, n.checked]; })",
                r#""true,false""#,
            ),
            (
                r"E(() => { const i = K('<input type=range min=0 max=10>').firstChild; const r = []; for (const v of ['20', '-3', '5', '5.5', 'abc', '0.5', '10', '10.0', '', '1e1', '-0']) { i.value = v; r.push(i.value); } return r; })",
                r#""10,0,5,6,5,1,10,10,5,10,0""#,
            ),
            (
                r"E(() => { const i = K('<input type=range min=1 max=10 step=3>').firstChild; const r = [i.value]; for (const v of ['2', '3', '5', '6', '12', '0', '9.9', '10']) { i.value = v; r.push(i.value); } return r; })",
                r#""7,1,4,4,7,10,1,10,10""#,
            ),
            (
                r"E(() => { const i = K('<input type=range min=0 max=1 step=any>').firstChild; const r = []; for (const v of ['0.3333', '2', '-1', '0.5']) { i.value = v; r.push(i.value); } return r; })",
                r#""0.3333,1,0,0.5""#,
            ),
            (
                r"E(() => { const i = K('<input type=range min=0 max=1 step=0.25>').firstChild; const r = []; for (const v of ['0.1', '0.13', '0.6', '0.875', '5']) { i.value = v; r.push(i.value); } return r; })",
                r#""0,0.25,0.5,1,1""#,
            ),
            (
                r"E(() => { const i = K('<input type=range min=10 max=5>').firstChild; const r = [i.value]; for (const v of ['20', '1', '7']) { i.value = v; r.push(i.value); } return r; })",
                r#""10,10,10,10""#,
            ),
            (
                r"E(() => { const i = K('<input type=range value=500>').firstChild; const j = K('<input type=range min=20 value=3>').firstChild; return [i.value, j.value, i.getAttribute('value')]; })",
                r#""100,20,500""#,
            ),
            (
                r"E(() => { const i = K('<input type=range min=0 max=10>').firstChild; i.value = '7'; i.max = '5'; const r = [i.value]; i.max = '100'; r.push(i.value); i.min = '8'; r.push(i.value); return r; })",
                r#""5,5,8""#,
            ),
            (
                r"E(() => { const i = K('<input type=checkbox>').firstChild; const r = [i.matches(':indeterminate')]; i.indeterminate = true; r.push(i.matches(':indeterminate'), document.querySelectorAll(':indeterminate').length); i.indeterminate = false; r.push(i.matches(':indeterminate')); return r; })",
                r#""false,true,2,false""#,
            ),
        ],
    },
    Case {
        name: "forms-fix-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const i = K('<input>').firstChild; i.indeterminate = true; return [i.matches(':indeterminate'), i.indeterminate]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const i = K('<input type=radio>').firstChild; i.indeterminate = true; return [i.matches(':indeterminate'), i.indeterminate]; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { const i = K('<input type=checkbox>').firstChild; i.indeterminate = true; i.checked = true; const r = [i.matches(':indeterminate')]; i.type = 'text'; r.push(i.matches(':indeterminate')); return r; })",
                r#""true,false""#,
            ),
            (
                r"E(() => { const i = document.createElement('input'); i.indeterminate = true; i.type = 'checkbox'; document.body.appendChild(i); return [i.matches(':indeterminate')]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const i = K('<input>').firstChild; i.value = 'x'; i.type = 'checkbox'; return [i.value, i.getAttribute('value'), i.checked]; })",
                r#""x,x,false""#,
            ),
            (
                r"E(() => { const i = K('<input>').firstChild; i.value = 'x'; i.type = 'hidden'; const r = [i.value, i.getAttribute('value')]; i.type = 'text'; r.push(i.value); return r; })",
                r#""x,x,x""#,
            ),
            (
                r"E(() => { const i = K('<input>').firstChild; i.value = 'x'; i.setAttribute('type', 'radio'); return [i.value, i.getAttribute('value')]; })",
                r#""x,x""#,
            ),
            (
                r"E(() => { const i = K('<input value=a>').firstChild; i.value = 'x'; i.type = 'submit'; return [i.value, i.getAttribute('value')]; })",
                r#""x,x""#,
            ),
            (
                r"E(() => { const i = K('<input>').firstChild; i.type = 'checkbox'; return [i.value, i.getAttribute('value')]; })",
                r#""on,""#,
            ),
            (
                r"E(() => { const i = K('<input>').firstChild; i.value = 'x'; i.type = 'file'; return [i.value, i.getAttribute('value')]; })",
                r#"",""#,
            ),
            (
                r"E(() => { const i = K('<input type=checkbox value=q>').firstChild; i.type = 'text'; const r = [i.value]; i.value = 'w'; i.type = 'radio'; r.push(i.value, i.getAttribute('value')); return r; })",
                r#""q,w,w""#,
            ),
            (
                r"E(() => { const s = K('<select id=fxs><optgroup id=fxg><option id=fxo1>a</optgroup><option id=fxo2>b</select>').firstChild; const n = document.createElement('option'); n.id = 'fxn'; try { s.add(n, s.querySelector('#fxo1')); } catch (e) { return e.name; } return ids(s.options); })",
                r#""NotFoundError""#,
            ),
            (
                r"E(() => { const s = K('<select id=fxs><optgroup id=fxg><option id=fxo1>a</optgroup><option id=fxo2>b</select>').firstChild; const n = document.createElement('option'); n.id = 'fxn'; s.add(n, s.querySelector('#fxo2')); return ids(s.options); })",
                r#""fxo1,fxn,fxo2""#,
            ),
            (
                r"E(() => { const s = K('<select id=fxs><optgroup id=fxg><option id=fxo1>a</optgroup><option id=fxo2>b</select>').firstChild; const n = document.createElement('option'); n.id = 'fxn'; s.add(n, 0); return [ids(s.options), s.firstChild.id]; })",
                r#""fxn,fxo1,fxo2,fxg""#,
            ),
            (
                r"E(() => { const s = K('<select id=fxs><optgroup id=fxg><option id=fxo1>a</optgroup><option id=fxo2>b</select>').firstChild; const n = document.createElement('option'); try { s.add(n, s.querySelector('#fxg')); } catch (e) { return e.name; } return s.firstChild.id; })",
                r#""""#,
            ),
        ],
    },
];
