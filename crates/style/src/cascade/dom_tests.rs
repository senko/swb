//! The targeted restyle after DOM changes (ADR 0042), on small documents:
//! one test per path of section 2, the removed styles of section 3, the
//! fallbacks of section 5, and hostile documents with time bounds. Each
//! change is checked against a full restyle ([`StyleMap::first_difference`]).

use std::fmt::Write;
use std::time::{Duration, Instant};

use swb_css::MediaEnvironment;
use swb_dom::{Change, Document, parse_html};

use super::targeted::{FullReason, Restyle, restyle_targeted};
use super::*;
use crate::DomChanges;

fn base() -> Url {
    Url::parse("https://example.com/").expect("valid URL")
}

fn node(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

fn stylist(doc: &Document, css: &str) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    stylist
}

/// Adds the changes of `doc`'s journal to `changes`, as one drain of the
/// engine records them.
fn record_into(doc: &mut Document, changes: &mut DomChanges) {
    let journal = doc.take_journal();
    assert!(!journal.incomplete());
    for change in journal.iter() {
        let target = match &change {
            Change::Children { parent, .. } => *parent,
            Change::Attribute { element, .. } | Change::TopLayer { element } => *element,
            Change::Text { node, .. } => *node,
        };
        let connected = doc.shadow_including_root(target) == NodeId::DOCUMENT;
        changes.record(doc, &change, connected);
    }
}

/// The changes of `doc`'s journal, as one drain of the engine records them.
fn record(doc: &mut Document) -> DomChanges {
    let mut changes = DomChanges::default();
    record_into(doc, &mut changes);
    changes
}

/// An edit of a document between two drains.
type Edit<'a> = Box<dyn FnOnce(&mut Document) + 'a>;

/// Styles `html` with `css`, applies `edit`, and runs the targeted restyle;
/// a targeted result must equal a full restyle. Returns the result and the
/// targeted map.
fn dom_change<'a>(
    html: &str,
    css: &str,
    edit: impl FnOnce(&mut Document) + 'a,
) -> (Restyle, StyleMap, Document) {
    dom_changes_in_drains(html, css, vec![Box::new(edit)])
}

/// As [`dom_change`], with a drain of the journal after each edit (the
/// tasks of a page before one style flush).
fn dom_changes_in_drains(
    html: &str,
    css: &str,
    edits: Vec<Edit<'_>>,
) -> (Restyle, StyleMap, Document) {
    let mut doc = parse_html(html);
    let st = stylist(&doc, css);
    let env = MediaEnvironment::default();
    let sizes = ContainerSizes::new();
    let states = ElementStates::default();
    let mut map = compute_styles_with_containers(&doc, &st, &env, &states, &base(), &sizes);
    doc.set_journaling(true);
    let _ = doc.take_journal();
    let mut changes = DomChanges::default();
    for edit in edits {
        edit(&mut doc);
        record_into(&mut doc, &mut changes);
    }
    let before = map.clone();
    let result = restyle_targeted(
        &doc,
        &st,
        &env,
        &base(),
        &sizes,
        &states,
        &states,
        &changes,
        &mut map,
    );
    let full = compute_styles_with_containers(&doc, &st, &env, &states, &base(), &sizes);
    match &result {
        Restyle::Done { .. } => {
            if let Some(difference) = map.first_difference(&full) {
                panic!("{css}: {difference}");
            }
            assert!(map.same_result(&full), "{css}: the maps differ");
        }
        Restyle::Full(_) => assert!(map.same_result(&before), "a fallback keeps the map"),
    }
    (result, map, doc)
}

/// The number of restyled elements of a targeted result.
fn restyled(result: &Restyle) -> usize {
    match result {
        Restyle::Done { restyled, .. } => *restyled,
        Restyle::Full(reason) => panic!("a full restyle: {reason:?}"),
    }
}

fn element(doc: &mut Document, name: &str, class: &str) -> NodeId {
    let name = swb_dom::QualName::new(None, swb_dom::ns!(html), swb_dom::LocalName::from(name));
    let e = doc.create_element(name, Vec::new()).expect("element");
    if !class.is_empty() {
        doc.set_attr(e, "class", class).expect("class");
    }
    e
}

const LIST: &str = "<!DOCTYPE html><body><div id=p class=p>\
    <span id=a class=a>a</span><span id=b class=b>b</span><span id=c class=c>c</span>\
    <ul id=u><li id=l1>1</li><li id=l2>2</li><li id=l3>3</li></ul>\
    <p id=e></p><p id=t>text</p></div><div id=q></div></body>";

#[test]
fn class_changes_restyle_what_the_class_reaches() {
    let css = ".x .c { color: red } .x + .b { color: blue } .b.x ~ .c { margin: 1px }";
    let (result, ..) = dom_change(LIST, css, |d| {
        let p = node(d, "p");
        d.set_attr(p, "class", "p x").expect("class");
    });
    assert!(restyled(&result) <= 4, "{result:?}");
    dom_change(LIST, css, |d| {
        let a = node(d, "a");
        d.set_attr(a, "class", "a x").expect("class");
    });
    dom_change(LIST, css, |d| {
        let b = node(d, "b");
        d.set_attr(b, "class", "b x").expect("class");
    });
    // A class that no rule names restyles the element only.
    let (result, ..) = dom_change(LIST, css, |d| {
        let p = node(d, "p");
        d.set_attr(p, "class", "p y").expect("class");
    });
    assert_eq!(restyled(&result), 1);
}

#[test]
fn ids_attributes_and_attribute_derived_pseudo_classes() {
    let css = "#n .c { color: red } [data-s=on] > .b { color: blue } \
               :lang(de) .a { color: green } :dir(rtl) .c { margin: 1px } \
               a:link { color: red } input:disabled + span { color: red } \
               [hidden] { display: none }";
    dom_change(LIST, css, |d| {
        let p = node(d, "p");
        d.set_attr(p, "id", "n").expect("id");
    });
    dom_change(LIST, css, |d| {
        let p = node(d, "p");
        d.set_attr(p, "data-s", "on").expect("attr");
    });
    dom_change(LIST, css, |d| {
        let p = node(d, "p");
        d.set_attr(p, "lang", "de").expect("attr");
    });
    dom_change(LIST, css, |d| {
        let body = d.body().expect("body");
        d.set_attr(body, "dir", "rtl").expect("attr");
    });
    dom_change(LIST, css, |d| {
        let u = node(d, "u");
        d.set_attr(u, "hidden", "").expect("attr");
    });
    dom_change(
        "<!DOCTYPE html><a id=l>x</a><input id=i><span>s</span>",
        css,
        |d| {
            let l = node(d, "l");
            d.set_attr(l, "href", "#").expect("attr");
            let i = node(d, "i");
            d.set_attr(i, "disabled", "").expect("attr");
        },
    );
}

#[test]
fn attributes_read_by_other_elements() {
    let html = "<!DOCTYPE html><body><table id=t><tr><td id=d>1</td></tr></table>\
        <a id=a href=#>link</a><fieldset id=f><input id=i></fieldset>\
        <select><optgroup id=g><option id=o>o</option></optgroup></select>\
        <div id=e><p id=p>e</p></div><ol id=ol><li>1</li><li id=li>2</li></ol></body>";
    let css = "input:disabled { color: red } option:disabled { color: red } \
               :read-write { color: blue } li::marker { color: green }";
    dom_change(html, css, |d| {
        let t = node(d, "t");
        d.set_attr(t, "cellpadding", "9").expect("attr");
    });
    dom_change(html, css, |d| {
        let t = node(d, "t");
        d.set_attr(t, "border", "2").expect("attr");
    });
    dom_change(html, css, |d| {
        let body = d.body().expect("body");
        d.set_attr(body, "link", "red").expect("attr");
    });
    dom_change(html, css, |d| {
        let f = node(d, "f");
        d.set_attr(f, "disabled", "").expect("attr");
    });
    dom_change(html, css, |d| {
        let g = node(d, "g");
        d.set_attr(g, "disabled", "").expect("attr");
    });
    dom_change(html, css, |d| {
        let e = node(d, "e");
        d.set_attr(e, "contenteditable", "").expect("attr");
    });
    dom_change(html, css, |d| {
        let ol = node(d, "ol");
        d.set_attr(ol, "start", "5").expect("attr");
        let li = node(d, "li");
        d.set_attr(li, "value", "9").expect("attr");
    });
}

#[test]
fn insertions_style_the_subtree_and_what_it_reaches() {
    let css = ".a + .b { color: red } .a ~ .c { color: blue } li:nth-child(odd) { margin: 1px } \
               li:last-child { padding: 1px } li:first-child + li { color: red } \
               .p:has(.n) { outline: 1px solid } .p:has(> .n) .c { color: green } \
               .x:empty { height: 2px }";
    // Between `.a` and `.b`: `+` stops matching.
    dom_change(LIST, css, |d| {
        let n = element(d, "span", "n");
        let b = node(d, "b");
        let p = node(d, "p");
        d.insert_before(p, n, Some(b)).expect("insert");
    });
    // At the start and the end of a list.
    dom_change(LIST, css, |d| {
        let u = node(d, "u");
        let l1 = node(d, "l1");
        let li = element(d, "li", "");
        d.insert_before(u, li, Some(l1)).expect("insert");
        let li = element(d, "li", "");
        d.append_child(u, li).expect("append");
    });
    // A subtree with descendants.
    let (result, map, doc) = dom_change(LIST, css, |d| {
        let q = node(d, "q");
        let div = element(d, "div", "p");
        let inner = element(d, "span", "c");
        d.append_child(div, inner).expect("append");
        d.append_child(q, div).expect("append");
    });
    assert!(restyled(&result) >= 2);
    let q = node(&doc, "q");
    assert!(
        doc.descendants(q)
            .all(|n| doc.element(n).is_none() || map.get(n).is_some())
    );
}

#[test]
fn removals_and_moves_leave_no_style_behind() {
    let css = ".a + .b { color: red } li:nth-child(odd) { margin: 1px } \
               .p:has(.b) { outline: 1px solid } .h { display: none }";
    // A removed subtree: its nodes have no style (DOM node IDs are never
    // reused: `swb_dom::tree`, so the walk at the flush finds them). The
    // same markup gives the same node IDs.
    let u = node(&parse_html(LIST), "u");
    let (_, map, doc) = dom_change(LIST, css, |d| d.remove(u));
    assert!(doc.descendants(u).all(|n| map.get(n).is_none()));
    // Removed, changed while detached, and moved inside the detached tree.
    dom_change(LIST, css, |d| {
        let (l1, l2, l3) = (node(d, "l1"), node(d, "l2"), node(d, "l3"));
        d.remove(u);
        d.set_attr(l1, "class", "a").expect("class");
        d.remove(l3);
        d.append_child(l2, l3).expect("append");
    });
    // Moved into an element without a box: no style there.
    dom_change(LIST, css, |d| {
        let q = node(d, "q");
        d.set_attr(q, "class", "h").expect("class");
        d.remove(u);
        d.append_child(q, u).expect("append");
    });
    // Moved into a new element that is inserted into a hidden one.
    dom_change(LIST, css, |d| {
        let q = node(d, "q");
        d.set_attr(q, "class", "h").expect("class");
        let wrap = element(d, "div", "");
        let b = node(d, "b");
        d.remove(b);
        d.append_child(wrap, b).expect("append");
        d.append_child(q, wrap).expect("append");
    });
    // Moved to another place in the document.
    dom_change(LIST, css, |d| {
        let a = node(d, "a");
        let q = node(d, "q");
        d.remove(a);
        d.append_child(q, a).expect("append");
    });
}

#[test]
fn class_and_id_changes_then_removal_in_a_later_drain() {
    // The styles still have the old `class` and `id` of an element whose
    // attribute changed in one drain and that left in a later one: the
    // removal must look up the old keys too (found in the review).
    let html = "<!DOCTYPE html><body><div id=p class=p><div id=w><span id=s class=x>\
        </span></div></div><div id=r><div id=a class=a></div><i></i>\
        <div class=b><span class=c></span></div></div><div id=q></div></body>";
    let css = ".p:has(.x) { color: red } .a ~ .b { color: blue } .a ~ .b .c { margin: 1px } \
               .a:first-child ~ .b { padding: 1px } #a ~ .b { border-top: 1px solid }";
    let doc = parse_html(html);
    let (s, w, a, q) = (
        node(&doc, "s"),
        node(&doc, "w"),
        node(&doc, "a"),
        node(&doc, "q"),
    );
    let class = |n: NodeId, value: &'static str| -> Edit<'static> {
        Box::new(move |d: &mut Document| d.set_attr(n, "class", value).expect("class"))
    };
    let remove = |n: NodeId| -> Edit<'static> { Box::new(move |d: &mut Document| d.remove(n)) };
    dom_changes_in_drains(html, css, vec![class(s, "y"), remove(s)]);
    dom_changes_in_drains(html, css, vec![class(s, "y"), remove(w)]);
    dom_changes_in_drains(
        html,
        css,
        vec![
            class(s, "y"),
            Box::new(move |d| {
                d.remove(s);
                d.append_child(q, s).expect("append");
            }),
        ],
    );
    dom_changes_in_drains(html, css, vec![class(a, "c"), remove(a)]);
    dom_changes_in_drains(
        html,
        css,
        vec![
            Box::new(move |d| d.set_attr(a, "id", "z").expect("id")),
            remove(a),
        ],
    );
}

#[test]
fn text_changes_and_empty() {
    let css = "p:empty { height: 2px } textarea:placeholder-shown { color: red } \
               div:has(> p:empty) { margin: 1px }";
    dom_change(LIST, css, |d| {
        let e = node(d, "e");
        let t = d.create_text("x").expect("text");
        d.append_child(e, t).expect("append");
    });
    dom_change(LIST, css, |d| {
        let t = node(d, "t");
        let text = d.children(t).next().expect("text");
        d.set_data(text, "").expect("data");
    });
    dom_change(
        "<!DOCTYPE html><textarea id=x placeholder=p></textarea>",
        css,
        |d| {
            let x = node(d, "x");
            let t = d.create_text("v").expect("text");
            d.append_child(x, t).expect("append");
        },
    );
}

#[test]
fn details_summary_follows_its_children() {
    let html = "<!DOCTYPE html><details id=d open><summary id=s>s</summary>x</details>";
    dom_change(html, "", |d| {
        let s = node(d, "s");
        d.remove(s);
    });
}

#[test]
fn structure_changes_restyle_in_full() {
    let (result, ..) = dom_change(LIST, "", |d| {
        let body = d.body().expect("body");
        d.remove(body);
    });
    assert_eq!(result, Restyle::Full(FullReason::Structure));
}

#[test]
fn large_change_sets_restyle_in_full() {
    // More inserted elements than half of the styled ones.
    let (result, ..) = dom_change(LIST, "", |d| {
        let q = node(d, "q");
        for _ in 0..100 {
            let e = element(d, "span", "");
            d.append_child(q, e).expect("append");
        }
    });
    assert_eq!(result, Restyle::Full(FullReason::TooLarge));
    // More connected entries than the limit.
    let (result, ..) = dom_change(LIST, "", |d| {
        let p = node(d, "p");
        for i in 0..10_001 {
            d.set_attr(p, "data-i", &i.to_string()).expect("attr");
        }
    });
    assert_eq!(result, Restyle::Full(FullReason::TooLarge));
}

/// Runs `f` and fails if it took longer than `limit`.
fn within(limit: Duration, what: &str, f: impl FnOnce()) {
    let started = Instant::now();
    f();
    let took = started.elapsed();
    assert!(took < limit, "{what}: {took:?}");
}

/// Applies `edit` to `doc` `rounds` times, with a targeted restyle after
/// each; the last result must equal a full restyle.
fn rounds(
    doc: &mut Document,
    css: &str,
    rounds: usize,
    mut edit: impl FnMut(&mut Document, usize),
) {
    let st = stylist(doc, css);
    let env = MediaEnvironment::default();
    let sizes = ContainerSizes::new();
    let states = ElementStates::default();
    let mut map = compute_styles_with_containers(doc, &st, &env, &states, &base(), &sizes);
    doc.set_journaling(true);
    let _ = doc.take_journal();
    for round in 0..rounds {
        edit(doc, round);
        let changes = record(doc);
        let result = restyle_targeted(
            doc,
            &st,
            &env,
            &base(),
            &sizes,
            &states,
            &states,
            &changes,
            &mut map,
        );
        if let Restyle::Full(_) = result {
            map = compute_styles_with_containers(doc, &st, &env, &states, &base(), &sizes);
        }
    }
    let full = compute_styles_with_containers(doc, &st, &env, &states, &base(), &sizes);
    assert!(map.same_result(&full));
}

#[test]
fn hostile_class_toggles_among_20000_siblings() {
    // (The matching of `~` in the full restyles of the check is quadratic in
    // the siblings: 20,000, not 100,000.)
    let mut doc = parse_html(&format!("<ul>{}</ul>", "<li>x".repeat(20_000)));
    let items: Vec<NodeId> = doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| doc.element(n).is_some_and(|e| &**e.local_name() == "li"))
        .collect();
    let css = "li:nth-child(2n) { color: red } .a ~ li { color: blue } .a + li { color: green }";
    within(Duration::from_secs(30), "class toggles", || {
        rounds(&mut doc, css, 20, |d, i| {
            let li = items[19_980 + i % 20];
            let class = if i % 2 == 0 { "a" } else { "" };
            d.set_attr(li, "class", class).expect("class");
        });
    });
}

#[test]
fn hostile_deep_has() {
    // (`:has()` in the full restyles of the check is quadratic in the
    // depth.)
    let depth = 3_000;
    let mut html = String::from("<!DOCTYPE html><body>");
    for _ in 0..depth {
        html.push_str("<div class='x z'>");
    }
    html.push_str("<span id=leaf>x</span>");
    let mut doc = parse_html(&html);
    let leaf = node(&doc, "leaf");
    let css = ".x:has(.y) { color: red } .x:has(> .y) .z { color: blue } \
               .x:has(.y) + .w { color: green }";
    within(Duration::from_secs(60), "deep :has()", || {
        rounds(&mut doc, css, 4, |d, i| {
            let class = if i % 2 == 0 { "y" } else { "" };
            d.set_attr(leaf, "class", class).expect("class");
        });
    });
}

#[test]
fn hostile_insertion_of_100000_children() {
    let mut doc = parse_html("<!DOCTYPE html><ul id=u><li>a</li></ul>");
    let u = node(&doc, "u");
    within(Duration::from_secs(30), "100,000 children", || {
        rounds(&mut doc, "li:nth-child(odd) { color: red }", 2, |d, i| {
            if i == 0 {
                for _ in 0..100_000 {
                    let li = element(d, "li", "");
                    d.append_child(u, li).expect("append");
                }
            } else {
                let children: Vec<NodeId> = d.children(u).collect();
                for c in children {
                    d.remove(c);
                }
            }
        });
    });
}

#[test]
fn hostile_many_attribute_rules() {
    let mut css = String::new();
    for i in 0..10_000 {
        let _ = write!(css, "[data-x{i}] .d{i} {{ color: red }}");
    }
    let mut doc = parse_html(&format!(
        "<div id=r>{}</div>",
        "<p class=c>x</p>".repeat(1_000)
    ));
    let r = node(&doc, "r");
    within(Duration::from_secs(30), "attribute rules", || {
        rounds(&mut doc, &css, 200, |d, i| {
            d.set_attr(r, &format!("data-x{i}"), "").expect("attr");
        });
    });
}
