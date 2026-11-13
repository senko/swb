//! Events through shadow trees (M10 sub-feature 5d, ADR 0036 section 7),
//! with and without the GC stress mode: the path through hosts and slots,
//! retargeting, `relatedTarget` per struct, closed trees in `composedPath`,
//! and the clearing of the targets after the dispatch. The cases that
//! Chromium measured are in `tools/probes/web-shadow-dom.json` and run in
//! `crates/engine/tests/probe_net.rs`; these tests add shapes that the
//! probe lacks (nested trees, slots, deep chains) and the wrappers that the
//! dispatch makes for each struct.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use common::{answer, expected, load, on_big_stack};

/// `o > h` is a host with the light children `l` (for the slot `sl`) and
/// `u` (no slot); `h`'s root `r1` holds `in > (sl, p)`, and `p` is a host
/// with the root `r2 > em`. A listener (capture `^` and bubble) on every
/// node of interest logs `currentTarget:target:phase`.
const PAGE: &str = "<!DOCTYPE html><html><body><div id=o><div id=h>\
    <b id=l slot=s>l</b><i id=u>u</i></div></div><span id=out>out</span>\
    <script>\
    function k(e) { if (e == null) return String(e); if (e === window) return 'win'; \
      if (e.nodeType === 9) return 'doc'; \
      if (e.nodeType === 11) return 'root(' + k(e.host) + ')'; \
      return e.localName + (e.id ? '#' + e.id : ''); }\
    var L = [];\
    function T(x) { [true, false].forEach(function (c) { \
      x.addEventListener('x', function (e) { \
        L.push(k(e.currentTarget) + (c ? '^' : '') + ':' + k(e.target) + ':' + e.eventPhase); \
      }, c); }); }\
    var r1 = h.attachShadow({mode: 'open'});\
    r1.innerHTML = '<div id=in><slot id=sl name=s></slot><p id=p>p</p></div>';\
    var inner = r1.getElementById('in'), sl = r1.getElementById('sl'), p = r1.getElementById('p');\
    var r2 = p.attachShadow({mode: 'open'});\
    r2.innerHTML = '<em id=em>e</em>';\
    var em = r2.firstChild;\
    [window, document, o, h, r1, inner, sl, p, r2, em, l, u, out].forEach(T);\
    function run(target, init) { L.length = 0; var e = new Event('x', init); \
      target.dispatchEvent(e); \
      return L.join(' ') + ' | after:' + k(e.target) + ':' + e.composedPath().length; }\
    </script></body></html>";

/// Evaluates `expression` on the page, with and without the GC stress mode,
/// and compares the string with `want`.
fn check(expression: &str, want: &str) {
    for stress in [false, true] {
        let expression = expression.to_owned();
        let want = want.to_owned();
        on_big_stack(move || {
            let mut rt = load(PAGE, stress);
            let got = answer(&mut rt, &expression);
            assert_eq!(got, expected(&format!("{want:?}")), "stress {stress}");
        });
    }
}

/// A composed event at an element of the inner tree: the listeners see the
/// target retargeted against their own tree; the capture pass runs from the
/// window down; a host is at its target while the path is inside its tree.
#[test]
fn composed_event_through_nested_trees_is_retargeted() {
    check(
        "run(em, {bubbles: true, composed: true})",
        "win^:div#h:1 doc^:div#h:1 div#o^:div#h:1 div#h^:div#h:2 \
         root(div#h)^:p#p:1 div#in^:p#p:1 p#p^:p#p:2 root(p#p)^:em#em:1 em#em^:em#em:2 \
         em#em:em#em:2 root(p#p):em#em:3 p#p:p#p:2 div#in:p#p:3 root(div#h):p#p:3 \
         div#h:div#h:2 div#o:div#h:3 doc:div#h:3 win:div#h:3 | after:div#h:0",
    );
}

/// A non-composed event stops at the root of its target's tree.
#[test]
fn not_composed_event_stops_at_the_root() {
    check(
        "run(em, {bubbles: true})",
        "root(p#p)^:em#em:1 em#em^:em#em:2 em#em:em#em:2 root(p#p):em#em:3 | after:null:0",
    );
}

/// A light child goes through its slot: its target is not retargeted (its
/// tree is the document's), and the host is passed once, from the root.
#[test]
fn a_slotted_node_goes_through_its_slot() {
    check(
        "run(l, {bubbles: true})",
        "win^:b#l:1 doc^:b#l:1 div#o^:b#l:1 div#h^:b#l:1 root(div#h)^:b#l:1 div#in^:b#l:1 \
         slot#sl^:b#l:1 b#l^:b#l:2 b#l:b#l:2 slot#sl:b#l:3 div#in:b#l:3 root(div#h):b#l:3 \
         div#h:b#l:3 div#o:b#l:3 doc:b#l:3 win:b#l:3 | after:b#l:0",
    );
}

/// Listeners at each level of nested trees read the target of their own
/// level: a shadow host that is slotted into another tree is the target
/// for the outer listeners, and the outer host is not (a root that the
/// path leaves in the middle of the chain is not the root of the first
/// target; the review of 5d found that a test did not tell).
#[test]
fn a_slotted_host_is_the_target_of_the_outer_tree() {
    check(
        "(function () { var h1 = document.createElement('div'); h1.id = 'h1'; \
         var h2 = document.createElement('div'); h2.id = 'h2'; h2.slot = 'a'; \
         h1.appendChild(h2); document.body.appendChild(h1); \
         var r1 = h1.attachShadow({mode: 'open'}); r1.innerHTML = '<slot id=sa name=a></slot>'; \
         var r2 = h2.attachShadow({mode: 'open'}); r2.innerHTML = '<b id=eb>e</b>'; \
         var eb = r2.firstChild, sa = r1.firstChild, seen = []; \
         [window, document, h1, r1, sa, h2, r2, eb].forEach(function (n) { \
           [true, false].forEach(function (c) { n.addEventListener('w', function (e) { \
             seen.push(k(e.currentTarget) + (c ? '^' : '') + ':' + k(e.target) + ':' + e.eventPhase); \
           }, c); }); }); \
         eb.dispatchEvent(new Event('w', {bubbles: true, composed: true})); \
         return seen.join(' '); })()",
        "win^:div#h2:1 doc^:div#h2:1 div#h1^:div#h2:1 root(div#h1)^:div#h2:1 slot#sa^:div#h2:1 \
         div#h2^:div#h2:2 root(div#h2)^:b#eb:1 b#eb^:b#eb:2 b#eb:b#eb:2 root(div#h2):b#eb:3 \
         div#h2:div#h2:2 slot#sa:div#h2:3 root(div#h1):div#h2:3 div#h1:div#h2:3 \
         doc:div#h2:3 win:div#h2:3",
    );
}

/// A light child that no slot takes has the host as parent; a composed
/// event at the host does not enter its tree.
#[test]
fn an_unassigned_node_and_a_host_keep_to_the_light_tree() {
    check(
        "run(u, {bubbles: true})",
        "win^:i#u:1 doc^:i#u:1 div#o^:i#u:1 div#h^:i#u:1 i#u^:i#u:2 i#u:i#u:2 div#h:i#u:3 \
         div#o:i#u:3 doc:i#u:3 win:i#u:3 | after:i#u:0",
    );
    check(
        "run(h, {bubbles: true, composed: true})",
        "win^:div#h:1 doc^:div#h:1 div#o^:div#h:1 div#h^:div#h:2 div#h:div#h:2 \
         div#o:div#h:3 doc:div#h:3 win:div#h:3 | after:div#h:0",
    );
}

/// A closed root hides its tree from `composedPath()` of the listeners
/// outside it and its slot; the listeners inside see all.
#[test]
fn closed_trees_are_hidden_in_the_composed_path() {
    check(
        "(function () { var c = document.createElement('div'); c.id = 'c'; o.appendChild(c); \
         var cr = c.attachShadow({mode: 'closed'}); cr.innerHTML = '<span id=cs>s</span>'; \
         var cs = cr.firstChild, seen = []; \
         document.addEventListener('y', function (e) { seen.push('doc=' + e.composedPath().map(k).join('>')); }); \
         cr.addEventListener('y', function (e) { seen.push('root=' + e.composedPath().map(k).join('>')); }); \
         cs.dispatchEvent(new Event('y', {bubbles: true, composed: true})); \
         return seen.join(' ') + ' ' + k(cs.getRootNode().host); })()",
        "root=span#cs>root(div#c)>div#c>div#o>body>html>doc>win \
         doc=div#c>div#o>body>html>doc>win div#c",
    );
}

/// `relatedTarget` is retargeted for each struct against the struct, and a
/// struct that the retargeted related target equals is skipped.
#[test]
fn related_target_is_retargeted_per_struct() {
    // From inside the tree to an outside element: the related target is
    // outside every tree and stays as it is.
    check(
        "(function () { var seen = []; \
         [em, p, h, document].forEach(function (n) { n.addEventListener('mouseover', function (e) { \
           seen.push(k(e.currentTarget) + ':' + k(e.target) + '/' + k(e.relatedTarget)); }); }); \
         var e = new MouseEvent('mouseover', {bubbles: true, composed: true, relatedTarget: out}); \
         em.dispatchEvent(e); \
         return seen.join(' ') + ' | after:' + k(e.target) + '/' + k(e.relatedTarget); })()",
        "em#em:em#em/span#out p#p:p#p/span#out div#h:div#h/span#out doc:div#h/span#out \
         | after:div#h/span#out",
    );
    // From an outside element to the inside: the related target is the
    // outermost host.
    check(
        "(function () { var seen = []; \
         [out, document].forEach(function (n) { n.addEventListener('mouseover', function (e) { \
           seen.push(k(e.currentTarget) + ':' + k(e.target) + '/' + k(e.relatedTarget)); }); }); \
         var e = new MouseEvent('mouseover', {bubbles: true, composed: true, relatedTarget: em}); \
         out.dispatchEvent(e); \
         return seen.join(' ') + ' | after:' + k(e.target) + '/' + k(e.relatedTarget); })()",
        "span#out:span#out/div#h doc:span#out/div#h | after:span#out/div#h",
    );
    // At the host with a related target inside its tree: the retargeted
    // related target is the target, so nothing is invoked.
    check(
        "(function () { var seen = []; \
         h.addEventListener('mouseover', function (e) { seen.push('h'); }); \
         document.addEventListener('mouseover', function (e) { seen.push('doc'); }); \
         h.dispatchEvent(new MouseEvent('mouseover', {bubbles: true, composed: true, relatedTarget: em})); \
         return seen.join(' ') + '.'; })()",
        ".",
    );
    // An element of the same tree as the related target: the path goes on
    // up to the host and ends below the host that the related target
    // retargets to.
    check(
        "(function () { var seen = []; \
         [em, p, inner, h, document].forEach(function (n) { n.addEventListener('mouseover', function (e) { \
           seen.push(k(e.currentTarget) + ':' + k(e.target) + '/' + k(e.relatedTarget)); }); }); \
         var e = new MouseEvent('mouseover', {bubbles: true, composed: true, relatedTarget: sl}); \
         em.dispatchEvent(e); \
         return seen.join(' ') + ' | after:' + k(e.target) + '/' + k(e.relatedTarget); })()",
        "em#em:em#em/slot#sl p#p:p#p/slot#sl div#in:p#p/slot#sl | after:null/null",
    );
}

/// `FocusEvent` goes through the same retargeting of `relatedTarget`.
#[test]
fn focus_events_retarget_the_related_target() {
    check(
        "(function () { var seen = []; \
         [em, h, document].forEach(function (n) { n.addEventListener('focusin', function (e) { \
           seen.push(k(e.currentTarget) + ':' + k(e.target) + '/' + k(e.relatedTarget)); }); }); \
         em.dispatchEvent(new FocusEvent('focusin', {bubbles: true, composed: true, relatedTarget: u})); \
         return seen.join(' '); })()",
        "em#em:em#em/i#u div#h:div#h/i#u doc:div#h/i#u",
    );
}

/// An event with a listener that stops propagation in a shadow tree: the
/// outer listeners get nothing, and the targets are cleared as usual.
#[test]
fn stop_propagation_in_a_tree_and_clear_targets() {
    check(
        "(function () { L.length = 0; var stop = function (e) { e.stopPropagation(); }; \
         inner.addEventListener('x', stop); \
         var e = new Event('x', {bubbles: true, composed: true}); \
         em.dispatchEvent(e); \
         return L.filter(function (s) { return s.indexOf('doc:') == 0 || s.indexOf('win:') == 0; }).length \
           + ' ' + k(e.target); })()",
        "0 div#h",
    );
}

/// A path through 200 nested trees: every root's listener runs, the
/// document's listener sees the outermost host, and the path holds all the
/// nodes; the wrappers of the structs live through collections.
#[test]
fn a_deep_chain_of_trees() {
    for stress in [false, true] {
        on_big_stack(move || {
            let mut rt = load(PAGE, stress);
            let got = answer(
                &mut rt,
                "(function () { var top = document.createElement('div'); \
                 document.body.appendChild(top); var cur = top, count = 0, bottom; \
                 for (var i = 0; i < 200; i++) { var r = cur.attachShadow({mode: 'open'}); \
                   r.addEventListener('z', function () { count++; }); \
                   var d = document.createElement('div'); r.appendChild(d); cur = d; } \
                 var seen; \
                 document.addEventListener('z', function (e) { \
                   seen = [e.target === top, e.composedPath().length]; }); \
                 cur.dispatchEvent(new Event('z', {bubbles: true, composed: true})); \
                 return [count].concat(seen); })()",
            );
            assert_eq!(got, expected("[200, true, 405]"), "stress {stress}");
        });
    }
}
