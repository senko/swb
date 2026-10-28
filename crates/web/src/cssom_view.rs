//! Geometry, scrolling and the window's metrics for scripts: CSSOM View
//! (<https://drafts.csswg.org/cssom-view/>: `getClientRects`,
//! `getBoundingClientRect`, `offset*`, `client*`, `scroll*`, the scroll
//! methods of the window and of elements, `scrollIntoView`,
//! `elementFromPoint`, `elementsFromPoint`, `scrollingElement`, `screen`,
//! `innerWidth` and the other metrics of the window) and the rectangles of
//! Geometry Interfaces (<https://drafts.fxtf.org/geometry/>: `DOMRect`,
//! `DOMRectReadOnly`, `DOMRectList`). M8 feature 7b; ADR 0028 sections 9
//! and "Feature 7b notes".
//!
//! The engine answers ([`crate::EngineHost`]: `client_rects`,
//! `offset_metrics`, ...); each query that can lay out is followed by
//! [`Runtime::check_time`], since a flush in one native call is invisible to
//! the interpreter's countdown (a loop of style writes and geometry reads
//! must stop at the task's deadline). The window's metrics need no layout.
//!
//! Measured in Chromium 148 (`tools/probes/web-geometry.json`,
//! `web-scroll.json`): `screen` is the viewport (800x600 in the probe's
//! headless Chromium, `colorDepth` 24, orientation `landscape-primary` at
//! angle 0); `outerWidth` and `outerHeight` equal the viewport; `screenX`
//! and `screenY` are 0. The coordinates of the scroll methods are
//! normalized (NaN and the infinities are 0) and clamped by the engine;
//! `behavior: 'smooth'` scrolls at once (Chromium animates: a departure).

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::generated::types::{
    BooleanOrScrollIntoViewOptions, DOMRectInit, ScrollLogicalPosition, ScrollToOptions,
};
use crate::host::{DomRect, ScrollAlign, ScrollRequest};
use crate::state::{WebState, engine, engine_mut, state, state_mut};

// ----- DOMRect -----

/// The data of a `DOMRect` or `DOMRectReadOnly`.
struct RectData(DomRect);

impl GenericData for RectData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// A new `DOMRect` (or another rectangle class) of `realm`.
fn new_rect(rt: &mut Runtime, realm: u32, rect: DomRect) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(RectData(rect))?;
    rt.new_host_object(realm, g::DOM_RECT, data)
}

/// The rectangle of a `DOMRect` or `DOMRectReadOnly`.
fn rect_of(rt: &Runtime, this: Platform) -> b::Result<DomRect> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<RectData>(handle)
            .map_err(VmError::from)?
            .0),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// Changes the rectangle of a `DOMRect`.
fn change_rect(rt: &mut Runtime, this: Platform, f: impl FnOnce(&mut DomRect)) -> b::Result<()> {
    match this.data {
        HostData::Generic(handle) => {
            let data = rt
                .heap_mut()
                .generic_mut::<RectData>(handle)
                .map_err(VmError::from)?;
            f(&mut data.0);
            Ok(())
        }
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// `Math.min`: NaN if either is NaN.
fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.max`: NaN if either is NaN.
fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

fn from_init(init: &DOMRectInit) -> DomRect {
    DomRect {
        x: init.x,
        y: init.y,
        width: init.width,
        height: init.height,
    }
}

impl g::dom_rect_read_only::DOMRectReadOnlyImpl for WebState {
    fn x(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.x)
    }

    fn y(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.y)
    }

    fn width(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.width)
    }

    fn height(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.height)
    }

    fn top(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        let r = rect_of(cx.rt, this)?;
        Ok(min(r.y, r.y + r.height))
    }

    fn right(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        let r = rect_of(cx.rt, this)?;
        Ok(max(r.x, r.x + r.width))
    }

    fn bottom(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        let r = rect_of(cx.rt, this)?;
        Ok(max(r.y, r.y + r.height))
    }

    fn left(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        let r = rect_of(cx.rt, this)?;
        Ok(min(r.x, r.x + r.width))
    }

    fn constructor(
        cx: &mut Cx<'_>,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(RectData(DomRect {
            x,
            y,
            width,
            height,
        }))?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn from_rect(cx: &mut Cx<'_>, other: DOMRectInit) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(RectData(from_init(&other)))?;
        let realm = cx.realm();
        Ok(cx.rt.new_host_object(realm, g::DOM_RECT_READ_ONLY, data)?)
    }

    /// The default `toJSON` (Web IDL §3.7.7.1): an object with the eight
    /// attributes.
    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let r = rect_of(cx.rt, this)?;
        let entries = [
            ("x", r.x),
            ("y", r.y),
            ("width", r.width),
            ("height", r.height),
            ("top", min(r.y, r.y + r.height)),
            ("right", max(r.x, r.x + r.width)),
            ("bottom", max(r.y, r.y + r.height)),
            ("left", min(r.x, r.x + r.width)),
        ];
        let realm = cx.realm();
        let object = cx.rt.new_plain_object(realm)?;
        for (key, value) in entries {
            b::to_js::dictionary_member(cx.rt, object, key, swb_js::Value::number(value))?;
        }
        Ok(object)
    }
}

impl g::dom_rect::DOMRectImpl for WebState {
    fn x(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.x)
    }

    fn set_x(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        change_rect(cx.rt, this, |r| r.x = value)
    }

    fn y(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.y)
    }

    fn set_y(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        change_rect(cx.rt, this, |r| r.y = value)
    }

    fn width(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.width)
    }

    fn set_width(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        change_rect(cx.rt, this, |r| r.width = value)
    }

    fn height(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(rect_of(cx.rt, this)?.height)
    }

    fn set_height(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        change_rect(cx.rt, this, |r| r.height = value)
    }

    fn constructor(
        cx: &mut Cx<'_>,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(RectData(DomRect {
            x,
            y,
            width,
            height,
        }))?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn from_rect(cx: &mut Cx<'_>, other: DOMRectInit) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        Ok(new_rect(cx.rt, realm, from_init(&other))?)
    }
}

// ----- DOMRectList -----

/// The data of a `DOMRectList`: its rectangles.
struct RectListData(Vec<Gc<Object>>);

impl GenericData for RectListData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for rect in &self.0 {
            tracer.object(*rect);
        }
    }

    fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<Gc<Object>>()
    }
}

fn rects_of(rt: &Runtime, this: Platform) -> b::Result<&[Gc<Object>]> {
    match this.data {
        HostData::Generic(handle) => Ok(&rt
            .heap()
            .generic::<RectListData>(handle)
            .map_err(VmError::from)?
            .0),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

impl g::dom_rect_list::DOMRectListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(u32::try_from(rects_of(cx.rt, this)?.len()).unwrap_or(u32::MAX))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<Gc<Object>>> {
        let rects = rects_of(cx.rt, this)?;
        Ok(usize::try_from(index)
            .ok()
            .and_then(|i| rects.get(i))
            .copied())
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

// ----- Element -----

/// The client rectangles of `element` from the engine (a layout flush),
/// with the deadline checked after it.
fn client_rects(rt: &mut Runtime, element: NodeId) -> b::Result<Vec<DomRect>> {
    let rects = engine_mut(rt)?.client_rects(element);
    rt.check_time()?;
    Ok(rects)
}

/// `getClientRects()`: a new `DOMRectList` of new `DOMRect`s.
pub(crate) fn get_client_rects(cx: &mut Cx<'_>, element: NodeId) -> b::Result<Gc<Object>> {
    let rects = client_rects(cx.rt, element)?;
    let realm = cx.realm();
    let mut objects = Vec::with_capacity(rects.len());
    for rect in rects {
        let object = new_rect(cx.rt, realm, rect)?;
        b::keep(cx.rt, object.into());
        objects.push(object);
    }
    let data = cx.rt.alloc_host_data(RectListData(objects))?;
    Ok(cx.rt.new_host_object(realm, g::DOM_RECT_LIST, data)?)
}

/// `getBoundingClientRect()` (CSSOM View §6): the union of the client
/// rectangles that are not empty (zero width and height count as empty
/// only if all are), the first one if all are, or zero.
pub(crate) fn get_bounding_client_rect(cx: &mut Cx<'_>, element: NodeId) -> b::Result<Gc<Object>> {
    let rects = client_rects(cx.rt, element)?;
    let realm = cx.realm();
    Ok(new_rect(cx.rt, realm, bounding_rect(&rects))?)
}

/// The bounding rectangle of client rectangles (CSSOM View
/// `getBoundingClientRect`).
fn bounding_rect(rects: &[DomRect]) -> DomRect {
    let mut sized = rects.iter().filter(|r| r.width != 0.0 || r.height != 0.0);
    let Some(first) = sized.next() else {
        return rects.first().copied().unwrap_or_default();
    };
    let (mut x0, mut y0) = (first.x, first.y);
    let (mut x1, mut y1) = (first.x + first.width, first.y + first.height);
    for r in sized {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.x + r.width);
        y1 = y1.max(r.y + r.height);
    }
    DomRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// `offsetParent` and the `offset*` values of `element`.
pub(crate) fn offset_metrics(rt: &mut Runtime, element: NodeId) -> b::Result<crate::OffsetMetrics> {
    let metrics = engine_mut(rt)?.offset_metrics(element);
    rt.check_time()?;
    Ok(metrics)
}

/// `client*` of `element` (left, top, width, height).
pub(crate) fn client_metrics(rt: &mut Runtime, element: NodeId) -> b::Result<[i32; 4]> {
    let metrics = engine_mut(rt)?.client_metrics(element);
    rt.check_time()?;
    Ok(metrics)
}

/// `scroll*` of `element`.
pub(crate) fn scroll_metrics(rt: &mut Runtime, element: NodeId) -> b::Result<crate::ScrollMetrics> {
    let metrics = engine_mut(rt)?.scroll_metrics(element);
    rt.check_time()?;
    Ok(metrics)
}

/// Scrolls `element` (`None`: the viewport).
pub(crate) fn scroll(
    rt: &mut Runtime,
    element: Option<NodeId>,
    request: ScrollRequest,
) -> b::Result<()> {
    let engine = engine_mut(rt)?;
    match element {
        Some(element) => engine.scroll_element(element, request),
        None => engine.scroll_viewport(request),
    }
    rt.check_time()?;
    Ok(())
}

/// The request of a `ScrollToOptions` (`scroll`, `scrollTo`, `scrollBy`).
pub(crate) fn options_request(options: &ScrollToOptions, by: bool) -> ScrollRequest {
    ScrollRequest {
        x: options.left,
        y: options.top,
        by,
    }
}

/// The request of two coordinates.
pub(crate) fn point_request(x: f64, y: f64, by: bool) -> ScrollRequest {
    ScrollRequest {
        x: Some(x),
        y: Some(y),
        by,
    }
}

fn scroll_align(position: ScrollLogicalPosition) -> ScrollAlign {
    match position {
        ScrollLogicalPosition::Start => ScrollAlign::Start,
        ScrollLogicalPosition::Center => ScrollAlign::Center,
        ScrollLogicalPosition::End => ScrollAlign::End,
        ScrollLogicalPosition::Nearest => ScrollAlign::Nearest,
    }
}

/// `scrollIntoView(arg)`: `true` is `{block: "start", inline: "nearest"}`,
/// `false` is `{block: "end", inline: "nearest"}`.
pub(crate) fn scroll_into_view(
    rt: &mut Runtime,
    element: NodeId,
    arg: BooleanOrScrollIntoViewOptions,
) -> b::Result<()> {
    let (block, inline) = match arg {
        BooleanOrScrollIntoViewOptions::Boolean(true) => (ScrollAlign::Start, ScrollAlign::Nearest),
        BooleanOrScrollIntoViewOptions::Boolean(false) => (ScrollAlign::End, ScrollAlign::Nearest),
        BooleanOrScrollIntoViewOptions::ScrollIntoViewOptions(options) => {
            (scroll_align(options.block), scroll_align(options.inline))
        }
    };
    engine_mut(rt)?.scroll_into_view(element, block, inline);
    rt.check_time()?;
    Ok(())
}

// ----- Document -----

/// `elementFromPoint` and `elementsFromPoint` of the window's document;
/// other documents have no layout.
pub(crate) fn elements_from_point(
    rt: &mut Runtime,
    document: NodeId,
    x: f64,
    y: f64,
    all: bool,
) -> b::Result<Vec<NodeId>> {
    if document != NodeId::DOCUMENT {
        return Ok(Vec::new());
    }
    let elements = engine_mut(rt)?.elements_from_point(x, y, all);
    rt.check_time()?;
    Ok(elements)
}

/// `document.scrollingElement` of the window's document.
pub(crate) fn scrolling_element(rt: &mut Runtime, document: NodeId) -> b::Result<Option<NodeId>> {
    if document != NodeId::DOCUMENT {
        return Ok(None);
    }
    let element = engine_mut(rt)?.scrolling_element();
    rt.check_time()?;
    Ok(element)
}

// ----- Window -----

/// The viewport of the document (no flush).
fn viewport(rt: &Runtime) -> b::Result<crate::Viewport> {
    Ok(engine(rt)?.viewport())
}

/// A length of the viewport as a `long`.
fn long(value: f64) -> i32 {
    // `as` saturates.
    value.round() as i32
}

/// `innerWidth` and `innerHeight` (also `outerWidth` and `outerHeight` and
/// the size of `screen`, as headless Chromium gives them).
pub(crate) fn viewport_size(rt: &Runtime) -> b::Result<(i32, i32)> {
    let v = viewport(rt)?;
    Ok((long(v.width), long(v.height)))
}

/// `devicePixelRatio`.
pub(crate) fn device_pixel_ratio(rt: &Runtime) -> b::Result<f64> {
    Ok(viewport(rt)?.scale)
}

/// `scrollX` and `scrollY` (with the scroll of a `focus()` that waits,
/// which can lay out).
pub(crate) fn scroll_position(rt: &mut Runtime) -> b::Result<(f64, f64)> {
    let position = engine_mut(rt)?.scroll_position();
    rt.check_time()?;
    Ok(position)
}

/// `window.screen`: one object per window, made at the first access.
pub(crate) fn screen(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(screen) = state(cx.rt)?.screen {
        return Ok(screen);
    }
    let realm = cx.realm();
    let object = cx
        .rt
        .new_host_object(realm, g::SCREEN, crate::state::NOT_A_NODE)?;
    state_mut(cx.rt)?.screen = Some(object);
    Ok(object)
}

impl g::screen::ScreenImpl for WebState {
    fn avail_width(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(viewport_size(cx.rt)?.0)
    }

    fn avail_height(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(viewport_size(cx.rt)?.1)
    }

    fn width(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(viewport_size(cx.rt)?.0)
    }

    fn height(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(viewport_size(cx.rt)?.1)
    }

    fn color_depth(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        Ok(24)
    }

    fn pixel_depth(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        Ok(24)
    }

    fn avail_left(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    fn avail_top(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    /// `screen.orientation` (Screen Orientation): one object per window.
    fn orientation(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        if let Some(orientation) = state(cx.rt)?.screen_orientation {
            return Ok(orientation);
        }
        // An `EventTarget` keeps its listeners in its data.
        let data = cx
            .rt
            .alloc_host_data(crate::events::TargetData::default())?;
        let realm = cx.realm();
        let object = cx.rt.new_host_object(realm, g::SCREEN_ORIENTATION, data)?;
        state_mut(cx.rt)?.screen_orientation = Some(object);
        Ok(object)
    }
}

impl g::screen_orientation::ScreenOrientationImpl for WebState {
    fn angle(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }

    /// `landscape-primary` for a viewport at least as wide as high, else
    /// `portrait-primary`.
    fn type_(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        let (width, height) = viewport_size(cx.rt)?;
        Ok(if width >= height {
            "landscape-primary"
        } else {
            "portrait-primary"
        }
        .to_owned())
    }
}
