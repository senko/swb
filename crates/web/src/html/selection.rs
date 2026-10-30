//! The selection members of `input` and `textarea`: `selectionStart`,
//! `selectionEnd`, `selectionDirection`, `select()`, `setSelectionRange()`
//! and `setRangeText()` (HTML §4.10.5.5, "Common input element APIs").
//!
//! The selection is the engine's ([`crate::EngineHost`]: the caret and the
//! selection of the control's `TextEdit`, in UTF-16 code units for
//! scripts). An `input` of a type to which the API does not apply
//! (`email`, `number`, `checkbox`, ...) gives `null` and throws
//! `InvalidStateError` from the setters, as Chromium does.
//!
//! <https://html.spec.whatwg.org/multipage/input.html#common-input-element-apis>

use swb_dom::NodeId;

use crate::bindings::{self as b, Cx, Exception};
use crate::generated::types::SelectionMode;
use crate::host::{RangeTextMode, SelectionRange};
use crate::state::{dom, engine_mut};

/// The selection of the control, or `None` if the API does not apply.
fn range(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Option<SelectionRange>> {
    let range = engine_mut(cx.rt)?.control_selection(node);
    // The offsets are UTF-16 units: one scan of the value. The interpreter
    // does not see the time of a native call (ADR 0028 section 7).
    cx.rt.check_time()?;
    Ok(range)
}

/// The `InvalidStateError` of an input whose type has no selection.
fn unsupported(cx: &Cx<'_>, node: NodeId) -> b::Result<Exception> {
    let ty = dom(cx.rt)?
        .element(node)
        .and_then(|e| e.attr("type"))
        .map_or_else(|| "text".to_owned(), str::to_ascii_lowercase);
    Ok(Exception::dom(
        "InvalidStateError",
        format!("The input element's type ('{ty}') does not support selection."),
    ))
}

/// The selection of the control or the exception of an input without
/// the API.
fn required(cx: &mut Cx<'_>, node: NodeId) -> b::Result<SelectionRange> {
    match range(cx, node)? {
        Some(range) => Ok(range),
        None => Err(unsupported(cx, node)?),
    }
}

/// `selectionStart`.
pub(crate) fn selection_start(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Option<u32>> {
    Ok(range(cx, node)?.map(|r| r.start))
}

/// `selectionEnd`.
pub(crate) fn selection_end(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Option<u32>> {
    Ok(range(cx, node)?.map(|r| r.end))
}

/// `selectionDirection`: `forward` or `backward`; Chromium (measured) has
/// no `none`.
pub(crate) fn selection_direction(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Option<String>> {
    Ok(range(cx, node)?.map(|r| (if r.backward { "backward" } else { "forward" }).to_owned()))
}

/// Sets the selection (HTML "set the selection range") and stops the
/// task if the control is gone.
fn set(cx: &mut Cx<'_>, node: NodeId, start: u32, end: u32, backward: bool) -> b::Result<()> {
    engine_mut(cx.rt)?.set_control_selection(node, start, end, backward);
    Ok(cx.rt.check_time()?)
}

/// The setter of `selectionStart`: the end moves up to the start if it is
/// before it. `null` is 0.
pub(crate) fn set_selection_start(
    cx: &mut Cx<'_>,
    node: NodeId,
    value: Option<u32>,
) -> b::Result<()> {
    let current = required(cx, node)?;
    let start = value.unwrap_or(0);
    set(cx, node, start, current.end.max(start), current.backward)
}

/// The setter of `selectionEnd`.
pub(crate) fn set_selection_end(
    cx: &mut Cx<'_>,
    node: NodeId,
    value: Option<u32>,
) -> b::Result<()> {
    let current = required(cx, node)?;
    set(
        cx,
        node,
        current.start,
        value.unwrap_or(0),
        current.backward,
    )
}

/// The setter of `selectionDirection`: `backward` makes it backward,
/// anything else forward.
pub(crate) fn set_selection_direction(
    cx: &mut Cx<'_>,
    node: NodeId,
    value: Option<&str>,
) -> b::Result<()> {
    let current = required(cx, node)?;
    let backward = value == Some("backward");
    set(cx, node, current.start, current.end, backward)
}

/// `setSelectionRange(start, end, direction)`.
pub(crate) fn set_selection_range(
    cx: &mut Cx<'_>,
    node: NodeId,
    start: u32,
    end: u32,
    direction: Option<&str>,
) -> b::Result<()> {
    required(cx, node)?;
    set(cx, node, start, end, direction == Some("backward"))
}

/// `select()`: focuses the control (Chromium, measured) and selects all of
/// its text.
pub(crate) fn select(cx: &mut Cx<'_>, node: NodeId) -> b::Result<()> {
    if engine_mut(cx.rt)?.select_control_text(node) {
        cx.rt.check_time()?;
        Ok(super::focus::focus_node(cx.rt, node, false)?)
    } else {
        Ok(())
    }
}

/// `setRangeText(replacement)`: replaces the selection and selects the
/// new text.
pub(crate) fn set_range_text_selection(
    cx: &mut Cx<'_>,
    node: NodeId,
    replacement: &str,
) -> b::Result<()> {
    let current = required(cx, node)?;
    write_range(
        cx,
        node,
        replacement,
        current.start,
        current.end,
        RangeTextMode::Select,
    )
}

/// `setRangeText(replacement, start, end, selectionMode)`.
pub(crate) fn set_range_text(
    cx: &mut Cx<'_>,
    node: NodeId,
    replacement: &str,
    start: u32,
    end: u32,
    mode: SelectionMode,
) -> b::Result<()> {
    required(cx, node)?;
    if start > end {
        return Err(Exception::dom(
            "IndexSizeError",
            format!(
                "The provided start value ({start}) is larger than the provided end value ({end})."
            ),
        ));
    }
    let mode = match mode {
        SelectionMode::Select => RangeTextMode::Select,
        SelectionMode::Start => RangeTextMode::Start,
        SelectionMode::End => RangeTextMode::End,
        SelectionMode::Preserve => RangeTextMode::Preserve,
    };
    write_range(cx, node, replacement, start, end, mode)
}

/// Replaces the range and ends the task when the budget of the values is
/// used up.
fn write_range(
    cx: &mut Cx<'_>,
    node: NodeId,
    replacement: &str,
    start: u32,
    end: u32,
    mode: RangeTextMode,
) -> b::Result<()> {
    let result = engine_mut(cx.rt)?.set_control_range_text(node, replacement, start, end, mode);
    super::over_limit(cx, result)?;
    Ok(cx.rt.check_time()?)
}
