//! Tests of the counter pass: the text of `::before`, `::after` and
//! `::marker` for counters, `list-item` and list item numbers.
//!
//! The expected text comes from Chromium 148: the cases were rendered
//! with the same CSS and HTML, and the text of each pseudo-element was
//! read from the layout tree (CDP `DOMSnapshot.captureSnapshot`). Each
//! line is `owner::kind 'text'`, where `owner` is the element's id or its
//! upper-case tag name, in tree order (for each element: `::before`,
//! `::after`, `::marker`).

use std::fmt::Write as _;

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId, parse_html};
use url::Url;

use crate::style_map::{PseudoKind, StyleMap};
use crate::{ElementStates, Stylist, compute_styles, content_text};

fn render(css: &str, html: &str) -> (Document, StyleMap) {
    let base = Url::parse("https://example.com/").expect("valid URL");
    let doc = parse_html(&format!("<!DOCTYPE html>{html}"));
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base);
    let env = MediaEnvironment::default();
    let map = compute_styles(&doc, &stylist, &env, &ElementStates::default(), &base);
    (doc, map)
}

/// The generated text of all pseudo-elements, one line each.
fn generated(css: &str, html: &str) -> String {
    let (doc, map) = render(css, html);
    let mut out = String::new();
    let root = doc.document_element().expect("a root element");
    for node in std::iter::once(root).chain(doc.descendants(root)) {
        let (Some(element), Some(style)) = (doc.element(node), map.get(node)) else {
            continue;
        };
        let owner = element.id().map_or_else(
            || element.local_name().to_string().to_ascii_uppercase(),
            str::to_owned,
        );
        for (kind, name) in [(PseudoKind::Before, "before"), (PseudoKind::After, "after")] {
            if let Some(text) = map.pseudo(node, kind).and_then(|s| content_text(s)) {
                let _ = writeln!(out, "{owner}::{name} '{text}'");
            }
        }
        if let Some(text) = map.list_marker_text(node, style) {
            let _ = writeln!(out, "{owner}::marker '{text}'");
        }
    }
    out
}

fn check(css: &str, html: &str, expected: &str) {
    let actual = generated(css, html);
    let mut lines = String::new();
    for line in expected.lines().map(str::trim).filter(|l| !l.is_empty()) {
        lines.push_str(line);
        lines.push('\n');
    }
    assert_eq!(actual, lines);
}

/// Markers of nested lists, `start`, `reversed` and `value`, and
/// `counters()` in `::before`.
#[test]
fn nested_lists_and_counters() {
    check(
        r#"
.n ol { counter-reset: item }
.n li { display: block }
.n li::before { content: counters(item, ".") " "; counter-increment: item }
"#,
        r"
<ol id=a><li id=a1>x<li id=a2>y<ol><li id=a21>z<li id=a22>w</ol><li id=a3>v</ol>
<ol id=b start=5 reversed><li id=b1>x<li id=b2>y<li id=b3 value=10>z<li id=b4>w</ol>
<ol id=c reversed><li id=c1>x<li id=c2>y<li id=c3>z</ol>
<div class=n><ol><li id=n1>a<li id=n2>b<ol><li id=n21>c</ol><li id=n3>d</ol></div>
",
        r"
a1::marker '1. '
a2::marker '2. '
a21::marker '1. '
a22::marker '2. '
a3::marker '3. '
b1::marker '5. '
b2::marker '4. '
b3::marker '10. '
b4::marker '9. '
c1::marker '3. '
c2::marker '2. '
c3::marker '1. '
n1::before '1 '
n2::before '2 '
n21::before '2.1 '
n3::before '3 '
",
    );
}

/// Reset, then increment, then set; repeated names; several counters.
#[test]
fn properties_of_one_element() {
    check(
        r#"
.v::before { content: counter(c) }
#r1 { counter-reset: c 5; counter-increment: c 2; counter-set: c 1 }
#r2 { counter-reset: c; counter-increment: c 2 c 3 }
#r3 { counter-reset: c 1 c 2 }
#r4 { counter-reset: c; counter-set: c 1 c 2 }
#r5 { counter-reset: c 7 d 8 }
#r5::before { content: counter(c) "," counter(d) }
#r6 { counter-reset: C 3 c 4 }
#r6::before { content: counter(C) counter(c) }
#r7 { counter-increment: c 0 }
#r7::before { content: counters( c , '.' ) }
"#,
        r"
<section><div id=r1 class=v></div></section>
<section><div id=r2 class=v></div></section>
<section><div id=r3 class=v></div></section>
<section><div id=r4 class=v></div></section>
<section><div id=r5></div></section>
<section><div id=r6></div></section>
<section><div id=r7></div></section>
",
        r"
r1::before '1'
r2::before '5'
r3::before '2'
r4::before '2'
r5::before '7,8'
r6::before '34'
r7::before '0'
",
    );
}

/// Siblings inherit counters; a reset replaces the counter of a previous
/// sibling; a reset inside the scope of an ancestor's counter is nested and
/// ends with its element (Chromium).
#[test]
fn scopes() {
    check(
        r#"
.v::before { content: counter(c) }
.vs::before { content: counters(c, ".") }
.r1 { counter-reset: c 1 }
.r2 { counter-reset: c 2 }
.r3 { counter-reset: c 3 }
.inc { counter-increment: c }
.sib0 { counter-reset: s }
.sib { counter-increment: s }
.sib::before { content: counter(s) }
"#,
        r#"
<section><h1 class=sib0></h1><h2 id=s1 class=sib></h2><h2 id=s2 class=sib></h2><div><h3 id=s3 class=sib></h3></div><h2 id=s4 class=sib></h2></section>
<section><div class=r1><i class=r2></i><i id=e1 class=vs></i></div></section>
<section><div><i class=r2></i><i id=e2 class=vs></i></div></section>
<section><div class=r1><div><i class=r2></i><i id=e3 class=vs></i></div></div></section>
<section><div class=r1></div><div><i class=r2></i><i id=e5 class=vs></i></div></section>
<section><div class=r1><i class=r2></i><i class=r3></i><i id=e7 class=vs></i></div></section>
<section><div class=r1><i class=r2></i><i class=inc></i><i id=e8 class=vs></i></div></section>
<section><div class=r1><i class="r2 vs" id=e9><b class="vs" id=e9b></b></i><i id=e9c class=vs></i></div></section>
<section><i class=r1></i><i class=r2></i><i id=e10 class=vs></i></section>
<section><i class=r1></i><b><i class=r2></i><i id=e11 class=vs></i></b><i id=e11b class=vs></i></section>
<section><i class=r1></i><b><i class=r2></i><i class=inc></i></b><i id=e12 class=vs></i></section>
<section><div class=r1><i class=inc></i><i class=r2></i><i id=f4 class=vs></i></div></section>
<section><div><i class=inc></i><i class=r2></i><i id=f5 class=vs></i></div></section>
<section><div class=r1><i class=r2><b class=inc></b></i><i id=f6 class=vs></i></div></section>
<section><div class=r1><i class=r2><b class=r1></b><b id=f7 class=vs></b></i></div></section>
<section><div class=r1><i class=r2></i><i id=f8 class="r2 vs"></i><i id=f8b class=vs></i></div></section>
<section><div><i class=r1></i></div><i id=f9 class=vs></i></section>
<section><b id=f10 class=r2></b><b id=f11 class=vs></b></section>
<section class=r1><b id=f12></b><b id=f13 class=vs></b></section>
"#,
        r"
s1::before '1'
s2::before '2'
s3::before '3'
s4::before '4'
e1::before '1'
e2::before '2'
e3::before '1'
e5::before '1'
e7::before '1'
e8::before '2'
e9::before '1.2'
e9b::before '1.2'
e9c::before '1'
e10::before '2'
e11::before '1'
e11b::before '1'
e12::before '2'
f4::before '2'
f5::before '2'
f6::before '1'
f7::before '1.2'
f8::before '1.2'
f8b::before '1'
f9::before '0'
f11::before '2'
f13::before '1'
",
    );
}

/// The heading example of CSS Lists 3 section 4.7: Chromium shows B.3, not
/// B.1, because a reset of h2 on an h1 is nested in the h2 counter of the
/// body.
#[test]
fn headings() {
    check(
        r"
#hd { counter-reset: h1 h2 h3; }
#hd h1   { counter-increment: h1; counter-reset: h2 h3;}
#hd h2   { counter-increment: h2; counter-reset:    h3; }
#hd h3   { counter-increment: h3; }
#hd h1::before { content: counter(h1,upper-alpha) ' '; }
#hd h2::before { content: counter(h1,upper-alpha) '.' counter(h2,decimal) ' '; }
#hd h3::before { content: counter(h1,upper-alpha) '.' counter(h2,decimal) '.' counter(h3,lower-roman) ' '; }
",
        r"
<div id=hd>
<h1 id=h1a>First H1</h1>
<h2 id=h2a>First H2 in H1</h2>
<h2 id=h2b>Second H2 in H1</h2>
<h3 id=h3a>First H3 in H2</h3>
<h1 id=h1b>Second H1</h1>
<h2 id=h2c>First H2 in H1</h2>
</div>
",
        r"
h1a::before 'A '
h2a::before 'A.1 '
h2b::before 'A.2 '
h3a::before 'A.2.i '
h1b::before 'B '
h2c::before 'B.3 '
",
    );
}

/// `display: none` elements and pseudo-elements without content do not
/// change counters; `display: contents` elements do not either, but their
/// children and pseudo-elements do; `visibility: hidden` elements do.
#[test]
fn boxless_elements() {
    check(
        r#"
.v::before { content: counter(c) }
.vs::before { content: counters(c, ".") }
.hid { counter-increment: c; display: none }
.cont { counter-increment: c; display: contents }
.cont::before { content: "[" counter(c) "]" }
.vh { counter-increment: c; visibility: hidden }
.inc { counter-increment: c }
#pe { counter-reset: c 4 }
#pe::before { content: counter(c); counter-increment: c 3; display: none }
#pe2 { counter-reset: c 4 }
#pe2::before { counter-increment: c 3 }
#pe3 { counter-reset: c 4 }
#pe3::before { counter-increment: c 3; content: none }
#pe4 { counter-reset: c 4 }
#pe4::before { counter-increment: c 3; content: "" }
"#,
        r#"
<section style="counter-reset: c"><i class=hid></i><i id=hid1 class=v></i><i class=cont id=cont0></i><i id=cont1 class=v></i><i class=vh></i><i id=vh1 class=v></i></section>
<section style="counter-reset: c"><b class=cont id=cont2><i class=cont id=cont3></i></b><i id=cont4 class=v></i></section>
<section><i style="counter-reset: c 5; display: contents"></i><i id=e14 class=vs></i></section>
<section style="counter-reset: c"><i style="counter-set: c 5; display: contents"></i><i id=e15 class=vs></i></section>
<section style="counter-reset: c"><i style="display: contents"><b class=inc></b></i><i id=e16 class=vs></i></section>
<section style="counter-reset: c"><i style="display: contents" class="inc"><b class=inc></b></i><i id=e17 class=vs></i></section>
<section><span style="display: contents"><i class=inc></i><i id=e18 class=v></i></span><i id=e19 class=v></i></section>
<section><div id=pe></div><div id=pe1 class=v></div></section>
<section><div id=pe2></div><div id=pe2b class=v></div></section>
<section><div id=pe3></div><div id=pe3b class=v></div></section>
<section><div id=pe4></div><div id=pe4b class=v></div></section>
<section style="counter-reset: c 1"><img class=inc><select class=inc><option class=inc>o</option></select><i id=rep class=v></i></section>
"#,
        r"
hid1::before '0'
cont0::before '[0]'
cont1::before '0'
vh1::before '1'
cont2::before '[0]'
cont3::before '[0]'
cont4::before '0'
e14::before '0'
e15::before '0'
e16::before '1'
e17::before '1'
e18::before '2'
e19::before '2'
pe1::before '2'
pe2b::before '2'
pe3b::before '2'
pe4::before ''
pe4b::before '2'
rep::before '3'
",
    );
}

/// `::marker`, `::before`, the children, `::after`; `::marker` ignores
/// counter properties; `counter()` does not create a counter.
#[test]
fn pseudo_element_order() {
    check(
        r#"
.v::before { content: counter(c) }
#po { counter-reset: q; display: list-item; list-style-position: inside }
#po::marker { content: "m" counter(q) " "; counter-increment: q 1 }
#po::before { content: "b" counter(q); counter-increment: q 10 }
#po::after { content: "a" counter(q); counter-increment: q 100 }
#po2 { counter-reset: q }
#po2::before { content: "b" counter(q) }
#po2::after { content: "a" counter(q) }
#po2 > span { counter-increment: q 5 }
#u1::before { counter-increment: zz; content: counter(zz) }
#u2::before { content: counter(zz) }
#ins::before { content: counter(c) }
#ins > i > u { counter-reset: c 5 }
#pb::before { counter-reset: c 7; content: counter(c) }
#pb::after { content: counter(c) }
"#,
        r"
<ul><li id=po>x</li></ul>
<div id=po2><span></span>y<span></span></div>
<div><p id=u1></p><p id=u2></p></div>
<section><div id=ins><i><u></u><b id=bb class=v></b></i></div></section>
<section><div id=pb><i id=pbi class=v></i></div></section>
",
        r"
po::before 'b10'
po::after 'a110'
po::marker 'm0 '
po2::before 'b0'
po2::after 'a10'
u1::before '1'
u2::before '0'
ins::before '0'
bb::before '5'
pb::before '7'
pb::after '7'
pbi::before '7'
",
    );
}

/// Reset and set values and the combined value of one element saturate; an
/// increment that would overflow is ignored.
#[test]
fn overflow() {
    check(
        r"
.v::before { content: counter(c) }
#ov1 { counter-reset: c 2147483647; counter-increment: c }
#ov2 { counter-reset: c 99999999999 }
#ov3 { counter-reset: c -2147483648; counter-increment: c -1 }
#ov4 { counter-reset: c 2147483000; counter-increment: c 1000 }
#ov5 { counter-reset: c 2147483647 }
#ov5 > i { counter-increment: c }
#o1 { counter-reset: c 2147483000 }
#o1 > i { counter-increment: c 1000 }
#o2 { counter-reset: c -2147483000 }
#o2 > i { counter-increment: c -1000 }
#o3 { counter-reset: c 5 }
#o3 > i { counter-increment: c 2147483647 c 10 }
#o4 { counter-increment: c 2147483647 c 10 }
#c1 { counter-reset: c calc(1.5) }
#c2 { counter-reset: c calc(-1.5) }
#c3 { counter-reset: c calc(1e10) }
#c4 { counter-reset: c 1.5 }
#c5 { counter-reset: c 1e3 }
",
        r"
<section><div id=ov1 class=v></div></section>
<section><div id=ov2 class=v></div></section>
<section><div id=ov3 class=v></div></section>
<section><div id=ov4 class=v></div></section>
<section><div id=ov5><i></i><i></i><i id=ov5c class=v></i></div></section>
<section><div id=o1><i></i><i></i><i id=o1c class=v></i></div></section>
<section><div id=o2><i></i><i></i><i id=o2c class=v></i></div></section>
<section><div id=o3><i id=o3c class=v></i></div></section>
<section><div id=o4 class=v></div></section>
<section><div id=c1 class=v></div></section>
<section><div id=c2 class=v></div></section>
<section><div id=c3 class=v></div></section>
<section><div id=c4 class=v></div></section>
<section><div id=c5 class=v></div></section>
",
        r"
ov1::before '2147483647'
ov2::before '2147483647'
ov3::before '-2147483648'
ov4::before '2147483647'
ov5c::before '2147483647'
o1c::before '2147483000'
o2c::before '-2147483000'
o3c::before '5'
o4::before '2147483647'
c1::before '2'
c2::before '-1'
c3::before '2147483647'
c4::before '0'
c5::before '0'
",
    );
}

/// Counter styles in `counter()` and `counters()`; unknown names are
/// decimal.
#[test]
fn counter_styles() {
    check(
        r#"
#st { counter-reset: c -5 }
#st::before { content: counter(c, decimal) "|" counter(c, lower-roman) "|" counter(c, disc) "|" counter(c, circle) "|" counter(c, square) "|" counter(c, none) "|" counter(c, disclosure-open) "|" counter(c, disclosure-closed) "|" counter(c, decimal-leading-zero) "|" counter(c, lower-greek) "|" counter(c, upper-alpha) "|" counter(c, upper-latin) "|" counter(c, lower-latin) "|" counter(c, foo) }
#st2 { counter-reset: c 7 }
#st2::before { content: counter(c, decimal) "|" counter(c, lower-roman) "|" counter(c, disc) "|" counter(c, circle) "|" counter(c, square) "|" counter(c, none) "|" counter(c, disclosure-open) "|" counter(c, disclosure-closed) "|" counter(c, decimal-leading-zero) "|" counter(c, lower-greek) "|" counter(c, upper-alpha) "|" counters(c, "-", upper-roman) }
#st3 { counter-reset: c 3999 }
#st3 > i { counter-reset: c 28 }
#st3 > i > b { counter-reset: c 0 }
#st3 > i > b::before { content: counters(c, ".", upper-roman) " " counters(c, "/", lower-alpha) " " counters(c, "", lower-greek) }
#st4::before { content: counter(zz, upper-roman) "|" counters(zz, ".", lower-alpha) }
"#,
        r"
<section><div id=st></div></section>
<section><div id=st2></div></section>
<section><div id=st3><i><b id=st3b></b></i></div></section>
<section><div id=st4></div></section>
",
        r"
st::before '-5|-5|•|◦|■||▾|▸|-5|-5|-5|-5|-5|-5'
st2::before '7|vii|•|◦|■||▾|▸|07|η|G|VII'
st3b::before 'MMMCMXCIX.XXVIII.0 ewu/ab/0 ζχοαδ0'
st4::before '0|0'
",
    );
}

/// The `list-item` counter in `counter()`: lists reset it, `li` increments
/// it; author counter properties that name it replace the implicit ones;
/// markers use the ordinal value instead.
#[test]
fn list_item_counter() {
    check(
        r#"
.b::before { content: counter(list-item) }
.bs::before { content: counters(list-item, ".") }
.none > li { counter-increment: none }
.two > li { counter-increment: list-item 2 }
.zero > li { counter-increment: list-item 0 }
.foo { counter-reset: foo }
.r5 { counter-reset: list-item 5 }
.r10 { counter-reset: list-item 10 }
.i10 { counter-increment: list-item 10 }
.set > li:nth-child(2) { counter-set: list-item 7 }
.dli { display: list-item; list-style-position: inside }
.p5 > li::before { counter-increment: list-item 5; content: counter(list-item) }
.m > li::marker { content: counter(list-item) "|" counters(list-item, ".") " " }
"#,
        r#"
<section><ol id=o1><li id=o1a class=b>a<li id=o1b class=b>b</ol></section>
<section><ol class=none><li id=n1 class=b>a<li id=n2>b</ol></section>
<section><ol class=two><li id=t1 class=b>a<li id=t2>b</ol></section>
<section><ol class=zero><li id=z1 class=b>a<li id=z2>b</ol></section>
<section><ol id=outer><li id=fo1>a<ol class=foo><li id=fo2 class=bs>b<li id=fo3>c</ol><li id=fo4>d</ol></section>
<section><ol class=r5><li id=p1a class=b>a<li id=p1b>b</ol></section>
<section><ol><li id=p2a>a<li id=p2b class="r10 b">b<li id=p2c class=b>c</ol></section>
<section><ol><li id=p4a>a<div class=i10></div><li id=p4b class=b>b</ol></section>
<section><ol class=p5><li id=p5a>a<li id=p5b>b</ol></section>
<section><ol><li id=q1a class=b>a<div class=r10><span id=q1b class=bs></span></div><li id=q1c class=b>c</ol></section>
<section><ol><li id=q3a class=b>a<div class=r10><div id=q3b class="dli bs"></div></div><li id=q3c class=b>c</ol></section>
<section><ol start=5><li id=q10a class=bs>a<li id=q10b class=bs>b</ol></section>
<section><ol reversed start=5><li id=q11a class=bs>a<li id=q11b class=bs>b</ol></section>
<section><ol reversed><li id=q12a class=bs>a<li id=q12b class=bs>b<li id=q12c class=bs>c</ol></section>
<section><ol><li id=q13a class=bs value=7>a<li id=q13b class=bs>b</ol></section>
"#,
        r"
o1a::before '1'
o1a::marker '1. '
o1b::before '2'
o1b::marker '2. '
n1::before '1'
n1::marker '1. '
n2::marker '2. '
t1::before '2'
t1::marker '2. '
t2::marker '4. '
z1::before '0'
z1::marker '0. '
z2::marker '0. '
fo1::marker '1. '
fo2::before '1.1'
fo2::marker '1. '
fo3::marker '2. '
fo4::marker '2. '
p1a::before '6'
p1a::marker '1. '
p1b::marker '2. '
p2a::marker '1. '
p2b::before '10'
p2b::marker '2. '
p2c::before '2'
p2c::marker '3. '
p4a::marker '1. '
p4b::before '12'
p4b::marker '2. '
p5a::before '6'
p5a::marker '1. '
p5b::before '12'
p5b::marker '2. '
q1a::before '1'
q1a::marker '1. '
q1b::before '1.10'
q1c::before '2'
q1c::marker '2. '
q3a::before '1'
q3a::marker '1. '
q3b::before '1.10'
q3b::marker '2. '
q3c::before '2'
q3c::marker '3. '
q10a::before '5'
q10a::marker '5. '
q10b::before '6'
q10b::marker '6. '
q11a::before '5'
q11a::marker '5. '
q11b::before '4'
q11b::marker '4. '
q12a::before '0'
q12a::marker '3. '
q12b::before '-1'
q12b::marker '2. '
q12c::before '-2'
q12c::marker '1. '
q13a::before '1'
q13a::marker '7. '
q13b::before '2'
q13b::marker '8. '
",
    );
}

/// The `list-item` counter with `menu`, `dir`, `ul`, `counter-set`, block
/// `li`, markers with counters, `li` outside lists, `summary` and a
/// `display: contents` list.
#[test]
fn list_item_counter_in_other_lists() {
    check(
        r#"
.b::before { content: counter(list-item) }
.bs::before { content: counters(list-item, ".") }
.none > li { counter-increment: none }
.two > li { counter-increment: list-item 2 }
.zero > li { counter-increment: list-item 0 }
.foo { counter-reset: foo }
.r5 { counter-reset: list-item 5 }
.r10 { counter-reset: list-item 10 }
.i10 { counter-increment: list-item 10 }
.set > li:nth-child(2) { counter-set: list-item 7 }
.dli { display: list-item; list-style-position: inside }
.p5 > li::before { counter-increment: list-item 5; content: counter(list-item) }
.m > li::marker { content: counter(list-item) "|" counters(list-item, ".") " " }
"#,
        r#"
<section><ol><li id=q14>a<menu style="list-style-type: decimal"><li id=q14a class=bs>b</menu><li id=q14b class=bs>c</ol></section>
<section><ol><li id=q15>a<dir style="list-style-type: decimal"><li id=q15a class=bs>b</dir><li id=q15b class=bs>c</ol></section>
<section><ul class=foo><li id=q16a class=bs>a<li id=q16b class=bs>b</ul></section>
<section><ol class=set><li id=st1>a<li id=st2 class=b>b<li id=st3>c</ol></section>
<section><ol><li id=k1 style="display:block" class=b>a<li id=k2 class=b>b</ol></section>
<section><ol><li id=k3 class=b style="counter-increment: list-item 4; display: block">a<li id=k4 class=b>b</ol></section>
<section><ol class=m><li id=m1>a<ol class=m><li id=m2>b<ul class=m><li id=m3>c</ul></ol></ol></section>
<section><ol class=m reversed><li id=m4>a<li id=m5>b</ol></section>
<section><div><li id=g1 class=b>a</li></div><div><li id=g2 class=b>b</li></div></section>
<section><ol><li id=h1 class=b>a</ol><li id=h2 class=b>b</li><p id=h3 class=b></p></section>
<section><div class=dli id=j1></div><div class=dli id=j2></div><p id=j3 class=b></p></section>
<section><details open><summary id=sm1 class=b>s</summary><ol><li id=sm2>x</ol></details></section>
<section><ol><li id=sm3>a<details><summary id=sm4 class=b>s</summary></details><li id=sm5>b</ol></section>
<section><ol style="display: contents" start=3><li id=dc1 class=b>a<li id=dc2>b</ol></section>
"#,
        r"
q14::marker '1. '
q14a::before '1.1'
q14a::marker '1. '
q14b::before '2'
q14b::marker '2. '
q15::marker '1. '
q15a::before '1.1'
q15a::marker '2. '
q15b::before '2'
q15b::marker '3. '
q16a::before '1'
q16a::marker '• '
q16b::before '2'
q16b::marker '• '
st1::marker '1. '
st2::before '7'
st2::marker '7. '
st3::marker '8. '
k1::before '0'
k2::before '1'
k2::marker '1. '
k3::before '4'
k4::before '5'
k4::marker '1. '
m1::marker '1|1 '
m2::marker '1|1.1 '
m3::marker '1|1.1.1 '
m4::marker '0|0 '
m5::marker '-1|-1 '
g1::before '1'
g1::marker '• '
g2::before '1'
g2::marker '• '
h1::before '1'
h1::marker '1. '
h2::before '2'
h2::marker '• '
h3::before '2'
j1::marker '• '
j2::marker '• '
j3::before '0'
sm1::before '0'
sm1::marker '▾ '
sm2::marker '1. '
sm3::marker '1. '
sm4::before '1'
sm4::marker '▸ '
sm5::marker '2. '
dc1::before '1'
dc1::marker '3. '
dc2::marker '4. '
",
    );
}

/// Marker numbers (the ordinal values): `start`, `reversed` and `value`,
/// and how a reversed list counts its items.
#[test]
fn ordinal_values() {
    check(
        r#"
.two > li { counter-increment: list-item 2 }
.set > li:nth-child(2) { counter-set: list-item 7 }
.i10 { counter-increment: list-item 10 }
.dli { display: list-item; list-style-position: inside }
.p15 > li { counter-increment: list-item 3 }
.lsn > li { list-style-type: none }
.lsn > li::marker { content: "x" }
#mk > li::marker { content: none }
#mk3 > li::marker { content: "" }
"#,
        r#"
<ol reversed><li id=rv1>a<li id=rv2 value=10>b<li id=rv3>c</ol>
<ol reversed><li id=rw1>a<li id=rw2 style="display:none">b<li id=rw3>c<div><li id=rw4>d</div><div class=dli id=rw5>e</div><p id=rw6>f</p></ol>
<ol reversed><li id=rx1>a<ol reversed><li id=rx2>b<li id=rx3>c</ol><li id=rx4>d</ol>
<ol reversed class=two><li id=ry1>a<li id=ry2>b<li id=ry3>c</ol>
<ol reversed start=-2><li id=rz1>a<li id=rz2>b</ol>
<ol start=2147483647><li id=sb1>a<li id=sb2>b</ol>
<ol start=-2147483648 reversed><li id=sc1>a<li id=sc2>b</ol>
<ol start=99999999999><li id=sd1>a<li id=sd2>b</ol>
<ol start=" 3x"><li id=se1>a</ol>
<ol start="+4"><li id=sf1>a</ol>
<ol><li value="-3x" id=sg1>a<li value="  12" id=sg2>b<li value="x" id=sg3>c</ol>
"#,
        r"
rv1::marker '3. '
rv2::marker '10. '
rv3::marker '9. '
rw1::marker '4. '
rw3::marker '3. '
rw4::marker '2. '
rw5::marker '1. '
rx1::marker '2. '
rx2::marker '2. '
rx3::marker '1. '
rx4::marker '1. '
ry1::marker '6. '
ry2::marker '8. '
ry3::marker '10. '
rz1::marker '-2. '
rz2::marker '-3. '
sb1::marker '2147483647. '
sb2::marker '2147483647. '
sc1::marker '-2147483648. '
sc2::marker '-2147483648. '
sd1::marker '1. '
sd2::marker '2. '
se1::marker '3. '
sf1::marker '4. '
sg1::marker '-3. '
sg2::marker '12. '
sg3::marker '13. '
",
    );
}

/// Marker numbers set by the items: `counter-set` and `counter-increment`
/// of `list-item`, non-`li` list items, `menu` and `dir`, `content` of
/// markers, counter styles.
#[test]
fn ordinal_values_of_items() {
    check(
        r#"
.two > li { counter-increment: list-item 2 }
.set > li:nth-child(2) { counter-set: list-item 7 }
.i10 { counter-increment: list-item 10 }
.dli { display: list-item; list-style-position: inside }
.p15 > li { counter-increment: list-item 3 }
.lsn > li { list-style-type: none }
.lsn > li::marker { content: "x" }
#mk > li::marker { content: none }
#mk3 > li::marker { content: "" }
"#,
        r#"
<ol class=set><li id=st1>a<li id=st2>b<li id=st3>c</ol>
<ol reversed class=set><li id=su1>a<li id=su2>b<li id=su3>c</ol>
<ol reversed><li id=p22a>a<li id=p22b class=i10>b<li id=p22c>c</ol>
<ol><li id=p21a>a<li id=p21b style="counter-increment: list-item 5 list-item 2">b<li id=p21c style="counter-increment: list-item 0; counter-set: list-item 3">c<li id=p21d style="counter-reset: list-item 10; counter-increment: list-item 1">d</ol>
<ol class=p15><li id=p15a value=5>a<li id=p15b>b</ol>
<ol><li id=p12a>a<div class=dli id=p12b>b</div><li id=p12c>c</ol>
<ol><li id=q14>a<menu style="list-style-type: decimal"><li id=q14a>b</menu><li id=q14b>c</ol>
<ol><li id=q15>a<dir style="list-style-type: decimal"><li id=q15a>b</dir><li id=q15b>c</ol>
<ul class=lsn><li id=l1>a</ul>
<ol id=mk><li id=mk1>a<li id=mk2>b</ol>
<ol id=mk3><li id=mk3a>a</ol>
<ol style="list-style-type: lower-alpha"><li id=ty1 value=0>a<li id=ty2 value=-1>b<li id=ty3 value=27>c</ol>
<ol style="list-style-type: upper-roman"><li id=ty4 value=0>a<li id=ty5 value=3999>b<li id=ty6 value=4000>c<li id=ty7 value=-4>c</ol>
<ol style="list-style-type: decimal-leading-zero"><li id=ty8 value=-5>a<li id=ty9 value=0>b<li id=ty10 value=123>c</ol>
<ol style="list-style-type: lower-greek"><li id=ty11 value=0>a<li id=ty12 value=25>b</ol>
<ol style="list-style-type: square"><li id=ty13>a</ol>
<ol style="display: contents" reversed><li id=dc1>a<li id=dc2>b</ol>
<div><li id=g1>a<li id=g2 style="list-style-type: decimal">b</div>
"#,
        r"
st1::marker '1. '
st2::marker '7. '
st3::marker '8. '
su1::marker '3. '
su2::marker '7. '
su3::marker '6. '
p22a::marker '3. '
p22b::marker '13. '
p22c::marker '12. '
p21a::marker '1. '
p21b::marker '8. '
p21c::marker '3. '
p21d::marker '14. '
p15a::marker '5. '
p15b::marker '8. '
p12a::marker '1. '
p12b::marker '2. '
p12c::marker '3. '
q14::marker '1. '
q14a::marker '1. '
q14b::marker '2. '
q15::marker '1. '
q15a::marker '2. '
q15b::marker '3. '
l1::marker 'x'
mk3a::marker ''
ty1::marker '0. '
ty2::marker '-1. '
ty3::marker 'aa. '
ty4::marker '0. '
ty5::marker 'MMMCMXCIX. '
ty6::marker '4000. '
ty7::marker '-4. '
ty8::marker '-5. '
ty9::marker '00. '
ty10::marker '123. '
ty11::marker '0. '
ty12::marker 'αα. '
ty13::marker '■ '
dc1::marker '2. '
dc2::marker '1. '
g1::marker '• '
g2::marker '2. '
",
    );
}

/// The references list of the Wikipedia fixture: the backlink letters a, b,
/// c, ... (`mw-ref-linkback`) and the sub-reference numbers.
#[test]
fn wikipedia_references() {
    check(
        r#"
/* The sheet of the Cite extension, then the site sheet, in page order. */
span[rel='mw:referencedBy'] > a::before{content:counter(mw-references,var(--cite-counter-style)) var(--cite-backlink-separator) counter(mw-ref-linkback,var(--cite-counter-style))}
:root{--cite-backlink-separator:'.';--cite-counter-style:decimal}
ol.mw-references{counter-reset:mw-ref-details-parent mw-references list-item}
ol.mw-references > li{counter-increment:mw-ref-details-parent mw-references}
[rel~='mw:referencedBy']::before{content:'↑ '}
span[rel~='mw:referencedBy']{counter-reset:mw-ref-linkback -1}
span[rel~='mw:referencedBy'] a::before{counter-increment:mw-ref-linkback;line-height:1;vertical-align:super;font-size:smaller}
span[rel~='mw:referencedBy'] a::after{content:' ';line-height:1}
span[rel~='mw:referencedBy'] a:last-child::after{content:''}
span.mw-linkback-text{display:none}
ol.references{counter-reset:mw-ref-details-parent mw-references list-item}
ol.references > li{counter-increment:mw-ref-details-parent mw-references;counter-reset:mw-ref-details-child}
ol.references .mw-subreference-list > li{counter-increment:mw-ref-details-child}
ol.references .mw-subreference-list > li::marker{content:counter(mw-ref-details-parent,decimal) '.' counter(mw-ref-details-child,decimal) '. '}
span[rel="mw:referencedBy"]{counter-reset:mw-ref-linkback 0}
span[rel='mw:referencedBy'] > a::before{content:counter(mw-ref-linkback,lower-alpha);font-size:80%;font-weight:bold;font-style:italic}
a[rel="mw:referencedBy"]::before{font-weight:bold;content:"^"}
span[rel="mw:referencedBy"]::before{content:"^ "}
"#,
        r##"
<div class="mw-references-wrap"><ol class="mw-references references">
<li id="n1"><a href="#r1" rel="mw:referencedBy" id=up1><span class="mw-linkback-text">↑ </span></a> <span class="mw-reference-text">One.</span></li>
<li id="n2"><span rel="mw:referencedBy" class="mw-cite-backlink" id=bl2><a href="#r2a" id=a2a><span class="mw-linkback-text">1</span></a> <a href="#r2b" id=a2b><span class="mw-linkback-text">2</span></a> <a href="#r2c" id=a2c><span class="mw-linkback-text">3</span></a></span> <span class="mw-reference-text">Two.</span></li>
<li id="n3"><span rel="mw:referencedBy" class="mw-cite-backlink" id=bl3><a href="#r3a" id=a3a><span class="mw-linkback-text">1</span></a> <a href="#r3b" id=a3b><span class="mw-linkback-text">2</span></a></span> <span class="mw-reference-text">Three.</span>
<ol class="mw-subreference-list"><li id=n3a>Sub one.</li><li id=n3b>Sub two.</li></ol></li>
</ol></div>
"##,
        r"
n1::marker '1. '
up1::before '^'
n2::marker '2. '
bl2::before '^ '
a2a::before 'a'
a2a::after ' '
a2b::before 'b'
a2b::after ' '
a2c::before 'c'
a2c::after ''
n3::marker '3. '
bl3::before '^ '
a3a::before 'a'
a3a::after ' '
a3b::before 'b'
a3b::after ''
n3a::marker '3.1. '
n3b::marker '3.2. '
",
    );
}

/// Replaced elements and form controls with `display: list-item` are not
/// list items: they have no marker, take no number and do not count in
/// reversed lists.
#[test]
fn atomic_list_items() {
    check(
        r"
.li { display: list-item; }
",
        r"
<ol><li id=a1>a</li><img id=a2 class=li><li id=a3>b</li><input id=a4 class=li><button id=a5 class=li>x</button><select id=a6 class=li><option>o</option></select><li id=a7>c</li></ol>
<ol reversed><li id=b1>a</li><img class=li><button class=li>x</button><li id=b2>b</li></ol>
",
        r"
a1::marker '1. '
a3::marker '2. '
a7::marker '3. '
b1::marker '2. '
b2::marker '1. '
",
    );
}

/// A `::before` or `::after` with `display: contents` does not change
/// counters; one with `display: list-item` takes a number in its list (its
/// own marker is hidden here, because layout does not draw markers of
/// pseudo-elements).
#[test]
fn pseudo_elements_with_display_contents_or_list_item() {
    check(
        r#"
.v::before { content: counter(c) }
.x::before { content: "[" counter(c) "]"; display: contents; counter-increment: c 5 }
.y::after { content: "[" counter(c) "]"; display: contents; counter-reset: c 50 }
.p::before { content: "P"; display: list-item; list-style-type: none }
.q::after { content: "Q"; display: list-item; list-style-type: none }
"#,
        r#"
<section style="counter-reset: c"><div class=x id=x1></div><div class=v id=x2></div></section>
<section style="counter-reset: c 1"><div class=y id=y1></div><div class=v id=y2></div></section>
<ol><li id=p1>a<li id=p2 class=p>b<li id=p3>c</ol>
<ol reversed><li id=p4>a<li id=p5 class=q>b<li id=p6>c</ol>
<ol class=p><li id=p7>a</ol>
"#,
        r"
x1::before '[0]'
x2::before '0'
y1::after '[1]'
y2::before '1'
p1::marker '1. '
p2::before 'P'
p2::marker '2. '
p3::marker '4. '
p4::marker '4. '
p5::after 'Q'
p5::marker '3. '
p6::marker '1. '
OL::before 'P'
p7::marker '2. '
",
    );
}

/// A list with `display: contents` is not a list owner: its items belong to
/// the list owner around it, or else to their parent; a reversed one counts
/// all items inside it that are not inside a list owner.
#[test]
fn display_contents_lists() {
    check(
        r"
.c { display: contents }
",
        r"
<ol reversed class=c><li id=c1>x<ol reversed class=c><li id=c2>y<ol reversed class=c><li id=c3>z</ol></ol></ol>
<ol reversed><li id=d1>x<ol reversed class=c><li id=d2>y</ol><li id=d3>z</ol>
<ol start=5><li id=e1>x<ul class=c><li id=e2>y</ul><li id=e3>z</ol>
",
        r"
c1::marker '3. '
c2::marker '2. '
c3::marker '1. '
d1::marker '3. '
d2::marker '2. '
d3::marker '1. '
e1::marker '5. '
e2::marker '◦ '
e3::marker '7. '
",
    );
}

/// With `display: list-item`, `iframe`, `fieldset`, `progress`, `meter`,
/// `svg`, `embed` and `br` are not list items (like replaced elements and
/// form controls); `object` and `canvas` with fallback content, `details`,
/// `marquee`, `math` and `legend` are. The content of `progress` and
/// `meter` does not change counters; the content of `object`, `canvas`,
/// `fieldset` and `button` does.
#[test]
fn atomic_elements_are_not_list_items() {
    check(
        r#"
.li { display: list-item; }
.c::before { content: "C" counter(c); }
section { counter-reset: c; }
.inc > * { counter-increment: c; }
"#,
        r"
<ol><li>a<iframe class=li></iframe><fieldset class=li id=fieldset><legend>l</legend>f</fieldset><progress class=li>p</progress><meter class=li>m</meter><svg class=li width=10 height=10></svg><embed class=li><br class=li><li id=a2>b</ol>
<ol><li>a<object class=li id=object>fallback</object><canvas class=li id=canvas>fallback</canvas><details class=li id=details><summary>s</summary>d</details><marquee class=li id=marquee>m</marquee><math class=li id=math><mi>x</mi></math><legend class=li id=legend>l</legend><li id=b2>b</ol>
<ol reversed><li id=c1>a<iframe class=li></iframe><progress class=li></progress><object class=li id=c2>f</object><li id=c3>b</ol>
<section><progress class=inc><b id=pb></b></progress><meter class=inc><b id=mb></b></meter><object class=inc><b id=ob class=c></b></object><canvas class=inc><b id=cb class=c></b></canvas><fieldset class=inc><b id=fb class=c></b></fieldset><button class=inc><b id=bb class=c></b></button><i id=end class=c></i></section>
",
        r"
LI::marker '1. '
a2::marker '2. '
LI::marker '1. '
object::marker '2. '
canvas::marker '3. '
details::marker '4. '
SUMMARY::marker '▸ '
marquee::marker '5. '
math::marker '6. '
legend::marker '7. '
b2::marker '8. '
c1::marker '3. '
c2::marker '2. '
c3::marker '1. '
ob::before 'C1'
cb::before 'C2'
fb::before 'C3'
bb::before 'C4'
end::before 'C4'
",
    );
}

/// The options of a list-box `select` take part in counters; those of a
/// drop-down `select` and the child elements of an `option` do not.
/// Chromium takes the size from `size` (a 32-bit non-negative integer; 4
/// with `multiple` and 1 without when it is missing, invalid or 0), so
/// `multiple size=1` is a drop-down. `wbr` ignores its counter properties
/// and is not a list item.
#[test]
fn select_list_boxes() {
    check(
        r#"
section { counter-reset: c; }
.inc { counter-increment: c; }
.c::before { content: "C" counter(c); }
"#,
        r#"
<section><select size=3><option class=inc>a<option class="inc c" id=s1>b</select><i class=c id=e1></i></section>
<section><select multiple><option class=inc>a<option class="inc c" id=s2>b</select><i class=c id=e2></i></section>
<section><select size=1><option class=inc>a<option class=inc>b</select><i class=c id=e3></i></section>
<section><select><option class=inc>a<option class=inc>b</select><i class=c id=e4></i></section>
<section><select size=2><optgroup label=g class=inc><option class="inc c" id=s5>b</optgroup></select><i class=c id=e5></i></section>
<section><select size=0><option class=inc>a</select><i class=c id=e6></i></section>
<section><select size=" 4x"><option class=inc>a</select><i class=c id=e7></i></section>
<section><select multiple size=1><option class=inc>a<option class=inc>b</select><i class=c id=m1></i></section>
<section><select multiple size=0><option class=inc>a<option class=inc>b</select><i class=c id=m3></i></section>
<section><select multiple size=-1><option class=inc>a<option class=inc>b</select><i class=c id=m4></i></section>
<section><select size=4294967295><option class=inc>a<option class=inc>b</select><i class=c id=m5></i></section>
<section><select size=4294967296><option class=inc>a<option class=inc>b</select><i class=c id=m6></i></section>
<section><select multiple size=4294967296><option class=inc>a<option class=inc>b</select><i class=c id=m7></i></section>
<section><select size=3><option class=inc>x<div class=inc>y</div></option></select><i class=c id=m8></i></section>
<section><div><option class=inc>x<b class=inc>y</b></option></div><i class=c id=m9></i></section>
<section><wbr class=inc><i class=c id=m10></i></section>
<ol><li>a<wbr style="display: list-item"><li id=w2>b</ol>
"#,
        r"
s1::before 'C2'
e1::before 'C2'
s2::before 'C2'
e2::before 'C2'
e3::before 'C0'
e4::before 'C0'
s5::before 'C2'
e5::before 'C2'
e6::before 'C0'
e7::before 'C1'
m1::before 'C0'
m3::before 'C2'
m4::before 'C2'
m5::before 'C2'
m6::before 'C0'
m7::before 'C2'
m8::before 'C1'
m9::before 'C1'
m10::before 'C0'
LI::marker '1. '
w2::marker '2. '
",
    );
}

// Hostile input.

/// The text of the `::before` of every element with `::before`, in tree
/// order.
fn before_texts(css: &str, html: &str) -> Vec<String> {
    let (doc, map) = render(css, html);
    let root = doc.document_element().expect("a root element");
    doc.descendants(root)
        .filter_map(|n| map.pseudo(n, PseudoKind::Before))
        .map(|s| content_text(s).unwrap_or_default())
        .collect()
}

#[test]
fn deep_nesting_is_bounded_by_the_tree_depth() {
    // 1000 elements: deeper than the parser allows, and below the text limit.
    let html = "<div>".repeat(1000);
    let texts = before_texts(
        "div { counter-reset: c } div::before { content: counters(c, '.') }",
        &html,
    );
    let deepest = texts.last().expect("a ::before");
    let values = deepest.split('.').count();
    // The parser nests at most 512 elements.
    assert!((500..=512).contains(&values), "{values} values");
    assert!(deepest.split('.').all(|v| v == "0"));
}

#[test]
fn counter_text_is_limited() {
    let separator = "x".repeat(100_000);
    let css =
        format!("div {{ counter-reset: c }} div::before {{ content: counters(c, '{separator}') }}");
    let texts = before_texts(&css, &"<div>".repeat(200));
    let total: usize = texts.iter().map(String::len).sum();
    assert!(total <= super::MAX_COUNTER_TEXT, "{total} bytes");
    assert_eq!(texts[0], "0");
    assert!(texts[2].len() > 200_000);
    assert!(texts.last().is_some_and(String::is_empty));
}

#[test]
fn many_counter_names() {
    let names: Vec<String> = (0..1000).map(|i| format!("c{i}")).collect();
    let css = format!(
        "p {{ counter-increment: {} }} p::before {{ content: counter(c0) counter(c255) counter(c256) }}",
        names.join(" ")
    );
    let texts = before_texts(&css, &"<p></p>".repeat(2000));
    assert_eq!(texts.len(), 2000);
    // Only the first 256 names of the value count.
    assert_eq!(texts[1999], "200020000");
}

#[test]
fn nested_reversed_lists() {
    let mut html = String::new();
    // 200 levels of ol and li stay within the parser depth limit of 512.
    for _ in 0..200 {
        html.push_str("<ol reversed><li>a</li><li>b");
    }
    let (doc, map) = render("", &html);
    let root = doc.document_element().expect("a root element");
    let ordinals: Vec<i32> = doc
        .descendants(root)
        .filter_map(|n| map.list_item_ordinal(n))
        .collect();
    assert_eq!(ordinals.len(), 400);
    assert!(ordinals.chunks(2).all(|pair| pair == [2, 1]));
}

#[test]
fn nested_display_contents_reversed_lists() {
    // Lists with `display: contents` are not list owners, so each count
    // includes the items of the lists inside it; one walk computes all
    // counts.
    let mut html = String::new();
    for _ in 0..200 {
        html.push_str("<ol reversed style='display: contents'><li>x");
    }
    html.push_str(&"<i></i>".repeat(20_000));
    let (doc, map) = render("", &html);
    let root = doc.document_element().expect("a root element");
    let ordinals: Vec<i32> = doc
        .descendants(root)
        .filter_map(|n| map.list_item_ordinal(n))
        .collect();
    assert_eq!(ordinals, (1..=200).rev().collect::<Vec<i32>>());
}

/// The `content` items of the `::before` of the first element with one.
fn before_items(css: &str, html: &str) -> Vec<crate::values::ContentItem> {
    let (doc, map) = render(css, html);
    let root = doc.document_element().expect("a root element");
    doc.descendants(root)
        .find_map(|n| map.pseudo(n, PseudoKind::Before))
        .map(|s| match &s.content {
            crate::values::Content::Items(items) => items.to_vec(),
            _ => Vec::new(),
        })
        .expect("a ::before")
}

#[test]
fn runs_of_strings_and_counters_become_one_string() {
    use crate::values::ContentItem;
    let items = before_items(
        "p { counter-reset: c 3 } p::before { content: 'a' counter(c) 'b' counters(c, '.') }",
        "<p></p>",
    );
    assert_eq!(items, [ContentItem::String("a3b3".into())]);
    let items = before_items(
        "p { counter-reset: c 3 } p::before { content: counter(c) open-quote 'x' counter(c) }",
        "<p></p>",
    );
    assert_eq!(
        items,
        [
            ContentItem::String("3".into()),
            ContentItem::OpenQuote,
            ContentItem::String("x3".into()),
        ]
    );
}

#[test]
fn content_runs_around_other_items() {
    use crate::values::{ContentItem, Image};
    // An image ends a run; a counter without text gives an empty string.
    let items = before_items(
        "p { counter-reset: c 3 } p::before { content: counter(c) url(x.png) counter(c, none) }",
        "<p></p>",
    );
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], ContentItem::String("3".into()));
    assert!(matches!(items[1], ContentItem::Image(Image::Url(_))));
    assert_eq!(items[2], ContentItem::String("".into()));
    // A long string stays a separate (shared) item.
    let long = "x".repeat(100);
    let items = before_items(
        &format!("p {{ counter-reset: c 3 }} p::before {{ content: '{long}' counter(c) }}"),
        "<p></p>",
    );
    assert_eq!(
        items,
        [
            ContentItem::String(long.as_str().into()),
            ContentItem::String("3".into()),
        ]
    );
}

#[test]
fn counters_after_the_text_limit_give_no_text() {
    let separator = "x".repeat(1 << 20);
    let css = format!(
        "div {{ counter-reset: c }} div::before {{ content: counters(c, '{separator}') }} \
         i::before {{ content: 'a' counter(c) 'b' }}"
    );
    let html = format!("{}<i></i>", "<div>".repeat(8));
    let texts = before_texts(&css, &html);
    assert_eq!(texts.last().map(String::as_str), Some("ab"));
    // The strings around the counter still become one item.
    let (doc, map) = render(&css, &html);
    let i = doc
        .descendants(NodeId::DOCUMENT)
        .find(|&n| doc.element(n).is_some_and(|e| &**e.local_name() == "i"))
        .expect("an i element");
    let style = map.pseudo(i, PseudoKind::Before).expect("i::before");
    let crate::values::Content::Items(items) = &style.content else {
        panic!("items expected");
    };
    assert_eq!(&**items, [crate::values::ContentItem::String("ab".into())]);
}

#[test]
fn strings_up_to_the_item_size_are_merged() {
    use crate::values::ContentItem;
    let short = "s".repeat(super::MAX_MERGED_STRING);
    let long = "l".repeat(super::MAX_MERGED_STRING + 1);
    let items = before_items(
        &format!(
            "p {{ counter-reset: c 3 }} p::before {{ content: '{short}' counter(c) '{long}' }}"
        ),
        "<p></p>",
    );
    assert_eq!(
        items,
        [
            ContentItem::String(format!("{short}3").into()),
            ContentItem::String(long.as_str().into()),
        ]
    );
}

#[test]
fn directive_cache_is_bounded() {
    // More distinct counter lists than the cache holds (each `style`
    // attribute has its own list); the results stay right.
    let mut html = String::new();
    for k in 0..1500 {
        let _ = write!(html, "<p style='counter-increment: c {k}'></p>");
    }
    let texts = before_texts(
        "body { counter-reset: c } p::before { content: counter(c) }",
        &html,
    );
    assert_eq!(texts.len(), 1500);
    let expected: i64 = (0..1500).sum();
    assert_eq!(texts[1499], expected.to_string());
}

#[test]
fn many_counters_in_one_content_value() {
    // One string per pseudo-element, not one per counter; after the text
    // limit, the counters are removed.
    let css = format!(
        "p {{ counter-increment: c }} p::before {{ content: {} }}",
        "counter(c) ".repeat(2000)
    );
    let (doc, map) = render(&css, &"<p></p>".repeat(1000));
    let root = doc.document_element().expect("a root element");
    let mut total = 0;
    for style in doc
        .descendants(root)
        .filter_map(|n| map.pseudo(n, PseudoKind::Before))
    {
        let crate::values::Content::Items(items) = &style.content else {
            panic!("items expected");
        };
        let text = content_text(style).unwrap_or_default();
        assert!(items.len() <= 1, "{} items", items.len());
        total += text.len();
    }
    assert!(total <= super::MAX_COUNTER_TEXT, "{total} bytes");
    assert!(total > super::MAX_COUNTER_TEXT / 2, "{total} bytes");
}

#[test]
fn styles_without_changes_are_the_same() {
    let css = "li::before { content: counters(list-item, '.') }";
    let html = "<ol start=3><li>a<ol reversed><li>b<li>c</ol></ol>";
    let (_, first) = render(css, html);
    let (_, again) = render(css, html);
    assert!(first.same_styles(&again));
    let (_, other) = render(css, "<ol start=4><li>a<ol reversed><li>b<li>c</ol></ol>");
    assert!(!first.same_styles(&other));
    // Only the ordinal values differ.
    let (_, three) = render("", "<ol start=3><li>a</ol>");
    let (_, four) = render("", "<ol start=4><li>a</ol>");
    assert!(!three.same_styles(&four));
}
