// The cases of `tools/probes/web-geometry.json` and `web-scroll.json` with
// the values that Chromium 148 gave (`just probe --json FILE`, three runs,
// all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::Case;

/// Page 1 of the cases.
pub(crate) const PAGE_1: &str = r"<!DOCTYPE html><html><head><title>t</title><style>#a { margin: 10px 20px; padding: 5px; border: 3px solid; width: 300px; height: 50px } #b { margin-left: 10.5px; width: 33.3%; height: 10.25px; border-left: 1.5px solid } #c { width: 100.4px; height: 20.6px; padding: 0.3px } #d { margin-top: 0.7px; height: 7.33px; width: 50% } #e { border: 2px solid; border-width: 1px 2px 3px 4px; padding: 1px 2px 3px 4px; width: 50.5px; font: 16px/20px sans-serif } #f { height: 33.3px }</style></head><body><div id=a><div id=b></div><div id=c></div></div><div id=d></div><div id=e><div id=f>x</div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 2 of the cases.
pub(crate) const PAGE_2: &str = r"<!DOCTYPE html><html><head><title>t</title><style>#w { width: 200px; font: 16px/20px sans-serif } .p { padding: 2px 4px; border: 1px solid; margin: 0 3px }</style></head><body><div id=w>aaa <span id=s1>bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp</span> zz <span id=s2 class=p>pad</span> <span id=s3></span><b id=b1>bold <i id=i1>it</i></b><br id=br><span id=ib style='display:inline-block;width:50px;height:30px'></span><img id=im style='width:20px;height:10px'> <a id=lnk href=#>link text that wraps around the line</a><span id=s4 style='position:relative;left:3px;top:2px'>rel</span></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 3 of the cases.
pub(crate) const PAGE_3: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px; font: 16px/20px sans-serif } #rel { position: relative; left: 10px; top: 5px; width: 200px; height: 100px; border: 2px solid; padding: 3px } #abs { position: absolute; left: 20px; top: 30px; width: 50px; height: 40px; border: 1px solid } #inner { width: 10px; height: 10px; margin: 2px } #abs2 { position: absolute; right: 10%; bottom: 7.5px; width: 25.5px; height: 10px } #fix { position: fixed; right: 0; top: 100px; width: 30px; height: 30px } #fp { margin: 0; height: 5px } #st { position: sticky; top: 0; height: 20px } #rel2 { position: relative; margin: 4px; padding: 6px; border: 1px solid } #stat { padding: 2px } #deep { height: 5px }</style></head><body><div id=rel><div id=abs><div id=inner></div></div><div id=abs2></div></div><div id=fix><p id=fp></p></div><div id=st></div><div id=plain><span id=sp>x</span></div><div id=rel2><div id=stat><div id=deep></div></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 4 of the cases.
pub(crate) const PAGE_4: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } table { border-spacing: 2px; border: 1px solid } td, th { padding: 3px; border: 1px solid }</style></head><body><table id=t><caption id=cap>cap</caption><tbody id=tb><tr id=tr1><td id=td1>a</td><td id=td2><div id=d>in cell</div></td></tr><tr id=tr2><th id=th1 colspan=2>h</th></tr></tbody></table><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 5 of the cases.
pub(crate) const PAGE_5: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } #t1 { width: 100px; height: 50px; margin: 20px; transform: translate(10px, 5px) } #c { width: 10px; height: 10px; margin-left: 5px } #t2 { width: 100px; height: 100px; margin: 30px; transform: rotate(45deg) } #t3 { width: 80px; height: 40px; transform: scale(0.5); transform-origin: 0 0 } #t4 { display: inline-block; transform: translateX(3.3px) } #t5 { position: relative; width: 60px; height: 60px; transform: scale(2) } #t6 { position: absolute; left: 10px; top: 10px; width: 10px; height: 10px }</style></head><body><div id=t1><div id=c></div></div><div id=t2></div><div id=t3><span id=s>text</span></div><span id=t4>x</span><div id=t5><div id=t6></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 6 of the cases.
pub(crate) const PAGE_6: &str = r"<!DOCTYPE html><html><head><title>t</title><style>#sc { width: 200px; height: 100px; overflow: auto; border: 5px solid; padding: 10px; margin: 10px } #content { width: 500px; height: 400px } #item { margin-top: 150px; height: 10px; width: 10px } #h { width: 150px; height: 60px; overflow: hidden } #hc { height: 200px; width: 300px } #v { width: 100px; height: 50px } #vc { width: 300px; height: 100px } #e { width: 50px; height: 50px; overflow: scroll; border: 1px solid }</style></head><body><div id=sc><div id=content><div id=item></div></div></div><div id=h><div id=hc></div></div><div id=v><div id=vc></div></div><div id=e></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 7 of the cases.
pub(crate) const PAGE_7: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } svg { margin: 5px; border: 1px solid; padding: 2px }</style></head><body><svg id=s width=200 height=100><g id=g transform='translate(10,20)'><rect id=r x=5 y=5 width=50 height=30 /><circle id=ci cx=100 cy=50 r=10 /></g><text id=tx x=10 y=90>hello</text><foreignObject id=fo x=150 y=0 width=40 height=40><div id=fd>f</div></foreignObject></svg><div id=after>a</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 8 of the cases.
pub(crate) const PAGE_8: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px; font: 16px/20px sans-serif } #tall { height: 2000px; width: 1000px }</style></head><body><div id=d>text</div><div id=tall></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 9 of the cases.
pub(crate) const PAGE_9: &str = r"<html><head><title>t</title><style>body { margin: 8px; font: 16px/20px sans-serif } #tall { height: 2000px; width: 1000px }</style></head><body><div id=d>text</div><div id=tall></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 10 of the cases.
pub(crate) const PAGE_10: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px; font: 16px/20px sans-serif }</style></head><body><div id=d>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 11 of the cases.
pub(crate) const PAGE_11: &str = r"<html><head><title>t</title><style>body { margin: 8px; font: 16px/20px sans-serif }</style></head><body><div id=d>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 12 of the cases.
pub(crate) const PAGE_12: &str = r"<!DOCTYPE html><html><head><title>t</title><style>html { border: 3px solid; padding: 4px; margin: 5px } body { margin: 8px; border: 2px solid; padding: 1px; font: 16px/20px sans-serif }</style></head><body><div id=d>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 13 of the cases.
pub(crate) const PAGE_13: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif }</style></head><body><div id=n style='display:none'><p id=np>x</p></div><div id=dc style='display:contents'><p id=dcp>y</p></div><template id=tp><p>t</p></template><select id=sel><option id=op>o</option></select><map><area id=ar></map><span id=empty></span><div id=vh style='visibility:hidden;height:5px'></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 14 of the cases.
pub(crate) const PAGE_14: &str = r"<!DOCTYPE html><html><head><title>t</title><style>.r { position: absolute }</style></head><body><div id=r00 class=r style='left:10.25px;top:50px;width:20.25px;height:5.25px'></div><div id=r10 class=r style='left:10.5px;top:70px;width:20.5px;height:5.5px'></div><div id=r20 class=r style='left:10.75px;top:90px;width:20.75px;height:5.75px'></div><div id=r30 class=r style='left:10.4px;top:110px;width:20.4px;height:5.4px'></div><div id=r40 class=r style='left:10.6px;top:130px;width:20.6px;height:5.6px'></div><div id=r50 class=r style='left:10.49px;top:150px;width:20.51px;height:5.49px'></div><div id=r60 class=r style='left:0.3px;top:170px;width:0.3px;height:0.3px'></div><div id=r70 class=r style='left:0.5px;top:190px;width:0.5px;height:0.5px'></div><div id=r80 class=r style='left:7.7px;top:210px;width:0.7px;height:0.7px'></div><div id=bw style='position:absolute;left:300px;top:10px;width:10.5px;height:10px;border:0.5px solid;padding:0.25px'></div><div id=bw2 style='position:absolute;left:300.5px;top:40px;width:10px;height:10px;border:1.6px solid;padding:0.5px'></div><div id=sc style='position:absolute;left:400px;top:10px;width:50.5px;height:50.5px;overflow:auto'><div style='width:100.5px;height:100.4px'></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 15 of the cases.
pub(crate) const PAGE_15: &str = r"<!DOCTYPE html><html><head><title>t</title><style></style></head><body><div id=d>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 16 of the cases.
pub(crate) const PAGE_16: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; font: 16px/20px sans-serif } #a { position: absolute; left: 0; top: 0; width: 100px; height: 100px } #b { position: absolute; left: 50px; top: 50px; width: 100px; height: 100px } #c { position: absolute; left: 200px; top: 0; width: 50px; height: 50px; pointer-events: none } #d { margin-top: 300px; height: 2000px } #t { position: absolute; left: 300px; top: 0 } #sv { position: absolute; left: 400px; top: 0 } #hid { position: absolute; left: 500px; top: 0; width: 50px; height: 50px; visibility: hidden } #sc { position: absolute; left: 600px; top: 0; width: 100px; height: 100px; overflow: auto } #sci { height: 500px; margin-top: 200px }</style></head><body><div id=a><span id=as>a</span></div><div id=b></div><div id=c></div><div id=d><p id=p>text</p></div><span id=t>text here</span><svg id=sv width=50 height=50><rect id=rr x=10 y=10 width=20 height=20 /></svg><div id=hid></div><div id=sc><div id=sci></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 17 of the cases.
pub(crate) const PAGE_17: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } #fx { display: flex; width: 300px; gap: 7px; padding: 3px; align-items: center; height: 40px } #f1 { flex: 1 } #f2 { flex: 2; margin: 1.5px } #f3 { width: 20px; height: 20px } #gr { display: grid; grid-template-columns: 50px 1fr 2fr; width: 301px; column-gap: 5px } #fl { float: left; width: 60px; height: 30px; margin-right: 4px } #fp { margin: 0; width: 200px } #ul { margin: 4px 0; padding-left: 30px }</style></head><body><div id=fx><div id=f1>a</div><div id=f2>bb</div><div id=f3></div></div><div id=gr><div id=g1>x</div><div id=g2>y</div><div id=g3>z</div></div><div id=fl>left</div><p id=fp>text next to the float that wraps around it more than once here</p><div id=cl style='clear:both'></div><ul id=ul><li id=li1>one</li><li id=li2>two</li></ul><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 18 of the cases.
pub(crate) const PAGE_18: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px; font: 16px/20px sans-serif } #abs { position: absolute; left: 10px; top: 300px; width: 20px; height: 20px } #fix { position: fixed; left: 5px; bottom: 5px; width: 30px; height: 30px } #st { position: sticky; top: 10px; height: 15px; margin-top: 50px } #sc { position: relative; overflow: auto; width: 100px; height: 100px; margin-top: 20px } #scabs { position: absolute; left: 5px; top: 150px; width: 10px; height: 10px } #scin { height: 300px } #tall { height: 3000px }</style></head><body><div id=abs></div><div id=fix><span id=fs>f</span></div><div id=st></div><div id=sc><div id=scabs></div><div id=scin></div></div><div id=tall></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 19 of the cases.
pub(crate) const PAGE_19: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif } #w { width: 300px }</style></head><body><div id=w><span id=s>before<div id=d>block</div>after</span></div><div id=ib1>x <span id=ibs style='display:inline-block;width:40px'>a b c d e f g</span> y</div><p id=pr>a <span id=rot style='display:inline-block;transform:rotate(90deg);width:30px;height:10px'></span> b <span id=sk style='display:inline-block;transform:skewX(20deg) scale(1.5);width:20px;height:10px'></span></p><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 20 of the cases.
pub(crate) const PAGE_20: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { font: 16px/20px sans-serif }</style></head><body><details id=dt><summary id=sm>s</summary><p id=dp>hidden</p></details><details open id=dt2><summary id=sm2>t</summary><p id=dp2>shown</p></details><img id=im style='width:30px;height:20px;padding:2px 3px;border:1px solid;margin:4px'><input id=in style='width:100px'><button id=bt>go</button><textarea id=ta style='width:100px;height:40px'></textarea><select id=se><option>a</option></select><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 21 of the cases.
pub(crate) const PAGE_21: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 10px } #o { width: 100px; height: 100px; transform: rotate(30deg) } #i { width: 50px; height: 50px; margin: 10px; transform: scale(1.5) translate(5px, 5px) } #c { width: 10px; height: 10px } #sc { width: 100px; height: 100px; overflow: auto; margin-top: 60px } #sct { height: 300px; transform: translateY(10px) } #sctc { height: 10px; width: 10px; margin-top: 100px }</style></head><body><div id=o><div id=i><div id=c></div></div></div><div id=sc><div id=sct><div id=sctc></div></div></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 22 of the cases.
pub(crate) const PAGE_22: &str = r"<!DOCTYPE html><html><head><title>t</title><style></style></head><body><div id=d style='width:10px;height:20px;margin:3px'></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 23 of the cases.
pub(crate) const PAGE_23: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #o { margin: 20px; padding: 10px; border: 5px solid } #t { width: 100px; height: 50px; border: 3px solid; padding: 4px; margin-left: 7px } #sc { width: 100px; height: 100px; overflow: auto; border: 2px solid } #sct { height: 300px; margin-top: 10px; border: 1px solid } #rel { position: relative; left: 5px; margin: 10px; padding: 3px } #rt { width: 40px; height: 40px; padding: 2px } #tall { height: 2000px }</style></head><body><div id=o><div id=t></div></div><div id=sc><div id=sct></div></div><div id=rel><div id=rt></div></div><div id=tall></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 24 of the cases.
pub(crate) const PAGE_24: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px } #tall { height: 1500px } #tall2 { height: 1000px } #sc { height: 100px; overflow: auto } #sci { height: 400px }</style></head><body><div id=tall></div><input id=i><div id=tall2></div><button id=b2>b</button><div id=sc><div id=sci></div><input id=i2></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 25 of the cases.
pub(crate) const PAGE_25: &str = r"<!DOCTYPE html><html><head><title>t</title><style></style></head><body><img id=a src='data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw=='><img id=b style='display:none' width=40 height=30><div id=c></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function R(r) { return r.x + ' ' + r.y + ' ' + r.width + ' ' + r.height; }
function Q(s) { return [...document.querySelectorAll(s)]; }
function B(s) { return Q(s).map(e => N(e) + ' ' + R(e.getBoundingClientRect())); }
function L(s) { return Q(s).map(e => N(e) + ' [' + [...e.getClientRects()].map(R).join(' | ') + ']'); }
function O(s) { return Q(s).map(e => N(e) + ' ' + N(e.offsetParent) + ' ' + e.offsetLeft + ' ' + e.offsetTop + ' ' + e.offsetWidth + ' ' + e.offsetHeight); }
function C(s) { return Q(s).map(e => N(e) + ' ' + e.clientLeft + ' ' + e.clientTop + ' ' + e.clientWidth + ' ' + e.clientHeight); }
function S(s) { return Q(s).map(e => N(e) + ' ' + e.scrollLeft + ' ' + e.scrollTop + ' ' + e.scrollWidth + ' ' + e.scrollHeight); }
</script></body></html>";

/// Page 26 of the cases.
pub(crate) const PAGE_26: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 0; height: 3000px; width: 2000px; font: 16px/20px sans-serif } .sc { width: 100px; height: 100px; overflow: auto; border: 2px solid; padding: 3px } #in { width: 300px; height: 500px } #target { position: absolute; top: 1500px; left: 1200px; width: 50px; height: 50px } #it { margin-top: 300px; margin-left: 150px; height: 20px; width: 20px } #sc2 { position: absolute; top: 1000px; left: 100px } #in2 { height: 400px; padding-top: 250px } #t2 { height: 10px; width: 10px } #hid { overflow: hidden; width: 50px; height: 50px } #hidin { width: 200px; height: 200px } #big { position: absolute; top: 2000px; left: 0; width: 100px; height: 900px }</style></head><body><div id=sc class=sc><div id=in><div id=it></div></div></div><div id=target></div><div id=sc2 class=sc><div id=in2><div id=t2></div></div></div><div id=hid class=sc><div id=hidin></div></div><div id=big></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function P() { return [scrollX, scrollY]; }
function Z() { scrollTo(0, 0); for (const e of document.querySelectorAll('.sc')) { e.scrollTop = 0; e.scrollLeft = 0; } }
function A(f) { return E(() => { Z(); return f(); }); }
var log = [];
function W(e) { log.push(N(e.target) + '@' + N(e.currentTarget) + ':' + e.bubbles + ':' + e.cancelable + ':' + scrollX + ',' + scrollY); }
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
</script></body></html>";

/// Page 27 of the cases.
pub(crate) const PAGE_27: &str = r"<html><head><title>t</title><style>body { margin: 0; height: 3000px; width: 2000px; font: 16px/20px sans-serif } .sc { width: 100px; height: 100px; overflow: auto; border: 2px solid; padding: 3px } #in { width: 300px; height: 500px } #target { position: absolute; top: 1500px; left: 1200px; width: 50px; height: 50px } #it { margin-top: 300px; margin-left: 150px; height: 20px; width: 20px } #sc2 { position: absolute; top: 1000px; left: 100px } #in2 { height: 400px; padding-top: 250px } #t2 { height: 10px; width: 10px } #hid { overflow: hidden; width: 50px; height: 50px } #hidin { width: 200px; height: 200px } #big { position: absolute; top: 2000px; left: 0; width: 100px; height: 900px }</style></head><body><div id=sc class=sc><div id=in><div id=it></div></div></div><div id=target></div><div id=sc2 class=sc><div id=in2><div id=t2></div></div></div><div id=hid class=sc><div id=hidin></div></div><div id=big></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function P() { return [scrollX, scrollY]; }
function Z() { scrollTo(0, 0); for (const e of document.querySelectorAll('.sc')) { e.scrollTop = 0; e.scrollLeft = 0; } }
function A(f) { return E(() => { Z(); return f(); }); }
var log = [];
function W(e) { log.push(N(e.target) + '@' + N(e.currentTarget) + ':' + e.bubbles + ':' + e.cancelable + ':' + scrollX + ',' + scrollY); }
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
</script></body></html>";

/// Page 28 of the cases.
pub(crate) const PAGE_28: &str = r"<!DOCTYPE html><html><head><title>t</title><style>body { margin: 8px }</style></head><body><div id=d>x</div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function P() { return [scrollX, scrollY]; }
function Z() { scrollTo(0, 0); for (const e of document.querySelectorAll('.sc')) { e.scrollTop = 0; e.scrollLeft = 0; } }
function A(f) { return E(() => { Z(); return f(); }); }
var log = [];
function W(e) { log.push(N(e.target) + '@' + N(e.currentTarget) + ':' + e.bubbles + ':' + e.cancelable + ':' + scrollX + ',' + scrollY); }
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
</script></body></html>";

/// Page 29 of the cases.
pub(crate) const PAGE_29: &str = r"<!DOCTYPE html><html><head><title>t</title><style>#h { height: 1e9px; width: 1e9px }</style></head><body><div id=h></div><div id=end></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null && !Array.isArray(v) ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function N(e) { return e === null ? 'null' : e === undefined ? 'undefined' : e === window ? 'window' : e === document ? 'document' : e.id ? '#' + e.id : e.nodeName; }
function P() { return [scrollX, scrollY]; }
function Z() { scrollTo(0, 0); for (const e of document.querySelectorAll('.sc')) { e.scrollTop = 0; e.scrollLeft = 0; } }
function A(f) { return E(() => { Z(); return f(); }); }
var log = [];
function W(e) { log.push(N(e.target) + '@' + N(e.currentTarget) + ':' + e.bubbles + ':' + e.cancelable + ':' + scrollX + ',' + scrollY); }
function after(ms) { return new Promise(r => setTimeout(r, ms)); }
</script></body></html>";

/// The cases of `web-geometry.json`.
pub(crate) const GEOMETRY: &[Case] = &[
    Case {
        name: "block",
        html: PAGE_1,
        queries: &[
            (
                r#"E(() => B("div"))"#,
                r##"["#a 28 10 316 66", "#b 46.5 18 100.890625 10.25", "#c 36 28.25 100.984375 21.1875", "#d 8 86 392 7.328125", "#e 8 93.328125 62.5 41.296875", "#f 16 95.328125 50.5 33.296875"]"##,
            ),
            (
                r#"E(() => L("div"))"#,
                r##"["#a [28 10 316 66]", "#b [46.5 18 100.890625 10.25]", "#c [36 28.25 100.984375 21.1875]", "#d [8 86 392 7.328125]", "#e [8 93.328125 62.5 41.296875]", "#f [16 95.328125 50.5 33.296875]"]"##,
            ),
            (
                r#"E(() => O("div"))"#,
                r##"["#a BODY 28 10 316 66", "#b BODY 47 18 101 10", "#c BODY 36 28 101 21", "#d BODY 8 86 392 7", "#e BODY 8 93 63 41", "#f BODY 16 95 51 33"]"##,
            ),
            (
                r#"E(() => C("div"))"#,
                r##"["#a 3 3 310 60", "#b 1 0 100 10", "#c 0 0 101 21", "#d 0 0 392 7", "#e 4 1 57 37", "#f 0 0 51 33"]"##,
            ),
            (
                r#"E(() => S("div"))"#,
                r##"["#a 0 0 310 60", "#b 0 0 100 10", "#c 0 0 101 21", "#d 0 0 392 7", "#e 0 0 57 37", "#f 0 0 51 33"]"##,
            ),
        ],
    },
    Case {
        name: "inline",
        html: PAGE_2,
        queries: &[
            (
                r##"E(() => B("#w, #w *"))"##,
                r##"["#w 8 8 200 115", "#s1 8 9 194.515625 37", "#s2 161.28125 26 36.703125 23", "#s3 200.984375 29 0 17", "#b1 8 49 48 17", "#i1 46.21875 49 9.78125 17", "#br 56 49 0 17", "#ib 8 68 50 30", "#im 58 88 20 10", "#lnk 8 84 159.828125 37", "#s4 163.984375 106 17.78125 17"]"##,
            ),
            (
                r##"E(() => L("#w, #w *"))"##,
                r##"["#w [8 8 200 115]", "#s1 [39.140625 9 163.375 17 | 8 29 125.390625 17]", "#s2 [161.28125 26 36.703125 23]", "#s3 [200.984375 29 0 17]", "#b1 [8 49 38.21875 17 | 46.21875 49 9.78125 17]", "#i1 [46.21875 49 9.78125 17]", "#br [56 49 0 17]", "#ib [8 68 50 30]", "#im [58 88 20 10]", "#lnk [82.453125 84 85.375 17 | 8 104 152.984375 17]", "#s4 [163.984375 106 17.78125 17]"]"##,
            ),
            (
                r##"E(() => O("#w, #w *"))"##,
                r##"["#w BODY 8 8 200 115", "#s1 BODY 39 9 195 37", "#s2 BODY 161 26 37 23", "#s3 BODY 201 29 0 17", "#b1 BODY 8 49 48 17", "#i1 BODY 46 49 10 17", "#br BODY 0 0 0 0", "#ib BODY 8 68 50 30", "#im BODY 58 88 20 10", "#lnk BODY 82 84 160 37", "#s4 BODY 164 106 18 17"]"##,
            ),
            (
                r##"E(() => C("#w, #w *"))"##,
                r##"["#w 0 0 200 115", "#s1 0 0 0 0", "#s2 0 0 0 0", "#s3 0 0 0 0", "#b1 0 0 0 0", "#i1 0 0 0 0", "#br 0 0 0 0", "#ib 0 0 50 30", "#im 0 0 20 10", "#lnk 0 0 0 0", "#s4 0 0 0 0"]"##,
            ),
            (
                r##"E(() => S("#w, #w *"))"##,
                r##"["#w 0 0 200 115", "#s1 0 0 0 0", "#s2 0 0 0 0", "#s3 0 0 0 0", "#b1 0 0 0 0", "#i1 0 0 0 0", "#br 0 0 0 0", "#ib 0 0 50 30", "#im 0 0 20 10", "#lnk 0 0 0 0", "#s4 0 0 0 0"]"##,
            ),
        ],
    },
    Case {
        name: "positioned",
        html: PAGE_3,
        queries: &[
            (
                r#"E(() => B("body *:not(script)"))"#,
                r##"["#rel 18 13 210 110", "#abs 40 45 52 42", "#inner 43 48 10 10", "#abs2 179.90625 103.5 25.5 10", "#fix 770 100 30 30", "#fp 770 100 30 5", "#st 8 118 784 20", "#plain 8 138 784 20", "#sp 8 139 8 17", "#rel2 12 162 776 23", "#stat 19 169 762 9", "#deep 21 171 758 5"]"##,
            ),
            (
                r#"E(() => L("body *:not(script)"))"#,
                r##"["#rel [18 13 210 110]", "#abs [40 45 52 42]", "#inner [43 48 10 10]", "#abs2 [179.90625 103.5 25.5 10]", "#fix [770 100 30 30]", "#fp [770 100 30 5]", "#st [8 118 784 20]", "#plain [8 138 784 20]", "#sp [8 139 8 17]", "#rel2 [12 162 776 23]", "#stat [19 169 762 9]", "#deep [21 171 758 5]"]"##,
            ),
            (
                r#"E(() => O("body *:not(script)"))"#,
                r##"["#rel BODY 18 13 210 110", "#abs #rel 20 30 52 42", "#inner #abs 2 2 10 10", "#abs2 #rel 160 89 26 10", "#fix null 770 100 30 30", "#fp #fix 0 0 30 5", "#st BODY 8 118 784 20", "#plain BODY 8 138 784 20", "#sp BODY 8 139 8 17", "#rel2 BODY 12 162 776 23", "#stat #rel2 6 6 762 9", "#deep #rel2 8 8 758 5"]"##,
            ),
            (
                r#"E(() => C("body *:not(script)"))"#,
                r##"["#rel 2 2 206 106", "#abs 1 1 50 40", "#inner 0 0 10 10", "#abs2 0 0 26 10", "#fix 0 0 30 30", "#fp 0 0 30 5", "#st 0 0 784 20", "#plain 0 0 784 20", "#sp 0 0 0 0", "#rel2 1 1 774 21", "#stat 0 0 762 9", "#deep 0 0 758 5"]"##,
            ),
            (
                r#"E(() => S("body *:not(script)"))"#,
                r##"["#rel 0 0 206 106", "#abs 0 0 50 40", "#inner 0 0 10 10", "#abs2 0 0 26 10", "#fix 0 0 30 30", "#fp 0 0 30 5", "#st 0 0 784 20", "#plain 0 0 784 20", "#sp 0 0 0 0", "#rel2 0 0 774 21", "#stat 0 0 762 9", "#deep 0 0 758 5"]"##,
            ),
        ],
    },
    Case {
        name: "tables",
        html: PAGE_4,
        queries: &[
            (
                r#"E(() => B("table, table *"))"#,
                r##"["#t 8 8 73.8125 84", "#cap 8 8 73.8125 20", "#tb 11 31 67.8125 58", "#tr1 11 31 67.8125 28", "#td1 11 31 16.90625 28", "#td2 29.90625 31 48.90625 28", "#d 33.90625 35 40.90625 20", "#tr2 11 61 67.8125 28", "#th1 11 61 67.8125 28"]"##,
            ),
            (
                r#"E(() => L("table, table *"))"#,
                r##"["#t [8 8 73.8125 84]", "#cap [8 8 73.8125 20]", "#tb [11 31 67.8125 58]", "#tr1 [11 31 67.8125 28]", "#td1 [11 31 16.90625 28]", "#td2 [29.90625 31 48.90625 28]", "#d [33.90625 35 40.90625 20]", "#tr2 [11 61 67.8125 28]", "#th1 [11 61 67.8125 28]"]"##,
            ),
            (
                r#"E(() => O("table, table *"))"#,
                r##"["#t BODY 8 8 74 84", "#cap #t -1 -1 74 20", "#tb #t 2 22 68 58", "#tr1 #t 2 22 68 28", "#td1 #t 2 22 17 28", "#td2 #t 21 22 49 28", "#d #td2 3 3 41 20", "#tr2 #t 2 52 68 28", "#th1 #t 2 52 68 28"]"##,
            ),
            (
                r#"E(() => C("table, table *"))"#,
                r##"["#t 1 1 74 84", "#cap 0 0 74 20", "#tb 0 0 68 58", "#tr1 0 0 68 28", "#td1 1 1 15 26", "#td2 1 1 47 26", "#d 0 0 41 20", "#tr2 0 0 68 28", "#th1 1 1 66 26"]"##,
            ),
            (
                r#"E(() => S("table, table *"))"#,
                r##"["#t 0 0 73 82", "#cap 0 0 74 20", "#tb 0 0 68 58", "#tr1 0 0 68 28", "#td1 0 0 15 26", "#td2 0 0 47 26", "#d 0 0 41 20", "#tr2 0 0 68 28", "#th1 0 0 66 26"]"##,
            ),
        ],
    },
    Case {
        name: "transformed",
        html: PAGE_5,
        queries: &[
            (
                r#"E(() => B("body *:not(script)"))"#,
                r##"["#t1 38 25 100 50", "#c 43 25 10 10", "#t2 17.289321899414062 79.28932189941406 141.42135620117188 141.42135620117188", "#t3 8 230 40 20", "#s 8 230.5 12.8984375 8.5", "#t4 11.300000190734863 270 7.999999046325684 20", "#t5 -22 260 120 120", "#t6 -2 280 20 20"]"##,
            ),
            (
                r#"E(() => L("body *:not(script)"))"#,
                r##"["#t1 [38 25 100 50]", "#c [43 25 10 10]", "#t2 [17.289321899414062 79.28932189941406 141.42135620117188 141.42135620117188]", "#t3 [8 230 40 20]", "#s [8 230.5 12.8984375 8.5]", "#t4 [11.300000190734863 270 7.999999046325684 20]", "#t5 [-22 260 120 120]", "#t6 [-2 280 20 20]"]"##,
            ),
            (
                r#"E(() => O("body *:not(script)"))"#,
                r##"["#t1 BODY 28 20 100 50", "#c #t1 5 0 10 10", "#t2 BODY 38 100 100 100", "#t3 BODY 8 230 80 40", "#s #t3 0 1 26 17", "#t4 BODY 8 270 8 20", "#t5 BODY 8 290 60 60", "#t6 #t5 10 10 10 10"]"##,
            ),
            (
                r#"E(() => C("body *:not(script)"))"#,
                r##"["#t1 0 0 100 50", "#c 0 0 10 10", "#t2 0 0 100 100", "#t3 0 0 80 40", "#s 0 0 0 0", "#t4 0 0 8 20", "#t5 0 0 60 60", "#t6 0 0 10 10"]"##,
            ),
            (
                r#"E(() => S("body *:not(script)"))"#,
                r##"["#t1 0 0 100 50", "#c 0 0 10 10", "#t2 0 0 100 100", "#t3 0 0 80 40", "#s 0 0 0 0", "#t4 0 0 8 20", "#t5 0 0 60 60", "#t6 0 0 10 10"]"##,
            ),
        ],
    },
    Case {
        name: "scrolled",
        html: PAGE_6,
        queries: &[
            (
                r#"E(() => B("body *:not(script)"))"#,
                r##"["#sc 18 10 230 130", "#content 33 175 500 400", "#item 33 175 10 10", "#h 8 150 150 60", "#hc 8 150 300 200", "#v 8 210 100 50", "#vc 8 210 300 100", "#e 8 260 52 52"]"##,
            ),
            (
                r#"E(() => L("body *:not(script)"))"#,
                r##"["#sc [18 10 230 130]", "#content [33 175 500 400]", "#item [33 175 10 10]", "#h [8 150 150 60]", "#hc [8 150 300 200]", "#v [8 210 100 50]", "#vc [8 210 300 100]", "#e [8 260 52 52]"]"##,
            ),
            (
                r#"E(() => O("body *:not(script)"))"#,
                r##"["#sc BODY 18 10 230 130", "#content BODY 33 175 500 400", "#item BODY 33 175 10 10", "#h BODY 8 150 150 60", "#hc BODY 8 150 300 200", "#v BODY 8 210 100 50", "#vc BODY 8 210 300 100", "#e BODY 8 260 52 52"]"##,
            ),
            (
                r#"E(() => C("body *:not(script)"))"#,
                r##"["#sc 5 5 220 120", "#content 0 0 500 400", "#item 0 0 10 10", "#h 0 0 150 60", "#hc 0 0 300 200", "#v 0 0 100 50", "#vc 0 0 300 100", "#e 1 1 50 50"]"##,
            ),
            (
                r#"E(() => S("body *:not(script)"))"#,
                r##"["#sc 0 0 520 570", "#content 0 0 500 400", "#item 0 0 10 10", "#h 0 0 300 200", "#hc 0 0 300 200", "#v 0 0 300 100", "#vc 0 0 300 100", "#e 0 0 50 50"]"##,
            ),
            (
                r"E(() => (sc.scrollTop = 50, sc.scrollLeft = 30, [sc.scrollTop, sc.scrollLeft].concat(B('#item, #content'), O('#item'), S('#sc'))))",
                r##"[50, 30, "#content 3 125 500 400", "#item 3 125 10 10", "#item BODY 33 175 10 10", "#sc 30 50 520 570"]"##,
            ),
            (
                r"E(() => (h.scrollTop = 1000, h.scrollLeft = 1000, [h.scrollTop, h.scrollLeft].concat(B('#hc'))))",
                r##"[140, 150, "#hc -142 10 300 200"]"##,
            ),
            (
                r"E(() => (v.scrollTop = 10, [v.scrollTop, v.scrollLeft]))",
                r"[0, 0]",
            ),
            (
                r"E(() => (sc.scrollTop = 10.5, sc.scrollLeft = 0.25, [sc.scrollTop, sc.scrollLeft]))",
                r"[11, 0]",
            ),
            (
                r"E(() => (sc.scrollTop = -10, sc.scrollLeft = NaN, [sc.scrollTop, sc.scrollLeft]))",
                r"[0, 0]",
            ),
        ],
    },
    Case {
        name: "svg",
        html: PAGE_7,
        queries: &[
            (
                r"E(() => B('svg, svg *'))",
                r##"["#s 13 13 206 106", "#g 31 41 105 55", "#r 31 41 50 30", "#ci 116 76 20 20", "#tx 26 92 33.90625 17", "#fo 166 16 40 40", "#fd 166 16 40 20"]"##,
            ),
            (
                r"E(() => L('svg, svg *'))",
                r##"["#s [13 13 206 106]", "#g [31 41 105 55]", "#r [31 41 50 30]", "#ci [116 76 20 20]", "#tx [26 92 33.90625 17]", "#fo [166 16 40 40]", "#fd [166 16 40 20]"]"##,
            ),
            (
                r"E(() => C('svg, svg *'))",
                r##"["#s 1 1 204 104", "#g 0 0 0 0", "#r 0 0 0 0", "#ci 0 0 0 0", "#tx 0 0 44 93", "#fo 0 0 40 40", "#fd 0 0 40 20"]"##,
            ),
            (
                r"E(() => S('svg, svg *'))",
                r##"["#s 0 0 204 104", "#g 0 0 0 0", "#r 0 0 0 0", "#ci 0 0 0 0", "#tx 0 0 44 93", "#fo 0 0 40 40", "#fd 0 0 40 20"]"##,
            ),
            (
                r"E(() => [typeof r.offsetTop, typeof s.offsetParent, 'offsetTop' in s, 'clientTop' in r, fd.offsetParent && fd.offsetParent.id, fd.offsetLeft, fd.offsetTop])",
                r#"["undefined", "undefined", false, true, "fo", 0, 0]"#,
            ),
            (
                r"E(() => O('#fd, #after'))",
                r##"["#fd #fo 0 0 40 20", "#after BODY 8 129 784 20"]"##,
            ),
        ],
    },
    Case {
        name: "root-standards",
        html: PAGE_8,
        queries: &[
            (
                r"E(() => [document.compatMode, N(document.scrollingElement)])",
                r#"["CSS1Compat", "HTML"]"#,
            ),
            (
                r"E(() => B('html, body'))",
                r#"["HTML 0 0 800 2036", "BODY 8 8 784 2020"]"#,
            ),
            (
                r"E(() => O('html, body'))",
                r#"["HTML null 0 0 800 2036", "BODY null 0 0 784 2020"]"#,
            ),
            (
                r"E(() => C('html, body'))",
                r#"["HTML 0 0 800 600", "BODY 0 0 784 2020"]"#,
            ),
            (
                r"E(() => S('html, body'))",
                r#"["HTML 0 0 1008 2036", "BODY 0 0 1000 2020"]"#,
            ),
            (
                r"E(() => L('html, body'))",
                r#"["HTML [0 0 800 2036]", "BODY [8 8 784 2020]"]"#,
            ),
            (
                r"E(() => (scrollTo(30, 100), [scrollX, scrollY].concat(S('html, body'), B('html, body, #d'), O('#d'))))",
                r##"[30, 100, "HTML 30 100 1008 2036", "BODY 0 0 1000 2020", "HTML -30 -100 800 2036", "BODY -22 -92 784 2020", "#d -22 -92 784 20", "#d BODY 8 8 784 20"]"##,
            ),
            (
                r"E(() => (document.documentElement.scrollTop = 200, [scrollY, document.documentElement.scrollTop, document.body.scrollTop]))",
                r"[200, 200, 0]",
            ),
            (
                r"E(() => (document.body.scrollTop = 300, [scrollY, document.documentElement.scrollTop, document.body.scrollTop]))",
                r"[200, 200, 0]",
            ),
            (
                r"E(() => (document.documentElement.scrollLeft = 5, document.body.scrollLeft = 7, [scrollX, document.documentElement.scrollLeft, document.body.scrollLeft]))",
                r"[5, 5, 0]",
            ),
        ],
    },
    Case {
        name: "root-quirks",
        html: PAGE_9,
        queries: &[
            (
                r"E(() => [document.compatMode, N(document.scrollingElement)])",
                r#"["BackCompat", "BODY"]"#,
            ),
            (
                r"E(() => B('html, body'))",
                r#"["HTML 0 0 800 2036", "BODY 8 8 784 2020"]"#,
            ),
            (
                r"E(() => O('html, body'))",
                r#"["HTML null 0 0 800 2036", "BODY null 0 0 784 2020"]"#,
            ),
            (
                r"E(() => C('html, body'))",
                r#"["HTML 0 0 800 2036", "BODY 0 0 800 600"]"#,
            ),
            (
                r"E(() => S('html, body'))",
                r#"["HTML 0 0 1008 2036", "BODY 0 0 1008 2036"]"#,
            ),
            (
                r"E(() => L('html, body'))",
                r#"["HTML [0 0 800 2036]", "BODY [8 8 784 2020]"]"#,
            ),
            (
                r"E(() => (scrollTo(30, 100), [scrollX, scrollY].concat(S('html, body'), B('html, body, #d'), O('#d'))))",
                r##"[30, 100, "HTML 0 0 1008 2036", "BODY 30 100 1008 2036", "HTML -30 -100 800 2036", "BODY -22 -92 784 2020", "#d -22 -92 784 20", "#d BODY 8 8 784 20"]"##,
            ),
            (
                r"E(() => (document.documentElement.scrollTop = 200, [scrollY, document.documentElement.scrollTop, document.body.scrollTop]))",
                r"[100, 0, 100]",
            ),
            (
                r"E(() => (document.body.scrollTop = 300, [scrollY, document.documentElement.scrollTop, document.body.scrollTop]))",
                r"[300, 0, 300]",
            ),
            (
                r"E(() => (document.documentElement.scrollLeft = 5, document.body.scrollLeft = 7, [scrollX, document.documentElement.scrollLeft, document.body.scrollLeft]))",
                r"[7, 0, 7]",
            ),
        ],
    },
    Case {
        name: "root-short",
        html: PAGE_10,
        queries: &[
            (
                r"E(() => C('html, body'))",
                r#"["HTML 0 0 800 600", "BODY 0 0 784 20"]"#,
            ),
            (
                r"E(() => S('html, body'))",
                r#"["HTML 0 0 800 600", "BODY 0 0 784 20"]"#,
            ),
            (
                r"E(() => O('html, body'))",
                r#"["HTML null 0 0 800 36", "BODY null 0 0 784 20"]"#,
            ),
        ],
    },
    Case {
        name: "root-short-quirks",
        html: PAGE_11,
        queries: &[
            (
                r"E(() => C('html, body'))",
                r#"["HTML 0 0 800 600", "BODY 0 0 800 600"]"#,
            ),
            (
                r"E(() => S('html, body'))",
                r#"["HTML 0 0 800 600", "BODY 0 0 800 600"]"#,
            ),
            (
                r"E(() => O('html, body'))",
                r#"["HTML null 0 0 800 600", "BODY null 0 0 784 584"]"#,
            ),
        ],
    },
    Case {
        name: "root-borders",
        html: PAGE_12,
        queries: &[
            (
                r#"E(() => B("html, body, #d"))"#,
                r##"["HTML 5 5 790 56", "BODY 20 20 760 26", "#d 23 23 754 20"]"##,
            ),
            (
                r#"E(() => L("html, body, #d"))"#,
                r##"["HTML [5 5 790 56]", "BODY [20 20 760 26]", "#d [23 23 754 20]"]"##,
            ),
            (
                r#"E(() => O("html, body, #d"))"#,
                r##"["HTML null 5 5 790 56", "BODY null 0 0 760 26", "#d BODY 18 18 754 20"]"##,
            ),
            (
                r#"E(() => C("html, body, #d"))"#,
                r##"["HTML 3 3 800 600", "BODY 2 2 756 22", "#d 0 0 754 20"]"##,
            ),
            (
                r#"E(() => S("html, body, #d"))"#,
                r##"["HTML 0 0 800 600", "BODY 0 0 756 22", "#d 0 0 754 20"]"##,
            ),
        ],
    },
    Case {
        name: "not-rendered",
        html: PAGE_13,
        queries: &[
            (
                r"E(() => B('#n, #np, #dc, #op, #ar, #empty, #vh, head, title'))",
                r##"["HEAD 0 0 0 0", "TITLE 0 0 0 0", "#n 0 0 0 0", "#np 0 0 0 0", "#dc 0 0 0 0", "#op 0 0 0 0", "#ar 8 52 0 0", "#empty 38 53 0 17", "#vh 8 72 784 5"]"##,
            ),
            (
                r"E(() => L('#n, #np, #dc, #dcp, #op, #ar, #empty, #vh, head'))",
                r##"["HEAD []", "#n []", "#np []", "#dc []", "#dcp [8 16 784 20]", "#op []", "#ar [8 52 0 0]", "#empty [38 53 0 17]", "#vh [8 72 784 5]"]"##,
            ),
            (
                r"E(() => O('#n, #np, #dc, #dcp, #op, #ar, #empty, #vh, head'))",
                r##"["HEAD null 0 0 0 0", "#n null 0 0 0 0", "#np null 0 0 0 0", "#dc null 0 0 0 0", "#dcp BODY 8 16 784 20", "#op null 0 0 0 0", "#ar BODY 8 52 0 0", "#empty BODY 38 53 0 17", "#vh BODY 8 72 784 5"]"##,
            ),
            (
                r"E(() => C('#n, #np, #dc, #op, #ar, #empty, #vh, head'))",
                r##"["HEAD 0 0 0 0", "#n 0 0 0 0", "#np 0 0 0 0", "#dc 0 0 0 0", "#op 0 0 0 0", "#ar 0 0 0 0", "#empty 0 0 0 0", "#vh 0 0 784 5"]"##,
            ),
            (
                r"E(() => S('#n, #np, #dc, #op, #ar, #vh, head'))",
                r##"["HEAD 0 0 0 0", "#n 0 0 0 0", "#np 0 0 0 0", "#dc 0 0 0 0", "#op 0 0 0 0", "#ar 0 0 0 0", "#vh 0 0 784 5"]"##,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.style.width = '10px'; return [R(e.getBoundingClientRect()), e.getClientRects().length, N(e.offsetParent), e.offsetWidth, e.clientWidth, e.scrollWidth, e.scrollTop]; })())",
                r#"["0 0 0 0", 0, "null", 0, 0, 0, 0]"#,
            ),
            (
                r"E(() => (() => { const e = tp.content.firstChild; return [R(e.getBoundingClientRect()), e.getClientRects().length, N(e.offsetParent), e.offsetWidth]; })())",
                r#"["0 0 0 0", 0, "null", 0]"#,
            ),
            (
                r"E(() => (() => { const e = document.createElement('div'); e.scrollTop = 5; return e.scrollTop; })())",
                r"0",
            ),
        ],
    },
    Case {
        name: "rounding",
        html: PAGE_14,
        queries: &[
            (
                r#"E(() => B(".r, #bw, #bw2, #sc"))"#,
                r##"["#r00 10.25 50 20.25 5.25", "#r10 10.5 70 20.5 5.5", "#r20 10.75 90 20.75 5.75", "#r30 10.390625 110 20.390625 5.390625", "#r40 10.59375 130 20.59375 5.59375", "#r50 10.484375 150 20.5 5.484375", "#r60 0.296875 170 0.296875 0.296875", "#r70 0.5 190 0.5 0.5", "#r80 7.6875 210 0.6875 0.6875", "#bw 300 10 13 12.5", "#bw2 300.5 40 13 13", "#sc 400 10 50.5 50.5"]"##,
            ),
            (
                r#"E(() => L(".r, #bw, #bw2, #sc"))"#,
                r##"["#r00 [10.25 50 20.25 5.25]", "#r10 [10.5 70 20.5 5.5]", "#r20 [10.75 90 20.75 5.75]", "#r30 [10.390625 110 20.390625 5.390625]", "#r40 [10.59375 130 20.59375 5.59375]", "#r50 [10.484375 150 20.5 5.484375]", "#r60 [0.296875 170 0.296875 0.296875]", "#r70 [0.5 190 0.5 0.5]", "#r80 [7.6875 210 0.6875 0.6875]", "#bw [300 10 13 12.5]", "#bw2 [300.5 40 13 13]", "#sc [400 10 50.5 50.5]"]"##,
            ),
            (
                r#"E(() => O(".r, #bw, #bw2, #sc"))"#,
                r##"["#r00 BODY 10 50 20 5", "#r10 BODY 11 70 21 6", "#r20 BODY 11 90 21 6", "#r30 BODY 10 110 20 5", "#r40 BODY 11 130 21 6", "#r50 BODY 10 150 21 5", "#r60 BODY 0 170 0 0", "#r70 BODY 1 190 1 1", "#r80 BODY 8 210 1 1", "#bw BODY 300 10 13 13", "#bw2 BODY 301 40 13 13", "#sc BODY 400 10 51 51"]"##,
            ),
            (
                r#"E(() => C(".r, #bw, #bw2, #sc"))"#,
                r##"["#r00 0 0 20 5", "#r10 0 0 21 6", "#r20 0 0 21 6", "#r30 0 0 20 5", "#r40 0 0 21 6", "#r50 0 0 21 5", "#r60 0 0 0 0", "#r70 0 0 1 1", "#r80 0 0 1 1", "#bw 1 1 11 11", "#bw2 1 1 11 11", "#sc 0 0 51 51"]"##,
            ),
            (
                r#"E(() => S(".r, #bw, #bw2, #sc"))"#,
                r##"["#r00 0 0 20 5", "#r10 0 0 21 6", "#r20 0 0 21 6", "#r30 0 0 20 5", "#r40 0 0 21 6", "#r50 0 0 21 5", "#r60 0 0 0 0", "#r70 0 0 1 1", "#r80 0 0 1 1", "#bw 0 0 11 11", "#bw2 0 0 11 11", "#sc 0 0 101 100"]"##,
            ),
        ],
    },
    Case {
        name: "window-metrics",
        html: PAGE_15,
        queries: &[
            (
                r"E(() => [innerWidth, innerHeight, outerWidth, outerHeight, devicePixelRatio, screenX, screenY, screenLeft, screenTop])",
                r"[800, 600, 800, 600, 1, 0, 0, 0, 0]",
            ),
            (
                r"E(() => [screen.width, screen.height, screen.availWidth, screen.availHeight, screen.colorDepth, screen.pixelDepth, screen.availLeft, screen.availTop])",
                r"[800, 600, 800, 600, 24, 24, 0, 0]",
            ),
            (
                r"E(() => [screen.orientation.type, screen.orientation.angle, String(screen), String(screen.orientation), screen === screen, screen.orientation === screen.orientation, screen instanceof Screen, screen.orientation instanceof EventTarget])",
                r#"["landscape-primary", 0, "[object Screen]", "[object ScreenOrientation]", true, true, true, true]"#,
            ),
            (
                r"E(() => [Object.getOwnPropertyNames(Screen.prototype).join(), Object.getOwnPropertyNames(ScreenOrientation.prototype).join()])",
                r#"["availWidth,availHeight,width,height,colorDepth,pixelDepth,availLeft,availTop,orientation,constructor,onchange,isExtended", "angle,type,onchange,lock,unlock,constructor"]"#,
            ),
            (
                r"E(() => ['innerWidth', 'outerWidth', 'scrollX', 'pageXOffset', 'screen', 'devicePixelRatio', 'screenLeft', 'scrollTo', 'scrollBy', 'scroll'].map(n => n + ':' + JSON.stringify(Object.getOwnPropertyDescriptor(window, n) && Object.keys(Object.getOwnPropertyDescriptor(window, n)))))",
                r#"["innerWidth:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "outerWidth:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "scrollX:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "pageXOffset:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "screen:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "devicePixelRatio:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "screenLeft:[\"get\",\"set\",\"enumerable\",\"configurable\"]", "scrollTo:[\"value\",\"writable\",\"enumerable\",\"configurable\"]", "scrollBy:[\"value\",\"writable\",\"enumerable\",\"configurable\"]", "scroll:[\"value\",\"writable\",\"enumerable\",\"configurable\"]"]"#,
            ),
            (
                r"E(() => (innerWidth = 5, [innerWidth, typeof Object.getOwnPropertyDescriptor(window, 'innerWidth').value]))",
                r#"[5, "number"]"#,
            ),
            (
                r"E(() => (scrollY = 7, devicePixelRatio = 3, [scrollY, devicePixelRatio]))",
                r"[7, 3]",
            ),
            (
                r"E(() => [typeof window.matchMedia, typeof requestAnimationFrame, typeof document.elementFromPoint, typeof document.elementsFromPoint, typeof document.caretPositionFromPoint])",
                r#"["function", "function", "function", "function", "function"]"#,
            ),
        ],
    },
    Case {
        name: "element-from-point",
        html: PAGE_16,
        queries: &[
            (
                r"E(() => [[10,10],[60,60],[99.5,99.5],[100,100],[210,10],[-1,5],[5,-1],[5,700],[900,5],[799,599],[799.9,599.9],[800,0],[0,600],[305,5],[415,15],[405,5],[510,10],[650,50],[0,0]].map(p => p.join(',') + '=' + N(document.elementFromPoint(p[0], p[1]))))",
                r#"["10,10=#a", "60,60=#b", "99.5,99.5=#b", "100,100=#b", "210,10=HTML", "-1,5=null", "5,-1=null", "5,700=null", "900,5=null", "799,599=#d", "799.9,599.9=null", "800,0=null", "0,600=null", "305,5=#t", "415,15=#rr", "405,5=#sv", "510,10=HTML", "650,50=#sc", "0,0=#a"]"#,
            ),
            (
                r"E(() => [[60,60],[10,10],[210,10],[415,15],[650,50],[-1,5]].map(p => p.join(',') + '=' + document.elementsFromPoint(p[0], p[1]).map(N).join(' ')))",
                r#"["60,60=#b #a HTML", "10,10=#a HTML", "210,10=HTML", "415,15=#rr #sv HTML", "650,50=#sc HTML", "-1,5="]"#,
            ),
            (
                r"E(() => (scrollTo(0, 250), [N(document.elementFromPoint(5, 55)), N(document.elementFromPoint(5, 5)), N(document.elementFromPoint(60, 60))]))",
                r##"["#p", "HTML", "#p"]"##,
            ),
            (
                r"E(() => (sc.scrollTop = 150, [N(document.elementFromPoint(650, 60)), N(document.elementFromPoint(650, 40))]))",
                r##"["#p", "HTML"]"##,
            ),
            (
                r"E(() => [document.elementFromPoint.length, document.elementsFromPoint.length, Array.isArray(document.elementsFromPoint(1, 1))])",
                r"[2, 2, true]",
            ),
            (
                r"E(() => document.elementFromPoint())",
                r#""TypeError: Failed to execute 'elementFromPoint' on 'Document': 2 arguments required, but only 0 present.""#,
            ),
            (
                r"E(() => document.elementFromPoint(NaN, 1))",
                r#""TypeError: Failed to execute 'elementFromPoint' on 'Document': The provided double value is non-finite.""#,
            ),
            (
                r"E(() => N(document.elementFromPoint('5', '5')))",
                r#""HTML""#,
            ),
            (
                r"E(() => N(document.elementFromPoint(1e20, 1e20)))",
                r#""null""#,
            ),
        ],
    },
    Case {
        name: "flex-grid-float",
        html: PAGE_17,
        queries: &[
            (
                r#"E(() => B("body *:not(script)"))"#,
                r##"["#fx 8 8 306 46", "#f1 11 21 87.671875 20", "#f2 107.171875 21 175.328125 20", "#f3 291 21 20 20", "#gr 8 54 301 20", "#g1 8 54 50 20", "#g2 63 54 80.328125 20", "#g3 148.328125 54 160.671875 20", "#fl 8 74 60 30", "#fp 8 74 200 80", "#cl 8 154 784 0", "#ul 8 158 784 40", "#li1 38 158 754 20", "#li2 38 178 754 20"]"##,
            ),
            (
                r#"E(() => L("body *:not(script)"))"#,
                r##"["#fx [8 8 306 46]", "#f1 [11 21 87.671875 20]", "#f2 [107.171875 21 175.328125 20]", "#f3 [291 21 20 20]", "#gr [8 54 301 20]", "#g1 [8 54 50 20]", "#g2 [63 54 80.328125 20]", "#g3 [148.328125 54 160.671875 20]", "#fl [8 74 60 30]", "#fp [8 74 200 80]", "#cl [8 154 784 0]", "#ul [8 158 784 40]", "#li1 [38 158 754 20]", "#li2 [38 178 754 20]"]"##,
            ),
            (
                r#"E(() => O("body *:not(script)"))"#,
                r##"["#fx BODY 8 8 306 46", "#f1 BODY 11 21 88 20", "#f2 BODY 107 21 175 20", "#f3 BODY 291 21 20 20", "#gr BODY 8 54 301 20", "#g1 BODY 8 54 50 20", "#g2 BODY 63 54 80 20", "#g3 BODY 148 54 161 20", "#fl BODY 8 74 60 30", "#fp BODY 8 74 200 80", "#cl BODY 8 154 784 0", "#ul BODY 8 158 784 40", "#li1 BODY 38 158 754 20", "#li2 BODY 38 178 754 20"]"##,
            ),
            (
                r#"E(() => C("body *:not(script)"))"#,
                r##"["#fx 0 0 306 46", "#f1 0 0 88 20", "#f2 0 0 175 20", "#f3 0 0 20 20", "#gr 0 0 301 20", "#g1 0 0 50 20", "#g2 0 0 80 20", "#g3 0 0 161 20", "#fl 0 0 60 30", "#fp 0 0 200 80", "#cl 0 0 784 0", "#ul 0 0 784 40", "#li1 0 0 754 20", "#li2 0 0 754 20"]"##,
            ),
            (
                r#"E(() => S("body *:not(script)"))"#,
                r##"["#fx 0 0 306 46", "#f1 0 0 88 20", "#f2 0 0 175 20", "#f3 0 0 20 20", "#gr 0 0 301 20", "#g1 0 0 50 20", "#g2 0 0 80 20", "#g3 0 0 161 20", "#fl 0 0 60 30", "#fp 0 0 200 80", "#cl 0 0 784 0", "#ul 0 0 784 40", "#li1 0 0 754 20", "#li2 0 0 754 20"]"##,
            ),
        ],
    },
    Case {
        name: "scrolled-viewport",
        html: PAGE_18,
        queries: &[
            (
                r"E(() => B('#abs, #fix, #fs, #st, #sc, #scabs, #scin'))",
                r##"["#abs 10 300 20 20", "#fix 5 565 30 30", "#fs 5 566 4.453125 17", "#st 8 50 784 15", "#sc 8 85 100 100", "#scabs 13 235 10 10", "#scin 8 85 100 300"]"##,
            ),
            (
                r"E(() => O('#abs, #fix, #fs, #st, #scabs'))",
                r##"["#abs BODY 10 300 20 20", "#fix null 5 565 30 30", "#fs #fix 0 1 4 17", "#st BODY 8 50 784 15", "#scabs #sc 5 150 10 10"]"##,
            ),
            (
                r"E(() => (scrollTo(0, 200), sc.scrollTop = 30, B('#abs, #fix, #fs, #st, #sc, #scabs, #scin').concat(O('#abs, #fix, #fs, #st, #scabs'), L('#st'))))",
                r##"["#abs 10 100 20 20", "#fix 5 565 30 30", "#fs 5 566 4.453125 17", "#st 8 10 784 15", "#sc 8 -115 100 100", "#scabs 13 5 10 10", "#scin 8 -145 100 300", "#abs BODY 10 300 20 20", "#fix null 5 565 30 30", "#fs #fix 0 1 4 17", "#st BODY 8 210 784 15", "#scabs #sc 5 150 10 10", "#st [8 10 784 15]"]"##,
            ),
            (
                r"E(() => (scrollTo(0, 2000), B('#st, #fix')))",
                r##"["#fix 5 565 30 30", "#st 8 10 784 15"]"##,
            ),
        ],
    },
    Case {
        name: "block-in-inline",
        html: PAGE_19,
        queries: &[
            (
                r#"E(() => B("body *:not(script)"))"#,
                r##"["#w 8 8 300 60", "#s 8 9 300 57", "#d 8 28 300 20", "#ib1 8 68 784 60", "#ibs 20.453125 68 40 60", "#pr 8 144 784 20", "#rot 31.34375 139 10 30", "#sk 61.41084671020508 146.5 35.45955276489258 15"]"##,
            ),
            (
                r#"E(() => L("body *:not(script)"))"#,
                r##"["#w [8 8 300 60]", "#s [8 9 45.375 17 | 8 28 300 20 | 8 49 32.015625 17]", "#d [8 28 300 20]", "#ib1 [8 68 784 60]", "#ibs [20.453125 68 40 60]", "#pr [8 144 784 20]", "#rot [31.34375 139 10 30]", "#sk [61.41084671020508 146.5 35.45955276489258 15]"]"##,
            ),
            (
                r#"E(() => O("body *:not(script)"))"#,
                r##"["#w BODY 8 8 300 60", "#s BODY 8 9 300 57", "#d BODY 8 28 300 20", "#ib1 BODY 8 68 784 60", "#ibs BODY 20 68 40 60", "#pr BODY 8 144 784 20", "#rot BODY 21 149 30 10", "#sk BODY 69 149 20 10"]"##,
            ),
            (
                r#"E(() => C("body *:not(script)"))"#,
                r##"["#w 0 0 300 60", "#s 0 0 0 0", "#d 0 0 300 20", "#ib1 0 0 784 60", "#ibs 0 0 40 60", "#pr 0 0 784 20", "#rot 0 0 30 10", "#sk 0 0 20 10"]"##,
            ),
            (
                r#"E(() => S("body *:not(script)"))"#,
                r##"["#w 0 0 300 60", "#s 0 0 0 0", "#d 0 0 300 20", "#ib1 0 0 784 60", "#ibs 0 0 40 60", "#pr 0 0 784 25", "#rot 0 0 30 10", "#sk 0 0 20 10"]"##,
            ),
        ],
    },
    Case {
        name: "details-image",
        html: PAGE_20,
        queries: &[
            (
                r#"E(() => B("body *:not(script):not(option)"))"#,
                r##"["#dt 8 8 784 20", "#sm 8 8 784 20", "#dp 8 44 784 20", "#dt2 8 28 784 56", "#sm2 8 28 784 20", "#dp2 8 64 784 20", "#im 12 116 38 26", "#in 54 131 108 21", "#bt 162 131 30.828125 21", "#ta 192.828125 100 106 46", "#se 298.828125 132 30 19"]"##,
            ),
            (
                r#"E(() => L("body *:not(script):not(option)"))"#,
                r##"["#dt [8 8 784 20]", "#sm [8 8 784 20]", "#dp [8 44 784 20]", "#dt2 [8 28 784 56]", "#sm2 [8 28 784 20]", "#dp2 [8 64 784 20]", "#im [12 116 38 26]", "#in [54 131 108 21]", "#bt [162 131 30.828125 21]", "#ta [192.828125 100 106 46]", "#se [298.828125 132 30 19]"]"##,
            ),
            (
                r#"E(() => O("body *:not(script):not(option)"))"#,
                r##"["#dt BODY 8 8 784 20", "#sm BODY 8 8 784 20", "#dp BODY 8 44 784 20", "#dt2 BODY 8 28 784 56", "#sm2 BODY 8 28 784 20", "#dp2 BODY 8 64 784 20", "#im BODY 12 116 38 26", "#in BODY 54 131 108 21", "#bt BODY 162 131 31 21", "#ta BODY 193 100 106 46", "#se BODY 299 132 30 19"]"##,
            ),
            (
                r#"E(() => C("body *:not(script):not(option)"))"#,
                r##"["#dt 0 0 784 20", "#sm 0 0 784 20", "#dp 0 0 784 20", "#dt2 0 0 784 56", "#sm2 0 0 784 20", "#dp2 0 0 784 20", "#im 1 1 36 24", "#in 2 2 104 17", "#bt 2 2 27 17", "#ta 1 1 104 44", "#se 1 1 28 17"]"##,
            ),
            (
                r#"E(() => S("body *:not(script):not(option)"))"#,
                r##"["#dt 0 0 784 20", "#sm 0 0 784 20", "#dp 0 0 784 20", "#dt2 0 0 784 56", "#sm2 0 0 784 20", "#dp2 0 0 784 20", "#im 0 0 36 24", "#in 0 0 104 17", "#bt 0 0 27 17", "#ta 0 0 104 44", "#se 0 0 28 17"]"##,
            ),
        ],
    },
    Case {
        name: "nested-transforms",
        html: PAGE_21,
        queries: &[
            (
                r"E(() => B('body *:not(script)'))",
                r##"["#o -8.30126953125 -8.30126953125 136.6025390625 136.6025390625", "#i 11.028857231140137 -10.131397247314453 102.451904296875 102.451904296875", "#c 41.02885437011719 -10.131397247314453 20.49038314819336 20.490381240844727", "#sc 10 170 100 100", "#sct 10 280 100 300", "#sctc 10 280 10 10"]"##,
            ),
            (
                r"E(() => O('body *:not(script)'))",
                r##"["#o BODY 10 10 100 100", "#i #o 10 0 50 50", "#c #i 0 0 10 10", "#sc BODY 10 170 100 100", "#sct BODY 10 270 100 300", "#sctc #sct 0 0 10 10"]"##,
            ),
            (
                r"E(() => C('body *:not(script)'))",
                r##"["#o 0 0 100 100", "#i 0 0 50 50", "#c 0 0 10 10", "#sc 0 0 100 100", "#sct 0 0 100 300", "#sctc 0 0 10 10"]"##,
            ),
            (
                r"E(() => (sc.scrollTop = 50, B('#sct, #sctc')))",
                r##"["#sct 10 230 100 300", "#sctc 10 230 10 10"]"##,
            ),
        ],
    },
    Case {
        name: "domrect",
        html: PAGE_22,
        queries: &[
            (
                r"E(() => (() => { const r = new DOMRect(1, 2, 3, 4); return [String(r), r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left, JSON.stringify(r), JSON.stringify(r.toJSON())]; })())",
                r#"["[object DOMRect]", 1, 2, 3, 4, 2, 4, 6, 1, "{\"x\":1,\"y\":2,\"width\":3,\"height\":4,\"top\":2,\"right\":4,\"bottom\":6,\"left\":1}", "{\"x\":1,\"y\":2,\"width\":3,\"height\":4,\"top\":2,\"right\":4,\"bottom\":6,\"left\":1}"]"#,
            ),
            (
                r"E(() => (() => { const r = new DOMRect(10, 20, -3, -4); return [r.top, r.right, r.bottom, r.left]; })())",
                r"[16, 10, 20, 7]",
            ),
            (
                r"E(() => (() => { const r = new DOMRect(); r.x = 5; r.width = '7'; r.height = NaN; return [r.x, r.y, r.width, r.height, r.right, r.bottom]; })())",
                r"[5, 0, 7, NaN, 12, NaN]",
            ),
            (
                r"E(() => (() => { const r = new DOMRectReadOnly(1, 2, 3, 4); r.x = 9; return [r.x, String(r), r instanceof DOMRect, Object.getPrototypeOf(DOMRect.prototype) === DOMRectReadOnly.prototype]; })())",
                r#"[1, "[object DOMRectReadOnly]", false, true]"#,
            ),
            (
                r"E(() => (() => { 'use strict'; const r = new DOMRectReadOnly(1, 2, 3, 4); r.x = 9; return r.x; })())",
                r#""TypeError: Cannot set property x of #<DOMRectReadOnly> which has only a getter""#,
            ),
            (
                r"E(() => (() => { const r = DOMRect.fromRect({x: 1, width: 5}); const s = DOMRectReadOnly.fromRect(); return [String(r), R(r), String(s), R(s)]; })())",
                r#"["[object DOMRect]", "1 0 5 0", "[object DOMRectReadOnly]", "0 0 0 0"]"#,
            ),
            (
                r"E(() => (() => { const r = d.getBoundingClientRect(); return [String(r), r instanceof DOMRect, r === d.getBoundingClientRect(), JSON.stringify(r)]; })())",
                r#"["[object DOMRect]", true, false, "{\"x\":11,\"y\":8,\"width\":10,\"height\":20,\"top\":8,\"right\":21,\"bottom\":28,\"left\":11}"]"#,
            ),
            (
                r"E(() => (() => { const l = d.getClientRects(); return [String(l), l.length, R(l[0]), R(l.item(0)), l[1], l.item(1), l === d.getClientRects(), Object.keys(l).join(), typeof l[Symbol.iterator], Array.isArray(l), 'item' in l]; })())",
                r#"["[object DOMRectList]", 1, "11 8 10 20", "11 8 10 20", null, null, false, "0", "function", false, true]"#,
            ),
            (
                r"E(() => (() => { const r = d.getBoundingClientRect(); r.x = 100; r.width = 1; return [R(r), R(d.getBoundingClientRect())]; })())",
                r#"["100 8 1 20", "11 8 10 20"]"#,
            ),
            (
                r"E(() => [Object.getOwnPropertyNames(DOMRectReadOnly.prototype).join(), Object.getOwnPropertyNames(DOMRect.prototype).join(), Object.getOwnPropertyNames(DOMRectReadOnly).join(), Object.getOwnPropertyNames(DOMRect).join(), Object.getOwnPropertyNames(DOMRectList.prototype).join()])",
                r#"["x,y,width,height,top,right,bottom,left,toJSON,constructor", "x,y,width,height,constructor", "length,name,prototype,fromRect", "length,name,prototype,fromRect", "length,item,constructor"]"#,
            ),
            (
                r"E(() => [DOMRect.length, DOMRectReadOnly.length, typeof DOMRectList, DOMRect.fromRect.length])",
                r#"[0, 0, "function", 0]"#,
            ),
            (r"E(() => new DOMRect(1, 2, 3, 'x').width)", r"3"),
            (
                r"E(() => DOMRect(1, 2))",
                r#""TypeError: Failed to construct 'DOMRect': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new DOMRectList())",
                r#""TypeError: Failed to construct 'DOMRectList': Illegal constructor""#,
            ),
            (
                r"E(() => (() => { const r = new DOMRect(1e308, 1e308, 1e308, 1e308); return [r.right, r.bottom, new DOMRect(Infinity, 0, -Infinity, 0).left]; })())",
                r"[Infinity, Infinity, NaN]",
            ),
            (
                r"E(() => [Object.getOwnPropertyNames(Element.prototype).filter(n => /client|scroll|offset|Rect/.test(n)).join(), Object.getOwnPropertyNames(HTMLElement.prototype).filter(n => /client|scroll|offset|Rect|inner/.test(n)).join(), Object.getOwnPropertyNames(Document.prototype).filter(n => /scroll|Point/.test(n)).join()])",
                r#"["scrollTop,scrollLeft,scrollWidth,scrollHeight,clientTop,clientLeft,clientWidth,clientHeight,getBoundingClientRect,getClientRects,scroll,scrollBy,scrollIntoView,scrollIntoViewIfNeeded,scrollTo", "offsetParent,offsetTop,offsetLeft,offsetWidth,offsetHeight,innerText,onscroll,onscrollend,onscrollsnapchange,onscrollsnapchanging", "scrollingElement,onscroll,onscrollend,caretPositionFromPoint,caretRangeFromPoint,elementFromPoint,elementsFromPoint,exitPointerLock,onscrollsnapchange,onscrollsnapchanging"]"#,
            ),
        ],
    },
    Case {
        name: "item4-events",
        html: PAGE_23,
        queries: &[
            (
                r"E(() => (() => { const e = new MouseEvent('click', {clientX: 50, clientY: 60}); const before = [e.offsetX, e.offsetY, e.layerX, e.layerY, e.pageX, e.pageY]; t.dispatchEvent(e); return before.concat([e.offsetX, e.offsetY, e.layerX, e.layerY, e.pageX, e.pageY]); })())",
                r"[50, 60, 50, 60, 50, 60, -3, 22, 50, 60, 50, 60]",
            ),
            (
                r"E(() => (() => { const log = []; t.addEventListener('mousedown', e => log.push(e.offsetX, e.offsetY, e.layerX, e.layerY), {once: true}); t.dispatchEvent(new MouseEvent('mousedown', {clientX: 45.5, clientY: 70.25})); return log; })())",
                r"[-8, 32, 45, 70]",
            ),
            (
                r"E(() => (() => { sc.scrollTop = 40; const e = new MouseEvent('click', {clientX: 20, clientY: 30}); sct.dispatchEvent(e); return [e.offsetX, e.offsetY, e.layerX, e.layerY]; })())",
                r"[9, -77, 20, 30]",
            ),
            (
                r"E(() => (() => { const e = new MouseEvent('click', {clientX: 30, clientY: 300}); rt.dispatchEvent(e); return [e.offsetX, e.offsetY, e.layerX, e.layerY]; })())",
                r"[4, 49, 7, 52]",
            ),
            (
                r"E(() => (() => { scrollTo(0, 100); const e = new MouseEvent('click', {clientX: 30, clientY: 30}); t.dispatchEvent(e); const r = [e.offsetX, e.offsetY, e.layerX, e.layerY, e.pageX, e.pageY]; scrollTo(0, 0); return r; })())",
                r"[-23, 92, 30, 30, 30, 30]",
            ),
            (
                r"E(() => (() => { const e = new MouseEvent('click', {clientX: 30, clientY: 30}); document.body.dispatchEvent(e); const f = new MouseEvent('click', {clientX: 30, clientY: 30}); document.dispatchEvent(f); const g = new MouseEvent('click', {clientX: 30, clientY: 30}); const x = document.createElement('div'); x.dispatchEvent(g); return [e.offsetX, e.offsetY, e.layerX, e.layerY, f.offsetX, f.offsetY, f.layerX, f.layerY, g.offsetX, g.offsetY, g.layerX, g.layerY]; })())",
                r"[22, 10, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30]",
            ),
            (
                r"E(() => (() => { const e = new PointerEvent('pointerdown', {clientX: 50, clientY: 60}); t.dispatchEvent(e); const w = new WheelEvent('wheel', {clientX: 50, clientY: 60}); t.dispatchEvent(w); return [e.offsetX, e.offsetY, w.offsetX, w.offsetY]; })())",
                r"[-3, 22, -3, 22]",
            ),
        ],
    },
    Case {
        name: "item4-focus-scroll",
        html: PAGE_24,
        queries: &[
            (r"E(() => (i.focus(), [scrollX, scrollY]))", r"[0, 1219]"),
            (
                r"E(() => (scrollTo(0, 0), i.focus({preventScroll: true}), b2.focus({preventScroll: true}), [scrollY]))",
                r"[0]",
            ),
            (r"E(() => (scrollTo(0, 0), b2.focus(), [scrollY]))", r"[0]"),
            (
                r"E(() => (scrollTo(0, 0), i2.focus(), [scrollY, sc.scrollTop]))",
                r"[2058, 321]",
            ),
            (
                r"E(() => (scrollTo(0, 0), i.focus(), [scrollY]))",
                r"[1219]",
            ),
            (
                r"E(() => (() => { scrollTo(0, 0); i.blur(); i.focus(); const y = scrollY; return [y, document.activeElement.id]; })())",
                r#"[1219, "i"]"#,
            ),
        ],
    },
    Case {
        name: "item4-img-size",
        html: PAGE_25,
        queries: &[
            (
                r"E(() => (a.width = 1e10, [a.width, a.height, a.getAttribute('width'), a.naturalWidth]))",
                r#"[33554428, 33554428, "1410065408", 1]"#,
            ),
            (
                r"E(() => (a.height = 50000000, [a.width, a.height, a.getAttribute('height')]))",
                r#"[33554428, 33554428, "50000000"]"#,
            ),
            (
                r"E(() => (a.width = 33554430, a.height = 33554431, [a.width, a.height]))",
                r"[33554428, 33554428]",
            ),
            (
                r"E(() => (a.width = 2000, a.height = 3000, [a.width, a.height, a.offsetWidth, a.offsetHeight]))",
                r"[2000, 3000, 2000, 3000]",
            ),
            (
                r"E(() => (b.width = 1e10, [b.width, b.height]))",
                r"[1410065408, 30]",
            ),
            (
                r"E(() => (() => { const i = new Image(); i.width = 1e10; i.height = 33554430; return [i.width, i.height]; })())",
                r"[1410065408, 33554430]",
            ),
            (
                r"E(() => (() => { const i = new Image(); i.width = 1e10; c.appendChild(i); return [i.width, i.height]; })())",
                r"[33554428, 0]",
            ),
            (
                r"E(() => (() => { const i = new Image(); i.style.width = '10.6px'; i.style.height = '7.4px'; c.appendChild(i); return [i.width, i.height]; })())",
                r"[11, 7]",
            ),
        ],
    },
];

/// The cases of `web-scroll.json`.
pub(crate) const SCROLL: &[Case] = &[
    Case {
        name: "window",
        html: PAGE_26,
        queries: &[
            (
                r"A(() => [scrollX, scrollY, pageXOffset, pageYOffset, N(document.scrollingElement)])",
                r#"[0, 0, 0, 0, "HTML"]"#,
            ),
            (r"A(() => (scrollTo(10, 20), P()))", r"[10, 20]"),
            (r"A(() => (scrollTo({top: 100}), P()))", r"[0, 100]"),
            (
                r"A(() => (scrollTo({left: 5, top: 6, behavior: 'instant'}), P()))",
                r"[5, 6]",
            ),
            (r"A(() => (scroll(1, 2), P()))", r"[1, 2]"),
            (r"A(() => (scroll({top: 7}), P()))", r"[0, 7]"),
            (r"A(() => (scrollBy(3, 4), scrollBy(3, 4), P()))", r"[6, 8]"),
            (
                r"A(() => (scrollTo(5, 5), scrollBy({top: 10}), scrollBy({left: -2, behavior: 'auto'}), P()))",
                r"[3, 15]",
            ),
            (r"A(() => (scrollTo(0, 1e9), P()))", r"[0, 2400]"),
            (r"A(() => (scrollTo(1e9, 0), P()))", r"[1200, 0]"),
            (r"A(() => (scrollTo(-5, -5), P()))", r"[0, 0]"),
            (
                r"A(() => (scrollTo(0, 10.4), [scrollY, pageYOffset, document.documentElement.scrollTop]))",
                r"[10, 10, 10]",
            ),
            (
                r"A(() => (scrollTo(0, 10.5), [scrollY, document.documentElement.scrollTop]))",
                r"[11, 11]",
            ),
            (
                r"A(() => (scrollTo(0, 10.6), [scrollY, document.documentElement.scrollTop]))",
                r"[11, 11]",
            ),
            (r"A(() => (scrollTo(0.25, 0.75), P()))", r"[0, 1]"),
            (r"A(() => (scrollTo(NaN, Infinity), P()))", r"[0, 0]"),
            (r"A(() => (scrollTo(0, 1e20), P()))", r"[0, 2400]"),
            (
                r"A(() => (scrollTo(5, 5), scrollBy(0, -Infinity), P()))",
                r"[5, 5]",
            ),
            (
                r"A(() => (scrollTo(5, 5), scrollBy(NaN, 10), P()))",
                r"[5, 15]",
            ),
            (r"A(() => (scrollTo({top: NaN, left: 3}), P()))", r"[3, 0]"),
            (r"A(() => (scrollTo(), P()))", r"[0, 0]"),
            (r"A(() => (scrollTo(1), P()))", r"[0, 0]"),
            (r"A(() => (scrollTo(1, 2, 3), P()))", r"[1, 2]"),
            (r"A(() => (scrollTo({top: 50}, 7), P()))", r"[0, 7]"),
            (r"A(() => (scrollTo('10', '20'), P()))", r"[10, 20]"),
            (r"A(() => (scrollTo(null), P()))", r"[0, 0]"),
            (r"A(() => (scrollTo(undefined, 5), P()))", r"[0, 5]"),
            (
                r"A(() => scrollTo({behavior: 'fast'}))",
                r#""[object Promise]""#,
            ),
            (r"A(() => scrollTo(5))", r#""[object Promise]""#),
            (
                r"A(() => [scrollTo.length, scrollBy.length, scroll.length, Element.prototype.scrollTo.length, Element.prototype.scrollIntoView.length])",
                r"[0, 0, 0, 0, 0]",
            ),
            (
                r"A(() => (document.documentElement.scrollTop = 50, document.documentElement.scrollLeft = 40, [scrollY, scrollX, document.documentElement.scrollTop, document.body.scrollTop]))",
                r"[50, 40, 50, 0]",
            ),
            (
                r"A(() => (document.body.scrollTop = 70, [scrollY, document.body.scrollTop]))",
                r"[0, 0]",
            ),
            (
                r"A(() => (document.documentElement.scrollTo(3, 4), document.documentElement.scrollBy(1, 1), P()))",
                r"[4, 5]",
            ),
            (
                r"A(() => (document.documentElement.scrollTop = 1e9, [scrollY, document.documentElement.scrollTop]))",
                r"[2400, 2400]",
            ),
            (
                r"A(() => (document.documentElement.scrollTop = 'x', document.documentElement.scrollTop))",
                r"0",
            ),
            (
                r"A(() => (scrollTo({top: 500, behavior: 'smooth'}), P()))",
                r"[0, 0]",
            ),
            (
                r"(Z(), scrollTo({top: 500, behavior: 'smooth'}), after(1500).then(() => P()))",
                r"[0, 500]",
            ),
            (
                r"A(() => [String(scrollTo(0, 0)), scroll() instanceof Promise, scrollBy(0, 0) instanceof Promise, sc.scrollTo(0, 0) instanceof Promise, sc.scrollBy(1, 1) instanceof Promise, sc.scroll() instanceof Promise, String(target.scrollIntoView()), String(document.documentElement.scroll())])",
                r#"["undefined", false, false, false, false, false, "undefined", "undefined"]"#,
            ),
            (
                r"(Z(), scrollTo(5).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollTo' on 'Window': The provided value is not of type 'ScrollToOptions'.""#,
            ),
            (
                r"(Z(), scrollTo({behavior: 'fast'}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollTo' on 'Window': Failed to read the 'behavior' property from 'ScrollOptions': The provided value 'fast' is not a valid enum value of type ScrollBehavior.""#,
            ),
            (
                r"(Z(), target.scrollIntoView({block: 'bad'}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollIntoView' on 'Element': Failed to read the 'block' property from 'ScrollIntoViewOptions': The provided value 'bad' is not a valid enum value of type ScrollLogicalPosition.""#,
            ),
            (
                r"(Z(), sc.scrollTo({behavior: 'fast'}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollTo' on 'Element': Failed to read the 'behavior' property from 'ScrollOptions': The provided value 'fast' is not a valid enum value of type ScrollBehavior.""#,
            ),
            (
                r"(Z(), new Promise(r => { const l = []; document.documentElement.scrollTop = 30; l.push(scrollY); r(l); }))",
                r"[30]",
            ),
        ],
    },
    Case {
        name: "element",
        html: PAGE_26,
        queries: &[
            (
                r"A(() => [sc.scrollTop, sc.scrollLeft, sc.scrollWidth, sc.scrollHeight, sc.clientWidth, sc.clientHeight])",
                r"[0, 0, 306, 806, 106, 106]",
            ),
            (
                r"A(() => (sc.scrollTop = 50, sc.scrollLeft = 20, [sc.scrollTop, sc.scrollLeft]))",
                r"[50, 20]",
            ),
            (
                r"A(() => (sc.scrollTop = 1e9, sc.scrollLeft = 1e9, [sc.scrollTop, sc.scrollLeft]))",
                r"[700, 200]",
            ),
            (r"A(() => (sc.scrollTop = -5, [sc.scrollTop]))", r"[0]"),
            (
                r"A(() => (sc.scrollTop = 10.4, sc.scrollLeft = 10.5, [sc.scrollTop, sc.scrollLeft]))",
                r"[10, 11]",
            ),
            (
                r"A(() => (sc.scrollTop = 10.6, sc.scrollLeft = 0.5, [sc.scrollTop, sc.scrollLeft]))",
                r"[11, 1]",
            ),
            (
                r"A(() => (sc.scrollTo(10, 20), [sc.scrollLeft, sc.scrollTop]))",
                r"[10, 20]",
            ),
            (
                r"A(() => (sc.scrollTo({top: 30}), sc.scrollBy(5, 5), sc.scrollBy({left: 1}), [sc.scrollLeft, sc.scrollTop]))",
                r"[6, 35]",
            ),
            (
                r"A(() => (sc.scroll(7, 8), [sc.scrollLeft, sc.scrollTop]))",
                r"[7, 8]",
            ),
            (
                r"A(() => (sc.scrollTop = NaN, sc.scrollLeft = Infinity, [sc.scrollLeft, sc.scrollTop]))",
                r"[0, 0]",
            ),
            (
                r"A(() => (sc.scrollTop = 20, sc.scrollTop = Infinity, [sc.scrollTop]))",
                r"[0]",
            ),
            (r"A(() => (sc.scrollTop = 1e20, [sc.scrollTop]))", r"[700]"),
            (
                r"A(() => (target.scrollTop = 10, target.scrollTo(5, 5), [target.scrollTop, target.scrollLeft]))",
                r"[0, 0]",
            ),
            (
                r"A(() => (hid.scrollTop = 30, hid.scrollLeft = 25, [hid.scrollTop, hid.scrollLeft]))",
                r"[30, 25]",
            ),
            (
                r"A(() => (() => { const e = document.createElement('div'); e.scrollTop = 5; e.scrollTo(1, 1); return [e.scrollTop, e.scrollLeft]; })())",
                r"[0, 0]",
            ),
            (
                r"A(() => (sc.scrollTo({top: 50, behavior: 'smooth'}), [sc.scrollTop]))",
                r"[0]",
            ),
            (
                r"A(() => [N(document.scrollingElement), document.scrollingElement === document.documentElement])",
                r#"["HTML", true]"#,
            ),
        ],
    },
    Case {
        name: "into-view",
        html: PAGE_26,
        queries: &[
            (r"A(() => (target.scrollIntoView(), P()))", r"[450, 1500]"),
            (
                r"A(() => (target.scrollIntoView(true), P()))",
                r"[450, 1500]",
            ),
            (
                r"A(() => (target.scrollIntoView(false), P()))",
                r"[450, 950]",
            ),
            (r"A(() => (target.scrollIntoView({}), P()))", r"[450, 1500]"),
            (
                r"A(() => (target.scrollIntoView({block: 'center', inline: 'center'}), P()))",
                r"[825, 1225]",
            ),
            (
                r"A(() => (target.scrollIntoView({block: 'end', inline: 'end'}), P()))",
                r"[450, 950]",
            ),
            (
                r"A(() => (target.scrollIntoView({block: 'nearest', inline: 'nearest'}), P()))",
                r"[450, 950]",
            ),
            (
                r"A(() => (target.scrollIntoView({block: 'start', inline: 'start'}), P()))",
                r"[1200, 1500]",
            ),
            (
                r"A(() => (scrollTo(1000, 1300), target.scrollIntoView({block: 'nearest', inline: 'nearest'}), P()))",
                r"[1000, 1300]",
            ),
            (
                r"A(() => (scrollTo(1100, 1480), target.scrollIntoView({block: 'nearest', inline: 'nearest'}), P()))",
                r"[1100, 1480]",
            ),
            (
                r"A(() => (scrollTo(1100, 1480), target.scrollIntoView({block: 'center'}), P()))",
                r"[1100, 1225]",
            ),
            (
                r"A(() => (big.scrollIntoView({block: 'nearest'}), P()))",
                r"[0, 2000]",
            ),
            (
                r"A(() => (scrollTo(0, 2100), big.scrollIntoView({block: 'nearest'}), P()))",
                r"[0, 2100]",
            ),
            (
                r"A(() => (scrollTo(0, 2500), big.scrollIntoView({block: 'nearest'}), P()))",
                r"[0, 2300]",
            ),
            (
                r"A(() => (big.scrollIntoView({block: 'center'}), P()))",
                r"[0, 2150]",
            ),
            (
                r"A(() => (big.scrollIntoView({block: 'end'}), P()))",
                r"[0, 2300]",
            ),
            (
                r"A(() => (it.scrollIntoView(), [sc.scrollLeft, sc.scrollTop].concat(P())))",
                r"[67, 303, 0, 2]",
            ),
            (
                r"A(() => (it.scrollIntoView({block: 'center', inline: 'center'}), [sc.scrollLeft, sc.scrollTop].concat(P())))",
                r"[110, 260, 0, 0]",
            ),
            (
                r"A(() => (it.scrollIntoView({block: 'end', inline: 'end'}), [sc.scrollLeft, sc.scrollTop].concat(P())))",
                r"[67, 217, 0, 0]",
            ),
            (
                r"A(() => (it.scrollIntoView({block: 'nearest', inline: 'nearest'}), [sc.scrollLeft, sc.scrollTop].concat(P())))",
                r"[67, 217, 0, 0]",
            ),
            (
                r"A(() => (t2.scrollIntoView(), [sc2.scrollTop].concat(P())))",
                r"[253, 0, 1002]",
            ),
            (
                r"A(() => (t2.scrollIntoView(false), [sc2.scrollTop].concat(P())))",
                r"[157, 0, 508]",
            ),
            (
                r"A(() => (hidin.scrollIntoView({block: 'end', inline: 'end'}), [hid.scrollTop, hid.scrollLeft].concat(P())))",
                r"[147, 147, 0, 0]",
            ),
            (
                r"A(() => (target.scrollIntoView({block: 'bad'}), P()))",
                r"[0, 0]",
            ),
            (
                r"A(() => (target.scrollIntoView(null), P()))",
                r"[450, 1500]",
            ),
            (
                r"A(() => (target.scrollIntoView(undefined), P()))",
                r"[450, 1500]",
            ),
            (r"A(() => (target.scrollIntoView(0), P()))", r"[450, 950]"),
            (
                r"A(() => (target.scrollIntoView('x'), P()))",
                r"[450, 1500]",
            ),
            (
                r"A(() => (target.scrollIntoView({behavior: 'smooth'}), P()))",
                r"[0, 0]",
            ),
            (
                r"A(() => (() => { const e = document.createElement('div'); e.scrollIntoView(); return P(); })())",
                r"[0, 0]",
            ),
            (
                r"A(() => (document.documentElement.scrollIntoView(), document.body.scrollIntoView(false), P()))",
                r"[0, 2400]",
            ),
        ],
    },
    Case {
        name: "events",
        html: PAGE_26,
        queries: &[
            (
                r"new Promise(r => { log = []; addEventListener('scroll', W); document.addEventListener('scroll', W); sc.addEventListener('scroll', W); sc2.addEventListener('scroll', W); r(1); })",
                r"1",
            ),
            (
                r"(log = [], Z(), after(100).then(() => (log = [], scrollTo(0, 100), log.push('sync'), after(100))).then(() => log))",
                r#"["sync", "document@document:true:false:0,100", "document@window:true:false:0,100"]"#,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], sc.scrollTop = 10, scrollTo(0, 200), log.push('sync'), after(100))).then(() => log))",
                r##"["sync", "#sc@#sc:false:false:0,200", "document@document:true:false:0,200", "document@window:true:false:0,200"]"##,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], sc2.scrollTop = 10, sc.scrollTop = 20, log.push('sync'), after(100))).then(() => log))",
                r##"["sync", "#sc2@#sc2:false:false:0,200", "#sc@#sc:false:false:0,200"]"##,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], scrollTo(0, scrollY), sc.scrollTop = sc.scrollTop, after(100))).then(() => log))",
                r"[]",
            ),
            (
                r"(log = [], after(100).then(() => (log = [], scrollTo(0, 300), scrollTo(0, 200), after(100))).then(() => log))",
                r#"["document@document:true:false:0,200", "document@window:true:false:0,200"]"#,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], scrollTo(0, 300), scrollTo(0, 200), after(100))).then(() => log))",
                r#"["document@document:true:false:0,200", "document@window:true:false:0,200"]"#,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], target.scrollIntoView(), after(100))).then(() => log))",
                r#"["document@document:true:false:450,1500", "document@window:true:false:450,1500"]"#,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], t2.scrollIntoView(), after(100))).then(() => log))",
                r##"["#sc2@#sc2:false:false:105,1002", "document@document:true:false:105,1002", "document@window:true:false:105,1002"]"##,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], scrollTo(0, 0), after(0).then(() => { log.push('t0:' + scrollY); }), after(100))).then(() => log))",
                r#"["t0:0", "document@document:true:false:0,0", "document@window:true:false:0,0"]"#,
            ),
            (
                r"(log = [], after(100).then(() => (log = [], scrollTo(0, 50), new Promise(r => requestAnimationFrame(() => { log.push('raf'); r(); })))).then(() => after(50)).then(() => log))",
                r#"["document@document:true:false:0,50", "document@window:true:false:0,50", "raf"]"#,
            ),
        ],
    },
    Case {
        name: "rejections",
        html: PAGE_26,
        queries: &[
            (
                r"A(() => String(sc.scrollTo({get top() { throw new Error('boom'); }})))",
                r#""[object Promise]""#,
            ),
            (
                r"(Z(), sc.scrollTo({get top() { throw new Error('boom'); }}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected Error: boom""#,
            ),
            (
                r"(Z(), target.scrollIntoView({get inline() { throw new Error('boom'); }}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected Error: boom""#,
            ),
            (
                r"(Z(), scrollBy({get behavior() { throw new TypeError('b2'); }}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: b2""#,
            ),
            (
                r"(Z(), scroll({left: 1, behavior: 'fast'}).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scroll' on 'Window': Failed to read the 'behavior' property from 'ScrollOptions': The provided value 'fast' is not a valid enum value of type ScrollBehavior.""#,
            ),
            (
                r"(Z(), scrollIntoView = Element.prototype.scrollIntoView, Promise.resolve(scrollIntoView.call({})).then(() => 'resolved', e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollIntoView' on 'Element': Illegal invocation""#,
            ),
            (
                r"(Z(), Promise.resolve(Element.prototype.scrollTo.call(document, 0, 0)).then(v => 'resolved ' + v, e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollTo' on 'Element': Illegal invocation""#,
            ),
            (
                r"(Z(), Promise.resolve(scrollTo.call(document.body, 0, 0)).then(v => 'resolved ' + v, e => 'rejected ' + e))",
                r#""rejected TypeError: Failed to execute 'scrollTo' on 'Window': Illegal invocation""#,
            ),
            (
                r"A(() => (scrollTo({behavior: 'fast'}), scrollTo(0, 30), sc.scrollTo(5), sc.scrollTop = 7, P().concat([sc.scrollTop])))",
                r"[0, 30, 7]",
            ),
            (
                r"(Z(), new Promise(r => { const l = []; const h = e => { if (/ScrollToOptions/.test(e.reason.message)) l.push(e.reason.constructor.name + ' ' + (e.promise instanceof Promise)); e.preventDefault(); }; addEventListener('unhandledrejection', h); scrollTo(5); setTimeout(() => { removeEventListener('unhandledrejection', h); r(l); }, 50); }))",
                r#"["TypeError true"]"#,
            ),
        ],
    },
    Case {
        name: "quirks",
        html: PAGE_27,
        queries: &[
            (
                r"A(() => [N(document.scrollingElement), scrollY])",
                r#"["BODY", 0]"#,
            ),
            (
                r"A(() => (document.body.scrollTop = 70, [scrollY, document.body.scrollTop, document.documentElement.scrollTop]))",
                r"[70, 70, 0]",
            ),
            (
                r"A(() => (document.documentElement.scrollTop = 30, [scrollY, document.body.scrollTop, document.documentElement.scrollTop]))",
                r"[0, 0, 0]",
            ),
            (
                r"A(() => (document.body.scrollTo(0, 40), document.body.scrollBy(0, 2), [scrollY, document.body.scrollTop]))",
                r"[42, 42]",
            ),
            (
                r"A(() => (scrollTo(0, 20), [document.body.scrollTop, document.body.scrollHeight, document.body.clientHeight, document.documentElement.scrollHeight, document.documentElement.clientHeight]))",
                r"[20, 3000, 600, 3000, 3000]",
            ),
        ],
    },
    Case {
        name: "short",
        html: PAGE_28,
        queries: &[
            (
                r"A(() => (scrollTo(0, 100), document.documentElement.scrollTop = 100, P()))",
                r"[0, 0]",
            ),
            (r"A(() => (d.scrollIntoView(false), P()))", r"[0, 0]"),
        ],
    },
    Case {
        name: "huge",
        html: PAGE_29,
        queries: &[
            (
                r"A(() => [document.documentElement.scrollHeight, document.documentElement.scrollWidth, h.offsetHeight, h.getBoundingClientRect().height])",
                r"[33554432, 33554432, 33554428, 33554428]",
            ),
            (
                r"A(() => (scrollTo(1e12, 1e12), P()))",
                r"[33553632, 33553832]",
            ),
            (
                r"A(() => (end.scrollIntoView(), P().concat([end.getBoundingClientRect().top, end.offsetTop])))",
                r"[0, 33553832, 599.984375, 33554432]",
            ),
            (r"A(() => (scrollTo(0, 33554000), P()))", r"[0, 33553832]"),
        ],
    },
];
