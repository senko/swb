// The cases of `tools/probes/web-image-option.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-image-option.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::Case;

/// The page of the cases.
pub(crate) const PAGE: &str = r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=a class=k></div><select id=sel><option>1<option>2</select><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
const N = 'http://www.w3.org/2000/svg';
function X(t) { return document.createElementNS(N, t); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[Case {
    name: "forms-option-01",
    html: PAGE,
    queries: &[
        (
            r"E(() => { const o = new Option('a', 'b'); return [o.selected, o.defaultSelected].join(); })",
            r#""false,false""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true); return [o.selected, o.defaultSelected].join(); })",
            r#""false,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', false, true); return [o.selected, o.defaultSelected, o.getAttribute('selected')].join(); })",
            r#""true,false,""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, false); return [o.selected, o.defaultSelected, o.getAttribute('selected')].join(); })",
            r#""false,true,""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, undefined); return [o.selected, o.defaultSelected].join(); })",
            r#""false,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', false, 0); return [o.selected, o.defaultSelected].join(); })",
            r#""false,false""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1')); s.add(new Option('y', '2', false, true)); s.add(new Option('z', '3')); return [s.selectedIndex, s.value, s.options[0].selected, s.options[1].selected].join(); })",
            r#""1,2,false,true""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1')); s.add(new Option('y', '2', true)); s.add(new Option('z', '3')); return [s.selectedIndex, s.value, s.options[0].selected, s.options[1].selected].join(); })",
            r#""0,1,true,false""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1', true)); s.add(new Option('y', '2', true)); return [s.selectedIndex, s.options[0].selected, s.options[1].selected].join(); })",
            r#""0,true,false""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1', true, true)); s.add(new Option('y', '2', true, true)); return [s.selectedIndex, s.options[0].selected, s.options[1].selected].join(); })",
            r#""1,false,true""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1', false, true)); s.add(new Option('y', '2', false, true)); s.add(new Option('z', '3')); return [s.selectedIndex, s.options[0].selected, s.options[1].selected, s.options[2].selected].join(); })",
            r#""1,false,true,false""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1', false, false)); s.add(new Option('y', '2')); return [s.selectedIndex, s.options[0].selected].join(); })",
            r#""0,true""#,
        ),
        (
            r"E(() => { const s = document.createElement('select'); s.multiple = true; s.add(new Option('x', '1', false, true)); s.add(new Option('y', '2', false, true)); s.add(new Option('z', '3')); return [s.selectedIndex, s.selectedOptions.length, s.options[0].selected, s.options[1].selected].join(); })",
            r#""0,2,true,true""#,
        ),
        (
            r"E(() => { const s = document.getElementById('sel'); s.add(new Option('n', 'nv', false, true)); return [s.length, s.selectedIndex, s.value].join(); })",
            r#""3,2,nv""#,
        ),
        (
            r"E(() => { const s = document.getElementById('sel'); s.options[s.options.length] = new Option('n', 'nv', true); return [s.length, s.selectedIndex, s.value].join(); })",
            r#""4,2,nv""#,
        ),
        (
            r"E(() => { const s = document.getElementById('sel'); s.options.length = 0; s.add(new Option('p', 'pv')); s.add(new Option('q', 'qv')); s.value = 'qv'; return [s.length, s.selectedIndex, s.value, s.options[1].selected].join(); })",
            r#""2,1,qv,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b'); o.selected = true; return [o.selected, o.defaultSelected, o.hasAttribute('selected')].join(); })",
            r#""true,false,false""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true); o.defaultSelected = false; return [o.selected, o.defaultSelected, o.hasAttribute('selected')].join(); })",
            r#""false,false,false""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true); const s = document.createElement('select'); s.appendChild(o); s.appendChild(new Option('c')); return [s.selectedIndex, o.selected].join(); })",
            r#""0,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', false, true); const s = document.createElement('select'); s.appendChild(new Option('c')); s.appendChild(o); return [s.selectedIndex, o.selected].join(); })",
            r#""1,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', false, true); const s = document.createElement('select'); s.appendChild(new Option('c')); s.appendChild(o); document.body.appendChild(s); const r = [s.selectedIndex, o.selected]; s.remove(); return r.join(); })",
            r#""1,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, false); o.defaultSelected = false; o.defaultSelected = true; return [o.selected, o.defaultSelected].join(); })",
            r#""true,true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', false, false); o.defaultSelected = true; return [o.selected].join(); })",
            r#""true""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', false, true); o.defaultSelected = true; o.defaultSelected = false; return [o.selected].join(); })",
            r#""false""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, false); const s = document.getElementById('sel'); s.appendChild(o); o.defaultSelected = false; o.defaultSelected = true; return [o.selected, s.selectedIndex].join(); })",
            r#""true,2""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, false); o.selected = false; o.defaultSelected = false; o.defaultSelected = true; return [o.selected].join(); })",
            r#""false""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, false); o.setAttribute('selected', 'x'); return [o.selected].join(); })",
            r#""false""#,
        ),
        (
            r"E(() => { const o = new Option('a', 'b', true, false); o.removeAttribute('selected'); o.setAttribute('selected', ''); return [o.selected].join(); })",
            r#""true""#,
        ),
    ],
}];
