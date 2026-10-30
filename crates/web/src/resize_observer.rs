//! `ResizeObserver`, `ResizeObserverEntry` and `ResizeObserverSize` (Resize
//! Observer, <https://drafts.csswg.org/resize-observer/>; M8 feature 9a).
//!
//! The rendering update runs the loop of the specification after style and
//! layout ([`crate::ScriptRuntime::run_resize_observers`]): "gather active
//! resize observations at depth", "broadcast active resize observations"
//! (one task per observer callback), again below the depth of the
//! shallowest target that was delivered, until nothing is active; targets
//! that stayed skipped fire the loop error at the window ("`ResizeObserver`
//! loop completed with undelivered notifications."). A skipped target stays
//! active, so the next frame delivers it: a callback that resizes its target
//! without end runs once per frame and reports the error each time.
//!
//! Measured in Chromium 148 (`tools/probes/web-resize-observer.json`): a new
//! observation is active at once (the last reported size starts as none), so
//! a 0x0 element, a `display: none` element, a detached element and an inline
//! element are delivered with size 0; observing a target again replaces its
//! observation; the sizes are layout units (multiples of 1/64 px);
//! `devicePixelContentBoxSize` is the content size times the scale, rounded;
//! padding, border, margin and transforms do not change the content box.
//!
//! Lifetime: an observer lives while it has targets (the state holds it).
//!
//! Departures: no vertical writing modes (swb has no `writing-mode`); the
//! arrays of an entry are new at each read (Chromium keeps one); `zoom` is not
//! undone; an SVG element without a CSS box reports its bounding box.

use std::collections::{BTreeMap, HashMap};

use swb_dom::NodeId;
use swb_js::{
    Gc, GenericData, HostData, Object, Runtime, ScriptError, Tracer, Value, VmError, VmResult,
};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::cssom_view::new_rect_read_only;
use crate::generated as g;
use crate::generated::types::{
    ResizeObserverBoxOptions, ResizeObserverCallback, ResizeObserverOptions,
};
use crate::host::DomRect;
use crate::state::{WebState, dom, engine_mut, state, state_mut};

/// The most targets that all resize observers of a document observe.
pub(crate) const MAX_TARGETS: usize = 1 << 20;

/// The box a target is observed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoxKind {
    Content,
    Border,
    DevicePixel,
}

/// One observation.
#[derive(Clone, Copy, Debug)]
struct Target {
    node: NodeId,
    kind: BoxKind,
    /// The last reported size of the observed box (none: nothing reported).
    last: Option<(f64, f64)>,
}

/// The sizes of a target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sizes {
    border: (f64, f64),
    content: (f64, f64),
    device: (f64, f64),
    origin: (f64, f64),
}

impl Sizes {
    /// The size of the box `kind`.
    fn of(&self, kind: BoxKind) -> (f64, f64) {
        match kind {
            BoxKind::Content => self.content,
            BoxKind::Border => self.border,
            BoxKind::DevicePixel => self.device,
        }
    }
}

struct Observer {
    object: Gc<Object>,
    /// The observations in the order of the `observe()` calls, by number.
    targets: BTreeMap<u64, Target>,
    /// The number of the observation of each target node.
    index: HashMap<NodeId, u64>,
    next_seq: u64,
}

impl Observer {
    /// Adds an observation at the end; one for the same node is replaced.
    /// Returns true if it replaced one.
    fn add(&mut self, target: Target) -> bool {
        let replaced = self
            .index
            .remove(&target.node)
            .is_some_and(|seq| self.targets.remove(&seq).is_some());
        self.next_seq += 1;
        self.index.insert(target.node, self.next_seq);
        self.targets.insert(self.next_seq, target);
        replaced
    }

    /// Removes the observation of `node`; true if there was one.
    fn remove(&mut self, node: NodeId) -> bool {
        self.index
            .remove(&node)
            .is_some_and(|seq| self.targets.remove(&seq).is_some())
    }
}

/// The resize observers of the binding state.
#[derive(Default)]
pub(crate) struct RoState {
    next_id: u32,
    observers: BTreeMap<u32, Observer>,
    targets: usize,
    /// An observation was added since the last run of the loop.
    requested: bool,
}

impl RoState {
    /// True if an observation was added since the last run of the loop.
    pub(crate) fn requested(&self) -> bool {
        self.requested
    }

    /// Reports the handles that the state holds.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for observer in self.observers.values() {
            tracer.object(observer.object);
        }
    }

    /// True if an observer has targets.
    pub(crate) fn is_observing(&self) -> bool {
        !self.observers.is_empty()
    }
}

struct ObserverData {
    id: u32,
    callback: Gc<Object>,
}

impl GenericData for ObserverData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.callback);
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// The host data of a `ResizeObserverEntry`.
struct EntryData {
    target: NodeId,
    sizes: Sizes,
    content_rect: Option<Gc<Object>>,
}

impl GenericData for EntryData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(rect) = self.content_rect {
            tracer.object(rect);
        }
    }

    fn heap_size(&self) -> usize {
        size_of::<Sizes>()
    }
}

/// The host data of a `ResizeObserverSize`.
struct SizeData {
    inline: f64,
    block: f64,
}

impl GenericData for SizeData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

fn observer_id(rt: &Runtime, this: Platform) -> b::Result<u32> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<ObserverData>(handle)
            .map(|d| d.id)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

fn entry_of(rt: &Runtime, this: Platform) -> b::Result<&EntryData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<EntryData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

fn size_of_object(rt: &Runtime, this: Platform) -> b::Result<&SizeData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<SizeData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

impl g::resize_observer::ResizeObserverImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, callback: ResizeObserverCallback) -> b::Result<Gc<Object>> {
        let ro = &mut state_mut(cx.rt)?.ro;
        ro.next_id += 1;
        let id = ro.next_id;
        let data = cx.rt.alloc_host_data(ObserverData {
            id,
            callback: callback.0,
        })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn observe(
        cx: &mut Cx<'_>,
        this: Platform,
        target: NodeId,
        options: ResizeObserverOptions,
    ) -> b::Result<()> {
        let id = observer_id(cx.rt, this)?;
        let kind = match options.box_ {
            ResizeObserverBoxOptions::ContentBox => BoxKind::Content,
            ResizeObserverBoxOptions::BorderBox => BoxKind::Border,
            ResizeObserverBoxOptions::DevicePixelContentBox => BoxKind::DevicePixel,
        };
        let ro = &mut state_mut(cx.rt)?.ro;
        if ro.targets >= MAX_TARGETS {
            log::warn!("a document observes more than {MAX_TARGETS} targets with resize observers");
            return Err(cx.rt.terminate("too many resize observer targets").into());
        }
        let observer = ro.observers.entry(id).or_insert_with(|| Observer {
            object: this.object,
            targets: BTreeMap::new(),
            index: HashMap::new(),
            next_seq: 0,
        });
        // A second observation of a target replaces the first.
        let replaced = observer.add(Target {
            node: target,
            kind,
            last: None,
        });
        if !replaced {
            ro.targets += 1;
        }
        ro.requested = true;
        Ok(())
    }

    fn unobserve(cx: &mut Cx<'_>, this: Platform, target: NodeId) -> b::Result<()> {
        let id = observer_id(cx.rt, this)?;
        let ro = &mut state_mut(cx.rt)?.ro;
        let Some(observer) = ro.observers.get_mut(&id) else {
            return Ok(());
        };
        let removed = observer.remove(target);
        if observer.targets.is_empty() {
            ro.observers.remove(&id);
        }
        if removed {
            ro.targets -= 1;
        }
        Ok(())
    }

    fn disconnect(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = observer_id(cx.rt, this)?;
        let ro = &mut state_mut(cx.rt)?.ro;
        if let Some(observer) = ro.observers.remove(&id) {
            ro.targets -= observer.targets.len();
        }
        Ok(())
    }
}

/// A new `ResizeObserverSize` array of one item.
fn size_array(rt: &mut Runtime, realm: u32, size: (f64, f64)) -> VmResult<Vec<Gc<Object>>> {
    let data = rt.alloc_host_data(SizeData {
        inline: size.0,
        block: size.1,
    })?;
    let object = rt.new_host_object(realm, g::RESIZE_OBSERVER_SIZE, data)?;
    Ok(vec![object])
}

impl g::resize_observer_entry::ResizeObserverEntryImpl for WebState {
    fn target(cx: &mut Cx<'_>, this: Platform) -> b::Result<NodeId> {
        Ok(entry_of(cx.rt, this)?.target)
    }

    fn content_rect(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let data = entry_of(cx.rt, this)?;
        if let Some(rect) = data.content_rect {
            return Ok(rect);
        }
        let sizes = data.sizes;
        let rect = new_rect_read_only(
            cx.rt,
            cx.realm(),
            DomRect {
                x: sizes.origin.0,
                y: sizes.origin.1,
                width: sizes.content.0,
                height: sizes.content.1,
            },
        )?;
        if let HostData::Generic(handle) = this.data
            && let Ok(data) = cx.rt.heap_mut().generic_mut::<EntryData>(handle)
        {
            data.content_rect = Some(rect);
        }
        Ok(rect)
    }

    fn border_box_size(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let size = entry_of(cx.rt, this)?.sizes.border;
        Ok(size_array(cx.rt, cx.realm(), size)?)
    }

    fn content_box_size(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let size = entry_of(cx.rt, this)?.sizes.content;
        Ok(size_array(cx.rt, cx.realm(), size)?)
    }

    fn device_pixel_content_box_size(
        cx: &mut Cx<'_>,
        this: Platform,
    ) -> b::Result<Vec<Gc<Object>>> {
        let size = entry_of(cx.rt, this)?.sizes.device;
        Ok(size_array(cx.rt, cx.realm(), size)?)
    }
}

impl g::resize_observer_size::ResizeObserverSizeImpl for WebState {
    fn inline_size(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(size_of_object(cx.rt, this)?.inline)
    }

    fn block_size(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(size_of_object(cx.rt, this)?.block)
    }
}

// ----- The loop of the rendering update -----

/// The sizes of `node` now (after a layout flush).
fn sizes_of(rt: &mut Runtime, node: NodeId, scale: f64) -> b::Result<Sizes> {
    let found = engine_mut(rt)?.observed_box(node);
    rt.check_time()?;
    let sizes = match found {
        Some(layout) if !layout.inline => Sizes {
            border: layout.border_size,
            content: layout.content_size,
            device: (
                (layout.content_size.0 * scale).round(),
                (layout.content_size.1 * scale).round(),
            ),
            origin: layout.padding_origin,
        },
        Some(_) => Sizes::default(),
        None => {
            // An SVG element has no CSS box: its bounding box.
            let svg = dom(rt)?
                .element(node)
                .is_some_and(|e| e.name.ns == swb_dom::ns!(svg));
            match svg
                .then(|| engine_mut(rt).ok()?.svg_geometry(node))
                .flatten()
            {
                Some((rect, _)) => Sizes {
                    border: (rect.width, rect.height),
                    content: (rect.width, rect.height),
                    device: ((rect.width * scale).round(), (rect.height * scale).round()),
                    origin: (0.0, 0.0),
                },
                None => Sizes::default(),
            }
        }
    };
    Ok(sizes)
}

/// The depth of a node: the number of its ancestors.
fn depth_of(rt: &Runtime, node: NodeId) -> i32 {
    dom(rt).map_or(0, |d| {
        i32::try_from(d.ancestors(node).count()).unwrap_or(i32::MAX)
    })
}

/// What "gather active resize observations at depth" found.
#[derive(Debug, Default)]
pub(crate) struct Gathered {
    /// The observers with active observations: their numbers and the
    /// numbers of the active observations.
    active: BTreeMap<u32, Vec<u64>>,
    /// True if an active observation was not deep enough.
    skipped: bool,
}

impl Gathered {
    /// True if some observation is active.
    pub(crate) fn has_active(&self) -> bool {
        !self.active.is_empty()
    }

    /// True if some observation stayed skipped.
    pub(crate) fn has_skipped(&self) -> bool {
        self.skipped
    }

    /// The numbers of the observers that have active observations.
    pub(crate) fn observers(&self) -> Vec<u32> {
        self.active.keys().copied().collect()
    }

    fn targets_of(&self, id: u32) -> &[u64] {
        self.active.get(&id).map_or(&[], Vec::as_slice)
    }
}

/// True if a resize observer has targets.
pub(crate) fn is_observing(rt: &Runtime) -> bool {
    state(rt).is_ok_and(|s| s.ro.is_observing())
}

/// Asks for another run of the loop in the next frame (observations were
/// skipped).
pub(crate) fn request(rt: &mut Runtime) {
    if let Ok(s) = state_mut(rt) {
        s.ro.requested = true;
    }
}

/// Notes that the loop runs now (the observations added so far count).
pub(crate) fn clear_request(rt: &mut Runtime) {
    if let Ok(s) = state_mut(rt) {
        s.ro.requested = false;
    }
}

/// HTML "gather active resize observations at depth": the active
/// observations of the targets deeper than `limit`; `skipped` is set for
/// the active ones that are not.
pub(crate) fn gather(rt: &mut Runtime, limit: i32) -> b::Result<Gathered> {
    let scale = engine_mut(rt)?.viewport().scale;
    let ids: Vec<u32> = state(rt)?.ro.observers.keys().copied().collect();
    let mut gathered = Gathered::default();
    for id in ids {
        let targets: Vec<(u64, Target)> = match state(rt)?.ro.observers.get(&id) {
            Some(observer) => observer.targets.iter().map(|(&s, &t)| (s, t)).collect(),
            None => continue,
        };
        let mut active = Vec::new();
        for (seq, target) in targets {
            let sizes = sizes_of(rt, target.node, scale)?;
            if target.last == Some(sizes.of(target.kind)) {
                continue;
            }
            if depth_of(rt, target.node) > limit {
                active.push(seq);
            } else {
                gathered.skipped = true;
            }
        }
        if !active.is_empty() {
            gathered.active.insert(id, active);
        }
    }
    Ok(gathered)
}

/// HTML "broadcast active resize observations" for one observer: makes the
/// entries, records the reported sizes, calls the callback. Returns the
/// depth of the shallowest target that was delivered.
pub(crate) fn broadcast(
    rt: &mut Runtime,
    gathered: &Gathered,
    id: u32,
) -> Result<i32, ScriptError> {
    let task = crate::runtime::task_error;
    let scale = engine_mut(rt).map_err(task)?.viewport().scale;
    let seqs = gathered.targets_of(id);
    let (object, targets) = match state(rt).map_err(task)?.ro.observers.get(&id) {
        Some(observer) => {
            let found: Vec<(u64, Target)> = seqs
                .iter()
                .filter_map(|seq| observer.targets.get(seq).map(|t| (*seq, *t)))
                .collect();
            (observer.object, found)
        }
        None => return Ok(i32::MAX),
    };
    let Some(callback) = callback_of(rt, object) else {
        return Ok(i32::MAX);
    };
    let mut shallowest = i32::MAX;
    let mut entries = Vec::new();
    let mut reported = Vec::new();
    for (seq, target) in targets {
        let sizes = sizes_of(rt, target.node, scale).map_err(|e| task(exception_error(e)))?;
        shallowest = shallowest.min(depth_of(rt, target.node));
        let data = rt
            .alloc_host_data(EntryData {
                target: target.node,
                sizes,
                content_rect: None,
            })
            .map_err(task)?;
        let entry = rt
            .new_host_object(0, g::RESIZE_OBSERVER_ENTRY, data)
            .map_err(task)?;
        b::keep(rt, Value::Object(entry));
        entries.push(entry);
        reported.push((seq, target.node, sizes.of(target.kind)));
    }
    if let Some(observer) = state_mut(rt).map_err(task)?.ro.observers.get_mut(&id) {
        for (seq, node, size) in reported {
            if let Some(target) = observer.targets.get_mut(&seq)
                && target.node == node
            {
                target.last = Some(size);
            }
        }
    }
    let list =
        b::to_js::sequence(rt, 0, &entries, |_, item| Ok(Value::Object(*item))).map_err(task)?;
    b::keep(rt, list);
    b::keep(rt, Value::Object(callback));
    b::keep(rt, Value::Object(object));
    let result = rt.call_function(
        Value::Object(callback),
        Value::Object(object),
        &[list, Value::Object(object)],
    );
    match result {
        Ok(_) => {}
        Err(error @ (ScriptError::Terminated(_) | ScriptError::Internal(_))) => return Err(error),
        Err(error) => {
            let site = crate::errors::Site::named("a ResizeObserver callback");
            crate::errors::report(rt, &site, &error).map_err(task)?;
        }
    }
    Ok(shallowest)
}

/// A binding exception as the error of a task.
fn exception_error(exception: Exception) -> VmError {
    match exception {
        Exception::Js(error) => error,
        other => VmError::type_error(format!("{other:?}")),
    }
}

/// The callback of an observer object.
fn callback_of(rt: &Runtime, observer: Gc<Object>) -> Option<Gc<Object>> {
    let (_, data) = rt.host_object(Value::Object(observer))?;
    let HostData::Generic(handle) = data else {
        return None;
    };
    rt.heap()
        .generic::<ObserverData>(handle)
        .ok()
        .map(|d| d.callback)
}
