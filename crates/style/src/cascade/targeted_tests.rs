//! The targeted restyle after state changes (ADR 0041), on small documents:
//! one test per reach, per special path and per fallback. Each change is
//! checked against a full restyle ([`StyleMap::same_result`]).

use std::fmt::Write;
use std::time::{Duration, Instant};

use swb_css::MediaEnvironment;
use swb_dom::{Document, ShadowRootInit, ShadowRootMode, SlotAssignment, parse_html};

use super::targeted::{FullReason, Restyle, RestyleInputs, restyle_states};
use super::test_util::*;
use super::*;
use crate::cssom::sheet::StyleMemory;

fn hover(node: Option<NodeId>) -> ElementStates {
    ElementStates {
        hover: node,
        ..ElementStates::default()
    }
}

/// The result of a change from `old` to `new` states, checked against a
/// full restyle when the targeted path ran. Returns the result and the
/// styles after the change.
fn change_with(
    doc: &Document,
    stylist: &Stylist,
    sizes: &ContainerSizes,
    old: &ElementStates,
    new: &ElementStates,
) -> (Restyle, StyleMap) {
    let env = MediaEnvironment::default();
    let mut map = compute_styles_with_containers(doc, stylist, &env, old, &base(), sizes);
    let before = map.clone();
    let inputs = RestyleInputs {
        doc,
        stylist,
        env: &env,
        base_url: &base(),
        sizes,
    };
    let result = restyle_states(&inputs, old, new, &mut map);
    let full = compute_styles_with_containers(doc, stylist, &env, new, &base(), sizes);
    match &result {
        Restyle::Done { changed, .. } => {
            if let Some(difference) = map.first_difference(&full) {
                panic!("{old:?} -> {new:?}: {difference}");
            }
            assert_eq!(*changed, !before.same_styles(&full), "the change flag");
        }
        Restyle::Full(_) => assert!(map.same_result(&before), "a fallback keeps the map"),
    }
    (result, full)
}

fn change(doc: &Document, css: &str, old: &ElementStates, new: &ElementStates) -> Restyle {
    change_with(doc, &stylist(doc, css), &ContainerSizes::new(), old, new).0
}

/// Hovers `id` from nothing and back, checking both changes; returns the
/// restyled elements of the first.
fn hover_in_and_out(doc: &Document, css: &str, id: &str) -> usize {
    let on = hover(Some(node(doc, id)));
    let first = restyled(&change(doc, css, &hover(None), &on));
    restyled(&change(doc, css, &on, &hover(None)));
    first
}

const LIST: &str = "<!DOCTYPE html><body>\
    <div id=p class=p><span id=c1 class=c>1</span><span id=c2 class=c><b class=c id=g>g</b></span></div>\
    <div id=s1 class=a></div><div id=s2 class=b><i id=sd class=c></i></div><div id=s3 class=b></div>\
    </body>";

#[test]
fn self_and_children() {
    let doc = parse_html(LIST);
    assert_eq!(
        hover_in_and_out(&doc, ".p:hover { color: red }", "p"),
        4,
        "the element, its children and the grandchild below the changed child"
    );
    assert!(hover_in_and_out(&doc, ".p:hover > .c { color: red }", "p") <= 3);
    hover_in_and_out(&doc, ".p:hover { margin-left: 3px }", "p");
}

#[test]
fn descendants_and_siblings() {
    let doc = parse_html(LIST);
    hover_in_and_out(&doc, ".p:hover .c { color: red }", "p");
    hover_in_and_out(&doc, ".a:hover ~ .b { color: red }", "s1");
    hover_in_and_out(&doc, ".a:hover + .b { color: red }", "s1");
    hover_in_and_out(&doc, ".a:hover ~ .b .c { color: red }", "s1");
    hover_in_and_out(&doc, ":is(.a:hover ~ div) > i { color: red }", "s1");
    // Only the descendants with the subject's key are restyled (and the
    // children of those that changed).
    let only_g = hover_in_and_out(&doc, ".p:hover b { color: red }", "p");
    assert_eq!(only_g, 1);
}

#[test]
fn has_anchors() {
    let doc = parse_html(LIST);
    hover_in_and_out(&doc, ".p:has(:hover) { color: red }", "g");
    hover_in_and_out(&doc, ".a:has(+ .b:hover) { color: red }", "s2");
    hover_in_and_out(&doc, ".a:has(~ .b .c:hover) { color: red }", "sd");
    // ADR 0042: the dependency in any compound of the relative selector,
    // and `:has()` in a compound that is not the subject.
    hover_in_and_out(&doc, ".p:has(.b:hover .x) { color: red }", "s2");
    hover_in_and_out(&doc, "body:has(.b:hover) .a { color: red }", "s2");
    hover_in_and_out(&doc, ".p:has(.b:hover) > .a { color: red }", "s2");
    hover_in_and_out(&doc, ".a:has(+ .b:hover) ~ .b { color: red }", "s2");
}

#[test]
fn shapes_without_analysis_restyle_everything() {
    let doc = parse_html(LIST);
    let on = hover(Some(node(&doc, "s2")));
    for css in [
        ".b:nth-child(2 of .b:hover) { color: red }",
        ".p:has(:is(.b:hover .x)) { color: red }",
    ] {
        assert_eq!(
            change(&doc, css, &hover(None), &on),
            Restyle::Full(FullReason::Everything),
            "{css}"
        );
    }
    // The flipped element must have the key of the entry.
    let off_key = change(
        &doc,
        "nav:nth-child(2 of nav:hover) { color: red }",
        &hover(None),
        &on,
    );
    assert!(matches!(off_key, Restyle::Done { restyled: 0, .. }));
}

#[test]
fn display_none_to_and_from_block() {
    let doc = parse_html(
        "<!DOCTYPE html><div id=m class=m>menu<ul class=list><li>a<li>b<li id=l>c</ul>\
         <ol><li>x<li>y</ol></div><ol><li id=after>z</ol>",
    );
    let css = ".list { display: none } .m:hover .list { display: block } \
               li::marker { color: blue }";
    hover_in_and_out(&doc, css, "m");
    hover_in_and_out(&doc, ".m:hover ol { display: none }", "m");
}

#[test]
fn children_inherit_reset_properties_and_layout_parents() {
    let doc = parse_html(
        "<!DOCTYPE html><div id=p class=p><div class=q><span id=s>s</span></div>\
         <i id=i>i</i></div>",
    );
    hover_in_and_out(
        &doc,
        ".p:hover { background: red } i { background: inherit }",
        "p",
    );
    hover_in_and_out(
        &doc,
        ".p:hover { display: flex } .q { display: contents }",
        "p",
    );
    hover_in_and_out(&doc, ".p:hover { display: contents } ", "p");
}

#[test]
fn pseudo_elements_appear_and_go() {
    let doc = parse_html("<!DOCTYPE html><p id=p class=p>x</p><ul><li id=li>a</ul>");
    hover_in_and_out(&doc, ".p:hover::before { content: 'x' }", "p");
    hover_in_and_out(
        &doc,
        ".p::after { content: 'y' } .p:hover::after { content: none }",
        "p",
    );
    hover_in_and_out(&doc, "li:hover::marker { color: red }", "li");
}

#[test]
fn counters() {
    let doc = parse_html(
        "<!DOCTYPE html><body><ol><li id=a class=i>a<li id=b class=i>b<li>c</ol>\
         <p id=p class=n>x</p><p class=n>y</p></body>",
    );
    // No counter text: the counter pass runs again.
    hover_in_and_out(&doc, "li:hover { display: none }", "a");
    hover_in_and_out(&doc, "li:hover { display: block }", "b");
    // Counter text, inputs unchanged: the pass runs again.
    let css = "body { counter-reset: n } .n { counter-increment: n } \
               .n::before { content: counter(n) '. ' } \
               .n:hover::before { content: '[' counter(n, upper-roman) ']' }";
    hover_in_and_out(&doc, css, "p");
    // Counter text and a changed input: full restyle.
    let css = "body { counter-reset: n } .n { counter-increment: n } \
               .n::before { content: counter(n) } .n:hover { counter-increment: n 5 }";
    let on = hover(Some(node(&doc, "p")));
    assert_eq!(
        change(&doc, css, &hover(None), &on),
        Restyle::Full(FullReason::Counters)
    );
}

#[test]
fn use_instances_are_computed_again() {
    let doc = parse_html(
        "<!DOCTYPE html><svg id=svg class=s><defs><g id=sym><rect id=r class=r width=5 \
         height=5 /></g></defs><use href=#sym /></svg><p id=p class=p>p</p>",
    );
    hover_in_and_out(&doc, ".s:hover .r { fill: red }", "svg");
    hover_in_and_out(&doc, ".s:hover use { fill: red }", "svg");
    hover_in_and_out(&doc, ".p:hover { color: red }", "p");
}

/// A `use` element without a style before the change (below `display:
/// none`) gets an instance when it gets a style, and loses it again; an
/// instance copies elements of a hidden sprite sheet, which the walk does
/// not restyle.
#[test]
fn use_instances_follow_display_and_hidden_targets() {
    let doc = parse_html(
        "<!DOCTYPE html><body><div id=m class=m>m</div><svg id=s2 class=s2 width=5 height=5>\
         <use id=u2 href=#sym /></svg><svg id=sheet style='display: none'><g id=sym>\
         <rect id=r class=r width=5 height=5 /></g></svg></body>",
    );
    hover_in_and_out(
        &doc,
        ".s2 { display: none } .m:hover ~ .s2 { display: block }",
        "m",
    );
    hover_in_and_out(
        &doc,
        ".s2 { display: none } .s2:hover { display: block }",
        "s2",
    );
    hover_in_and_out(&doc, ".s2:hover { display: none }", "s2");
    hover_in_and_out(&doc, ".m:hover ~ svg .r { fill: red }", "m");
    hover_in_and_out(&doc, "body:hover .r { fill: red }", "m");
}

#[test]
fn query_containers() {
    let doc = parse_html(
        "<!DOCTYPE html><div id=c class=c><p id=x class=x>x</p></div><p id=y class=y>y</p>",
    );
    let mut sizes = ContainerSizes::new();
    sizes.insert(node(&doc, "c"), 500.0, 100.0);
    let css = ".c { container-type: inline-size } \
               @container (width > 300px) { .x:hover { color: red } } \
               .y:hover { color: red } .x:focus { color: blue }";
    let st = stylist(&doc, css);
    let on = |id: &str| hover(Some(node(&doc, id)));
    // The reader of the container.
    let (result, _) = change_with(&doc, &st, &sizes, &hover(None), &on("x"));
    assert_eq!(result, Restyle::Full(FullReason::Containers));
    // Outside the container: targeted; the log keeps its entries.
    let (result, _) = change_with(&doc, &st, &sizes, &hover(None), &on("y"));
    assert!(restyled(&result) > 0);
    // The container itself changes.
    let css = ".c { container-type: inline-size } .c:hover { color: red }";
    let (result, _) = change_with(&doc, &stylist(&doc, css), &sizes, &hover(None), &on("c"));
    assert_eq!(result, Restyle::Full(FullReason::Containers));
}

/// Styles left at the bound of the container passes: the log does not hold
/// the sizes of the last layout, so even a change outside the container
/// restyles in full (a targeted pass would record the new sizes and keep
/// the out-of-date query result of `.x`).
#[test]
fn container_sizes_that_the_log_does_not_hold() {
    let doc = parse_html(
        "<!DOCTYPE html><div id=c class=c><p id=x class=x>x</p></div><p id=y class=y>y</p>",
    );
    let css = ".c { container-type: inline-size } \
               @container (width > 300px) { .x { color: red } } .y:hover { color: red }";
    let st = stylist(&doc, css);
    let env = MediaEnvironment::default();
    let mut old_sizes = ContainerSizes::new();
    old_sizes.insert(node(&doc, "c"), 500.0, 100.0);
    let mut map =
        compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &old_sizes);
    let mut sizes = ContainerSizes::new();
    sizes.insert(node(&doc, "c"), 200.0, 100.0);
    let new = hover(Some(node(&doc, "y")));
    let inputs = RestyleInputs {
        doc: &doc,
        stylist: &st,
        env: &env,
        base_url: &base(),
        sizes: &sizes,
    };
    let result = restyle_states(&inputs, &hover(None), &new, &mut map);
    assert_eq!(result, Restyle::Full(FullReason::Containers));
    // With sizes that the log holds, the same change is targeted.
    let mut map =
        compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &old_sizes);
    let mut sizes = ContainerSizes::new();
    sizes.insert(node(&doc, "c"), 400.0, 100.0);
    let inputs = RestyleInputs {
        doc: &doc,
        stylist: &st,
        env: &env,
        base_url: &base(),
        sizes: &sizes,
    };
    let result = restyle_states(&inputs, &hover(None), &new, &mut map);
    assert!(restyled(&result) > 0);
}

#[test]
fn root_font_size() {
    let doc = parse_html("<!DOCTYPE html><p id=p>p</p>");
    let html = doc.document_element().expect("root");
    assert_eq!(
        change(
            &doc,
            "html:hover { font-size: 30px } p { margin: 1rem }",
            &hover(None),
            &hover(Some(html))
        ),
        Restyle::Full(FullReason::RootFontSize)
    );
    // The root's color alone is fine.
    restyled(&change(
        &doc,
        "html:hover { color: red }",
        &hover(None),
        &hover(Some(html)),
    ));
}

#[test]
fn root_line_height() {
    let doc = parse_html("<!DOCTYPE html><p id=p>p</p>");
    let html = doc.document_element().expect("root");
    assert_eq!(
        change(
            &doc,
            "html:hover { line-height: 30px } p { margin: 1rlh }",
            &hover(None),
            &hover(Some(html))
        ),
        Restyle::Full(FullReason::RootFontSize)
    );
}

#[test]
fn focus_states() {
    let doc = parse_html(
        "<!DOCTYPE html><form id=f class=f><input id=i class=i><a id=a class=a href=#>a</a>\
         </form><button id=b>b</button>",
    );
    let css = ".i:focus { color: red } .f:focus-within { color: blue } \
               .a:focus-visible { outline: 1px solid } .f:has(:focus-visible) { margin: 1px }";
    let focus = |id: &str, visible: bool| ElementStates {
        focus: Some(node(&doc, id)),
        focus_visible: visible,
        ..ElementStates::default()
    };
    for (a, b) in [
        (ElementStates::default(), focus("i", false)),
        (focus("i", false), focus("a", true)),
        (focus("a", true), focus("a", false)),
        (focus("a", false), focus("b", true)),
        (focus("b", true), ElementStates::default()),
    ] {
        change(&doc, css, &a, &b);
    }
    // `:target` is targeted too (ADR 0042 section 2).
    let target = ElementStates {
        target: Some(node(&doc, "a")),
        ..ElementStates::default()
    };
    assert!(matches!(
        change(
            &doc,
            ":target { color: red }",
            &ElementStates::default(),
            &target
        ),
        Restyle::Done { restyled: 1, .. }
    ));
}

fn attach(doc: &mut Document, host: NodeId, markup: &str) -> NodeId {
    let init = ShadowRootInit {
        mode: ShadowRootMode::Open,
        slot_assignment: SlotAssignment::Named,
        delegates_focus: false,
        clonable: false,
        serializable: false,
    };
    let root = doc.attach_shadow(host, init, false).expect("a host");
    let fragment = swb_dom::parse_fragment(doc, host, markup).expect("markup fits");
    doc.append_child(root, fragment).expect("a fragment");
    root
}

#[test]
fn shadow_trees() {
    let mut doc = parse_html(
        "<!DOCTYPE html><div id=h class=h><i id=x class=x>slotted</i></div><p id=p>p</p>",
    );
    let host = node(&doc, "h");
    let root = attach(
        &mut doc,
        host,
        "<span class=in>in</span><slot id=slot></slot><b part=pp class=pt>part</b>",
    );
    let inner = "\
        :host(:hover) .in { color: red } \
        ::slotted(.x:hover) { color: blue } \
        slot:hover::slotted(*) { margin: 2px } \
        .pt:hover { color: green }";
    let outer = ".h::part(pp):hover { color: orange } .h:hover::part(pp) { margin: 3px }";
    let mut st = stylist(&doc, outer);
    let memory = StyleMemory::default();
    let mut scoped = Stylist::new_scoped(doc.quirks_mode);
    scoped.add_author_sheet(&swb_css::parse_stylesheet(inner), &base());
    let index = st.add_scope(scoped, memory.reserve(1, "test").expect("fits"));
    st.set_root_scope(root, index);
    let part = doc
        .descendants(root)
        .find(|&n| doc.element(n).is_some_and(|e| e.has_class("pt")))
        .expect("part");
    let sizes = ContainerSizes::new();
    for target in [Some(node(&doc, "x")), Some(part), Some(host), None] {
        let (result, _) = change_with(
            &doc,
            &st,
            &sizes,
            &hover(Some(node(&doc, "p"))),
            &hover(target),
        );
        restyled(&result);
    }
}

/// A chain of `depth` nested divs with classes `a` and `b`, and a long
/// list of siblings.
fn hostile_document(depth: usize, siblings: usize) -> Document {
    let mut html = String::from("<!DOCTYPE html><body><div id=deep>");
    for _ in 0..depth {
        html.push_str("<div class='a b'>");
    }
    html.push_str("<span id=leaf>x</span>");
    for _ in 0..depth {
        html.push_str("</div>");
    }
    html.push_str("</div><ul id=list>");
    for i in 0..siblings {
        let _ = write!(html, "<li id=l{i}>x</li>");
    }
    html.push_str("</ul></body>");
    parse_html(&html)
}

#[test]
fn hostile_deep_tree() {
    let doc = hostile_document(5_000, 10);
    let css = "div:hover > div { color: red } .a:hover .b { margin: 1px } \
               .a:has(:hover) { padding: 1px } .b:hover ~ .a { color: blue }";
    let st = stylist(&doc, css);
    let env = MediaEnvironment::default();
    let leaf = hover(Some(node(&doc, "leaf")));
    let sizes = ContainerSizes::new();
    let mut map = compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &sizes);
    let inputs = RestyleInputs {
        doc: &doc,
        stylist: &st,
        env: &env,
        base_url: &base(),
        sizes: &sizes,
    };
    within(Duration::from_secs(20), "deep tree, in and out", || {
        for (a, b) in [(&hover(None), &leaf), (&leaf, &hover(None))] {
            let result = restyle_states(&inputs, a, b, &mut map);
            assert!(matches!(result, Restyle::Done { .. }), "{result:?}");
        }
    });
    let full = compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &sizes);
    assert!(map.same_result(&full));
}

/// Many nested roots that each add a key (`.cN:hover .dN` for each level of
/// a deep chain): the active key set stops growing at its limit, so the
/// walk stays linear in the depth. The parser limits nesting, so the chain
/// is built in parts.
#[test]
fn hostile_nested_roots_with_different_keys() {
    const PER_PART: usize = 250;
    let mut doc = parse_html("<!DOCTYPE html><body><div id=start></div></body>");
    let mut end = node(&doc, "start");
    let mut level = 0;
    for part in 0..32 {
        let mut markup = String::new();
        for _ in 0..PER_PART {
            let _ = write!(markup, "<div class=c{level}>");
            level += 1;
        }
        let _ = write!(markup, "<i id=e{part} class=d0></i>");
        let fragment = swb_dom::parse_fragment(&mut doc, end, &markup).expect("a fragment");
        doc.append_child(end, fragment)
            .expect("an element takes children");
        end = node(&doc, &format!("e{part}"));
    }
    let mut css = String::new();
    for i in 0..level {
        let _ = writeln!(css, ".c{i}:hover .d{i} {{ color: red }}");
    }
    let st = stylist(&doc, &css);
    let env = MediaEnvironment::default();
    let sizes = ContainerSizes::new();
    let on = hover(Some(end));
    let mut map = compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &sizes);
    let started = Instant::now();
    let full = compute_styles_with_containers(&doc, &st, &env, &on, &base(), &sizes);
    let full_took = started.elapsed();
    let started = Instant::now();
    let inputs = RestyleInputs {
        doc: &doc,
        stylist: &st,
        env: &env,
        base_url: &base(),
        sizes: &sizes,
    };
    let result = restyle_states(&inputs, &hover(None), &on, &mut map);
    let took = started.elapsed();
    assert!(matches!(result, Restyle::Done { .. }), "{result:?}");
    assert!(map.first_difference(&full).is_none());
    assert!(
        took < full_took * 2 + Duration::from_millis(200),
        "targeted {took:?}, full {full_took:?}"
    );
}

/// A long list of siblings: `~` restyles all following siblings (as a full
/// restyle would), `+` only the next one. The matching of `li:hover ~ li`
/// is quadratic in the list for any restyle, so the bound is relative to a
/// full restyle.
#[test]
fn hostile_long_sibling_list() {
    let doc = hostile_document(2, 5_000);
    let env = MediaEnvironment::default();
    let sizes = ContainerSizes::new();
    for css in [
        "li:hover ~ li { color: red } li:hover + li { margin: 1px }",
        "li:hover + li { margin: 1px } li:hover { color: red }",
    ] {
        let st = stylist(&doc, css);
        let mut map =
            compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &sizes);
        let mut states = hover(None);
        for id in ["l0", "l2500", "l4999"] {
            let next = hover(Some(node(&doc, id)));
            let started = Instant::now();
            let full = compute_styles_with_containers(&doc, &st, &env, &next, &base(), &sizes);
            let limit = started.elapsed() * 3 + Duration::from_millis(200);
            let inputs = RestyleInputs {
                doc: &doc,
                stylist: &st,
                env: &env,
                base_url: &base(),
                sizes: &sizes,
            };
            within(limit, css, || {
                let result = restyle_states(&inputs, &states, &next, &mut map);
                assert!(matches!(result, Restyle::Done { .. }), "{result:?}");
            });
            assert!(map.same_result(&full));
            states = next;
        }
    }
}

#[test]
fn hostile_many_rules() {
    let doc = hostile_document(20, 200);
    let mut css = String::new();
    for i in 0..10_000 {
        let _ = writeln!(
            css,
            ".c{i}:has(:hover) {{ color: red }} .c{i}:hover .d{i} {{ color: blue }}"
        );
    }
    let st = stylist(&doc, &css);
    let env = MediaEnvironment::default();
    let sizes = ContainerSizes::new();
    let mut map = compute_styles_with_containers(&doc, &st, &env, &hover(None), &base(), &sizes);
    let mut states = hover(None);
    let inputs = RestyleInputs {
        doc: &doc,
        stylist: &st,
        env: &env,
        base_url: &base(),
        sizes: &sizes,
    };
    within(Duration::from_secs(20), "many rules", || {
        for id in ["leaf", "l10", "l150"] {
            let next = hover(Some(node(&doc, id)));
            let result = restyle_states(&inputs, &states, &next, &mut map);
            assert!(
                matches!(result, Restyle::Done { restyled: 0, .. }),
                "{result:?}"
            );
            states = next;
        }
    });
    assert!(st.invalidation_index().len() >= 20_000);
}
