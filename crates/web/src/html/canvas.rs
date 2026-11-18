//! `HTMLCanvasElement.getContext()` (HTML §4.12.5,
//! <https://html.spec.whatwg.org/multipage/canvas.html#dom-canvas-getcontext>).
//!
//! swb has no Canvas 2D, no WebGL and no `OffscreenCanvas` (M12 owner
//! decision), so the method answers `null` for every context id, `2d`
//! included: the standard says `null` when the user agent does not support
//! the context type. A page that feature-detects WebGL (`MapLibre`, Matomo,
//! Leaflet's `L.Browser.canvas` test of `getContext`) then takes its path
//! for a browser without WebGL; this is also what Chromium does when WebGL
//! is off, and the OSM site shows its WebGL message then. The checks of the
//! arguments are Chromium's (measured, probe cases `canvas-2d-context` and
//! `webgl-detection` in `tools/probes/web-osm-gaps.json`): the context id
//! is required and converted to a string, and the options argument is
//! accepted and ignored. `getContext` is the only member of the
//! `HTMLCanvasElement` prototype besides the reflected `width` and `height`.

use swb_dom::NodeId;
use swb_js::{Gc, Object, Value};

use crate::bindings::{self as b, Cx};
use crate::events::{DispatchTarget, TrustedEvent, fire_trusted};
use crate::generated as g;
use crate::state::WebState;

/// The context ids of WebGL (the WebGL specification: `webgl` and `webgl2`;
/// `experimental-webgl` is the legacy alias that Matomo and older pages
/// try).
fn is_webgl(context_id: &str) -> bool {
    matches!(context_id, "webgl" | "webgl2" | "experimental-webgl")
}

impl g::html_canvas_element::HTMLCanvasElementImpl for WebState {
    /// For a WebGL context id the canvas gets a `webglcontextcreationerror`
    /// event first, as the WebGL specification says when the context cannot
    /// be created (§5.2.1, "fire a WebGL context event named
    /// `webglcontextcreationerror`", before the method returns `null`):
    /// `MapLibre` listens to it and puts its `type` in the message of its
    /// error, which OpenStreetMap's code reads (`JSON.parse(error.message).type`).
    /// Departure: the event is an `Event`, not a `WebGLContextEvent` (there
    /// is no `statusMessage`; swb has no such interface).
    fn get_context(
        cx: &mut Cx<'_>,
        this: NodeId,
        context_id: String,
        _options: Value,
    ) -> b::Result<Option<Gc<Object>>> {
        log::debug!("canvas.getContext({context_id:?}): swb has no rendering contexts; null");
        if is_webgl(&context_id) {
            let event = TrustedEvent {
                cancelable: true,
                ..TrustedEvent::new("webglcontextcreationerror")
            };
            fire_trusted(cx.rt, DispatchTarget::Node(this), &event)?;
        }
        Ok(None)
    }
}
