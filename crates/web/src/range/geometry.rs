//! `Range.getClientRects` and `Range.getBoundingClientRect` (CSSOM View
//! §6.2, <https://drafts.csswg.org/cssom-view/#extensions-to-the-range-interface>;
//! M10 sub-feature 9b-3, ADR 0039 section 2). The engine of the range's
//! document flushes the layout and gives the rectangles
//! (`EngineHost::range_rects`); the objects belong to the realm of the method,
//! as for `Element.getClientRects` (measured, case `geometry-frames`).

use swb_js::{Gc, Object};

use super::current;
use crate::bindings::{self as b, Cx, Platform};
use crate::cssom_view;
use crate::host::DomRect;
use crate::state;

/// The client rectangles of the range `this`, in the viewport coordinates of
/// its document.
fn rects(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<DomRect>> {
    let (_, key, range) = current(cx.rt, this)?;
    let entered = state::switch_if_other(cx.rt, key)?;
    let rects = state::engine_laid_out(cx.rt).map(|engine| engine.range_rects(range));
    state::leave(cx.rt, entered);
    let rects = rects?;
    cx.rt.check_time()?;
    Ok(rects)
}

/// `Range.getClientRects`: a new `DOMRectList`.
pub(super) fn get_client_rects(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
    let rects = rects(cx, this)?;
    let realm = cx.realm();
    Ok(cssom_view::rect_list(cx.rt, realm, rects)?)
}

/// `Range.getBoundingClientRect`: the union of the client rectangles with a
/// width and a height, the first one if none has both, or zero.
pub(super) fn get_bounding_client_rect(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
    let rects = rects(cx, this)?;
    let realm = cx.realm();
    Ok(cssom_view::range_bounding_rect(cx.rt, realm, &rects)?)
}
