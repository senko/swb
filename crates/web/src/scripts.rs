//! The inline classic scripts of a parsed document (feature 4c2): they run
//! in tree order after the parse. Feature 5c replaces this with the
//! script processing of HTML §4.12.1 (parser-blocking scripts at their
//! place in the parse, external scripts, `defer`, `async`, modules).
//!
//! The type test follows HTML §4.12.1.1 "prepare the script element"
//! (<https://html.spec.whatwg.org/multipage/scripting.html#prepare-the-script-element>),
//! steps 8 to 12: the type string from `type` or `language`, a
//! JavaScript MIME type essence match, and `nomodule`.

use swb_dom::{Document, NodeId, local_name};

/// The JavaScript MIME type essences (MIME Sniffing §4.6,
/// <https://mimesniff.spec.whatwg.org/#javascript-mime-type>).
const JAVASCRIPT_TYPES: &[&str] = &[
    "application/ecmascript",
    "application/javascript",
    "application/x-ecmascript",
    "application/x-javascript",
    "text/ecmascript",
    "text/javascript",
    "text/javascript1.0",
    "text/javascript1.1",
    "text/javascript1.2",
    "text/javascript1.3",
    "text/javascript1.4",
    "text/javascript1.5",
    "text/jscript",
    "text/livescript",
    "text/x-ecmascript",
    "text/x-javascript",
];

/// True if the `script` element `node` is a classic script by its type
/// (steps 8 to 10 of "prepare the script element") and has no
/// `nomodule` attribute.
fn is_classic(dom: &Document, node: NodeId) -> bool {
    let Some(element) = dom.element(node) else {
        return false;
    };
    let kind = match (element.attr("type"), element.attr("language")) {
        (Some(""), _) | (None, Some("") | None) => "text/javascript".to_owned(),
        (Some(kind), _) => kind
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .to_owned(),
        (None, Some(language)) => format!("text/{language}"),
    };
    JAVASCRIPT_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&kind))
        && !element.has_attr("nomodule")
}

/// The URL of the lent document (the name of the scripts of string
/// timer handlers), or empty without one.
pub(crate) fn document_url(rt: &swb_js::Runtime) -> String {
    crate::state::engine(rt).map_or_else(|_| String::new(), crate::host::EngineHost::url)
}

/// The inline classic scripts of the document, in tree order: the text
/// of each connected HTML `script` element without `src` whose type is
/// JavaScript. Scripts with `src` are skipped (feature 5c loads them).
pub(crate) fn inline_classic_scripts(dom: &Document) -> Vec<String> {
    let mut scripts = Vec::new();
    for node in dom.descendants(NodeId::DOCUMENT) {
        if !dom.is_html_element(node, &local_name!("script")) || !is_classic(dom, node) {
            continue;
        }
        if dom.element(node).is_some_and(|e| e.has_attr("src")) {
            log::debug!("an external script is not run (feature 5c)");
            continue;
        }
        let text = dom
            .children(node)
            .filter_map(|c| dom.node(c).as_text())
            .collect();
        scripts.push(text);
    }
    scripts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(html: &str) -> Vec<String> {
        inline_classic_scripts(&swb_dom::parse_html_with(html, true))
    }

    #[test]
    fn script_types() {
        let html = "<script>1</script><script type=''>2</script><script language=''>3</script>\
                    <script type=' TEXT/JavaScript '>4</script><script language=javascript>5</script>\
                    <script type=module>no</script><script type=text/plain>no</script>\
                    <script language=vbscript>no</script><script nomodule>no</script>\
                    <script src=a.js>no</script><svg><script>no</script></svg>\
                    <template><script>no</script></template>";
        assert_eq!(kinds(html), ["1", "2", "3", "4", "5"]);
    }
}
