//! `IntersectionObserver` and `IntersectionObserverEntry` (Intersection
//! Observer, <https://w3c.github.io/IntersectionObserver/>; M8 feature 9a).
//!
//! The rendering update computes the observations after style and layout
//! ([`update`], called through
//! [`crate::ScriptRuntime::update_intersection_observers`]): for each target
//! of each observer, in the order of creation and of `observe()`, the
//! intersection of the target's bounding box with the root, clipped by the
//! ancestors in its containing block chain that clip their content. An entry
//! is queued when the threshold index or `isIntersecting` changed since the
//! last computation (the first computation always queues one). The entries
//! are delivered by a task ([`crate::event_loop::Task::IntersectionObservers`])
//! that the update queues, so the callbacks run after the animation frame
//! callbacks and the resize observers of the same frame (measured in
//! Chromium 148: `raf1`, `ro`, `io`; `timeout0` before them).
//!
//! The geometry comes from the engine through [`crate::EngineHost`]
//! (`client_rects`, `observed_box`). Measured in Chromium (`tools/probes/
//! web-intersection-observer.json`): the root of an element root is its padding
//! box; a target that is not in the containing block chain of an explicit root,
//! or that has no box, gets an entry with all rectangles 0 (also
//! `rootBounds`); an edge-adjacent target intersects with a zero-area
//! `intersectionRect`; `overflow: clip` clips like the scroll containers;
//! `position: fixed` targets escape the clips of their ancestors; a margin in
//! `px` is cut to whole pixels.
//!
//! Lifetime: an observer lives while it has targets (the state holds it), as
//! the registered observer lists of the targets do in the specification.
//!
//! Departures: `scrollMargin`, `delay` and `trackVisibility` are accepted and
//! reported but have no effect (`isVisible` is false); `clip-path` and masks
//! do not clip; the root of an element root is never narrowed by a scrollbar.

use std::collections::{BTreeMap, HashMap};

use swb_dom::NodeId;
use swb_js::{
    Gc, GenericData, HostData, Object, Runtime, ScriptError, Tracer, Value, VmError, VmResult,
};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::cssom_view::{bounding_client_rect, new_rect_read_only};
use crate::event_loop::{Task, queue_task};
use crate::generated as g;
use crate::generated::types::{
    DoubleOrDoubleSequence, ElementOrDocument, IntersectionObserverCallback,
    IntersectionObserverInit,
};
use crate::host::{DomRect, ObservedBox, ObservedPosition};
use crate::state::{WebState, engine_mut, state, state_mut};

/// The most targets that all observers of a document observe.
pub(crate) const MAX_TARGETS: usize = 1 << 20;

/// The most entries that wait in the queue of one observer.
const MAX_QUEUE: usize = 1 << 20;

/// One side of a margin.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Margin {
    value: f64,
    percent: bool,
}

/// The configuration of an observer (immutable after the constructor).
#[derive(Clone, Debug)]
struct Config {
    root: Option<NodeId>,
    root_is_document: bool,
    root_margin: [Margin; 4],
    root_margin_text: String,
    scroll_margin_text: String,
    thresholds: Vec<f64>,
    delay: i32,
    track_visibility: bool,
}

/// The host data of an `IntersectionObserver`.
struct ObserverData {
    id: u32,
    callback: Gc<Object>,
    config: Config,
}

impl GenericData for ObserverData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.callback);
    }

    fn heap_size(&self) -> usize {
        self.config.thresholds.len() * size_of::<f64>() + 128
    }
}

/// One observed target.
#[derive(Clone, Copy, Debug)]
struct Target {
    node: NodeId,
    /// The threshold index of the last computation (-1: none yet).
    previous_threshold: i32,
    previous_intersecting: bool,
}

/// One queued entry, as Rust data.
#[derive(Clone, Copy, Debug)]
struct Entry {
    time: f64,
    root_bounds: DomRect,
    bounding: DomRect,
    intersection: DomRect,
    intersecting: bool,
    ratio: f64,
    target: NodeId,
}

/// An observer that has targets.
struct Observer {
    object: Gc<Object>,
    config: Config,
    /// The targets in the order of the `observe()` calls, by number.
    targets: BTreeMap<u64, Target>,
    /// The number of each target node.
    index: HashMap<NodeId, u64>,
    next_seq: u64,
    queue: Vec<Entry>,
}

/// The intersection observers of the binding state.
#[derive(Default)]
pub(crate) struct IoState {
    next_id: u32,
    observers: BTreeMap<u32, Observer>,
    targets: usize,
    delivery_queued: bool,
    /// An observation was added since the last computation.
    requested: bool,
}

impl IoState {
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

    /// True if an observation was added since the last computation.
    pub(crate) fn requested(&self) -> bool {
        self.requested
    }
}

/// The host data of an `IntersectionObserverEntry`.
struct EntryData {
    entry: Entry,
    root_bounds: Option<Gc<Object>>,
    bounding: Option<Gc<Object>>,
    intersection: Option<Gc<Object>>,
}

impl GenericData for EntryData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for rect in [self.root_bounds, self.bounding, self.intersection]
            .into_iter()
            .flatten()
        {
            tracer.object(rect);
        }
    }

    fn heap_size(&self) -> usize {
        size_of::<Entry>()
    }
}

fn observer_data(rt: &Runtime, this: Platform) -> b::Result<&ObserverData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<ObserverData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
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

fn entry_data_mut(rt: &mut Runtime, this: Platform) -> b::Result<&mut EntryData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap_mut()
            .generic_mut::<EntryData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

// ----- Margins -----

/// Parses a margin (`rootMargin`, `scrollMargin`): one to four values in
/// `px` or `%`, expanded to top, right, bottom, left.
fn parse_margin(text: &str, name: &str) -> b::Result<[Margin; 4]> {
    let tokens: Vec<&str> = text.split_ascii_whitespace().collect();
    if tokens.len() > 4 {
        return Err(Exception::dom(
            "SyntaxError",
            format!("Extra text found at the end of {name}."),
        ));
    }
    let mut values = Vec::new();
    for token in &tokens {
        let lower = token.to_ascii_lowercase();
        let (number, percent) = if let Some(n) = lower.strip_suffix("px") {
            (n, false)
        } else if let Some(n) = lower.strip_suffix('%') {
            (n, true)
        } else {
            return Err(unit_error(name));
        };
        let digits = number.strip_prefix(['+', '-']).unwrap_or(number);
        if digits.is_empty()
            || !digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            || digits.matches('.').count() > 1
        {
            return Err(unit_error(name));
        }
        let Ok(mut value) = number.parse::<f64>() else {
            return Err(unit_error(name));
        };
        if !percent {
            // Chromium keeps whole pixels (measured: `1.5px` is `1px`).
            value = value.trunc();
        }
        values.push(Margin { value, percent });
    }
    let zero = Margin {
        value: 0.0,
        percent: false,
    };
    Ok(match values.as_slice() {
        [] => [zero; 4],
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d, ..] => [*a, *b, *c, *d],
    })
}

fn unit_error(name: &str) -> Exception {
    Exception::dom(
        "SyntaxError",
        format!("{name} must be specified in pixels or percent."),
    )
}

/// The serialization of a margin (`rootMargin`).
fn margin_text(margin: &[Margin; 4]) -> String {
    margin
        .iter()
        .map(|m| format!("{}{}", m.value, if m.percent { "%" } else { "px" }))
        .collect::<Vec<_>>()
        .join(" ")
}

// ----- The interface -----

impl g::intersection_observer::IntersectionObserverImpl for WebState {
    fn root(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<ElementOrDocument>> {
        let config = &observer_data(cx.rt, this)?.config;
        Ok(config.root.map(|node| {
            if config.root_is_document {
                ElementOrDocument::Document(node)
            } else {
                ElementOrDocument::Element(node)
            }
        }))
    }

    fn root_margin(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(observer_data(cx.rt, this)?.config.root_margin_text.clone())
    }

    fn scroll_margin(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(observer_data(cx.rt, this)?
            .config
            .scroll_margin_text
            .clone())
    }

    fn thresholds(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<f64>> {
        Ok(observer_data(cx.rt, this)?.config.thresholds.clone())
    }

    fn delay(cx: &mut Cx<'_>, this: Platform) -> b::Result<i32> {
        Ok(observer_data(cx.rt, this)?.config.delay)
    }

    fn track_visibility(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(observer_data(cx.rt, this)?.config.track_visibility)
    }

    fn constructor(
        cx: &mut Cx<'_>,
        callback: IntersectionObserverCallback,
        options: IntersectionObserverInit,
    ) -> b::Result<Gc<Object>> {
        let root_margin = parse_margin(&options.root_margin, "rootMargin")?;
        let scroll_margin = parse_margin(&options.scroll_margin, "scrollMargin")?;
        let mut thresholds = match options.threshold {
            DoubleOrDoubleSequence::Double(t) => vec![t],
            DoubleOrDoubleSequence::DoubleSequence(list) => list,
        };
        if thresholds.iter().any(|t| !(0.0..=1.0).contains(t)) {
            return Err(Exception::range_error(
                "Threshold values must be numbers between 0 and 1",
            ));
        }
        if thresholds.is_empty() {
            thresholds.push(0.0);
        }
        thresholds.sort_by(f64::total_cmp);
        if options.track_visibility && options.delay < 100 {
            return Err(Exception::dom(
                "NotSupportedError",
                "To enable the 'trackVisibility' option, you must also use a 'delay' option with a value of at least 100. Visibility is more expensive to compute than the basic intersection; enabling this option may negatively affect your page's performance. Please make sure you *really* need visibility tracking before enabling the 'trackVisibility' option.",
            ));
        }
        let (root, root_is_document) = match options.root {
            Some(ElementOrDocument::Element(node)) => (Some(node), false),
            Some(ElementOrDocument::Document(node)) => (Some(node), true),
            None => (None, false),
        };
        let config = Config {
            root,
            root_is_document,
            root_margin,
            root_margin_text: margin_text(&root_margin),
            scroll_margin_text: margin_text(&scroll_margin),
            thresholds,
            delay: options.delay,
            track_visibility: options.track_visibility,
        };
        let io = &mut state_mut(cx.rt)?.io;
        io.next_id += 1;
        let id = io.next_id;
        let data = cx.rt.alloc_host_data(ObserverData {
            id,
            callback: callback.0,
            config,
        })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn observe(cx: &mut Cx<'_>, this: Platform, target: NodeId) -> b::Result<()> {
        let id = observer_data(cx.rt, this)?.id;
        let new_config = if state(cx.rt)?.io.observers.contains_key(&id) {
            None
        } else {
            Some(observer_data(cx.rt, this)?.config.clone())
        };
        let io = &mut state_mut(cx.rt)?.io;
        if io.targets >= MAX_TARGETS {
            log::warn!(
                "a document observes more than {MAX_TARGETS} targets with intersection observers"
            );
            return Err(cx
                .rt
                .terminate("too many intersection observer targets")
                .into());
        }
        if let Some(config) = new_config {
            io.observers.insert(
                id,
                Observer {
                    object: this.object,
                    config,
                    targets: BTreeMap::new(),
                    index: HashMap::new(),
                    next_seq: 0,
                    queue: Vec::new(),
                },
            );
        }
        let Some(observer) = io.observers.get_mut(&id) else {
            return Ok(());
        };
        if observer.index.contains_key(&target) {
            return Ok(());
        }
        observer.next_seq += 1;
        observer.index.insert(target, observer.next_seq);
        observer.targets.insert(
            observer.next_seq,
            Target {
                node: target,
                previous_threshold: -1,
                previous_intersecting: false,
            },
        );
        io.targets += 1;
        io.requested = true;
        Ok(())
    }

    fn unobserve(cx: &mut Cx<'_>, this: Platform, target: NodeId) -> b::Result<()> {
        let id = observer_data(cx.rt, this)?.id;
        let io = &mut state_mut(cx.rt)?.io;
        let Some(observer) = io.observers.get_mut(&id) else {
            return Ok(());
        };
        let removed = observer
            .index
            .remove(&target)
            .is_some_and(|seq| observer.targets.remove(&seq).is_some());
        if observer.targets.is_empty() {
            io.observers.remove(&id);
        }
        if removed {
            io.targets -= 1;
        }
        Ok(())
    }

    fn disconnect(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = observer_data(cx.rt, this)?.id;
        let io = &mut state_mut(cx.rt)?.io;
        if let Some(observer) = io.observers.remove(&id) {
            io.targets -= observer.targets.len();
        }
        Ok(())
    }

    fn take_records(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let id = observer_data(cx.rt, this)?.id;
        let entries = match state_mut(cx.rt)?.io.observers.get_mut(&id) {
            Some(observer) => std::mem::take(&mut observer.queue),
            None => Vec::new(),
        };
        entries_to_js(cx.rt, &entries).map_err(Exception::from)
    }
}

impl g::intersection_observer_entry::IntersectionObserverEntryImpl for WebState {
    fn time(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(entry_data(cx.rt, this)?.entry.time)
    }

    fn root_bounds(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let data = entry_data(cx.rt, this)?;
        if let Some(rect) = data.root_bounds {
            return Ok(Some(rect));
        }
        let value = data.entry.root_bounds;
        let rect = new_rect_read_only(cx.rt, cx.realm(), value)?;
        entry_data_mut(cx.rt, this)?.root_bounds = Some(rect);
        Ok(Some(rect))
    }

    fn bounding_client_rect(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let data = entry_data(cx.rt, this)?;
        if let Some(rect) = data.bounding {
            return Ok(rect);
        }
        let value = data.entry.bounding;
        let rect = new_rect_read_only(cx.rt, cx.realm(), value)?;
        entry_data_mut(cx.rt, this)?.bounding = Some(rect);
        Ok(rect)
    }

    fn intersection_rect(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let data = entry_data(cx.rt, this)?;
        if let Some(rect) = data.intersection {
            return Ok(rect);
        }
        let value = data.entry.intersection;
        let rect = new_rect_read_only(cx.rt, cx.realm(), value)?;
        entry_data_mut(cx.rt, this)?.intersection = Some(rect);
        Ok(rect)
    }

    fn is_intersecting(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(entry_data(cx.rt, this)?.entry.intersecting)
    }

    fn intersection_ratio(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(entry_data(cx.rt, this)?.entry.ratio)
    }

    fn target(cx: &mut Cx<'_>, this: Platform) -> b::Result<NodeId> {
        Ok(entry_data(cx.rt, this)?.entry.target)
    }

    fn is_visible(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }
}

/// The `IntersectionObserverEntry` objects of `entries`.
fn entries_to_js(rt: &mut Runtime, entries: &[Entry]) -> VmResult<Vec<Gc<Object>>> {
    let mut objects = Vec::with_capacity(entries.len());
    for &entry in entries {
        rt.check_time()?;
        let data = rt.alloc_host_data(EntryData {
            entry,
            root_bounds: None,
            bounding: None,
            intersection: None,
        })?;
        let object = rt.new_host_object(0, g::INTERSECTION_OBSERVER_ENTRY, data)?;
        b::keep(rt, Value::Object(object));
        objects.push(object);
    }
    Ok(objects)
}

// ----- Geometry -----

/// What the update keeps of an element in the containing block chain.
#[derive(Clone, Copy)]
struct Ancestor {
    layout: ObservedBox,
    /// The padding box in viewport coordinates.
    padding_box: DomRect,
}

/// The state of one update: caches of the geometry of the ancestors.
struct Update {
    time: f64,
    viewport: DomRect,
    ancestors: HashMap<NodeId, Option<Ancestor>>,
}

fn intersect(a: DomRect, b: DomRect) -> Option<DomRect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    (x1 >= x0 && y1 >= y0).then_some(DomRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// `rect` grown by the margin; percentages are of the size of `rect`.
fn expand(rect: DomRect, margin: &[Margin; 4]) -> DomRect {
    let amount = |m: &Margin, basis: f64| {
        if m.percent {
            // Chromium cuts a percentage to whole pixels (measured).
            (basis * m.value / 100.0).trunc()
        } else {
            m.value
        }
    };
    let top = amount(&margin[0], rect.height);
    let right = amount(&margin[1], rect.width);
    let bottom = amount(&margin[2], rect.height);
    let left = amount(&margin[3], rect.width);
    DomRect {
        x: rect.x - left,
        y: rect.y - top,
        width: (rect.width + left + right).max(0.0),
        height: (rect.height + top + bottom).max(0.0),
    }
}

impl Update {
    /// The geometry of `node` as an ancestor, cached for the update.
    fn ancestor(&mut self, rt: &mut Runtime, node: NodeId) -> b::Result<Option<Ancestor>> {
        if let Some(found) = self.ancestors.get(&node) {
            return Ok(*found);
        }
        let layout = engine_mut(rt)?.observed_box(node);
        let found = match layout {
            Some(layout) => {
                let rect = bounding_client_rect(rt, node)?;
                let [top, right, bottom, left] = layout.border_widths;
                Some(Ancestor {
                    layout,
                    padding_box: DomRect {
                        x: rect.x + left,
                        y: rect.y + top,
                        width: (rect.width - left - right).max(0.0),
                        height: (rect.height - top - bottom).max(0.0),
                    },
                })
            }
            None => None,
        };
        self.ancestors.insert(node, found);
        Ok(found)
    }

    /// The computation for one target: the entry, without the thresholds.
    fn compute(&mut self, rt: &mut Runtime, config: &Config, target: NodeId) -> b::Result<Entry> {
        let zero = DomRect::default();
        let time = self.time;
        let not_intersecting = |bounding: DomRect| Entry {
            time,
            root_bounds: zero,
            bounding,
            intersection: zero,
            intersecting: false,
            ratio: 0.0,
            target,
        };
        let rects = engine_mut(rt)?.client_rects(target);
        rt.check_time()?;
        if rects.is_empty() {
            return Ok(not_intersecting(zero));
        }
        let bounding = bounding_client_rect(rt, target)?;
        // The containing block chain from the target up to the root.
        let own = engine_mut(rt)?.observed_box(target);
        let mut mode = own.map_or(ObservedPosition::Static, |b| b.position);
        let document_element = crate::state::dom(rt)?.document_element();
        let body = crate::state::dom(rt)?.body();
        let mut clip: Option<DomRect> = Some(bounding);
        let mut root_rect = None;
        let mut node = crate::state::dom(rt)?.parent_element(target);
        while let Some(current) = node {
            if config.root == Some(current) && !config.root_is_document {
                let Some(ancestor) = self.ancestor(rt, current)? else {
                    break;
                };
                if chain_contains(mode, &ancestor.layout) {
                    root_rect = Some(ancestor.padding_box);
                }
                break;
            }
            if Some(current) == document_element || Some(current) == body {
                node = crate::state::dom(rt)?.parent_element(current);
                continue;
            }
            if let Some(ancestor) = self.ancestor(rt, current)?
                && chain_contains(mode, &ancestor.layout)
            {
                if ancestor.layout.clips {
                    clip = clip.and_then(|c| intersect(c, ancestor.padding_box));
                }
                mode = ancestor.layout.position;
            }
            node = crate::state::dom(rt)?.parent_element(current);
        }
        let root_rect = match config.root {
            Some(_) if !config.root_is_document => match root_rect {
                Some(rect) => rect,
                // The target is not in the containing block chain of the
                // root: nothing intersects (measured).
                None => return Ok(not_intersecting(zero)),
            },
            _ => self.viewport,
        };
        let root_bounds = expand(root_rect, &config.root_margin);
        let intersection = clip.and_then(|c| intersect(c, root_bounds));
        let target_area = bounding.width * bounding.height;
        let (intersecting, rect, ratio) = match intersection {
            Some(rect) => {
                let ratio = if target_area > 0.0 {
                    (rect.width * rect.height / target_area).min(1.0)
                } else {
                    1.0
                };
                (true, rect, ratio)
            }
            None => (false, zero, 0.0),
        };
        Ok(Entry {
            time,
            root_bounds,
            bounding,
            intersection: rect,
            intersecting,
            ratio,
            target,
        })
    }
}

/// True if an element with `layout` is in the containing block chain of a
/// box with the `position` `mode`: every ancestor is one of a box that is
/// static or relative; an absolutely positioned box has the nearest
/// positioned or transformed ancestor; a fixed one only the transformed.
fn chain_contains(mode: ObservedPosition, layout: &ObservedBox) -> bool {
    match mode {
        ObservedPosition::Fixed => layout.transformed,
        ObservedPosition::Absolute => {
            layout.position != ObservedPosition::Static || layout.transformed
        }
        _ => true,
    }
}

/// The index of the first threshold above `ratio`, or the count (the
/// specification's "thresholdIndex" for an intersecting target).
fn threshold_index(thresholds: &[f64], ratio: f64) -> i32 {
    let index = thresholds
        .iter()
        .position(|&t| t > ratio)
        .unwrap_or(thresholds.len());
    i32::try_from(index).unwrap_or(i32::MAX)
}

/// The "run the update intersection observations steps" of the rendering
/// update at the page time `frame_time` (the clock value `time`): queues an
/// entry where the threshold index or `isIntersecting` changed, and the
/// delivery task. The caller runs it in a task (a deadline, a handle scope).
pub(crate) fn update(rt: &mut Runtime, time: f64) -> Result<(), ScriptError> {
    let ids: Vec<u32> = {
        let io = &mut state_mut(rt).map_err(crate::runtime::task_error)?.io;
        io.requested = false;
        io.observers.keys().copied().collect()
    };
    if ids.is_empty() {
        return Ok(());
    }
    let viewport = match engine_mut(rt) {
        Ok(host) => {
            let v = host.viewport();
            DomRect {
                x: 0.0,
                y: 0.0,
                width: v.width,
                height: v.height,
            }
        }
        Err(_) => return Ok(()),
    };
    let mut update = Update {
        time,
        viewport,
        ancestors: HashMap::new(),
    };
    let mut queued = false;
    for id in ids {
        let Some((config, mut targets)) = state(rt)
            .ok()
            .and_then(|s| s.io.observers.get(&id))
            .map(|o| (o.config.clone(), o.targets.clone()))
        else {
            continue;
        };
        let mut entries = Vec::new();
        for target in targets.values_mut() {
            rt.check_time().map_err(crate::runtime::task_error)?;
            let entry = update
                .compute(rt, &config, target.node)
                .map_err(|e| crate::runtime::task_error(exception_error(e)))?;
            let index = if entry.intersecting {
                threshold_index(&config.thresholds, entry.ratio)
            } else {
                0
            };
            if index != target.previous_threshold
                || entry.intersecting != target.previous_intersecting
            {
                target.previous_threshold = index;
                target.previous_intersecting = entry.intersecting;
                entries.push(entry);
            }
        }
        // The observer may have been changed by nothing: no script ran.
        if let Some(observer) = state_mut(rt).ok().and_then(|s| s.io.observers.get_mut(&id)) {
            observer.targets = targets;
            let room = MAX_QUEUE.saturating_sub(observer.queue.len());
            if entries.len() > room {
                log::warn!("an IntersectionObserver queue is full; entries are dropped");
                entries.truncate(room);
            }
            queued |= !entries.is_empty();
            observer.queue.extend(entries);
        }
    }
    if queued {
        queue_delivery(rt).map_err(crate::runtime::task_error)?;
    }
    Ok(())
}

/// A binding exception as the error of a task.
fn exception_error(exception: Exception) -> VmError {
    match exception {
        Exception::Js(error) => error,
        other => VmError::type_error(format!("{other:?}")),
    }
}

/// Queues the delivery task unless one waits.
fn queue_delivery(rt: &mut Runtime) -> VmResult<()> {
    let io = &mut state_mut(rt)?.io;
    if io.delivery_queued {
        return Ok(());
    }
    io.delivery_queued = true;
    queue_task(rt, Task::IntersectionObservers)
}

/// The delivery task (HTML "queue an intersection observer task"): each
/// observer with entries gets them, in the order of creation, with a
/// microtask checkpoint after each callback.
pub(crate) fn deliver(rt: &mut Runtime) -> Result<(), ScriptError> {
    let ids: Vec<u32> = {
        let io = &mut state_mut(rt).map_err(crate::runtime::task_error)?.io;
        io.delivery_queued = false;
        io.observers.keys().copied().collect()
    };
    for id in ids {
        let Some((object, entries)) = state_mut(rt)
            .ok()
            .and_then(|s| s.io.observers.get_mut(&id))
            .filter(|o| !o.queue.is_empty())
            .map(|o| (o.object, std::mem::take(&mut o.queue)))
        else {
            continue;
        };
        let Some(callback) = callback_of(rt, object) else {
            continue;
        };
        let objects = entries_to_js(rt, &entries).map_err(crate::runtime::task_error)?;
        let list = b::to_js::sequence(rt, 0, &objects, |_, item| Ok(Value::Object(*item)))
            .map_err(crate::runtime::task_error)?;
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
            Err(error @ (ScriptError::Terminated(_) | ScriptError::Internal(_))) => {
                return Err(error);
            }
            Err(error) => {
                let site = crate::errors::Site::named("an IntersectionObserver callback");
                crate::errors::report(rt, &site, &error).map_err(crate::runtime::task_error)?;
            }
        }
        crate::event_loop::microtask_checkpoint(rt)?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_expand_and_serialize() {
        let m = parse_margin("1px 2px 3px", "rootMargin").unwrap_or(
            [Margin {
                value: 0.0,
                percent: false,
            }; 4],
        );
        assert_eq!(margin_text(&m), "1px 2px 3px 2px");
        let m = parse_margin(" 5px  -6% ", "rootMargin").unwrap_or(
            [Margin {
                value: 0.0,
                percent: false,
            }; 4],
        );
        assert_eq!(margin_text(&m), "5px -6% 5px -6%");
        assert_eq!(
            margin_text(&parse_margin("1.5px", "rootMargin").unwrap_or(
                [Margin {
                    value: 9.0,
                    percent: false
                }; 4]
            )),
            "1px 1px 1px 1px"
        );
        assert!(parse_margin("10", "rootMargin").is_err());
        assert!(parse_margin("10em", "rootMargin").is_err());
        assert!(parse_margin("1px 2px 3px 4px 5px", "rootMargin").is_err());
        assert!(parse_margin("px", "rootMargin").is_err());
    }

    #[test]
    fn intersections_include_edges() {
        let a = DomRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let touching = DomRect {
            x: 0.0,
            y: 10.0,
            width: 10.0,
            height: 10.0,
        };
        let apart = DomRect {
            x: 0.0,
            y: 11.0,
            width: 10.0,
            height: 10.0,
        };
        assert_eq!(intersect(a, touching).map(|r| r.height), Some(0.0));
        assert!(intersect(a, apart).is_none());
    }

    #[test]
    fn the_threshold_index_counts_thresholds_up_to_the_ratio() {
        let t = [0.0, 0.5, 1.0];
        assert_eq!(threshold_index(&t, 0.0), 1);
        assert_eq!(threshold_index(&t, 0.5), 2);
        assert_eq!(threshold_index(&t, 0.75), 2);
        assert_eq!(threshold_index(&t, 1.0), 3);
    }
}
