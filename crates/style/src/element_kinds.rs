//! What kind of box an HTML element generates, where more than one stage
//! needs the answer: the replaced elements of swb, the elements whose
//! content generates no boxes, the elements that cannot be list items,
//! and the elements without `::before` and `::after`.
//!
//! The sets follow Chromium 148 (measured with `display: list-item` and
//! with counters in the content of each element), except where a
//! function says otherwise. Layout uses [`is_replaced_element`] for its
//! replaced boxes; the cascade uses [`generates_content_pseudos`]; the
//! counter pass (`counters.rs`) uses the others.

use swb_dom::{ElementData, local_name, ns, parse_integer};

/// True for the replaced elements that swb lays out as replaced boxes:
/// `img`, `video`, `audio` and `svg` in the SVG namespace (an `svg`
/// element that layout reaches is always the outermost one: the content
/// of a replaced box gets no boxes).
pub fn is_replaced_element(element: &ElementData) -> bool {
    element.is_html()
        && matches!(
            element.local_name(),
            &local_name!("img") | &local_name!("video") | &local_name!("audio")
        )
        || is_svg_element(element)
}

/// True for an `svg` element in the SVG namespace.
pub(crate) fn is_svg_element(element: &ElementData) -> bool {
    element.name.ns == ns!(svg) && *element.local_name() == local_name!("svg")
}

/// False for elements whose element children generate no boxes: the
/// replaced elements, `input`, `textarea`, a drop-down `select` (the
/// options of a list box have boxes), `option` (its text is rendered,
/// its child elements are not), and `progress` and `meter`. swb's layout
/// still renders the content of `progress`, `meter` and `option` outside
/// a `select` (a layout gap); counters in it have no text.
pub(crate) fn renders_children(element: &ElementData) -> bool {
    if !element.is_html() {
        return true;
    }
    match element.local_name() {
        &local_name!("input")
        | &local_name!("textarea")
        | &local_name!("option")
        | &local_name!("progress")
        | &local_name!("meter") => false,
        &local_name!("select") => is_list_box(element),
        _ => !is_replaced_element(element),
    }
}

/// True if a `select` element is a list box: its `size` is above 1. As
/// measured in Chromium 148, the `size` attribute is parsed by the HTML
/// rules for non-negative integers into 32 bits; if it is missing,
/// invalid, out of range or 0, the size is 4 with `multiple` and 1
/// without. (So `multiple size=1` is a drop-down, unlike the HTML
/// specification, which makes every `multiple` select a list box.)
fn is_list_box(select: &ElementData) -> bool {
    let size = select
        .attr("size")
        .and_then(parse_integer)
        .and_then(|size| u32::try_from(size).ok())
        .filter(|&size| size != 0)
        .unwrap_or(if select.has_attr("multiple") { 4 } else { 1 });
    size > 1
}

/// True for elements that are not list items with `display: list-item`,
/// as measured in Chromium 148: the replaced elements, form controls
/// (`input`, `select`, `textarea`, `button`), `iframe`, `embed`, `svg`,
/// `fieldset`, `progress`, `meter`, `br` and `wbr`. `object` and `canvas`
/// are list items, because they show their fallback content (swb loads
/// no plugins and runs no scripts). Chromium also gives a number (with
/// an empty marker) to an `img` that failed to load and has `alt` text,
/// and to an `input type=image`; swb does not (style does not know the
/// load state).
pub(crate) fn cannot_be_list_item(element: &ElementData) -> bool {
    is_replaced_element(element)
        || element.is_html()
            && matches!(
                element.local_name(),
                &local_name!("input")
                    | &local_name!("select")
                    | &local_name!("textarea")
                    | &local_name!("button")
                    | &local_name!("iframe")
                    | &local_name!("embed")
                    | &local_name!("fieldset")
                    | &local_name!("progress")
                    | &local_name!("meter")
                    | &local_name!("br")
                    | &local_name!("wbr")
            )
}

/// True for elements whose own counter properties have no effect: `wbr`
/// (as measured in Chromium 148).
pub(crate) fn ignores_counter_properties(element: &ElementData) -> bool {
    element.is_html_named(&local_name!("wbr"))
}

/// True if `::before` and `::after` apply: not for replaced elements, form
/// controls and SVG elements. (Measured differences from Chromium 148,
/// not changed here: Chromium generates them for `canvas`, `object` and
/// list-box `select` elements, and not for `math`.)
pub(crate) fn generates_content_pseudos(element: &ElementData) -> bool {
    element.name.ns != ns!(svg)
        && !(element.is_html()
            && matches!(
                &**element.local_name(),
                "img"
                    | "input"
                    | "select"
                    | "textarea"
                    | "iframe"
                    | "video"
                    | "audio"
                    | "canvas"
                    | "embed"
                    | "object"
                    | "br"
                    | "wbr"
            ))
}

#[cfg(test)]
mod tests {
    use swb_dom::parse_html;

    use super::*;

    #[test]
    fn element_sets() {
        let doc = parse_html(
            "<img><video></video><audio></audio><input><select></select><textarea></textarea>\
             <progress></progress><meter></meter><button></button><iframe></iframe><embed>\
             <svg></svg><fieldset></fieldset><br><object></object><canvas></canvas><div></div>\
             <select multiple></select><select size=' 4x'></select><select size=1></select>\
             <select multiple size=1></select><select size=4294967296></select>\
             <select multiple size=0></select><option></option><wbr>",
        );
        let body = doc.body().expect("a body");
        let kinds: Vec<(String, bool, bool, bool)> = doc
            .element_children(body)
            .filter_map(|n| doc.element(n))
            .map(|e| {
                (
                    e.local_name().to_string(),
                    is_replaced_element(e),
                    renders_children(e),
                    cannot_be_list_item(e),
                )
            })
            .collect();
        let expected = [
            ("img", true, false, true),
            ("video", true, false, true),
            ("audio", true, false, true),
            ("input", false, false, true),
            ("select", false, false, true),
            ("textarea", false, false, true),
            ("progress", false, false, true),
            ("meter", false, false, true),
            ("button", false, true, true),
            ("iframe", false, true, true),
            ("embed", false, true, true),
            ("svg", true, true, true),
            ("fieldset", false, true, true),
            ("br", false, true, true),
            ("object", false, true, false),
            ("canvas", false, true, false),
            ("div", false, true, false),
            ("select", false, true, true),
            ("select", false, true, true),
            ("select", false, false, true),
            ("select", false, false, true),
            ("select", false, false, true),
            ("select", false, true, true),
            ("option", false, false, false),
            ("wbr", false, true, true),
        ];
        let expected: Vec<(String, bool, bool, bool)> = expected
            .iter()
            .map(|&(n, a, b, c)| (n.to_owned(), a, b, c))
            .collect();
        assert_eq!(kinds, expected);
    }
}
