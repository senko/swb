//! `performance` (High Resolution Time, Performance Timeline, User Timing,
//! Navigation Timing, Resource Timing; M8 feature 8a2): `now`, `timeOrigin`,
//! `toJSON`, the entry buffer with `mark`, `measure`, `getEntries*` and
//! `clear*`, `timing`, `navigation` and the navigation entry. The observer
//! is in `performance_observer.rs`.
//!
//! - <https://w3c.github.io/hr-time/>
//! - <https://w3c.github.io/performance-timeline/>
//! - <https://w3c.github.io/user-timing/>
//! - <https://w3c.github.io/navigation-timing/>
//!
//! **The buffer** holds the marks and measures of the document in the order
//! they were made, at most [`MAX_ENTRIES`]; the navigation entry is the
//! implicit first one. A list that a script gets is filtered and sorted by
//! `startTime` (a stable sort, so equal times keep the order of making;
//! measured: marks and measures with one time stay in order). Entries are
//! host objects whose data ([`EntryData`]) holds the name, the times and the
//! serialized `detail`; `detail` is deserialized at its first read and the
//! same object comes back afterwards (measured).
//!
//! **Times.** `startTime` of a mark is `performance.now()` (the document
//! clock) unless given. The load times of the document come from the
//! runtime's own milestones ([`record`]); swb's loader gives no times for the
//! phases of the request, so `fetchStart` and everything up to `responseEnd`
//! is the time origin (0), and `unloadEvent*`, `redirect*` and
//! `secureConnectionStart` are 0, as in Chromium for a document without a
//! previous document of its origin or redirects. The values of the phases that
//! did not happen yet are 0 (measured: Chromium shows `loadEventEnd` 0
//! before the load event ended).

use std::rc::Rc;

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{
    DomStringOrDouble, DomStringOrPerformanceMeasureOptions, NavigationTimingType,
    PerformanceMarkOptions, PerformanceMeasureOptions, RenderBlockingStatusType,
};
use crate::host::{EngineHost, NavigationKind};
use crate::state::{NOT_A_NODE, WebState, engine, state, state_mut};
use crate::structured::{self, Serialized};

/// The most marks and measures in the buffer of a document. A document
/// that makes more still gets its entries, but they are not kept (the
/// buffer holds the first ones), with one warning.
pub(crate) const MAX_ENTRIES: usize = 131_072;

/// The entry types that swb supports (`PerformanceObserver.supportedEntryTypes`).
pub(crate) const SUPPORTED_TYPES: [&str; 3] = ["mark", "measure", "navigation"];

/// The names of the attributes of `PerformanceTiming` (a mark can not have
/// them; `measure` takes them as marks at the time of that phase).
const TIMING_NAMES: [&str; 21] = [
    "navigationStart",
    "unloadEventStart",
    "unloadEventEnd",
    "redirectStart",
    "redirectEnd",
    "fetchStart",
    "domainLookupStart",
    "domainLookupEnd",
    "connectStart",
    "connectEnd",
    "secureConnectionStart",
    "requestStart",
    "responseStart",
    "responseEnd",
    "domLoading",
    "domInteractive",
    "domContentLoadedEventStart",
    "domContentLoadedEventEnd",
    "domComplete",
    "loadEventStart",
    "loadEventEnd",
];

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Mark,
    Measure,
    Navigation,
}

impl EntryKind {
    /// The `entryType`.
    pub(crate) fn type_name(self) -> &'static str {
        match self {
            EntryKind::Mark => "mark",
            EntryKind::Measure => "measure",
            EntryKind::Navigation => "navigation",
        }
    }

    /// The kind of an `entryType`, if swb supports it.
    pub(crate) fn from_type(name: &str) -> Option<EntryKind> {
        match name {
            "mark" => Some(EntryKind::Mark),
            "measure" => Some(EntryKind::Measure),
            "navigation" => Some(EntryKind::Navigation),
            _ => None,
        }
    }
}

/// The host data of an entry object.
struct EntryData {
    kind: EntryKind,
    name: String,
    start: f64,
    duration: f64,
    /// The `detail` as a record (a mark or a measure that has one).
    detail: Option<Rc<Serialized>>,
    /// The deserialized `detail`, made at the first read.
    detail_value: Option<Value>,
}

impl GenericData for EntryData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(value) = self.detail_value {
            tracer.value(value);
        }
    }

    fn heap_size(&self) -> usize {
        self.name.capacity() + self.detail.as_ref().map_or(0, |d| d.byte_size())
    }
}

/// The load milestones of the document, in milliseconds from its time
/// origin (`None` before they happen).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NavigationTimes {
    dom_loading: Option<f64>,
    dom_interactive: Option<f64>,
    content_loaded_start: Option<f64>,
    content_loaded_end: Option<f64>,
    dom_complete: Option<f64>,
    load_start: Option<f64>,
    load_end: Option<f64>,
}

/// A step of the load of the document (HTML "the end", `document_load.rs`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Milestone {
    /// The parser starts (`domLoading`).
    DomLoading,
    /// The parser finished, the readiness is "interactive".
    DomInteractive,
    /// `DOMContentLoaded` is about to fire.
    ContentLoadedStart,
    /// `DOMContentLoaded` was fired.
    ContentLoadedEnd,
    /// The readiness is "complete" and `load` is about to fire
    /// (`domComplete` and `loadEventStart`).
    LoadStart,
    /// `load` was fired.
    LoadEnd,
}

/// Records the time of a load step (the first time only).
pub(crate) fn record(rt: &mut Runtime, milestone: Milestone) {
    let Ok(state) = state_mut(rt) else {
        return;
    };
    let now = state.clock.now();
    let times = &mut state.perf.times;
    let set = |slot: &mut Option<f64>| {
        slot.get_or_insert(now);
    };
    match milestone {
        Milestone::DomLoading => set(&mut times.dom_loading),
        Milestone::DomInteractive => set(&mut times.dom_interactive),
        Milestone::ContentLoadedStart => set(&mut times.content_loaded_start),
        Milestone::ContentLoadedEnd => set(&mut times.content_loaded_end),
        Milestone::LoadStart => {
            set(&mut times.dom_complete);
            set(&mut times.load_start);
        }
        Milestone::LoadEnd => set(&mut times.load_end),
    }
}

/// Records the URL of the lent document as the `name` of the navigation
/// entry (the first time only): the entry keeps the URL of the navigation
/// when `pushState`, `replaceState` or a fragment change the document URL
/// later, as in Chromium.
pub(crate) fn record_url(rt: &mut Runtime) {
    let Ok(url) = engine(rt).map(EngineHost::url) else {
        return;
    };
    if let Ok(state) = state_mut(rt) {
        state.perf.nav_url.get_or_insert(url);
    }
}

/// What a name stands for in `PerformanceTiming`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TimingName {
    /// The name is no attribute of `PerformanceTiming`.
    Other,
    /// A step that did not happen yet.
    NotYet,
    /// The time of the step, in milliseconds from the time origin.
    At(f64),
}

impl NavigationTimes {
    /// The time of an attribute of `PerformanceTiming`.
    fn of_timing_name(&self, name: &str) -> TimingName {
        let step = |time: Option<f64>| time.map_or(TimingName::NotYet, TimingName::At);
        match name {
            "navigationStart"
            | "fetchStart"
            | "domainLookupStart"
            | "domainLookupEnd"
            | "connectStart"
            | "connectEnd"
            | "requestStart"
            | "responseStart"
            | "responseEnd"
            | "unloadEventStart"
            | "unloadEventEnd"
            | "redirectStart"
            | "redirectEnd"
            | "secureConnectionStart" => TimingName::At(0.0),
            "domLoading" => step(self.dom_loading),
            "domInteractive" => step(self.dom_interactive),
            "domContentLoadedEventStart" => step(self.content_loaded_start),
            "domContentLoadedEventEnd" => step(self.content_loaded_end),
            "domComplete" => step(self.dom_complete),
            "loadEventStart" => step(self.load_start),
            "loadEventEnd" => step(self.load_end),
            _ => TimingName::Other,
        }
    }
}

/// The performance state of a document (in the binding state).
#[derive(Debug, Default)]
pub(crate) struct PerfState {
    pub(crate) timing: Option<Gc<Object>>,
    pub(crate) navigation: Option<Gc<Object>>,
    nav_entry: Option<Gc<Object>>,
    /// The URL of the navigation ([`record_url`]).
    nav_url: Option<String>,
    /// The marks and measures, in the order they were made.
    entries: Vec<Gc<Object>>,
    /// The observers that observe something, in the order of their first
    /// `observe` (`performance_observer.rs`).
    pub(crate) observers: Vec<Gc<Object>>,
    /// True while a task that delivers to the observers waits in the queue.
    pub(crate) delivery_queued: bool,
    full_warned: bool,
    pub(crate) times: NavigationTimes,
}

impl PerfState {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in [self.timing, self.navigation, self.nav_entry]
            .into_iter()
            .flatten()
            .chain(self.entries.iter().copied())
            .chain(self.observers.iter().copied())
        {
            tracer.object(object);
        }
    }
}

fn entry_data(rt: &Runtime, this: Platform) -> b::Result<&EntryData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<EntryData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// The kind, name and start of an entry object.
fn info(rt: &Runtime, object: Gc<Object>) -> Option<(EntryKind, &str, f64)> {
    let (_, data) = rt.host_object(Value::Object(object))?;
    let HostData::Generic(handle) = data else {
        return None;
    };
    let data = rt.heap().generic::<EntryData>(handle).ok()?;
    Some((data.kind, data.name.as_str(), data.start))
}

/// The entries of `objects` with the name and the type (if given), sorted
/// by `startTime` (Performance Timeline "filter buffer map by name and
/// type").
pub(crate) fn filter_sorted(
    rt: &Runtime,
    objects: impl Iterator<Item = Gc<Object>>,
    name: Option<&str>,
    entry_type: Option<&str>,
) -> Vec<Gc<Object>> {
    let mut found: Vec<(f64, Gc<Object>)> = objects
        .filter_map(|object| {
            let (kind, entry_name, start) = info(rt, object)?;
            let wanted = name.is_none_or(|n| n == entry_name)
                && entry_type.is_none_or(|t| t == kind.type_name());
            wanted.then_some((start, object))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, object)| object).collect()
}

/// The navigation entry, made at the first use.
fn navigation_entry(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(object) = state(cx.rt)?.perf.nav_entry {
        return Ok(object);
    }
    let name = match &state(cx.rt)?.perf.nav_url {
        Some(url) => url.clone(),
        None => engine(cx.rt)?.url(),
    };
    let data = cx.rt.alloc_host_data(EntryData {
        kind: EntryKind::Navigation,
        name,
        start: 0.0,
        duration: 0.0,
        detail: None,
        detail_value: None,
    })?;
    let object = cx
        .rt
        .new_host_object(cx.realm(), g::PERFORMANCE_NAVIGATION_TIMING, data)?;
    state_mut(cx.rt)?.perf.nav_entry = Some(object);
    Ok(object)
}

/// The buffer of the document: the navigation entry first.
pub(crate) fn buffer(cx: &mut Cx<'_>) -> b::Result<Vec<Gc<Object>>> {
    let nav = navigation_entry(cx)?;
    let mut all = vec![nav];
    all.extend(state(cx.rt)?.perf.entries.iter().copied());
    Ok(all)
}

fn singleton(
    cx: &mut Cx<'_>,
    get: fn(&WebState) -> Option<Gc<Object>>,
    set: fn(&mut WebState, Gc<Object>),
    class: swb_js::ClassId,
) -> b::Result<Gc<Object>> {
    if let Some(object) = get(state(cx.rt)?) {
        return Ok(object);
    }
    let object = cx.rt.new_host_object(cx.realm(), class, NOT_A_NODE)?;
    set(state_mut(cx.rt)?, object);
    Ok(object)
}

// ----- Performance -----

impl g::performance::PerformanceImpl for WebState {
    fn time_origin(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(state(cx.rt)?.clock.time_origin())
    }

    fn timing(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        singleton(
            cx,
            |s| s.perf.timing,
            |s, o| s.perf.timing = Some(o),
            g::PERFORMANCE_TIMING,
        )
    }

    fn navigation(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        singleton(
            cx,
            |s| s.perf.navigation,
            |s, o| s.perf.navigation = Some(o),
            g::PERFORMANCE_NAVIGATION,
        )
    }

    fn now(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(state(cx.rt)?.clock.now())
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let origin = <Self as g::performance::PerformanceImpl>::time_origin(cx, this)?;
        let timing = <Self as g::performance::PerformanceImpl>::timing(cx, this)?;
        let navigation = <Self as g::performance::PerformanceImpl>::navigation(cx, this)?;
        let realm = cx.realm();
        let object = cx.rt.new_plain_object(realm)?;
        for (key, value) in [
            ("timeOrigin", Value::number(origin)),
            ("timing", Value::Object(timing)),
            ("navigation", Value::Object(navigation)),
        ] {
            b::to_js::dictionary_member(cx.rt, object, key, value)?;
        }
        Ok(object)
    }

    fn get_entries(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let all = buffer(cx)?;
        Ok(filter_sorted(cx.rt, all.into_iter(), None, None))
    }

    fn get_entries_by_type(
        cx: &mut Cx<'_>,
        _this: Platform,
        type_: String,
    ) -> b::Result<Vec<Gc<Object>>> {
        let all = buffer(cx)?;
        Ok(filter_sorted(cx.rt, all.into_iter(), None, Some(&type_)))
    }

    fn get_entries_by_name(
        cx: &mut Cx<'_>,
        _this: Platform,
        name: String,
        type_: Option<String>,
    ) -> b::Result<Vec<Gc<Object>>> {
        let all = buffer(cx)?;
        Ok(filter_sorted(
            cx.rt,
            all.into_iter(),
            Some(&name),
            type_.as_deref(),
        ))
    }

    fn clear_resource_timings(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }

    fn set_resource_timing_buffer_size(
        _cx: &mut Cx<'_>,
        _this: Platform,
        _max_size: u32,
    ) -> b::Result<()> {
        Ok(())
    }

    fn mark(
        cx: &mut Cx<'_>,
        _this: Platform,
        mark_name: String,
        mark_options: PerformanceMarkOptions,
    ) -> b::Result<Gc<Object>> {
        let entry = make_mark(cx, mark_name, &mark_options)?;
        add_entry(cx, entry, EntryKind::Mark)?;
        Ok(entry)
    }

    fn clear_marks(cx: &mut Cx<'_>, _this: Platform, mark_name: Option<String>) -> b::Result<()> {
        clear(cx, EntryKind::Mark, mark_name.as_deref())
    }

    fn measure(
        cx: &mut Cx<'_>,
        _this: Platform,
        measure_name: String,
        start_or_measure_options: DomStringOrPerformanceMeasureOptions,
        end_mark: Option<String>,
    ) -> b::Result<Gc<Object>> {
        let entry = make_measure(
            cx,
            measure_name,
            start_or_measure_options,
            end_mark.as_deref(),
        )?;
        add_entry(cx, entry, EntryKind::Measure)?;
        Ok(entry)
    }

    fn clear_measures(
        cx: &mut Cx<'_>,
        _this: Platform,
        measure_name: Option<String>,
    ) -> b::Result<()> {
        clear(cx, EntryKind::Measure, measure_name.as_deref())
    }
}

/// Removes the entries of `kind` (with `name`, if given) from the buffer.
fn clear(cx: &mut Cx<'_>, kind: EntryKind, name: Option<&str>) -> b::Result<()> {
    let entries = std::mem::take(&mut state_mut(cx.rt)?.perf.entries);
    let kept: Vec<Gc<Object>> = entries
        .into_iter()
        .filter(|&object| {
            info(cx.rt, object).is_none_or(|(k, n, _)| k != kind || name.is_some_and(|w| w != n))
        })
        .collect();
    state_mut(cx.rt)?.perf.entries = kept;
    Ok(())
}

/// Puts a new entry in the buffer (if there is room) and queues it for the
/// observers.
fn add_entry(cx: &mut Cx<'_>, entry: Gc<Object>, kind: EntryKind) -> b::Result<()> {
    let perf = &mut state_mut(cx.rt)?.perf;
    if perf.entries.len() < MAX_ENTRIES {
        perf.entries.push(entry);
    } else if !perf.full_warned {
        perf.full_warned = true;
        log::warn!(
            "the performance entry buffer is full ({MAX_ENTRIES} entries): new marks and measures are not kept"
        );
    }
    crate::performance_observer::queue_for_observers(cx.rt, entry, kind)?;
    Ok(())
}

/// A new entry object of `kind`.
fn new_entry(cx: &mut Cx<'_>, data: EntryData) -> b::Result<Gc<Object>> {
    let class = match data.kind {
        EntryKind::Mark => g::PERFORMANCE_MARK,
        EntryKind::Measure => g::PERFORMANCE_MEASURE,
        EntryKind::Navigation => g::PERFORMANCE_NAVIGATION_TIMING,
    };
    let data = cx.rt.alloc_host_data(data)?;
    Ok(cx.rt.new_host_object(cx.realm(), class, data)?)
}

/// The record of a `detail` (`undefined` and `null` give none).
fn serialize_detail(cx: &mut Cx<'_>, detail: Option<Value>) -> b::Result<Option<Rc<Serialized>>> {
    match detail {
        None | Some(Value::Undefined | Value::Null) => Ok(None),
        Some(value) => Ok(Some(Rc::new(structured::serialize(cx.rt, value)?))),
    }
}

/// The `PerformanceMark` constructor steps (User Timing §2.2.1), up to the
/// object.
fn mark_data(
    cx: &mut Cx<'_>,
    name: String,
    options: &PerformanceMarkOptions,
) -> b::Result<EntryData> {
    if TIMING_NAMES.contains(&name.as_str()) {
        return Err(Exception::dom(
            "SyntaxError",
            format!(
                "'{name}' is part of the PerformanceTiming interface, and cannot be used as a mark name."
            ),
        ));
    }
    let start = match options.start_time {
        Some(time) if time < 0.0 => {
            return Err(Exception::type_error(format!(
                "'{name}' cannot have a negative start time."
            )));
        }
        Some(time) => time,
        None => state(cx.rt)?.clock.now(),
    };
    let detail = serialize_detail(cx, options.detail)?;
    Ok(EntryData {
        kind: EntryKind::Mark,
        name,
        start,
        duration: 0.0,
        detail,
        detail_value: None,
    })
}

/// A new mark for `performance.mark()`.
fn make_mark(
    cx: &mut Cx<'_>,
    name: String,
    options: &PerformanceMarkOptions,
) -> b::Result<Gc<Object>> {
    let data = mark_data(cx, name, options)?;
    new_entry(cx, data)
}

/// "Convert a mark to a timestamp" (User Timing §3): a number, the name of
/// a `PerformanceTiming` attribute or the most recent mark of that name.
fn timestamp_of(cx: &mut Cx<'_>, mark: &DomStringOrDouble, measure_name: &str) -> b::Result<f64> {
    let name = match mark {
        DomStringOrDouble::Double(time) if *time < 0.0 => {
            return Err(Exception::type_error(format!(
                "'{measure_name}' cannot have a negative time stamp."
            )));
        }
        DomStringOrDouble::Double(time) => return Ok(*time),
        DomStringOrDouble::DomString(name) => name,
    };
    match state(cx.rt)?.perf.times.of_timing_name(name) {
        TimingName::At(time) => return Ok(time),
        TimingName::NotYet => {
            return Err(Exception::dom(
                "InvalidAccessError",
                format!(
                    "'{name}' is empty: either the event hasn't happened yet, or it would provide cross-origin timing information."
                ),
            ));
        }
        TimingName::Other => {}
    }
    let rt: &Runtime = cx.rt;
    state(rt)?
        .perf
        .entries
        .iter()
        .rev()
        .find_map(|&object| {
            info(rt, object)
                .filter(|(kind, entry_name, _)| *kind == EntryKind::Mark && entry_name == name)
                .map(|(_, _, start)| start)
        })
        .ok_or_else(|| Exception::dom("SyntaxError", format!("The mark '{name}' does not exist.")))
}

/// "Measure a performance entry" (User Timing §2.1.3) with Chromium's
/// errors.
fn make_measure(
    cx: &mut Cx<'_>,
    name: String,
    start_or_options: DomStringOrPerformanceMeasureOptions,
    end_mark: Option<&str>,
) -> b::Result<Gc<Object>> {
    let (start_mark, options) = match start_or_options {
        DomStringOrPerformanceMeasureOptions::DomString(mark) => {
            (Some(DomStringOrDouble::DomString(mark)), None)
        }
        DomStringOrPerformanceMeasureOptions::PerformanceMeasureOptions(options) => {
            (None, Some(options))
        }
    };
    let options = options.unwrap_or(PerformanceMeasureOptions {
        detail: None,
        duration: None,
        end: None,
        start: None,
    });
    if options.start.is_some()
        || options.end.is_some()
        || options.duration.is_some()
        || options.detail.is_some()
    {
        check_options(&options, end_mark.is_some())?;
    }
    let end = if let Some(mark) = end_mark {
        let mark = DomStringOrDouble::DomString(mark.to_owned());
        Some(timestamp_of(cx, &mark, &name)?)
    } else if let Some(end) = &options.end {
        Some(timestamp_of(cx, end, &name)?)
    } else if let (Some(start), Some(duration)) = (&options.start, options.duration) {
        Some(timestamp_of(cx, start, &name)? + duration)
    } else {
        None
    };
    let start = if let Some(start) = &options.start {
        timestamp_of(cx, start, &name)?
    } else if let (Some(duration), Some(end)) = (options.duration, &options.end) {
        timestamp_of(cx, end, &name)? - duration
    } else if let Some(mark) = &start_mark {
        timestamp_of(cx, mark, &name)?
    } else {
        0.0
    };
    let end = match end {
        Some(end) => end,
        None => state(cx.rt)?.clock.now(),
    };
    let detail = serialize_detail(cx, options.detail)?;
    new_entry(
        cx,
        EntryData {
            kind: EntryKind::Measure,
            name,
            start,
            duration: end - start,
            detail,
            detail_value: None,
        },
    )
}

/// The checks on a non-empty `PerformanceMeasureOptions` (Chromium's words).
fn check_options(options: &PerformanceMeasureOptions, end_mark_given: bool) -> b::Result<()> {
    if end_mark_given {
        return Err(Exception::type_error(
            "If a non-empty PerformanceMeasureOptions object was passed, |end_mark| must not be passed.",
        ));
    }
    if options.start.is_none() && options.end.is_none() {
        return Err(Exception::type_error(
            "If a non-empty PerformanceMeasureOptions object was passed, at least one of its 'start' or 'end' properties must be present.",
        ));
    }
    if options.start.is_some() && options.end.is_some() && options.duration.is_some() {
        return Err(Exception::type_error(
            "If a non-empty PerformanceMeasureOptions object was passed, it must not have all of its 'start', 'duration', and 'end' properties defined",
        ));
    }
    Ok(())
}

// ----- Entries -----

/// The `detail` of a mark or a measure: the same object at each read.
fn detail_of(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
    let data = entry_data(cx.rt, this)?;
    if let Some(value) = data.detail_value {
        return Ok(value);
    }
    let Some(record) = data.detail.clone() else {
        return Ok(Value::Null);
    };
    let value = structured::deserialize(cx.rt, cx.realm(), &record)?;
    if let HostData::Generic(handle) = this.data {
        cx.rt
            .heap_mut()
            .generic_mut::<EntryData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation"))?
            .detail_value = Some(value);
    }
    Ok(value)
}

impl g::performance_entry::PerformanceEntryImpl for WebState {
    fn name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(entry_data(cx.rt, this)?.name.clone())
    }

    fn entry_type(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(entry_data(cx.rt, this)?.kind.type_name().to_owned())
    }

    fn start_time(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(entry_data(cx.rt, this)?.start)
    }

    fn duration(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        let data = entry_data(cx.rt, this)?;
        if data.kind == EntryKind::Navigation {
            // The duration of the navigation entry is `loadEventEnd`.
            return Ok(state(cx.rt)?.perf.times.load_end.unwrap_or(0.0));
        }
        Ok(data.duration)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let (kind, name, start, duration) = {
            let data = entry_data(cx.rt, this)?;
            (data.kind, data.name.clone(), data.start, data.duration)
        };
        let duration = if kind == EntryKind::Navigation {
            state(cx.rt)?.perf.times.load_end.unwrap_or(0.0)
        } else {
            duration
        };
        let realm = cx.realm();
        let object = cx.rt.new_plain_object(realm)?;
        let name = b::to_js::string(cx.rt, &name)?;
        let entry_type = b::to_js::string(cx.rt, kind.type_name())?;
        for (key, value) in [
            ("name", name),
            ("entryType", entry_type),
            ("startTime", Value::number(start)),
            ("duration", Value::number(duration)),
        ] {
            b::to_js::dictionary_member(cx.rt, object, key, value)?;
        }
        if kind == EntryKind::Navigation {
            navigation_members(cx, object)?;
        }
        Ok(object)
    }
}

impl g::performance_mark::PerformanceMarkImpl for WebState {
    fn detail(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        detail_of(cx, this)
    }

    fn constructor(
        cx: &mut Cx<'_>,
        mark_name: String,
        mark_options: PerformanceMarkOptions,
    ) -> b::Result<Gc<Object>> {
        // The constructor does not put the mark in the buffer (measured).
        let data = mark_data(cx, mark_name, &mark_options)?;
        let data = cx.rt.alloc_host_data(data)?;
        Ok(cx.new_instance::<WebState>(data)?)
    }
}

impl g::performance_measure::PerformanceMeasureImpl for WebState {
    fn detail(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        detail_of(cx, this)
    }
}

// ----- Navigation Timing -----

/// `performance.timing`: whole milliseconds since the epoch.
fn timing_ms(cx: &mut Cx<'_>, rel: Option<f64>) -> b::Result<u64> {
    let Some(rel) = rel else {
        return Ok(0);
    };
    let origin = state(cx.rt)?.clock.time_origin();
    Ok((origin + rel).floor().max(0.0) as u64)
}

/// A `PerformanceTiming` attribute by name.
fn timing_attribute(cx: &mut Cx<'_>, name: &str) -> b::Result<u64> {
    let zero = matches!(
        name,
        "unloadEventStart"
            | "unloadEventEnd"
            | "redirectStart"
            | "redirectEnd"
            | "secureConnectionStart"
    );
    let rel = match state(cx.rt)?.perf.times.of_timing_name(name) {
        TimingName::At(time) if !zero => Some(time),
        _ => None,
    };
    timing_ms(cx, rel)
}

macro_rules! timing_attributes {
    ($($method:ident => $name:literal),* $(,)?) => {
        impl g::performance_timing::PerformanceTimingImpl for WebState {
            $(
                fn $method(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
                    timing_attribute(cx, $name)
                }
            )*

            fn to_json(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
                let realm = cx.realm();
                let object = cx.rt.new_plain_object(realm)?;
                for name in TIMING_NAMES {
                    let value = timing_attribute(cx, name)?;
                    b::to_js::dictionary_member(cx.rt, object, name, Value::number(value as f64))?;
                }
                Ok(object)
            }
        }
    };
}

timing_attributes! {
    navigation_start => "navigationStart",
    unload_event_start => "unloadEventStart",
    unload_event_end => "unloadEventEnd",
    redirect_start => "redirectStart",
    redirect_end => "redirectEnd",
    fetch_start => "fetchStart",
    domain_lookup_start => "domainLookupStart",
    domain_lookup_end => "domainLookupEnd",
    connect_start => "connectStart",
    connect_end => "connectEnd",
    secure_connection_start => "secureConnectionStart",
    request_start => "requestStart",
    response_start => "responseStart",
    response_end => "responseEnd",
    dom_loading => "domLoading",
    dom_interactive => "domInteractive",
    dom_content_loaded_event_start => "domContentLoadedEventStart",
    dom_content_loaded_event_end => "domContentLoadedEventEnd",
    dom_complete => "domComplete",
    load_event_start => "loadEventStart",
    load_event_end => "loadEventEnd",
}

impl g::performance_navigation::PerformanceNavigationImpl for WebState {
    fn type_(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(match engine(cx.rt)?.navigation_info().kind {
            NavigationKind::Navigate => 0,
            NavigationKind::Reload => 1,
            NavigationKind::BackForward => 2,
        })
    }

    fn redirect_count(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let kind = <Self as g::performance_navigation::PerformanceNavigationImpl>::type_(cx, this)?;
        let realm = cx.realm();
        let object = cx.rt.new_plain_object(realm)?;
        b::to_js::dictionary_member(cx.rt, object, "type", Value::number(f64::from(kind)))?;
        b::to_js::dictionary_member(cx.rt, object, "redirectCount", Value::number(0.0))?;
        Ok(object)
    }
}

/// A value of the navigation entry, read from the load times.
fn nav_time(cx: &mut Cx<'_>, pick: impl Fn(&NavigationTimes) -> Option<f64>) -> b::Result<f64> {
    Ok(pick(&state(cx.rt)?.perf.times).unwrap_or(0.0))
}

/// The members of the navigation entry beyond the four of every entry, as
/// `toJSON` lists them (the attributes of `PerformanceResourceTiming`, then
/// those of `PerformanceNavigationTiming`).
#[allow(clippy::too_many_lines, reason = "one line per member of the entry")]
fn navigation_members(cx: &mut Cx<'_>, object: Gc<Object>) -> b::Result<()> {
    use g::performance_navigation_timing::PerformanceNavigationTimingImpl as Nav;
    use g::performance_resource_timing::PerformanceResourceTimingImpl as Res;
    let this = Platform {
        object,
        data: NOT_A_NODE,
    };
    let strings = [
        (
            "initiatorType",
            <WebState as Res>::initiator_type(cx, this)?,
        ),
        ("deliveryType", <WebState as Res>::delivery_type(cx, this)?),
        (
            "nextHopProtocol",
            <WebState as Res>::next_hop_protocol(cx, this)?,
        ),
        (
            "renderBlockingStatus",
            <WebState as Res>::render_blocking_status(cx, this)?
                .as_str()
                .to_owned(),
        ),
        ("contentType", <WebState as Res>::content_type(cx, this)?),
        (
            "contentEncoding",
            <WebState as Res>::content_encoding(cx, this)?,
        ),
    ];
    for (key, text) in strings {
        let value = b::to_js::string(cx.rt, &text)?;
        b::to_js::dictionary_member(cx.rt, object, key, value)?;
    }
    let numbers = [
        ("workerStart", <WebState as Res>::worker_start(cx, this)?),
        (
            "redirectStart",
            <WebState as Res>::redirect_start(cx, this)?,
        ),
        ("redirectEnd", <WebState as Res>::redirect_end(cx, this)?),
        ("fetchStart", <WebState as Res>::fetch_start(cx, this)?),
        (
            "domainLookupStart",
            <WebState as Res>::domain_lookup_start(cx, this)?,
        ),
        (
            "domainLookupEnd",
            <WebState as Res>::domain_lookup_end(cx, this)?,
        ),
        ("connectStart", <WebState as Res>::connect_start(cx, this)?),
        (
            "secureConnectionStart",
            <WebState as Res>::secure_connection_start(cx, this)?,
        ),
        ("connectEnd", <WebState as Res>::connect_end(cx, this)?),
        ("requestStart", <WebState as Res>::request_start(cx, this)?),
        (
            "responseStart",
            <WebState as Res>::response_start(cx, this)?,
        ),
        (
            "firstInterimResponseStart",
            <WebState as Res>::first_interim_response_start(cx, this)?,
        ),
        (
            "finalResponseHeadersStart",
            <WebState as Res>::final_response_headers_start(cx, this)?,
        ),
        ("responseEnd", <WebState as Res>::response_end(cx, this)?),
        (
            "transferSize",
            <WebState as Res>::transfer_size(cx, this)? as f64,
        ),
        (
            "encodedBodySize",
            <WebState as Res>::encoded_body_size(cx, this)? as f64,
        ),
        (
            "decodedBodySize",
            <WebState as Res>::decoded_body_size(cx, this)? as f64,
        ),
        (
            "responseStatus",
            f64::from(<WebState as Res>::response_status(cx, this)?),
        ),
        (
            "unloadEventStart",
            <WebState as Nav>::unload_event_start(cx, this)?,
        ),
        (
            "unloadEventEnd",
            <WebState as Nav>::unload_event_end(cx, this)?,
        ),
        (
            "domInteractive",
            <WebState as Nav>::dom_interactive(cx, this)?,
        ),
        (
            "domContentLoadedEventStart",
            <WebState as Nav>::dom_content_loaded_event_start(cx, this)?,
        ),
        (
            "domContentLoadedEventEnd",
            <WebState as Nav>::dom_content_loaded_event_end(cx, this)?,
        ),
        ("domComplete", <WebState as Nav>::dom_complete(cx, this)?),
        (
            "loadEventStart",
            <WebState as Nav>::load_event_start(cx, this)?,
        ),
        ("loadEventEnd", <WebState as Nav>::load_event_end(cx, this)?),
    ];
    for (key, number) in numbers {
        b::to_js::dictionary_member(cx.rt, object, key, Value::number(number))?;
    }
    let kind = <WebState as Nav>::type_(cx, this)?;
    let kind = b::to_js::string(cx.rt, kind.as_str())?;
    b::to_js::dictionary_member(cx.rt, object, "type", kind)?;
    let count = f64::from(<WebState as Nav>::redirect_count(cx, this)?);
    b::to_js::dictionary_member(cx.rt, object, "redirectCount", Value::number(count))?;
    let restart = <WebState as Nav>::critical_ch_restart(cx, this)?;
    b::to_js::dictionary_member(cx.rt, object, "criticalCHRestart", Value::number(restart))?;
    Ok(())
}

impl g::performance_resource_timing::PerformanceResourceTimingImpl for WebState {
    fn initiator_type(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("navigation".to_owned())
    }

    fn delivery_type(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn next_hop_protocol(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn worker_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn redirect_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn redirect_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn fetch_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn domain_lookup_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn domain_lookup_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn connect_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn connect_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn secure_connection_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn request_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn final_response_headers_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn first_interim_response_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn response_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn response_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn transfer_size(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(engine(cx.rt)?.navigation_info().body_bytes)
    }

    fn encoded_body_size(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(engine(cx.rt)?.navigation_info().body_bytes)
    }

    fn decoded_body_size(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u64> {
        Ok(engine(cx.rt)?.navigation_info().body_bytes)
    }

    fn response_status(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(engine(cx.rt)?.navigation_info().status)
    }

    fn render_blocking_status(
        _cx: &mut Cx<'_>,
        _this: Platform,
    ) -> b::Result<RenderBlockingStatusType> {
        Ok(RenderBlockingStatusType::NonBlocking)
    }

    fn content_type(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(engine(cx.rt)?.content_type().to_owned())
    }

    fn content_encoding(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        <WebState as g::performance_entry::PerformanceEntryImpl>::to_json(cx, this)
    }
}

impl g::performance_navigation_timing::PerformanceNavigationTimingImpl for WebState {
    fn unload_event_start(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn unload_event_end(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn dom_interactive(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.dom_interactive)
    }

    fn dom_content_loaded_event_start(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.content_loaded_start)
    }

    fn dom_content_loaded_event_end(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.content_loaded_end)
    }

    fn dom_complete(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.dom_complete)
    }

    fn load_event_start(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.load_start)
    }

    fn load_event_end(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        nav_time(cx, |t| t.load_end)
    }

    fn type_(cx: &mut Cx<'_>, _this: Platform) -> b::Result<NavigationTimingType> {
        Ok(match engine(cx.rt)?.navigation_info().kind {
            NavigationKind::Navigate => NavigationTimingType::Navigate,
            NavigationKind::Reload => NavigationTimingType::Reload,
            NavigationKind::BackForward => NavigationTimingType::BackForward,
        })
    }

    fn redirect_count(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }

    fn critical_ch_restart(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        <WebState as g::performance_entry::PerformanceEntryImpl>::to_json(cx, this)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timing_names_are_the_attributes_of_performance_timing() {
        let times = NavigationTimes::default();
        for name in TIMING_NAMES {
            assert_ne!(times.of_timing_name(name), TimingName::Other, "{name}");
        }
        assert_eq!(times.of_timing_name("loadEventEnd"), TimingName::NotYet);
        assert_eq!(times.of_timing_name("fetchStart"), TimingName::At(0.0));
        assert_eq!(times.of_timing_name("mine"), TimingName::Other);
    }

    #[test]
    fn a_measure_with_all_three_bounds_is_refused() {
        let options = PerformanceMeasureOptions {
            detail: None,
            duration: Some(1.0),
            end: Some(DomStringOrDouble::Double(3.0)),
            start: Some(DomStringOrDouble::Double(1.0)),
        };
        assert!(check_options(&options, false).is_err());
        let options = PerformanceMeasureOptions {
            duration: None,
            ..options
        };
        assert!(check_options(&options, false).is_ok());
        assert!(check_options(&options, true).is_err());
    }

    #[test]
    fn the_entry_types_round_trip() {
        for name in SUPPORTED_TYPES {
            assert_eq!(
                EntryKind::from_type(name).map(EntryKind::type_name),
                Some(name)
            );
        }
        assert!(EntryKind::from_type("resource").is_none());
    }
}
