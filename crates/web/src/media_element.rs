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
//! The playback API (feature 6b): `play()` and `pause()`, the setter of
//! `currentTime`, `ended`, `played`, `volume`, `muted`, `playbackRate` (a
//! stored value) and the factory `Audio` (`html/factories.rs`). The engine
//! runs the algorithms. The promises of `play()` live here, by element and
//! the engine's number (`DocState::plays`): the engine settles them with
//! the same delivery as the events ([`MediaTask`]), so a promise settles in
//! its place among the events (`play`, `playing`, then the promise).

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::host::{
    MediaCommand, MediaDelivery, MediaErrorInfo, MediaTask, PlayRejection, PlayStart,
};
use crate::runtime::ScriptRuntime;
use crate::state::{Slot, WebState, engine_mut, set_side_object, side_object, state_mut};

impl ScriptRuntime {
    /// Delivers what the engine queued for media elements and their
    /// `source` children, in order: each `(node, task)` is a task of its own
    /// (an event such as `loadstart` or `error`, or the settling of the
    /// `play()` promises of an element).
    pub fn media_events(&mut self, events: Vec<MediaDelivery>) {
        for delivery in events {
            let _ = self.task("a media element event", |rt| {
                deliver(rt, delivery).map_err(crate::runtime::task_error)
            });
        }
    }
}

fn deliver(rt: &mut Runtime, delivery: MediaDelivery) -> VmResult<()> {
    let MediaDelivery {
        target,
        owner,
        epoch,
        task,
    } = delivery;
    // A task that a load of its element dropped since the batch was handed
    // over does not run.
    if !engine_mut(rt)?.media_task_begin(owner, epoch, task) {
        return Ok(());
    }
    match task {
        MediaTask::Event(kind) => crate::scripts::fire_at_element(rt, target, kind),
        MediaTask::ResolvePlay(upto) => settle_plays(rt, owner, upto, None),
        MediaTask::RejectPlay(upto, why) => settle_plays(rt, owner, upto, Some(why)),
    }
}

/// Settles the `play()` promises of `node` with a number up to `upto`:
/// resolves them, or rejects them with `why`.
fn settle_plays(
    rt: &mut Runtime,
    node: NodeId,
    upto: u64,
    why: Option<PlayRejection>,
) -> VmResult<()> {
    let mut settled = Vec::new();
    {
        let plays = &mut state_mut(rt)?.doc.plays;
        if let Some(list) = plays.get_mut(&node) {
            while list.front().is_some_and(|(id, _, _)| *id <= upto) {
                settled.extend(list.pop_front());
            }
            if list.is_empty() {
                plays.remove(&node);
            }
        }
    }
    for (_, promise, realm) in settled {
        match why {
            None => rt.resolve_pending_promise(promise, Value::Undefined)?,
            Some(why) => {
                let error = rejection(rt, realm, why)?;
                rt.reject_pending_promise(promise, error)?;
            }
        }
    }
    Ok(())
}

/// The `DOMException` of a rejected `play()` (Chromium's names and
/// messages, measured).
fn rejection(rt: &mut Runtime, realm: u32, why: PlayRejection) -> VmResult<Value> {
    let (name, message) = match why {
        PlayRejection::Pause => (
            "AbortError",
            "The play() request was interrupted by a call to pause(). https://goo.gl/LdLk22",
        ),
        PlayRejection::Load => (
            "AbortError",
            "The play() request was interrupted by a new load request. https://goo.gl/LdLk22",
        ),
        PlayRejection::NotSupported => (
            "NotSupportedError",
            "Failed to load because no supported source was found.",
        ),
    };
    Ok(b::dom_exception::create(rt, realm, g::DOM_EXCEPTION, name, message)?.into())
}

/// A promise that is already rejected with `name` and `message`.
fn rejected(cx: &mut Cx<'_>, name: &str, message: &str) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let promise = cx.rt.new_pending_promise(realm)?;
    let error = b::dom_exception::create(cx.rt, realm, g::DOM_EXCEPTION, name, message)?;
    cx.rt.reject_pending_promise(promise, error.into())?;
    Ok(promise)
}

/// `play()`.
/// <https://html.spec.whatwg.org/multipage/media.html#dom-media-play>
fn play(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Gc<Object>> {
    match engine_mut(cx.rt)?.media_play(node) {
        PlayStart::Pending(id) => {
            let realm = cx.realm();
            let promise = cx.rt.new_pending_promise(realm)?;
            state_mut(cx.rt)?
                .doc
                .plays
                .entry(node)
                .or_default()
                .push_back((id, promise, realm));
            Ok(promise)
        }
        PlayStart::NotAllowed => rejected(
            cx,
            "NotAllowedError",
            "play() failed because the user didn't interact with the document first. https://goo.gl/xX8pDD",
        ),
        PlayStart::NotSupported => rejected(
            cx,
            "NotSupportedError",
            "The element has no supported sources.",
        ),
        PlayStart::TooMany => rejected(cx, "AbortError", "Too many play() requests are waiting."),
    }
}

/// A number as Chromium writes it in the message of an exception: six
/// significant digits. Measured (probe `media-api-edges.json`): `100000`,
/// `12345.7`, `0.000012345` and `0.06` without trailing zeros; with a
/// decimal exponent below -6 or above 5 (after the rounding) the form
/// `1.50000e-7`, `1.23457e+6`, `1.00000e+9`, with the zeros and the sign.
fn number_text(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return "0".to_owned();
    }
    let scientific = format!("{x:.5e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-6..=5).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{}", exponent.abs());
    }
    let digits = usize::try_from(5 - exponent).unwrap_or(0);
    let text = format!("{x:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Whether Chromium supports `rate` as a playback rate: 0, or from 1/16 to
/// 16 (probe `media-api-edges.json`: 0.0625 and 16 are in, 0.0624 and 16.1
/// are out).
fn supported_rate(rate: f64) -> bool {
    rate == 0.0 || (0.0625..=16.0).contains(&rate)
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

    /// `currentTime = seconds`: HTML "seek" (a value that is no finite
    /// number is a `TypeError` of the generated glue).
    fn set_current_time(cx: &mut Cx<'_>, this: NodeId, value: f64) -> b::Result<()> {
        engine_mut(cx.rt)?.media_command(this, MediaCommand::Seek(value));
        Ok(())
    }

    fn duration(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(engine_mut(cx.rt)?.media_state(this).duration)
    }

    fn paused(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(engine_mut(cx.rt)?.media_state(this).paused)
    }

    fn default_playback_rate(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(engine_mut(cx.rt)?.media_state(this).default_playback_rate)
    }

    /// `defaultPlaybackRate = value`: a rate outside the supported range is
    /// ignored (Chromium throws only for `playbackRate`).
    fn set_default_playback_rate(cx: &mut Cx<'_>, this: NodeId, value: f64) -> b::Result<()> {
        if supported_rate(value) {
            engine_mut(cx.rt)?.media_command(this, MediaCommand::SetDefaultPlaybackRate(value));
        }
        Ok(())
    }

    fn playback_rate(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(engine_mut(cx.rt)?.media_state(this).playback_rate)
    }

    /// `playbackRate = value`: `NotSupportedError` outside the supported
    /// range (the glue adds `Failed to set the 'playbackRate' property on
    /// 'HTMLMediaElement': `). The value is stored only: the playback runs at
    /// rate 1 (backlog).
    fn set_playback_rate(cx: &mut Cx<'_>, this: NodeId, value: f64) -> b::Result<()> {
        if !supported_rate(value) {
            return Err(Exception::dom(
                "NotSupportedError",
                format!(
                    "The provided playback rate ({}) is not in the supported playback range.",
                    number_text(value)
                ),
            ));
        }
        engine_mut(cx.rt)?.media_command(this, MediaCommand::SetPlaybackRate(value));
        Ok(())
    }

    fn played(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        let ranges = engine_mut(cx.rt)?.media_state(this).played;
        let realm = cx.realm();
        Ok(new_ranges(cx.rt, realm, ranges)?)
    }

    fn ended(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(engine_mut(cx.rt)?.media_state(this).ended)
    }

    fn volume(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(engine_mut(cx.rt)?.media_state(this).volume)
    }

    /// `volume = value`: `IndexSizeError` outside 0 to 1 (the glue adds
    /// `Failed to set the 'volume' property on 'HTMLMediaElement': `).
    fn set_volume(cx: &mut Cx<'_>, this: NodeId, value: f64) -> b::Result<()> {
        if !(0.0..=1.0).contains(&value) {
            return Err(Exception::dom(
                "IndexSizeError",
                format!(
                    "The volume provided ({}) is outside the range [0, 1].",
                    number_text(value)
                ),
            ));
        }
        engine_mut(cx.rt)?.media_command(this, MediaCommand::SetVolume(value));
        Ok(())
    }

    fn muted(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(engine_mut(cx.rt)?.media_state(this).muted)
    }

    fn set_muted(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        engine_mut(cx.rt)?.media_command(this, MediaCommand::SetMuted(value));
        Ok(())
    }

    /// `play()`.
    fn play(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        play(cx, this)
    }

    /// `pause()`: HTML "internal pause steps".
    /// <https://html.spec.whatwg.org/multipage/media.html#dom-media-pause>
    fn pause(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        engine_mut(cx.rt)?.media_command(this, MediaCommand::Pause);
        Ok(())
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

impl g::html_audio_element::HTMLAudioElementImpl for WebState {
    /// `new Audio(src)`.
    fn legacy_factory_audio(cx: &mut Cx<'_>, src: Option<String>) -> b::Result<Gc<Object>> {
        crate::html::factories::audio(cx, src.as_deref())
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

#[cfg(test)]
mod tests {
    use super::{number_text, supported_rate};

    /// The texts of the rows `rate message *` of `media-api-edges.json`.
    #[test]
    fn numbers_are_written_as_chromium_writes_them() {
        for (x, text) in [
            (0.06, "0.06"),
            (16.1, "16.1"),
            (100.0, "100"),
            (-1.0, "-1"),
            (-0.5, "-0.5"),
            (1.5, "1.5"),
            (100_000.0, "100000"),
            (12_345.678, "12345.7"),
            (17.123_456_789, "17.1235"),
            (0.000_123_45, "0.00012345"),
            (0.000_012_345, "0.000012345"),
            (1.5e-7, "1.50000e-7"),
            (1e-9, "1.00000e-9"),
            (-1e-9, "-1.00000e-9"),
            (1_234_567.0, "1.23457e+6"),
            (999_999.5, "1.00000e+6"),
            (123_456_789.0, "1.23457e+8"),
            (1e9, "1.00000e+9"),
            (2.5e300, "2.50000e+300"),
        ] {
            assert_eq!(number_text(x), text, "{x}");
        }
    }

    #[test]
    fn the_rates_that_chromium_supports_are_zero_and_one_sixteenth_to_sixteen() {
        for rate in [0.0, 0.0625, 0.07, 0.5, 1.0, 16.0] {
            assert!(supported_rate(rate), "{rate}");
        }
        for rate in [0.06, 0.0624, 16.1, -1.0, 100.0, 1e-9, f64::NAN] {
            assert!(!supported_rate(rate), "{rate}");
        }
    }
}
