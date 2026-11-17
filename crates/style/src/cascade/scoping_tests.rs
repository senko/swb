//! Style scoping in shadow trees (ADR 0036 section 5, sub-feature 5c-1):
//! the rules of a shadow tree's sheets, `:host`, `::slotted()`, `::part()`,
//! the context step of the cascade and the layers of each scope. The
//! values are those that Chromium 148 measured in the cases of
//! `tools/probes/web-shadow-dom.json` named in each test.

use swb_css::MediaEnvironment;
use swb_dom::{Document, ShadowRootInit, ShadowRootMode, SlotAssignment, parse_html};

use super::test_util::{base, node};
use super::*;
use crate::cssom::sheet::StyleMemory;
use crate::values::*;

/// Attaches an open root to `host` with `markup` as its content and returns
/// the root.
fn shadow(doc: &mut Document, host: NodeId, markup: &str) -> NodeId {
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

/// The element with id `id` in the tree of `root`.
fn in_root(doc: &Document, root: NodeId, id: &str) -> NodeId {
    doc.descendants(root)
        .find(|&n| doc.element(n).and_then(ElementData::id) == Some(id))
        .unwrap_or_else(|| panic!("no #{id} in the root"))
}

/// A document stylist with the sheet `css` and, for each `(root, css)` of
/// `scopes`, a scoped stylist of that sheet; roots with the same text share
/// one, as in the engine.
fn stylist(doc: &Document, css: &str, scopes: &[(NodeId, &str)]) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    let memory = StyleMemory::default();
    let mut built: HashMap<&str, u32> = HashMap::new();
    for &(root, css) in scopes {
        let index = *built.entry(css).or_insert_with(|| {
            let mut scoped = Stylist::new_scoped(doc.quirks_mode);
            scoped.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
            let hold = memory.reserve(1, "test").expect("fits");
            stylist.add_scope(scoped, hold)
        });
        stylist.set_root_scope(root, index);
    }
    stylist
}

fn styles(doc: &Document, css: &str, scopes: &[(NodeId, &str)]) -> StyleMap {
    compute_styles(
        doc,
        &stylist(doc, css, scopes),
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
    )
}

/// The style of `node` from [`compute_element_styles`] (`getComputedStyle`).
fn element_style(
    doc: &Document,
    css: &str,
    scopes: &[(NodeId, &str)],
    node: NodeId,
) -> Arc<ComputedStyle> {
    let chain = compute_element_styles(
        doc,
        &stylist(doc, css, scopes),
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
        node,
    );
    let (last, style) = chain.last().expect("a style");
    assert_eq!(*last, node);
    Arc::clone(style)
}

const RED: Rgba = Rgba::rgb(255, 0, 0);
const GREEN: Rgba = Rgba::rgb(0, 128, 0);
const BLUE: Rgba = Rgba::rgb(0, 0, 255);
const BLACK: Rgba = Rgba::rgb(0, 0, 0);

fn color(map: &StyleMap, node: NodeId) -> Rgba {
    map.get(node).expect("a style").color
}

#[test]
fn shadow_rules_match_their_tree_only() {
    // `style-scoping`: document rules do not match shadow nodes, shadow
    // rules do not match light nodes.
    let mut doc = parse_html(
        "<!DOCTYPE html><div id=h><span id=ls>light</span></div><span id=out>out</span>",
    );
    let h = node(&doc, "h");
    let root = shadow(&mut doc, h, "<span id=in>in</span><slot></slot>");
    let scopes = [(
        root,
        "span { color: rgb(0, 128, 0) } :host span { font-size: 30px }",
    )];
    let map = styles(&doc, "span { color: rgb(255, 0, 0) }", &scopes);
    let inner = in_root(&doc, root, "in");
    assert_eq!(color(&map, inner), GREEN);
    assert_eq!(map.get(inner).expect("a style").font_size, 30.0);
    assert_eq!(color(&map, node(&doc, "ls")), RED);
    assert_eq!(color(&map, node(&doc, "out")), RED);
    // The same through `getComputedStyle`.
    let style = element_style(&doc, "span { color: red }", &scopes, inner);
    assert_eq!((style.color, style.font_size), (GREEN, 30.0));
}

#[test]
fn host_rules_come_from_the_host_own_tree_only() {
    let mut doc = parse_html("<!DOCTYPE html><div id=h class=a></div>");
    let h = node(&doc, "h");
    let root = shadow(&mut doc, h, "<div id=nested></div>");
    let nested = in_root(&doc, root, "nested");
    let nested_root = shadow(&mut doc, nested, "x");
    let scopes = [
        (
            root,
            ":host { color: rgb(0, 128, 0); outline-style: dotted } \
             :host(.a) { font-size: 30px } :host(.b) { font-weight: 900 }",
        ),
        (nested_root, "p { color: red }"),
    ];
    // A document `:host` rule matches nothing (`host-featureless`).
    let map = styles(&doc, ":host { font-weight: 100 }", &scopes);
    let host = map.get(h).expect("a style");
    assert_eq!(
        (host.color, host.font_size, host.font_weight),
        (GREEN, 30.0, 400.0)
    );
    assert_eq!(host.outline_style, OutlineStyle::Dotted);
    // The nested host is an element of the outer tree, not its host: it
    // gets no `:host` rule of the outer tree (`outline-style` does not
    // inherit).
    let nested_style = map.get(nested).expect("a style");
    assert_eq!(nested_style.outline_style, OutlineStyle::None);
}

#[test]
fn context_ranks_normal_and_important() {
    // `style-scoping` and `host-attribute-and-hints`: for normal
    // declarations the outer context wins (the document over `:host`, also
    // the host's `style` attribute), for `!important` the inner one (also
    // over the attribute). The presentational hint loses to `:host`.
    let mut doc = parse_html(
        "<!DOCTYPE html><div id=h1></div><div id=h2></div>\
         <div id=h3 style='color: rgb(255, 0, 0)'></div>\
         <div id=h4 style='color: rgb(255, 0, 0) !important'></div><div id=h5 align=right></div>",
    );
    let mut scopes = Vec::new();
    for (id, css) in [
        ("h1", ":host { color: rgb(0, 128, 0) }"),
        ("h2", ":host { color: rgb(0, 128, 0) !important }"),
        ("h3", ":host { color: rgb(0, 128, 0) }"),
        ("h4", ":host { color: rgb(0, 128, 0) !important }"),
        ("h5", ":host { text-align: left }"),
    ] {
        let host = node(&doc, id);
        scopes.push((shadow(&mut doc, host, "x"), css));
    }
    let map = styles(
        &doc,
        "#h1 { color: rgb(255, 0, 0) } #h2 { color: rgb(255, 0, 0) !important }",
        &scopes,
    );
    assert_eq!(color(&map, node(&doc, "h1")), RED);
    assert_eq!(color(&map, node(&doc, "h2")), GREEN);
    assert_eq!(color(&map, node(&doc, "h3")), RED);
    assert_eq!(color(&map, node(&doc, "h4")), GREEN);
    assert_eq!(
        map.get(node(&doc, "h5")).expect("a style").text_align,
        TextAlign::Left
    );
}

#[test]
fn slotted_rules_match_assigned_elements_through_nested_slots() {
    // `style-scoping` and `slotted-nested-cascade`.
    let mut doc = parse_html(
        "<!DOCTYPE html><div id=a><span id=s>s</span><b id=lb><i id=li>deep</i></b></div>",
    );
    let a = node(&doc, "a");
    let ra = shadow(&mut doc, a, "<div id=b><slot id=as></slot></div>");
    let b = in_root(&doc, ra, "b");
    let rb = shadow(&mut doc, b, "<slot id=bs></slot>");
    let scopes = [
        (
            ra,
            "::slotted(span) { color: rgb(0, 128, 0); font-weight: 100 !important; \
             text-decoration-line: underline } ::slotted(b) { font-size: 30px } \
             ::slotted(i) { font-weight: 900 } ::slotted([data-x]) { font-style: oblique }",
        ),
        (
            rb,
            "::slotted(span) { color: rgb(255, 0, 0); font-weight: 900 !important; \
             font-style: italic }",
        ),
    ];
    let map = styles(&doc, "", &scopes);
    let s = map.get(node(&doc, "s")).expect("a style");
    // Normal: the outer tree's rule wins; `!important`: the inner tree's.
    assert_eq!((s.color, s.font_weight), (GREEN, 900.0));
    // The inner tree's rule matches through the outer slot.
    assert_eq!(s.font_style, FontStyle::Italic);
    assert!(
        s.text_decoration_line
            .contains(TextDecorationLine::UNDERLINE)
    );
    // Only assigned elements: the `i` inside the slotted `b` inherits (the
    // `b`'s bold).
    assert_eq!(map.get(node(&doc, "lb")).expect("a style").font_size, 30.0);
    let li = map.get(node(&doc, "li")).expect("a style");
    assert_eq!((li.font_size, li.font_weight), (30.0, 700.0));
    // A document rule wins over `::slotted()` for normal declarations.
    let map = styles(&doc, "#s { color: rgb(0, 0, 255) }", &scopes);
    assert_eq!(color(&map, node(&doc, "s")), BLUE);
}

#[test]
fn part_rules_reach_one_tree_down() {
    // `part-depth`: the document's `::part()` matches a part of its host's
    // tree, not of a nested host's tree; the middle tree's does.
    let mut doc = parse_html("<!DOCTYPE html><div id=p1></div>");
    let p1 = node(&doc, "p1");
    let r1 = shadow(
        &mut doc,
        p1,
        "<div id=ih part=mid></div><span id=sp part='a b' style='color: rgb(255, 0, 0)'>s</span>",
    );
    let ih = in_root(&doc, r1, "ih");
    let r2 = shadow(&mut doc, ih, "<span id=d part=deep>d</span>");
    let scopes = [
        (
            r1,
            "#ih::part(deep) { font-weight: 900 } span { font-style: italic } \
             :host::part(a) { font-size: 30px }",
        ),
        (r2, "span { color: rgb(255, 0, 0) !important }"),
    ];
    let css = "#p1::part(deep) { font-size: 40px } #p1::part(mid) { color: rgb(0, 128, 0) } \
               ::part(b a) { color: rgb(0, 0, 255); font-style: normal }";
    let map = styles(&doc, css, &scopes);
    assert_eq!(color(&map, ih), GREEN);
    let d = map.get(in_root(&doc, r2, "d")).expect("a style");
    assert_eq!((d.font_size, d.font_weight, d.color), (16.0, 900.0, RED));
    // The outer `::part()` wins over the part's own rules and `style`
    // attribute for normal declarations; `:host::part()` of its own tree
    // matches.
    let sp = map.get(in_root(&doc, r1, "sp")).expect("a style");
    assert_eq!(
        (sp.color, sp.font_style, sp.font_size),
        (BLUE, FontStyle::Normal, 30.0)
    );
}

#[test]
fn each_scope_has_its_own_layers() {
    // `layers-per-scope`.
    let mut doc = parse_html("<!DOCTYPE html><div id=lh></div>");
    let lh = node(&doc, "lh");
    let root = shadow(&mut doc, lh, "<span id=s>x</span>");
    let scopes = [(
        root,
        "@layer b { span { color: rgb(255, 0, 0) } } @layer a { span { color: rgb(0, 128, 0) } } \
         @layer b { :host { font-weight: 900 } } @layer a { :host { font-weight: 100 } }",
    )];
    let map = styles(
        &doc,
        "@layer a, b; @layer b { #lh { color: rgb(0, 0, 255) } }",
        &scopes,
    );
    assert_eq!(color(&map, in_root(&doc, root, "s")), GREEN);
    let host = map.get(lh).expect("a style");
    assert_eq!((host.font_weight, host.color), (100.0, BLUE));
}

#[test]
fn revert_layer_stays_in_its_context() {
    // A `revert-layer` of a shadow tree's layer rolls back that tree's
    // layer only: the document's rule for the host stays.
    let mut doc = parse_html("<!DOCTYPE html><div id=h></div>");
    let h = node(&doc, "h");
    let root = shadow(&mut doc, h, "x");
    let scopes = [(
        root,
        "@layer a { :host { font-size: 30px } } \
         @layer b { :host { color: revert-layer !important; font-size: revert-layer } }",
    )];
    // The inner `!important` wins and rolls back its layer; the document's
    // `!important` (unlayered, a later position in its own context) stays.
    let map = styles(&doc, "#h { color: rgb(0, 128, 0) !important }", &scopes);
    assert_eq!(color(&map, h), GREEN);
    // Normal: the shadow tree's later layer rolls back to its first layer.
    assert_eq!(map.get(h).expect("a style").font_size, 30.0);
    // The document's `revert-layer` (the outer context wins for normal
    // declarations) does not drop the shadow tree's `:host` rule.
    let scopes = [(root, ":host { color: rgb(0, 128, 0) }")];
    let map = styles(
        &doc,
        "@layer z { #h { color: revert-layer } } #x { color: red }",
        &scopes,
    );
    assert_eq!(color(&map, h), GREEN);
}

#[test]
fn revert_layer_floors_of_two_contexts() {
    // `revert-layer-contexts`: the shadow tree's `revert-layer !important`
    // rolls back its own `span` rule; the document's `::part()`
    // `revert-layer` of another context must not replace that floor.
    let mut doc = parse_html("<!DOCTYPE html><div id=rl></div><div id=rm></div><div id=rc></div>");
    let span = "<span id=s part=p>s</span>";
    let roots = ["rl", "rm", "rc"].map(|id| {
        let host = node(&doc, id);
        shadow(&mut doc, host, span)
    });
    let inner = "span { color: rgb(0, 128, 0) } span { color: revert-layer !important }";
    let custom = "span { --c: rgb(0, 128, 0); color: var(--c, rgb(0, 0, 255)) } \
                  span { --c: revert-layer !important }";
    let scopes = [(roots[0], inner), (roots[1], inner), (roots[2], custom)];
    let css = "#rl::part(p) { color: revert-layer } #rm::part(p) { color: revert-layer !important } \
               #rc::part(p) { --c: revert-layer }";
    let map = styles(&doc, css, &scopes);
    let colors = roots.map(|root| color(&map, in_root(&doc, root, "s")));
    assert_eq!(colors, [BLACK, BLACK, BLUE]);
}

#[test]
fn host_context_follows_the_flat_tree() {
    // `host-context-ancestors`.
    let mut doc = parse_html(
        "<!DOCTYPE html><div id=o><div id=x>light</div></div>\
         <div class=x><div id=o3><div id=x3></div></div></div>",
    );
    let o = node(&doc, "o");
    shadow(&mut doc, o, "<div class=x><slot></slot></div>");
    let o3 = node(&doc, "o3");
    shadow(&mut doc, o3, "no slot");
    let css = ":host-context(.x) { color: rgb(0, 128, 0) } :host { color: rgb(255, 0, 0) }";
    let x = node(&doc, "x");
    let x3 = node(&doc, "x3");
    let rx = shadow(&mut doc, x, "t");
    let rx3 = shadow(&mut doc, x3, "t");
    let scopes = [(rx, css), (rx3, css)];
    let map = styles(&doc, "", &scopes);
    assert_eq!(color(&map, x), GREEN);
    // Outside the flat tree: only its shadow-including ancestors have `.x`.
    assert_eq!(element_style(&doc, "", &scopes, x3).color, RED);
}

#[test]
fn host_context_answers_are_shared_in_a_pass() {
    // The walks of hosts with common flat ancestors share their answers
    // (`MatchingContext`): both kinds of answer must stay right.
    let mut doc = parse_html(
        "<!DOCTYPE html><div class=x><p id=a></p><p id=b></p></div>\
         <div><p id=c></p><p id=d></p></div>",
    );
    let css = ":host-context(.x) { color: rgb(0, 128, 0) } :host { color: rgb(255, 0, 0) }";
    let mut scopes = Vec::new();
    for id in ["a", "b", "c", "d"] {
        let host = node(&doc, id);
        scopes.push((shadow(&mut doc, host, "t"), css));
    }
    let map = styles(&doc, "", &scopes);
    let colors: Vec<Rgba> = ["a", "b", "c", "d"]
        .iter()
        .map(|id| color(&map, node(&doc, id)))
        .collect();
    assert_eq!(colors, [GREEN, GREEN, RED, RED]);
}

#[test]
fn pseudo_elements_of_host_and_slotted_rules() {
    // `scoped-pseudo-elements`.
    let mut doc = parse_html("<!DOCTYPE html><div id=q><span id=qs>s</span></div>");
    let q = node(&doc, "q");
    let root = shadow(&mut doc, q, "<b id=b part=p>b</b><slot></slot>");
    let scopes = [(
        root,
        ":host::before { content: 'H' } ::slotted(span)::after { content: 'A' } \
         ::slotted(span)::before { content: 'B'; color: rgb(0, 128, 0) }",
    )];
    let map = styles(&doc, "#q::part(p)::before { content: 'P' }", &scopes);
    let text = |node: NodeId, kind: PseudoKind| {
        let style = map.pseudo(node, kind).expect("a pseudo-element");
        crate::content::content_text(style).unwrap_or_default()
    };
    assert_eq!(text(q, PseudoKind::Before), "H");
    let qs = node(&doc, "qs");
    assert_eq!(text(qs, PseudoKind::After), "A");
    assert_eq!(text(qs, PseudoKind::Before), "B");
    assert_eq!(
        map.pseudo(qs, PseudoKind::Before).expect("a style").color,
        GREEN
    );
    assert_eq!(text(in_root(&doc, root, "b"), PseudoKind::Before), "P");
    // `getComputedStyle(host, '::before')`.
    let chain = compute_element_styles(
        &doc,
        &stylist(&doc, "", &scopes),
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
        q,
    );
    let before = compute_pseudo_element_style(
        &doc,
        &stylist(&doc, "", &scopes),
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
        &chain,
        PseudoKind::Before,
    )
    .expect("a style");
    assert_eq!(crate::content::content_text(&before).as_deref(), Some("H"));
}

#[test]
fn scoped_rules_do_not_share_styles_across_trees() {
    // Two hosts with equal parents and equal document rules, where only one
    // tree has `:host` rules: the style cache must not give one's style to
    // the other.
    let mut doc = parse_html("<!DOCTYPE html><div><p id=a></p><p id=b></p></div>");
    let a = node(&doc, "a");
    let b = node(&doc, "b");
    let ra = shadow(&mut doc, a, "x");
    shadow(&mut doc, b, "x");
    let map = styles(&doc, "", &[(ra, ":host { color: rgb(0, 128, 0) }")]);
    assert_eq!(color(&map, a), GREEN);
    assert_eq!(color(&map, b), BLACK);
}
