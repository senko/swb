// The cases of `tools/probes/web-computed-style.json` with the values that Chromium 148
// gave (`just probe --json tools/probes/web-computed-style.json`, three runs, all equal;
// the URLs of the probe's temporary directory are written `BASE/`); where
// swb departs on purpose, the test overrides the value. Generated.

use crate::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0 } #c { width: 400px; height: 300px; font-size: 20px; position: relative; color: rgb(0, 0, 255) }</style></head><body><div id=c></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 2 of the cases.
pub(crate) const PAGE_2: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } </style></head><body><div id=d style='--x: 1; color: rgb(1, 2, 3)'>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 3 of the cases.
pub(crate) const PAGE_3: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #a { color: red; background-color: rgba(0, 0, 255, 0.5); border: 1px solid; outline: 2px solid; text-decoration: underline } #b { color: transparent; border-top: 1px solid currentcolor } #c2 { color: hsl(120 100% 25%); background: #f008 } #k { color: inherit; border-bottom-color: currentcolor } svg { color: rgb(9, 8, 7) } #r { fill: currentcolor; stroke: rgb(1, 2, 3) } #t { color: rgba(10, 20, 30, 0.333); background-color: transparent; border-left-color: rgba(0, 0, 0, 0) }</style></head><body><div id=a>a<span id=k>k</span></div><div id=b>b</div><div id=c2>c</div><div id=t>t</div><svg width=10 height=10><rect id=r width=5 height=5 /></svg><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 4 of the cases.
pub(crate) const PAGE_4: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #box { width: 400px; padding: 10px; border: 5px solid; position: relative; font-size: 10px } #pct { width: 50%; margin: 10% auto 5%; padding: 5% 2%; height: 25%; min-width: 10%; max-width: 90% } #abs { position: absolute; left: 10%; top: auto; width: 30px; height: 20px; margin: 2%; } #abs2 { position: absolute; right: 5px; bottom: 10%; } #rel { position: relative; left: auto; right: 10px; top: 5% } #rel2 { position: relative; left: 5px; right: 10px; bottom: auto } #centered { width: 100px; margin: 0 auto; height: 3em } #inl { width: 100px; height: 100px; margin: 10%; padding: 5%; top: 5% } #bb { box-sizing: border-box; width: 100px; height: 50px; padding: 10px; border: 2px solid } #none { display: none; width: 50%; margin: 10%; height: auto } #nonechild { width: 10px; padding: 10%; left: 5% } #fixed { position: fixed; right: 0; bottom: 0; width: 10px; height: 10px } #sticky { position: sticky; top: 10%; height: 10px } #flex { display: flex; width: 300px; height: 40px } #item { flex: 1 } #item2 { width: 50px; margin: auto } #tf { width: 200px; height: 100px; transform: translate(50%, 10%) rotate(30deg); transform-origin: left 25% } #tf2 { width: 100px; height: 60px; transform: scale(2) } #ib { display: inline-block; width: 30%; height: 7px; padding: 1% } #cell { display: table-cell; width: 40px; padding: 2px } #lh { line-height: normal; height: auto } #lh2 { line-height: 1.5; font-size: 20px } #contents { display: contents; width: 10px; margin: 10% } </style></head><body><div id=box><div id=pct></div><div id=abs></div><div id=abs2>x</div><div id=rel></div><div id=rel2></div><div id=centered></div><span id=inl>x</span><div id=bb></div><div id=none><p id=nonechild></p></div><div id=fixed></div><div id=sticky></div><div id=flex><div id=item></div><div id=item2></div></div><div id=tf></div><div id=tf2></div><div id=ib></div><div id=cell></div><div id=lh>a</div><div id=lh2>b</div><div id=contents></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 5 of the cases.
pub(crate) const PAGE_5: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #g { display: grid; grid-template-columns: 1fr 2fr 30px; grid-template-rows: auto 20px; width: 330px; gap: 10px } #g2 { display: grid; grid-template-columns: repeat(2, 50px); grid-auto-rows: 15px } #ng { grid-template-columns: 1fr 1fr } #ga { display: grid; grid-template-areas: 'a b'; grid-template-columns: [x] 40px [y] 1fr; width: 100px }</style></head><body><div id=g><div>a</div><div></div><div></div><div></div></div><div id=g2><i></i><i></i><i></i></div><div id=ng></div><div id=ga><b></b><b></b></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 6 of the cases.
pub(crate) const PAGE_6: &str = r#"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #f1 { font: italic bold 12px/30px Arial, sans-serif } #f2 { font-family: 'Times New Roman', serif; font-size: 2em; font-weight: bolder } #f3 { font-size: larger; font-family: monospace } #f4 { font: 10px a b, 'c', "d e", sans-serif; font-stretch: condensed; font-variant: small-caps } #f5 { font-size: 1.5rem; line-height: 1.2em; letter-spacing: 0.1em; word-spacing: 1em; text-indent: 2em }</style></head><body><div id=f1>a</div><div id=f2>b</div><div id=f3>c</div><div id=f4>d</div><div id=f5>e</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>"#;

/// Page 7 of the cases.
pub(crate) const PAGE_7: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #s1 { margin: 1px 2px 3px 4px; padding: 1px 2px; border-width: 1px 2px; border-style: solid dashed; border-color: red blue; border-radius: 1px 2px 3px 4px / 5px; outline: 3px dotted green; overflow: hidden auto; list-style: square inside; flex-flow: column wrap; gap: 1px 2px; place-items: center start; inset: 1px 2px 3px 4px; position: relative } #s2 { border: 2px solid rgb(1, 2, 3); border-radius: 50%; background: url(a.png) no-repeat 10px 20px / 30px red; columns: 3 100px; grid-area: 1 / 2 / 3 / 4; text-decoration: underline dotted red; flex: 2 3 10% } #s3 { border-top: 1px solid red; background-position: right 5px bottom 10%; overflow: clip; list-style: none; border-spacing: 2px 3px; place-content: center; place-self: end; contain-intrinsic-size: 10px 20px; mask: none }</style></head><body><div id=s1>a</div><div id=s2>b</div><table id=s3><tr><td>c</td></tr></table><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 8 of the cases.
pub(crate) const PAGE_8: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #p { width: 200px; color: rgb(0, 128, 0) } #p::before { content: 'ab'; color: red; width: 50px; display: inline-block; padding: 10%; height: 1em } #p::after { content: none; color: blue } #q::after { content: counter(x) '.'; position: absolute; left: 10% } #q { position: relative; width: 100px } li::marker { color: rgb(1, 2, 3) } #p::first-line { color: rgb(4, 5, 6) } #hidden::before { content: 'x'; display: none } #p::selection { color: rgb(7, 8, 9) } input::placeholder { color: rgb(10, 11, 12) }</style></head><body><div id=p>text</div><div id=q></div><ul><li id=li>i</li></ul><div id=r></div><div id=hidden></div><input id=in placeholder=x><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 9 of the cases.
pub(crate) const PAGE_9: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } .x { color: rgb(1, 2, 3); width: 50% } #nd { display: none; color: rgb(9, 9, 9) } #nd span { width: 20px } [hidden] { color: rgb(5, 5, 5) } head, title, script { color: rgb(2, 2, 2) }</style></head><body><div id=nd><span id=ndc class=x>a</span></div><p id=h hidden>h</p><template id=tpl><p class=x>t</p></template><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 10 of the cases.
pub(crate) const PAGE_10: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #w { width: 100px } #w.wide { width: 300px; color: rgb(1, 1, 1) } [data-x=y] { color: rgb(2, 2, 2); margin-left: 7px } .a + p { color: rgb(3, 3, 3) } .a ~ #sib2 { width: 33px } #list > :first-child { color: rgb(4, 4, 4) } #par { width: 400px } #child { width: 50%; padding-left: 10% } #t1 { width: 120px } #t1 span { font-size: 20px }</style></head><body><div id=w></div><p id=sib1>s</p><p id=sib2>s</p><div id=list><p id=l1>1</p></div><div id=par><div id=child></div></div><div id=txt></div><div id=t1></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 11 of the cases.
pub(crate) const PAGE_11: &str = r"<html><head><title>t</title><style>#a { width: 50% } </style></head><body><div id=a style='height: 10'>a</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

/// Page 12 of the cases.
pub(crate) const PAGE_12: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #fx { display: flex; width: 200px } #gx { display: grid; width: 200px } .m { min-width: auto; min-height: auto; transform: translate(50%, 25%); width: 40px; height: 20px } #dn { display: none; transform: translate(50%); width: 40px } #sp { transform: translate(50%); transform-origin: 10% 20% } #am { position: absolute; left: 0; right: 0; width: 100px; margin: auto; top: 30px; height: 10px; margin-top: 5% } #rb { position: relative; width: 300px; height: 200px } #fl { float: left; width: 50px; margin: auto 10% } #ia { display: inline-block; width: 10px; margin: auto } #ov { overflow: auto; height: 50px; width: 100px } #ov > p { height: 200px; margin: 0; padding: 10% }</style></head><body><div id=fx><p class=m id=fi></p></div><div id=gx><p class=m id=gi></p></div><div id=dn></div><span id=sp>x</span><div id=rb><div id=am></div></div><div id=fl></div><span id=ia></span><div id=ov><p id=ovp></p></div><div id=plain></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(p, vs) { const c = document.getElementById('c'); return vs.map(v => { const e = document.createElement('div'); e.style.setProperty(p, v); c.appendChild(e); const r = getComputedStyle(e).getPropertyValue(p); e.remove(); return v + ' => ' + r; }); }
function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "values-longhand-colors",
        html: PAGE_1,
        queries: &[
            (
                r##"E(() => C("color", ["red", "RED", "#f00", "#FF0000", "#f008", "#ff000080", "rgb(255,0,0)", "rgb(255 0 0 / 50%)", "rgba(0,0,0,0.5)", "rgba(0,0,0,0)", "transparent", "currentColor", "CurrentColor", "hsl(120, 100%, 50%)", "hsla(120,100%,50%,0.3)", "rgb(10.5, 20, 30)", "rgba(0,0,0,0.123456)", "rgba(0,0,0,0.3)", "rgba(1,2,3,0.25)", "rgba(1,2,3,.1)", "rgb(300,-5,0)", "rgb(50%,0%,10%)", "foo", "#ggg", "Canvas", "canvastext", "-webkit-link", "lab(50% 20 30)", "oklch(0.5 0.1 120)", "color(srgb 1 0 0)", "color-mix(in srgb, red, blue)", "hwb(120 0% 0%)", "rgb(0 0 0 / 0.5)", "#ABCDEF", "aliceblue", "rgba(255,0,0,1)", "rgb(1,2,3,0.5)", "  red  ", "red !important"]))"##,
                r##"["red => rgb(255, 0, 0)", "RED => rgb(255, 0, 0)", "#f00 => rgb(255, 0, 0)", "#FF0000 => rgb(255, 0, 0)", "#f008 => rgba(255, 0, 0, 0.533)", "#ff000080 => rgba(255, 0, 0, 0.5)", "rgb(255,0,0) => rgb(255, 0, 0)", "rgb(255 0 0 / 50%) => rgba(255, 0, 0, 0.5)", "rgba(0,0,0,0.5) => rgba(0, 0, 0, 0.5)", "rgba(0,0,0,0) => rgba(0, 0, 0, 0)", "transparent => rgba(0, 0, 0, 0)", "currentColor => rgb(0, 0, 255)", "CurrentColor => rgb(0, 0, 255)", "hsl(120, 100%, 50%) => rgb(0, 255, 0)", "hsla(120,100%,50%,0.3) => rgba(0, 255, 0, 0.3)", "rgb(10.5, 20, 30) => rgb(11, 20, 30)", "rgba(0,0,0,0.123456) => rgba(0, 0, 0, 0.12)", "rgba(0,0,0,0.3) => rgba(0, 0, 0, 0.3)", "rgba(1,2,3,0.25) => rgba(1, 2, 3, 0.25)", "rgba(1,2,3,.1) => rgba(1, 2, 3, 0.1)", "rgb(300,-5,0) => rgb(255, 0, 0)", "rgb(50%,0%,10%) => rgb(128, 0, 26)", "foo => rgb(0, 0, 255)", "#ggg => rgb(0, 0, 255)", "Canvas => rgb(255, 255, 255)", "canvastext => rgb(0, 0, 0)", "-webkit-link => rgb(0, 0, 238)", "lab(50% 20 30) => lab(50 20 30)", "oklch(0.5 0.1 120) => oklch(0.5 0.1 120)", "color(srgb 1 0 0) => color(srgb 1 0 0)", "color-mix(in srgb, red, blue) => color(srgb 0.5 0 0.5)", "hwb(120 0% 0%) => rgb(0, 255, 0)", "rgb(0 0 0 / 0.5) => rgba(0, 0, 0, 0.5)", "#ABCDEF => rgb(171, 205, 239)", "aliceblue => rgb(240, 248, 255)", "rgba(255,0,0,1) => rgb(255, 0, 0)", "rgb(1,2,3,0.5) => rgba(1, 2, 3, 0.5)", "  red   => rgb(255, 0, 0)", "red !important => rgb(0, 0, 255)"]"##,
            ),
            (
                r##"E(() => C("background-color", ["red", "#0f0"]))"##,
                r##"["red => rgb(255, 0, 0)", "#0f0 => rgb(0, 255, 0)"]"##,
            ),
            (
                r#"E(() => C("border-top-color", ["currentcolor", "blue"]))"#,
                r#"["currentcolor => rgb(0, 0, 255)", "blue => rgb(0, 0, 255)"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-lengths",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("width", ["10px", "10PX", "0", "0.5em", ".5em", "10.0px", "1e2px", "50%", "auto", "calc(10px + 5px)", "calc(100% - 10px)", "calc(10px*2)", "calc( 50% + 2em )", "calc(100%/3)", "-10px", "10vw", "3rem", "1in", "1cm", "12pt", "1mm", "1q", "1pc", "2ex", "2ch", "min-content", "fit-content", "max-content", "10", "1.23456789px", "0.0001px", "1e-7px", "123456789px", "1234567.5px", "0.1px", "33.333333px", "+5px", "-0px", "0.000001px", "1e21px", "fit-content(10px)", "10px 20px", "stretch", "-webkit-fill-available", "min(10px, 5%)", "max(1px,2px)", "clamp(1px, 5%, 10px)"]))"#,
                r#"["10px => 10px", "10PX => 10px", "0 => 0px", "0.5em => 10px", ".5em => 10px", "10.0px => 10px", "1e2px => 100px", "50% => 200px", "auto => 400px", "calc(10px + 5px) => 15px", "calc(100% - 10px) => 390px", "calc(10px*2) => 20px", "calc( 50% + 2em ) => 240px", "calc(100%/3) => 133.328px", "-10px => 400px", "10vw => 80px", "3rem => 48px", "1in => 96px", "1cm => 37.7812px", "12pt => 16px", "1mm => 3.76562px", "1q => 0.9375px", "1pc => 16px", "2ex => 18.3594px", "2ch => 20px", "min-content => 0px", "fit-content => 0px", "max-content => 0px", "10 => 400px", "1.23456789px => 1.23438px", "0.0001px => 0px", "1e-7px => 0px", "123456789px => 3.35544e+07px", "1234567.5px => 1.23457e+06px", "0.1px => 0.09375px", "33.333333px => 33.3281px", "+5px => 5px", "-0px => 0px", "0.000001px => 0px", "1e21px => 3.35544e+07px", "fit-content(10px) => 400px", "10px 20px => 400px", "stretch => 400px", "-webkit-fill-available => 400px", "min(10px, 5%) => 10px", "max(1px,2px) => 2px", "clamp(1px, 5%, 10px) => 10px"]"#,
            ),
            (
                r#"E(() => C("margin-top", ["auto", "-10px", "0", "5%", "-0.5em"]))"#,
                r#"["auto => 0px", "-10px => -10px", "0 => 0px", "5% => 20px", "-0.5em => -10px"]"#,
            ),
            (
                r#"E(() => C("top", ["auto", "0", "-3px", "10%"]))"#,
                r#"["auto => auto", "0 => 0px", "-3px => -3px", "10% => 10%"]"#,
            ),
            (
                r#"E(() => C("min-width", ["auto", "0", "10px"]))"#,
                r#"["auto => 0px", "0 => 0px", "10px => 10px"]"#,
            ),
            (
                r#"E(() => C("max-height", ["none", "10px"]))"#,
                r#"["none => none", "10px => 10px"]"#,
            ),
            (
                r#"E(() => C("padding-left", ["0", "1px", "-1px"]))"#,
                r#"["0 => 0px", "1px => 1px", "-1px => 0px"]"#,
            ),
            (
                r#"E(() => C("border-top-width", ["thin", "medium", "thick", "0", "2px"]))"#,
                r#"["thin => 0px", "medium => 0px", "thick => 0px", "0 => 0px", "2px => 0px"]"#,
            ),
            (
                r#"E(() => C("letter-spacing", ["normal", "0", "1px", "0.1em"]))"#,
                r#"["normal => normal", "0 => normal", "1px => 1px", "0.1em => 2px"]"#,
            ),
            (
                r#"E(() => C("word-spacing", ["normal", "2px"]))"#,
                r#"["normal => 0px", "2px => 2px"]"#,
            ),
            (
                r#"E(() => C("text-indent", ["10px", "5%", "0"]))"#,
                r#"["10px => 10px", "5% => 5%", "0 => 0px"]"#,
            ),
            (
                r#"E(() => C("outline-offset", ["0", "2px"]))"#,
                r#"["0 => 0px", "2px => 2px"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-keywords",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("display", ["block", "BLOCK", "inline-block", "flex", "grid", "none", "contents", "table-cell", "list-item", "inline flex", "block flow", "inline-flex", "flow-root", "inline-grid", "table", "-webkit-box", "ruby", "math"]))"#,
                r#"["block => block", "BLOCK => block", "inline-block => inline-block", "flex => flex", "grid => grid", "none => none", "contents => contents", "table-cell => table-cell", "list-item => list-item", "inline flex => inline-flex", "block flow => block", "inline-flex => inline-flex", "flow-root => flow-root", "inline-grid => inline-grid", "table => table", "-webkit-box => -webkit-box", "ruby => ruby", "math => inline"]"#,
            ),
            (
                r#"E(() => C("position", ["absolute", "sticky", "-webkit-sticky", "fixed", "static", "relative"]))"#,
                r#"["absolute => absolute", "sticky => sticky", "-webkit-sticky => static", "fixed => fixed", "static => static", "relative => relative"]"#,
            ),
            (
                r#"E(() => C("float", ["left", "none", "inline-start"]))"#,
                r#"["left => left", "none => none", "inline-start => inline-start"]"#,
            ),
            (r#"E(() => C("clear", ["both"]))"#, r#"["both => both"]"#),
            (
                r#"E(() => C("visibility", ["hidden", "collapse"]))"#,
                r#"["hidden => hidden", "collapse => collapse"]"#,
            ),
            (
                r#"E(() => C("overflow-x", ["hidden", "auto", "clip", "overlay"]))"#,
                r#"["hidden => hidden", "auto => auto", "clip => clip", "overlay => auto"]"#,
            ),
            (
                r#"E(() => C("white-space", ["nowrap", "pre-wrap", "pre-line", "break-spaces", "normal", "pre"]))"#,
                r#"["nowrap => nowrap", "pre-wrap => pre-wrap", "pre-line => pre-line", "break-spaces => break-spaces", "normal => normal", "pre => pre"]"#,
            ),
            (
                r#"E(() => C("text-align", ["center", "-webkit-center", "start", "justify", "match-parent", "LEFT"]))"#,
                r#"["center => center", "-webkit-center => -webkit-center", "start => start", "justify => justify", "match-parent => start", "LEFT => left"]"#,
            ),
            (
                r#"E(() => C("cursor", ["pointer", "default", "url(a.png), auto", "url(a.png) 1 2, pointer", "grab", "zoom-in"]))"#,
                r#"["pointer => pointer", "default => default", "url(a.png), auto => url(\"BASE/a.png\"), auto", "url(a.png) 1 2, pointer => url(\"BASE/a.png\") 1 2, pointer", "grab => grab", "zoom-in => zoom-in"]"#,
            ),
            (
                r#"E(() => C("box-sizing", ["border-box"]))"#,
                r#"["border-box => border-box"]"#,
            ),
            (
                r#"E(() => C("text-transform", ["uppercase", "none"]))"#,
                r#"["uppercase => uppercase", "none => none"]"#,
            ),
            (
                r#"E(() => C("word-break", ["break-all", "break-word"]))"#,
                r#"["break-all => break-all", "break-word => break-word"]"#,
            ),
            (
                r#"E(() => C("overflow-wrap", ["anywhere", "break-word"]))"#,
                r#"["anywhere => anywhere", "break-word => break-word"]"#,
            ),
            (
                r#"E(() => C("pointer-events", ["none", "auto", "visiblePainted"]))"#,
                r#"["none => none", "auto => auto", "visiblePainted => visiblepainted"]"#,
            ),
            (
                r#"E(() => C("user-select", ["none", "text", "all"]))"#,
                r#"["none => none", "text => text", "all => all"]"#,
            ),
            (
                r#"E(() => C("-webkit-user-select", ["none"]))"#,
                r#"["none => none"]"#,
            ),
            (
                r#"E(() => C("vertical-align", ["middle", "-2px", "10%", "baseline", "text-top"]))"#,
                r#"["middle => middle", "-2px => -2px", "10% => 10%", "baseline => baseline", "text-top => text-top"]"#,
            ),
            (
                r#"E(() => C("text-overflow", ["ellipsis", "clip"]))"#,
                r#"["ellipsis => ellipsis", "clip => clip"]"#,
            ),
            (
                r#"E(() => C("table-layout", ["fixed"]))"#,
                r#"["fixed => fixed"]"#,
            ),
            (
                r#"E(() => C("border-collapse", ["collapse"]))"#,
                r#"["collapse => collapse"]"#,
            ),
            (
                r#"E(() => C("list-style-type", ["disc", "DECIMAL", "none", "'-'", "\"*\"", "lower-roman", "foo"]))"#,
                r#"["disc => disc", "DECIMAL => decimal", "none => none", "'-' => \"-\"", "\"*\" => \"*\"", "lower-roman => lower-roman", "foo => foo"]"#,
            ),
            (
                r#"E(() => C("list-style-position", ["inside"]))"#,
                r#"["inside => inside"]"#,
            ),
            (r#"E(() => C("direction", ["rtl"]))"#, r#"["rtl => rtl"]"#),
            (
                r#"E(() => C("unicode-bidi", ["isolate", "bidi-override"]))"#,
                r#"["isolate => isolate", "bidi-override => bidi-override"]"#,
            ),
            (
                r#"E(() => C("object-fit", ["cover"]))"#,
                r#"["cover => cover"]"#,
            ),
            (
                r#"E(() => C("scrollbar-width", ["none", "thin"]))"#,
                r#"["none => none", "thin => thin"]"#,
            ),
            (r#"E(() => C("hyphens", ["auto"]))"#, r#"["auto => auto"]"#),
            (
                r#"E(() => C("caption-side", ["bottom"]))"#,
                r#"["bottom => bottom"]"#,
            ),
            (
                r#"E(() => C("empty-cells", ["hide"]))"#,
                r#"["hide => hide"]"#,
            ),
            (
                r#"E(() => C("text-decoration-line", ["underline", "underline overline", "overline underline", "line-through underline", "none"]))"#,
                r#"["underline => underline", "underline overline => underline overline", "overline underline => underline overline", "line-through underline => underline line-through", "none => none"]"#,
            ),
            (
                r#"E(() => C("text-decoration-style", ["dotted"]))"#,
                r#"["dotted => dotted"]"#,
            ),
            (
                r#"E(() => C("flex-direction", ["column"]))"#,
                r#"["column => column"]"#,
            ),
            (
                r#"E(() => C("flex-wrap", ["wrap"]))"#,
                r#"["wrap => wrap"]"#,
            ),
            (
                r#"E(() => C("justify-content", ["center", "space-between", "flex-start", "safe center", "normal", "left"]))"#,
                r#"["center => center", "space-between => space-between", "flex-start => flex-start", "safe center => safe center", "normal => normal", "left => left"]"#,
            ),
            (
                r#"E(() => C("align-items", ["center", "baseline", "first baseline", "last baseline", "stretch", "normal"]))"#,
                r#"["center => center", "baseline => baseline", "first baseline => baseline", "last baseline => last baseline", "stretch => stretch", "normal => normal"]"#,
            ),
            (
                r#"E(() => C("align-self", ["auto", "flex-end"]))"#,
                r#"["auto => auto", "flex-end => flex-end"]"#,
            ),
            (
                r#"E(() => C("align-content", ["space-around"]))"#,
                r#"["space-around => space-around"]"#,
            ),
            (
                r#"E(() => C("justify-items", ["legacy center", "center legacy", "start"]))"#,
                r#"["legacy center => legacy center", "center legacy => legacy center", "start => start"]"#,
            ),
            (
                r#"E(() => C("justify-self", ["end"]))"#,
                r#"["end => end"]"#,
            ),
            (
                r#"E(() => C("contain", ["strict", "content", "layout paint", "paint layout", "size layout style paint"]))"#,
                r#"["strict => strict", "content => content", "layout paint => layout paint", "paint layout => layout paint", "size layout style paint => strict"]"#,
            ),
            (
                r#"E(() => C("border-top-style", ["solid", "none", "hidden", "double", "inset"]))"#,
                r#"["solid => solid", "none => none", "hidden => hidden", "double => double", "inset => inset"]"#,
            ),
            (
                r#"E(() => C("outline-style", ["auto", "solid"]))"#,
                r#"["auto => auto", "solid => solid"]"#,
            ),
            (
                r#"E(() => C("font-style", ["italic", "oblique", "oblique 10deg", "normal"]))"#,
                r#"["italic => italic", "oblique => italic", "oblique 10deg => oblique 10deg", "normal => normal"]"#,
            ),
            (
                r#"E(() => C("font-variant-caps", ["small-caps", "all-small-caps"]))"#,
                r#"["small-caps => small-caps", "all-small-caps => all-small-caps"]"#,
            ),
            (
                r#"E(() => C("font-stretch", ["condensed", "75%", "normal", "ultra-expanded", "150.5%"]))"#,
                r#"["condensed => 75%", "75% => 75%", "normal => 100%", "ultra-expanded => 200%", "150.5% => 150.5%"]"#,
            ),
            (
                r#"E(() => C("background-attachment", ["fixed", "scroll, local"]))"#,
                r#"["fixed => fixed", "scroll, local => scroll"]"#,
            ),
            (
                r#"E(() => C("background-repeat", ["no-repeat", "repeat-x", "repeat-y", "repeat no-repeat", "repeat repeat", "space round", "no-repeat, repeat"]))"#,
                r#"["no-repeat => no-repeat", "repeat-x => repeat-x", "repeat-y => repeat-y", "repeat no-repeat => repeat-x", "repeat repeat => repeat", "space round => space round", "no-repeat, repeat => no-repeat"]"#,
            ),
            (
                r#"E(() => C("background-origin", ["content-box"]))"#,
                r#"["content-box => content-box"]"#,
            ),
            (
                r#"E(() => C("background-clip", ["text", "padding-box"]))"#,
                r#"["text => text", "padding-box => padding-box"]"#,
            ),
            (
                r#"E(() => C("fill-rule", ["evenodd"]))"#,
                r#"["evenodd => evenodd"]"#,
            ),
            (
                r#"E(() => C("stroke-linecap", ["round"]))"#,
                r#"["round => round"]"#,
            ),
            (
                r#"E(() => C("shape-rendering", ["crispEdges"]))"#,
                r#"["crispEdges => crispedges"]"#,
            ),
            (
                r#"E(() => C("mask-mode", ["alpha"]))"#,
                r#"["alpha => alpha"]"#,
            ),
            (
                r#"E(() => C("mask-type", ["alpha"]))"#,
                r#"["alpha => alpha"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-numbers",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("opacity", ["0.5", ".5", "50%", "1.0", "2", "-1", "0", "1", "0.333333333", "1e-3", "calc(0.5 + 0.1)"]))"#,
                r#"["0.5 => 0.5", ".5 => 0.5", "50% => 0.5", "1.0 => 1", "2 => 1", "-1 => 0", "0 => 0", "1 => 1", "0.333333333 => 0.333333", "1e-3 => 0.001", "calc(0.5 + 0.1) => 0.6"]"#,
            ),
            (
                r#"E(() => C("z-index", ["10", "auto", "1.5", "-1", "+3", "0", "2147483648", "1e3"]))"#,
                r#"["10 => 10", "auto => auto", "1.5 => auto", "-1 => -1", "+3 => 3", "0 => 0", "2147483648 => 2147483647", "1e3 => auto"]"#,
            ),
            (
                r#"E(() => C("flex-grow", ["1", "1.5", "0", "-1"]))"#,
                r#"["1 => 1", "1.5 => 1.5", "0 => 0", "-1 => 0"]"#,
            ),
            (
                r#"E(() => C("flex-shrink", ["0", "2"]))"#,
                r#"["0 => 0", "2 => 2"]"#,
            ),
            (
                r#"E(() => C("order", ["-1", "3"]))"#,
                r#"["-1 => -1", "3 => 3"]"#,
            ),
            (
                r#"E(() => C("line-height", ["1.5", "normal", "20px", "150%", "0", "1", "1.25em", "-1"]))"#,
                r#"["1.5 => 30px", "normal => normal", "20px => 20px", "150% => 30px", "0 => 0px", "1 => 20px", "1.25em => 25px", "-1 => normal"]"#,
            ),
            (
                r#"E(() => C("font-weight", ["bold", "700", "550", "normal", "bolder", "lighter", "100", "1000", "0", "1", "400.5"]))"#,
                r#"["bold => 700", "700 => 700", "550 => 550", "normal => 400", "bolder => 700", "lighter => 100", "100 => 100", "1000 => 1000", "0 => 400", "1 => 1", "400.5 => 400.5"]"#,
            ),
            (
                r#"E(() => C("tab-size", ["4", "10px", "0"]))"#,
                r#"["4 => 4", "10px => 10px", "0 => 0"]"#,
            ),
            (
                r#"E(() => C("column-count", ["auto", "3"]))"#,
                r#"["auto => auto", "3 => 3"]"#,
            ),
            (
                r#"E(() => C("fill-opacity", ["0.5", "50%"]))"#,
                r#"["0.5 => 0.5", "50% => 0.5"]"#,
            ),
            (
                r#"E(() => C("stroke-width", ["1", "2px", "5%"]))"#,
                r#"["1 => 1px", "2px => 2px", "5% => 5%"]"#,
            ),
            (r#"E(() => C("stroke-miterlimit", ["4"]))"#, r#"["4 => 4"]"#),
            (
                r#"E(() => C("stroke-dasharray", ["none", "1 2", "1,2", "5px 10%"]))"#,
                r#"["none => none", "1 2 => 1px, 2px", "1,2 => 1px, 2px", "5px 10% => 5px, 10%"]"#,
            ),
            (
                r#"E(() => C("aspect-ratio", ["auto", "16 / 9", "16/9", "2", "auto 1/1", "1 / 1 auto", "0.5"]))"#,
                r#"["auto => auto", "16 / 9 => 16 / 9", "16/9 => 16 / 9", "2 => 2 / 1", "auto 1/1 => auto 1 / 1", "1 / 1 auto => auto 1 / 1", "0.5 => 0.5 / 1"]"#,
            ),
            (
                r#"E(() => C("font-size", ["12px", "medium", "larger", "120%", "1.2em", "XX-Small", "xxx-large", "0", "smaller", "math"]))"#,
                r#"["12px => 12px", "medium => 16px", "larger => 24px", "120% => 24px", "1.2em => 24px", "XX-Small => 9px", "xxx-large => 48px", "0 => 0px", "smaller => 16.6667px", "math => 20px"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-fonts",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("font-family", ["Arial", "\"Times New Roman\"", "sans-serif", "'a b'", "Arial  ,  Helvetica", "Helvetica Neue", "\"Arial\"", "SANS-SERIF", "'sans-serif'", "system-ui", "-apple-system", "BlinkMacSystemFont, 'Segoe UI', Roboto", "a\\,b", "\"a\\\"b\"", "1abc", "serif, monospace"]))"#,
                r#"["Arial => Arial", "\"Times New Roman\" => \"Times New Roman\"", "sans-serif => sans-serif", "'a b' => \"a b\"", "Arial  ,  Helvetica => Arial, Helvetica", "Helvetica Neue => \"Helvetica Neue\"", "\"Arial\" => Arial", "SANS-SERIF => sans-serif", "'sans-serif' => \"sans-serif\"", "system-ui => system-ui", "-apple-system => -apple-system", "BlinkMacSystemFont, 'Segoe UI', Roboto => BlinkMacSystemFont, \"Segoe UI\", Roboto", "a\\,b => \"a,b\"", "\"a\\\"b\" => \"a\\\"b\"", "1abc => \"Times New Roman\"", "serif, monospace => serif, monospace"]"#,
            ),
            (
                r#"E(() => C("font-feature-settings", ["normal", "\"liga\" 0", "'smcp', 'kern' 1", "\"liga\" on"]))"#,
                r#"["normal => normal", "\"liga\" 0 => \"liga\" 0", "'smcp', 'kern' 1 => \"kern\", \"smcp\"", "\"liga\" on => \"liga\""]"#,
            ),
            (
                r#"E(() => C("font-variation-settings", ["normal", "\"wght\" 400", "'wdth' 75.5"]))"#,
                r#"["normal => normal", "\"wght\" 400 => \"wght\" 400", "'wdth' 75.5 => \"wdth\" 75.5"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-transforms",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("transform", ["none", "translate(10px, 20px)", "translate(10px,20px)", "translateX(10px)", "translate(10px)", "rotate(45deg)", "scale(2)", "scale(1.5, 2)", "matrix(1,0,0,1,0,0)", "translate3d(1px,2px,3px)", "translate3d(10px, 20px, 0)", "rotate(0.5turn)", "skewX(10deg)", "translate(0)", "translateX(0)", "translate(10px) rotate(5deg)", "TRANSLATE(1PX)", "rotate(0)", "scale3d(1,2,3)", "matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1)", "perspective(100px)", "translateY(-50%)", "rotateX(10deg)", "rotateZ(1rad)", "skew(10deg, 20deg)", "skew(10deg)", "scaleX(-1)", "translate(calc(10px + 5px))", "rotate(90grad)"]))"#,
                r#"["none => none", "translate(10px, 20px) => matrix(1, 0, 0, 1, 10, 20)", "translate(10px,20px) => matrix(1, 0, 0, 1, 10, 20)", "translateX(10px) => matrix(1, 0, 0, 1, 10, 0)", "translate(10px) => matrix(1, 0, 0, 1, 10, 0)", "rotate(45deg) => matrix(0.707107, 0.707107, -0.707107, 0.707107, 0, 0)", "scale(2) => matrix(2, 0, 0, 2, 0, 0)", "scale(1.5, 2) => matrix(1.5, 0, 0, 2, 0, 0)", "matrix(1,0,0,1,0,0) => matrix(1, 0, 0, 1, 0, 0)", "translate3d(1px,2px,3px) => matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 1)", "translate3d(10px, 20px, 0) => matrix(1, 0, 0, 1, 10, 20)", "rotate(0.5turn) => matrix(-1, 0, 0, -1, 0, 0)", "skewX(10deg) => matrix(1, 0, 0.176327, 1, 0, 0)", "translate(0) => matrix(1, 0, 0, 1, 0, 0)", "translateX(0) => matrix(1, 0, 0, 1, 0, 0)", "translate(10px) rotate(5deg) => matrix(0.996195, 0.0871557, -0.0871557, 0.996195, 10, 0)", "TRANSLATE(1PX) => matrix(1, 0, 0, 1, 1, 0)", "rotate(0) => matrix(1, 0, 0, 1, 0, 0)", "scale3d(1,2,3) => matrix3d(1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 1)", "matrix3d(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1) => matrix(1, 0, 0, 1, 0, 0)", "perspective(100px) => matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, -0.01, 0, 0, 0, 1)", "translateY(-50%) => matrix(1, 0, 0, 1, 0, 0)", "rotateX(10deg) => matrix3d(1, 0, 0, 0, 0, 0.984808, 0.173648, 0, 0, -0.173648, 0.984808, 0, 0, 0, 0, 1)", "rotateZ(1rad) => matrix(0.540302, 0.841471, -0.841471, 0.540302, 0, 0)", "skew(10deg, 20deg) => matrix(1, 0.36397, 0.176327, 1, 0, 0)", "skew(10deg) => matrix(1, 0, 0.176327, 1, 0, 0)", "scaleX(-1) => matrix(-1, 0, 0, 1, 0, 0)", "translate(calc(10px + 5px)) => matrix(1, 0, 0, 1, 15, 0)", "rotate(90grad) => matrix(0.156434, 0.987688, -0.987688, 0.156434, 0, 0)"]"#,
            ),
            (
                r#"E(() => C("-webkit-transform", ["rotate(1deg)"]))"#,
                r#"["rotate(1deg) => matrix(0.999848, 0.0174524, -0.0174524, 0.999848, 0, 0)"]"#,
            ),
            (
                r#"E(() => C("transform-origin", ["center", "left top", "top left", "10px 20px", "50% 50% 0", "50% 50%", "right", "bottom", "0 0", "center center", "left 10px", "10px 20px 5px"]))"#,
                r#"["center => 200px 0px", "left top => 0px 0px", "top left => 0px 0px", "10px 20px => 10px 20px", "50% 50% 0 => 200px 0px", "50% 50% => 200px 0px", "right => 400px 0px", "bottom => 200px 0px", "0 0 => 0px 0px", "center center => 200px 0px", "left 10px => 0px 10px", "10px 20px 5px => 10px 20px 5px"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-images",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("background-image", ["url(a.png)", "url(\"a.png\")", "url('a b.png')", "none", "linear-gradient(red, blue)", "linear-gradient(to right, red 0%, blue 100%)", "linear-gradient(45deg,red,blue)", "radial-gradient(circle, red, blue)", "url(a.png), none", "url(data:image/png;base64,AA==)", "url()", "image-set(url(a.png) 1x)", "-webkit-linear-gradient(top, red, blue)", "repeating-linear-gradient(red 0, blue 10px)", "conic-gradient(red, blue)", "linear-gradient(rgba(0,0,0,.5), #fff)"]))"#,
                r#"["url(a.png) => url(\"BASE/a.png\")", "url(\"a.png\") => url(\"BASE/a.png\")", "url('a b.png') => url(\"BASE/a%20b.png\")", "none => none", "linear-gradient(red, blue) => linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255))", "linear-gradient(to right, red 0%, blue 100%) => linear-gradient(to right, rgb(255, 0, 0) 0%, rgb(0, 0, 255) 100%)", "linear-gradient(45deg,red,blue) => linear-gradient(45deg, rgb(255, 0, 0), rgb(0, 0, 255))", "radial-gradient(circle, red, blue) => radial-gradient(circle, rgb(255, 0, 0), rgb(0, 0, 255))", "url(a.png), none => url(\"BASE/a.png\"), none", "url(data:image/png;base64,AA==) => url(\"data:image/png;base64,AA==\")", "url() => url(\"\")", "image-set(url(a.png) 1x) => image-set(url(\"BASE/a.png\") 1dppx)", "-webkit-linear-gradient(top, red, blue) => -webkit-linear-gradient(top, rgb(255, 0, 0), rgb(0, 0, 255))", "repeating-linear-gradient(red 0, blue 10px) => repeating-linear-gradient(rgb(255, 0, 0) 0px, rgb(0, 0, 255) 10px)", "conic-gradient(red, blue) => conic-gradient(rgb(255, 0, 0), rgb(0, 0, 255))", "linear-gradient(rgba(0,0,0,.5), #fff) => linear-gradient(rgba(0, 0, 0, 0.5), rgb(255, 255, 255))"]"#,
            ),
            (
                r#"E(() => C("list-style-image", ["none", "url(a.png)"]))"#,
                r#"["none => none", "url(a.png) => url(\"BASE/a.png\")"]"#,
            ),
            (
                r#"E(() => C("mask-image", ["url(a.svg)", "none"]))"#,
                r#"["url(a.svg) => url(\"BASE/a.svg\")", "none => none"]"#,
            ),
            (
                r#"E(() => C("content", ["\"a\"", "none", "normal", "counter(x)", "attr(title)", "\"a\" \"b\"", "url(a.png)", "counters(x, '.')", "open-quote", "''", "\"\\\"\"", "\"a\" attr(x) counter(y, upper-roman)", "\"x\" / \"alt\""]))"#,
                r#"["\"a\" => \"a\"", "none => normal", "normal => normal", "counter(x) => counter(x)", "attr(title) => \"\"", "\"a\" \"b\" => \"ab\"", "url(a.png) => url(\"BASE/a.png\")", "counters(x, '.') => counters(x, \".\")", "open-quote => open-quote", "'' => \"\"", "\"\\\"\" => \"\\\"\"", "\"a\" attr(x) counter(y, upper-roman) => \"a\" counter(y, upper-roman)", "\"x\" / \"alt\" => \"x\" / \"alt\""]"#,
            ),
            (
                r#"E(() => C("counter-reset", ["x 1", "x", "none", "x 1 y", "x -1", "list-item"]))"#,
                r#"["x 1 => x 1", "x => x 0", "none => none", "x 1 y => x 1 y 0", "x -1 => x -1", "list-item => list-item 0"]"#,
            ),
            (
                r#"E(() => C("counter-increment", ["x", "x 2"]))"#,
                r#"["x => x 1", "x 2 => x 2"]"#,
            ),
            (
                r#"E(() => C("clip-path", ["none", "inset(10px)", "circle(50%)", "url(#a)", "polygon(0 0, 10px 0, 0 10px)", "inset(1px 2px round 3px)"]))"#,
                r##"["none => none", "inset(10px) => inset(10px)", "circle(50%) => circle(50%)", "url(#a) => url(\"#a\")", "polygon(0 0, 10px 0, 0 10px) => polygon(0px 0px, 10px 0px, 0px 10px)", "inset(1px 2px round 3px) => inset(1px 2px round 3px)"]"##,
            ),
            (
                r#"E(() => C("clip", ["rect(1px, 2px, 3px, 4px)", "rect(1px 2px 3px 4px)", "auto", "rect(auto, auto, auto, auto)"]))"#,
                r#"["rect(1px, 2px, 3px, 4px) => rect(1px, 2px, 3px, 4px)", "rect(1px 2px 3px 4px) => rect(1px, 2px, 3px, 4px)", "auto => auto", "rect(auto, auto, auto, auto) => rect(auto, auto, auto, auto)"]"#,
            ),
            (
                r#"E(() => C("fill", ["red", "none", "url(#g)", "url(#g) blue", "context-fill", "currentcolor"]))"#,
                r##"["red => rgb(255, 0, 0)", "none => none", "url(#g) => url(\"#g\")", "url(#g) blue => url(\"#g\") rgb(0, 0, 255)", "context-fill => context-fill", "currentcolor => rgb(0, 0, 255)"]"##,
            ),
            (
                r##"E(() => C("stroke", ["none", "#000"]))"##,
                r##"["none => none", "#000 => rgb(0, 0, 0)"]"##,
            ),
        ],
    },
    Case {
        name: "values-longhand-positions",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("background-position-x", ["left", "10px", "50%", "right 10px", "center", "left, right"]))"#,
                r#"["left => 0%", "10px => 10px", "50% => 50%", "right 10px => calc(100% - 10px)", "center => 50%", "left, right => 0%"]"#,
            ),
            (
                r#"E(() => C("background-position-y", ["top", "bottom 5px"]))"#,
                r#"["top => 0%", "bottom 5px => calc(100% - 5px)"]"#,
            ),
            (
                r#"E(() => C("background-size", ["cover", "contain", "10px auto", "auto", "auto auto", "50% 50%", "10px", "cover, 10px 20px"]))"#,
                r#"["cover => cover", "contain => contain", "10px auto => 10px", "auto => auto", "auto auto => auto", "50% 50% => 50% 50%", "10px => 10px", "cover, 10px 20px => cover"]"#,
            ),
            (
                r#"E(() => C("object-position", ["center", "left top", "10px 20px", "right 10px bottom 5px", "50% 50%"]))"#,
                r#"["center => 50% 50%", "left top => 0% 0%", "10px 20px => 10px 20px", "right 10px bottom 5px => calc(100% - 10px) calc(100% - 5px)", "50% 50% => 50% 50%"]"#,
            ),
            (
                r#"E(() => C("mask-size", ["contain"]))"#,
                r#"["contain => contain"]"#,
            ),
            (
                r#"E(() => C("mask-repeat", ["no-repeat"]))"#,
                r#"["no-repeat => no-repeat"]"#,
            ),
            (
                r#"E(() => C("mask-composite", ["add"]))"#,
                r#"["add => add"]"#,
            ),
            (
                r#"E(() => C("mask-origin", ["content-box"]))"#,
                r#"["content-box => content-box"]"#,
            ),
            (
                r#"E(() => C("mask-clip", ["no-clip"]))"#,
                r#"["no-clip => no-clip"]"#,
            ),
            (
                r#"E(() => C("-webkit-mask-position-x", ["left"]))"#,
                r#"["left => 0%"]"#,
            ),
            (
                r#"E(() => C("border-top-left-radius", ["5px", "5px 10px", "5px 5px", "50%", "0"]))"#,
                r#"["5px => 5px", "5px 10px => 5px 10px", "5px 5px => 5px", "50% => 50%", "0 => 0px"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-grid",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("grid-template-columns", ["1fr 1fr", "repeat(3, 1fr)", "100px auto", "none", "[a] 10px [b c] 1fr", "minmax(100px, 1fr) 2fr", "repeat(auto-fill, minmax(100px, 1fr))", "subgrid", "fit-content(100px)", "repeat(2, [a] 10px)", "masonry"]))"#,
                r#"["1fr 1fr => 1fr 1fr", "repeat(3, 1fr) => repeat(3, 1fr)", "100px auto => 100px auto", "none => none", "[a] 10px [b c] 1fr => [a] 10px [b c] 1fr", "minmax(100px, 1fr) 2fr => minmax(100px, 1fr) 2fr", "repeat(auto-fill, minmax(100px, 1fr)) => repeat(auto-fill, minmax(100px, 1fr))", "subgrid => subgrid", "fit-content(100px) => fit-content(100px)", "repeat(2, [a] 10px) => repeat(2, [a] 10px)", "masonry => none"]"#,
            ),
            (
                r#"E(() => C("grid-template-rows", ["auto", "10px 20px"]))"#,
                r#"["auto => auto", "10px 20px => 10px 20px"]"#,
            ),
            (
                r#"E(() => C("grid-template-areas", ["none", "\"a b\" \"c d\"", "'a  b'  'c d'", "\"a . b\"", "\"a ... b\""]))"#,
                r#"["none => none", "\"a b\" \"c d\" => \"a b\" \"c d\"", "'a  b'  'c d' => \"a b\" \"c d\"", "\"a . b\" => \"a . b\"", "\"a ... b\" => \"a . b\""]"#,
            ),
            (
                r#"E(() => C("grid-auto-columns", ["1fr", "100px 200px", "minmax(10px, auto)"]))"#,
                r#"["1fr => 1fr", "100px 200px => 100px 200px", "minmax(10px, auto) => minmax(10px, auto)"]"#,
            ),
            (
                r#"E(() => C("grid-auto-rows", ["auto"]))"#,
                r#"["auto => auto"]"#,
            ),
            (
                r#"E(() => C("grid-auto-flow", ["row", "column", "dense", "row dense", "dense column", "column dense"]))"#,
                r#"["row => row", "column => column", "dense => dense", "row dense => dense", "dense column => column dense", "column dense => column dense"]"#,
            ),
            (
                r#"E(() => C("grid-row-start", ["1", "auto", "span 2", "a", "a 2", "2 a", "span a", "-1", "span 2 a"]))"#,
                r#"["1 => 1", "auto => auto", "span 2 => span 2", "a => a", "a 2 => 2 a", "2 a => 2 a", "span a => span a", "-1 => -1", "span 2 a => span 2 a"]"#,
            ),
            (
                r#"E(() => C("grid-column-end", ["span 3", "auto"]))"#,
                r#"["span 3 => span 3", "auto => auto"]"#,
            ),
            (
                r#"E(() => C("row-gap", ["normal", "10px", "5%"]))"#,
                r#"["normal => normal", "10px => 10px", "5% => 5%"]"#,
            ),
            (
                r#"E(() => C("column-gap", ["normal", "1em"]))"#,
                r#"["normal => normal", "1em => 20px"]"#,
            ),
            (
                r#"E(() => C("column-width", ["auto", "100px"]))"#,
                r#"["auto => auto", "100px => 100px"]"#,
            ),
            (
                r#"E(() => C("contain-intrinsic-width", ["none", "100px", "auto 100px", "auto none"]))"#,
                r#"["none => none", "100px => 100px", "auto 100px => auto 100px", "auto none => auto none"]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-custom",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("--x", ["10px", "  a  b  ", "red", "{a:b}", "", "  ", "  10px  /* c */  ", "var(--y)", "initial", "INHERIT", "a!important"]))"#,
                r#"["10px => 10px", "  a  b   => a  b", "red => red", "{a:b} => {a:b}", " => ", "   => ", "  10px  /* c */   => 10px", "var(--y) => ", "initial => ", "INHERIT => ", "a!important => "]"#,
            ),
            (r#"E(() => C("--Foo", ["bar"]))"#, r#"["bar => bar"]"#),
            (
                r#"E(() => C("color", ["var(--c)", "var(--c, red)", "VAR(--c)", "  var( --c )  ", "env(foo)", "var(--c) !important"]))"#,
                r#"["var(--c) => rgb(0, 0, 255)", "var(--c, red) => rgb(255, 0, 0)", "VAR(--c) => rgb(0, 0, 255)", "  var( --c )   => rgb(0, 0, 255)", "env(foo) => rgb(0, 0, 255)", "var(--c) !important => rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("width", ["calc(var(--w) * 2)"]))"#,
                r#"["calc(var(--w) * 2) => 400px"]"#,
            ),
            (
                r#"E(() => C("margin", ["var(--m)", "1px var(--m)"]))"#,
                r#"["var(--m) => 0px", "1px var(--m) => 0px"]"#,
            ),
            (
                r#"E(() => C("border", ["var(--b)"]))"#,
                r#"["var(--b) => 0px none rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("font", ["var(--f)"]))"#,
                r#"["var(--f) => 20px \"Times New Roman\""]"#,
            ),
        ],
    },
    Case {
        name: "values-longhand-wide",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("color", ["inherit", "INITIAL", "unset", "revert", "revert-layer", "inherit inherit", "default"]))"#,
                r#"["inherit => rgb(0, 0, 255)", "INITIAL => rgb(0, 0, 0)", "unset => rgb(0, 0, 255)", "revert => rgb(0, 0, 255)", "revert-layer => rgb(0, 0, 255)", "inherit inherit => rgb(0, 0, 255)", "default => rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("margin", ["inherit", "initial", "unset", "revert"]))"#,
                r#"["inherit => 0px", "initial => 0px", "unset => 0px", "revert => 0px"]"#,
            ),
            (
                r#"E(() => C("border", ["initial"]))"#,
                r#"["initial => 0px none rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("background", ["inherit"]))"#,
                r#"["inherit => rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box"]"#,
            ),
            (
                r#"E(() => C("font", ["inherit"]))"#,
                r#"["inherit => 20px \"Times New Roman\""]"#,
            ),
            (r#"E(() => C("--x", ["inherit"]))"#, r#"["inherit => "]"#),
        ],
    },
    Case {
        name: "values-shorthands",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => C("margin", ["1px", "1px 2px", "1px 2px 3px", "1px 2px 3px 4px", "auto", "0", "1px 1px 1px 1px", "1px 2px 1px 2px", "1px 2px 3px 2px", "0 auto", "1px 2px 3px 4px 5px", "1px,2px"]))"#,
                r#"["1px => 1px", "1px 2px => 1px 2px", "1px 2px 3px => 1px 2px 3px", "1px 2px 3px 4px => 1px 2px 3px 4px", "auto => 0px", "0 => 0px", "1px 1px 1px 1px => 1px", "1px 2px 1px 2px => 1px 2px", "1px 2px 3px 2px => 1px 2px 3px", "0 auto => 0px", "1px 2px 3px 4px 5px => 0px", "1px,2px => 0px"]"#,
            ),
            (
                r#"E(() => C("padding", ["1px 2px", "0", "-1px"]))"#,
                r#"["1px 2px => 1px 2px", "0 => 0px", "-1px => 0px"]"#,
            ),
            (
                r#"E(() => C("inset", ["0", "1px auto", "auto"]))"#,
                r#"["0 => 0px", "1px auto => 1px auto", "auto => auto"]"#,
            ),
            (
                r#"E(() => C("margin-block", ["1px", "1px 2px", "auto 1px"]))"#,
                r#"["1px => 1px", "1px 2px => 1px 2px", "auto 1px => 0px 1px"]"#,
            ),
            (
                r#"E(() => C("margin-inline", ["1px 2px"]))"#,
                r#"["1px 2px => 1px 2px"]"#,
            ),
            (
                r#"E(() => C("padding-inline", ["3px"]))"#,
                r#"["3px => 3px"]"#,
            ),
            (
                r#"E(() => C("inset-inline", ["1px"]))"#,
                r#"["1px => 1px"]"#,
            ),
            (
                r#"E(() => C("border", ["1px solid red", "solid", "1px", "none", "2px dashed", "red", "0", "medium none currentcolor", "1px solid", "solid red 1px", "thin solid #f00", "1px solid rgba(0,0,0,.1)", "1px 2px solid"]))"#,
                r#"["1px solid red => 1px solid rgb(255, 0, 0)", "solid => 3px solid rgb(0, 0, 255)", "1px => 0px none rgb(0, 0, 255)", "none => 0px none rgb(0, 0, 255)", "2px dashed => 2px dashed rgb(0, 0, 255)", "red => 0px none rgb(255, 0, 0)", "0 => 0px none rgb(0, 0, 255)", "medium none currentcolor => 0px none rgb(0, 0, 255)", "1px solid => 1px solid rgb(0, 0, 255)", "solid red 1px => 1px solid rgb(255, 0, 0)", "thin solid #f00 => 1px solid rgb(255, 0, 0)", "1px solid rgba(0,0,0,.1) => 1px solid rgba(0, 0, 0, 0.1)", "1px 2px solid => 0px none rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("border-top", ["1px solid red", "none", "2px", "0"]))"#,
                r#"["1px solid red => 1px solid rgb(255, 0, 0)", "none => 0px none rgb(0, 0, 255)", "2px => 0px none rgb(0, 0, 255)", "0 => 0px none rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("border-left", ["thick double blue"]))"#,
                r#"["thick double blue => 5px double rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("border-width", ["1px 2px", "1px", "thin medium", "1px 2px 3px 4px"]))"#,
                r#"["1px 2px => 0px", "1px => 0px", "thin medium => 0px", "1px 2px 3px 4px => 0px"]"#,
            ),
            (
                r#"E(() => C("border-style", ["solid dashed", "none"]))"#,
                r#"["solid dashed => solid dashed", "none => none"]"#,
            ),
            (
                r#"E(() => C("border-color", ["red blue", "red", "red #f00", "currentcolor"]))"#,
                r#"["red blue => rgb(255, 0, 0) rgb(0, 0, 255)", "red => rgb(255, 0, 0)", "red #f00 => rgb(255, 0, 0)", "currentcolor => rgb(0, 0, 255)"]"#,
            ),
            (
                r#"E(() => C("border-block", ["1px solid red"]))"#,
                r#"["1px solid red => 1px solid rgb(255, 0, 0)"]"#,
            ),
            (
                r#"E(() => C("border-inline-width", ["1px 2px"]))"#,
                r#"["1px 2px => 0px"]"#,
            ),
            (
                r#"E(() => C("border-radius", ["5px", "5px 10px", "5px / 10px", "1px 2px 3px 4px / 5px 6px", "50%", "0", "5px 5px", "1px 2px 3px", "1px / 1px"]))"#,
                r#"["5px => 5px", "5px 10px => 5px 10px", "5px / 10px => 5px / 10px", "1px 2px 3px 4px / 5px 6px => 1px 2px 3px 4px / 5px 6px", "50% => 50%", "0 => 0px", "5px 5px => 5px", "1px 2px 3px => 1px 2px 3px", "1px / 1px => 1px"]"#,
            ),
            (
                r#"E(() => C("border-spacing", ["1px", "1px 2px", "0"]))"#,
                r#"["1px => 1px", "1px 2px => 1px 2px", "0 => 0px"]"#,
            ),
            (
                r#"E(() => C("outline", ["1px solid red", "none", "auto", "2px dotted", "red", "0", "thin"]))"#,
                r#"["1px solid red => rgb(255, 0, 0) solid 1px", "none => rgb(0, 0, 255) none 3px", "auto => rgb(0, 0, 255) auto 3px", "2px dotted => rgb(0, 0, 255) dotted 2px", "red => rgb(255, 0, 0) none 3px", "0 => rgb(0, 0, 255) none 0px", "thin => rgb(0, 0, 255) none 1px"]"#,
            ),
            (
                r##"E(() => C("background", ["red", "url(a.png) no-repeat center", "none", "red url(a.png)", "transparent", "url(a.png) center / cover no-repeat", "url(a.png) 0 0 repeat-x", "#fff url(a.png) repeat-y left top", "linear-gradient(red, blue)", "url(a.png), red", "url(a.png) fixed", "0 0", "content-box", "padding-box content-box", "url(a.png) left 10px top 5px / 10px 20px no-repeat scroll border-box padding-box red", "url(a.png), url(b.png) no-repeat", "center"]))"##,
                r##"["red => rgb(255, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box", "url(a.png) no-repeat center => rgba(0, 0, 0, 0) url(\"BASE/a.png\") no-repeat scroll 50% 50% / auto padding-box border-box", "none => rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box", "red url(a.png) => rgb(255, 0, 0) url(\"BASE/a.png\") repeat scroll 0% 0% / auto padding-box border-box", "transparent => rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box", "url(a.png) center / cover no-repeat => rgba(0, 0, 0, 0) url(\"BASE/a.png\") no-repeat scroll 50% 50% / cover padding-box border-box", "url(a.png) 0 0 repeat-x => rgba(0, 0, 0, 0) url(\"BASE/a.png\") repeat-x scroll 0px 0px / auto padding-box border-box", "#fff url(a.png) repeat-y left top => rgb(255, 255, 255) url(\"BASE/a.png\") repeat-y scroll 0% 0% / auto padding-box border-box", "linear-gradient(red, blue) => rgba(0, 0, 0, 0) linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)) repeat scroll 0% 0% / auto padding-box border-box", "url(a.png), red => url(\"BASE/a.png\") repeat scroll 0% 0% / auto padding-box border-box, rgb(255, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box", "url(a.png) fixed => rgba(0, 0, 0, 0) url(\"BASE/a.png\") repeat fixed 0% 0% / auto padding-box border-box", "0 0 => rgba(0, 0, 0, 0) none repeat scroll 0px 0px / auto padding-box border-box", "content-box => rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto content-box content-box", "padding-box content-box => rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto padding-box content-box", "url(a.png) left 10px top 5px / 10px 20px no-repeat scroll border-box padding-box red => rgb(255, 0, 0) url(\"BASE/a.png\") no-repeat scroll 10px 5px / 10px 20px border-box padding-box", "url(a.png), url(b.png) no-repeat => url(\"BASE/a.png\") repeat scroll 0% 0% / auto padding-box border-box, rgba(0, 0, 0, 0) url(\"BASE/b.png\") no-repeat scroll 0% 0% / auto padding-box border-box", "center => rgba(0, 0, 0, 0) none repeat scroll 50% 50% / auto padding-box border-box"]"##,
            ),
            (
                r#"E(() => C("background-position", ["center", "left top", "10px 20px", "right 10px bottom 5px", "50%", "top", "0 0, center", "left 10px top"]))"#,
                r#"["center => 50% 50%", "left top => 0% 0%", "10px 20px => 10px 20px", "right 10px bottom 5px => calc(100% - 10px) calc(100% - 5px)", "50% => 50% 50%", "top => 50% 0%", "0 0, center => 0px 0px", "left 10px top => 10px 0%"]"#,
            ),
            (
                r#"E(() => C("font", ["12px Arial", "bold 12px/1.5 Arial, sans-serif", "italic small-caps bold 12px Arial", "12px/20px sans-serif", "caption", "menu", "small-caption", "message-box", "icon", "status-bar", "normal 400 16px/normal serif", "condensed 12px a", "italic 12px a", "16px", "12px \"Times New Roman\"", "oblique 10deg 12px a", "bold italic 12px a", "1.2em/1.2 a", "x-large a", "100 12px a", "small-caps 12px a", "normal normal 12px a"]))"#,
                r#"["12px Arial => 12px Arial", "bold 12px/1.5 Arial, sans-serif => 700 12px / 18px Arial, sans-serif", "italic small-caps bold 12px Arial => italic small-caps 700 12px Arial", "12px/20px sans-serif => 12px / 20px sans-serif", "caption => 16px Arial", "menu => 16px Arial", "small-caption => 16px Arial", "message-box => 16px Arial", "icon => 16px Arial", "status-bar => 16px Arial", "normal 400 16px/normal serif => 16px serif", "condensed 12px a => condensed 12px a", "italic 12px a => italic 12px a", "16px => 20px \"Times New Roman\"", "12px \"Times New Roman\" => 12px \"Times New Roman\"", "oblique 10deg 12px a => oblique 10deg 12px a", "bold italic 12px a => italic 700 12px a", "1.2em/1.2 a => 24px / 28.8px a", "x-large a => 24px a", "100 12px a => 100 12px a", "small-caps 12px a => small-caps 12px a", "normal normal 12px a => 12px a"]"#,
            ),
            (
                r#"E(() => C("font-variant", ["small-caps", "normal", "none", "all-small-caps", "common-ligatures"]))"#,
                r#"["small-caps => small-caps", "normal => normal", "none => none", "all-small-caps => all-small-caps", "common-ligatures => common-ligatures"]"#,
            ),
            (
                r#"E(() => C("list-style", ["none", "square inside", "disc", "url(a.png)", "inside", "square url(a.png) inside", "decimal outside none", "none square", "none none"]))"#,
                r#"["none => outside none none", "square inside => inside none square", "disc => outside none disc", "url(a.png) => outside url(\"BASE/a.png\") disc", "inside => inside none disc", "square url(a.png) inside => inside url(\"BASE/a.png\") square", "decimal outside none => outside none decimal", "none square => outside none square", "none none => outside none none"]"#,
            ),
            (
                r#"E(() => C("text-decoration", ["underline", "underline red dotted", "none", "red", "underline overline", "line-through 2px", "underline solid"]))"#,
                r#"["underline => underline", "underline red dotted => underline dotted rgb(255, 0, 0)", "none => none", "red => rgb(255, 0, 0)", "underline overline => underline overline", "line-through 2px => line-through 2px", "underline solid => underline"]"#,
            ),
            (
                r#"E(() => C("flex", ["1", "auto", "none", "1 1 0", "2 3 10px", "0 0 auto", "1 0", "10px", "0", "1 1 0%", "1 1 auto", "initial", "2 2", "1 auto", "content", "0 1 auto", "1 1 0px"]))"#,
                r#"["1 => 1 1 0%", "auto => 1 1 auto", "none => 0 0 auto", "1 1 0 => 1 1 0px", "2 3 10px => 2 3 10px", "0 0 auto => 0 0 auto", "1 0 => 1 0 0%", "10px => 1 1 10px", "0 => 0 1 0%", "1 1 0% => 1 1 0%", "1 1 auto => 1 1 auto", "initial => 0 1 auto", "2 2 => 2 2 0%", "1 auto => 1 1 auto", "content => 1 1 content", "0 1 auto => 0 1 auto", "1 1 0px => 1 1 0px"]"#,
            ),
            (
                r#"E(() => C("flex-flow", ["row wrap", "column", "wrap", "row nowrap", "wrap-reverse column-reverse"]))"#,
                r#"["row wrap => row wrap", "column => column nowrap", "wrap => row wrap", "row nowrap => row nowrap", "wrap-reverse column-reverse => column-reverse wrap-reverse"]"#,
            ),
            (
                r#"E(() => C("gap", ["10px", "10px 20px", "normal", "1em normal"]))"#,
                r#"["10px => 10px", "10px 20px => 10px 20px", "normal => normal", "1em normal => 20px normal"]"#,
            ),
            (r#"E(() => C("grid-gap", ["5px"]))"#, r#"["5px => 5px"]"#),
            (
                r#"E(() => C("columns", ["3", "100px", "100px 3", "auto", "auto auto", "3 auto"]))"#,
                r#"["3 => 3", "100px => 100px", "100px 3 => 100px 3", "auto => auto", "auto auto => auto", "3 auto => 3"]"#,
            ),
            (
                r#"E(() => C("overflow", ["hidden", "hidden auto", "auto", "visible hidden", "clip"]))"#,
                r#"["hidden => hidden", "hidden auto => hidden auto", "auto => auto", "visible hidden => auto hidden", "clip => clip"]"#,
            ),
            (
                r#"E(() => C("place-items", ["center", "start end", "center legacy", "baseline"]))"#,
                r#"["center => center", "start end => start end", "center legacy => center normal", "baseline => baseline"]"#,
            ),
            (
                r#"E(() => C("place-content", ["center", "space-between center"]))"#,
                r#"["center => center", "space-between center => space-between center"]"#,
            ),
            (
                r#"E(() => C("place-self", ["auto", "start end"]))"#,
                r#"["auto => auto", "start end => start end"]"#,
            ),
            (
                r#"E(() => C("contain-intrinsic-size", ["100px", "100px 200px", "auto 100px", "none"]))"#,
                r#"["100px => 100px", "100px 200px => 100px 200px", "auto 100px => auto 100px", "none => none"]"#,
            ),
            (
                r#"E(() => C("grid-row", ["1", "1 / 3", "span 2", "a", "1 / span 2", "auto", "a / b", "auto / auto"]))"#,
                r#"["1 => 1", "1 / 3 => 1 / 3", "span 2 => span 2", "a => a", "1 / span 2 => 1 / span 2", "auto => auto", "a / b => a / b", "auto / auto => auto"]"#,
            ),
            (
                r#"E(() => C("grid-column", ["2 / -1"]))"#,
                r#"["2 / -1 => 2 / -1"]"#,
            ),
            (
                r#"E(() => C("grid-area", ["1 / 2", "a", "1 / 2 / 3 / 4", "auto", "1", "a / b / c", "1 / 2 / 3"]))"#,
                r#"["1 / 2 => 1 / 2", "a => a", "1 / 2 / 3 / 4 => 1 / 2 / 3 / 4", "auto => auto", "1 => 1", "a / b / c => a / b / c", "1 / 2 / 3 => 1 / 2 / 3"]"#,
            ),
            (
                r#"E(() => C("grid-template", ["none", "100px / 1fr 1fr", "\"a b\" 10px / 1fr 1fr", "auto 1fr / auto"]))"#,
                r#"["none => none", "100px / 1fr 1fr => 100px / 1fr 1fr", "\"a b\" 10px / 1fr 1fr => \"a b\" 10px / 1fr 1fr", "auto 1fr / auto => auto 1fr / auto"]"#,
            ),
            (
                r#"E(() => C("grid", ["none", "100px / 1fr", "auto-flow / 1fr 1fr", "\"a\" 10px / 1fr", "1fr / auto-flow dense 100px"]))"#,
                r#"["none => none / none / none / row / auto / auto", "100px / 1fr => 100px / 1fr / none / row / auto / auto", "auto-flow / 1fr 1fr => none / 1fr 1fr / none / row / auto / auto", "\"a\" 10px / 1fr => 10px / 1fr / \"a\" / row / auto / auto", "1fr / auto-flow dense 100px => 1fr / none / none / column dense / auto / 100px"]"#,
            ),
            (
                r#"E(() => C("mask", ["url(a.svg)", "none", "url(a.svg) no-repeat center / contain"]))"#,
                r#"["url(a.svg) => url(\"BASE/a.svg\")", "none => none", "url(a.svg) no-repeat center / contain => url(\"BASE/a.svg\") 50% 50% / contain no-repeat"]"#,
            ),
            (
                r#"E(() => C("-webkit-mask", ["url(a.svg)"]))"#,
                r#"["url(a.svg) => url(\"BASE/a.svg\")"]"#,
            ),
            (
                r#"E(() => C("mask-position", ["center"]))"#,
                r#"["center => 50% 50%"]"#,
            ),
            (
                r#"E(() => C("-webkit-mask-position", ["left top"]))"#,
                r#"["left top => 0% 0%"]"#,
            ),
            (
                r#"E(() => C("-webkit-mask-origin", ["content-box"]))"#,
                r#"["content-box => content-box"]"#,
            ),
            (
                r#"E(() => C("-webkit-mask-clip", ["text"]))"#,
                r#"["text => text"]"#,
            ),
            (
                r#"E(() => C("-webkit-mask-composite", ["source-over"]))"#,
                r#"["source-over => source-over"]"#,
            ),
            (
                r#"E(() => C("word-wrap", ["break-word"]))"#,
                r#"["break-word => break-word"]"#,
            ),
            (
                r#"E(() => C("-webkit-box-sizing", ["border-box"]))"#,
                r#"["border-box => border-box"]"#,
            ),
            (
                r#"E(() => C("margin-inline-start", ["5px"]))"#,
                r#"["5px => 5px"]"#,
            ),
            (
                r#"E(() => C("inline-size", ["10px"]))"#,
                r#"["10px => 10px"]"#,
            ),
            (
                r#"E(() => C("border-start-start-radius", ["3px"]))"#,
                r#"["3px => 3px"]"#,
            ),
            (
                r#"E(() => C("overflow-inline", ["hidden"]))"#,
                r#"["hidden => hidden"]"#,
            ),
            (
                r#"E(() => C("inset-block-start", ["1px"]))"#,
                r#"["1px => 1px"]"#,
            ),
            (r#"E(() => C("-webkit-flex", ["1"]))"#, r#"["1 => 1 1 0%"]"#),
            (
                r#"E(() => C("-webkit-border-radius", ["4px"]))"#,
                r#"["4px => 4px"]"#,
            ),
            (
                r#"E(() => C("border-block-start", ["1px solid red"]))"#,
                r#"["1px solid red => 1px solid rgb(255, 0, 0)"]"#,
            ),
            (
                r#"E(() => C("font-width", ["condensed"]))"#,
                r#"["condensed => "]"#,
            ),
            (
                r#"E(() => C("text-decoration-thickness", ["2px"]))"#,
                r#"["2px => 2px"]"#,
            ),
            (
                r#"E(() => C("text-underline-offset", ["2px"]))"#,
                r#"["2px => 2px"]"#,
            ),
        ],
    },
    Case {
        name: "object",
        html: PAGE_2,
        queries: &[
            (
                r"E(() => { const s = getComputedStyle(d); return [s === getComputedStyle(d), s instanceof CSSStyleDeclaration, String(s), s.parentRule, s.cssText, typeof s.length, s.length > 100, s.getPropertyPriority('color'), 'color' in s, s.hasOwnProperty('color'), 'fooBar' in s, s.color, s['background-color'], s.cssFloat, s.float, s.webkitTransform]; })",
                r#"[false, true, "[object CSSStyleDeclaration]", null, "", "number", true, "", true, true, false, "rgb(1, 2, 3)", "rgba(0, 0, 0, 0)", "none", "none", "none"]"#,
            ),
            (
                r"E(() => getComputedStyle(d).setProperty('color', 'red'))",
                r#""DOMException NoModificationAllowedError: Failed to execute 'setProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'color' property is read-only.""#,
            ),
            (
                r"E(() => getComputedStyle(d).setProperty('foo', 'red'))",
                r#""DOMException NoModificationAllowedError: Failed to execute 'setProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'foo' property is read-only.""#,
            ),
            (
                r"E(() => getComputedStyle(d).setProperty('color', ''))",
                r#""DOMException NoModificationAllowedError: Failed to execute 'setProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'color' property is read-only.""#,
            ),
            (
                r"E(() => getComputedStyle(d).removeProperty('color'))",
                r#""DOMException NoModificationAllowedError: Failed to execute 'removeProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'color' property is read-only.""#,
            ),
            (
                r"E(() => getComputedStyle(d).removeProperty('foo'))",
                r#""DOMException NoModificationAllowedError: Failed to execute 'removeProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'foo' property is read-only.""#,
            ),
            (
                r"E(() => { getComputedStyle(d).cssText = 'color: red'; })",
                r#""DOMException NoModificationAllowedError: Failed to set the 'cssText' property on 'CSSStyleDeclaration': These styles are computed, and therefore read-only.""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); s.color = 'red'; return s.color; })",
                r#""DOMException NoModificationAllowedError: Failed to set a named property 'color' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'color' property is read-only.""#,
            ),
            (
                r"E(() => { 'use strict'; const s = getComputedStyle(d); s.color = 'red'; return s.color; })",
                r#""DOMException NoModificationAllowedError: Failed to set a named property 'color' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'color' property is read-only.""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); s.cssFloat = 'left'; return s.cssFloat; })",
                r#""DOMException NoModificationAllowedError: Failed to set the 'cssFloat' property on 'CSSStyleDeclaration': These styles are computed, and therefore the 'float' property is read-only.""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); s.foo = 1; return [s.foo, delete s.color, s.color]; })",
                r#"[1, true, "rgb(1, 2, 3)"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); s['background-color'] = 'red'; return s.backgroundColor; })",
                r#""DOMException NoModificationAllowedError: Failed to set a named property 'background-color' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'background-color' property is read-only.""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); s.backgroundColor = 'red'; return s.backgroundColor; })",
                r#""DOMException NoModificationAllowedError: Failed to set a named property 'backgroundColor' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'background-color' property is read-only.""#,
            ),
            (
                r"E(() => getComputedStyle())",
                r#""TypeError: Failed to execute 'getComputedStyle' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => getComputedStyle(document))",
                r#""TypeError: Failed to execute 'getComputedStyle' on 'Window': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => getComputedStyle(document.createTextNode('x')))",
                r#""TypeError: Failed to execute 'getComputedStyle' on 'Window': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => getComputedStyle(null))",
                r#""TypeError: Failed to execute 'getComputedStyle' on 'Window': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => getComputedStyle({}))",
                r#""TypeError: Failed to execute 'getComputedStyle' on 'Window': parameter 1 is not of type 'Element'.""#,
            ),
            (
                r"E(() => [getComputedStyle.length, getComputedStyle.name, typeof window.getComputedStyle, JSON.stringify(Object.getOwnPropertyDescriptor(window, 'getComputedStyle')), 'getComputedStyle' in Window.prototype])",
                r#"[1, "getComputedStyle", "function", "{\"writable\":true,\"enumerable\":true,\"configurable\":true}", false]"#,
            ),
            (
                r"E(() => [getComputedStyle(d, null).color, getComputedStyle(d, undefined).color, getComputedStyle(d, '').color, getComputedStyle(d, 'before').color])",
                r#"["rgb(1, 2, 3)", "rgb(1, 2, 3)", "rgb(1, 2, 3)", "rgb(1, 2, 3)"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); const a = s.color; d.style.color = 'red'; const b = s.color; d.style.color = 'rgb(1, 2, 3)'; return [a, b, s.color]; })",
                r#"["rgb(1, 2, 3)", "rgb(255, 0, 0)", "rgb(1, 2, 3)"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); const a = Array.from(s); return [s.length === a.length, a.includes('color'), a.includes('margin'), a.includes('margin-top'), a.includes('--x'), a.includes('-webkit-mask-position-x'), JSON.stringify(a) === JSON.stringify(a.slice().sort()), s[s.length], s.item(100000), typeof s[0]]; })",
                r#"[true, true, false, true, true, true, false, null, "", "string"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); const a = Array.from(s); return [a.indexOf('-webkit-border-horizontal-spacing') > a.indexOf('z-index'), a.filter(n => n.startsWith('--')).join(), a[a.length - 1]]; })",
                r#"[true, "--x", "--x"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); return [s.getPropertyValue('COLOR'), s.getPropertyValue('backgroundColor'), s.getPropertyValue('--x'), s.getPropertyValue('--y'), s.getPropertyValue('foo'), s.getPropertyValue('float'), s.getPropertyValue('word-wrap'), s.getPropertyValue('-webkit-transform'), s.getPropertyValue(' color')]; })",
                r#"["rgb(1, 2, 3)", "", "1", "", "", "none", "normal", "none", ""]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(d); return [Object.keys(s).length > 100, Object.keys(s).includes('0'), Object.keys(s).includes('color'), Object.keys(s).includes('backgroundColor'), Object.keys(s).includes('background-color')]; })",
                r"[true, true, true, true, false]",
            ),
        ],
    },
    Case {
        name: "colors",
        html: PAGE_3,
        queries: &[
            (
                r"E(() => G('#a', ['color', 'background-color', 'border-top-color', 'border-left-color', 'outline-color', 'text-decoration-color', 'fill', 'stroke', 'border-color', 'border', 'border-top', 'outline', 'text-decoration']))",
                r#"["color: rgb(255, 0, 0)", "background-color: rgba(0, 0, 255, 0.5)", "border-top-color: rgb(255, 0, 0)", "border-left-color: rgb(255, 0, 0)", "outline-color: rgb(255, 0, 0)", "text-decoration-color: rgb(255, 0, 0)", "fill: rgb(0, 0, 0)", "stroke: none", "border-color: rgb(255, 0, 0)", "border: 1px solid rgb(255, 0, 0)", "border-top: 1px solid rgb(255, 0, 0)", "outline: rgb(255, 0, 0) solid 2px", "text-decoration: underline"]"#,
            ),
            (
                r"E(() => G('#b', ['color', 'border-top-color', 'border-bottom-color', 'background-color', 'outline-color']))",
                r#"["color: rgba(0, 0, 0, 0)", "border-top-color: rgba(0, 0, 0, 0)", "border-bottom-color: rgba(0, 0, 0, 0)", "background-color: rgba(0, 0, 0, 0)", "outline-color: rgba(0, 0, 0, 0)"]"#,
            ),
            (
                r"E(() => G('#c2', ['color', 'background-color', 'background']))",
                r#"["color: rgb(0, 128, 0)", "background-color: rgba(255, 0, 0, 0.533)", "background: rgba(255, 0, 0, 0.533) none repeat scroll 0% 0% / auto padding-box border-box"]"#,
            ),
            (
                r"E(() => G('#k', ['color', 'border-bottom-color', 'background-color']))",
                r#"["color: rgb(255, 0, 0)", "border-bottom-color: rgb(255, 0, 0)", "background-color: rgba(0, 0, 0, 0)"]"#,
            ),
            (
                r"E(() => G('#t', ['color', 'background-color', 'border-left-color', 'border-top-color']))",
                r#"["color: rgba(10, 20, 30, 0.333)", "background-color: rgba(0, 0, 0, 0)", "border-left-color: rgba(0, 0, 0, 0)", "border-top-color: rgba(10, 20, 30, 0.333)"]"#,
            ),
            (
                r"E(() => G('#r', ['color', 'fill', 'stroke', 'fill-opacity', 'stroke-width', 'stroke-dasharray']))",
                r#"["color: rgb(9, 8, 7)", "fill: rgb(9, 8, 7)", "stroke: rgb(1, 2, 3)", "fill-opacity: 1", "stroke-width: 1px", "stroke-dasharray: none"]"#,
            ),
        ],
    },
    Case {
        name: "layout",
        html: PAGE_4,
        queries: &[
            (
                r#"E(() => G('#pct', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "min-width", "max-width", "margin", "padding"]))"#,
                r#"["width: 200px", "height: 0px", "margin-top: 40px", "margin-right: 92px", "margin-bottom: 20px", "margin-left: 92px", "padding-top: 20px", "padding-right: 8px", "padding-bottom: 20px", "padding-left: 8px", "min-width: 10%", "max-width: 90%", "margin: 40px 92px 20px", "padding: 20px 8px"]"#,
            ),
            (
                r#"E(() => G('#abs', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "top", "right", "bottom", "left", "inset"]))"#,
                r#"["width: 30px", "height: 20px", "margin-top: 8.39062px", "margin-right: 8.39062px", "margin-bottom: 8.39062px", "margin-left: 8.39062px", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px", "top: 110px", "right: 331.219px", "bottom: 336.219px", "left: 42px", "inset: 110px 331.219px 336.219px 42px"]"#,
            ),
            (
                r#"E(() => G('#abs2', ["width", "height", "top", "right", "bottom", "left"]))"#,
                r#"["width: 5px", "height: 11px", "top: 423.703px", "right: 5px", "bottom: 48.2969px", "left: 410px"]"#,
            ),
            (
                r#"E(() => G('#rel', ["top", "right", "bottom", "left"]))"#,
                r#"["top: 23.1406px", "right: 10px", "bottom: -23.15px", "left: -10px"]"#,
            ),
            (
                r#"E(() => G('#rel2', ["top", "right", "bottom", "left"]))"#,
                r#"["top: 0px", "right: 10px", "bottom: 0px", "left: 5px"]"#,
            ),
            (
                r#"E(() => G('#centered', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 100px", "height: 30px", "margin-top: 0px", "margin-right: 150px", "margin-bottom: 0px", "margin-left: 150px", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px"]"#,
            ),
            (
                r#"E(() => G('#inl', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "top", "right", "bottom", "left"]))"#,
                r#"["width: 100px", "height: 100px", "margin-top: 10%", "margin-right: 10%", "margin-bottom: 10%", "margin-left: 10%", "padding-top: 5%", "padding-right: 5%", "padding-bottom: 5%", "padding-left: 5%", "top: 5%", "right: auto", "bottom: auto", "left: auto"]"#,
            ),
            (
                r#"E(() => G('#bb', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 100px", "height: 50px", "margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "padding-top: 10px", "padding-right: 10px", "padding-bottom: 10px", "padding-left: 10px"]"#,
            ),
            (
                r#"E(() => G('#none', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 50%", "height: auto", "margin-top: 10%", "margin-right: 10%", "margin-bottom: 10%", "margin-left: 10%", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px"]"#,
            ),
            (
                r#"E(() => G('#nonechild', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "top", "right", "bottom", "left"]))"#,
                r#"["width: 10px", "height: auto", "margin-top: 10px", "margin-right: 0px", "margin-bottom: 10px", "margin-left: 0px", "padding-top: 10%", "padding-right: 10%", "padding-bottom: 10%", "padding-left: 10%", "top: auto", "right: auto", "bottom: auto", "left: 5%"]"#,
            ),
            (
                r#"E(() => G('#fixed', ["width", "height", "top", "right", "bottom", "left"]))"#,
                r#"["width: 10px", "height: 10px", "top: 590px", "right: 0px", "bottom: 0px", "left: 790px"]"#,
            ),
            (
                r#"E(() => G('#sticky', ["width", "height", "top", "right", "bottom", "left"]))"#,
                r#"["width: 400px", "height: 10px", "top: 60px", "right: auto", "bottom: auto", "left: auto"]"#,
            ),
            (
                r#"E(() => G('#item', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "flex-basis", "flex"]))"#,
                r#"["width: 250px", "height: 40px", "margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px", "flex-basis: 0%", "flex: 1 1 0%"]"#,
            ),
            (
                r#"E(() => G('#item2', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 50px", "height: 0px", "margin-top: 20px", "margin-right: 0px", "margin-bottom: 20px", "margin-left: 0px", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px"]"#,
            ),
            (
                r"E(() => G('#tf', ['transform', 'transform-origin', 'width']))",
                r#"["transform: matrix(0.866025, 0.5, -0.5, 0.866025, 100, 10)", "transform-origin: 0px 25px", "width: 200px"]"#,
            ),
            (
                r"E(() => G('#tf2', ['transform', 'transform-origin']))",
                r#"["transform: matrix(2, 0, 0, 2, 0, 0)", "transform-origin: 50px 30px"]"#,
            ),
            (
                r#"E(() => G('#ib', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 120px", "height: 7px", "margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "padding-top: 4px", "padding-right: 4px", "padding-bottom: 4px", "padding-left: 4px"]"#,
            ),
            (
                r#"E(() => G('#cell', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 40px", "height: 0px", "margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "padding-top: 2px", "padding-right: 2px", "padding-bottom: 2px", "padding-left: 2px"]"#,
            ),
            (
                r"E(() => G('#lh', ['line-height', 'height', 'font-size']))",
                r#"["line-height: normal", "height: 11px", "font-size: 10px"]"#,
            ),
            (
                r"E(() => G('#lh2', ['line-height', 'height']))",
                r#"["line-height: 30px", "height: 30px"]"#,
            ),
            (
                r#"E(() => G('#contents', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 10px", "height: auto", "margin-top: 10%", "margin-right: 10%", "margin-bottom: 10%", "margin-left: 10%", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px"]"#,
            ),
            (
                r#"E(() => G('#box', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "border-top-width", "border"]))"#,
                r#"["width: 400px", "height: 463px", "margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "padding-top: 10px", "padding-right: 10px", "padding-bottom: 10px", "padding-left: 10px", "border-top-width: 5px", "border: 5px solid rgb(0, 0, 0)"]"#,
            ),
            (
                r#"E(() => G('body', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 784px", "height: 493px", "margin-top: 8px", "margin-right: 8px", "margin-bottom: 8px", "margin-left: 8px", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px"]"#,
            ),
            (
                r#"E(() => G('html', ["width", "height", "margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left"]))"#,
                r#"["width: 800px", "height: 509px", "margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "padding-top: 0px", "padding-right: 0px", "padding-bottom: 0px", "padding-left: 0px"]"#,
            ),
        ],
    },
    Case {
        name: "grid",
        html: PAGE_5,
        queries: &[
            (
                r"E(() => G('#g', ['grid-template-columns', 'grid-template-rows', 'grid-auto-rows', 'gap', 'row-gap', 'grid-template', 'grid']))",
                r#"["grid-template-columns: 93.3281px 186.672px 30px", "grid-template-rows: 18px 20px", "grid-auto-rows: auto", "gap: 10px", "row-gap: 10px", "grid-template: auto 20px / 1fr 2fr 30px", "grid: 18px 20px / 93.3281px 186.672px 30px / none / row / auto / auto"]"#,
            ),
            (
                r"E(() => G('#g2', ['grid-template-columns', 'grid-template-rows', 'grid-auto-rows']))",
                r#"["grid-template-columns: 50px 50px", "grid-template-rows: 15px 15px", "grid-auto-rows: 15px"]"#,
            ),
            (
                r"E(() => G('#ng', ['grid-template-columns', 'grid-template-rows']))",
                r#"["grid-template-columns: 1fr 1fr", "grid-template-rows: none"]"#,
            ),
            (
                r"E(() => G('#ga', ['grid-template-columns', 'grid-template-areas', 'grid-area']))",
                r#"["grid-template-columns: [x] 40px [y] 60px", "grid-template-areas: \"a b\"", "grid-area: auto"]"#,
            ),
        ],
    },
    Case {
        name: "fonts",
        html: PAGE_6,
        queries: &[
            (
                r#"E(() => G('#f1', ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-caps", "line-height", "font", "font-variant"]))"#,
                r#"["font-family: Arial, sans-serif", "font-size: 12px", "font-weight: 700", "font-style: italic", "font-stretch: 100%", "font-variant-caps: normal", "line-height: 30px", "font: italic 700 12px / 30px Arial, sans-serif", "font-variant: normal"]"#,
            ),
            (
                r#"E(() => G('#f2', ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-caps", "line-height", "font", "font-variant"]))"#,
                r#"["font-family: \"Times New Roman\", serif", "font-size: 32px", "font-weight: 700", "font-style: normal", "font-stretch: 100%", "font-variant-caps: normal", "line-height: normal", "font: 700 32px \"Times New Roman\", serif", "font-variant: normal"]"#,
            ),
            (
                r#"E(() => G('#f3', ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-caps", "line-height", "font", "font-variant"]))"#,
                r#"["font-family: monospace", "font-size: 15.6px", "font-weight: 400", "font-style: normal", "font-stretch: 100%", "font-variant-caps: normal", "line-height: normal", "font: 15.6px monospace", "font-variant: normal"]"#,
            ),
            (
                r#"E(() => G('#f4', ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-caps", "line-height", "font", "font-variant"]))"#,
                r#"["font-family: \"a b\", c, \"d e\", sans-serif", "font-size: 10px", "font-weight: 400", "font-style: normal", "font-stretch: 75%", "font-variant-caps: small-caps", "line-height: normal", "font: small-caps condensed 10px \"a b\", c, \"d e\", sans-serif", "font-variant: small-caps"]"#,
            ),
            (
                r#"E(() => G('#f5', ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-caps", "line-height", "font", "font-variant", "letter-spacing", "word-spacing", "text-indent"]))"#,
                r#"["font-family: \"Times New Roman\"", "font-size: 24px", "font-weight: 400", "font-style: normal", "font-stretch: 100%", "font-variant-caps: normal", "line-height: 28.8px", "font: 24px / 28.8px \"Times New Roman\"", "font-variant: normal", "letter-spacing: 2.4px", "word-spacing: 24px", "text-indent: 48px"]"#,
            ),
            (
                r#"E(() => G('body', ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-caps", "line-height", "font", "font-variant"]))"#,
                r#"["font-family: \"Times New Roman\"", "font-size: 16px", "font-weight: 400", "font-style: normal", "font-stretch: 100%", "font-variant-caps: normal", "line-height: normal", "font: 16px \"Times New Roman\"", "font-variant: normal"]"#,
            ),
        ],
    },
    Case {
        name: "shorthands",
        html: PAGE_7,
        queries: &[
            (
                r#"E(() => G('#s1', ["margin", "padding", "border", "border-top", "border-right", "border-width", "border-style", "border-color", "border-radius", "outline", "overflow", "list-style", "flex-flow", "gap", "place-items", "inset", "background", "background-position", "columns", "grid-area", "grid-row", "grid-column", "text-decoration", "flex", "border-spacing", "place-content", "place-self", "contain-intrinsic-size", "mask", "font", "font-variant", "border-block", "border-inline", "margin-block", "margin-inline", "padding-inline", "inset-block", "border-block-start", "border-inline-width", "border-image", "grid", "grid-template", "mask-position", "-webkit-mask-position", "overflow-x"]))"#,
                r#"["margin: 1px 2px 3px 4px", "padding: 1px 2px", "border: ", "border-top: 1px solid rgb(255, 0, 0)", "border-right: 2px dashed rgb(0, 0, 255)", "border-width: 1px 2px", "border-style: solid dashed", "border-color: rgb(255, 0, 0) rgb(0, 0, 255)", "border-radius: 1px 2px 3px 4px / 5px", "outline: rgb(0, 128, 0) dotted 3px", "overflow: hidden auto", "list-style: inside none square", "flex-flow: column wrap", "gap: 1px 2px", "place-items: center start", "inset: 1px 2px 3px 4px", "background: rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box", "background-position: 0% 0%", "columns: auto", "grid-area: auto", "grid-row: auto", "grid-column: auto", "text-decoration: none", "flex: 0 1 auto", "border-spacing: 0px", "place-content: normal", "place-self: auto", "contain-intrinsic-size: none", "mask: none", "font: 16px \"Times New Roman\"", "font-variant: normal", "border-block: 1px solid rgb(255, 0, 0)", "border-inline: 2px dashed rgb(0, 0, 255)", "margin-block: 1px 3px", "margin-inline: 4px 2px", "padding-inline: 2px", "inset-block: 1px 3px", "border-block-start: 1px solid rgb(255, 0, 0)", "border-inline-width: 2px", "border-image: none", "grid: none / none / none / row / auto / auto", "grid-template: none", "mask-position: 0% 0%", "-webkit-mask-position: 0% 0%", "overflow-x: hidden"]"#,
            ),
            (
                r#"E(() => G('#s2', ["margin", "padding", "border", "border-top", "border-right", "border-width", "border-style", "border-color", "border-radius", "outline", "overflow", "list-style", "flex-flow", "gap", "place-items", "inset", "background", "background-position", "columns", "grid-area", "grid-row", "grid-column", "text-decoration", "flex", "border-spacing", "place-content", "place-self", "contain-intrinsic-size", "mask", "font", "font-variant", "border-block", "border-inline", "margin-block", "margin-inline", "padding-inline", "inset-block", "border-block-start", "border-inline-width", "border-image", "grid", "grid-template", "mask-position", "-webkit-mask-position", "overflow-x"]))"#,
                r#"["margin: 0px", "padding: 0px", "border: 2px solid rgb(1, 2, 3)", "border-top: 2px solid rgb(1, 2, 3)", "border-right: 2px solid rgb(1, 2, 3)", "border-width: 2px", "border-style: solid", "border-color: rgb(1, 2, 3)", "border-radius: 50%", "outline: rgb(0, 0, 0) none 3px", "overflow: visible", "list-style: outside none disc", "flex-flow: row nowrap", "gap: normal", "place-items: normal", "inset: auto", "background: rgb(255, 0, 0) url(\"BASE/a.png\") no-repeat scroll 10px 20px / 30px padding-box border-box", "background-position: 10px 20px", "columns: 100px 3", "grid-area: 1 / 2 / 3 / 4", "grid-row: 1 / 3", "grid-column: 2 / 4", "text-decoration: underline dotted rgb(255, 0, 0)", "flex: 2 3 10%", "border-spacing: 0px", "place-content: normal", "place-self: auto", "contain-intrinsic-size: none", "mask: none", "font: 16px \"Times New Roman\"", "font-variant: normal", "border-block: 2px solid rgb(1, 2, 3)", "border-inline: 2px solid rgb(1, 2, 3)", "margin-block: 0px", "margin-inline: 0px", "padding-inline: 0px", "inset-block: auto", "border-block-start: 2px solid rgb(1, 2, 3)", "border-inline-width: 2px", "border-image: none", "grid: none / none / none / row / auto / auto", "grid-template: none", "mask-position: 0% 0%", "-webkit-mask-position: 0% 0%", "overflow-x: visible"]"#,
            ),
            (
                r#"E(() => G('#s3', ["margin", "padding", "border", "border-top", "border-right", "border-width", "border-style", "border-color", "border-radius", "outline", "overflow", "list-style", "flex-flow", "gap", "place-items", "inset", "background", "background-position", "columns", "grid-area", "grid-row", "grid-column", "text-decoration", "flex", "border-spacing", "place-content", "place-self", "contain-intrinsic-size", "mask", "font", "font-variant", "border-block", "border-inline", "margin-block", "margin-inline", "padding-inline", "inset-block", "border-block-start", "border-inline-width", "border-image", "grid", "grid-template", "mask-position", "-webkit-mask-position", "overflow-x"]))"#,
                r#"["margin: 0px", "padding: 0px", "border: ", "border-top: 1px solid rgb(255, 0, 0)", "border-right: 0px none rgb(128, 128, 128)", "border-width: 1px 0px 0px", "border-style: solid none none", "border-color: rgb(255, 0, 0) rgb(128, 128, 128) rgb(128, 128, 128)", "border-radius: 0px", "outline: rgb(0, 0, 0) none 3px", "overflow: clip", "list-style: outside none none", "flex-flow: row nowrap", "gap: normal", "place-items: normal", "inset: auto", "background: rgba(0, 0, 0, 0) none repeat scroll calc(100% - 5px) 90% / auto padding-box border-box", "background-position: calc(100% - 5px) 90%", "columns: auto", "grid-area: auto", "grid-row: auto", "grid-column: auto", "text-decoration: none", "flex: 0 1 auto", "border-spacing: 2px 3px", "place-content: center", "place-self: end", "contain-intrinsic-size: 10px 20px", "mask: none", "font: 16px \"Times New Roman\"", "font-variant: normal", "border-block: ", "border-inline: 0px none rgb(128, 128, 128)", "margin-block: 0px", "margin-inline: 0px", "padding-inline: 0px", "inset-block: auto", "border-block-start: 1px solid rgb(255, 0, 0)", "border-inline-width: 0px", "border-image: none", "grid: none / none / none / row / auto / auto", "grid-template: none", "mask-position: 0% 0%", "-webkit-mask-position: 0% 0%", "overflow-x: clip"]"#,
            ),
        ],
    },
    Case {
        name: "pseudo",
        html: PAGE_8,
        queries: &[
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::before'))"#,
                r#"["content: \"ab\"", "color: rgb(255, 0, 0)", "width: 50px", "height: 16px", "display: inline-block", "padding-left: 20px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], ':before'))"#,
                r#"["content: \"ab\"", "color: rgb(255, 0, 0)", "width: 50px", "height: 16px", "display: inline-block", "padding-left: 20px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::BEFORE'))"#,
                r#"["content: \"ab\"", "color: rgb(255, 0, 0)", "width: 50px", "height: 16px", "display: inline-block", "padding-left: 20px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::after'))"#,
                r#"["content: none", "color: rgb(0, 0, 255)", "width: auto", "height: auto", "display: inline", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#q', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::after'))"#,
                r#"["content: counter(x) \".\"", "color: rgb(0, 0, 0)", "width: 12px", "height: 18px", "display: block", "padding-left: 0px", "left: 10px", "position: absolute"]"#,
            ),
            (
                r#"E(() => G('#r', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::before'))"#,
                r#"["content: none", "color: rgb(0, 0, 0)", "width: auto", "height: auto", "display: inline", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#hidden', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::before'))"#,
                r#"["content: \"x\"", "color: rgb(0, 0, 0)", "width: auto", "height: auto", "display: none", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#li', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::marker'))"#,
                r#"["content: normal", "color: rgb(1, 2, 3)", "width: 7px", "height: 18px", "display: inline-block", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::marker'))"#,
                r#"["content: normal", "color: rgb(0, 128, 0)", "width: auto", "height: auto", "display: inline-block", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::first-line'))"#,
                r#"["content: normal", "color: rgb(4, 5, 6)", "width: auto", "height: auto", "display: inline", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#p', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::selection'))"#,
                r#"["content: normal", "color: rgb(7, 8, 9)", "width: auto", "height: auto", "display: inline", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r#"E(() => G('#in', ["content", "color", "width", "height", "display", "padding-left", "left", "position"], '::placeholder'))"#,
                r#"["content: normal", "color: rgb(10, 11, 12)", "width: 177px", "height: 15px", "display: block", "padding-left: 0px", "left: auto", "position: static"]"#,
            ),
            (
                r"E(() => ['::foo', 'before', '::before ', ' ::before', ':after', '::-webkit-scrollbar', ':hover', '::before::after', '::first-letter', ':first-line', '::backdrop', '::part(x)', '::slotted(p)', '::details-content'].map(p => { const s = getComputedStyle(document.getElementById('p'), p); return p + ' => ' + s.color + ' ' + s.content + ' ' + s.length; }))",
                r#"["::foo =>   0", "before => rgb(255, 0, 0) \"ab\" 456", "::before  =>   0", " ::before => rgb(0, 128, 0) normal 456", ":after => rgb(0, 0, 255) none 456", "::-webkit-scrollbar => rgb(0, 128, 0) normal 456", ":hover =>   0", "::before::after =>   0", "::first-letter => rgb(0, 128, 0) normal 456", ":first-line => rgb(4, 5, 6) normal 456", "::backdrop => rgb(0, 128, 0) normal 456", "::part(x) => rgb(0, 128, 0) normal 456", "::slotted(p) => rgb(0, 128, 0) normal 456", "::details-content => rgb(0, 128, 0) normal 456"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('p'), '::before'); const a = s.content; document.getElementById('p').id = 'p2'; const b = s.content; document.getElementById('p2').id = 'p'; return [a, b, s.content, s === getComputedStyle(document.getElementById('p'), '::before')]; })",
                r#"["\"ab\"", "none", "\"ab\"", false]"#,
            ),
        ],
    },
    Case {
        name: "special",
        html: PAGE_9,
        queries: &[
            (
                r"E(() => { const e = document.createElement('div'); e.className = 'x'; e.style.margin = '3px'; const s = getComputedStyle(e); const a = [s.color, s.display, s.width, s.marginTop, s.length, s.getPropertyValue('color'), s.cssText]; document.body.appendChild(e); const b = [s.color, s.display, s.marginTop, s.length > 0]; e.remove(); return a.concat(b, [s.color]); })",
                r#"["", "", "", "", 0, "", "", "rgb(1, 2, 3)", "block", "3px", true, ""]"#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); const f = document.createElement('p'); f.className = 'x'; e.appendChild(f); const s = getComputedStyle(f); return [s.color, s.display, s.length]; })",
                r#"["", "", 0]"#,
            ),
            (
                r"E(() => G('#nd', ['display', 'color', 'width', 'height', 'margin-top']))",
                r#"["display: none", "color: rgb(9, 9, 9)", "width: auto", "height: auto", "margin-top: 0px"]"#,
            ),
            (
                r"E(() => G('#ndc', ['display', 'color', 'width', 'height', 'font-size', 'line-height']))",
                r#"["display: inline", "color: rgb(1, 2, 3)", "width: 20px", "height: auto", "font-size: 16px", "line-height: normal"]"#,
            ),
            (
                r"E(() => G('#h', ['display', 'color', 'width']))",
                r#"["display: none", "color: rgb(5, 5, 5)", "width: auto"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('tpl').content.firstElementChild); return [s.color, s.display, s.width, s.length]; })",
                r#"["", "", "", 0]"#,
            ),
            (
                r#"E(() => { const d = new DOMParser().parseFromString('<p class=x style="color: red">p</p>', 'text/html'); const s = getComputedStyle(d.querySelector('p')); return [s.color, s.display, s.width, s.length]; })"#,
                r#"["", "", "", 0]"#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument('t'); const p = d.createElement('p'); d.body.appendChild(p); const s = getComputedStyle(p); return [s.color, s.display, s.length]; })",
                r#"["", "", 0]"#,
            ),
            (
                r"E(() => [G('head', ['display', 'color']), G('title', ['display', 'width']), G('script', ['display'])].join(' | '))",
                r#""display: none,color: rgb(2, 2, 2) | display: none,width: auto | display: none""#,
            ),
            (
                r"E(() => G('html', ['display', 'color', 'font-family', 'width', 'height', 'overflow', 'background-color']))",
                r#"["display: block", "color: rgb(0, 0, 0)", "font-family: \"Times New Roman\"", "width: 800px", "height: 8px", "overflow: visible", "background-color: rgba(0, 0, 0, 0)"]"#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'rect'); document.body.appendChild(e); const s = getComputedStyle(e); const r = [s.display, s.width, s.fill, s.color]; e.remove(); return r; })",
                r#"["inline", "auto", "rgb(0, 0, 0)", "rgb(0, 0, 0)"]"#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://example.com/x', 'foo'); document.body.appendChild(e); const s = getComputedStyle(e); const r = [s.display, s.width, s.color]; e.remove(); return r; })",
                r#"["inline", "auto", "rgb(0, 0, 0)"]"#,
            ),
        ],
    },
    Case {
        name: "changes",
        html: PAGE_10,
        queries: &[
            (
                r"E(() => { const e = document.getElementById('w'); const s = getComputedStyle(e); const a = [s.width, s.color]; e.classList.add('wide'); const b = [s.width, s.color]; e.classList.remove('wide'); return a.concat(b, [s.width]); })",
                r#"["100px", "rgb(0, 0, 0)", "300px", "rgb(1, 1, 1)", "100px"]"#,
            ),
            (
                r"E(() => { const e = document.getElementById('w'); const s = getComputedStyle(e); e.hidden = true; const a = [s.display, s.width]; e.hidden = false; return a.concat([s.display, s.width]); })",
                r#"["none", "100px", "block", "100px"]"#,
            ),
            (
                r"E(() => { const e = document.getElementById('w'); const s = getComputedStyle(e); const st = document.createElement('style'); st.textContent = '#w { color: rgb(6, 6, 6); width: 10px !important }'; document.head.appendChild(st); const a = [s.color, s.width]; st.remove(); return a.concat([s.color, s.width]); })",
                r#"["rgb(6, 6, 6)", "10px", "rgb(0, 0, 0)", "100px"]"#,
            ),
            (
                r"E(() => { const e = document.getElementById('w'); const s = getComputedStyle(e); const st = document.createElement('style'); document.head.appendChild(st); const a = s.width; st.textContent = '#w { width: 11px }'; const b = s.width; st.firstChild.data = '#w { width: 12px }'; const c = s.width; st.remove(); return [a, b, c, s.width]; })",
                r#"["100px", "11px", "12px", "100px"]"#,
            ),
            (
                r"E(() => { const e = document.getElementById('w'); const s = getComputedStyle(e); e.style.width = '123px'; const a = s.width; e.style.padding = '10%'; const b = [s.paddingLeft, s.paddingTop]; e.style.cssText = ''; return [a].concat(b, [s.width, s.paddingLeft]); })",
                r#"["123px", "78.3906px", "78.3906px", "100px", "0px"]"#,
            ),
            (
                r"E(() => { const e = document.getElementById('w'); const s = getComputedStyle(e); e.setAttribute('data-x', 'y'); const a = [s.color, s.marginLeft]; e.setAttribute('data-x', 'z'); const b = [s.color, s.marginLeft]; e.removeAttribute('data-x'); return a.concat(b); })",
                r#"["rgb(2, 2, 2)", "7px", "rgb(0, 0, 0)", "0px"]"#,
            ),
            (
                r"E(() => { const s1 = getComputedStyle(sib1), s2 = getComputedStyle(sib2); const a = [s1.color, s2.width]; const d = document.createElement('div'); d.className = 'a'; sib1.before(d); const b = [s1.color, s2.width]; d.remove(); return a.concat(b, [s1.color, s2.width]); })",
                r#"["rgb(0, 0, 0)", "784px", "rgb(3, 3, 3)", "33px", "rgb(0, 0, 0)", "784px"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(l1); const a = s.color; const n = document.createElement('p'); list.prepend(n); const b = [s.color, getComputedStyle(n).color]; n.remove(); return [a].concat(b, [s.color]); })",
                r#"["rgb(4, 4, 4)", "rgb(0, 0, 0)", "rgb(4, 4, 4)", "rgb(4, 4, 4)"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(child); const a = [s.width, s.paddingLeft]; par.style.width = '200px'; const b = [s.width, s.paddingLeft]; par.style.width = ''; return a.concat(b, [s.width]); })",
                r#"["200px", "40px", "100px", "20px", "200px"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(txt); const a = s.height; txt.textContent = 'x'; const b = s.height; txt.innerHTML = 'x<br>y'; const c = s.height; txt.textContent = ''; return [a, b, c, s.height]; })",
                r#"["0px", "18px", "36px", "0px"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(t1); t1.innerHTML = '<span>abc abc abc abc abc abc abc abc</span>'; const a = s.height; t1.style.width = '60px'; const b = s.height; return [a, b]; })",
                r#"["69px", "184px"]"#,
            ),
            (
                r"E(() => { const r = []; for (let i = 0; i < 5; i++) { w.style.width = (i * 10) + 'px'; r.push(getComputedStyle(w).width); } w.style.width = ''; return r; })",
                r#"["0px", "10px", "20px", "30px", "40px"]"#,
            ),
        ],
    },
    Case {
        name: "quirks",
        html: PAGE_11,
        queries: &[
            (
                r"E(() => G('#a', ['width', 'height', 'line-height', 'display', 'margin-top']))",
                r#"["width: 392px", "height: 10px", "line-height: normal", "display: block", "margin-top: 0px"]"#,
            ),
            (
                r"E(() => G('body', ['width', 'height', 'margin-top']))",
                r#"["width: 784px", "height: 584px", "margin-top: 8px"]"#,
            ),
        ],
    },
    Case {
        name: "extra",
        html: PAGE_12,
        queries: &[
            (
                r"E(() => G('#fi', ['min-width', 'min-height', 'transform', 'transform-origin', 'width']))",
                r#"["min-width: auto", "min-height: auto", "transform: matrix(1, 0, 0, 1, 20, 5)", "transform-origin: 20px 10px", "width: 40px"]"#,
            ),
            (
                r"E(() => G('#gi', ['min-width', 'min-height', 'transform', 'transform-origin', 'width']))",
                r#"["min-width: auto", "min-height: auto", "transform: matrix(1, 0, 0, 1, 20, 5)", "transform-origin: 20px 10px", "width: 40px"]"#,
            ),
            (
                r"E(() => G('#dn', ['min-width', 'transform', 'transform-origin', 'width', 'perspective-origin']))",
                r#"["min-width: 0px", "transform: none", "transform-origin: 50% 50%", "width: 40px", "perspective-origin: 50% 50%"]"#,
            ),
            (
                r"E(() => G('#sp', ['min-width', 'transform', 'transform-origin', 'width', 'height']))",
                r#"["min-width: 0px", "transform: matrix(1, 0, 0, 1, 0, 0)", "transform-origin: 0px 0px", "width: auto", "height: auto"]"#,
            ),
            (
                r"E(() => G('#am', ['margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'left', 'right', 'top', 'bottom']))",
                r#"["margin-top: 15px", "margin-right: 100px", "margin-bottom: 0px", "margin-left: 100px", "left: 0px", "right: 0px", "top: 30px", "bottom: 145px"]"#,
            ),
            (
                r"E(() => G('#fl', ['margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'min-width']))",
                r#"["margin-top: 0px", "margin-right: 78.3906px", "margin-bottom: 0px", "margin-left: 78.3906px", "min-width: 0px"]"#,
            ),
            (
                r"E(() => G('#ia', ['margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'width', 'height']))",
                r#"["margin-top: 0px", "margin-right: 0px", "margin-bottom: 0px", "margin-left: 0px", "width: 10px", "height: 0px"]"#,
            ),
            (
                r"E(() => G('#ovp', ['width', 'height', 'padding-top', 'padding-left']))",
                r#"["width: 80px", "height: 200px", "padding-top: 10px", "padding-left: 10px"]"#,
            ),
            (
                r"E(() => G('#plain', ['min-width', 'min-height', 'max-width', 'transform-origin']))",
                r#"["min-width: 0px", "min-height: 0px", "max-width: none", "transform-origin: 392px 0px"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('plain')); s.backgroundColor = 'red'; })",
                r#""DOMException NoModificationAllowedError: Failed to set a named property 'backgroundColor' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'background-color' property is read-only.""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('plain')); s.webkitTransform = 'none'; })",
                r#""DOMException NoModificationAllowedError: Failed to set a named property 'webkitTransform' on 'CSSStyleDeclaration': These styles are computed, and therefore the '-webkit-transform' property is read-only.""#,
            ),
            (
                r"E(() => getComputedStyle(document.getElementById('plain')).setProperty('COLOR', 'red'))",
                r#""DOMException NoModificationAllowedError: Failed to execute 'setProperty' on 'CSSStyleDeclaration': These styles are computed, and therefore the 'COLOR' property is read-only.""#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('plain')); return [s.getPropertyValue('text-decoration-thickness'), s.getPropertyValue('text-underline-offset'), s.getPropertyValue('text-underline-position'), s.getPropertyValue('text-decoration-skip-ink'), s.getPropertyValue('mask-type'), s.getPropertyValue('overflow-inline'), s.getPropertyValue('border-image-source'), s.getPropertyValue('border-image-slice'), s.getPropertyValue('border-image-width'), s.getPropertyValue('border-image-outset'), s.getPropertyValue('border-image-repeat'), s.getPropertyValue('-webkit-border-horizontal-spacing'), s.getPropertyValue('inline-size'), s.getPropertyValue('block-size')]; })",
                r#"["auto", "auto", "auto", "auto", "luminance", "visible", "none", "100%", "1", "0", "stretch", "0px", "784px", "0px"]"#,
            ),
            (
                r"E(() => { const s = getComputedStyle(document.getElementById('plain')); const a = Array.from(s); return [a.slice(0, 3).join(), a.indexOf('z-index') < a.indexOf('-webkit-border-horizontal-spacing'), a.filter(n => n.startsWith('-webkit-')).slice(0, 3).join(), a.indexOf('align-content') < a.indexOf('align-items'), a.indexOf('margin-inline-start') >= 0, a.indexOf('inset-block-start') >= 0, a.indexOf('-webkit-mask-position-x'), a.indexOf('mask-position')]; })",
                r#"["accent-color,align-content,align-items", true, "-webkit-border-horizontal-spacing,-webkit-border-image,-webkit-border-vertical-spacing", true, true, true, 441, 228]"#,
            ),
        ],
    },
    Case {
        name: "fixed-lengths-em",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0;font-size:16px}
#a{padding:0.4em 0.35em;margin:1.15em 0.7em;width:50%}
#b{padding:3.3%;margin:1.1% 2.2%}
#c{margin:-0.3em auto;width:10em}
</style></head><body><div id=a></div><div id=b></div><div id=c></div><script>function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>",
        queries: &[
            (
                r#"G('#a',["padding-top", "padding-left", "margin-top", "margin-left", "margin-right", "width"])"#,
                r#"["padding-top: 6.4px", "padding-left: 5.6px", "margin-top: 18.4px", "margin-left: 11.2px", "margin-right: 11.2px", "width: 400px"]"#,
            ),
            (
                r#"G('#b',["padding-top", "padding-left", "margin-top", "margin-left", "margin-right", "width"])"#,
                r#"["padding-top: 26.3906px", "padding-left: 26.3906px", "margin-top: 8.79688px", "margin-left: 17.5938px", "margin-right: 17.5938px", "width: 712.031px"]"#,
            ),
            (
                r#"G('#c',["padding-top", "padding-left", "margin-top", "margin-left", "margin-right", "width"])"#,
                r#"["padding-top: 0px", "padding-left: 0px", "margin-top: -4.8px", "margin-left: 320px", "margin-right: 320px", "width: 160px"]"#,
            ),
        ],
    },
    Case {
        name: "fixed-lengths-positioned",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}
#p{width:333px;height:200px;position:relative}
#r1{position:relative;left:5.3px;top:7.77px;width:5.3px;height:6.1px;padding:1.1px;margin:2.3px}
#r2{position:relative;left:3.3%;bottom:2.2%}
#s1{position:sticky;top:5.3px;left:3.3%}
#a1{position:absolute;left:5.3px;top:7.77px;width:5.3px;height:6.1px}
#a2{position:absolute;right:5.3px;bottom:3.3%;width:10px;height:10px}
</style></head><body><div id=p><div id=r1></div><div id=r2></div><div id=s1></div><div id=a1></div><div id=a2></div></div><script>function G(sel, ps, pseudo) { const s = getComputedStyle(document.querySelector(sel), pseudo); return ps.map(p => p + ': ' + s.getPropertyValue(p)); }
</script></body></html>",
        queries: &[
            (
                r#"G('#r1',["left", "right", "top", "bottom", "width", "height", "padding-top", "margin-top"])"#,
                r#"["left: 5.3px", "right: -5.3px", "top: 7.77px", "bottom: -7.77px", "width: 5.29688px", "height: 6.09375px", "padding-top: 1.1px", "margin-top: 2.3px"]"#,
            ),
            (
                r#"G('#r2',["left", "right", "top", "bottom", "width", "height", "padding-top", "margin-top"])"#,
                r#"["left: 10.9844px", "right: -10.989px", "top: -4.4px", "bottom: 4.39062px", "width: 333px", "height: 0px", "padding-top: 0px", "margin-top: 0px"]"#,
            ),
            (
                r#"G('#s1',["left", "right", "top", "bottom", "width", "height", "padding-top", "margin-top"])"#,
                r#"["left: 26.3906px", "right: auto", "top: 5.3px", "bottom: auto", "width: 333px", "height: 0px", "padding-top: 0px", "margin-top: 0px"]"#,
            ),
            (
                r#"G('#a1',["left", "right", "top", "bottom", "width", "height", "padding-top", "margin-top"])"#,
                r#"["left: 5.3px", "right: 322.406px", "top: 7.77px", "bottom: 186.141px", "width: 5.29688px", "height: 6.09375px", "padding-top: 0px", "margin-top: 0px"]"#,
            ),
            (
                r#"G('#a2',["left", "right", "top", "bottom", "width", "height", "padding-top", "margin-top"])"#,
                r#"["left: 317.703px", "right: 5.3px", "top: 183.406px", "bottom: 6.59375px", "width: 10px", "height: 10px", "padding-top: 0px", "margin-top: 0px"]"#,
            ),
        ],
    },
];
