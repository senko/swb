//! HTML serialization of a node (`outerHTML`) and of the children of a
//! node (`innerHTML`).
//!
//! Follows the HTML fragment serialization algorithm:
//! <https://html.spec.whatwg.org/multipage/parsing.html#serialising-html-fragments>.
//! The text of a `<noscript>` is written as it is if scripting is enabled
//! for the parent ([`Document::scripting_for`]), else it is escaped as
//! usual. Departure from the algorithm, as in Chromium (measured in
//! `tools/probes/web-dom-query.json`): the tag name of every element is its
//! qualified name (the algorithm uses the local name in the HTML, SVG and
//! `MathML` namespaces), and a `pre` gets no extra line feed. The walk is
//! iterative, so a tree of any depth serializes.

use html5ever::{local_name, ns};

use crate::tree::{Document, ElementData, NodeData, NodeId};

/// Serializes `node` and its subtree as HTML (the value of `outerHTML`).
pub fn outer_html(doc: &Document, node: NodeId) -> String {
    serialize(doc, vec![Step::Node(node)])
}

/// Serializes the children of `node` as HTML (the value of `innerHTML`);
/// for a `template` element the children of its template contents.
pub fn inner_html(doc: &Document, node: NodeId) -> String {
    let content = doc
        .element(node)
        .and_then(|e| e.template_contents)
        .unwrap_or(node);
    let mut stack: Vec<Step> = doc.children(content).map(Step::Node).collect();
    stack.reverse();
    serialize(doc, stack)
}

/// A step of the serialization walk.
enum Step {
    Node(NodeId),
    EndTag(String),
}

/// Runs the steps on a stack (the next step is the last).
fn serialize(doc: &Document, mut stack: Vec<Step>) -> String {
    let mut out = String::new();
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
                let raw = doc.parent(id).is_some_and(|parent| {
                    doc.element(parent)
                        .is_some_and(|p| is_raw_text_parent(p, doc.scripting_for(parent)))
                });
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

/// The tag name: the qualified name (Chromium; see the module
/// documentation).
fn tag_name(element: &ElementData) -> String {
    let name = &element.name;
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

/// True for elements whose text is serialized without escaping (`noscript`
/// only with the scripting flag on).
fn is_raw_text_parent(element: &ElementData, scripting: bool) -> bool {
    element.is_html()
        && match *element.local_name() {
            local_name!("noscript") => scripting,
            local_name!("style")
            | local_name!("script")
            | local_name!("xmp")
            | local_name!("iframe")
            | local_name!("noembed")
            | local_name!("noframes")
            | local_name!("plaintext") => true,
            _ => false,
        }
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
    fn noscript_text_is_raw_only_with_scripting() {
        let html = "<!DOCTYPE html><body><noscript id=n><p>a &amp; b</p></noscript>";
        let off = parse_html(html);
        let n = off.element_by_id("n").expect("noscript");
        assert_eq!(
            outer_html(&off, n),
            "<noscript id=\"n\"><p>a &amp; b</p></noscript>"
        );
        let on = crate::parse_html_with(html, true);
        let n = on.element_by_id("n").expect("noscript");
        // The text node holds the markup as text, written as it is.
        assert_eq!(
            outer_html(&on, n),
            "<noscript id=\"n\"><p>a &amp; b</p></noscript>"
        );
        let text = on.children(n).next().expect("text");
        assert_eq!(on.text_content(text), "<p>a &amp; b</p>");
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
    fn inner_html_serializes_the_children() {
        let doc = parse_html("<div id=d><b>x</b>&amp;<template id=t><i>y</i></template></div>");
        let d = doc.element_by_id("d").expect("element");
        assert_eq!(
            inner_html(&doc, d),
            "<b>x</b>&amp;<template id=\"t\"><i>y</i></template>"
        );
        let t = doc.element_by_id("t").expect("element");
        assert_eq!(inner_html(&doc, t), "<i>y</i>");
    }

    #[test]
    fn a_deep_tree_serializes() {
        let depth = 100_000;
        let html = format!("{}x{}", "<span>".repeat(depth), "</span>".repeat(depth));
        let doc = parse_html(&html);
        let body = doc.body().expect("body");
        assert_eq!(inner_html(&doc, body).len(), html.len());
    }

    #[test]
    fn prefixed_elements_keep_the_prefix() {
        let mut doc = Document::new();
        let name =
            html5ever::QualName::new(Some("s".into()), ns!(svg), html5ever::LocalName::from("g"));
        let e = doc
            .create_element(name, Vec::new())
            .expect("an element in a new arena");
        assert_eq!(outer_html(&doc, e), "<s:g></s:g>");
    }

    #[test]
    fn foreign_elements() {
        assert_eq!(
            outer("<svg id=s><use xlink:href='#x'/></svg>", "s"),
            "<svg id=\"s\"><use xlink:href=\"#x\"></use></svg>"
        );
    }
}
