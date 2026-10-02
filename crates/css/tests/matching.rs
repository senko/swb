//! Selector matching against a small mock element tree that implements
//! `swb_css::Element`.

use swb_css::{
    CaseSensitivity, Element, ElementState, MatchingContext, QuirksMode, SelectorList, matches_any,
    matches_with_scope,
};

#[derive(Default)]
struct Node {
    name: String,
    html: bool,
    attributes: Vec<(String, String)>,
    parent: Option<usize>,
    children: Vec<usize>,
    has_text: bool,
    state: ElementState,
}

#[derive(Default)]
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// Adds an element. Attribute names must be lowercase for HTML elements.
    fn add(&mut self, parent: Option<usize>, name: &str, attributes: &[(&str, &str)]) -> usize {
        self.add_node(parent, name, true, attributes)
    }

    fn add_node(
        &mut self,
        parent: Option<usize>,
        name: &str,
        html: bool,
        attributes: &[(&str, &str)],
    ) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            name: name.to_owned(),
            html,
            attributes: attributes
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            parent,
            ..Node::default()
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    fn el(&self, id: usize) -> El<'_> {
        El { tree: self, id }
    }
}

#[derive(Clone, Copy)]
struct El<'t> {
    tree: &'t Tree,
    id: usize,
}

impl El<'_> {
    fn node(&self) -> &Node {
        &self.tree.nodes[self.id]
    }

    fn sibling(&self, offset: isize) -> Option<Self> {
        let parent = self.node().parent?;
        let siblings = &self.tree.nodes[parent].children;
        let index = siblings.iter().position(|&c| c == self.id)?;
        let other = index.checked_add_signed(offset)?;
        siblings.get(other).map(|&id| El {
            tree: self.tree,
            id,
        })
    }
}

impl Element for El<'_> {
    fn parent_element(&self) -> Option<Self> {
        self.node().parent.map(|id| El {
            tree: self.tree,
            id,
        })
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn first_child_element(&self) -> Option<Self> {
        self.node().children.first().map(|&id| El {
            tree: self.tree,
            id,
        })
    }

    fn local_name(&self) -> &str {
        &self.node().name
    }

    fn is_html_element(&self) -> bool {
        self.node().html
    }

    fn id(&self) -> Option<&str> {
        self.attribute("id")
    }

    fn has_class(&self, name: &str, case: CaseSensitivity) -> bool {
        self.attribute("class")
            .is_some_and(|classes| classes.split_ascii_whitespace().any(|c| case.eq(c, name)))
    }

    fn attribute(&self, local_name: &str) -> Option<&str> {
        self.node()
            .attributes
            .iter()
            .find(|(k, _)| k == local_name)
            .map(|(_, v)| v.as_str())
    }

    fn is_root(&self) -> bool {
        self.node().parent.is_none()
    }

    fn is_link(&self) -> bool {
        matches!(self.local_name(), "a" | "area" | "link") && self.attribute("href").is_some()
    }

    fn has_children(&self) -> bool {
        !self.node().children.is_empty() || self.node().has_text
    }

    fn state(&self) -> ElementState {
        self.node().state
    }

    fn same_element(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// The test document:
///
/// ```text
/// html
///   head
///   body.main[lang=en-US]
///     div#content.box.Big[data-x="Foo Bar"][dir=rtl]
///       p.first           (text)
///       p#p2.second[type=TEXT][title="Hello"]
///       span.note         (text)
///       p.third[lang=fr]  (empty)
///     ul
///       li × 6 (li 1, 3 and 5 have class "odd"; li 4 has class "x")
///     a[href=/x]          (hover)
///     a                   (no href)
///     svg
///       foreignObject
///         div.inner
/// ```
struct Doc {
    tree: Tree,
    html: usize,
    head: usize,
    body: usize,
    content: usize,
    p1: usize,
    p2: usize,
    span: usize,
    p3: usize,
    ul: usize,
    items: Vec<usize>,
    link: usize,
    anchor: usize,
    svg: usize,
    foreign: usize,
    inner: usize,
}

fn doc() -> Doc {
    let mut t = Tree::default();
    let html = t.add(None, "html", &[]);
    let head = t.add(Some(html), "head", &[]);
    let body = t.add(Some(html), "body", &[("class", "main"), ("lang", "en-US")]);
    let content = t.add(
        Some(body),
        "div",
        &[
            ("id", "content"),
            ("class", "box Big"),
            ("data-x", "Foo Bar"),
            ("dir", "rtl"),
        ],
    );
    let p1 = t.add(Some(content), "p", &[("class", "first")]);
    t.nodes[p1].has_text = true;
    let p2 = t.add(
        Some(content),
        "p",
        &[
            ("id", "p2"),
            ("class", "second"),
            ("type", "TEXT"),
            ("title", "Hello"),
        ],
    );
    let span = t.add(Some(content), "span", &[("class", "note")]);
    t.nodes[span].has_text = true;
    let p3 = t.add(Some(content), "p", &[("class", "third"), ("lang", "fr")]);
    let ul = t.add(Some(body), "ul", &[]);
    let items = (1..=6)
        .map(|i| {
            let class = match i {
                1 | 3 | 5 => "odd",
                4 => "x",
                _ => "",
            };
            t.add(Some(ul), "li", &[("class", class)])
        })
        .collect();
    let link = t.add(Some(body), "a", &[("href", "/x")]);
    t.nodes[link].state = ElementState::HOVER;
    let anchor = t.add(Some(body), "a", &[]);
    let svg = t.add_node(Some(body), "svg", false, &[("viewBox", "0 0 1 1")]);
    let foreign = t.add_node(Some(svg), "foreignObject", false, &[]);
    let inner = t.add(Some(foreign), "div", &[("class", "inner")]);
    Doc {
        tree: t,
        html,
        head,
        body,
        content,
        p1,
        p2,
        span,
        p3,
        ul,
        items,
        link,
        anchor,
        svg,
        foreign,
        inner,
    }
}

impl Doc {
    fn matches_in(&self, quirks_mode: QuirksMode, id: usize, selector: &str) -> bool {
        let list = SelectorList::parse_str(selector)
            .unwrap_or_else(|e| panic!("{selector:?} should parse: {e}"));
        let mut context = MatchingContext::new(quirks_mode);
        matches_any(&list, &self.tree.el(id), &mut context)
    }

    fn matches(&self, id: usize, selector: &str) -> bool {
        self.matches_in(QuirksMode::NoQuirks, id, selector)
    }

    /// All elements that match, in tree order.
    fn select(&self, selector: &str) -> Vec<usize> {
        (0..self.tree.nodes.len())
            .filter(|&id| self.matches(id, selector))
            .collect()
    }
}

#[test]
fn simple_selectors() {
    let d = doc();
    assert!(d.matches(d.p1, "p"));
    assert!(d.matches(d.p1, "P"));
    assert!(d.matches(d.p1, "*"));
    assert!(d.matches(d.p1, ".first"));
    assert!(!d.matches(d.p1, ".FIRST"));
    assert!(d.matches(d.p2, "#p2"));
    assert!(!d.matches(d.p2, "#P2"));
    assert!(d.matches(d.content, "div.box.Big#content"));
    assert!(!d.matches(d.content, ".big"));
    assert!(!d.matches(d.p1, "div"));
    assert_eq!(d.select("p"), vec![d.p1, d.p2, d.p3]);
}

#[test]
fn type_selectors_on_non_html_elements_are_case_sensitive() {
    let d = doc();
    assert!(d.matches(d.foreign, "foreignObject"));
    assert!(!d.matches(d.foreign, "foreignobject"));
    assert!(!d.matches(d.foreign, "FOREIGNOBJECT"));
    assert!(d.matches(d.svg, "svg"));
    assert!(!d.matches(d.svg, "SVG"));
    assert!(d.matches(d.inner, "foreignObject > div"));
    assert!(d.matches(d.svg, "[viewBox]"));
    assert!(!d.matches(d.svg, "[viewbox]"));
}

#[test]
fn attribute_selectors() {
    let d = doc();
    let cases: &[(usize, &str, bool)] = &[
        (d.content, "[data-x]", true),
        (d.content, "[DATA-X]", true),
        (d.content, "[data-y]", false),
        (d.content, "[data-x='Foo Bar']", true),
        (d.content, "[data-x='foo bar']", false),
        (d.content, "[data-x='foo bar' i]", true),
        (d.content, "[data-x~=Bar]", true),
        (d.content, "[data-x~=bar]", false),
        (d.content, "[data-x~=bar i]", true),
        (d.content, "[data-x~='Foo Bar']", false),
        (d.content, "[data-x~='']", false),
        (d.content, "[data-x^=Foo]", true),
        (d.content, "[data-x^='']", false),
        (d.content, "[data-x$=Bar]", true),
        (d.content, "[data-x$=bar]", false),
        (d.content, "[data-x*='o B']", true),
        (d.content, "[data-x*='O b' i]", true),
        (d.content, "[data-x*='']", false),
        (d.content, "[class|=box]", false),
        (d.body, "[lang|=en]", true),
        (d.body, "[lang|=EN]", true),
        (d.body, "[lang|=en-us]", true),
        (d.body, "[lang|=e]", false),
        (d.p3, "[lang|=fr]", true),
        // `type` is case-insensitive on HTML elements, unless `s` is given.
        (d.p2, "[type=text]", true),
        (d.p2, "[type=text s]", false),
        (d.p2, "[type=TEXT s]", true),
        (d.p2, "[type^=te]", true),
        (d.p2, "[title=hello]", false),
        (d.p2, "[title=hello i]", true),
        (d.content, "[dir=RTL]", true),
        (d.p1, "[|class=first]", true),
        (d.p1, "[*|class=first]", true),
    ];
    for (id, selector, expected) in cases {
        assert_eq!(d.matches(*id, selector), *expected, "{selector}");
    }
}

#[test]
fn combinators() {
    let d = doc();
    assert!(d.matches(d.p1, "body p"));
    assert!(d.matches(d.p1, "html p"));
    assert!(d.matches(d.p1, "div > p"));
    assert!(!d.matches(d.p1, "body > p"));
    assert!(d.matches(d.p2, "p + p"));
    assert!(!d.matches(d.p1, "p + p"));
    assert!(d.matches(d.p3, "p ~ p"));
    assert!(d.matches(d.p3, ".first ~ .third"));
    assert!(!d.matches(d.p3, "p + p"));
    assert!(d.matches(d.p3, "span + p"));
    assert!(d.matches(d.span, ".first + .second + span"));
    assert!(d.matches(d.span, "body > div p ~ span"));
    assert!(d.matches(d.span, "html .main > #content > .second ~ .note"));
    assert!(!d.matches(d.span, "ul span"));
    assert!(!d.matches(d.span, "p span"));
    assert!(d.matches(d.items[5], "body > ul > li ~ li"));
    assert!(!d.matches(d.items[0], "li ~ li"));
    assert_eq!(
        d.select("div > p + p"),
        vec![d.p2],
        "only the second paragraph follows a paragraph directly"
    );
    // Backtracking: the first ancestor that matches `div` is not the right
    // one, but another choice works.
    assert!(d.matches(d.inner, "body div"));
    assert!(d.matches(d.inner, "svg > foreignObject div.inner"));
    assert!(!d.matches(d.inner, "div div"));
    assert!(d.matches(d.p2, "head ~ body p"));
    assert!(!d.matches(d.p2, "head + div p"));
}

#[test]
fn structural_pseudo_classes() {
    let d = doc();
    assert!(d.matches(d.html, ":root"));
    assert!(!d.matches(d.body, ":root"));
    assert!(d.matches(d.p3, ":empty"));
    assert!(!d.matches(d.p1, ":empty"));
    assert!(d.matches(d.head, ":empty"));
    assert!(!d.matches(d.ul, ":empty"));
    assert!(d.matches(d.p1, ":first-child"));
    assert!(!d.matches(d.p2, ":first-child"));
    assert!(d.matches(d.p3, ":last-child"));
    assert!(d.matches(d.foreign, ":only-child"));
    assert!(!d.matches(d.p1, ":only-child"));
    assert!(d.matches(d.span, ":first-of-type"));
    assert!(d.matches(d.span, ":last-of-type"));
    assert!(d.matches(d.span, ":only-of-type"));
    assert!(d.matches(d.p1, ":first-of-type"));
    assert!(!d.matches(d.p2, ":first-of-type"));
    assert!(d.matches(d.p3, ":last-of-type"));
    assert!(!d.matches(d.p3, ":only-of-type"));
    assert!(d.matches(d.link, "a:first-of-type"));
    assert!(d.matches(d.anchor, "a:last-of-type"));
}

#[test]
fn nth_pseudo_classes() {
    let d = doc();
    let items = |sel: &str| -> Vec<usize> {
        d.items
            .iter()
            .enumerate()
            .filter(|&(_, &id)| d.matches(id, sel))
            .map(|(i, _)| i + 1)
            .collect()
    };
    assert_eq!(items(":nth-child(odd)"), vec![1, 3, 5]);
    assert_eq!(items(":nth-child(even)"), vec![2, 4, 6]);
    assert_eq!(items(":nth-child(2n+1)"), vec![1, 3, 5]);
    assert_eq!(items(":nth-child(3n)"), vec![3, 6]);
    assert_eq!(items(":nth-child(-n+3)"), vec![1, 2, 3]);
    assert_eq!(items(":nth-child(n+4)"), vec![4, 5, 6]);
    assert_eq!(items(":nth-child(4)"), vec![4]);
    assert_eq!(items(":nth-child(0)"), Vec::<usize>::new());
    assert_eq!(items(":nth-last-child(1)"), vec![6]);
    assert_eq!(items(":nth-last-child(-n+2)"), vec![5, 6]);
    assert_eq!(items(":nth-of-type(2)"), vec![2]);
    assert_eq!(items(":nth-last-of-type(2n)"), vec![1, 3, 5]);
    // `of S` counts only siblings that match S.
    assert_eq!(items(":nth-child(2 of .odd)"), vec![3]);
    assert_eq!(items(":nth-child(odd of .odd)"), vec![1, 5]);
    assert_eq!(items(":nth-last-child(1 of .odd, .x)"), vec![5]);
    assert_eq!(items(":nth-child(1 of :not(.odd))"), vec![2]);
    assert!(d.matches(d.p3, "p:nth-of-type(3)"));
    assert!(d.matches(d.p3, "p:nth-child(4)"));
    assert!(d.matches(d.span, ":nth-of-type(1)"));
}

#[test]
fn logical_pseudo_classes() {
    let d = doc();
    assert!(d.matches(d.p1, "p:not(.second)"));
    assert!(!d.matches(d.p2, "p:not(.second)"));
    assert!(!d.matches(d.p2, "p:not(.first, .second)"));
    assert!(d.matches(d.p3, "p:not(.first, .second)"));
    assert!(d.matches(d.p1, ":not(body > p)"));
    assert!(!d.matches(d.p1, ":not(div > p)"));
    assert!(d.matches(d.p1, ":is(.first, .third)"));
    assert!(!d.matches(d.p2, ":is(.first, .third)"));
    assert!(d.matches(d.p2, ":where(#p2)"));
    assert!(d.matches(d.p1, ":is(body .box) > p"));
    assert!(d.matches(d.p1, ":is(:is(:not(span)))"));
    // Invalid items in a forgiving list are dropped.
    assert!(d.matches(d.p1, ":is(::before, :-webkit-x, .first)"));
    assert!(!d.matches(d.p1, ":is()"));
    assert!(d.matches(d.p1, ":not(:is())"));
}

#[test]
fn has_pseudo_class() {
    let d = doc();
    assert!(d.matches(d.content, ":has(> p)"));
    assert!(d.matches(d.content, ":has(p)"));
    assert!(!d.matches(d.content, ":has(> li)"));
    assert!(d.matches(d.body, ":has(li)"));
    assert!(!d.matches(d.body, ":has(> li)"));
    assert!(d.matches(d.body, ":has(> ul > li.x)"));
    assert!(d.matches(d.body, ":has(ul .x)"));
    assert!(!d.matches(d.body, ":has(div .x)"));
    assert!(d.matches(d.p1, ":has(+ p)"));
    assert!(!d.matches(d.p1, ":has(+ span)"));
    assert!(d.matches(d.p1, ":has(~ span)"));
    assert!(!d.matches(d.p3, ":has(~ p)"));
    assert!(d.matches(d.content, ":has(+ ul li:nth-child(4))"));
    assert!(d.matches(d.content, ":has(~ svg div.inner)"));
    assert!(!d.matches(d.content, ":has(~ svg > div)"));
    assert!(d.matches(d.html, ":has(.inner)"));
    // The anchor is the :has() element, not some other element that
    // happens to match the leftmost compound.
    assert!(!d.matches(d.p1, ":has(p)"));
    assert!(d.matches(d.ul, "body:has(div) > ul"));
    assert!(d.matches(d.content, "div:has(.note, .missing)"));
    assert_eq!(d.select("p:has(+ span)"), vec![d.p2]);
    assert_eq!(d.select(":has(> foreignObject)"), vec![d.svg]);
}

#[test]
fn state_pseudo_classes() {
    let mut d = doc();
    assert!(d.matches(d.link, "a:hover"));
    assert!(!d.matches(d.anchor, "a:hover"));
    assert!(d.matches(d.link, ":link"));
    assert!(d.matches(d.link, ":any-link"));
    assert!(!d.matches(d.link, ":visited"));
    assert!(!d.matches(d.anchor, ":link"));
    assert!(!d.matches(d.anchor, ":any-link"));
    d.tree.nodes[d.link].state = ElementState::VISITED | ElementState::FOCUS;
    assert!(d.matches(d.link, ":visited:focus"));
    assert!(!d.matches(d.link, ":link"));
    assert!(d.matches(d.link, ":any-link"));
    let states = [
        (ElementState::ACTIVE, ":active"),
        (ElementState::FOCUS_VISIBLE, ":focus-visible"),
        (ElementState::FOCUS_WITHIN, ":focus-within"),
        (ElementState::TARGET, ":target"),
        (ElementState::CHECKED, ":checked"),
        (ElementState::DISABLED, ":disabled"),
        (ElementState::ENABLED, ":enabled"),
        (ElementState::INDETERMINATE, ":indeterminate"),
        (ElementState::REQUIRED, ":required"),
        (ElementState::OPTIONAL, ":optional"),
        (ElementState::READ_ONLY, ":read-only"),
        (ElementState::READ_WRITE, ":read-write"),
        (ElementState::PLACEHOLDER_SHOWN, ":placeholder-shown"),
        (ElementState::DEFAULT, ":default"),
        (ElementState::DEFINED, ":defined"),
        (ElementState::INVALID, ":invalid"),
        (ElementState::OPEN, ":open"),
    ];
    for (state, selector) in states {
        d.tree.nodes[d.p1].state = state;
        assert!(d.matches(d.p1, selector), "{selector}");
        d.tree.nodes[d.p1].state = ElementState::empty();
        assert!(!d.matches(d.p1, selector), "{selector}");
    }
}

#[test]
fn lang_and_dir() {
    let d = doc();
    assert!(d.matches(d.p1, ":lang(en)"));
    assert!(d.matches(d.p1, ":lang(EN-us)"));
    assert!(!d.matches(d.p1, ":lang(en-GB)"));
    assert!(!d.matches(d.p1, ":lang(e)"));
    assert!(d.matches(d.p1, ":lang(de, en)"));
    assert!(d.matches(d.p1, ":lang('*')"));
    assert!(d.matches(d.p3, ":lang(fr)"));
    assert!(!d.matches(d.p3, ":lang(en)"));
    assert!(!d.matches(d.html, ":lang(en)"));
    assert!(d.matches(d.p1, ":dir(rtl)"));
    assert!(!d.matches(d.p1, ":dir(ltr)"));
    assert!(d.matches(d.ul, ":dir(ltr)"));
}

#[test]
fn quirks_mode() {
    let d = doc();
    let quirks = |id, sel| d.matches_in(QuirksMode::Quirks, id, sel);
    // Class and ID selectors match case-insensitively.
    assert!(quirks(d.content, ".big"));
    assert!(quirks(d.content, ".BOX"));
    assert!(quirks(d.p2, "#P2"));
    assert!(!d.matches_in(QuirksMode::LimitedQuirks, d.content, ".big"));
    // Attribute values are not affected.
    assert!(!quirks(d.content, "[data-x='foo bar']"));
    // The :hover quirk: a compound with only :hover matches links only.
    let mut d = doc();
    d.tree.nodes[d.p1].state = ElementState::HOVER;
    let quirks = |d: &Doc, id, sel| d.matches_in(QuirksMode::Quirks, id, sel);
    assert!(!quirks(&d, d.p1, ":hover"));
    assert!(!quirks(&d, d.p1, "*:hover"));
    assert!(!quirks(&d, d.p1, "div :hover"));
    assert!(quirks(&d, d.p1, "p:hover"));
    assert!(quirks(&d, d.p1, ".first:hover"));
    assert!(quirks(&d, d.p1, ":first-child:hover"));
    assert!(quirks(&d, d.p1, ":is(:hover)"));
    assert!(quirks(&d, d.link, ":hover"));
    // The quirk does not apply to a compound with a pseudo-element.
    assert!(quirks(&d, d.p1, ":hover::before"));
    assert!(!quirks(&d, d.p1, ":hover > ::before"));
    assert!(d.matches(d.p1, ":hover"));
}

#[test]
fn scope() {
    let d = doc();
    let list = SelectorList::parse_str(":scope > p").expect("valid");
    let selector = &list.selectors()[0];
    let mut context = MatchingContext::default();
    let content = d.tree.el(d.content);
    assert!(matches_with_scope(
        selector,
        &d.tree.el(d.p1),
        Some(&content),
        &mut context
    ));
    let body = d.tree.el(d.body);
    assert!(!matches_with_scope(
        selector,
        &d.tree.el(d.p1),
        Some(&body),
        &mut context
    ));
    // Without a scope, `:scope` is the root.
    assert!(d.matches(d.html, ":scope"));
    assert!(d.matches(d.body, ":scope > body"));
}

#[test]
fn pseudo_elements_match_the_originating_element() {
    let d = doc();
    let list = SelectorList::parse_str("p.first::before, li::marker").expect("valid");
    let mut context = MatchingContext::default();
    assert!(list.selectors()[0].matches(&d.tree.el(d.p1), &mut context));
    assert!(!list.selectors()[0].matches(&d.tree.el(d.p2), &mut context));
    assert!(list.selectors()[1].matches(&d.tree.el(d.items[0]), &mut context));
}

#[test]
fn deep_trees_do_not_overflow() {
    // A chain of 50 000 nested elements, with selectors that walk all of it.
    let mut tree = Tree::default();
    let mut parent = tree.add(None, "html", &[]);
    for _ in 0..50_000 {
        parent = tree.add(Some(parent), "div", &[]);
    }
    let leaf = tree.add(Some(parent), "span", &[]);
    let mut context = MatchingContext::default();
    let check = |sel: &str, id: usize, context: &mut MatchingContext| {
        let list = SelectorList::parse_str(sel).expect("valid");
        matches_any(&list, &tree.el(id), context)
    };
    assert!(check("html span", leaf, &mut context));
    assert!(!check("nav span", leaf, &mut context));
    assert!(!check("nav div div div span", leaf, &mut context));
    assert!(check("html > div div div > span", leaf, &mut context));
    assert!(check(":has(span)", 0, &mut context));
    assert!(!check(":has(nav)", 0, &mut context));
}

#[test]
fn many_siblings_are_fast_enough() {
    // `a ~ b ~ c` style selectors on a flat list must not backtrack
    // exponentially.
    let mut tree = Tree::default();
    let root = tree.add(None, "ul", &[]);
    let mut last = root;
    for _ in 0..2000 {
        last = tree.add(Some(root), "li", &[]);
    }
    let list = SelectorList::parse_str("p ~ li ~ li ~ li ~ li ~ li").expect("valid");
    let mut context = MatchingContext::default();
    assert!(!matches_any(&list, &tree.el(last), &mut context));
    let list = SelectorList::parse_str("nav li li li li li").expect("valid");
    assert!(!matches_any(&list, &tree.el(last), &mut context));
}

#[test]
fn long_sibling_chains_do_not_overflow() {
    // `i+i+i+...` recurses once per compound; long chains are rejected at
    // parse time, shorter ones match.
    let mut tree = Tree::default();
    let root = tree.add(None, "div", &[]);
    let mut last = root;
    for _ in 0..5000 {
        last = tree.add(Some(root), "i", &[]);
    }
    assert!(SelectorList::parse_str(&vec!["i"; 5000].join("+")).is_err());
    let list = SelectorList::parse_str(&vec!["i"; 200].join("+")).expect("valid");
    let mut context = MatchingContext::default();
    assert!(matches_any(&list, &tree.el(last), &mut context));
    let list = SelectorList::parse_str(&vec!["i"; 200].join(" ~ ")).expect("valid");
    assert!(matches_any(&list, &tree.el(last), &mut context));
}

#[test]
fn nested_arguments_are_bounded_in_time() {
    // Each `:is(... *)` level walks all ancestors again; without a budget
    // this is exponential in the nesting depth.
    let mut tree = Tree::default();
    let mut parent = tree.add(None, "html", &[]);
    for _ in 0..40 {
        parent = tree.add(Some(parent), "div", &[]);
    }
    let mut selector = String::from(".x *");
    for _ in 0..12 {
        selector = format!(":is({selector}) *");
    }
    let list = SelectorList::parse_str(&selector).expect("valid");
    let mut context = MatchingContext::default();
    let start = std::time::Instant::now();
    assert!(!matches_any(&list, &tree.el(parent), &mut context));
    assert!(start.elapsed().as_secs() < 5, "took {:?}", start.elapsed());
    // A budget overrun is a non-match even under `:not()`.
    let list = SelectorList::parse_str(&format!(":not({selector})")).expect("valid");
    assert!(!matches_any(&list, &tree.el(parent), &mut context));
}

/// The tree with cache keys, to test the sibling index cache.
#[derive(Clone, Copy)]
struct Keyed<'t>(El<'t>);

impl Element for Keyed<'_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(Keyed)
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(Keyed)
    }
    fn next_sibling_element(&self) -> Option<Self> {
        self.0.next_sibling_element().map(Keyed)
    }
    fn first_child_element(&self) -> Option<Self> {
        self.0.first_child_element().map(Keyed)
    }
    fn local_name(&self) -> &str {
        self.0.local_name()
    }
    fn is_html_element(&self) -> bool {
        self.0.is_html_element()
    }
    fn id(&self) -> Option<&str> {
        self.0.id()
    }
    fn has_class(&self, name: &str, case: CaseSensitivity) -> bool {
        self.0.has_class(name, case)
    }
    fn attribute(&self, local_name: &str) -> Option<&str> {
        self.0.attribute(local_name)
    }
    fn is_root(&self) -> bool {
        self.0.is_root()
    }
    fn is_link(&self) -> bool {
        self.0.is_link()
    }
    fn has_children(&self) -> bool {
        self.0.has_children()
    }
    fn state(&self) -> ElementState {
        self.0.state()
    }
    fn same_element(&self, other: &Self) -> bool {
        self.0.same_element(&other.0)
    }
    fn cache_key(&self) -> Option<usize> {
        Some(self.0.id)
    }
}

#[test]
fn sibling_index_cache_gives_the_same_results() {
    // A mixed list: p, span, p, span, ...
    let mut tree = Tree::default();
    let root = tree.add(None, "div", &[]);
    let children: Vec<usize> = (0..60)
        .map(|i| tree.add(Some(root), if i % 3 == 0 { "span" } else { "p" }, &[]))
        .collect();
    let selectors = [
        ":nth-child(3n+1)",
        ":nth-last-child(odd)",
        "p:nth-of-type(4n)",
        "span:nth-last-of-type(-n+3)",
        ":first-of-type",
        ":last-of-type",
        ":nth-child(2 of span)",
    ];
    for selector in selectors {
        let list = SelectorList::parse_str(selector).expect("valid");
        let mut plain = MatchingContext::default();
        let mut cached = MatchingContext::default();
        // Visit in two different orders so that the cache is filled from
        // both ends.
        for pass in 0..2 {
            let order: Vec<usize> = if pass == 0 {
                children.clone()
            } else {
                children.iter().rev().copied().collect()
            };
            for &id in &order {
                let expected = matches_any(&list, &tree.el(id), &mut plain);
                let actual = matches_any(&list, &Keyed(tree.el(id)), &mut cached);
                assert_eq!(actual, expected, "{selector} on child {id}, pass {pass}");
            }
        }
        cached.clear_caches();
    }
}

#[test]
fn sibling_index_cache_makes_long_lists_fast() {
    let mut tree = Tree::default();
    let root = tree.add(None, "ul", &[]);
    let items: Vec<usize> = (0..20_000)
        .map(|_| tree.add(Some(root), "li", &[]))
        .collect();
    let list =
        SelectorList::parse_str("li:nth-child(odd), li:nth-last-of-type(3n)").expect("valid");
    let mut context = MatchingContext::default();
    let start = std::time::Instant::now();
    let count = items
        .iter()
        .filter(|&&id| matches_any(&list, &Keyed(tree.el(id)), &mut context))
        .count();
    assert_eq!(count, 10_000 + 3_333);
    assert!(start.elapsed().as_secs() < 2, "took {:?}", start.elapsed());
}
