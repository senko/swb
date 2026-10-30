//! The `load` and `error` events of images and style sheets, as the engine
//! delivers them ([`crate::ScriptRuntime::element_events`], feature 10b),
//! and `HTMLImageElement.decode()`.
//!
//! The engine watches the requests of `img` elements and the loads of
//! `link` style sheets (`engine/src/document/element_events.rs`); a result
//! arrives here as `(node, ok)`. The event is a simple trusted `Event` that
//! neither bubbles nor can be canceled (measured, `web-image-events.json`).
//! The promises that `decode()` returned for the element settle first: they
//! resolve for `load`, and reject with an `EncodingError` for `error`.

use swb_dom::NodeId;
use swb_js::{Gc, Object, Runtime, Value, VmResult};

use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::runtime::ScriptRuntime;
use crate::state::{engine_mut, state_mut};

/// The most images with `decode()` promises that wait, and the most
/// promises that wait for one image.
const MAX_DECODES: usize = 65_536;
const MAX_DECODES_PER_IMAGE: usize = 1024;

impl ScriptRuntime {
    /// Fires `load` or `error` at the elements of `events` (each a task;
    /// `true` is `load`).
    pub fn element_events(&mut self, events: Vec<(NodeId, bool)>) {
        for (node, ok) in events {
            let _ = self.task("an image or style sheet event", |rt| {
                deliver(rt, node, ok).map_err(crate::runtime::task_error)
            });
        }
    }
}

fn deliver(rt: &mut Runtime, node: NodeId, ok: bool) -> VmResult<()> {
    let waiting = state_mut(rt)?.decodes.remove(&node).unwrap_or_default();
    for promise in waiting {
        if ok {
            rt.resolve_pending_promise(promise, Value::Undefined)?;
        } else {
            let error = encoding_error(rt)?;
            rt.reject_pending_promise(promise, error)?;
        }
    }
    crate::scripts::fire_at_element(rt, node, if ok { "load" } else { "error" })
}

fn encoding_error(rt: &mut Runtime) -> VmResult<Value> {
    Ok(b::dom_exception::create(
        rt,
        0,
        g::DOM_EXCEPTION,
        "EncodingError",
        "The source image cannot be decoded.",
    )?
    .into())
}

/// `img.decode()`.
pub(crate) fn decode_image(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let info = engine_mut(cx.rt)?.image_info(node);
    let promise = cx.rt.new_pending_promise(realm)?;
    if info.complete {
        if info.natural_width > 0 || info.natural_height > 0 {
            cx.rt.resolve_pending_promise(promise, Value::Undefined)?;
        } else {
            let error = encoding_error(cx.rt)?;
            cx.rt.reject_pending_promise(promise, error)?;
        }
        return Ok(promise);
    }
    let decodes = &mut state_mut(cx.rt)?.decodes;
    let list = decodes.entry(node).or_default();
    if list.len() >= MAX_DECODES_PER_IMAGE || decodes.len() > MAX_DECODES {
        return Err(Exception::dom(
            "EncodingError",
            "Too many decode() calls are waiting.",
        ));
    }
    let list = decodes.entry(node).or_default();
    list.push(promise);
    Ok(promise)
}
