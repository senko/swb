//! Size and inline-size containment of `container-type` and `contain`
//! (ADR 0038 section 5): the intrinsic sizes of an empty box, the auto
//! height of a size container, and the independent formatting context. The
//! boxes are those that Chromium 148 measured in the cases of
//! `tools/probes/container-queries.json` named in each test.

use crate::test_support::layout_html;

const HEAD: &str = "<!DOCTYPE html><style>body { margin: 0; font: 16px/20px sans-serif } \
    .c { container-type: inline-size } .s { container-type: size } \
    span { display: inline-block; width: 80px; height: 20px }</style>";

#[test]
fn shrink_to_fit_containers_are_as_wide_as_an_empty_box() {
    // `containment-shrink-to-fit`.
    let l = layout_html(&format!(
        "{HEAD}<div id=ib style='display:inline-block'><div class=c id=c1><span></span></div></div>\
         <div style='display:flex'><div class=c id=c3><span></span></div><div id=sib>x</div></div>\
         <div class=c id=c9 style='display:inline-block;contain-intrinsic-inline-size:50px'><span></span></div>\
         <div class=s id=c14 style='display:inline-block;contain-intrinsic-size:40px 30px'><span></span></div>\
         <div id=k style='display:inline-block;contain:inline-size'><span></span></div>"
    ));
    assert_eq!(l.rect("ib").width, 0.0);
    assert_eq!(l.rect("c1").width, 0.0);
    assert_eq!(
        l.rect("c3").width,
        0.0,
        "a flex item has no automatic minimum"
    );
    assert_eq!(l.rect("sib").x, 0.0);
    assert_eq!(l.rect("c9").width, 50.0);
    assert_eq!((l.rect("c14").width, l.rect("c14").height), (40.0, 30.0));
    assert_eq!(
        l.rect("k").width,
        0.0,
        "`contain: inline-size` acts the same"
    );
}

#[test]
fn size_containers_have_the_height_of_an_empty_box() {
    let l = layout_html(&format!(
        "{HEAD}<div class=s id=a><span></span></div>\
         <div class=s id=b style='contain-intrinsic-height:25px'><span></span></div>\
         <div id=strict style='contain:strict'><span></span></div>\
         <div class=c id=i><span></span></div>"
    ));
    assert_eq!(l.rect("a").height, 0.0);
    assert_eq!(l.rect("b").height, 25.0);
    assert_eq!(l.rect("strict").height, 0.0);
    assert!(
        l.rect("i").height > 0.0,
        "inline-size containment keeps the height"
    );
}

#[test]
fn containers_are_independent_formatting_contexts() {
    // `containment-layout-effects`: no margin collapsing through the
    // container, floats inside it count for its height, and an absolute
    // box keeps its containing block outside.
    let l = layout_html(&format!(
        "{HEAD}<div class=c id=a1><p id=p1 style='margin:20px 0;height:10px'></p></div>\
         <div class=c id=a2><div style='float:left;width:50px;height:60px'></div></div>\
         <div style='position:relative;height:100px' id=rel><div class=c style='margin-left:40px'>\
         <div id=abs style='position:absolute;left:0;top:0;width:10px;height:10px'></div></div></div>"
    ));
    assert_eq!((l.rect("a1").y, l.rect("a1").height), (0.0, 50.0));
    assert_eq!(l.rect("p1").y, 20.0);
    assert_eq!(l.rect("a2").height, 60.0);
    assert_eq!(l.rect("abs").x, 0.0);
}

/// The `y` and height of the box `id` of the layout `l`.
fn y_h(l: &crate::test_support::TestLayout, id: &str) -> (f32, f32) {
    (l.rect(id).y, l.rect(id).height)
}

#[test]
fn contain_alone_is_not_a_formatting_context() {
    // `contain-size-bfc`: margins collapse through boxes with `contain:
    // size` or `inline-size`, and they do not contain their floats; the
    // `container-type` of the same boxes does (the test above).
    let l = layout_html(
        "<!DOCTYPE html><style>body{margin:0;font:16px/20px sans-serif} p{margin:20px 0;height:10px}</style>\
         <div id=a1 style='contain:size;height:50px'><p id=p1>m</p></div>\
         <div id=a2 style='contain:size;height:50px'><div id=f1 style='float:left;width:50px;height:60px'></div></div>\
         <div id=after2>after float</div>\
         <div id=a3 style='contain:inline-size'><p id=p3>m</p></div>\
         <div id=a4 style='margin-top:30px;contain:size;height:0'><p id=p4 style='margin-top:50px'>x</p></div>\
         <div id=n1>n</div>",
    );
    assert_eq!(y_h(&l, "a1"), (20.0, 50.0));
    assert_eq!(l.rect("p1").y, 20.0);
    assert_eq!(y_h(&l, "a2"), (70.0, 50.0));
    assert_eq!(y_h(&l, "f1"), (70.0, 60.0));
    assert_eq!(y_h(&l, "after2"), (120.0, 20.0));
    assert_eq!(y_h(&l, "a3"), (160.0, 10.0));
    assert_eq!(l.rect("p3").y, 160.0);
    assert_eq!(y_h(&l, "a4"), (220.0, 0.0));
    assert_eq!(l.rect("p4").y, 220.0);
    assert_eq!(l.rect("n1").y, 220.0);
}

#[test]
fn contain_alone_keeps_the_sizes_of_an_empty_box() {
    // `contain-nonbfc-intrinsic`: the intrinsic widths of a block that is
    // no formatting context root (in a float), the auto height of `contain:
    // size`, with the margins of the children as usual.
    let l = layout_html(
        "<!DOCTYPE html><style>body{margin:0;font:16px/20px sans-serif}\
         .w{display:inline-block;width:80px;height:20px}p{margin:20px 0;height:10px}</style>\
         <div style='float:left;clear:left'><div id=i1 style='contain:inline-size'><span class=w></span></div></div>\
         <div style='float:left;clear:left'><div id=i2 style='contain:inline-size;contain-intrinsic-inline-size:50px'><span class=w></span></div></div>\
         <div style='float:left;clear:left'><div id=i3 style='contain:size'><span class=w></span></div></div>\
         <div style='float:left;clear:left'><div id=i4 style='contain:size;contain-intrinsic-size:60px 30px'><span class=w></span></div></div>\
         <div style='float:left;clear:left'><div id=i5 style='container-type:inline-size'><span class=w></span></div></div>\
         <div style='clear:both'></div>\
         <div id=s1 style='contain:size'><p id=sp1>m</p></div>\
         <div id=s2 style='contain:size;contain-intrinsic-height:25px'><p id=sp2>m</p></div>\
         <div id=s3 style='contain:inline-size'><p id=sp3>m</p></div>\
         <div id=s4 style='contain:size;padding:3px;border:2px solid'><p id=sp4>m</p></div>\
         <div id=n1>n</div>",
    );
    let wh = |id: &str| (l.rect(id).width, l.rect(id).height);
    assert_eq!(wh("i1"), (0.0, 25.0));
    assert_eq!(wh("i2"), (50.0, 25.0));
    assert_eq!(wh("i3"), (0.0, 0.0));
    assert_eq!(wh("i4"), (60.0, 30.0));
    assert_eq!(wh("i5"), (0.0, 25.0));
    assert_eq!(y_h(&l, "s1"), (125.0, 0.0));
    assert_eq!(l.rect("sp1").y, 125.0);
    assert_eq!(y_h(&l, "s2"), (145.0, 25.0));
    assert_eq!(y_h(&l, "s3"), (190.0, 10.0));
    assert_eq!(y_h(&l, "s4"), (220.0, 10.0));
    assert_eq!(l.rect("sp4").y, 245.0);
    assert_eq!(l.rect("n1").y, 230.0);
}

#[test]
fn boxes_without_size_containment_are_not_contained() {
    // An inline container and a table: size containment does not apply.
    let l = layout_html(&format!(
        "{HEAD}<div style='display:inline-block' id=w><span class=c id=s><span></span></span></div>\
         <table class=c id=t><tr><td><span></span></td></tr></table>"
    ));
    assert_eq!(l.rect("w").width, 80.0);
    assert!(l.rect("t").width >= 80.0);
}
