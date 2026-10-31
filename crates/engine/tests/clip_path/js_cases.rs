// The cases of `tools/probes/clip-path.json` with the values that Chromium 148 gave
// (`just probe tools/probes/clip-path.json`, three runs, all equal); where swb departs on
// purpose, the test overrides the value. Generated.

use crate::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "cp-syntax-keywords",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r##"E(() => T(["none", "initial", "inherit", "unset", "revert", "NONE", "auto", "url(#a)", "url('#a')", "url(\"#a\")", "url()", "url(#a) url(#b)", "url(#a) border-box", "none border-box", "border-box none", "var(--x)", "inherit border-box"]))"##,
            r##""\"none\" => true [none] {none} <clip-path: none;>\n\"initial\" => true [initial] {none} <clip-path: initial;>\n\"inherit\" => true [inherit] {none} <clip-path: inherit;>\n\"unset\" => true [unset] {none} <clip-path: unset;>\n\"revert\" => true [revert] {none} <clip-path: revert;>\n\"NONE\" => true [none] {none} <clip-path: none;>\n\"auto\" => false [] {} <null>\n\"url(#a)\" => true [url(\"#a\")] {url(\"#a\")} <clip-path: url(\"#a\");>\n\"url('#a')\" => true [url(\"#a\")] {url(\"#a\")} <clip-path: url(\"#a\");>\n\"url(\\\"#a\\\")\" => true [url(\"#a\")] {url(\"#a\")} <clip-path: url(\"#a\");>\n\"url()\" => true [url(\"\")] {url(\"\")} <clip-path: url(\"\");>\n\"url(#a) url(#b)\" => false [] {} <null>\n\"url(#a) border-box\" => false [] {} <null>\n\"none border-box\" => false [] {} <null>\n\"border-box none\" => false [] {} <null>\n\"var(--x)\" => true [var(--x)] {none} <clip-path: var(--x);>\n\"inherit border-box\" => false [] {} <null>""##,
        )],
    },
    Case {
        name: "cp-syntax-inset",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["inset(10px)", "inset(10px 20px)", "inset(10px 20px 30px)", "inset(10px 20px 30px 40px)", "inset(1px 2px 3px 4px 5px)", "inset()", "inset(0)", "inset(-5px)", "inset(10%)", "inset(1em 2em)", "inset(calc(10px + 5%))", "inset(calc(10px + 5px))", "inset(10px,20px)", "inset(auto)", "inset(10)", "inset(1px round)", "INSET(10px)", "inset( 10px )", "inset(10px round 5px)", "inset(10px round 5px 10px)", "inset(10px round 5px 10px 15px 20px)", "inset(10px round 5px / 10px)", "inset(10px round 5px 10px / 1px 2px 3px 4px)", "inset(10px round 10%)", "inset(10px round 0)", "inset(10px round -5px)", "inset(10px round 5px 6px 7px 8px 9px)", "inset(10px 20px round 50%)", "inset(round 5px)", "inset(10px round 5px 5px)", "inset(10px round 5px 5px 5px 5px / 5px)", "inset(10px round 5px / 5px 5px)"]))"#,
            r#""\"inset(10px)\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset(10px 20px)\" => true [inset(10px 20px)] {inset(10px 20px)} <clip-path: inset(10px 20px);>\n\"inset(10px 20px 30px)\" => true [inset(10px 20px 30px)] {inset(10px 20px 30px)} <clip-path: inset(10px 20px 30px);>\n\"inset(10px 20px 30px 40px)\" => true [inset(10px 20px 30px 40px)] {inset(10px 20px 30px 40px)} <clip-path: inset(10px 20px 30px 40px);>\n\"inset(1px 2px 3px 4px 5px)\" => false [] {} <null>\n\"inset()\" => false [] {} <null>\n\"inset(0)\" => true [inset(0px)] {inset(0px)} <clip-path: inset(0px);>\n\"inset(-5px)\" => true [inset(-5px)] {inset(-5px)} <clip-path: inset(-5px);>\n\"inset(10%)\" => true [inset(10%)] {inset(10%)} <clip-path: inset(10%);>\n\"inset(1em 2em)\" => true [inset(1em 2em)] {inset(20px 40px)} <clip-path: inset(1em 2em);>\n\"inset(calc(10px + 5%))\" => true [inset(calc(5% + 10px))] {inset(calc(5% + 10px))} <clip-path: inset(calc(5% + 10px));>\n\"inset(calc(10px + 5px))\" => true [inset(calc(15px))] {inset(15px)} <clip-path: inset(calc(15px));>\n\"inset(10px,20px)\" => false [] {} <null>\n\"inset(auto)\" => false [] {} <null>\n\"inset(10)\" => false [] {} <null>\n\"inset(1px round)\" => false [] {} <null>\n\"INSET(10px)\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset( 10px )\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset(10px round 5px)\" => true [inset(10px round 5px)] {inset(10px round 5px)} <clip-path: inset(10px round 5px);>\n\"inset(10px round 5px 10px)\" => true [inset(10px round 5px 10px)] {inset(10px round 5px 10px)} <clip-path: inset(10px round 5px 10px);>\n\"inset(10px round 5px 10px 15px 20px)\" => true [inset(10px round 5px 10px 15px 20px)] {inset(10px round 5px 10px 15px 20px)} <clip-path: inset(10px round 5px 10px 15px 20px);>\n\"inset(10px round 5px / 10px)\" => true [inset(10px round 5px / 10px)] {inset(10px round 5px / 10px)} <clip-path: inset(10px round 5px / 10px);>\n\"inset(10px round 5px 10px / 1px 2px 3px 4px)\" => true [inset(10px round 5px 10px / 1px 2px 3px 4px)] {inset(10px round 5px 10px / 1px 2px 3px 4px)} <clip-path: inset(10px round 5px 10px / 1px 2px 3px 4px);>\n\"inset(10px round 10%)\" => true [inset(10px round 10%)] {inset(10px round 10%)} <clip-path: inset(10px round 10%);>\n\"inset(10px round 0)\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset(10px round -5px)\" => false [] {} <null>\n\"inset(10px round 5px 6px 7px 8px 9px)\" => false [] {} <null>\n\"inset(10px 20px round 50%)\" => true [inset(10px 20px round 50%)] {inset(10px 20px round 50%)} <clip-path: inset(10px 20px round 50%);>\n\"inset(round 5px)\" => false [] {} <null>\n\"inset(10px round 5px 5px)\" => true [inset(10px round 5px)] {inset(10px round 5px)} <clip-path: inset(10px round 5px);>\n\"inset(10px round 5px 5px 5px 5px / 5px)\" => true [inset(10px round 5px)] {inset(10px round 5px)} <clip-path: inset(10px round 5px);>\n\"inset(10px round 5px / 5px 5px)\" => true [inset(10px round 5px)] {inset(10px round 5px)} <clip-path: inset(10px round 5px);>""#,
        )],
    },
    Case {
        name: "cp-syntax-circle",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["circle()", "circle(50%)", "circle(10px)", "circle(closest-side)", "circle(farthest-side)", "circle(at center)", "circle(at 10px 20px)", "circle(50% at 10px 20px)", "circle(at left top)", "circle(at top left)", "circle(at right 10px bottom 20px)", "circle(at left 10px top 20px)", "circle(-5px)", "circle(0)", "circle(0px)", "circle(10px at center)", "circle(at 10px)", "circle(at 10%)", "circle(at top)", "circle(at left)", "circle(at center center)", "circle(at 50% 50%)", "circle(at 0 0)", "circle(10px 20px)", "circle(10px at)", "circle(calc(10px + 5%))", "circle(1em)", "circle(at 1em 2em)", "circle(closest-side at 10px 10px)", "circle(at 10px 20px 30px)", "circle(at left 10px top)", "circle(at bottom right)", "circle(at 10px top)", "circle(at top 10px)", "circle(at right 10px)", "circle(at center 10px)", "circle(at 10px center)", "circle(at 10px 20px, 30px)", "CIRCLE(50%)", "circle(50%at 10px 10px)", "circle(50% at 10px 10px)", "circle(  50%   at   10px   10px  )", "circle(at 50% 50% 50%)", "circle(sqrt(4) * 1px)"]))"#,
            r#""\"circle()\" => true [circle()] {circle()} <clip-path: circle();>\n\"circle(50%)\" => true [circle(50%)] {circle(50%)} <clip-path: circle(50%);>\n\"circle(10px)\" => true [circle(10px)] {circle(10px)} <clip-path: circle(10px);>\n\"circle(closest-side)\" => true [circle()] {circle()} <clip-path: circle();>\n\"circle(farthest-side)\" => true [circle(farthest-side)] {circle(farthest-side)} <clip-path: circle(farthest-side);>\n\"circle(at center)\" => true [circle(at 50% 50%)] {circle(at 50% 50%)} <clip-path: circle(at 50% 50%);>\n\"circle(at 10px 20px)\" => true [circle(at 10px 20px)] {circle(at 10px 20px)} <clip-path: circle(at 10px 20px);>\n\"circle(50% at 10px 20px)\" => true [circle(50% at 10px 20px)] {circle(50% at 10px 20px)} <clip-path: circle(50% at 10px 20px);>\n\"circle(at left top)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(at top left)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(at right 10px bottom 20px)\" => true [circle(at right 10px bottom 20px)] {circle(at right 10px bottom 20px)} <clip-path: circle(at right 10px bottom 20px);>\n\"circle(at left 10px top 20px)\" => true [circle(at 10px 20px)] {circle(at 10px 20px)} <clip-path: circle(at 10px 20px);>\n\"circle(-5px)\" => false [] {} <null>\n\"circle(0)\" => true [circle(0px)] {circle(0px)} <clip-path: circle(0px);>\n\"circle(0px)\" => true [circle(0px)] {circle(0px)} <clip-path: circle(0px);>\n\"circle(10px at center)\" => true [circle(10px at 50% 50%)] {circle(10px at 50% 50%)} <clip-path: circle(10px at 50% 50%);>\n\"circle(at 10px)\" => true [circle(at 10px 50%)] {circle(at 10px 50%)} <clip-path: circle(at 10px 50%);>\n\"circle(at 10%)\" => true [circle(at 10% 50%)] {circle(at 10% 50%)} <clip-path: circle(at 10% 50%);>\n\"circle(at top)\" => true [circle(at 50% 0%)] {circle(at 50% 0%)} <clip-path: circle(at 50% 0%);>\n\"circle(at left)\" => true [circle(at 0% 50%)] {circle(at 0% 50%)} <clip-path: circle(at 0% 50%);>\n\"circle(at center center)\" => true [circle(at 50% 50%)] {circle(at 50% 50%)} <clip-path: circle(at 50% 50%);>\n\"circle(at 50% 50%)\" => true [circle(at 50% 50%)] {circle(at 50% 50%)} <clip-path: circle(at 50% 50%);>\n\"circle(at 0 0)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(10px 20px)\" => false [] {} <null>\n\"circle(10px at)\" => false [] {} <null>\n\"circle(calc(10px + 5%))\" => true [circle(calc(5% + 10px))] {circle(calc(5% + 10px))} <clip-path: circle(calc(5% + 10px));>\n\"circle(1em)\" => true [circle(1em)] {circle(20px)} <clip-path: circle(1em);>\n\"circle(at 1em 2em)\" => true [circle(at 1em 2em)] {circle(at 20px 40px)} <clip-path: circle(at 1em 2em);>\n\"circle(closest-side at 10px 10px)\" => true [circle(at 10px 10px)] {circle(at 10px 10px)} <clip-path: circle(at 10px 10px);>\n\"circle(at 10px 20px 30px)\" => false [] {} <null>\n\"circle(at left 10px top)\" => false [] {} <null>\n\"circle(at bottom right)\" => true [circle(at 100% 100%)] {circle(at 100% 100%)} <clip-path: circle(at 100% 100%);>\n\"circle(at 10px top)\" => true [circle(at 10px 0%)] {circle(at 10px 0%)} <clip-path: circle(at 10px 0%);>\n\"circle(at top 10px)\" => false [] {} <null>\n\"circle(at right 10px)\" => true [circle(at 100% 10px)] {circle(at 100% 10px)} <clip-path: circle(at 100% 10px);>\n\"circle(at center 10px)\" => true [circle(at 50% 10px)] {circle(at 50% 10px)} <clip-path: circle(at 50% 10px);>\n\"circle(at 10px center)\" => true [circle(at 10px 50%)] {circle(at 10px 50%)} <clip-path: circle(at 10px 50%);>\n\"circle(at 10px 20px, 30px)\" => false [] {} <null>\n\"CIRCLE(50%)\" => true [circle(50%)] {circle(50%)} <clip-path: circle(50%);>\n\"circle(50%at 10px 10px)\" => true [circle(50% at 10px 10px)] {circle(50% at 10px 10px)} <clip-path: circle(50% at 10px 10px);>\n\"circle(50% at 10px 10px)\" => true [circle(50% at 10px 10px)] {circle(50% at 10px 10px)} <clip-path: circle(50% at 10px 10px);>\n\"circle(  50%   at   10px   10px  )\" => true [circle(50% at 10px 10px)] {circle(50% at 10px 10px)} <clip-path: circle(50% at 10px 10px);>\n\"circle(at 50% 50% 50%)\" => false [] {} <null>\n\"circle(sqrt(4) * 1px)\" => false [] {} <null>""#,
        )],
    },
    Case {
        name: "cp-syntax-ellipse",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["ellipse()", "ellipse(10px 20px)", "ellipse(closest-side farthest-side)", "ellipse(10px 20px at 30px 40px)", "ellipse(10px)", "ellipse(at center)", "ellipse(50% 25%)", "ellipse(50% 25% at 25% 75%)", "ellipse(closest-side closest-side)", "ellipse(10px closest-side)", "ellipse(-1px 2px)", "ellipse(0 0)", "ellipse(10px 20px 30px)", "ellipse(closest-side)", "ellipse(at 10px 20px)", "ellipse(farthest-side farthest-side at left top)"]))"#,
            r#""\"ellipse()\" => true [ellipse()] {ellipse()} <clip-path: ellipse();>\n\"ellipse(10px 20px)\" => true [ellipse(10px 20px)] {ellipse(10px 20px)} <clip-path: ellipse(10px 20px);>\n\"ellipse(closest-side farthest-side)\" => true [ellipse(closest-side farthest-side)] {ellipse(closest-side farthest-side)} <clip-path: ellipse(closest-side farthest-side);>\n\"ellipse(10px 20px at 30px 40px)\" => true [ellipse(10px 20px at 30px 40px)] {ellipse(10px 20px at 30px 40px)} <clip-path: ellipse(10px 20px at 30px 40px);>\n\"ellipse(10px)\" => false [] {} <null>\n\"ellipse(at center)\" => true [ellipse(at 50% 50%)] {ellipse(at 50% 50%)} <clip-path: ellipse(at 50% 50%);>\n\"ellipse(50% 25%)\" => true [ellipse(50% 25%)] {ellipse(50% 25%)} <clip-path: ellipse(50% 25%);>\n\"ellipse(50% 25% at 25% 75%)\" => true [ellipse(50% 25% at 25% 75%)] {ellipse(50% 25% at 25% 75%)} <clip-path: ellipse(50% 25% at 25% 75%);>\n\"ellipse(closest-side closest-side)\" => true [ellipse()] {ellipse()} <clip-path: ellipse();>\n\"ellipse(10px closest-side)\" => true [ellipse(10px closest-side)] {ellipse(10px closest-side)} <clip-path: ellipse(10px closest-side);>\n\"ellipse(-1px 2px)\" => false [] {} <null>\n\"ellipse(0 0)\" => true [ellipse(0px 0px)] {ellipse(0px 0px)} <clip-path: ellipse(0px 0px);>\n\"ellipse(10px 20px 30px)\" => false [] {} <null>\n\"ellipse(closest-side)\" => false [] {} <null>\n\"ellipse(at 10px 20px)\" => true [ellipse(at 10px 20px)] {ellipse(at 10px 20px)} <clip-path: ellipse(at 10px 20px);>\n\"ellipse(farthest-side farthest-side at left top)\" => true [ellipse(farthest-side farthest-side at 0% 0%)] {ellipse(farthest-side farthest-side at 0% 0%)} <clip-path: ellipse(farthest-side farthest-side at 0% 0%);>""#,
        )],
    },
    Case {
        name: "cp-syntax-polygon",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["polygon(0 0, 10px 0, 0 10px)", "polygon(nonzero, 0 0, 10px 0, 0 10px)", "polygon(evenodd, 0 0, 10px 0, 0 10px)", "polygon()", "polygon(0 0)", "polygon(0 0, 1px)", "polygon(0px 0px, 100% 0%, 50% 100%)", "polygon(calc(10px + 5%) 0, 10px 10px)", "polygon(0 0, 10px 0, 0 10px,)", "polygon(0 0 10px 0)", "polygon(NONZERO, 0 0, 10px 10px)", "polygon(0 0, 10px 10px) nonzero", "polygon(0 0,10px 10px)", "polygon(evenodd 0 0, 10px 10px)", "polygon(evenodd,)", "polygon(1em 2em, 3em 4em)", "polygon(-1px -2px, 3px 4px)", "polygon(0 0, 10px 0, 10px 10px, 0 10px) border-box", "polygon(0 0, 10px 0, 10px 10px, 0 10px) content-box"]))"#,
            r#""\"polygon(0 0, 10px 0, 0 10px)\" => true [polygon(0px 0px, 10px 0px, 0px 10px)] {polygon(0px 0px, 10px 0px, 0px 10px)} <clip-path: polygon(0px 0px, 10px 0px, 0px 10px);>\n\"polygon(nonzero, 0 0, 10px 0, 0 10px)\" => true [polygon(0px 0px, 10px 0px, 0px 10px)] {polygon(0px 0px, 10px 0px, 0px 10px)} <clip-path: polygon(0px 0px, 10px 0px, 0px 10px);>\n\"polygon(evenodd, 0 0, 10px 0, 0 10px)\" => true [polygon(evenodd, 0px 0px, 10px 0px, 0px 10px)] {polygon(evenodd, 0px 0px, 10px 0px, 0px 10px)} <clip-path: polygon(evenodd, 0px 0px, 10px 0px, 0px 10px);>\n\"polygon()\" => false [] {} <null>\n\"polygon(0 0)\" => true [polygon(0px 0px)] {polygon(0px 0px)} <clip-path: polygon(0px 0px);>\n\"polygon(0 0, 1px)\" => false [] {} <null>\n\"polygon(0px 0px, 100% 0%, 50% 100%)\" => true [polygon(0px 0px, 100% 0%, 50% 100%)] {polygon(0px 0px, 100% 0%, 50% 100%)} <clip-path: polygon(0px 0px, 100% 0%, 50% 100%);>\n\"polygon(calc(10px + 5%) 0, 10px 10px)\" => true [polygon(calc(5% + 10px) 0px, 10px 10px)] {polygon(calc(5% + 10px) 0px, 10px 10px)} <clip-path: polygon(calc(5% + 10px) 0px, 10px 10px);>\n\"polygon(0 0, 10px 0, 0 10px,)\" => false [] {} <null>\n\"polygon(0 0 10px 0)\" => false [] {} <null>\n\"polygon(NONZERO, 0 0, 10px 10px)\" => true [polygon(0px 0px, 10px 10px)] {polygon(0px 0px, 10px 10px)} <clip-path: polygon(0px 0px, 10px 10px);>\n\"polygon(0 0, 10px 10px) nonzero\" => false [] {} <null>\n\"polygon(0 0,10px 10px)\" => true [polygon(0px 0px, 10px 10px)] {polygon(0px 0px, 10px 10px)} <clip-path: polygon(0px 0px, 10px 10px);>\n\"polygon(evenodd 0 0, 10px 10px)\" => false [] {} <null>\n\"polygon(evenodd,)\" => false [] {} <null>\n\"polygon(1em 2em, 3em 4em)\" => true [polygon(1em 2em, 3em 4em)] {polygon(20px 40px, 60px 80px)} <clip-path: polygon(1em 2em, 3em 4em);>\n\"polygon(-1px -2px, 3px 4px)\" => true [polygon(-1px -2px, 3px 4px)] {polygon(-1px -2px, 3px 4px)} <clip-path: polygon(-1px -2px, 3px 4px);>\n\"polygon(0 0, 10px 0, 10px 10px, 0 10px) border-box\" => true [polygon(0px 0px, 10px 0px, 10px 10px, 0px 10px)] {polygon(0px 0px, 10px 0px, 10px 10px, 0px 10px)} <clip-path: polygon(0px 0px, 10px 0px, 10px 10px, 0px 10px);>\n\"polygon(0 0, 10px 0, 10px 10px, 0 10px) content-box\" => true [polygon(0px 0px, 10px 0px, 10px 10px, 0px 10px) content-box] {polygon(0px 0px, 10px 0px, 10px 10px, 0px 10px) content-box} <clip-path: polygon(0px 0px, 10px 0px, 10px 10px, 0px 10px) content-box;>""#,
        )],
    },
    Case {
        name: "cp-syntax-path",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["path('M0 0 L10 0 L10 10 z')", "path(\"M0 0 L10 0 L10 10 z\")", "path(evenodd, 'M0 0 h10 v10 z')", "path(nonzero, 'M0 0 h10 v10 z')", "path('invalid')", "path()", "path('')", "path('M0 0')", "path('M 0 0 L 10 0')", "path('M0,0L10,0L10,10Z')", "path('m0 0 l10 0 l0 10 z')", "path('M0 0 C 10 0 10 10 0 10')", "path('M0 0 L 10')", "path(M0 0 L10 0)", "path('M0 0 L10 0',)", "path(evenodd 'M0 0 h10')", "path('M0 0 h10 v10 z') content-box", "path('M0 0 A 10 10 0 0 1 20 0 z')", "path('M0 0 Q 10 10 20 0 T 40 0 S 10 10 5 5 z')", "path('M0 0 H 10 V 10 z m 5 5 h 3')", "path('M0 0 L10 0 L10 10 z  ')", "path(' M0 0 L10 0 L10 10 z')", "path('M0 0 L10 0 L10 10 z X')", "path('L0 0')", "path('M0 0 L1e1 0')", "path('M0 0 L.5 .5')", "path('M0 0 L-.5-.5')", "PATH('M0 0 L10 0')"]))"#,
            r#""\"path('M0 0 L10 0 L10 10 z')\" => true [path(\"M 0 0 L 10 0 L 10 10 Z\")] {path(\"M 0 0 L 10 0 L 10 10 Z\")} <clip-path: path(\"M 0 0 L 10 0 L 10 10 Z\");>\n\"path(\\\"M0 0 L10 0 L10 10 z\\\")\" => true [path(\"M 0 0 L 10 0 L 10 10 Z\")] {path(\"M 0 0 L 10 0 L 10 10 Z\")} <clip-path: path(\"M 0 0 L 10 0 L 10 10 Z\");>\n\"path(evenodd, 'M0 0 h10 v10 z')\" => true [path(evenodd, \"M 0 0 h 10 v 10 Z\")] {path(evenodd, \"M 0 0 H 10 V 10 Z\")} <clip-path: path(evenodd, \"M 0 0 h 10 v 10 Z\");>\n\"path(nonzero, 'M0 0 h10 v10 z')\" => true [path(\"M 0 0 h 10 v 10 Z\")] {path(evenodd, \"M 0 0 H 10 V 10 Z\")} <clip-path: path(\"M 0 0 h 10 v 10 Z\");>\n\"path('invalid')\" => false [] {} <null>\n\"path()\" => false [] {} <null>\n\"path('')\" => false [] {} <null>\n\"path('M0 0')\" => true [path(\"M 0 0\")] {path(\"M 0 0\")} <clip-path: path(\"M 0 0\");>\n\"path('M 0 0 L 10 0')\" => true [path(\"M 0 0 L 10 0\")] {path(\"M 0 0 L 10 0\")} <clip-path: path(\"M 0 0 L 10 0\");>\n\"path('M0,0L10,0L10,10Z')\" => true [path(\"M 0 0 L 10 0 L 10 10 Z\")] {path(\"M 0 0 L 10 0 L 10 10 Z\")} <clip-path: path(\"M 0 0 L 10 0 L 10 10 Z\");>\n\"path('m0 0 l10 0 l0 10 z')\" => true [path(\"m 0 0 l 10 0 l 0 10 Z\")] {path(\"M 0 0 L 10 0 L 10 10 Z\")} <clip-path: path(\"m 0 0 l 10 0 l 0 10 Z\");>\n\"path('M0 0 C 10 0 10 10 0 10')\" => true [path(\"M 0 0 C 10 0 10 10 0 10\")] {path(\"M 0 0 C 10 0 10 10 0 10\")} <clip-path: path(\"M 0 0 C 10 0 10 10 0 10\");>\n\"path('M0 0 L 10')\" => false [] {} <null>\n\"path(M0 0 L10 0)\" => false [] {} <null>\n\"path('M0 0 L10 0',)\" => false [] {} <null>\n\"path(evenodd 'M0 0 h10')\" => false [] {} <null>\n\"path('M0 0 h10 v10 z') content-box\" => true [path(\"M 0 0 h 10 v 10 Z\") content-box] {path(\"M 0 0 H 10 V 10 Z\") content-box} <clip-path: path(\"M 0 0 h 10 v 10 Z\") content-box;>\n\"path('M0 0 A 10 10 0 0 1 20 0 z')\" => true [path(\"M 0 0 A 10 10 0 0 1 20 0 Z\")] {path(\"M 0 0 A 10 10 0 0 1 20 0 Z\")} <clip-path: path(\"M 0 0 A 10 10 0 0 1 20 0 Z\");>\n\"path('M0 0 Q 10 10 20 0 T 40 0 S 10 10 5 5 z')\" => true [path(\"M 0 0 Q 10 10 20 0 T 40 0 S 10 10 5 5 Z\")] {path(\"M 0 0 Q 10 10 20 0 T 40 0 S 10 10 5 5 Z\")} <clip-path: path(\"M 0 0 Q 10 10 20 0 T 40 0 S 10 10 5 5 Z\");>\n\"path('M0 0 H 10 V 10 z m 5 5 h 3')\" => true [path(\"M 0 0 H 10 V 10 Z m 5 5 h 3\")] {path(\"M 0 0 H 10 V 10 Z M 5 5 H 8\")} <clip-path: path(\"M 0 0 H 10 V 10 Z m 5 5 h 3\");>\n\"path('M0 0 L10 0 L10 10 z  ')\" => true [path(\"M 0 0 L 10 0 L 10 10 Z\")] {path(\"M 0 0 L 10 0 L 10 10 Z\")} <clip-path: path(\"M 0 0 L 10 0 L 10 10 Z\");>\n\"path(' M0 0 L10 0 L10 10 z')\" => true [path(\"M 0 0 L 10 0 L 10 10 Z\")] {path(\"M 0 0 L 10 0 L 10 10 Z\")} <clip-path: path(\"M 0 0 L 10 0 L 10 10 Z\");>\n\"path('M0 0 L10 0 L10 10 z X')\" => false [] {} <null>\n\"path('L0 0')\" => false [] {} <null>\n\"path('M0 0 L1e1 0')\" => true [path(\"M 0 0 L 10 0\")] {path(\"M 0 0 L 10 0\")} <clip-path: path(\"M 0 0 L 10 0\");>\n\"path('M0 0 L.5 .5')\" => true [path(\"M 0 0 L 0.5 0.5\")] {path(\"M 0 0 L 0.5 0.5\")} <clip-path: path(\"M 0 0 L 0.5 0.5\");>\n\"path('M0 0 L-.5-.5')\" => true [path(\"M 0 0 L -0.5 -0.5\")] {path(\"M 0 0 L -0.5 -0.5\")} <clip-path: path(\"M 0 0 L -0.5 -0.5\");>\n\"PATH('M0 0 L10 0')\" => true [path(\"M 0 0 L 10 0\")] {path(\"M 0 0 L 10 0\")} <clip-path: path(\"M 0 0 L 10 0\");>""#,
        )],
    },
    Case {
        name: "cp-syntax-rect",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["rect(0 100% 100% 0)", "rect(auto auto auto auto)", "rect(10px 20px 30px 40px round 5px)", "rect(0 0 0 0)", "rect(1px 2px 3px)", "rect(1px, 2px, 3px, 4px)", "rect(10%)", "rect(auto)", "rect(1px 2px 3px 4px 5px)", "rect(-1px -2px -3px -4px)", "rect(10px 20px 30px 40px round 5px / 10px)", "rect(calc(10px + 5%) 0 0 0)"]))"#,
            r#""\"rect(0 100% 100% 0)\" => true [rect(0px 100% 100% 0px)] {inset(0px 0% 0% 0px)} <clip-path: rect(0px 100% 100% 0px);>\n\"rect(auto auto auto auto)\" => true [rect(auto auto auto auto)] {inset(0%)} <clip-path: rect(auto auto auto auto);>\n\"rect(10px 20px 30px 40px round 5px)\" => true [rect(10px 20px 30px 40px round 5px)] {inset(10px calc(100% - 20px) calc(100% - 30px) 40px round 5px)} <clip-path: rect(10px 20px 30px 40px round 5px);>\n\"rect(0 0 0 0)\" => true [rect(0px 0px 0px 0px)] {inset(0px 100% 100% 0px)} <clip-path: rect(0px 0px 0px 0px);>\n\"rect(1px 2px 3px)\" => false [] {} <null>\n\"rect(1px, 2px, 3px, 4px)\" => false [] {} <null>\n\"rect(10%)\" => false [] {} <null>\n\"rect(auto)\" => false [] {} <null>\n\"rect(1px 2px 3px 4px 5px)\" => false [] {} <null>\n\"rect(-1px -2px -3px -4px)\" => true [rect(-1px -2px -3px -4px)] {inset(-1px calc(100% + 2px) calc(100% + 3px) -4px)} <clip-path: rect(-1px -2px -3px -4px);>\n\"rect(10px 20px 30px 40px round 5px / 10px)\" => true [rect(10px 20px 30px 40px round 5px / 10px)] {inset(10px calc(100% - 20px) calc(100% - 30px) 40px round 5px / 10px)} <clip-path: rect(10px 20px 30px 40px round 5px / 10px);>\n\"rect(calc(10px + 5%) 0 0 0)\" => true [rect(calc(5% + 10px) 0px 0px 0px)] {inset(calc(5% + 10px) 100% 100% 0px)} <clip-path: rect(calc(5% + 10px) 0px 0px 0px);>""#,
        )],
    },
    Case {
        name: "cp-syntax-xywh",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["xywh(0 0 100% 100%)", "xywh(10px 20px 30px 40px round 5px)", "xywh(0 0 -1px 1px)", "xywh(0 0 0 0)", "xywh(10px 20px)", "xywh(10px 20px 30px)", "xywh(10px 20px 30px 40px 50px)", "xywh(-10px -20px 30px 40px)", "xywh(10% 20% 30% 40%)", "xywh(auto 0 1px 1px)", "xywh(1px 2px 3px 4px round 5px 6px 7px 8px / 9px)"]))"#,
            r#""\"xywh(0 0 100% 100%)\" => true [xywh(0px 0px 100% 100%)] {inset(0px 0% 0% 0px)} <clip-path: xywh(0px 0px 100% 100%);>\n\"xywh(10px 20px 30px 40px round 5px)\" => true [xywh(10px 20px 30px 40px round 5px)] {inset(20px calc(100% - 40px) calc(100% - 60px) 10px round 5px)} <clip-path: xywh(10px 20px 30px 40px round 5px);>\n\"xywh(0 0 -1px 1px)\" => false [] {} <null>\n\"xywh(0 0 0 0)\" => true [xywh(0px 0px 0px 0px)] {inset(0px 100% 100% 0px)} <clip-path: xywh(0px 0px 0px 0px);>\n\"xywh(10px 20px)\" => false [] {} <null>\n\"xywh(10px 20px 30px)\" => false [] {} <null>\n\"xywh(10px 20px 30px 40px 50px)\" => false [] {} <null>\n\"xywh(-10px -20px 30px 40px)\" => true [xywh(-10px -20px 30px 40px)] {inset(-20px calc(100% - 20px) calc(100% - 20px) -10px)} <clip-path: xywh(-10px -20px 30px 40px);>\n\"xywh(10% 20% 30% 40%)\" => true [xywh(10% 20% 30% 40%)] {inset(20% 60% 40% 10%)} <clip-path: xywh(10% 20% 30% 40%);>\n\"xywh(auto 0 1px 1px)\" => false [] {} <null>\n\"xywh(1px 2px 3px 4px round 5px 6px 7px 8px / 9px)\" => true [xywh(1px 2px 3px 4px round 5px 6px 7px 8px / 9px)] {inset(2px calc(100% - 4px) calc(100% - 6px) 1px round 5px 6px 7px 8px / 9px)} <clip-path: xywh(1px 2px 3px 4px round 5px 6px 7px 8px / 9px);>""#,
        )],
    },
    Case {
        name: "cp-syntax-boxes",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["border-box", "padding-box", "content-box", "margin-box", "fill-box", "stroke-box", "view-box", "BORDER-BOX", "circle(50%) border-box", "border-box circle(50%)", "circle(50%) padding-box margin-box", "border-box border-box", "circle() circle()", "inset(0) fill-box", "inset(0) stroke-box", "inset(0) view-box", "view-box inset(0)", "content-box polygon(0 0, 10px 10px)", "polygon(0 0, 10px 10px) content-box", "path('M0 0 h10') margin-box", "none content-box", "url(#a) content-box", "circle(at center) content-box", "circle(50%)  content-box", "scroll-box", "text", "no-clip", "geometry-box", "border-box padding-box"]))"#,
            r#""\"border-box\" => true [border-box] {border-box} <clip-path: border-box;>\n\"padding-box\" => true [padding-box] {padding-box} <clip-path: padding-box;>\n\"content-box\" => true [content-box] {content-box} <clip-path: content-box;>\n\"margin-box\" => true [margin-box] {margin-box} <clip-path: margin-box;>\n\"fill-box\" => true [fill-box] {fill-box} <clip-path: fill-box;>\n\"stroke-box\" => true [stroke-box] {stroke-box} <clip-path: stroke-box;>\n\"view-box\" => true [view-box] {view-box} <clip-path: view-box;>\n\"BORDER-BOX\" => true [border-box] {border-box} <clip-path: border-box;>\n\"circle(50%) border-box\" => true [circle(50%)] {circle(50%)} <clip-path: circle(50%);>\n\"border-box circle(50%)\" => true [circle(50%)] {circle(50%)} <clip-path: circle(50%);>\n\"circle(50%) padding-box margin-box\" => false [] {} <null>\n\"border-box border-box\" => false [] {} <null>\n\"circle() circle()\" => false [] {} <null>\n\"inset(0) fill-box\" => true [inset(0px) fill-box] {inset(0px) fill-box} <clip-path: inset(0px) fill-box;>\n\"inset(0) stroke-box\" => true [inset(0px) stroke-box] {inset(0px) stroke-box} <clip-path: inset(0px) stroke-box;>\n\"inset(0) view-box\" => true [inset(0px) view-box] {inset(0px) view-box} <clip-path: inset(0px) view-box;>\n\"view-box inset(0)\" => true [inset(0px) view-box] {inset(0px) view-box} <clip-path: inset(0px) view-box;>\n\"content-box polygon(0 0, 10px 10px)\" => true [polygon(0px 0px, 10px 10px) content-box] {polygon(0px 0px, 10px 10px) content-box} <clip-path: polygon(0px 0px, 10px 10px) content-box;>\n\"polygon(0 0, 10px 10px) content-box\" => true [polygon(0px 0px, 10px 10px) content-box] {polygon(0px 0px, 10px 10px) content-box} <clip-path: polygon(0px 0px, 10px 10px) content-box;>\n\"path('M0 0 h10') margin-box\" => true [path(\"M 0 0 h 10\") margin-box] {path(\"M 0 0 H 10\") margin-box} <clip-path: path(\"M 0 0 h 10\") margin-box;>\n\"none content-box\" => false [] {} <null>\n\"url(#a) content-box\" => false [] {} <null>\n\"circle(at center) content-box\" => true [circle(at 50% 50%) content-box] {circle(at 50% 50%) content-box} <clip-path: circle(at 50% 50%) content-box;>\n\"circle(50%)  content-box\" => true [circle(50%) content-box] {circle(50%) content-box} <clip-path: circle(50%) content-box;>\n\"scroll-box\" => false [] {} <null>\n\"text\" => false [] {} <null>\n\"no-clip\" => false [] {} <null>\n\"geometry-box\" => false [] {} <null>\n\"border-box padding-box\" => false [] {} <null>""#,
        )],
    },
    Case {
        name: "cp-syntax-pos",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["circle(at right 10px top 20px)", "circle(at left 10px bottom 20px)", "circle(at bottom 10px right 20px)", "circle(at top 10px left 20px)", "circle(at 10px top 20px)", "circle(at left 10px top 20px)", "circle(at right 10px bottom 20px)", "circle(at center top 10px)", "circle(at left 10px center)", "circle(at right 20%)", "circle(at 20% bottom)", "circle(at bottom 20%)", "circle(at right bottom)", "circle(at left bottom 10px)", "circle(at right 10%)", "circle(at top left 10px)", "circle(at left top 10px)", "circle(at calc(1px + 2%) 5px)", "circle(at left 10px top)", "circle(at 10px 20px) content-box", "circle(at right 10px bottom 20px) border-box", "circle(at center bottom)", "circle(at center left)", "circle(at 50% left)", "circle(at left 50%)", "circle(at top 50%)", "circle(at 10px right)", "circle(at 0 10px)", "circle(at 10px 0)", "circle(at 0)", "ellipse(at 0 0)", "circle(at 0px 0px)", "circle(at left 0 top 0)", "circle(at right 0 bottom 0)", "circle(at 0% 0)", "circle(at calc(0px) 0)", "circle(at 1px 0)"]))"#,
            r#""\"circle(at right 10px top 20px)\" => true [circle(at right 10px top 20px)] {circle(at right 10px top 20px)} <clip-path: circle(at right 10px top 20px);>\n\"circle(at left 10px bottom 20px)\" => true [circle(at left 10px bottom 20px)] {circle(at left 10px bottom 20px)} <clip-path: circle(at left 10px bottom 20px);>\n\"circle(at bottom 10px right 20px)\" => true [circle(at right 20px bottom 10px)] {circle(at right 20px bottom 10px)} <clip-path: circle(at right 20px bottom 10px);>\n\"circle(at top 10px left 20px)\" => true [circle(at 20px 10px)] {circle(at 20px 10px)} <clip-path: circle(at 20px 10px);>\n\"circle(at 10px top 20px)\" => false [] {} <null>\n\"circle(at left 10px top 20px)\" => true [circle(at 10px 20px)] {circle(at 10px 20px)} <clip-path: circle(at 10px 20px);>\n\"circle(at right 10px bottom 20px)\" => true [circle(at right 10px bottom 20px)] {circle(at right 10px bottom 20px)} <clip-path: circle(at right 10px bottom 20px);>\n\"circle(at center top 10px)\" => false [] {} <null>\n\"circle(at left 10px center)\" => false [] {} <null>\n\"circle(at right 20%)\" => true [circle(at 100% 20%)] {circle(at 100% 20%)} <clip-path: circle(at 100% 20%);>\n\"circle(at 20% bottom)\" => true [circle(at 20% 100%)] {circle(at 20% 100%)} <clip-path: circle(at 20% 100%);>\n\"circle(at bottom 20%)\" => false [] {} <null>\n\"circle(at right bottom)\" => true [circle(at 100% 100%)] {circle(at 100% 100%)} <clip-path: circle(at 100% 100%);>\n\"circle(at left bottom 10px)\" => false [] {} <null>\n\"circle(at right 10%)\" => true [circle(at 100% 10%)] {circle(at 100% 10%)} <clip-path: circle(at 100% 10%);>\n\"circle(at top left 10px)\" => false [] {} <null>\n\"circle(at left top 10px)\" => false [] {} <null>\n\"circle(at calc(1px + 2%) 5px)\" => true [circle(at calc(2% + 1px) 5px)] {circle(at calc(2% + 1px) 5px)} <clip-path: circle(at calc(2% + 1px) 5px);>\n\"circle(at left 10px top)\" => false [] {} <null>\n\"circle(at 10px 20px) content-box\" => true [circle(at 10px 20px) content-box] {circle(at 10px 20px) content-box} <clip-path: circle(at 10px 20px) content-box;>\n\"circle(at right 10px bottom 20px) border-box\" => true [circle(at right 10px bottom 20px)] {circle(at right 10px bottom 20px)} <clip-path: circle(at right 10px bottom 20px);>\n\"circle(at center bottom)\" => true [circle(at 50% 100%)] {circle(at 50% 100%)} <clip-path: circle(at 50% 100%);>\n\"circle(at center left)\" => true [circle(at 0% 50%)] {circle(at 0% 50%)} <clip-path: circle(at 0% 50%);>\n\"circle(at 50% left)\" => false [] {} <null>\n\"circle(at left 50%)\" => true [circle(at 0% 50%)] {circle(at 0% 50%)} <clip-path: circle(at 0% 50%);>\n\"circle(at top 50%)\" => false [] {} <null>\n\"circle(at 10px right)\" => false [] {} <null>\n\"circle(at 0 10px)\" => true [circle(at 0% 10px)] {circle(at 0% 10px)} <clip-path: circle(at 0% 10px);>\n\"circle(at 10px 0)\" => true [circle(at 10px 0%)] {circle(at 10px 0%)} <clip-path: circle(at 10px 0%);>\n\"circle(at 0)\" => true [circle(at 0% 50%)] {circle(at 0% 50%)} <clip-path: circle(at 0% 50%);>\n\"ellipse(at 0 0)\" => true [ellipse(at 0% 0%)] {ellipse(at 0% 0%)} <clip-path: ellipse(at 0% 0%);>\n\"circle(at 0px 0px)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(at left 0 top 0)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(at right 0 bottom 0)\" => true [circle(at 100% 100%)] {circle(at 100% 100%)} <clip-path: circle(at 100% 100%);>\n\"circle(at 0% 0)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(at calc(0px) 0)\" => true [circle(at 0% 0%)] {circle(at 0% 0%)} <clip-path: circle(at 0% 0%);>\n\"circle(at 1px 0)\" => true [circle(at 1px 0%)] {circle(at 1px 0%)} <clip-path: circle(at 1px 0%);>""#,
        )],
    },
    Case {
        name: "cp-syntax-inset2",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[(
            r#"E(() => T(["inset(10px 10px)", "inset(10px 10px 10px)", "inset(10px 10px 10px 10px)", "inset(10px 20px 10px 20px)", "inset(10px 20px 30px 20px)", "inset(10px 20px 10px)", "inset(1px 2px 3px 4px round 5px 5px 5px 5px)", "inset(0 round 1px 2px 1px 2px)", "inset(0 round 1px 2px 3px 2px)", "inset(0 round 1px 2px 3px)", "inset(0 round 1px 2px 1px)", "inset(0 round 1px 1px / 2px 2px)", "inset(0 round 1px / 1px 2px 3px 4px)", "inset(0 round 10px 10px 10px 10px / 10px 10px)", "inset(0 round 1px 2px / 3px)", "inset(0 round 1px 2px 3px 4px / 1px 2px 3px 4px)", "inset(0 round 1px 1px 2px 2px)", "rect(1px 2px 3px 4px round 5px 5px)", "xywh(1px 2px 3px 4px round 5px 5px 5px 5px)", "polygon(10% 10%, calc(100% - 10px) 50%)", "circle(50%) fill-box", "circle(50%) stroke-box", "ellipse(50% 50%) padding-box", "inset(1px) margin-box"]))"#,
            r#""\"inset(10px 10px)\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset(10px 10px 10px)\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset(10px 10px 10px 10px)\" => true [inset(10px)] {inset(10px)} <clip-path: inset(10px);>\n\"inset(10px 20px 10px 20px)\" => true [inset(10px 20px)] {inset(10px 20px)} <clip-path: inset(10px 20px);>\n\"inset(10px 20px 30px 20px)\" => true [inset(10px 20px 30px)] {inset(10px 20px 30px)} <clip-path: inset(10px 20px 30px);>\n\"inset(10px 20px 10px)\" => true [inset(10px 20px)] {inset(10px 20px)} <clip-path: inset(10px 20px);>\n\"inset(1px 2px 3px 4px round 5px 5px 5px 5px)\" => true [inset(1px 2px 3px 4px round 5px)] {inset(1px 2px 3px 4px round 5px)} <clip-path: inset(1px 2px 3px 4px round 5px);>\n\"inset(0 round 1px 2px 1px 2px)\" => true [inset(0px round 1px 2px)] {inset(0px round 1px 2px)} <clip-path: inset(0px round 1px 2px);>\n\"inset(0 round 1px 2px 3px 2px)\" => true [inset(0px round 1px 2px 3px)] {inset(0px round 1px 2px 3px)} <clip-path: inset(0px round 1px 2px 3px);>\n\"inset(0 round 1px 2px 3px)\" => true [inset(0px round 1px 2px 3px)] {inset(0px round 1px 2px 3px)} <clip-path: inset(0px round 1px 2px 3px);>\n\"inset(0 round 1px 2px 1px)\" => true [inset(0px round 1px 2px)] {inset(0px round 1px 2px)} <clip-path: inset(0px round 1px 2px);>\n\"inset(0 round 1px 1px / 2px 2px)\" => true [inset(0px round 1px / 2px)] {inset(0px round 1px / 2px)} <clip-path: inset(0px round 1px / 2px);>\n\"inset(0 round 1px / 1px 2px 3px 4px)\" => true [inset(0px round 1px / 1px 2px 3px 4px)] {inset(0px round 1px / 1px 2px 3px 4px)} <clip-path: inset(0px round 1px / 1px 2px 3px 4px);>\n\"inset(0 round 10px 10px 10px 10px / 10px 10px)\" => true [inset(0px round 10px)] {inset(0px round 10px)} <clip-path: inset(0px round 10px);>\n\"inset(0 round 1px 2px / 3px)\" => true [inset(0px round 1px 2px / 3px)] {inset(0px round 1px 2px / 3px)} <clip-path: inset(0px round 1px 2px / 3px);>\n\"inset(0 round 1px 2px 3px 4px / 1px 2px 3px 4px)\" => true [inset(0px round 1px 2px 3px 4px)] {inset(0px round 1px 2px 3px 4px)} <clip-path: inset(0px round 1px 2px 3px 4px);>\n\"inset(0 round 1px 1px 2px 2px)\" => true [inset(0px round 1px 1px 2px 2px)] {inset(0px round 1px 1px 2px 2px)} <clip-path: inset(0px round 1px 1px 2px 2px);>\n\"rect(1px 2px 3px 4px round 5px 5px)\" => true [rect(1px 2px 3px 4px round 5px)] {inset(1px calc(100% - 2px) calc(100% - 3px) 4px round 5px)} <clip-path: rect(1px 2px 3px 4px round 5px);>\n\"xywh(1px 2px 3px 4px round 5px 5px 5px 5px)\" => true [xywh(1px 2px 3px 4px round 5px)] {inset(2px calc(100% - 4px) calc(100% - 6px) 1px round 5px)} <clip-path: xywh(1px 2px 3px 4px round 5px);>\n\"polygon(10% 10%, calc(100% - 10px) 50%)\" => true [polygon(10% 10%, calc(100% - 10px) 50%)] {polygon(10% 10%, calc(100% - 10px) 50%)} <clip-path: polygon(10% 10%, calc(100% - 10px) 50%);>\n\"circle(50%) fill-box\" => true [circle(50%) fill-box] {circle(50%) fill-box} <clip-path: circle(50%) fill-box;>\n\"circle(50%) stroke-box\" => true [circle(50%) stroke-box] {circle(50%) stroke-box} <clip-path: circle(50%) stroke-box;>\n\"ellipse(50% 50%) padding-box\" => true [ellipse(50% 50%) padding-box] {ellipse(50% 50%) padding-box} <clip-path: ellipse(50% 50%) padding-box;>\n\"inset(1px) margin-box\" => true [inset(1px) margin-box] {inset(1px) margin-box} <clip-path: inset(1px) margin-box;>""#,
        )],
    },
    Case {
        name: "cp-syntax-misc",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}#c{width:200px;height:100px;font-size:20px}</style></head><body><div id=c></div><script>
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : String(v); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function T(vs, p) { p = p || 'clip-path'; const c = document.getElementById('c'); return vs.map((v) => { const e = document.createElement('div'); e.style.setProperty(p, v); const s = e.style.getPropertyValue(p); let comp = ''; if (s) { c.appendChild(e); comp = getComputedStyle(e).getPropertyValue(p); e.remove(); } return S(v) + ' => ' + CSS.supports(p, v) + ' [' + s + '] {' + comp + '} <' + e.getAttribute('style') + '>'; }).join('\n'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => T(["circle(50%)", "none", "url(#a)", "border-box"], '-webkit-clip-path'))"#,
                r##""\"circle(50%)\" => true [circle(50%)] {circle(50%)} <clip-path: circle(50%);>\n\"none\" => true [none] {none} <clip-path: none;>\n\"url(#a)\" => true [url(\"#a\")] {url(\"#a\")} <clip-path: url(\"#a\");>\n\"border-box\" => true [border-box] {border-box} <clip-path: border-box;>""##,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.webkitClipPath = 'circle(50%)'; return [e.style.clipPath, e.style.cssText, 'webkitClipPath' in e.style]; })())",
                r#""[\"circle(50%)\",\"clip-path: circle(50%);\",true]""#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.cssText = 'clip-path: polygon(0 0, 10px 0, 0 10px); clip-path: bogus(1)'; return [e.style.clipPath, e.style.cssText, e.style.length]; })())",
                r#""[\"polygon(0px 0px, 10px 0px, 0px 10px)\",\"clip-path: polygon(0px 0px, 10px 0px, 0px 10px);\",1]""#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.clipPath = 'circle(50% at 10px 20px) content-box'; return [e.style.clipPath, e.style.getPropertyValue('clip-path'), e.style.cssText, e.style.length, e.style.item(0)]; })())",
                r#""[\"circle(50% at 10px 20px) content-box\",\"circle(50% at 10px 20px) content-box\",\"clip-path: circle(50% at 10px 20px) content-box;\",1,\"clip-path\"]""#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.clipPath = 'circle(50%)'; e.style.clipPath = 'bogus'; return [e.style.clipPath]; })())",
                r#""[\"circle(50%)\"]""#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.clipPath = 'circle(50%)'; e.style.clipPath = ''; return [e.style.clipPath, e.style.cssText]; })())",
                r#""[\"\",\"\"]""#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.clipPath = 'inset(1px) !important'; return [e.style.clipPath, e.style.cssText]; })())",
                r#""[\"\",\"\"]""#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.setProperty('clip-path', 'inset(1px)', 'important'); return [e.style.clipPath, e.style.cssText, e.style.getPropertyPriority('clip-path')]; })())",
                r#""[\"inset(1px)\",\"clip-path: inset(1px) !important;\",\"important\"]""#,
            ),
            (
                r"E(() => (() => { const c = document.getElementById('c'); const e = document.createElement('div'); e.style.fontSize = '10px'; c.appendChild(e); const r = []; for (const v of ['inset(1em)', 'circle(2em at 3em 4em)', 'ellipse(1em 2em)', 'polygon(1em 2em, 3em 4em)', 'inset(calc(1em + 5%) round 1em)', 'inset(1rem)', 'inset(1vw 2vh)', 'inset(1in)', 'inset(1pt)', 'inset(1cm)', 'circle(calc(10px + 50%))', 'circle(calc(100% - 10px) at calc(50% + 1px) calc(1em))', 'polygon(calc(1px + 1px) calc(2px * 2), 1px 1px)', 'circle(min(10px, 20px))', 'inset(max(1px, 1em))', 'circle(1ex)', 'circle(1ch)', 'inset(1Q)', 'inset(1mm)', 'inset(1pc)', 'inset(1e1px)', 'inset(0.5px)', 'inset(.5px)', 'inset(+5px)', 'inset(10.0px)', 'inset(1.50px)']) { e.style.clipPath = v; r.push(v + ' => ' + getComputedStyle(e).clipPath + ' | ' + e.style.clipPath); } e.remove(); return r.join('\n'); })())",
                r#""inset(1em) => inset(10px) | inset(1em)\ncircle(2em at 3em 4em) => circle(20px at 30px 40px) | circle(2em at 3em 4em)\nellipse(1em 2em) => ellipse(10px 20px) | ellipse(1em 2em)\npolygon(1em 2em, 3em 4em) => polygon(10px 20px, 30px 40px) | polygon(1em 2em, 3em 4em)\ninset(calc(1em + 5%) round 1em) => inset(calc(5% + 10px) round 10px) | inset(calc(5% + 1em) round 1em)\ninset(1rem) => inset(16px) | inset(1rem)\ninset(1vw 2vh) => inset(9px 14px) | inset(1vw 2vh)\ninset(1in) => inset(96px) | inset(1in)\ninset(1pt) => inset(1.33333px) | inset(1pt)\ninset(1cm) => inset(37.7953px) | inset(1cm)\ncircle(calc(10px + 50%)) => circle(calc(50% + 10px)) | circle(calc(50% + 10px))\ncircle(calc(100% - 10px) at calc(50% + 1px) calc(1em)) => circle(calc(100% - 10px) at calc(50% + 1px) 10px) | circle(calc(100% - 10px) at calc(50% + 1px) calc(1em))\npolygon(calc(1px + 1px) calc(2px * 2), 1px 1px) => polygon(2px 4px, 1px 1px) | polygon(calc(2px) calc(4px), 1px 1px)\ncircle(min(10px, 20px)) => circle(10px) | circle(calc(10px))\ninset(max(1px, 1em)) => inset(10px) | inset(max(1px, 1em))\ncircle(1ex) => circle(4.58984px) | circle(1ex)\ncircle(1ch) => circle(5px) | circle(1ch)\ninset(1Q) => inset(0.944882px) | inset(1q)\ninset(1mm) => inset(3.77953px) | inset(1mm)\ninset(1pc) => inset(16px) | inset(1pc)\ninset(1e1px) => inset(10px) | inset(10px)\ninset(0.5px) => inset(0.5px) | inset(0.5px)\ninset(.5px) => inset(0.5px) | inset(0.5px)\ninset(+5px) => inset(5px) | inset(5px)\ninset(10.0px) => inset(10px) | inset(10px)\ninset(1.50px) => inset(1.5px) | inset(1.5px)""#,
            ),
            (
                r#"E(() => (() => { const c = document.getElementById('c'); const r = []; for (const v of ['circle(50%)', 'inset(10px)', 'polygon(0 0, 10px 10px)', 'path("M0 0 L10 0")']) { const p = document.createElement('div'); p.style.clipPath = v; c.appendChild(p); const e = document.createElement('span'); p.appendChild(e); r.push(v + ' child=' + getComputedStyle(e).clipPath); p.remove(); } return r.join('\n'); })())"#,
                r#""circle(50%) child=none\ninset(10px) child=none\npolygon(0 0, 10px 10px) child=none\npath(\"M0 0 L10 0\") child=none""#,
            ),
            (
                r"E(() => (() => { const c = document.getElementById('c'); const p = document.createElement('div'); p.style.clipPath = 'circle(50%)'; const e = document.createElement('span'); e.style.clipPath = 'inherit'; p.appendChild(e); c.appendChild(p); const r = getComputedStyle(e).clipPath; p.remove(); return r; })())",
                r#""circle(50%)""#,
            ),
            (
                r"E(() => (() => { const c = document.getElementById('c'); const e = document.createElement('div'); c.appendChild(e); const r = []; for (const v of ['inset(10px)', 'circle(50%)']) { e.style.clipPath = v; r.push(getComputedStyle(e).getPropertyValue('clip-path'), e.style.clipPath); } e.remove(); return r.join('|'); })())",
                r#""inset(10px)|inset(10px)|circle(50%)|circle(50%)""#,
            ),
            (
                r"E(() => CSS.supports('clip-path: circle(50%)') + ' ' + CSS.supports('(clip-path: polygon(1px 1px))') + ' ' + CSS.supports('clip-path', 'polygon(1px 1px)') + ' ' + CSS.supports('clip-path', 'polygon(1px 1px, 2px 2px)'))",
                r#""true true true true""#,
            ),
        ],
    },
    Case {
        name: "cp-hit",
        html: r"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0}
.b{position:absolute;width:100px;height:100px;background:#888}
.u{position:absolute;left:0;top:0;width:400px;height:400px;background:#ccc}
#a{left:10px;top:10px;clip-path:circle(50%)}
#b{left:130px;top:10px;clip-path:inset(25%)}
#c{left:250px;top:10px;clip-path:polygon(0 0,100% 0,0 100%)}
#d{left:10px;top:130px;clip-path:inset(0 round 50%)}
#e{left:130px;top:130px;clip-path:circle(30px at 0 0);}
#f{left:250px;top:130px;clip-path:inset(50%)}
#g{left:10px;top:250px;clip-path:path('M0 0 h50 v50 h-50 z')}
#h{left:130px;top:250px;clip-path:margin-box;border-radius:50px}
#i{left:250px;top:250px;clip-path:circle(50%);pointer-events:none}
#j{left:370px;top:10px;background:none;clip-path:circle(50%)}
#k{left:50px;top:50px;width:60px;height:60px;background:#444;clip-path:none}
</style></head><body><div class=u id=u></div><div class=b id=a><div class=b id=k></div></div><div class=b id=b></div><div class=b id=c></div><div class=b id=d></div><div class=b id=e></div><div class=b id=f></div><div class=b id=g></div><div class=b id=h></div><div class=b id=i></div><div class=b id=j></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function P(x, y) { const e = document.elementFromPoint(x, y); return e ? e.id : 'null'; }
function PS(x, y) { return document.elementsFromPoint(x, y).map((e) => e.id || e.localName).join(); }
</script></body></html>",
        queries: &[
            (
                r"E(() => [P(12,12),P(57,57),P(105,12),P(12,105),P(108,108),P(35,35),P(85,85),P(60,12),P(30,60)].join())",
                r#""u,a,u,u,u,a,k,a,a""#,
            ),
            (
                r"E(() => [P(132,12),P(177,57),P(225,12),P(132,105),P(228,108),P(155,35),P(205,85),P(180,12),P(150,60)].join())",
                r#""u,b,u,u,u,b,u,u,u""#,
            ),
            (
                r"E(() => [P(252,12),P(297,57),P(345,12),P(252,105),P(348,108),P(275,35),P(325,85),P(300,12),P(270,60)].join())",
                r#""c,c,c,c,u,c,u,c,c""#,
            ),
            (
                r"E(() => [P(12,132),P(57,177),P(105,132),P(12,225),P(108,228),P(35,155),P(85,205),P(60,132),P(30,180)].join())",
                r#""u,d,u,u,u,d,d,d,d""#,
            ),
            (
                r"E(() => [P(132,132),P(177,177),P(225,132),P(132,225),P(228,228),P(155,155),P(205,205),P(180,132),P(150,180)].join())",
                r#""e,u,u,u,u,u,u,u,u""#,
            ),
            (
                r"E(() => [P(252,132),P(297,177),P(345,132),P(252,225),P(348,228),P(275,155),P(325,205),P(300,132),P(270,180)].join())",
                r#""u,u,u,u,u,u,u,u,u""#,
            ),
            (
                r"E(() => [P(12,252),P(57,297),P(105,252),P(12,345),P(108,348),P(35,275),P(85,325),P(60,252),P(30,300)].join())",
                r#""g,g,u,u,u,g,u,u,u""#,
            ),
            (
                r"E(() => [P(132,252),P(177,297),P(225,252),P(132,345),P(228,348),P(155,275),P(205,325),P(180,252),P(150,300)].join())",
                r#""u,h,u,u,u,h,h,h,h""#,
            ),
            (
                r"E(() => [P(252,252),P(297,297),P(345,252),P(252,345),P(348,348),P(275,275),P(325,325),P(300,252),P(270,300)].join())",
                r#""u,u,u,u,u,u,u,u,u""#,
            ),
            (
                r"E(() => [P(372,12),P(417,57),P(465,12),P(372,105),P(468,108),P(395,35),P(445,85),P(420,12),P(390,60)].join())",
                r#""u,j,,u,,j,j,j,j""#,
            ),
            (
                r"E(() => [PS(60,60), PS(15,15), PS(95,95), PS(140,50)])",
                r#""[\"k,a,u,html\",\"u,html\",\"k,a,u,html\",\"u,html\"]""#,
            ),
            (
                r"E(() => [document.getElementById('a').getBoundingClientRect().width, document.getElementById('a').offsetWidth, document.getElementById('e').getClientRects().length])",
                r#""[100,100,1]""#,
            ),
        ],
    },
    Case {
        name: "cp-stack",
        html: r#"<!DOCTYPE html><html><head><title>t</title><style>
body{margin:0}
.p{position:absolute;left:10px;width:200px;height:100px;background:#ccc}
.lo{position:relative;z-index:-1;width:50px;height:50px;background:#f00}
.ab{position:absolute;left:0;top:0;width:30px;height:30px;background:#00f}
#p1{top:10px;clip-path:inset(0)}
#p2{top:130px}
#p3{top:250px;clip-path:inset(0)}
#p4{top:370px;clip-path:inset(0)}
#p5{top:490px;clip-path:inset(0);position:static;margin-left:10px}
</style></head><body>
<div class=p id=p1><div class=lo id=lo1></div></div>
<div class=p id=p2><div class=lo id=lo2></div></div>
<div class=p id=p3><div class=ab id=ab3></div></div>
<div class=p id=p4 style="overflow:visible"><div style="position:fixed;left:50px;top:300px;width:20px;height:20px;background:#0a0" id=fx4></div></div>
<div class=p id=p5 style="position:static"><div class=ab id=ab5></div></div>
<script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? JSON.stringify(v) : v); } catch (e) { return e.constructor.name + ': ' + e.message; } }
function R(id) { const r = document.getElementById(id).getBoundingClientRect(); return [r.x, r.y, r.width, r.height].join(); }
</script></body></html>"#,
        queries: &[
            (
                r"E(() => [R('lo1'), R('lo2'), R('ab3'), R('fx4'), R('ab5')].join(' | '))",
                r#""10,10,50,50 | 10,130,50,50 | 10,250,30,30 | 50,300,20,20 | 0,0,30,30""#,
            ),
            (
                r"E(() => [document.elementFromPoint(20, 140).id, document.elementFromPoint(300, 140).id])",
                r#""[\"p2\",\"\"]""#,
            ),
        ],
    },
];
