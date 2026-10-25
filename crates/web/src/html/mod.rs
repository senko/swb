//! The HTML element interfaces for scripts (HTML Standard, "Elements",
//! <https://html.spec.whatwg.org/multipage/semantics.html>), feature 6c of
//! M8.
//!
//! - `classes.rs`: which interface an element gets (a table by tag name);
//! - `element.rs`: `HTMLElement` and the elements that have a few members
//!   of their own (`title`, `base`, `a`, `img`, `link`, ...);
//! - `hyperlink.rs`: the URL decomposition members of `a` and `area`
//!   (HTML `HTMLHyperlinkElementUtils`);
//! - `token_lists.rs`: `relList`, `sizes`, `sandbox`, `blocking`,
//!   `htmlFor` and `supports`;
//! - `factories.rs`: the factory functions `Image` and `Option`;
//! - `owner.rs`: form owners and the listed elements of a form;
//! - `form_controls.rs`: `value`, `checked`, `selectedIndex` and the other
//!   members that depend on the state of a control (the engine's form
//!   state, [`crate::EngineHost`]);
//! - `focus.rs`: `focus()`, `blur()`, `activeElement` and `hasFocus()`;
//! - `activation.rs`: `click()`, the activation behaviors and the form
//!   methods `submit()`, `requestSubmit()` and `reset()`;
//! - `forms.rs`: `form`, `label`, `select`, `option`, `fieldset`,
//!   `output`, `textarea` and the collections of a form and a select;
//! - `tables.rs`: `table`, `tbody`, `tr` and `td`.
//!
//! Reflected attributes (`[Reflect]` and its variants) are generated from
//! the IDL (`crates/web/idl/html.idl`); this module has what the standard
//! describes in prose.

pub(crate) mod activation;
mod classes;
pub(crate) mod element;
mod factories;
pub(crate) mod focus;
mod form_controls;
mod forms;
mod hyperlink;
mod owner;
mod tables;
mod token_lists;

pub(crate) use classes::class_of_html;
pub(crate) use form_controls::{after_type_change, before_type_change};
pub(crate) use owner::{
    form_owner, labeled_control, legend_form, option_form, owner_with, select_of_option,
};
pub(crate) use token_lists::{supported_tokens, token_list};

use swb_dom::{LocalName, NodeId, QualName, ns};

use crate::bindings::{self as b, Cx};
use crate::state::dom;

/// The value of the content attribute `name` of `element` (a copy).
pub(crate) fn attribute(cx: &Cx<'_>, element: NodeId, name: &str) -> b::Result<Option<String>> {
    Ok(dom(cx.rt)?
        .element(element)
        .and_then(|e| e.attr(name))
        .map(str::to_owned))
}

/// Writes the value of a text field or text area through the engine. The
/// task ends if the values that scripts wrote reached the budget of the
/// document (ADR 0028 section 16).
pub(crate) fn write_control_value(cx: &mut Cx<'_>, node: NodeId, value: &str) -> b::Result<()> {
    if crate::state::engine_mut(cx.rt)?.set_control_value(node, value)
        == crate::ControlWrite::OverLimit
    {
        return Err(cx.rt.terminate("the budget of form control values").into());
    }
    Ok(())
}

/// Sets the content attribute `name` of `element`.
pub(crate) fn set_attribute(
    cx: &mut Cx<'_>,
    element: NodeId,
    name: &str,
    value: &str,
) -> b::Result<()> {
    let name = QualName::new(None, ns!(), LocalName::from(name));
    crate::dom::set_attribute(cx.rt, element, &name, value)
}

/// Removes the content attribute `name` of `element`.
pub(crate) fn remove_attribute(cx: &mut Cx<'_>, element: NodeId, name: &str) -> b::Result<()> {
    let name = QualName::new(None, ns!(), LocalName::from(name));
    crate::dom::remove_attribute(cx.rt, element, &name)?;
    Ok(())
}

/// True if `element` is an HTML element with the local name `local`.
pub(crate) fn is_html(cx: &Cx<'_>, element: NodeId, local: &str) -> b::Result<bool> {
    Ok(dom(cx.rt)?
        .element(element)
        .is_some_and(|e| e.is_html() && &**e.local_name() == local))
}
