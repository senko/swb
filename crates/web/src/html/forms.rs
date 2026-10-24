//! The form elements that are not controls with a value: `form`,
//! `label`, `fieldset`, `legend`, `output`, `datalist`, `map`, `object`,
//! `button`, the collections of a form (`HTMLFormControlsCollection`,
//! `RadioNodeList`), and `select` with `option` and its
//! `HTMLOptionsCollection` (HTML §4.10).
//!
//! <https://html.spec.whatwg.org/multipage/forms.html>

use swb_dom::{Document, ElementData, NodeData, NodeId, QualName, local_name, ns};
use swb_js::{Gc, Object};

use super::attribute;
use super::element::{action_url, child_text_content};
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::dom::ElementList;
use crate::dom::lists;
use crate::generated as g;
use crate::generated::types::{
    HTMLElementOrLong, HTMLOptionElementOrHTMLOptGroupElement, RadioNodeListOrElement,
};
use crate::state::{WebState, dom, dom_mut, engine_mut};

/// The form owner of `node` as a binding result.
fn owner(cx: &Cx<'_>, node: NodeId) -> b::Result<Option<NodeId>> {
    Ok(super::form_owner(dom(cx.rt)?, node))
}

/// A list of the element as a platform object.
fn list(cx: &mut Cx<'_>, root: NodeId, kind: ElementList) -> b::Result<Platform> {
    let object = lists::element_list(cx, root, kind)?;
    lists::platform(cx.rt, object)
}

impl g::html_form_element::HTMLFormElementImpl for WebState {
    fn action(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        action_url(cx, this, "action")
    }

    fn rel_list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        super::token_list(cx, this, "rel")
    }

    fn elements(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::FormControls)
    }

    fn length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        let controls = list(cx, this, ElementList::FormControls)?;
        lists::length_of(cx, controls)
    }

    fn index_count(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        Self::length(cx, this)
    }

    fn indexed_getter(cx: &mut Cx<'_>, this: NodeId, index: u32) -> b::Result<NodeId> {
        let controls = list(cx, this, ElementList::FormControls)?;
        lists::item_of(cx, controls, index)?
            .ok_or_else(|| Exception::type_error("the index is out of range"))
    }

    fn supported_names(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Vec<String>> {
        let controls = list(cx, this, ElementList::FormControls)?;
        <WebState as g::html_collection::HTMLCollectionImpl>::supported_names(cx, controls)
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: NodeId, name: &str) -> b::Result<bool> {
        let controls = list(cx, this, ElementList::FormControls)?;
        <WebState as g::html_collection::HTMLCollectionImpl>::is_supported_name(cx, controls, name)
    }

    /// The controls of the form named `name`: the element if there is one,
    /// else a `RadioNodeList`. (Departure: the standard also names `img`
    /// elements, image buttons and past names; swb has the names of
    /// `form.elements`.)
    fn named_getter(
        cx: &mut Cx<'_>,
        this: NodeId,
        name: String,
    ) -> b::Result<RadioNodeListOrElement> {
        let controls = list(cx, this, ElementList::FormControls)?;
        named_controls(cx, controls, this, &name)?
            .ok_or_else(|| Exception::type_error("the name is not supported"))
    }
}

/// The controls named `name`: one element, a `RadioNodeList` of several,
/// or none.
fn named_controls(
    cx: &mut Cx<'_>,
    controls: Platform,
    form: NodeId,
    name: &str,
) -> b::Result<Option<RadioNodeListOrElement>> {
    if name.is_empty() {
        return Ok(None);
    }
    let mut nodes = lists::named_nodes(cx, controls, name)?;
    Ok(match nodes.len() {
        0 => None,
        1 => nodes.pop().map(RadioNodeListOrElement::Element),
        _ => Some(RadioNodeListOrElement::RadioNodeList(
            lists::new_controls_named(cx, form, name, nodes)?,
        )),
    })
}

impl g::html_form_controls_collection::HTMLFormControlsCollectionImpl for WebState {
    fn named_item(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
    ) -> b::Result<Option<RadioNodeListOrElement>> {
        let form = lists::root_of(cx.rt, this)?;
        named_controls(cx, this, form, &name)
    }

    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        <WebState as g::html_collection::HTMLCollectionImpl>::supported_names(cx, this)
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        <WebState as g::html_collection::HTMLCollectionImpl>::is_supported_name(cx, this, name)
    }
}

/// The value of a radio button for `RadioNodeList.value`.
fn radio_value(e: &ElementData) -> &str {
    e.attr("value").unwrap_or("on")
}

impl g::radio_node_list::RadioNodeListImpl for WebState {
    /// The value of the first checked radio button of the list.
    /// <https://html.spec.whatwg.org/multipage/common-dom-interfaces.html#dom-radionodelist-value>
    fn value(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        for node in lists::nodes_of(cx, this)? {
            if !is_radio(dom(cx.rt)?, node) {
                continue;
            }
            if engine_mut(cx.rt)?.control_checked(node) == Some(true) {
                return Ok(dom(cx.rt)?
                    .element(node)
                    .map(|e| radio_value(e).to_owned())
                    .unwrap_or_default());
            }
        }
        Ok(String::new())
    }

    fn set_value(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        for node in lists::nodes_of(cx, this)? {
            let matches = {
                let dom = dom(cx.rt)?;
                is_radio(dom, node) && dom.element(node).is_some_and(|e| radio_value(e) == value)
            };
            if matches {
                engine_mut(cx.rt)?.set_control_checked(node, true);
                break;
            }
        }
        Ok(())
    }
}

/// True if `node` is an `input` of type `radio`.
fn is_radio(dom: &Document, node: NodeId) -> bool {
    dom.element(node).is_some_and(|e| {
        e.is_html()
            && &**e.local_name() == "input"
            && e.attr("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("radio"))
    })
}

impl g::html_label_element::HTMLLabelElementImpl for WebState {
    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let dom = dom(cx.rt)?;
        Ok(super::labeled_control(dom, this).and_then(|c| super::form_owner(dom, c)))
    }

    fn control(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(super::labeled_control(dom(cx.rt)?, this))
    }
}

impl g::html_button_element::HTMLButtonElementImpl for WebState {
    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        owner(cx, this)
    }

    fn form_action(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        action_url(cx, this, "formaction")
    }
}

impl g::html_object_element::HTMLObjectElementImpl for WebState {
    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        owner(cx, this)
    }
}

impl g::html_legend_element::HTMLLegendElementImpl for WebState {
    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(super::legend_form(dom(cx.rt)?, this))
    }
}

impl g::html_field_set_element::HTMLFieldSetElementImpl for WebState {
    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        owner(cx, this)
    }

    fn type_(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok("fieldset".to_owned())
    }

    fn elements(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::FieldsetElements)
    }
}

impl g::html_output_element::HTMLOutputElementImpl for WebState {
    fn html_for(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        super::token_list(cx, this, "for")
    }

    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        owner(cx, this)
    }

    fn type_(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok("output".to_owned())
    }

    fn default_value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(dom(cx.rt)?.text_content(this))
    }

    fn set_default_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        crate::dom::string_replace_all(cx, this, &value)
    }

    fn value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(dom(cx.rt)?.text_content(this))
    }

    fn set_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        crate::dom::string_replace_all(cx, this, &value)
    }
}

impl g::html_data_list_element::HTMLDataListElementImpl for WebState {
    fn options(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::DatalistOptions)
    }
}

impl g::html_map_element::HTMLMapElementImpl for WebState {
    fn areas(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::Areas)
    }
}

// ----- textarea -----

impl g::html_text_area_element::HTMLTextAreaElementImpl for WebState {
    fn autocomplete(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        super::element::autocomplete(cx, this)
    }

    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        owner(cx, this)
    }

    fn type_(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok("textarea".to_owned())
    }

    fn default_value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        child_text_content(cx, this)
    }

    fn set_default_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        crate::dom::string_replace_all(cx, this, &value)
    }

    fn value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        if let Some(value) = engine_mut(cx.rt)?.control_value(this) {
            return Ok(value);
        }
        Ok(dom(cx.rt)?
            .text_content(this)
            .replace("\r\n", "\n")
            .replace('\r', "\n"))
    }

    fn set_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        super::write_control_value(cx, this, &value)
    }

    fn text_length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        let value = Self::value(cx, this)?;
        Ok(u32::try_from(value.encode_utf16().count()).unwrap_or(u32::MAX))
    }
}

// ----- select and option -----

/// The options of a select, as a platform object.
fn options_list(cx: &mut Cx<'_>, select: NodeId) -> b::Result<Platform> {
    list(cx, select, ElementList::SelectOptions)
}

/// The text of an option: its text content with white space stripped and
/// collapsed.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#concept-option-label>
fn option_text(dom: &Document, option: NodeId) -> String {
    dom.text_content_without_scripts(option)
        .split(|c: char| c.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The value of an option: its `value` attribute, else its text.
fn option_value(dom: &Document, option: NodeId) -> String {
    match dom.element(option).and_then(|e| e.attr("value")) {
        Some(value) => value.to_owned(),
        None => option_text(dom, option),
    }
}

/// A new `option` element without attributes and children, in the
/// document of `select`.
fn new_option(cx: &mut Cx<'_>, select: NodeId) -> b::Result<NodeId> {
    let document = dom(cx.rt)?.node_document(select);
    let name = QualName::new(None, ns!(html), local_name!("option"));
    let data = NodeData::Element(ElementData::new(name, Vec::new()));
    let result = dom_mut(cx.rt)?.create_node_in(document, data);
    crate::dom::dom_result(cx.rt, result)
}

/// `select.add(element, before)` and `options.add`.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#dom-select-add>
#[allow(
    clippy::needless_pass_by_value,
    reason = "the glue passes owned values"
)]
fn add(
    cx: &mut Cx<'_>,
    select: NodeId,
    element: HTMLOptionElementOrHTMLOptGroupElement,
    before: Option<HTMLElementOrLong>,
) -> b::Result<()> {
    let element = match element {
        HTMLOptionElementOrHTMLOptGroupElement::HTMLOptionElement(n)
        | HTMLOptionElementOrHTMLOptGroupElement::HTMLOptGroupElement(n) => n,
    };
    if dom(cx.rt)?.is_host_including_inclusive_ancestor(element, select) {
        return Err(crate::dom::tree_ops::hierarchy(
            "The new child element contains the parent.",
        ));
    }
    let reference = match before {
        None => None,
        Some(HTMLElementOrLong::HTMLElement(node)) => {
            // The standard says "descendant"; Chromium (measured) accepts
            // only a child of the select, so an option in an `optgroup`
            // throws NotFoundError.
            if dom(cx.rt)?.parent(node) != Some(select) {
                return Err(crate::dom::tree_ops::not_found(
                    "The node before which the new node is to be inserted is not a child of this node.",
                ));
            }
            Some(node)
        }
        Some(HTMLElementOrLong::Long(index)) => {
            let options = options_list(cx, select)?;
            u32::try_from(index)
                .ok()
                .map(|i| lists::item_of(cx, options, i))
                .transpose()?
                .flatten()
        }
    };
    if reference == Some(element) {
        return Ok(());
    }
    let parent = reference.and_then(|r| dom_parent(cx, r)).unwrap_or(select);
    crate::dom::tree_ops::pre_insert(cx, parent, element, reference)
}

/// The parent of `node`.
fn dom_parent(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    dom(cx.rt).ok().and_then(|d| d.parent(node))
}

/// `remove(index)` of a select and of its options collection.
fn remove_at(cx: &mut Cx<'_>, select: NodeId, index: i32) -> b::Result<()> {
    let Ok(index) = u32::try_from(index) else {
        return Ok(());
    };
    let options = options_list(cx, select)?;
    if let Some(option) = lists::item_of(cx, options, index)? {
        crate::dom::tree_ops::remove(cx, option)?;
    }
    Ok(())
}

/// The setter of `length`: more options are appended (at most up to
/// 100,000), fewer are removed from the end.
/// <https://html.spec.whatwg.org/multipage/form-elements.html#dom-select-length>
fn set_length(cx: &mut Cx<'_>, select: NodeId, length: u32) -> b::Result<()> {
    let options = options_list(cx, select)?;
    let current = lists::length_of(cx, options)?;
    if length > current {
        if length > 100_000 {
            return Ok(());
        }
        for _ in current..length {
            let option = new_option(cx, select)?;
            crate::dom::tree_ops::pre_insert(cx, select, option, None)?;
        }
    } else if length < current {
        let nodes = lists::nodes_of(cx, options)?;
        for option in nodes.into_iter().skip(length as usize) {
            crate::dom::tree_ops::remove(cx, option)?;
        }
    }
    Ok(())
}

/// The setter of an indexed property of a select and of its options
/// collection.
/// <https://html.spec.whatwg.org/multipage/common-dom-interfaces.html#htmloptionscollection>
fn set_indexed(
    cx: &mut Cx<'_>,
    select: NodeId,
    index: u32,
    value: Option<NodeId>,
) -> b::Result<()> {
    let Some(option) = value else {
        return remove_at(cx, select, i32::try_from(index).unwrap_or(i32::MAX));
    };
    let options = options_list(cx, select)?;
    let length = lists::length_of(cx, options)?;
    if index > length {
        // Padding: empty options, at most 100,000.
        if index - length > 100_000 {
            return Ok(());
        }
        for _ in length..index {
            let padding = new_option(cx, select)?;
            crate::dom::tree_ops::pre_insert(cx, select, padding, None)?;
        }
    }
    let options = options_list(cx, select)?;
    let current = lists::item_of(cx, options, index)?;
    match current {
        Some(old) if old == option => Ok(()),
        Some(old) => match dom_parent(cx, old) {
            Some(parent) => crate::dom::tree_ops::replace(cx, parent, option, old),
            None => Ok(()),
        },
        None => crate::dom::tree_ops::pre_insert(cx, select, option, None),
    }
}

impl g::html_select_element::HTMLSelectElementImpl for WebState {
    fn autocomplete(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        super::element::autocomplete(cx, this)
    }

    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        owner(cx, this)
    }

    fn type_(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let multiple = attribute(cx, this, "multiple")?.is_some();
        Ok(if multiple {
            "select-multiple"
        } else {
            "select-one"
        }
        .to_owned())
    }

    fn options(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::SelectOptions)
    }

    fn length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        let options = options_list(cx, this)?;
        lists::length_of(cx, options)
    }

    fn set_length(cx: &mut Cx<'_>, this: NodeId, value: u32) -> b::Result<()> {
        set_length(cx, this, value)
    }

    fn selected_options(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::SelectedOptions)
    }

    fn selected_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(engine_mut(cx.rt)?.selected_index(this))
    }

    fn set_selected_index(cx: &mut Cx<'_>, this: NodeId, value: i32) -> b::Result<()> {
        engine_mut(cx.rt)?.set_selected_index(this, value);
        Ok(())
    }

    fn value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let index = engine_mut(cx.rt)?.selected_index(this);
        let Ok(index) = u32::try_from(index) else {
            return Ok(String::new());
        };
        let options = options_list(cx, this)?;
        Ok(match lists::item_of(cx, options, index)? {
            Some(option) => option_value(dom(cx.rt)?, option),
            None => String::new(),
        })
    }

    /// The first option with this value is the only selected one; none if
    /// there is no such option.
    fn set_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        let options = options_list(cx, this)?;
        let nodes = lists::nodes_of(cx, options)?;
        let dom = dom(cx.rt)?;
        let position = nodes
            .iter()
            .position(|&o| option_value(dom, o) == value)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1);
        engine_mut(cx.rt)?.set_selected_index(this, position);
        Ok(())
    }

    fn item(cx: &mut Cx<'_>, this: NodeId, index: u32) -> b::Result<Option<NodeId>> {
        let options = options_list(cx, this)?;
        lists::item_of(cx, options, index)
    }

    fn named_item(cx: &mut Cx<'_>, this: NodeId, name: String) -> b::Result<Option<NodeId>> {
        let options = options_list(cx, this)?;
        lists::named_item(cx, options, &name)
    }

    fn add(
        cx: &mut Cx<'_>,
        this: NodeId,
        element: HTMLOptionElementOrHTMLOptGroupElement,
        before: Option<HTMLElementOrLong>,
    ) -> b::Result<()> {
        add(cx, this, element, before)
    }

    fn remove_1(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        crate::dom::tree_ops::remove(cx, this)
    }

    fn remove_2(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<()> {
        remove_at(cx, this, index)
    }

    fn index_count(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        Self::length(cx, this)
    }

    fn indexed_setter(
        cx: &mut Cx<'_>,
        this: NodeId,
        index: u32,
        value: Option<NodeId>,
    ) -> b::Result<()> {
        set_indexed(cx, this, index, value)
    }
}

impl g::html_options_collection::HTMLOptionsCollectionImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        lists::length_of(cx, this)
    }

    fn set_length(cx: &mut Cx<'_>, this: Platform, value: u32) -> b::Result<()> {
        let select = lists::root_of(cx.rt, this)?;
        set_length(cx, select, value)
    }

    fn selected_index(cx: &mut Cx<'_>, this: Platform) -> b::Result<i32> {
        let select = lists::root_of(cx.rt, this)?;
        Ok(engine_mut(cx.rt)?.selected_index(select))
    }

    fn set_selected_index(cx: &mut Cx<'_>, this: Platform, value: i32) -> b::Result<()> {
        let select = lists::root_of(cx.rt, this)?;
        engine_mut(cx.rt)?.set_selected_index(select, value);
        Ok(())
    }

    fn add(
        cx: &mut Cx<'_>,
        this: Platform,
        element: HTMLOptionElementOrHTMLOptGroupElement,
        before: Option<HTMLElementOrLong>,
    ) -> b::Result<()> {
        let select = lists::root_of(cx.rt, this)?;
        add(cx, select, element, before)
    }

    fn remove(cx: &mut Cx<'_>, this: Platform, index: i32) -> b::Result<()> {
        let select = lists::root_of(cx.rt, this)?;
        remove_at(cx, select, index)
    }

    fn indexed_setter(
        cx: &mut Cx<'_>,
        this: Platform,
        index: u32,
        value: Option<NodeId>,
    ) -> b::Result<()> {
        let select = lists::root_of(cx.rt, this)?;
        set_indexed(cx, select, index, value)
    }
}

impl g::html_option_element::HTMLOptionElementImpl for WebState {
    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(super::option_form(dom(cx.rt)?, this))
    }

    fn label(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match attribute(cx, this, "label")? {
            Some(label) => label,
            None => option_text(dom(cx.rt)?, this),
        })
    }

    fn selected(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(engine_mut(cx.rt)?.option_selected(this))
    }

    fn set_selected(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        engine_mut(cx.rt)?.set_option_selected(this, value);
        Ok(())
    }

    fn value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(option_value(dom(cx.rt)?, this))
    }

    fn text(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(option_text(dom(cx.rt)?, this))
    }

    fn set_text(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        crate::dom::string_replace_all(cx, this, &value)
    }

    /// The position in the list of options of the select, else 0.
    fn index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        let Some(select) = super::select_of_option(dom(cx.rt)?, this) else {
            return Ok(0);
        };
        let options = options_list(cx, select)?;
        let nodes = lists::nodes_of(cx, options)?;
        Ok(nodes
            .iter()
            .position(|&o| o == this)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(0))
    }
}
