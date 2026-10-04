//! The options of a `<select>`: the list of options, their text, label
//! and value, the default selectedness, and the placeholder label option.

use swb_dom::{Document, ElementData, NodeId, is_html_whitespace, local_name};
use swb_style::DisabledElements;

use super::{ControlState, OptionState, parse_non_negative};

/// True if the first option of a required select is its placeholder label
/// option: a child of the select with an empty value, in a select without
/// `multiple` and with a display size of 1.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#placeholder-label-option>
pub(super) fn has_placeholder_label_option(
    doc: &Document,
    select: NodeId,
    state: &ControlState,
) -> bool {
    let Some(e) = doc.element(select) else {
        return false;
    };
    let Some(first) = state.options.first() else {
        return false;
    };
    !e.has_attr("multiple")
        && display_size(e) == 1
        && doc.parent(first.node) == Some(select)
        && option_value(doc, first.node).is_empty()
}

/// The display size of a select: its `size` attribute, or 4 with
/// `multiple`, else 1.
fn display_size(e: &ElementData) -> u32 {
    e.attr("size")
        .and_then(parse_non_negative)
        .filter(|&s| s > 0)
        .unwrap_or(if e.has_attr("multiple") { 4 } else { 1 })
}

/// The options of a select with their default selectedness, after the
/// selectedness setting algorithm.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#selectedness-setting-algorithm>
pub(super) fn select_options(
    doc: &Document,
    select: NodeId,
    disabled: &DisabledElements,
) -> Vec<OptionState> {
    let mut options: Vec<OptionState> = list_of_options(doc, select)
        .into_iter()
        .map(|node| OptionState {
            node,
            selected: doc.element(node).is_some_and(|e| e.has_attr("selected")),
        })
        .collect();
    let Some(e) = doc.element(select) else {
        return options;
    };
    if e.has_attr("multiple") {
        return options;
    }
    let selected = options.iter().filter(|o| o.selected).count();
    if selected == 0 && display_size(e) == 1 {
        if let Some(first) = options.iter_mut().find(|o| !disabled.contains(o.node)) {
            first.selected = true;
        }
    } else if selected > 1 {
        let last = options.iter().rposition(|o| o.selected);
        for (i, option) in options.iter_mut().enumerate() {
            option.selected = Some(i) == last;
        }
    }
    options
}

/// The list of options of a select: its `option` children and the
/// `option` children of its `optgroup` children, in tree order.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#concept-select-option-list>
fn list_of_options(doc: &Document, select: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    for child in doc.element_children(select) {
        if doc.is_html_element(child, &local_name!("option")) {
            out.push(child);
        } else if doc.is_html_element(child, &local_name!("optgroup")) {
            out.extend(
                doc.element_children(child)
                    .filter(|&c| doc.is_html_element(c, &local_name!("option"))),
            );
        }
    }
    out
}

/// The text of an option: its descendant text with white space stripped
/// and collapsed.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#concept-option-text>
fn option_text(doc: &Document, option: NodeId) -> String {
    doc.text_content(option)
        .split(is_html_whitespace)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The label of an option: its `label` attribute, or its text.
pub(crate) fn option_label(doc: &Document, option: NodeId) -> String {
    match doc
        .element(option)
        .and_then(|e| e.attr("label"))
        .filter(|l| !l.is_empty())
    {
        Some(label) => label.to_owned(),
        None => option_text(doc, option),
    }
}

/// The value of an option: its `value` attribute, or its text.
pub(crate) fn option_value(doc: &Document, option: NodeId) -> String {
    match doc.element(option).and_then(|e| e.attr("value")) {
        Some(value) => value.to_owned(),
        None => option_text(doc, option),
    }
}
