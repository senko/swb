//! The SVG geometry methods that the target pages call (M8 feature 7b2):
//! `getBBox` (SVG 2 `SVGGraphicsElement`), `getTotalLength`
//! (`SVGGeometryElement`) and `createSVGRect` (`SVGSVGElement`), with
//! Chromium's `SVGRect`.
//!
//! The engine answers from the content of the outer `<svg>` as layout drew
//! it ([`crate::EngineHost::svg_geometry`], a layout flush; the deadline of
//! the task is checked after it). Measured in Chromium 148
//! (`tools/probes/web-svg-geometry.json`): `getBBox` and `createSVGRect`
//! return a new `SVGRect` (not a `DOMRect`) at each call; the box of the
//! `<svg>` is the union of its content in its user space; `getTotalLength`
//! of a basic shape is the length of its outline, of a path the length of
//! its segments (moves add nothing). Departures: Chromium measures curves
//! with its own approximation (a circle of radius 10 is 62.427 long), swb
//! with 64 chords per curve (62.839); an element that swb does not draw
//! has the box 0, 0, 0, 0 and the length 0, where Chromium computes them
//! from the attributes (a `rect` in `defs`: Chromium 0, 0, 5, 5; elements
//! in a nested `<svg>`, which swb draws as part of the outer one: Chromium
//! the box of the nested content); text has no box in swb.

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Tracer, VmError};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::generated::types::SVGBoundingBoxOptions;
use crate::state::{WebState, engine_mut};

/// The data of an `SVGRect`: x, y, width, height.
struct SvgRectData([f32; 4]);

impl GenericData for SvgRectData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// A new `SVGRect`.
fn new_rect(cx: &mut Cx<'_>, values: [f32; 4]) -> b::Result<Gc<Object>> {
    let data = cx.rt.alloc_host_data(SvgRectData(values))?;
    let realm = cx.realm();
    Ok(cx.rt.new_host_object(realm, g::SVG_RECT, data)?)
}

/// The box and the length of `element` from the engine (zeros if it is
/// not drawn).
fn geometry(cx: &mut Cx<'_>, element: NodeId) -> b::Result<([f32; 4], f32)> {
    let found = engine_mut(cx.rt)?.svg_geometry(element);
    cx.rt.check_time()?;
    Ok(found.map_or(([0.0; 4], 0.0), |(r, length)| {
        // `as` saturates; the values come from `f32` layout values.
        let f = |v: f64| v as f32;
        ([f(r.x), f(r.y), f(r.width), f(r.height)], length as f32)
    }))
}

impl g::svg_graphics_element::SVGGraphicsElementImpl for WebState {
    /// `getBBox()`: the options do not change the box (swb's box is the
    /// fill box; Chromium gave the same box with `stroke: true`).
    fn get_b_box(
        cx: &mut Cx<'_>,
        this: NodeId,
        _options: SVGBoundingBoxOptions,
    ) -> b::Result<Gc<Object>> {
        let (rect, _) = geometry(cx, this)?;
        new_rect(cx, rect)
    }
}

impl g::svg_geometry_element::SVGGeometryElementImpl for WebState {
    fn get_total_length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f32> {
        Ok(geometry(cx, this)?.1)
    }
}

/// `createSVGRect()`: a new `SVGRect` of zeros.
pub(super) fn create_svg_rect(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    new_rect(cx, [0.0; 4])
}

/// The values of the `SVGRect` `this`.
fn values(cx: &Cx<'_>, this: Platform) -> b::Result<[f32; 4]> {
    match this.data {
        HostData::Generic(handle) => Ok(cx
            .rt
            .heap()
            .generic::<SvgRectData>(handle)
            .map_err(VmError::from)?
            .0),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// Sets value `index` of the `SVGRect` `this`.
fn set(cx: &mut Cx<'_>, this: Platform, index: usize, value: f32) -> b::Result<()> {
    match this.data {
        HostData::Generic(handle) => {
            let data = cx
                .rt
                .heap_mut()
                .generic_mut::<SvgRectData>(handle)
                .map_err(VmError::from)?;
            if let Some(slot) = data.0.get_mut(index) {
                *slot = value;
            }
            Ok(())
        }
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

impl g::svg_rect::SVGRectImpl for WebState {
    fn x(cx: &mut Cx<'_>, this: Platform) -> b::Result<f32> {
        Ok(values(cx, this)?[0])
    }

    fn set_x(cx: &mut Cx<'_>, this: Platform, value: f32) -> b::Result<()> {
        set(cx, this, 0, value)
    }

    fn y(cx: &mut Cx<'_>, this: Platform) -> b::Result<f32> {
        Ok(values(cx, this)?[1])
    }

    fn set_y(cx: &mut Cx<'_>, this: Platform, value: f32) -> b::Result<()> {
        set(cx, this, 1, value)
    }

    fn width(cx: &mut Cx<'_>, this: Platform) -> b::Result<f32> {
        Ok(values(cx, this)?[2])
    }

    fn set_width(cx: &mut Cx<'_>, this: Platform, value: f32) -> b::Result<()> {
        set(cx, this, 2, value)
    }

    fn height(cx: &mut Cx<'_>, this: Platform) -> b::Result<f32> {
        Ok(values(cx, this)?[3])
    }

    fn set_height(cx: &mut Cx<'_>, this: Platform, value: f32) -> b::Result<()> {
        set(cx, this, 3, value)
    }
}
