//! `input`: the members that depend on the type and on the state of the
//! control: `value` (HTML "value" IDL attribute modes), `checked`,
//! `indeterminate`, `form`, `formAction` and `list`.
//!
//! The value of a text field, the checkedness of a checkbox or radio
//! button and the selectedness of options are the engine's form state
//! ([`crate::EngineHost`]); the other values are content attributes.
//!
//! <https://html.spec.whatwg.org/multipage/input.html#dom-input-value>

use swb_dom::NodeId;

use super::element::action_url;
use super::{attribute, set_attribute};
use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::state::{WebState, dom, engine_mut};

/// How the `value` IDL attribute of an input works for its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueMode {
    /// The value is the state of the control (text fields, numbers,
    /// dates, ranges, colors).
    Value,
    /// The value is the `value` attribute (buttons, `hidden`).
    Default,
    /// The value is the `value` attribute, `on` without it (checkboxes
    /// and radio buttons).
    DefaultOn,
    /// The value is the name of the first selected file.
    Filename,
}

/// The value mode of an input with this `type` attribute.
/// <https://html.spec.whatwg.org/multipage/input.html#dom-input-value>
fn mode_of_type(ty: Option<&str>) -> ValueMode {
    match ty.map(str::to_ascii_lowercase).as_deref() {
        Some("hidden" | "submit" | "image" | "reset" | "button") => ValueMode::Default,
        Some("checkbox" | "radio") => ValueMode::DefaultOn,
        Some("file") => ValueMode::Filename,
        _ => ValueMode::Value,
    }
}

/// The value mode of the input `node`.
fn value_mode(cx: &Cx<'_>, node: NodeId) -> b::Result<ValueMode> {
    let ty = attribute(cx, node, "type")?;
    Ok(mode_of_type(ty.as_deref()))
}

/// What a change of the `type` attribute of an input needs: the old
/// value mode and value.
pub(crate) struct TypeChange {
    mode: ValueMode,
    value: String,
}

/// Before a change of the `type` attribute of `element`: the value mode
/// and the value of an input (`None` for the other changes).
pub(crate) fn before_type_change(
    rt: &mut swb_js::Runtime,
    element: NodeId,
    name: &swb_dom::QualName,
) -> swb_js::VmResult<Option<TypeChange>> {
    if !name.ns.is_empty() || &*name.local != "type" {
        return Ok(None);
    }
    let is_input = dom(rt)?
        .element(element)
        .is_some_and(|e| e.is_html() && &**e.local_name() == "input");
    if !is_input {
        return Ok(None);
    }
    let mode = mode_of_type(dom(rt)?.element(element).and_then(|e| e.attr("type")));
    let value = if mode == ValueMode::Value {
        engine_mut(rt)?.control_value(element).unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Some(TypeChange { mode, value }))
}

/// After the change: the value mode switch of the `type` attribute. A
/// value that was the state of the control becomes the `value` content
/// attribute when the new type shows the attribute (`text` to `checkbox`).
/// <https://html.spec.whatwg.org/multipage/input.html#input-type-change>
pub(crate) fn after_type_change(
    rt: &mut swb_js::Runtime,
    element: NodeId,
    change: Option<TypeChange>,
) -> b::Result<()> {
    let Some(change) = change else {
        return Ok(());
    };
    let new = mode_of_type(dom(rt)?.element(element).and_then(|e| e.attr("type")));
    if change.mode == ValueMode::Value
        && !change.value.is_empty()
        && matches!(new, ValueMode::Default | ValueMode::DefaultOn)
    {
        let name = swb_dom::QualName::new(None, swb_dom::ns!(), swb_dom::LocalName::from("value"));
        crate::dom::set_attribute(rt, element, &name, &change.value)?;
    }
    let old_value = (change.mode == ValueMode::Value).then_some(change.value);
    engine_mut(rt)?.input_type_changed(element, old_value);
    Ok(())
}

/// The `width` or `height` content attribute of an image button.
fn image_size(cx: &mut Cx<'_>, this: NodeId, name: &str) -> b::Result<u32> {
    let image = attribute(cx, this, "type")?.is_some_and(|t| t.eq_ignore_ascii_case("image"));
    if !image {
        return Ok(0);
    }
    b::reflect::get_unsigned_long::<WebState>(
        cx,
        this,
        name,
        b::reflect::UnsignedLimit::Range(0, u32::MAX),
        None,
    )
}

impl g::html_input_element::HTMLInputElementImpl for WebState {
    fn selection_start(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<u32>> {
        super::selection::selection_start(cx, this)
    }

    fn set_selection_start(cx: &mut Cx<'_>, this: NodeId, value: Option<u32>) -> b::Result<()> {
        super::selection::set_selection_start(cx, this, value)
    }

    fn selection_end(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<u32>> {
        super::selection::selection_end(cx, this)
    }

    fn set_selection_end(cx: &mut Cx<'_>, this: NodeId, value: Option<u32>) -> b::Result<()> {
        super::selection::set_selection_end(cx, this, value)
    }

    fn selection_direction(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        super::selection::selection_direction(cx, this)
    }

    fn set_selection_direction(
        cx: &mut Cx<'_>,
        this: NodeId,
        value: Option<String>,
    ) -> b::Result<()> {
        super::selection::set_selection_direction(cx, this, value.as_deref())
    }

    fn select(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        super::selection::select(cx, this)
    }

    fn set_range_text_1(cx: &mut Cx<'_>, this: NodeId, replacement: String) -> b::Result<()> {
        super::selection::set_range_text_selection(cx, this, &replacement)
    }

    fn set_range_text_2(
        cx: &mut Cx<'_>,
        this: NodeId,
        replacement: String,
        start: u32,
        end: u32,
        selection_mode: g::types::SelectionMode,
    ) -> b::Result<()> {
        super::selection::set_range_text(cx, this, &replacement, start, end, selection_mode)
    }

    fn set_selection_range(
        cx: &mut Cx<'_>,
        this: NodeId,
        start: u32,
        end: u32,
        direction: Option<String>,
    ) -> b::Result<()> {
        super::selection::set_selection_range(cx, this, start, end, direction.as_deref())
    }

    fn autocomplete(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        super::element::autocomplete(cx, this)
    }

    /// `width` and `height` apply to `type=image`; the others give 0.
    fn width(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        image_size(cx, this, "width")
    }

    fn height(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        image_size(cx, this, "height")
    }

    fn checked(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        if let Some(checked) = engine_mut(cx.rt)?.control_checked(this) {
            return Ok(checked);
        }
        Ok(attribute(cx, this, "checked")?.is_some())
    }

    fn set_checked(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        engine_mut(cx.rt)?.set_control_checked(this, value);
        Ok(())
    }

    fn form(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(super::form_owner(dom(cx.rt)?, this))
    }

    fn form_action(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        action_url(cx, this, "formaction")
    }

    fn indeterminate(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(crate::state::state(cx.rt)?.indeterminate.contains(&this))
    }

    fn set_indeterminate(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        let set = &mut crate::state::state_mut(cx.rt)?.indeterminate;
        if value {
            set.insert(this);
        } else {
            set.remove(&this);
        }
        engine_mut(cx.rt)?.set_control_indeterminate(this, value);
        Ok(())
    }

    /// The `datalist` that the `list` attribute names.
    fn list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let Some(id) = attribute(cx, this, "list")? else {
            return Ok(None);
        };
        let dom = dom(cx.rt)?;
        Ok(dom
            .get_element_by_id(dom.root(this), &id)
            .filter(|&n| dom.is_html_element(n, &swb_dom::local_name!("datalist"))))
    }

    fn value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        match value_mode(cx, this)? {
            ValueMode::Value => {
                if let Some(value) = engine_mut(cx.rt)?.control_value(this) {
                    return Ok(value);
                }
                Ok(attribute(cx, this, "value")?.unwrap_or_default())
            }
            ValueMode::Default => Ok(attribute(cx, this, "value")?.unwrap_or_default()),
            ValueMode::DefaultOn => {
                Ok(attribute(cx, this, "value")?.unwrap_or_else(|| "on".to_owned()))
            }
            ValueMode::Filename => Ok(String::new()),
        }
    }

    fn set_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        match value_mode(cx, this)? {
            ValueMode::Value => super::write_control_value(cx, this, &value),
            ValueMode::Default | ValueMode::DefaultOn => set_attribute(cx, this, "value", &value),
            ValueMode::Filename if value.is_empty() => Ok(()),
            ValueMode::Filename => Err(Exception::dom(
                "InvalidStateError",
                "This input element accepts a filename, which may only be programmatically set to the empty string.",
            )),
        }
    }
}
