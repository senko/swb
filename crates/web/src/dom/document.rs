//! `Document` (DOM §4.5, <https://dom.spec.whatwg.org/#interface-document>)
//! with the tree accessors of HTML §3.1.4
//! (<https://html.spec.whatwg.org/multipage/dom.html#dom-tree-accessors>):
//! `title`, `body` and `head`.
//!
//! Only the document of `NodeId::DOCUMENT` has a URL, an encoding and a
//! quirks mode; the other documents of the arena (the inert documents of
//! template contents) are `about:blank`, UTF-8 and no-quirks.

use swb_dom::{Document, ElementData, NodeData, NodeId, QualName, QuirksMode, local_name, ns};

use super::{child_element_count, dom_result, first_element_child, last_element_child};
use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::state::{WebState, dom, dom_mut, engine};

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
    fn url(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        if !is_main(this) {
            return Ok("about:blank".to_owned());
        }
        Ok(engine(cx.rt)?.url())
    }

    fn document_uri(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Self::url(cx, this)
    }

    fn compat_mode(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let quirks = is_main(this) && dom(cx.rt)?.quirks_mode == QuirksMode::Quirks;
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
        string_replace_all(cx, this, element, &value)
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
        let result = match (old, document_element(dom, this)) {
            (Some(old), _) => match dom.parent(old) {
                Some(parent) => dom_mut(cx.rt)?.replace_child(parent, value, old),
                None => return Ok(()),
            },
            (None, None) => {
                return Err(Exception::dom(
                    "HierarchyRequestError",
                    "No document element exists.",
                ));
            }
            (None, Some(root)) => dom_mut(cx.rt)?.append_child(root, value),
        };
        dom_result(cx.rt, result)
    }

    fn head(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(head_element(dom(cx.rt)?, this))
    }

    fn create_event(
        cx: &mut Cx<'_>,
        this: NodeId,
        interface: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        crate::events::create_event(cx, this, &interface)
    }

    fn first_element_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        first_element_child(cx, this)
    }

    fn last_element_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        last_element_child(cx, this)
    }

    fn child_element_count(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        child_element_count(cx, this)
    }
}

/// DOM "string replace all": replaces the children of `parent` with a text
/// node of `value` (none for the empty string) of the document `doc`.
/// <https://dom.spec.whatwg.org/#string-replace-all>
fn string_replace_all(cx: &mut Cx<'_>, doc: NodeId, parent: NodeId, value: &str) -> b::Result<()> {
    let node = if value.is_empty() {
        None
    } else {
        let result = dom_mut(cx.rt)?.create_node_in(doc, NodeData::Text(value.to_owned()));
        Some(dom_result(cx.rt, result)?)
    };
    let result = dom_mut(cx.rt)?.replace_all(parent, node);
    dom_result(cx.rt, result)
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
    fn strips_and_collapses_ascii_whitespace() {
        assert_eq!(strip_and_collapse("  Hello \n  world "), "Hello world");
        assert_eq!(strip_and_collapse("a\u{a0}b"), "a\u{a0}b");
        assert_eq!(strip_and_collapse("\t\r\x0c"), "");
    }
}
