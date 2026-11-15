//! Tests of the parser's pauses for custom elements (ADR 0035 section 8):
//! the tree with pauses equals the tree without definitions, the tokenizer
//! state after a pause, the recorded insertion, the alias table, written
//! text and the pause for reactions.

use std::cell::RefCell;
use std::rc::Rc;

use html5ever::{LocalName, QualName, ns};

use super::{parse_with_driver, test_pages};
use crate::custom::CustomElementState;
use crate::dump::dump_tree;
use crate::journal::{Change, MutationSink};
use crate::parser::{HtmlParser, ParseStep, WrittenStep, decode_html_bytes, parse_html_with};
use crate::serialize::outer_html;
use crate::tree::{Attribute, Document, ElementData, Limits, NodeData, NodeId};

/// What the test's runtime does at a pause for a custom element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Construct {
    /// A new element with the token's attributes takes the placeholder's
    /// place (a constructor that succeeded).
    Element,
    /// No constructor ran: the placeholder is inserted.
    Nothing,
}

/// The element that a constructor makes for the placeholder: a new element
/// with the name, state `custom`, then the token's attributes.
fn construct(doc: &mut Document, placeholder: NodeId, attrs: &[Attribute]) -> NodeId {
    let data = doc.element(placeholder).expect("a placeholder element");
    assert!(
        data.attributes().is_empty(),
        "a placeholder has no attributes"
    );
    assert_eq!(doc.parent(placeholder), None, "a placeholder is detached");
    let mut data = ElementData::new(data.name.clone(), Vec::new());
    data.set_custom_element_state(CustomElementState::Custom, 1);
    let element = doc.create_node(NodeData::Element(data)).expect("room");
    for a in attrs {
        doc.set_attribute(element, &a.name, &a.value).expect("room");
    }
    element
}

/// Answers a pause for a custom element as `how` says.
fn answer(parser: &mut HtmlParser, doc: &mut Document, placeholder: NodeId, how: Construct) {
    let element = match how {
        Construct::Element => {
            let attrs = parser.custom_element_attributes();
            Some(construct(doc, placeholder, &attrs))
        }
        Construct::Nothing => None,
    };
    parser.finish_custom_element(doc, placeholder, element);
}

/// Parses `text` with the custom element names `names` defined, answering
/// each pause for a custom element as `how` says. Returns the document and
/// the number of those pauses.
fn parse_defined(text: &str, names: &[&str], how: Construct, scripting: bool) -> (Document, usize) {
    let mut doc = Document::new();
    for name in names {
        doc.add_defined_name(LocalName::from(*name));
    }
    let mut parser = HtmlParser::new(&mut doc, text, scripting);
    let mut pauses = 0;
    loop {
        match parser.resume(&mut doc).expect("the parser's document") {
            ParseStep::Script(_) | ParseStep::Reactions => {}
            ParseStep::CustomElement(placeholder) => {
                pauses += 1;
                answer(&mut parser, &mut doc, placeholder, how);
            }
            ParseStep::Finished => break,
        }
    }
    (doc, pauses)
}

/// The markup around the cases of the tokenizer state: a custom element
/// before raw text, script data, `plaintext`, foreign content, tables with
/// foster parenting, `select`, the adoption agency, templates, `head`,
/// `frameset`, forms and lists.
const CASES: &[&str] = &[
    "<x-a></x-a><textarea><b>x</b>&amp;</textarea><p>after",
    "<x-a><script>if (a<b) x('</p>')</script>after",
    "<x-a id=1 class=c><plaintext><b>x</b></plaintext>",
    "<x-a><style>a<b{}</style><title>&lt;t</title><xmp><i></xmp><iframe><b></iframe>",
    "<x-a><noscript><b>x</b></noscript><noembed><i></noembed>",
    "<svg><x-a></x-a><foreignObject><x-a id=f><![CDATA[c]]></x-a></foreignObject><![CDATA[d]]></svg>",
    "<math><annotation-xml encoding='text/html'><x-a><b>t</b></x-a></annotation-xml><mi>x</mi></math>",
    "<table><x-a>f<b>g</b></x-a><tr><td>c<x-a>d</x-a></td></tr></table>",
    "<table><tr><x-a>f</x-a><td>c</td></tr><x-b><x-a></x-a></x-b></table>",
    "<select><x-a>t</x-a><option>o<x-b>p</x-b></option></select>",
    "<b><x-a><p>t</b>y",
    "<b>1<p>2<x-a id=m></x-a>3</b><i>after</i>4</p>",
    "<p><b><x-a>z<div>q</div></x-a></b>y</p>",
    "<a><x-a><a>t</a></x-a>",
    "<x-a/><x-b><x-a>n</x-a></x-b>",
    "<template><x-a id=t>c<x-b></x-b></x-a></template><x-a>",
    "<head><x-a>h</x-a><title>t</title>",
    "<x-a>a<x-a>b<x-a>c",
    "<x-a><frameset>",
    "<x-a>text<![CDATA[x]]>&lt;",
    "<html><x-a><body class=b>",
    "<form><x-a><input></x-a></form><x-a><form><input>",
    "<ul><li><x-a><li>",
    "<x-a></x-b></x-a></p>",
    "<x-a a=1 b=2 a=3 xlink:href=u>",
    "<!DOCTYPE html><x-a><table><x-b><tr><x-a>",
];

/// The tree with pauses (constructed elements or placeholders) equals the
/// tree without definitions, for each case, with and without scripting.
#[test]
fn pauses_for_custom_elements_keep_the_tree() {
    let names = ["x-a", "x-b"];
    for text in CASES {
        for scripting in [false, true] {
            let expected = parse_html_with(text, scripting);
            for how in [Construct::Element, Construct::Nothing] {
                let (doc, _) = parse_defined(text, &names, how, scripting);
                assert_eq!(
                    dump_tree(&doc),
                    dump_tree(&expected),
                    "{text} {how:?} {scripting}"
                );
                assert_eq!(
                    outer_html(&doc, NodeId::DOCUMENT),
                    outer_html(&expected, NodeId::DOCUMENT),
                    "{text} {how:?} {scripting}"
                );
            }
        }
    }
}

/// The connected elements with defined names are the constructed ones, and
/// no placeholder is in the tree; the elements in template contents are
/// not constructed (their document has no registry).
#[test]
fn constructed_elements_take_the_place_of_the_placeholders() {
    let text =
        "<x-a id=1 a=b><x-b>t</x-b><template><x-a id=t></x-a></template></x-a><svg><x-a/></svg>";
    let (doc, pauses) = parse_defined(text, &["x-a", "x-b"], Construct::Element, true);
    assert_eq!(pauses, 2);
    let html_named = |doc: &Document, n: NodeId| {
        doc.element(n)
            .is_some_and(|e| e.is_html() && e.name.local.starts_with("x-"))
    };
    let connected: Vec<NodeId> = doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| html_named(&doc, n))
        .collect();
    assert_eq!(connected.len(), 2);
    for n in connected {
        assert_eq!(doc.custom_element_state(n).0, CustomElementState::Custom);
    }
    let x1 = doc.element_by_id("1").expect("x-a#1");
    assert_eq!(doc.element(x1).unwrap().attr("a"), Some("b"));
    let template = doc
        .descendants(NodeId::DOCUMENT)
        .find(|&n| doc.element(n).is_some_and(|e| &*e.name.local == "template"))
        .expect("the template");
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    let inner = doc.node(contents).first_child().expect("x-a#t");
    assert_eq!(doc.element(inner).unwrap().attr("id"), Some("t"));
    assert_eq!(
        doc.custom_element_state(inner).0,
        CustomElementState::Undefined
    );
}

/// The parser pauses after the token of the custom element: at the pause the
/// text that follows is not parsed yet, and the next step goes on with it.
#[test]
fn the_pause_comes_after_the_token() {
    let mut doc = Document::new();
    doc.add_defined_name(LocalName::from("x-a"));
    let mut parser = HtmlParser::new(&mut doc, "<p>a<x-a k=v>b</x-a>c<x-a>", true);
    let ParseStep::CustomElement(placeholder) = parser.resume(&mut doc).unwrap() else {
        panic!("a pause for x-a");
    };
    let p = doc
        .body()
        .and_then(|b| doc.node(b).first_child())
        .expect("p");
    assert_eq!(doc.text_content(p), "a", "nothing after the token");
    let attrs = parser.custom_element_attributes();
    assert_eq!(attrs.len(), 1);
    assert_eq!(&*attrs[0].name.local, "k");
    let element = construct(&mut doc, placeholder, &attrs);
    parser.finish_custom_element(&mut doc, placeholder, Some(element));
    assert_eq!(doc.parent(element), Some(p));
    // The next token goes into the constructed element (the alias).
    let ParseStep::CustomElement(second) = parser.resume(&mut doc).unwrap() else {
        panic!("a pause for the second x-a");
    };
    assert_eq!(doc.text_content(element), "b");
    assert_eq!(doc.text_content(p), "abc");
    parser.finish_custom_element(&mut doc, second, None);
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    assert_eq!(doc.parent(second), Some(p));
}

/// A constructor's script can change the tree: an element that a callback
/// moved elsewhere is moved to its place; a removed parent gets the element
/// and the rest of its content detached; an insertion that would make a
/// cycle is dropped.
#[test]
fn the_recorded_insertion_survives_changes_of_scripts() {
    let mut doc = Document::new();
    doc.add_defined_name(LocalName::from("x-a"));
    let text = "<div id=d><x-a id=1></x-a><span id=s></span></div><i id=last></i><p id=p><x-a id=2>c</x-a></p>";
    let mut parser = HtmlParser::new(&mut doc, text, true);
    // 1: a callback put the element into `head`, and the constructor
    // removed the parent `div`.
    let ParseStep::CustomElement(first) = parser.resume(&mut doc).unwrap() else {
        panic!("x-a#1");
    };
    let attrs = parser.custom_element_attributes();
    let element = construct(&mut doc, first, &attrs);
    let head = doc.head().expect("head");
    doc.append_child(head, element).unwrap();
    let div = doc.element_by_id("d").expect("the div");
    doc.remove(div);
    parser.finish_custom_element(&mut doc, first, Some(element));
    assert_eq!(doc.parent(element), Some(div));
    // 2: the element became an ancestor of its parent `p`.
    let ParseStep::CustomElement(second) = parser.resume(&mut doc).unwrap() else {
        panic!("x-a#2");
    };
    let p = doc.element_by_id("p").expect("p");
    let element2 = construct(&mut doc, second, &parser.custom_element_attributes());
    let body = doc.body().unwrap();
    doc.append_child(body, element2).unwrap();
    doc.append_child(element2, p).unwrap();
    parser.finish_custom_element(&mut doc, second, Some(element2));
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    assert!(
        doc.element_by_id("s").is_none(),
        "the span is in the removed div"
    );
    assert_eq!(
        doc.parent(doc.element_by_id("1").unwrap_or(element)),
        Some(div)
    );
    assert!(doc.element_by_id("last").is_some());
    assert_eq!(doc.parent(element2), Some(body), "no cycle");
    assert_eq!(doc.parent(p), Some(element2));
    assert_eq!(
        doc.text_content(element2),
        "c",
        "the parser goes on in the element"
    );
}

/// A custom element in written text pauses `resume_written`; the written
/// text and the rest of the document follow.
#[test]
fn a_custom_element_in_written_text_pauses_the_write() {
    let mut doc = Document::new();
    doc.add_defined_name(LocalName::from("x-a"));
    let mut parser = HtmlParser::new(&mut doc, "<script>w</script><p id=after>", true);
    let ParseStep::Script(_) = parser.resume(&mut doc).unwrap() else {
        panic!("the script");
    };
    parser.enter_script();
    assert!(parser.write("<x-a id=w a=1>t</x-a><i id=i></i>"));
    let WrittenStep::CustomElement(placeholder) = parser.resume_written(&mut doc).unwrap() else {
        panic!("a pause in the written text");
    };
    assert!(doc.element_by_id("i").is_none(), "the rest waits");
    answer(&mut parser, &mut doc, placeholder, Construct::Element);
    assert_eq!(parser.resume_written(&mut doc), Ok(WrittenStep::Parsed));
    parser.leave_script();
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    let w = doc.element_by_id("w").expect("x-a#w");
    assert_eq!(doc.custom_element_state(w).0, CustomElementState::Custom);
    assert_eq!(doc.element(w).unwrap().attr("a"), Some("1"));
    assert_eq!(doc.text_content(w), "t");
    let ids: Vec<&str> = doc
        .descendants(doc.body().unwrap())
        .filter_map(|n| doc.element(n).and_then(|e| e.attr("id")))
        .collect();
    assert_eq!(ids, ["w", "i", "after"]);
}

/// A mutation sink that asks the parser for a pause at each change that
/// removes a child (as the custom element reactions of a move do).
struct PauseAtRemovals {
    log: Rc<RefCell<Vec<String>>>,
}

impl MutationSink for PauseAtRemovals {
    fn change(&mut self, doc: &Document, change: &Change<'_>) {
        if let Change::Children { removed, .. } = change
            && !removed.is_empty()
        {
            self.log
                .borrow_mut()
                .push(doc.text_content(NodeId::DOCUMENT));
            doc.request_parser_pause();
        }
    }
}

/// A request of the mutation sink pauses the parser after the token (the
/// adoption agency's moves at `</b>`), and the text after it waits.
#[test]
fn the_mutation_sink_can_pause_the_parser_after_a_token() {
    let text = "<b>1<p>2<x-a id=m></x-a>3</b>4</p><b>5</b>";
    let mut doc = Document::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    doc.set_mutation_sink(Some(Box::new(PauseAtRemovals {
        log: Rc::clone(&log),
    })));
    let mut parser = HtmlParser::new(&mut doc, text, true);
    let mut seen = Vec::new();
    loop {
        match parser.resume(&mut doc).unwrap() {
            ParseStep::Reactions => seen.push(doc.text_content(NodeId::DOCUMENT)),
            ParseStep::Finished => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(seen, ["123"], "one pause, after `</b>`, before `4`");
    assert!(!log.borrow().is_empty());
    assert_eq!(dump_tree(&doc), dump_tree(&parse_with_driver(text, true)));
}

/// The parse without definitions never pauses, and the test pages give the
/// same tree with every custom element name of the page defined (each
/// element constructed or not).
#[test]
fn test_pages_with_their_custom_elements_defined_keep_their_trees() {
    let mut pauses = 0;
    for file in test_pages() {
        let bytes = std::fs::read(&file).expect("a test page");
        let (text, _) = decode_html_bytes(&bytes, None);
        let plain = parse_html_with(&text, true);
        let mut names: Vec<String> = (0..plain.len())
            .filter_map(NodeId::from_index)
            .filter(|&n| plain.custom_element_state(n).0 == CustomElementState::Undefined)
            .filter_map(|n| plain.element(n).map(|e| e.name.local.to_string()))
            .collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            continue;
        }
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        for how in [Construct::Element, Construct::Nothing] {
            let (doc, n) = parse_defined(&text, &names, how, true);
            pauses += n;
            assert_eq!(dump_tree(&doc), dump_tree(&plain), "{}", file.display());
        }
    }
    assert!(pauses > 0, "the test pages have custom elements");
}

/// At the node limit the parser stops; a custom element that waits is
/// inserted as it was parsed, and nothing panics.
#[test]
fn custom_elements_at_the_node_limit() {
    let mut doc = Document::with_limits(Limits {
        max_nodes: 40,
        max_text_bytes: 1 << 20,
    });
    doc.add_defined_name(LocalName::from("x-a"));
    let text = "<x-a a=1>".repeat(100);
    let mut parser = HtmlParser::new(&mut doc, &text, true);
    let mut pauses = 0;
    loop {
        match parser.resume(&mut doc).unwrap() {
            ParseStep::CustomElement(placeholder) => {
                pauses += 1;
                answer(&mut parser, &mut doc, placeholder, Construct::Element);
            }
            ParseStep::Finished => break,
            _ => {}
        }
    }
    assert!(parser.stopped_at_limit());
    assert!(pauses > 5 && pauses < 40, "{pauses}");
    assert!(doc.len() < 50, "{}", doc.len());
}

/// A name that is defined but not in the HTML namespace (SVG, `MathML`) or
/// not a custom element name of an HTML element never pauses.
#[test]
fn only_html_elements_with_defined_names_pause() {
    let text = "<svg><x-a></x-a></svg><math><x-a></x-a></math><x-b></x-b>";
    let (_, pauses) = parse_defined(text, &["x-a"], Construct::Element, true);
    assert_eq!(pauses, 0);
    let name = QualName::new(None, ns!(html), LocalName::from("x-a"));
    assert!(!Document::new().is_defined_name(&name.local));
}

/// A foster-parented custom element records "into the parent, before the
/// table". The constructor moves the table elsewhere: the element goes to
/// the end of the recorded parent (the rule of `finish_pending`; Chromium
/// gives the same tree, `rv7-foster-sibling-moved`).
#[test]
fn a_recorded_sibling_that_moved_makes_the_insertion_an_append() {
    let mut doc = Document::new();
    doc.add_defined_name(LocalName::from("x-a"));
    let text = "<div id=d><table id=t><x-a id=1></x-a></table><b id=b></b></div>";
    let mut parser = HtmlParser::new(&mut doc, text, true);
    let ParseStep::CustomElement(placeholder) = parser.resume(&mut doc).unwrap() else {
        panic!("x-a");
    };
    let element = construct(&mut doc, placeholder, &parser.custom_element_attributes());
    let (div, table) = (
        doc.element_by_id("d").unwrap(),
        doc.element_by_id("t").unwrap(),
    );
    let body = doc.body().unwrap();
    doc.append_child(body, table).unwrap();
    parser.finish_custom_element(&mut doc, placeholder, Some(element));
    assert_eq!(doc.parent(element), Some(div), "appended to the parent");
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    assert_eq!(doc.parent(table), Some(body));
    let b = doc.element_by_id("b").unwrap();
    assert_eq!(doc.parent(b), Some(div));
}

/// A script adopts an element that is open in the parser into another
/// document (`take_subtree`: the arena keeps the id as a detached comment).
/// The tree builder goes on with the handle: no panic, nothing written into
/// the comment, the rest of the page is parsed. The content that would go
/// into the element is dropped (Chromium parses it into the element in the
/// other document; ADR 0035 4c notes).
#[test]
fn a_handle_whose_element_left_the_arena_drops_what_the_parser_adds() {
    let cases = [
        "<div id=d><script>s</script><i id=i>t</i>text<x-a id=1>c</x-a></div><p id=after>",
        "<div id=d><script>s</script><template><b>x</b></template><table><tr><x-a>f</x-a><td>c</td></tr></table>t</div><p id=after>",
        "<form id=d><script>s</script><input id=i><p>q<b>r</form><p id=after>",
        "<div id=d><script>s</script><b><i id=i>1<p>2</b>3</div><p id=after>",
        "<div id=d><script>s</script><x-a id=1><x-a id=2>t</x-a></x-a></div><p id=after>",
    ];
    for text in cases {
        for how in [Construct::Element, Construct::Nothing] {
            let mut doc = Document::new();
            doc.add_defined_name(LocalName::from("x-a"));
            let mut parser = HtmlParser::new(&mut doc, text, true);
            let mut adopted = None;
            loop {
                match parser.resume(&mut doc).expect("the parser's document") {
                    ParseStep::Script(_) if adopted.is_none() => {
                        let div = doc.element_by_id("d").expect("the open element");
                        doc.remove(div);
                        drop(doc.take_subtree(div));
                        adopted = Some(div);
                    }
                    ParseStep::Script(_) | ParseStep::Reactions => {}
                    ParseStep::CustomElement(placeholder) => {
                        answer(&mut parser, &mut doc, placeholder, how);
                    }
                    ParseStep::Finished => break,
                }
            }
            let gone = adopted.expect("the script ran");
            assert!(
                matches!(&doc.node(gone).data, NodeData::Comment(c) if c.is_empty()),
                "{text}"
            );
            assert_eq!(doc.children(gone).count(), 0, "{text}");
            assert!(doc.element_by_id("i").is_none(), "{text}");
            assert!(doc.element_by_id("1").is_none(), "{text}");
            let after = doc.element_by_id("after").expect("the page goes on");
            assert_eq!(doc.parent(after), doc.body(), "{text}");
        }
    }
}
