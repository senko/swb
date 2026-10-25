//! `Document` (DOM §4.5, <https://dom.spec.whatwg.org/#interface-document>)
//! with the tree accessors of HTML §3.1.4
//! (<https://html.spec.whatwg.org/multipage/dom.html#dom-tree-accessors>):
//! `title`, `body` and `head`; `readyState` and `currentScript` (feature
//! 5c, `scripts.rs`); the node factories, `importNode`, `adoptNode`,
//! `getElementById`, `referrer` and `defaultView` (feature 6a).
//!
//! Only the document of `NodeId::DOCUMENT` has a URL of its own and a
//! script state; the other documents of the arena (the inert documents of
//! template contents, `createHTMLDocument`, `DOMParser`) are UTF-8 and
//! `text/html`, have their own quirks mode, and are `about:blank` except
//! those of `DOMParser`, which have the URL of the window's document.
//! Feature 6b adds the queries, the document collections and
//! `implementation`.

use swb_dom::{Document, ElementData, NodeData, NodeId, QualName, QuirksMode, local_name, ns};

use super::attrs::{self, validate_and_extract};
use super::lists::{self, DocCollection};
use super::{dom_result, string_replace_all};
use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::generated::types::DomStringOrElementCreationOptions;
use crate::state::{WebState, dom, dom_mut, engine, state};

/// The document element of the document `doc`: its element child.
fn document_element(dom: &Document, doc: NodeId) -> Option<NodeId> {
    dom.element_children(doc).next()
}

/// "The html element of a document": the document element if it is an
/// HTML `html` element.
fn html_element(dom: &Document, doc: NodeId) -> Option<NodeId> {
    document_element(dom, doc).filter(|&e| dom.is_html_element(e, &local_name!("html")))
}

/// The first child of the html element that is a `body` or `frameset`
/// element ("the body element").
fn body_element(dom: &Document, doc: NodeId) -> Option<NodeId> {
    let html = html_element(dom, doc)?;
    dom.element_children(html).find(|&c| {
        dom.is_html_element(c, &local_name!("body"))
            || dom.is_html_element(c, &local_name!("frameset"))
    })
}

/// The first `head` child of the html element ("the head element").
fn head_element(dom: &Document, doc: NodeId) -> Option<NodeId> {
    let html = html_element(dom, doc)?;
    dom.element_children(html)
        .find(|&c| dom.is_html_element(c, &local_name!("head")))
}

/// True if the document element of `doc` is an SVG `svg` element.
fn svg_root(dom: &Document, doc: NodeId) -> Option<NodeId> {
    document_element(dom, doc).filter(|&e| {
        dom.element(e)
            .is_some_and(|d| d.name.ns == ns!(svg) && d.name.local == local_name!("svg"))
    })
}

/// "The title element": the first HTML `title` element of the document in
/// tree order, or, under an SVG root, its first SVG `title` child.
fn title_element(dom: &Document, doc: NodeId) -> Option<NodeId> {
    if let Some(svg) = svg_root(dom, doc) {
        return dom.element_children(svg).find(|&c| {
            dom.element(c)
                .is_some_and(|d| d.name.ns == ns!(svg) && d.name.local == local_name!("title"))
        });
    }
    dom.descendants(doc)
        .find(|&n| dom.is_html_element(n, &local_name!("title")))
}

/// The child text content of a node: the data of its text children.
fn child_text_content(dom: &Document, node: NodeId) -> String {
    dom.children(node)
        .filter_map(|c| dom.node(c).as_text())
        .collect()
}

/// "Strip and collapse ASCII whitespace".
fn strip_and_collapse(text: &str) -> String {
    text.split(|c: char| c.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The document of `this` when it is the main document (only that one has
/// a URL, an encoding and a content type).
fn is_main(this: NodeId) -> bool {
    this == NodeId::DOCUMENT
}

impl g::document::DocumentImpl for WebState {
    fn active_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        crate::html::focus::active_element(cx, this)
    }

    fn has_focus(_cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(crate::html::focus::has_focus(this))
    }

    fn url(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        if !is_main(this) && !state(cx.rt)?.parsed_documents.contains(&this) {
            return Ok("about:blank".to_owned());
        }
        Ok(engine(cx.rt)?.url())
    }

    fn document_uri(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Self::url(cx, this)
    }

    fn compat_mode(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let quirks = dom(cx.rt)?.quirks_mode_of(this) == QuirksMode::Quirks;
        Ok(if quirks { "BackCompat" } else { "CSS1Compat" }.to_owned())
    }

    fn character_set(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        if !is_main(this) {
            return Ok("UTF-8".to_owned());
        }
        Ok(engine(cx.rt)?.character_set().to_owned())
    }

    fn charset(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Self::character_set(cx, this)
    }

    fn input_encoding(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Self::character_set(cx, this)
    }

    fn content_type(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        if !is_main(this) {
            return Ok("text/html".to_owned());
        }
        Ok(engine(cx.rt)?.content_type().to_owned())
    }

    fn doctype(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let dom = dom(cx.rt)?;
        Ok(dom
            .children(this)
            .find(|&c| matches!(dom.node(c).data, NodeData::Doctype { .. })))
    }

    fn document_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(document_element(dom(cx.rt)?, this))
    }

    /// <https://html.spec.whatwg.org/multipage/dom.html#document.title>
    fn title(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let dom = dom(cx.rt)?;
        Ok(title_element(dom, this)
            .map(|t| strip_and_collapse(&child_text_content(dom, t)))
            .unwrap_or_default())
    }

    /// The setter of `document.title`: the text of the title element,
    /// which the document creates if it has none (in the head of an HTML
    /// document, as the first child of an SVG root).
    fn set_title(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        let dom = dom(cx.rt)?;
        let Some(root) = document_element(dom, this) else {
            return Ok(());
        };
        let svg = svg_root(dom, this).is_some();
        let html = dom.element(root).is_some_and(|e| e.name.ns == ns!(html));
        let element = match title_element(dom, this) {
            Some(title) if svg || html => title,
            Some(_) | None if !svg && !html => return Ok(()),
            _ if svg => {
                let first = dom.node(root).first_child();
                let name = QualName::new(None, ns!(svg), local_name!("title"));
                let title = create_element(cx, this, name)?;
                let result = dom_mut(cx.rt)?.insert_before(root, title, first);
                dom_result(cx.rt, result)?;
                title
            }
            _ => {
                let Some(head) = head_element(dom, this) else {
                    return Ok(());
                };
                let name = QualName::new(None, ns!(html), local_name!("title"));
                let title = create_element(cx, this, name)?;
                let result = dom_mut(cx.rt)?.append_child(head, title);
                dom_result(cx.rt, result)?;
                title
            }
        };
        string_replace_all(cx, element, &value)
    }

    fn body(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(body_element(dom(cx.rt)?, this))
    }

    /// <https://html.spec.whatwg.org/multipage/dom.html#dom-document-body>,
    /// with Chromium's messages (`tools/probes/web-dom-tree.json`).
    fn set_body(cx: &mut Cx<'_>, this: NodeId, value: Option<NodeId>) -> b::Result<()> {
        let dom = dom(cx.rt)?;
        let Some(value) = value.filter(|&v| {
            dom.is_html_element(v, &local_name!("body"))
                || dom.is_html_element(v, &local_name!("frameset"))
        }) else {
            return Err(match value {
                Some(v) => Exception::dom(
                    "HierarchyRequestError",
                    format!(
                        "The new body element is of type '{}'. It must be either a 'BODY' or 'FRAMESET' element.",
                        super::tag_name(&dom.node(v).data)
                    ),
                ),
                None => Exception::dom(
                    "HierarchyRequestError",
                    "The 1st argument provided is either null, or an invalid HTMLElement object.",
                ),
            });
        };
        let old = body_element(dom, this);
        if old == Some(value) {
            return Ok(());
        }
        let (parent, result) = match (old, document_element(dom, this)) {
            (Some(old), _) => match dom.parent(old) {
                Some(parent) => (parent, dom_mut(cx.rt)?.replace_child(parent, value, old)),
                None => return Ok(()),
            },
            (None, None) => {
                return Err(Exception::dom(
                    "HierarchyRequestError",
                    "No document element exists.",
                ));
            }
            (None, Some(root)) => (root, dom_mut(cx.rt)?.append_child(root, value)),
        };
        dom_result(cx.rt, result)?;
        Ok(crate::scripts::after_insertion(cx.rt, parent, &[value])?)
    }

    fn head(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(head_element(dom(cx.rt)?, this))
    }

    fn ready_state(
        cx: &mut Cx<'_>,
        this: NodeId,
    ) -> b::Result<crate::generated::types::DocumentReadyState> {
        Ok(crate::scripts::ready_state(cx.rt, this)?)
    }

    fn current_script(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        if !is_main(this) {
            return Ok(None);
        }
        Ok(crate::scripts::scripts(cx.rt)?.current_script)
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createelement>, for an
    /// HTML document: the name in ASCII lowercase, an element of the HTML
    /// namespace. Chromium puts the `is` option into an `is` attribute.
    fn create_element(
        cx: &mut Cx<'_>,
        this: NodeId,
        local_name: String,
        _options: DomStringOrElementCreationOptions,
    ) -> b::Result<NodeId> {
        if !is_valid_element_local_name(&local_name) {
            return Err(Exception::dom(
                "InvalidCharacterError",
                format!("The tag name provided ('{local_name}') is not a valid name."),
            ));
        }
        let name = QualName::new(
            None,
            ns!(html),
            swb_dom::LocalName::from(local_name.to_ascii_lowercase()),
        );
        // The `is` option is accepted and ignored: Chromium sets no attribute.
        create_element(cx, this, name)
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createelementns>
    fn create_element_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        qualified_name: String,
        _options: DomStringOrElementCreationOptions,
    ) -> b::Result<NodeId> {
        let parts = validate_and_extract(namespace, &qualified_name, false)?;
        let name = QualName::new(
            parts.prefix.map(Into::into),
            swb_dom::Namespace::from(parts.namespace.unwrap_or_default()),
            swb_dom::LocalName::from(parts.local),
        );
        // The `is` option is accepted and ignored: Chromium sets no attribute.
        create_element(cx, this, name)
    }

    fn create_document_fragment(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        let result = dom_mut(cx.rt)?.create_node_in(this, NodeData::DocumentFragment);
        dom_result(cx.rt, result)
    }

    fn create_text_node(cx: &mut Cx<'_>, this: NodeId, data: String) -> b::Result<NodeId> {
        let result = dom_mut(cx.rt)?.create_node_in(this, NodeData::Text(data));
        dom_result(cx.rt, result)
    }

    fn create_comment(cx: &mut Cx<'_>, this: NodeId, data: String) -> b::Result<NodeId> {
        let result = dom_mut(cx.rt)?.create_node_in(this, NodeData::Comment(data));
        dom_result(cx.rt, result)
    }

    /// <https://dom.spec.whatwg.org/#dom-document-importnode>
    fn import_node(
        cx: &mut Cx<'_>,
        this: NodeId,
        node: NodeId,
        subtree: bool,
    ) -> b::Result<NodeId> {
        if matches!(dom(cx.rt)?.node(node).data, NodeData::Document) {
            return Err(Exception::dom(
                "NotSupportedError",
                "The node provided is a document, which may not be imported.",
            ));
        }
        super::clone(cx, node, this, subtree)
    }

    /// <https://dom.spec.whatwg.org/#dom-document-adoptnode>
    fn adopt_node(cx: &mut Cx<'_>, this: NodeId, node: NodeId) -> b::Result<NodeId> {
        if matches!(dom(cx.rt)?.node(node).data, NodeData::Document) {
            return Err(Exception::dom(
                "NotSupportedError",
                "The node provided is of type '#document', which may not be adopted.",
            ));
        }
        let result = dom_mut(cx.rt)?.adopt(node, this);
        dom_result(cx.rt, result)?;
        Ok(node)
    }

    fn create_attribute(
        cx: &mut Cx<'_>,
        _this: NodeId,
        local_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        attrs::create_attribute(cx, &local_name)
    }

    fn create_attribute_ns(
        cx: &mut Cx<'_>,
        _this: NodeId,
        namespace: Option<String>,
        qualified_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        attrs::create_attribute_ns(cx, namespace, &qualified_name)
    }

    fn create_event(
        cx: &mut Cx<'_>,
        this: NodeId,
        interface: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        crate::events::create_event(cx, this, &interface)
    }

    /// <https://dom.spec.whatwg.org/#dom-nonelementparentnode-getelementbyid>
    fn get_element_by_id(
        cx: &mut Cx<'_>,
        this: NodeId,
        element_id: String,
    ) -> b::Result<Option<NodeId>> {
        if element_id.is_empty() {
            return Ok(None);
        }
        Ok(dom(cx.rt)?.get_element_by_id(this, &element_id))
    }

    fn referrer(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        if !is_main(this) {
            return Ok(String::new());
        }
        Ok(engine(cx.rt)?.referrer())
    }

    /// The window of the main document, or null.
    fn default_view(
        cx: &mut Cx<'_>,
        this: NodeId,
    ) -> b::Result<Option<swb_js::Gc<swb_js::Object>>> {
        if !is_main(this) {
            return Ok(None);
        }
        let realm = cx.realm();
        Ok(Some(cx.rt.global(realm)?))
    }

    fn implementation(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::implementation::implementation(cx, this)
    }

    fn get_elements_by_tag_name(
        cx: &mut Cx<'_>,
        this: NodeId,
        qualified_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::tag_name(cx, this, &qualified_name)
    }

    fn get_elements_by_tag_name_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::tag_name_ns(cx, this, namespace.as_deref(), &local_name)
    }

    fn get_elements_by_class_name(
        cx: &mut Cx<'_>,
        this: NodeId,
        class_names: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::class_names(cx, this, &class_names)
    }

    fn get_elements_by_name(
        cx: &mut Cx<'_>,
        this: NodeId,
        element_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::name(cx, this, &element_name)
    }

    fn images(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Images)
    }

    fn embeds(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Embeds)
    }

    fn plugins(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Plugins)
    }

    fn links(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Links)
    }

    fn forms(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Forms)
    }

    fn scripts(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Scripts)
    }

    fn anchors(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Anchors)
    }

    fn applets(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::document_collection(cx, this, DocCollection::Applets)
    }

    parent_node_methods!("Document");
}

/// DOM "valid element local name"
/// (<https://dom.spec.whatwg.org/#valid-element-local-name>).
fn is_valid_element_local_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first.is_ascii_alphabetic() {
        return !name
            .chars()
            .any(|c| c.is_ascii_whitespace() || matches!(c, '\0' | '/' | '>'));
    }
    (first == ':' || first == '_' || first >= '\u{80}')
        && chars.all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '_') || c >= '\u{80}'
        })
}

/// A new element of the document `doc`.
fn create_element(cx: &mut Cx<'_>, doc: NodeId, name: QualName) -> b::Result<NodeId> {
    let data = NodeData::Element(ElementData::new(name, Vec::new()));
    let result = dom_mut(cx.rt)?.create_node_in(doc, data);
    dom_result(cx.rt, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_local_names() {
        for name in [
            "a",
            "my-element",
            "a@b",
            "a:b",
            "\u{e9}t\u{e9}",
            "_x",
            ":x",
            "A",
        ] {
            assert!(is_valid_element_local_name(name), "{name}");
        }
        for name in ["", "1a", "a b", "a/b", "a>", "-x", "_x@", "a\0"] {
            assert!(!is_valid_element_local_name(name), "{name:?}");
        }
    }

    #[test]
    fn strips_and_collapses_ascii_whitespace() {
        assert_eq!(strip_and_collapse("  Hello \n  world "), "Hello world");
        assert_eq!(strip_and_collapse("a\u{a0}b"), "a\u{a0}b");
        assert_eq!(strip_and_collapse("\t\r\x0c"), "");
    }
}
