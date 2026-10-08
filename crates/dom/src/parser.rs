//! HTML parsing: html5ever's tree builder writes directly into our
//! [`Document`] through the [`TreeSink`] trait.
//!
//! Scripting is reported as disabled to the tree builder, because swb does
//! not run JavaScript. As a result, `<noscript>` content is parsed as normal
//! markup and is rendered, as in a browser with JavaScript turned off.
//!
//! The tree depth is limited to about [`MAX_TREE_DEPTH`], as in Blink:
//! elements that would be deeper are attached to the ancestor at the limit.
//! The limit is not strict: the tree builder can move existing subtrees
//! (adoption agency, reparenting), which can make a few nodes deeper, and
//! template contents are counted from their own fragment. Later stages do
//! not rely on it: style is iterative and box construction has its own
//! depth limit.

use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::collections::HashSet;

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode as HtmlQuirksMode, TreeSink};
use html5ever::{ParseOpts, QualName, parse_document};

use crate::encoding::decode_html;
use crate::tree::{Attribute, Document, NodeData, NodeId, QuirksMode};

/// Parses an HTML document from text.
pub fn parse_html(text: &str) -> Document {
    let opts = ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let sink = Sink::default();
    parse_document(sink, opts).one(StrTendril::from(text))
}

/// Parses an HTML document from bytes. The character encoding is determined
/// from the byte order mark, the transport-layer charset (from the
/// `Content-Type` header), `<meta>` tags, and content sniffing, in that
/// order. Returns the document and the name of the encoding used.
pub fn parse_html_bytes(bytes: &[u8], transport_charset: Option<&str>) -> (Document, &'static str) {
    let (text, encoding) = decode_html(bytes, transport_charset);
    (parse_html(&text), encoding.name())
}

/// The maximum depth of a node below the root of its tree (the document,
/// or a template's content fragment). Blink uses the same limit
/// (`kMaximumHTMLParserDOMTreeDepth`).
pub const MAX_TREE_DEPTH: usize = 512;

/// The tree sink. html5ever's trait takes `&self`, so the document is in a
/// `RefCell`. Every method except `elem_name` releases its borrow before it
/// returns. `elem_name` returns a shared borrow; html5ever drops it before
/// it calls a method that mutates the tree.
#[derive(Default)]
struct Sink {
    doc: RefCell<Document>,
    /// Elements that are `MathML` `annotation-xml` integration points.
    integration_points: RefCell<HashSet<NodeId>>,
}

fn convert_attrs(attrs: Vec<html5ever::Attribute>) -> Vec<Attribute> {
    attrs
        .into_iter()
        .map(|a| Attribute {
            name: a.name,
            value: a.value.to_string(),
        })
        .collect()
}

/// `parent`, or the ancestor of `parent` at depth `MAX_TREE_DEPTH - 1` if a
/// child of `parent` would be deeper than [`MAX_TREE_DEPTH`].
fn depth_limited_parent(doc: &Document, parent: NodeId) -> NodeId {
    let depth = doc.ancestors(parent).count();
    if depth < MAX_TREE_DEPTH {
        return parent;
    }
    doc.ancestors(parent)
        .nth(depth - MAX_TREE_DEPTH)
        .unwrap_or(parent)
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Document;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Document {
        self.doc.into_inner()
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        log::trace!("HTML parse error: {msg}");
    }

    fn get_document(&self) -> NodeId {
        NodeId::DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.doc.borrow(), |doc| {
            &doc.element(*target)
                .expect("html5ever asks for names of elements only")
                .name
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        flags: ElementFlags,
    ) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        let id = doc.create_element(name, convert_attrs(attrs));
        if flags.template {
            let contents = doc.create_node(NodeData::DocumentFragment);
            if let Some(e) = doc.element_mut(id) {
                e.template_contents = Some(contents);
            }
        }
        if flags.mathml_annotation_xml_integration_point {
            self.integration_points.borrow_mut().insert(id);
        }
        id
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.doc
            .borrow_mut()
            .create_node(NodeData::Comment(text.to_string()))
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeId {
        self.doc
            .borrow_mut()
            .create_node(NodeData::ProcessingInstruction {
                target: target.to_string(),
                data: data.to_string(),
            })
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        let mut doc = self.doc.borrow_mut();
        let parent = &depth_limited_parent(&doc, *parent);
        match child {
            NodeOrText::AppendNode(node) => doc.append_child(*parent, node),
            NodeOrText::AppendText(text) => doc.append_text(*parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.doc.borrow().parent(*element).is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut doc = self.doc.borrow_mut();
        let id = doc.create_node(NodeData::Doctype {
            name: name.to_string(),
            public_id: public_id.to_string(),
            system_id: system_id.to_string(),
        });
        doc.append_child(NodeId::DOCUMENT, id);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.doc
            .borrow()
            .element(*target)
            .and_then(|e| e.template_contents)
            .expect("html5ever asks for contents of template elements only")
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: HtmlQuirksMode) {
        self.doc.borrow_mut().quirks_mode = match mode {
            HtmlQuirksMode::Quirks => QuirksMode::Quirks,
            HtmlQuirksMode::LimitedQuirks => QuirksMode::LimitedQuirks,
            HtmlQuirksMode::NoQuirks => QuirksMode::NoQuirks,
        };
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let mut doc = self.doc.borrow_mut();
        match new_node {
            NodeOrText::AppendNode(node) => doc.insert_before(*sibling, node),
            NodeOrText::AppendText(text) => doc.insert_text_before(*sibling, &text),
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<html5ever::Attribute>) {
        let mut doc = self.doc.borrow_mut();
        let Some(element) = doc.element_mut(*target) else {
            return;
        };
        for attr in convert_attrs(attrs) {
            element.add_attr_if_missing(attr);
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.doc.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        self.doc.borrow_mut().reparent_children(*node, *new_parent);
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        self.integration_points.borrow().contains(handle)
    }

    /// Records the form element pointer for a form-associated element, if
    /// the form and the element's intended parent are in the same tree
    /// (the document or one template's contents).
    fn associate_with_form(
        &self,
        target: &NodeId,
        form: &NodeId,
        nodes: (&NodeId, Option<&NodeId>),
    ) {
        let mut doc = self.doc.borrow_mut();
        let root = |doc: &Document, node: NodeId| doc.ancestors(node).last().unwrap_or(node);
        if root(&doc, *form) != root(&doc, *nodes.0) {
            return;
        }
        if let Some(element) = doc.element_mut(*target) {
            element.parser_form = Some(*form);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::dump_tree;

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
}
