//! The HTML element constructors (`[HTMLConstructor]`, HTML §3.2.3,
//! <https://html.spec.whatwg.org/multipage/dom.html#html-element-constructors>;
//! ADR 0035 section 4) and the construction stack of a definition.
//!
//! `new C()` with a defined class `C` makes a new element of the current
//! window's document. During an upgrade, the stack holds the element, and
//! `super()` returns it with the prototype of `new.target`; a second
//! `super()` finds the "already constructed" marker.

use swb_dom::{CustomElementState, ElementData, NodeData, NodeId, QualName, ns};
use swb_js::{ClassId, NativeCall, Runtime, Value};

use super::StackEntry;
use crate::bindings::{self as b, Exception};
use crate::generated as g;
use crate::state::{dom_mut, state};

/// Chromium's message of an element constructor without a definition.
const ILLEGAL: &str = "Illegal constructor";

/// The steps of the HTML element constructor of the interface `class`.
pub(crate) fn construct(rt: &mut Runtime, call: &NativeCall, class: ClassId) -> b::Result<Value> {
    let Value::Object(new_target) = call.new_target() else {
        return Err(Exception::type_error(ILLEGAL));
    };
    // Step 2: `new HTMLElement()` itself.
    if new_target == call.callee() {
        return Err(Exception::type_error(ILLEGAL));
    }
    // Step 3: the definition of the current global object's registry.
    let Some(&number) = state(rt)?.doc.custom.by_constructor.get(&new_target) else {
        return Err(Exception::type_error(ILLEGAL));
    };
    // Step 5: an autonomous custom element extends `HTMLElement`.
    if class != g::HTML_ELEMENT {
        return Err(Exception::type_error(
            "Illegal constructor: autonomous custom elements must extend HTMLElement",
        ));
    }
    // Step 7.
    let prototype = rt.prototype_from_new_target(call, class)?;
    let definition = super::definition(rt, number)?;
    let Some(&top) = definition.construction_stack.last() else {
        // Step 8: a new element of the current window's document.
        let mut data = ElementData::new(
            QualName::new(None, ns!(html), definition.name.clone()),
            Vec::new(),
        );
        data.set_custom_element_state(CustomElementState::Custom, number);
        let created = dom_mut(rt)?.create_node_in(NodeId::DOCUMENT, NodeData::Element(data));
        let element = crate::dom::dom_result(rt, created)?;
        let wrapper = crate::state::wrap_node_with_proto(rt, element, Some(prototype))?;
        return Ok(Value::Object(wrapper));
    };
    // Steps 9 to 13.
    let StackEntry::Element(key, element) = top else {
        return Err(Exception::type_error(
            "This instance is already constructed",
        ));
    };
    let entered = crate::state::switch_if_other(rt, key)?;
    let wrapper = give_prototype(rt, element, prototype);
    crate::state::leave(rt, entered);
    let wrapper = wrapper?;
    if let Some(entry) = super::definition_mut(rt, number)?
        .construction_stack
        .last_mut()
    {
        *entry = StackEntry::AlreadyConstructed(key, element);
    }
    Ok(Value::Object(wrapper))
}

/// The wrapper of an element of the current document with `prototype`:
/// made with it, or given it (HTML: "Perform ? `element.[[SetPrototypeOf]]`").
fn give_prototype(
    rt: &mut Runtime,
    element: NodeId,
    prototype: swb_js::Gc<swb_js::Object>,
) -> b::Result<swb_js::Gc<swb_js::Object>> {
    if let Some(wrapper) = crate::state::existing_wrapper(rt, element)? {
        // An ordinary object refuses only without throwing; HTML ignores it.
        rt.set_prototype(wrapper, Some(prototype))?;
        return Ok(wrapper);
    }
    Ok(crate::state::wrap_node_with_proto(
        rt,
        element,
        Some(prototype),
    )?)
}
