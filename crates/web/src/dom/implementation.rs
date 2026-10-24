//! `DOMImplementation` (DOM §4.5.1,
//! <https://dom.spec.whatwg.org/#interface-domimplementation>),
//! `DOMParser` (<https://w3c.github.io/DOM-Parsing/#the-domparser-interface>)
//! and `HTMLTemplateElement.content`.
//!
//! The documents that these make are more documents of the arena (ADR 0028
//! section 3): the engine renders none of them, their scripts never run
//! (`createHTMLDocument` has no scripts of its own; the scripts that
//! `DOMParser` parses are "already started"), and their `noscript` content
//! is markup ([`swb_dom::Document::scripting_for`]). A `DOMParser` document
//! has the URL of the window's document; a `createHTMLDocument` document
//! is `about:blank`. Departure: `DOMParser` refuses the XML types with a
//! `NotSupportedError` (swb has no XML parser; Chromium returns an
//! `XMLDocument`), and `DOMImplementation` has no `createDocument`.

use swb_dom::{ElementData, NodeData, NodeId, QualName, local_name, ns};
use swb_js::{GenericData, HostData, Tracer};

use super::{dom_result, string_replace_all};
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::SupportedType;
use crate::state::{Slot, WebState, dom, dom_mut, same_object, state_mut};

/// The document of a `DOMImplementation` object (its host data is the
/// node index of the document).
fn document_of(this: Platform) -> b::Result<NodeId> {
    match this.data {
        HostData::Word(index) => usize::try_from(index)
            .ok()
            .and_then(NodeId::from_index)
            .ok_or_else(|| Exception::type_error("Illegal invocation")),
        HostData::Generic(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// `Document.implementation`: the one `DOMImplementation` of the document.
pub(super) fn implementation(
    cx: &mut Cx<'_>,
    document: NodeId,
) -> b::Result<swb_js::Gc<swb_js::Object>> {
    let realm = cx.realm();
    Ok(same_object(cx.rt, document, Slot::Implementation, |rt| {
        rt.new_host_object(
            realm,
            g::DOM_IMPLEMENTATION,
            HostData::Word(document.index() as u64),
        )
    })?)
}

/// A new node of `document`.
fn make(cx: &mut Cx<'_>, document: NodeId, data: NodeData) -> b::Result<NodeId> {
    let result = dom_mut(cx.rt)?.create_node_in(document, data);
    dom_result(cx.rt, result)
}

/// A new HTML element of `document`.
fn make_element(cx: &mut Cx<'_>, document: NodeId, local: swb_dom::LocalName) -> b::Result<NodeId> {
    let name = QualName::new(None, ns!(html), local);
    make(
        cx,
        document,
        NodeData::Element(ElementData::new(name, Vec::new())),
    )
}

/// Appends `child` to `parent` (both new, so no check can fail but the
/// limits).
fn append(cx: &mut Cx<'_>, parent: NodeId, child: NodeId) -> b::Result<()> {
    let result = dom_mut(cx.rt)?.append_child(parent, child);
    dom_result(cx.rt, result)
}

impl g::dom_implementation::DOMImplementationImpl for WebState {
    /// <https://dom.spec.whatwg.org/#dom-domimplementation-createdocumenttype>,
    /// with Chromium's check of the name: no ASCII whitespace, `>` or NUL
    /// (measured; `/` and the other characters pass).
    fn create_document_type(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        public_id: String,
        system_id: String,
    ) -> b::Result<NodeId> {
        let document = document_of(this)?;
        if name
            .chars()
            .any(|c| c.is_ascii_whitespace() || matches!(c, '>' | '\0'))
        {
            return Err(Exception::dom(
                "InvalidCharacterError",
                format!("The provided doctype name ('{name}') contains an invalid character."),
            ));
        }
        make(
            cx,
            document,
            NodeData::Doctype {
                name,
                public_id,
                system_id,
            },
        )
    }

    /// <https://dom.spec.whatwg.org/#dom-domimplementation-createhtmldocument>:
    /// a doctype, `html`, `head` (with a `title` if `title` is given) and
    /// `body`.
    fn create_html_document(
        cx: &mut Cx<'_>,
        _this: Platform,
        title: Option<String>,
    ) -> b::Result<NodeId> {
        let result = dom_mut(cx.rt)?.create_document();
        let document = dom_result(cx.rt, result)?;
        let doctype = make(
            cx,
            document,
            NodeData::Doctype {
                name: "html".to_owned(),
                public_id: String::new(),
                system_id: String::new(),
            },
        )?;
        append(cx, document, doctype)?;
        let html = make_element(cx, document, local_name!("html"))?;
        append(cx, document, html)?;
        let head = make_element(cx, document, local_name!("head"))?;
        append(cx, html, head)?;
        if let Some(title) = title {
            let element = make_element(cx, document, local_name!("title"))?;
            append(cx, head, element)?;
            string_replace_all(cx, element, &title)?;
        }
        let body = make_element(cx, document, local_name!("body"))?;
        append(cx, html, body)?;
        Ok(document)
    }

    fn has_feature(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(true)
    }
}

/// The data of a `DOMParser`: none.
struct ParserData;

impl GenericData for ParserData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

impl g::dom_parser::DOMParserImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let data = cx.rt.alloc_host_data(ParserData)?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    /// <https://w3c.github.io/DOM-Parsing/#dom-domparser-parsefromstring>
    /// for `text/html`: the HTML parser with scripting disabled makes the
    /// document; its scripts are marked "already started".
    fn parse_from_string(
        cx: &mut Cx<'_>,
        _this: Platform,
        string: String,
        type_: SupportedType,
    ) -> b::Result<NodeId> {
        if type_ != SupportedType::TextHtml {
            return Err(Exception::dom(
                "NotSupportedError",
                "swb does not parse XML documents.",
            ));
        }
        let result = swb_dom::parse_document(dom_mut(cx.rt)?, &string);
        let document = dom_result(cx.rt, result)?;
        state_mut(cx.rt)?.parsed_documents.insert(document);
        Ok(document)
    }
}

impl g::html_template_element::HTMLTemplateElementImpl for WebState {
    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-template-content>:
    /// the template contents, a fragment of the inert document.
    fn content(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        dom(cx.rt)?
            .element(this)
            .and_then(|e| e.template_contents)
            .ok_or_else(|| Exception::type_error("the template has no contents"))
    }
}
