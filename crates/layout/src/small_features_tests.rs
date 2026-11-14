//! Layout of M10 sub-feature 7b (ADR 0038, section "7b"): layout and paint
//! containment (`contain: layout`, `paint`, `content`, `strict`), the skipped
//! contents of `content-visibility`, and `field-sizing: content`. The boxes
//! are those that Chromium 148 measured in the cases of
//! `tools/probes/css-small-features.json` named in each test.

use crate::test_support::{TestLayout, layout_html};

const HEAD: &str = "<!DOCTYPE html><style>body { margin: 0; font: 16px/20px sans-serif } \
    .k { width: 200px }</style>";

fn y_h(l: &TestLayout, id: &str) -> (f32, f32) {
    (l.rect(id).y, l.rect(id).height)
}

#[test]
fn layout_and_paint_containment_make_independent_formatting_contexts() {
    // `ct-layout`, `ct-paint`, `ct-content`: the box contains its floats,
    // its margins do not collapse with its children's, and it avoids
    // the floats beside it. `ct-style` and `ct-size` do not.
    for contain in ["layout", "paint", "content", "strict"] {
        let l = layout_html(&format!(
            "{HEAD}<div class=k id=a style='contain:{contain};height:auto'>\
             <div style='float:left;width:50px;height:50px'></div></div>\
             <div class=k id=b style='contain:{contain};margin-top:10px;height:auto'>\
             <div id=m style='margin-top:30px;height:10px'></div></div>"
        ));
        let height = if contain == "strict" { 0.0 } else { 50.0 };
        assert_eq!(l.rect("a").height, height, "contain: {contain}");
        if contain != "strict" {
            assert_eq!(y_h(&l, "b"), (60.0, 40.0), "contain: {contain}");
            assert_eq!(l.rect("m").y, 90.0, "contain: {contain}");
        }
    }
    let l = layout_html(&format!(
        "{HEAD}<div class=k id=a style='contain:style'>\
         <div style='float:left;width:50px;height:50px'></div></div>\
         <div class=k id=b style='contain:size;height:auto'></div>"
    ));
    assert_eq!(l.rect("a").height, 0.0, "contain: style");
}

#[test]
fn a_box_with_layout_containment_avoids_the_floats_beside_it() {
    // `ct-layout-float-clear`.
    let l = layout_html(&format!(
        "{HEAD}<div style='float:left;width:50px;height:80px'></div>\
         <div id=a style='contain:layout;width:100px;height:20px'>text</div>\
         <div id=b style='width:100px;height:20px'>text</div>"
    ));
    assert_eq!(l.rect("a").x, 50.0);
    assert_eq!(l.rect("b").x, 0.0);
}

#[test]
fn layout_and_paint_containment_are_containing_blocks_of_absolute_and_fixed_boxes() {
    // `ct-containing-block`.
    let l = layout_html(&format!(
        "{HEAD}<div id=l style='contain:layout;width:100px;height:100px;margin:20px'>\
         <div id=l1 style='position:absolute;right:0;bottom:0;width:10px;height:10px'></div>\
         <div id=l2 style='position:fixed;left:0;top:0;width:10px;height:10px'></div></div>\
         <div id=p style='contain:paint;width:100px;height:100px;margin:20px'>\
         <div id=p1 style='position:absolute;right:0;bottom:0;width:10px;height:10px'></div>\
         <div id=p2 style='position:fixed;left:0;top:0;width:10px;height:10px'></div></div>\
         <div id=s style='contain:style;width:100px;height:100px;margin:20px'>\
         <div id=s1 style='position:absolute;right:0;bottom:0;width:10px;height:10px'></div></div>\
         <span id=i style='contain:layout'>\
         <div id=i1 style='position:absolute;right:0;bottom:0;width:10px;height:10px'></div></span>"
    ));
    let xy = |id: &str| (l.rect(id).x, l.rect(id).y);
    assert_eq!(xy("l1"), (110.0, 110.0));
    assert_eq!(xy("l2"), (20.0, 20.0));
    assert_eq!(xy("p1"), (110.0, 230.0));
    assert_eq!(xy("p2"), (20.0, 140.0));
    // Contain style: no containing block; an inline box ignores layout
    // containment: the initial containing block holds the box.
    assert_eq!(xy("s1").0, 790.0);
    assert_eq!(xy("i1").0, 790.0);
}

#[test]
fn a_box_with_layout_containment_has_no_baseline() {
    // `ct-layout-baseline`: an inline-block sits on the line with its
    // bottom margin edge.
    let l = layout_html(&format!(
        "{HEAD}<div>text <span id=a style='display:inline-block;contain:layout;width:30px'>inner</span> \
         <span id=b style='display:inline-block;width:30px'>inner</span></div>"
    ));
    // The first sits with its bottom on the baseline (20 px below the top
    // of the line); the second with the baseline of its text.
    assert_eq!(l.rect("a").y, 0.0);
    assert_eq!(l.rect("b").y, 5.0);
}

#[test]
fn skipped_contents_have_the_intrinsic_size_of_the_box() {
    // `cv-hidden-boxes`: no content height; `contain-intrinsic-size` is
    // the size; the descendants are laid out as usual.
    let l = layout_html(&format!(
        "{HEAD}<style>.in {{ height: 100px }}</style>\
         <div class=k id=h style='content-visibility:hidden'><div class=in id=c1></div></div>\
         <div class=k id=h2 style='content-visibility:hidden;contain-intrinsic-size:150px 60px'>\
         <div class=in id=c2></div></div>\
         <div class=k id=h3 style='content-visibility:hidden;height:30px'><div class=in id=c3></div></div>\
         <div class=k id=h4 style='content-visibility:hidden;contain-intrinsic-height:40px'>\
         <div class=in id=c4></div></div>\
         <div id=h5 style='content-visibility:hidden;contain-intrinsic-width:90px;display:inline-block'>\
         <div class=in id=c5></div></div>"
    ));
    assert_eq!(y_h(&l, "h"), (0.0, 0.0));
    assert_eq!(y_h(&l, "h2"), (0.0, 60.0));
    assert_eq!(y_h(&l, "h3"), (60.0, 30.0));
    assert_eq!(y_h(&l, "h4"), (90.0, 40.0));
    assert_eq!(l.rect("h5").width, 90.0);
    // The descendants still have boxes (a script can ask for them).
    assert_eq!(y_h(&l, "c3"), (60.0, 100.0));
    assert_eq!(l.rect("c2").width, 200.0);
}

#[test]
fn content_visibility_auto_keeps_its_contents_and_size() {
    // `cv-auto-onscreen-boxes`: a relevant `auto` box is sized by its
    // content; it contains its floats and clips its content (paint
    // containment).
    let l = layout_html(&format!(
        "{HEAD}<div class=k id=a style='content-visibility:auto'><div id=a1 style='height:100px'></div></div>\
         <div class=k id=b style='content-visibility:auto;height:20px'><div id=b1 style='height:100px'></div></div>\
         <div class=k id=c style='content-visibility:auto'><div style='float:left;height:50px;width:10px'></div></div>"
    ));
    assert_eq!(y_h(&l, "a"), (0.0, 100.0));
    assert_eq!(y_h(&l, "b"), (100.0, 20.0));
    assert_eq!(y_h(&l, "c"), (120.0, 50.0));
}

const FIELD: &str = "<!DOCTYPE html><style>body { margin: 0; font: 16px/20px sans-serif } \
    input, textarea { margin: 0; font: 16px/20px sans-serif } .c { field-sizing: content } \
    select { margin: 0; font: 16px sans-serif; height: 21px; vertical-align: top }</style>";

fn width(l: &TestLayout, id: &str) -> f32 {
    l.rect(id).width
}

#[test]
fn a_text_field_is_as_wide_as_its_value_or_placeholder() {
    // `fs-input-basic`: 8 px of border and padding; the content is the
    // text (1 px if empty); `size` does not count.
    let l = layout_html(&format!(
        "{FIELD}<div><input id=a class=c></div><div><input id=b class=c value=abc></div>\
         <div><input id=c class=c placeholder=hello></div>\
         <div><input id=d class=c value=abc placeholder=hello></div>\
         <div><input id=e class=c size=30></div><div><input id=f value=abc></div>\
         <div><input id=g class=c style='min-width:100px' value=a></div>\
         <div><input id=h class=c style='max-width:30px' value=abcdefghijklmnop></div>"
    ));
    assert_eq!(width(&l, "a"), 9.0);
    assert!(
        width(&l, "b") > 9.0 && width(&l, "b") < 60.0,
        "{}",
        width(&l, "b")
    );
    assert_eq!(width(&l, "d"), width(&l, "b"), "the value wins");
    assert!(width(&l, "c") > width(&l, "b"), "hello is wider than abc");
    assert_eq!(width(&l, "e"), 9.0, "size is ignored");
    assert!(
        width(&l, "f") > 200.0,
        "without field-sizing: the default width"
    );
    assert_eq!(width(&l, "g"), 108.0, "min-width is the content box");
    assert_eq!(width(&l, "h"), 38.0);
}

#[test]
fn number_and_search_fields_keep_room_for_their_buttons() {
    // `fs-input-types`, `fs-input-types-empty`: 15 px more with a value or
    // nothing, not with a placeholder.
    let l = layout_html(&format!(
        "{FIELD}<div><input id=t class=c value=abc></div>\
         <div><input id=s class=c type=search value=abc></div>\
         <div><input id=n class=c type=number value=abc></div>\
         <div><input id=e class=c type=search></div>\
         <div><input id=p class=c type=search placeholder=abc></div>\
         <div><input id=r class=c type=range value=5></div>"
    ));
    assert_eq!(width(&l, "s"), width(&l, "t") + 15.0);
    assert_eq!(width(&l, "e"), 24.0);
    assert_eq!(width(&l, "p"), width(&l, "t"));
    // Types that swb shows as text fields but that Chromium does not size
    // by content keep the default width.
    assert!(width(&l, "r") > 200.0);
}

#[test]
fn a_text_area_follows_its_lines() {
    // `fs-textarea`: the widest line, one line at least, `cols` and `rows`
    // do not count; a final line feed is a line.
    let l = layout_html(&format!(
        "{FIELD}<div><textarea id=a class=c></textarea></div>\
         <div><textarea id=b class=c cols=40 rows=5>abc</textarea></div>\
         <div><textarea id=c class=c>a\nbb\nccc</textarea></div>\
         <div><textarea id=d class=c>a\n</textarea></div>\
         <div><textarea id=e class=c style='min-height:100px;min-width:100px'>a</textarea></div>\
         <div><textarea id=f>a</textarea></div>"
    ));
    assert_eq!((width(&l, "a"), l.rect("a").height), (7.0, 26.0));
    assert_eq!(l.rect("b").height, 26.0);
    assert_eq!(l.rect("c").height, 66.0);
    assert_eq!(l.rect("d").height, 46.0);
    assert_eq!((width(&l, "e"), l.rect("e").height), (106.0, 106.0));
    assert_eq!(l.rect("f").height, 46.0, "rows apply without field-sizing");
}

#[test]
fn a_select_is_as_wide_as_its_selected_option() {
    // `fs-select`: the selected option, not the widest; no rounding up.
    let l = layout_html(&format!(
        "{FIELD}<div><select id=a><option>a<option>bbbbbbbbbb</select></div>\
         <div><select id=b class=c><option>a<option>bbbbbbbbbb</select></div>\
         <div><select id=c class=c><option>a<option selected>bbbbbbbbbb</select></div>\
         <div><select id=d class=c style='display:block;height:auto'></select></div>"
    ));
    assert_eq!(width(&l, "a"), 111.0);
    assert!(
        width(&l, "b") < 31.0 && width(&l, "b") > 30.0,
        "{}",
        width(&l, "b")
    );
    assert!(
        width(&l, "c") > 110.5 && width(&l, "c") < 111.0,
        "{}",
        width(&l, "c")
    );
    assert_eq!((width(&l, "d"), l.rect("d").height), (22.0, 4.0));
}

#[test]
fn containment_and_skipped_contents_form_stacking_contexts() {
    // `ct-stacking-and-clip`: layout or paint containment, and
    // `content-visibility` other than `visible`, make a stacking context
    // (also without positioning); `contain: size` and `style` do not.
    let l = layout_html(&format!(
        "{HEAD}<div id=layout style='contain:layout'></div>\
         <div id=paint style='contain:paint'></div>\
         <div id=content style='contain:content'></div>\
         <div id=hidden style='content-visibility:hidden'></div>\
         <div id=auto style='content-visibility:auto'></div>\
         <div id=size style='contain:size'></div>\
         <div id=style style='contain:style'></div>\
         <div id=plain></div>"
    ));
    let forms = |id: &str| {
        let node = l.node(id);
        let mut found = None;
        l.tree.walk(|f, _| {
            if let crate::FragmentRef::Box(b) = f
                && b.node == Some(node)
                && b.pseudo.is_none()
            {
                found = Some(crate::positioned::forms_stacking_context(b));
            }
        });
        found.unwrap_or_else(|| panic!("#{id} has no box"))
    };
    for id in ["layout", "paint", "content", "hidden", "auto"] {
        assert!(forms(id), "#{id}");
    }
    for id in ["size", "style", "plain"] {
        assert!(!forms(id), "#{id}");
    }
}
