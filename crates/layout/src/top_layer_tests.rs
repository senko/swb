//! The boxes of the top layer (ADR 0037 section 8, sub-feature 6b):
//! children of the initial containing block after the root box, placed
//! without the containing blocks, transforms and clips of their DOM
//! ancestors. The positions are those that Chromium 148 measured in the
//! cases of `tools/probes/top-layer.json` named in each test.

use swb_dom::NodeId;
use swb_style::PseudoKind;

use crate::test_support::{TestLayout, layout_html, layout_with_popovers};
use crate::{BoxFragment, Fragment, Rect};

/// The node and pseudo-element of each fragment of the top layer.
fn top_layer(l: &TestLayout) -> Vec<(Option<NodeId>, Option<PseudoKind>)> {
    l.tree
        .top_layer
        .iter()
        .map(|b| (b.node, b.pseudo))
        .collect()
}

/// True if the subtree of `b` has a box of `node`.
fn contains(b: &BoxFragment, node: NodeId) -> bool {
    b.node == Some(node)
        || b.children
            .iter()
            .any(|c| matches!(c, Fragment::Box(c) if contains(c, node)))
}

const POPOVER: &str = "<!DOCTYPE html><style>body { margin: 0 } .w { margin: 20px; width: 50px; \
    height: 50px } [popover] { inset: auto; left: 300px; top: 200px; margin: 0; width: 80px; \
    height: 40px; border: 0; padding: 0 }</style>";

#[test]
fn top_layer_boxes_are_children_of_the_initial_containing_block() {
    // `escape-ancestors`: transformed, clipped, contained, filtered and
    // translucent ancestors do not move or clip the popovers.
    let html = format!(
        "{POPOVER}<div class=w style='transform: translate(30px, 30px)'><div popover=manual id=p1>t</div></div>\
         <div class=w style='overflow: hidden; position: relative'><div popover=manual id=p2 style='top: 250px'>o</div></div>\
         <div class=w style='contain: paint'><div popover=manual id=p3 style='top: 300px'>c</div></div>\
         <div class=w style='filter: blur(0px)'><div popover=manual id=p4 style='top: 350px'>f</div></div>\
         <div class=w style='opacity: 0.5'><div popover=manual id=p5 style='top: 400px'>o</div></div>"
    );
    let l = layout_with_popovers(&html, &["p1", "p2", "p3", "p4", "p5"]);
    let ids = ["p1", "p2", "p3", "p4", "p5"];
    let expected: Vec<_> = ids
        .iter()
        .flat_map(|id| {
            let n = Some(l.node(id));
            [(n, Some(PseudoKind::Backdrop)), (n, None)]
        })
        .collect();
    assert_eq!(top_layer(&l), expected);
    for (i, id) in ids.iter().enumerate() {
        let top = 200.0 + 50.0 * i as f32;
        assert_eq!(l.rect(id), Rect::new(300.0, top, 80.0, 40.0), "#{id}");
        assert!(
            !contains(l.root(), l.node(id)),
            "#{id} has no box at its place"
        );
    }
    // The backdrop fills the viewport.
    assert_eq!(
        l.tree.top_layer[0].border_rect,
        Rect::new(0.0, 0.0, 800.0, 600.0)
    );
}

#[test]
fn no_box_inside_a_display_none_ancestor() {
    // `escape-ancestors` #6, `popover-author-display`: an open popover in
    // a `display: none` ancestor, or with `display: none` itself, has no
    // box; the hidden attribute does not hide an open popover.
    let html = format!(
        "{POPOVER}<style>[popover] {{ display: block }}</style>\
         <div style='display: none'><div popover=manual id=a>a</div></div>\
         <div popover=manual id=b style='display: none'>b</div>\
         <div popover=manual hidden id=c>c</div>"
    );
    let l = layout_with_popovers(&html, &["a", "b", "c"]);
    let c = Some(l.node("c"));
    assert_eq!(
        top_layer(&l),
        [(c, Some(PseudoKind::Backdrop)), (c, None)],
        "only #c has boxes"
    );
}

#[test]
fn containing_block_and_static_position() {
    // `popover-author-position`: a relative ancestor is not the containing
    // block; `static` computes to `absolute` with the user agent's
    // `inset: 0`. `popover-static-position`: with `inset: auto` the static
    // position is the origin; an absolutely positioned top-layer box
    // extends the scrollable area, a fixed one does not.
    let l = layout_with_popovers(
        "<!DOCTYPE html><style>body { margin: 0 } #w { position: relative; left: 100px; \
         top: 100px; width: 200px; height: 200px } #p { position: absolute; inset: auto; \
         left: 30px; top: 40px; margin: 0 } #q { position: static; margin: 0 } \
         #s { inset: auto; margin: 0; width: 50px; height: 20px; border: 0; padding: 0 } \
         #t { position: absolute; inset: auto; top: 2000px; left: 10px; margin: 0 } \
         #f { inset: auto; top: 3000px; margin: 0 }</style>\
         <div id=w><div popover=manual id=p>abs</div><div popover=manual id=q>static</div>\
         <p>x</p><div popover=manual id=s>s</div></div>\
         <div popover=manual id=t>t</div><div popover=manual id=f>f</div>",
        &["p", "q", "s", "t", "f"],
    );
    assert_eq!((l.rect("p").x, l.rect("p").y), (30.0, 40.0));
    assert_eq!((l.rect("q").x, l.rect("q").y), (0.0, 0.0));
    assert_eq!(l.rect("s"), Rect::new(0.0, 0.0, 50.0, 20.0));
    assert_eq!((l.rect("t").x, l.rect("t").y), (10.0, 2000.0));
    let t = l.rect("t");
    assert_eq!(l.tree.scroll_size.height, t.y + t.height);
}

#[test]
fn nested_top_layer_elements_have_their_own_boxes() {
    // A popover inside an open popover: each is an entry of its own, and
    // the outer one's box does not contain the inner one.
    let l = layout_with_popovers(
        &format!("{POPOVER}<div popover=manual id=a>a<div popover=manual id=b>b</div></div>"),
        &["a", "b"],
    );
    let (a, b) = (l.node("a"), l.node("b"));
    assert_eq!(
        top_layer(&l),
        [
            (Some(a), Some(PseudoKind::Backdrop)),
            (Some(a), None),
            (Some(b), Some(PseudoKind::Backdrop)),
            (Some(b), None)
        ]
    );
    assert!(!contains(&l.tree.top_layer[1], b));
}

#[test]
fn documents_without_a_top_layer_have_no_top_layer_boxes() {
    let l = layout_html(&format!("{POPOVER}<div popover id=a>a</div><p>x</p>"));
    assert!(l.tree.top_layer.is_empty());
    assert!(
        !contains(l.root(), l.node("a")),
        "a closed popover has no box"
    );
}

#[test]
fn nested_entries_are_in_layer_order_whichever_is_shown_first() {
    // `top-layer-nested-inner-first`: the inner popover is shown before
    // the outer one, so it is earlier in the layer; it is reached only in
    // the walk of the outer one's box, and still comes first.
    let l = layout_with_popovers(
        &format!("{POPOVER}<div popover=manual id=a>a<div popover=manual id=b>b</div></div>"),
        &["b", "a"],
    );
    let (a, b) = (l.node("a"), l.node("b"));
    assert_eq!(
        top_layer(&l),
        [
            (Some(b), Some(PseudoKind::Backdrop)),
            (Some(b), None),
            (Some(a), Some(PseudoKind::Backdrop)),
            (Some(a), None)
        ]
    );
}

#[test]
fn no_boxes_where_the_flat_ancestors_render_nothing() {
    // `top-layer-not-rendered-details` and `-video`: an open popover in a
    // closed `details` or in the fallback content of a `video` has no box
    // (Chromium: no client rect, `elementFromPoint` gives the page); one
    // in an open `details` has.
    let html = format!(
        "{POPOVER}<details><summary>s</summary><div popover=manual id=a>a</div></details>\
         <video><div popover=manual id=b>b</div></video>\
         <details open><summary>s</summary><div popover=manual id=c>c</div></details>\
         <div popover=manual id=d>d</div>"
    );
    let l = layout_with_popovers(&html, &["a", "b", "c", "d"]);
    let (c, d) = (Some(l.node("c")), Some(l.node("d")));
    assert_eq!(
        top_layer(&l),
        [
            (c, Some(PseudoKind::Backdrop)),
            (c, None),
            (d, Some(PseudoKind::Backdrop)),
            (d, None)
        ]
    );
}

#[test]
fn a_nested_entry_inside_an_unrendered_one_has_no_boxes() {
    // The outer popover is in a closed `details`: the inner one, reached
    // only through the outer one, is not reached either.
    let html = format!(
        "{POPOVER}<details><summary>s</summary><div popover=manual id=a>a\
         <div popover=manual id=b>b</div></div></details>"
    );
    let l = layout_with_popovers(&html, &["b", "a"]);
    assert!(l.tree.top_layer.is_empty());
}

#[test]
fn display_contents_top_layer_element_has_a_block_box() {
    // `top-layer-display-contents`: computed `block`, the child laid out
    // in its box.
    let l = layout_with_popovers(
        &format!(
            "{POPOVER}<div popover=manual id=a style='display: contents'>\
             <span id=s style='display: block; width: 30px; height: 30px'>s</span></div>"
        ),
        &["a"],
    );
    assert_eq!(l.rect("a"), Rect::new(300.0, 200.0, 80.0, 40.0));
    assert_eq!(l.rect("s"), Rect::new(300.0, 200.0, 30.0, 30.0));
}

#[test]
fn ten_thousand_nested_popovers_shown_inner_first_build_in_linear_time() {
    // Each entry is reached only in the box of its parent: the build must
    // not scan the layer per entry.
    use std::fmt::Write;
    const N: usize = 10_000;
    let mut html = String::from(POPOVER);
    for i in 0..N {
        let _ = write!(html, "<div popover=manual id=p{i}>");
    }
    for _ in 0..N {
        html.push_str("</div>");
    }
    let ids: Vec<String> = (0..N).rev().map(|i| format!("p{i}")).collect();
    let open: Vec<&str> = ids.iter().map(String::as_str).collect();
    let start = std::time::Instant::now();
    let l = layout_with_popovers(&html, &open);
    assert_eq!(l.tree.top_layer.len(), 2 * N);
    // Layer order: the innermost first.
    assert_eq!(
        l.tree.top_layer[1].node,
        Some(l.node(&format!("p{}", N - 1)))
    );
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
}
