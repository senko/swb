//! The script API of the media elements (M14 feature 6a, ADR 0047 section
//! 10; HTML "media elements",
//! <https://html.spec.whatwg.org/multipage/media.html>): the members of
//! `HTMLMediaElement` and `HTMLVideoElement` that the core implements,
//! `MediaError`, `TimeRanges`, and the events that the engine delivers.
//!
//! The state lives in the engine (`engine/src/media/`): every getter asks
//! [`EngineHost::media_state`](crate::EngineHost::media_state), which
//! processes the DOM changes first, so an assignment to `src` shows in
//! `networkState` at once. `buffered` and `seekable` are snapshots (a new
//! `TimeRanges` for each read, as in Chromium). `error` is the same
//! `MediaError` object until the engine records a new error. The events
//! (`loadstart`, `progress`, ..., `error`) arrive as
//! [`ScriptRuntime::media_events`]: each is a media element task that fires
//! a simple event (it neither bubbles nor can be canceled) at an element
//! (also at a `source` element for its `error`).
//!
//! The members of feature 6b (`play`, `pause`, the setter of `currentTime`,
//! `ended`, `played`, `volume`, `muted`, `Audio`) are not here.

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::host::{MediaCommand, MediaErrorInfo};
use crate::runtime::ScriptRuntime;
use crate::state::{Slot, WebState, engine_mut, set_side_object, side_object};

impl ScriptRuntime {
    /// Fires the events of media elements and their `source` children, in
    /// order: each `(node, type)` is a task (the type is `loadstart`,
    /// `error`, ...).
    pub fn media_events(&mut self, events: Vec<(NodeId, &'static str)>) {
        for (node, kind) in events {
            let _ = self.task("a media element event", |rt| {
                crate::scripts::fire_at_element(rt, node, kind).map_err(crate::runtime::task_error)
            });
        }
    }
}

// ----- TimeRanges -----

/// The data of a `TimeRanges`: its ranges in seconds.
struct RangesData(Vec<(f64, f64)>);

impl GenericData for RangesData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<(f64, f64)>()
    }
}

fn new_ranges(rt: &mut Runtime, realm: u32, ranges: Vec<(f64, f64)>) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(RangesData(ranges))?;
    rt.new_host_object(realm, g::TIME_RANGES, data)
}

fn ranges_of(rt: &Runtime, this: Platform) -> b::Result<&[(f64, f64)]> {
    match this.data {
        HostData::Generic(handle) => Ok(&rt
            .heap()
            .generic::<RangesData>(handle)
            .map_err(VmError::from)?
            .0),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// `TimeRanges.start(index)` and `end(index)`: `IndexSizeError` for an
/// index that is not below the length (Chromium's message; the glue adds
/// `Failed to execute 'end' on 'TimeRanges': `).
fn range_at(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<(f64, f64)> {
    let ranges = ranges_of(cx.rt, this)?;
    match usize::try_from(index).ok().and_then(|i| ranges.get(i)) {
        Some(range) => Ok(*range),
        None => Err(Exception::dom(
            "IndexSizeError",
            format!(
                "The index provided ({index}) is greater than the maximum bound ({}).",
                ranges.len()
            ),
        )),
    }
}

impl g::time_ranges::TimeRangesImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(u32::try_from(ranges_of(cx.rt, this)?.len()).unwrap_or(u32::MAX))
    }

    fn start(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<f64> {
        Ok(range_at(cx, this, index)?.0)
    }

    fn end(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<f64> {
        Ok(range_at(cx, this, index)?.1)
    }
}

// ----- MediaError -----

/// The data of a `MediaError`.
struct ErrorData(MediaErrorInfo);

impl GenericData for ErrorData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.0.message.capacity()
    }
}

fn error_of(rt: &Runtime, this: Platform) -> b::Result<&MediaErrorInfo> {
    match this.data {
        HostData::Generic(handle) => Ok(&rt
            .heap()
            .generic::<ErrorData>(handle)
            .map_err(VmError::from)?
            .0),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

impl g::media_error::MediaErrorImpl for WebState {
    fn code(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(error_of(cx.rt, this)?.code)
    }

    fn message(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(error_of(cx.rt, this)?.message.clone())
    }
}

/// The `MediaError` object of `node`: the one that `error` returned before
/// while the engine's error is the same.
fn media_error(cx: &mut Cx<'_>, node: NodeId, info: &MediaErrorInfo) -> VmResult<Gc<Object>> {
    let realm = cx.realm();
    if let Some(object) = side_object(cx.rt, node, Slot::MediaError)?
        && let Some((_, HostData::Generic(handle))) = cx.rt.host_object(object.into())
        && cx
            .rt
            .heap()
            .generic::<ErrorData>(handle)
            .is_ok_and(|data| data.0.serial == info.serial)
    {
        return Ok(object);
    }
    let data = cx.rt.alloc_host_data(ErrorData(info.clone()))?;
    let object = cx.rt.new_host_object(realm, g::MEDIA_ERROR, data)?;
    set_side_object(cx.rt, node, Slot::MediaError, object)?;
    Ok(object)
}

// ----- HTMLMediaElement and HTMLVideoElement -----

impl g::html_media_element::HTMLMediaElementImpl for WebState {
    fn current_src(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(engine_mut(cx.rt)?.media_state(this).current_src)
    }

    fn network_state(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u16> {
        Ok(engine_mut(cx.rt)?.media_state(this).network_state)
    }

    fn buffered(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        let ranges = engine_mut(cx.rt)?.media_state(this).buffered;
        let realm = cx.realm();
        Ok(new_ranges(cx.rt, realm, ranges)?)
    }

    fn ready_state(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u16> {
        Ok(engine_mut(cx.rt)?.media_state(this).ready_state)
    }

    fn seeking(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(engine_mut(cx.rt)?.media_state(this).seeking)
    }

    fn current_time(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(engine_mut(cx.rt)?.media_state(this).current_time)
    }

    fn duration(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(engine_mut(cx.rt)?.media_state(this).duration)
    }

    fn paused(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(engine_mut(cx.rt)?.media_state(this).paused)
    }

    fn seekable(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        let ranges = engine_mut(cx.rt)?.media_state(this).seekable;
        let realm = cx.realm();
        Ok(new_ranges(cx.rt, realm, ranges)?)
    }

    fn error(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<Gc<Object>>> {
        let Some(info) = engine_mut(cx.rt)?.media_state(this).error else {
            return Ok(None);
        };
        Ok(Some(media_error(cx, this, &info)?))
    }

    /// `load()`: the media element load algorithm.
    /// <https://html.spec.whatwg.org/multipage/media.html#dom-media-load>
    fn load(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        engine_mut(cx.rt)?.media_command(this, MediaCommand::Load);
        Ok(())
    }

    /// `canPlayType()`: `""`, `"maybe"` or `"probably"`.
    fn can_play_type(cx: &mut Cx<'_>, _this: NodeId, type_: String) -> b::Result<String> {
        Ok(engine_mut(cx.rt)?.can_play_type(&type_).to_owned())
    }
}

impl g::html_video_element::HTMLVideoElementImpl for WebState {
    fn video_width(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        Ok(engine_mut(cx.rt)?.media_state(this).video_width)
    }

    fn video_height(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        Ok(engine_mut(cx.rt)?.media_state(this).video_height)
    }
}
