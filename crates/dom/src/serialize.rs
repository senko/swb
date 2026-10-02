//! HTML serialization of a node (`outerHTML`).
//!
//! Follows the HTML fragment serialization algorithm:
//! <https://html.spec.whatwg.org/multipage/parsing.html#serialising-html-fragments>.
//! Scripting is disabled in swb, so `<noscript>` content is escaped.

use html5ever::{local_name, ns};

use crate::tree::{Document, ElementData, NodeData, NodeId};

/// Serializes `node` and its subtree as HTML (the value of `outerHTML`).
pub fn outer_html(doc: &Document, node: NodeId) -> String {
    enum Step {
        Node(NodeId),
        EndTag(String),
    }
    let mut out = String::new();
    let mut stack = vec![Step::Node(node)];
    while let Some(step) = stack.pop() {
        let id = match step {
            Step::EndTag(name) => {
                out.push_str("</");
                out.push_str(&name);
                out.push('>');
                continue;
            }
            Step::Node(id) => id,
        };
        let Some(n) = doc.get(id) else {
            continue;
        };
        match &n.data {
            NodeData::Element(element) => {
                let name = tag_name(element);
                start_tag(element, &name, &mut out);
                if is_void(element) {
                    continue;
                }
                stack.push(Step::EndTag(name));
                let content = element.template_contents.unwrap_or(id);
                let first = stack.len();
                stack.extend(doc.children(content).map(Step::Node));
                stack[first..].reverse();
            }
            NodeData::Text(text) => {
                let raw = doc
                    .parent(id)
                    .and_then(|p| doc.element(p))
                    .is_some_and(is_raw_text_parent);
                if raw {
                    out.push_str(text);
                } else {
                    escape(text, false, &mut out);
                }
            }
            NodeData::Comment(data) => {
                out.push_str("<!--");
                out.push_str(data);
                out.push_str("-->");
            }
            NodeData::ProcessingInstruction { target, data } => {
                out.push_str("<?");
                out.push_str(target);
                out.push(' ');
                out.push_str(data);
                out.push('>');
            }
            NodeData::Doctype { name, .. } => {
                out.push_str("<!DOCTYPE ");
                out.push_str(name);
                out.push('>');
            }
            NodeData::Document | NodeData::DocumentFragment => {
                let first = stack.len();
                stack.extend(doc.children(id).map(Step::Node));
                stack[first..].reverse();
            }
        }
    }
    out
}

/// The tag name: the local name for HTML, SVG and `MathML` elements,
/// otherwise the qualified name.
fn tag_name(element: &ElementData) -> String {
    let name = &element.name;
    if name.ns == ns!(html) || name.ns == ns!(svg) || name.ns == ns!(mathml) {
        return name.local.to_string();
    }
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

fn start_tag(element: &ElementData, name: &str, out: &mut String) {
    out.push('<');
    out.push_str(name);
    for attr in element.attributes() {
        out.push(' ');
        let a = &attr.name;
        if a.ns == ns!(xml) {
            out.push_str("xml:");
        } else if a.ns == ns!(xmlns) && &*a.local != "xmlns" {
            out.push_str("xmlns:");
        } else if a.ns == ns!(xlink) {
            out.push_str("xlink:");
        } else if let Some(prefix) = a.prefix.as_ref().filter(|_| a.ns != ns!()) {
            out.push_str(prefix);
            out.push(':');
        }
        out.push_str(&a.local);
        out.push_str("=\"");
        escape(&attr.value, true, out);
        out.push('"');
    }
    out.push('>');
}

/// True for void elements, which have no end tag.
fn is_void(element: &ElementData) -> bool {
    element.is_html()
        && matches!(
            *element.local_name(),
            local_name!("area")
                | local_name!("base")
                | local_name!("basefont")
                | local_name!("bgsound")
                | local_name!("br")
                | local_name!("col")
                | local_name!("embed")
                | local_name!("frame")
                | local_name!("hr")
                | local_name!("img")
                | local_name!("input")
                | local_name!("keygen")
                | local_name!("link")
                | local_name!("meta")
                | local_name!("param")
                | local_name!("source")
                | local_name!("track")
                | local_name!("wbr")
        )
}

/// True for elements whose text is serialized without escaping.
fn is_raw_text_parent(element: &ElementData) -> bool {
    element.is_html()
        && matches!(
            *element.local_name(),
            local_name!("style")
                | local_name!("script")
                | local_name!("xmp")
                | local_name!("iframe")
                | local_name!("noembed")
                | local_name!("noframes")
                | local_name!("plaintext")
        )
}

/// Escapes text (or an attribute value).
fn escape(text: &str, attribute: bool, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{A0}' => out.push_str("&nbsp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_html;

    fn outer(html: &str, id: &str) -> String {
        let doc = parse_html(html);
        outer_html(&doc, doc.element_by_id(id).expect("element"))
    }

    #[test]
    fn elements_text_and_attributes() {
        assert_eq!(
            outer(
                "<div id=d class='a\"b'>x &amp; <b>y</b><br>&nbsp;<!--c--></div>",
                "d"
            ),
            "<div id=\"d\" class=\"a&quot;b\">x &amp; <b>y</b><br>&nbsp;<!--c--></div>"
        );
    }

    #[test]
    fn raw_text_and_templates() {
        assert_eq!(
            outer(
                "<div id=d><style>a > b {}</style><template><p>t</p></template></div>",
                "d"
            ),
            "<div id=\"d\"><style>a > b {}</style><template><p>t</p></template></div>"
        );
    }

    #[test]
    fn foreign_elements() {
        assert_eq!(
            outer("<svg id=s><use xlink:href='#x'/></svg>", "s"),
            "<svg id=\"s\"><use xlink:href=\"#x\"></use></svg>"
        );
    }
}
