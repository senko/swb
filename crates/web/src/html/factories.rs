//! The legacy factory functions `Image` and `Option` (HTML Standard,
//! <https://html.spec.whatwg.org/multipage/embedded-content.html#dom-image>
//! and <https://html.spec.whatwg.org/multipage/form-elements.html#dom-option>;
//! Web IDL `[LegacyFactoryFunction]`). The generated glue checks `new` and
//! converts the arguments; these functions make the element in the
//! document of the window, set its attributes and wrap it (with the
//! prototype of `new.target`, so `class A extends Image` works).

use swb_dom::{ElementData, LocalName, NodeData, NodeId, QualName, ns};
use swb_js::{Gc, Object};

use super::set_attribute;
use crate::bindings::{self as b, Cx};
use crate::dom::{dom_result, string_replace_all};
use crate::state::{dom_mut, engine_mut, wrap_node_with_proto};

/// The largest value of a reflected `unsigned long` that is limited to
/// the range of `long`: a larger value sets 0 (HTML "reflecting content
/// attributes in IDL attributes", the rule of `width` and `height`).
const MAX_LIMITED: u32 = 2_147_483_647;

/// A new HTML element `local` in the window's document.
fn new_html_element(cx: &mut Cx<'_>, local: &str) -> b::Result<NodeId> {
    let name = QualName::new(None, ns!(html), LocalName::from(local));
    let data = NodeData::Element(ElementData::new(name, Vec::new()));
    let result = dom_mut(cx.rt)?.create_node(data);
    dom_result(cx.rt, result)
}

/// Makes the element with `make` and wraps it. The prototype of
/// `new.target` is read first: a getter that throws leaves no node.
fn wrap(
    cx: &mut Cx<'_>,
    make: impl FnOnce(&mut Cx<'_>) -> b::Result<NodeId>,
) -> b::Result<Gc<Object>> {
    let proto = cx.prototype_from_new_target()?;
    let node = make(cx)?;
    let realm = cx.realm();
    Ok(wrap_node_with_proto(cx.rt, realm, node, proto)?)
}

/// `new Image(width, height)`: an `img` with the `width` and `height`
/// attributes that the arguments give.
pub(super) fn image(
    cx: &mut Cx<'_>,
    width: Option<u32>,
    height: Option<u32>,
) -> b::Result<Gc<Object>> {
    wrap(cx, |cx| {
        let node = new_html_element(cx, "img")?;
        for (name, value) in [("width", width), ("height", height)] {
            if let Some(value) = value {
                let value = if value > MAX_LIMITED { 0 } else { value };
                set_attribute(cx, node, name, &value.to_string())?;
            }
        }
        Ok(node)
    })
}

/// `new Option(text, value, defaultSelected, selected)`: an `option` with
/// the text (a text node if it is not empty), a `value` attribute if
/// `value` is given, a `selected` attribute if `default_selected`. The
/// selectedness is set to `selected` without the dirtiness flag, as the
/// HTML Standard says: `new Option("a", "b", true).selected` is false, and
/// a later change of the `selected` attribute changes the selectedness.
pub(super) fn option(
    cx: &mut Cx<'_>,
    text: &str,
    value: Option<&str>,
    default_selected: bool,
    selected: bool,
) -> b::Result<Gc<Object>> {
    wrap(cx, |cx| {
        let node = new_html_element(cx, "option")?;
        if !text.is_empty() {
            string_replace_all(cx, node, text)?;
        }
        if let Some(value) = value {
            set_attribute(cx, node, "value", value)?;
        }
        if default_selected {
            set_attribute(cx, node, "selected", "")?;
        }
        if selected != default_selected {
            engine_mut(cx.rt)?.set_option_initial_selected(node, selected);
        }
        Ok(node)
    })
}
