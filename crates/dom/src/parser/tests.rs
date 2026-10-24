//! Tests of the parser.

use std::path::{Path, PathBuf};

use html5ever::local_name;

use super::*;
use crate::dump::dump_tree;
use crate::journal::Change;
use crate::serialize::outer_html;
use crate::tree::{Limits, NodeData, QuirksMode};

#[test]
fn builds_implied_elements() {
    let doc = parse_html("<title>T</title><p>Hello <b>world</b>");
    assert_eq!(
        dump_tree(&doc),
        "\
| <html>
|   <head>
|     <title>
|       \"T\"
|   <body>
|     <p>
|       \"Hello \"
|       <b>
|         \"world\"
"
    );
    assert_eq!(
        doc.quirks_mode,
        QuirksMode::Quirks,
        "no doctype means quirks mode"
    );
}

#[test]
fn doctype_sets_no_quirks() {
    let doc = parse_html("<!DOCTYPE html><p>x");
    assert_eq!(doc.quirks_mode, QuirksMode::NoQuirks);
}

#[test]
fn adoption_agency_and_foster_parenting() {
    let doc = parse_html("<!DOCTYPE html><table><tr><td>a</td></tr>x</table><b>1<p>2</b>3");
    let dump = dump_tree(&doc);
    // "x" is foster-parented before the table.
    assert!(dump.contains("|     \"x\"\n|     <table>"), "{dump}");
    // The adoption agency algorithm splits <b> around <p>.
    assert!(dump.contains("|       <b>\n|         \"2\""), "{dump}");
}

#[test]
fn noscript_content_is_parsed_as_markup() {
    let doc = parse_html("<!DOCTYPE html><body><noscript><p>no js</p></noscript>");
    assert!(dump_tree(&doc).contains("<noscript>\n|       <p>"));
}

#[test]
fn noscript_is_raw_text_with_scripting() {
    let html = "<!DOCTYPE html><head><noscript><link rel=stylesheet href=a.css><style>p{}</style></noscript></head>\
                <body><noscript><p>no js</p><img src=x></noscript><p id=after>";
    let tags = |doc: &Document| {
        doc.descendants(NodeId::DOCUMENT)
            .filter_map(|n| doc.element(n).map(|e| e.local_name().to_string()))
            .collect::<Vec<_>>()
    };
    let on = parse_html_with(html, true);
    assert!(on.scripting());
    assert_eq!(
        tags(&on),
        ["html", "head", "noscript", "body", "noscript", "p"]
    );
    let dump = dump_tree(&on);
    assert!(dump.contains("<noscript>\n|       \"<p>no js</p><img src=x>\""));
    // The flag is a property of the document: off by default.
    let off = parse_html(html);
    assert!(!off.scripting());
    let tags_off = tags(&off);
    assert!(tags_off.contains(&"link".to_owned()), "{tags_off:?}");
    assert!(tags_off.contains(&"img".to_owned()), "{tags_off:?}");
    let (from_bytes, _) = parse_html_bytes_with(html.as_bytes(), None, true);
    assert!(from_bytes.scripting());
}

#[test]
fn template_contents_go_into_fragment() {
    let doc = parse_html("<!DOCTYPE html><template><p>t</p></template>");
    let template = doc
        .find_element(NodeId::DOCUMENT, |e| e.local_name() == "template")
        .unwrap();
    assert_eq!(doc.children(template).count(), 0);
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    assert_eq!(doc.text_content(contents), "t");
}

#[test]
fn controls_in_tables_belong_to_the_open_form() {
    let doc = parse_html("<table><form id=f><tr><td><input id=i></td></tr></form></table>");
    let form = doc.element_by_id("f").expect("form");
    let input = doc.element_by_id("i").expect("input");
    assert!(!doc.ancestors(input).any(|a| a == form));
    assert_eq!(doc.element(input).and_then(|e| e.parser_form), Some(form));
}

#[test]
fn svg_hrefs_and_the_id_index() {
    let doc = parse_html(
        "<svg><use id=a href='#x' xlink:href='#y'/><use id=b xlink:href='#y'/><use id=c /></svg>\
         <p id=x></p><p id=x></p>",
    );
    let href = |id: &str| {
        doc.element(doc.element_by_id(id).unwrap())
            .unwrap()
            .svg_href()
            .map(str::to_owned)
    };
    // `href` wins over `xlink:href`.
    assert_eq!(href("a").as_deref(), Some("#x"));
    assert_eq!(href("b").as_deref(), Some("#y"));
    assert_eq!(href("c"), None);
    // The index has the first element with each id.
    let ids = doc.element_ids();
    assert_eq!(ids.get("x"), doc.element_by_id("x").as_ref());
    assert_eq!(ids.len(), 4);
}

#[test]
fn attributes_are_kept() {
    let doc = parse_html("<!DOCTYPE html><a href='/x' class='c d'>link</a>");
    let a = doc
        .find_element(NodeId::DOCUMENT, |e| e.local_name() == "a")
        .unwrap();
    let e = doc.element(a).unwrap();
    assert_eq!(e.attr("href"), Some("/x"));
    assert!(e.has_class("d"));
}

#[test]
fn tree_depth_is_limited() {
    let html = format!("<!DOCTYPE html><body>{}x", "<div>".repeat(2000));
    let doc = parse_html(&html);
    let depth = |n: NodeId| doc.ancestors(n).count();
    let deepest = doc.descendants(NodeId::DOCUMENT).map(depth).max().unwrap();
    assert_eq!(deepest, MAX_TREE_DEPTH);
    // No content is lost: all elements and the text are in the tree.
    let divs = doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| doc.is_html_element(n, &html5ever::local_name!("div")))
        .count();
    assert_eq!(divs, 2000);
    assert_eq!(doc.text_content(NodeId::DOCUMENT), "x");
}

#[test]
fn bytes_with_meta_charset() {
    let bytes = b"<!DOCTYPE html><meta charset=windows-1250><p>\x9a";
    let (doc, encoding) = parse_html_bytes(bytes, None);
    assert_eq!(encoding, "windows-1250");
    assert!(doc.text_content(NodeId::DOCUMENT).contains('š'));
}

// ----- The resumable parser -----

/// Parses with html5ever's own driver into the same sink, in one call: the
/// parse as it was before the resumable parser.
fn parse_with_driver(text: &str, scripting: bool) -> Document {
    use html5ever::tendril::TendrilSink;
    let mut doc = Document::new();
    doc.set_scripting(scripting);
    let opts = html5ever::ParseOpts {
        tree_builder: TreeBuilderOpts {
            scripting_enabled: scripting,
            ..Default::default()
        },
        ..Default::default()
    };
    let sink = Sink::new(doc, Rc::new(BufferQueue::default()));
    html5ever::parse_document(sink, opts).one(StrTendril::from(text))
}

/// Parses with a pause at each script end tag. At each pause the document
/// leaves the parser (it is moved and serialized, as a script would use
/// it) and comes back. Returns the document and the number of pauses.
fn parse_with_pauses(text: &str, scripting: bool) -> (Document, usize) {
    let mut doc = Document::new();
    let mut parser = HtmlParser::new(&mut doc, text, scripting);
    let mut pauses = 0;
    while let ParseStep::Script(script) = parser.resume(&mut doc).expect("the parser's document") {
        pauses += 1;
        assert!(doc.element(script).is_some(), "a script element");
        let lent = std::mem::take(&mut doc);
        assert_ne!(outer_html(&lent, NodeId::DOCUMENT), "");
        doc = lent;
    }
    assert!(parser.is_finished());
    (doc, pauses)
}

/// The HTML files of the page fixtures and the layout tests.
fn test_pages() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    let pages = std::fs::read_dir(root.join("fixtures/pages")).expect("fixtures/pages");
    for page in pages.flatten() {
        if let Ok(entries) = std::fs::read_dir(page.path().join("files")) {
            files.extend(entries.flatten().map(|e| e.path()));
        }
    }
    let layout = std::fs::read_dir(root.join("tests/layout")).expect("tests/layout");
    files.extend(layout.flatten().map(|e| e.path()));
    files.retain(|f| f.extension().is_some_and(|e| e == "html"));
    files.sort();
    assert!(files.len() > 100, "found the test pages");
    files
}

/// Checks that replaying the journal of a parse gives the parent of every
/// node of the arena: each node is listed once as added to its parent (and
/// once as removed and added again for each move).
fn check_journal(doc: &Document) {
    let journal = doc.journal();
    assert!(!journal.incomplete());
    let mut parents = std::collections::HashMap::<NodeId, NodeId>::new();
    for change in journal.iter() {
        match change {
            Change::Children {
                parent,
                added,
                removed,
                ..
            } => {
                for node in removed {
                    assert_eq!(parents.remove(node), Some(parent), "{node:?} was a child");
                }
                for &node in added {
                    assert_eq!(parents.insert(node, parent), None, "{node:?} was detached");
                }
            }
            Change::Text { node, .. } => {
                assert!(matches!(doc.node(node).data, NodeData::Text(_)));
            }
            Change::Attribute { element, .. } => assert!(doc.element(element).is_some()),
        }
    }
    for index in 0..doc.len() {
        let node = NodeId::from_index(index).expect("an index of the arena");
        assert_eq!(parents.get(&node).copied(), doc.parent(node), "{node:?}");
    }
}

#[test]
fn pauses_and_one_parse_give_the_same_tree_for_all_test_pages() {
    let mut pauses = 0;
    for file in test_pages() {
        let bytes = std::fs::read(&file).expect("a test page");
        let (text, _) = decode_html_bytes(&bytes, None);
        for scripting in [false, true] {
            let expected = parse_with_driver(&text, scripting);
            let (paused, n) = parse_with_pauses(&text, scripting);
            pauses += n;
            let whole = parse_html_with(&text, scripting);
            for doc in [&paused, &whole] {
                assert_eq!(dump_tree(doc), dump_tree(&expected), "{}", file.display());
                assert_eq!(
                    outer_html(doc, NodeId::DOCUMENT),
                    outer_html(&expected, NodeId::DOCUMENT),
                    "{}",
                    file.display()
                );
                assert_eq!(doc.quirks_mode, expected.quirks_mode);
                assert_eq!(doc.len(), expected.len(), "the same nodes");
            }
            // The journal of the parser lists every node; the parse without
            // pauses records nothing.
            check_journal(&paused);
            assert!(whole.journal().incomplete());
            assert_eq!(whole.journal().len(), 0);
        }
    }
    assert!(pauses > 100, "the fixtures have scripts: {pauses}");
}

#[test]
fn the_parser_pauses_at_script_end_tags() {
    let html = "<!DOCTYPE html><p id=a>a<script id=s1>one()</script><p id=b>b\
                <svg><script id=s2>two()</script></svg><script id=s3></script>";
    let mut doc = Document::new();
    let mut parser = HtmlParser::new(&mut doc, html, true);
    let step = parser.resume(&mut doc).expect("the parser's document");
    let s1 = doc.element_by_id("s1").expect("the first script");
    assert_eq!(step, ParseStep::Script(s1));
    assert_eq!(doc.text_content(s1), "one()");
    // The rest is not parsed yet.
    assert!(doc.element_by_id("b").is_none());
    let mut scripts = vec![s1];
    while let ParseStep::Script(s) = parser.resume(&mut doc).expect("the parser's document") {
        scripts.push(s);
    }
    let id = |name: &str| doc.element_by_id(name).expect("element");
    // The SVG script is not an HTML script: html5ever does not pause there.
    assert_eq!(scripts, [s1, id("s3")]);
    assert_eq!(
        parser.resume(&mut doc),
        Ok(ParseStep::Finished),
        "finished stays finished"
    );
}

#[test]
fn the_parser_continues_into_nodes_removed_meanwhile() {
    let html = "<body><div id=d><script></script>after</div><p id=p>";
    let mut doc = Document::new();
    let mut parser = HtmlParser::new(&mut doc, html, true);
    let ParseStep::Script(_) = parser.resume(&mut doc).expect("the parser's document") else {
        panic!("a pause at the script");
    };
    let div = doc.element_by_id("d").expect("div");
    doc.remove(div);
    while parser.resume(&mut doc) != Ok(ParseStep::Finished) {}
    // The text goes into the open (removed) div; the paragraph closes it.
    assert_eq!(doc.text_content(div), "after");
    assert!(!doc.is_connected(div));
    assert!(doc.element_by_id("p").is_some());
    assert_eq!(doc.element_by_id("d"), None, "removed from the id index");
}

#[test]
fn the_parser_refuses_another_document() {
    let mut doc = Document::new();
    let mut parser = HtmlParser::new(&mut doc, "<p>x", false);
    let mut other = Document::new();
    assert_eq!(parser.resume(&mut other), Err(DomError::WrongDocument));
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    assert_eq!(doc.text_content(NodeId::DOCUMENT), "x");
}

#[test]
fn the_parser_reports_to_the_mutation_sink() {
    #[derive(Default)]
    struct Count(Rc<std::cell::Cell<usize>>);
    impl crate::MutationSink for Count {
        fn change(&mut self, _doc: &Document, _change: &Change<'_>) {
            self.0.set(self.0.get() + 1);
        }
    }
    let count = Count::default();
    let seen = Rc::clone(&count.0);
    let mut doc = Document::new();
    doc.set_mutation_sink(Some(Box::new(count)));
    let mut parser = HtmlParser::new(&mut doc, "<p>a<b>b</b>c<!--x--><i id=x>", false);
    while parser.resume(&mut doc) != Ok(ParseStep::Finished) {}
    assert_eq!(seen.get(), doc.journal().len());
    assert_eq!(doc.generation(), doc.journal().len() as u64);
    assert!(seen.get() >= 9, "{}", seen.get());
}

#[test]
fn the_parser_stops_at_the_node_limit() {
    let mut doc = Document::with_limits(Limits {
        max_nodes: 100,
        max_text_bytes: 1 << 20,
    });
    let html = "<p>x".repeat(1000);
    let mut parser = HtmlParser::new(&mut doc, &html, false);
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    assert!(parser.stopped_at_limit());
    assert!((100..110).contains(&doc.len()), "{}", doc.len());
    // The document is complete up to the stop.
    let body = doc.body().expect("body");
    assert!(doc.element_children(body).count() > 40);
    check_journal(&doc);
}

#[test]
fn the_parser_stops_at_the_byte_limit() {
    // Each paragraph clones the formatting element with its 1,000-byte
    // attribute (the reconstruction of the active formatting elements).
    let html = format!(
        "<p><b title='{}'></p>{}",
        "t".repeat(1000),
        "<p>x</p>".repeat(1000)
    );
    let mut doc = Document::with_limits(Limits {
        max_nodes: 1 << 20,
        max_text_bytes: 50_000,
    });
    let mut parser = HtmlParser::new(&mut doc, &html, false);
    assert_eq!(parser.resume(&mut doc), Ok(ParseStep::Finished));
    assert!(parser.stopped_at_limit());
    assert!(
        (50_000..52_000).contains(&doc.text_bytes()),
        "{}",
        doc.text_bytes()
    );
    // Without the limit, all 1,000 copies exist.
    let all = parse_html(&html);
    assert!(all.text_bytes() > 1_000_000);
}

#[test]
fn template_contents_belong_to_an_inert_document() {
    let doc = parse_html("<!DOCTYPE html><template id=t><p id=p>t</p></template>");
    let template = doc.element_by_id("t").expect("template");
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    let p = doc.children(contents).next().expect("p");
    let inert = doc.node_document(contents);
    assert_ne!(inert, NodeId::DOCUMENT);
    assert_eq!(doc.node_document(p), inert);
    assert_eq!(doc.node_document(inert), inert);
    assert_eq!(doc.node_document(template), NodeId::DOCUMENT);
    // Not connected, so not in the id index.
    assert_eq!(doc.element_by_id("p"), None);
}

// ----- Fragment parsing -----

fn fragment_html(doc: &Document, fragment: NodeId) -> String {
    doc.children(fragment).map(|c| outer_html(doc, c)).collect()
}

#[test]
fn fragments_follow_the_context() {
    let mut doc = parse_html(
        "<!DOCTYPE html><table><tr id=tr><td id=td></td></tr></table>\
         <textarea id=ta></textarea><form id=f><div id=in-form></div></form><div id=div></div>",
    );
    let cases = [
        ("div", "<p>a<b>b</p>c", "<p>a<b>b</b></p><b>c</b>"),
        ("tr", "<td>x</td><th>y", "<td>x</td><th>y</th>"),
        ("td", "<td>x</td><p>y", "x<p>y</p>"),
        ("ta", "<p>x</p>", "&lt;p&gt;x&lt;/p&gt;"),
        // A form inside a form is ignored.
        ("in-form", "<form><input></form>", "<input>"),
        ("div", "<form><input></form>", "<form><input></form>"),
    ];
    for (context, markup, expected) in cases {
        let context = doc.element_by_id(context).expect("context");
        let fragment = parse_fragment(&mut doc, context, markup).expect("fits");
        assert_eq!(fragment_html(&doc, fragment), expected, "{markup}");
        assert!(matches!(
            doc.node(fragment).data,
            NodeData::DocumentFragment
        ));
        assert_eq!(doc.parent(fragment), None);
        assert_eq!(doc.node_document(fragment), NodeId::DOCUMENT);
    }
    let text = doc.create_text("x").unwrap();
    assert_eq!(
        parse_fragment(&mut doc, text, "<p>"),
        Err(DomError::InvalidNodeType)
    );
}

#[test]
fn noscript_is_markup_unless_scripting_is_enabled_for_the_context() {
    let mut doc = parse_html_with(
        "<!DOCTYPE html><div id=d></div><template id=t></template>",
        true,
    );
    let markup = "<noscript><b>x</b></noscript>";
    for (context, text_child) in [("d", true), ("t", false)] {
        let context = doc.element_by_id(context).expect("context");
        let fragment = parse_fragment(&mut doc, context, markup).expect("fits");
        let noscript = doc.children(fragment).next().expect("noscript");
        let child = doc.children(noscript).next().expect("child");
        assert_eq!(
            matches!(doc.node(child).data, NodeData::Text(_)),
            text_child,
            "{context:?}"
        );
    }
    // A document that scripts make has no scripting.
    let other = doc.create_document().unwrap();
    assert!(doc.scripting_for(NodeId::DOCUMENT));
    assert!(!doc.scripting_for(other));
}

#[test]
fn a_document_parse_makes_a_new_document_of_the_arena() {
    let mut doc = parse_html("<!DOCTYPE html><p id=main>x</p>");
    doc.take_journal();
    let before = doc.len();
    let document = parse_document(
        &mut doc,
        "<p id=main class=A>y<script>1</script><noscript><i>n</i></noscript><template><b>t</b></template>",
    )
    .unwrap();
    assert!(doc.len() > before);
    assert!(matches!(doc.node(document).data, NodeData::Document));
    assert_eq!(doc.node_document(document), document);
    // Without a doctype the new document is in quirks mode; the main
    // document keeps its own.
    assert_eq!(doc.quirks_mode_of(document), QuirksMode::Quirks);
    assert_eq!(doc.quirks_mode_of(NodeId::DOCUMENT), QuirksMode::NoQuirks);
    // The ids of the two documents are separate.
    let main = doc.get_element_by_id(NodeId::DOCUMENT, "main").unwrap();
    let other = doc.get_element_by_id(document, "main").unwrap();
    assert_ne!(main, other);
    assert_eq!(doc.node_document(other), document);
    assert_eq!(doc.count_elements_by_id(NodeId::DOCUMENT, "main", 5), 1);
    // Scripts are marked "already started", `noscript` is markup, and a
    // template has contents of the inert document.
    let script = doc
        .descendants(document)
        .find(|&n| doc.is_html_element(n, &local_name!("script")))
        .unwrap();
    assert!(doc.element(script).unwrap().script_already_started);
    let noscript = doc
        .descendants(document)
        .find(|&n| doc.is_html_element(n, &local_name!("noscript")))
        .unwrap();
    assert!(
        doc.node(doc.node(noscript).first_child().unwrap())
            .is_element()
    );
    let template = doc
        .descendants(document)
        .find(|&n| doc.is_html_element(n, &local_name!("template")))
        .unwrap();
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    assert_eq!(doc.children(contents).count(), 1);
    assert_eq!(
        crate::serialize::inner_html(&doc, template),
        "<b>t</b>",
        "the contents"
    );
}

#[test]
fn a_document_parse_respects_the_limits() {
    let mut doc = Document::with_limits(Limits {
        max_nodes: 100,
        max_text_bytes: 1 << 20,
    });
    let len = doc.len();
    assert_eq!(
        parse_document(&mut doc, &"<i></i>".repeat(500)),
        Err(DomError::NodeLimit(100))
    );
    assert_eq!(doc.len(), len, "nothing was added");
    assert!(parse_document(&mut doc, &"<i></i>".repeat(20)).is_ok());
}

#[test]
fn fragment_nodes_are_new_journaled_nodes_of_the_arena() {
    let mut doc = parse_html("<div id=d></div>");
    let div = doc.element_by_id("d").unwrap();
    doc.take_journal();
    let before = doc.len();
    let fragment =
        parse_fragment(&mut doc, div, "<i id=x>a</i><template><b>t</b></template>").unwrap();
    assert!(fragment.index() >= before);
    let top: Vec<NodeId> = doc.children(fragment).collect();
    assert_eq!(top.len(), 2);
    let journal = doc.take_journal();
    let added: Vec<NodeId> = journal
        .iter()
        .filter_map(|c| match c {
            Change::Children { parent, added, .. } if parent == fragment => Some(added.to_vec()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(added, top);
    // The template got its contents, in the inert document.
    let template = top[1];
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    assert_eq!(fragment_html(&doc, contents), "<b>t</b>");
    assert_ne!(doc.node_document(contents), NodeId::DOCUMENT);
    // The fragment is detached: its ids are not in the index until it is
    // inserted.
    assert_eq!(doc.element_by_id("x"), None);
    doc.append_child(div, fragment).unwrap();
    assert_eq!(doc.element_by_id("x"), Some(top[0]));
    assert_eq!(doc.children(fragment).count(), 0);
}

#[test]
fn fragment_parsing_respects_the_limits() {
    let mut doc = Document::with_limits(Limits {
        max_nodes: 200,
        max_text_bytes: 1 << 20,
    });
    let mut parser = HtmlParser::new(&mut doc, "<div id=d></div>", false);
    while parser.resume(&mut doc) != Ok(ParseStep::Finished) {}
    let div = doc.element_by_id("d").unwrap();
    let len = doc.len();
    assert_eq!(
        parse_fragment(&mut doc, div, &"<i></i>".repeat(500)),
        Err(DomError::NodeLimit(200))
    );
    assert_eq!(doc.len(), len, "nothing was added");
    assert!(parse_fragment(&mut doc, div, &"<i></i>".repeat(50)).is_ok());
}

#[test]
fn script_positions_count_from_the_document() {
    // As in the case `errors-inline-positions` of
    // tools/probes/web-scripts.json: the start of each script's text (line
    // and column from 0; also an empty one). html5ever does not pause at
    // an SVG `script`.
    let text = "<!DOCTYPE html><head><script>var a</script></head><body>\n<p>x</p>   <script>throw 1</script>\n\
                <script>\n  var a = 1;\n</script><script>f()</script>\n<script>  }</script><script src=x></script>\
                <svg><script>s</script></svg><script></script>";
    let mut doc = Document::new();
    let mut parser = HtmlParser::new(&mut doc, text, true);
    let mut found = Vec::new();
    while let ParseStep::Script(_) = parser.resume(&mut doc).expect("the parser's document") {
        let p = parser.script_position();
        found.push((p.line, p.column));
    }
    assert_eq!(
        found,
        [(0, 29), (1, 19), (2, 8), (4, 17), (5, 8), (5, 34), (5, 80)]
    );
    // Without the scripting flag the parser does not keep the text.
    let mut doc = Document::new();
    let mut parser = HtmlParser::new(&mut doc, text, false);
    while let ParseStep::Script(_) = parser.resume(&mut doc).expect("the parser's document") {
        assert_eq!(parser.script_position(), TextPosition::default());
    }
}
