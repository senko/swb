//! The state of the `video` and `audio` elements (M14 feature 6a, ADR 0047
//! section 10; HTML "media elements",
//! <https://html.spec.whatwg.org/multipage/media.html>): one [`Element`]
//! per media element that has started to load, in [`Media`] (a field of the
//! document's state).
//!
//! **The algorithms.** `select.rs` has the choice of a resource
//! ("resource selection algorithm": the `src` attribute, else the `source`
//! children whose `type` and `media` fit), `load.rs` the "media element
//! load algorithm" (`abort`, `emptied`, the reset of the state) and the
//! failures of a load, `step.rs` what the page's pump does with a fetch:
//! the notes of its pipeline become state changes and events.
//!
//! **Lockstep.** The state that scripts read changes only in `step`, one
//! unit for each element at a time: a unit changes the state and queues its
//! events, and the page delivers the events (each a task) before the next
//! unit. So the values that a listener reads (`readyState`,
//! `networkState`) are the same on every run, whatever the speed of the
//! threads. The reads of scripts first process the DOM changes
//! ([`crate::DocumentState`]'s `media_info`), so `v.src = x` shows in
//! `networkState` (`NETWORK_NO_SOURCE`) at once; the part of the resource
//! selection that HTML runs "in a stable state" waits for the next pump.
//!
//! **Events** are queued as `(node, type)` and delivered in order with
//! `swb_web::ScriptRuntime::media_events`. They follow Chromium's order
//! (probe `media-states.json`): `loadstart`, `progress`, `suspend`,
//! `durationchange`, `resize`, `loadedmetadata`, then `loadeddata`,
//! `canplay`, `canplaythrough` together when the data is enough (Chromium
//! goes from `HAVE_METADATA` to `HAVE_ENOUGH_DATA` in one step), and
//! `error` (at the element, or at a `source`). `HAVE_CURRENT_DATA` and
//! `HAVE_FUTURE_DATA` are not visible: no probe row shows them.
//!
//! **Fetches.** A load that starts (not under `preload=none`, until
//! `load()` or `play()`) opens a pipeline through [`Fetch`]
//! (`backend.rs`); without the Cargo feature `media` it fails with code 4
//! and no request. A pipeline that has been idle for 15 s with all its
//! data suspends (ADR 0047 section 6); an element that leaves the document
//! with all its data suspends at once (one that still loads goes on).
//!
//! **Not here** (feature 6b): `play()`, `pause()`, seeks, `ended`,
//! `loop`, `volume`, `muted`, `played`, autoplay, `new Audio()`.

mod backend;
mod load;
mod select;
mod step;

use std::collections::{BTreeMap, VecDeque};

use swb_dom::NodeId;
use swb_net::Url;
use swb_web::{MediaErrorInfo, MediaInfo};

pub(crate) use backend::MediaContext;
use backend::{Fetch, Preload};
pub(crate) use select::Cx;

/// `networkState` values (HTML "network states").
pub(crate) const NETWORK_EMPTY: u16 = 0;
pub(crate) const NETWORK_IDLE: u16 = 1;
pub(crate) const NETWORK_LOADING: u16 = 2;
pub(crate) const NETWORK_NO_SOURCE: u16 = 3;

/// `readyState` values (HTML "ready states").
pub(crate) const HAVE_NOTHING: u16 = 0;
pub(crate) const HAVE_METADATA: u16 = 1;
pub(crate) const HAVE_ENOUGH_DATA: u16 = 4;

/// An idle pipeline suspends after this many ms of page time (ADR 0047
/// section 6: swb's choice).
pub(crate) const SUSPEND_AFTER_MS: f64 = 15_000.0;

/// The most media elements that have a state. At the limit, the states of
/// elements outside the document go first; with none to drop, a new element
/// does not load (one warning).
pub(crate) const MAX_ELEMENTS: usize = 16_384;

/// The most events that one delivery hands over.
pub(crate) const MAX_DELIVERY: usize = 4096;

/// The `preload` attribute of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreloadAttr {
    None,
    Metadata,
    Auto,
}

impl PreloadAttr {
    /// How much a fetch reads ahead (`none` only after a load that was
    /// asked for).
    pub(crate) fn read_ahead(self) -> Preload {
        match self {
            PreloadAttr::Auto => Preload::Auto,
            PreloadAttr::None | PreloadAttr::Metadata => Preload::Metadata,
        }
    }
}

/// Where an element is in the load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Nothing runs: `NETWORK_EMPTY`.
    Empty,
    /// The load algorithm ran; the part of the resource selection that HTML
    /// runs in a stable state waits for the next pump.
    Selecting,
    /// A resource was chosen and `preload=none` defers its fetch.
    Deferred,
    /// A fetch runs, or ended with the data that it has.
    Fetching,
    /// The `source` children are used up: waits for a new one.
    Waiting,
    /// The `src` attribute failed.
    Failed,
}

/// What an element knows of its load.
pub(crate) struct Element {
    /// A `video` element (else `audio`).
    pub(crate) video: bool,
    pub(crate) phase: Phase,
    pub(crate) network_state: u16,
    pub(crate) ready_state: u16,
    /// `currentSrc`.
    pub(crate) current_src: String,
    pub(crate) error: Option<MediaErrorInfo>,
    pub(crate) meta: Option<backend::Meta>,
    /// The load was asked for by `load()` (or a later `play()`): it does not
    /// wait for the user under `preload=none`.
    pub(crate) forced: bool,
    /// Children mode: the pointer of the selection in the children (after
    /// the last `source` that it took); `None` before children mode.
    pub(crate) pointer: Option<select::Pointer>,
    /// The `source` whose resource is fetched (`None` in attribute mode).
    pub(crate) candidate: Option<NodeId>,
    /// The resource that `preload=none` keeps back.
    pub(crate) deferred: Option<Url>,
    pub(crate) fetch: Option<Fetch>,
    /// `progress` was queued for this fetch.
    pub(crate) progress_sent: bool,
    /// `suspend` was queued for this fetch (the network is idle).
    pub(crate) suspend_sent: bool,
    /// The page time (ms) since which the element has been idle with all its
    /// data, for the suspend of its pipeline.
    pub(crate) idle_since: Option<f64>,
}

impl Element {
    fn new(video: bool) -> Element {
        Element {
            video,
            phase: Phase::Empty,
            network_state: NETWORK_EMPTY,
            ready_state: HAVE_NOTHING,
            current_src: String::new(),
            error: None,
            meta: None,
            forced: false,
            pointer: None,
            candidate: None,
            deferred: None,
            fetch: None,
            progress_sent: false,
            suspend_sent: false,
            idle_since: None,
        }
    }

    /// What the element reports to scripts.
    fn info(&self) -> MediaInfo {
        let meta = self.meta.as_ref();
        let buffered = match (&self.fetch, meta) {
            (Some(fetch), Some(m)) => fetch.buffered(m.duration),
            _ => Vec::new(),
        };
        let seekable = match meta {
            Some(m) if m.seekable => vec![(0.0, m.duration)],
            Some(_) => vec![(0.0, 0.0)],
            None => Vec::new(),
        };
        let size = meta.and_then(|m| m.size).filter(|_| self.video);
        MediaInfo {
            network_state: self.network_state,
            ready_state: self.ready_state,
            current_src: self.current_src.clone(),
            error: self.error.clone(),
            duration: meta.map_or(f64::NAN, |m| m.duration),
            current_time: match (&self.fetch, meta) {
                (Some(fetch), Some(_)) => fetch.position(),
                _ => 0.0,
            },
            paused: true,
            seeking: false,
            video_width: size.map_or(0, |s| s.0),
            video_height: size.map_or(0, |s| s.1),
            buffered,
            seekable,
        }
    }

    /// Whether virtual time must wait for this element: a selection step, a
    /// fetch that opens or has notes that the page has not handled, data
    /// that is not enough yet.
    fn pending(&self) -> bool {
        match self.phase {
            Phase::Selecting => true,
            Phase::Fetching => {
                let Some(fetch) = &self.fetch else {
                    return false;
                };
                if fetch.is_suspended() {
                    return false;
                }
                fetch.has_notes()
                    || fetch.pending()
                    || (self.meta.is_some() && self.ready_state < HAVE_ENOUGH_DATA)
                    || (self.meta.is_some() && !self.suspend_sent && !fetch.net_idle())
            }
            Phase::Empty | Phase::Deferred | Phase::Waiting | Phase::Failed => false,
        }
    }
}

/// The media elements of a document.
pub(crate) struct Media {
    /// The page's shared state (the bound on open pipelines).
    pub(super) ctx: MediaContext,
    /// By node: the iteration order, and so the order of the events of
    /// different elements, is the same on every run.
    pub(super) elements: BTreeMap<NodeId, Element>,
    /// The events that wait for the script runtime, in order.
    pub(super) events: VecDeque<(NodeId, &'static str)>,
    /// The number of the last `MediaError` that an element got.
    pub(super) error_serial: u64,
    /// The page time of the last step, in microseconds.
    pub(super) now_us: i64,
    pub(super) warned: bool,
}

impl Media {
    /// No elements.
    pub(crate) fn new(ctx: MediaContext) -> Media {
        Media {
            ctx,
            elements: BTreeMap::new(),
            events: VecDeque::new(),
            error_serial: 0,
            now_us: 0,
            warned: false,
        }
    }

    /// Forgets every element: the pipelines close (a document that left its
    /// frame).
    pub(crate) fn clear(&mut self) {
        self.elements.clear();
        self.events.clear();
    }

    /// True if no media element has a state.
    pub(crate) fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// The number of events that wait for delivery.
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    /// True if events wait for delivery.
    pub(crate) fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// Takes the next events for the script runtime (at most
    /// [`MAX_DELIVERY`]).
    pub(crate) fn take_events(&mut self) -> Vec<(NodeId, &'static str)> {
        let count = self.events.len().min(MAX_DELIVERY);
        self.events.drain(..count).collect()
    }

    /// Queues the event `name` at `node`.
    pub(super) fn queue(&mut self, node: NodeId, name: &'static str) {
        self.events.push_back((node, name));
    }

    /// What `node` reports to scripts (the default for an element that has
    /// not started to load).
    pub(crate) fn info(&self, node: NodeId) -> MediaInfo {
        self.elements
            .get(&node)
            .map_or_else(MediaInfo::default, Element::info)
    }

    /// Whether virtual time must wait for a media element.
    pub(crate) fn pending(&self) -> bool {
        self.elements.values().any(Element::pending)
    }

    /// The page time (ms) of the next suspend of an idle pipeline.
    pub(crate) fn next_due(&self) -> Option<f64> {
        self.elements
            .values()
            .filter_map(|e| e.idle_since)
            .map(|since| since + SUSPEND_AFTER_MS)
            .reduce(f64::min)
    }

    /// The number of elements with a pipeline that runs: not suspended, not
    /// failed, not waiting for a place.
    pub(crate) fn open_fetches(&self) -> usize {
        self.elements
            .values()
            .filter(|e| e.fetch.as_ref().is_some_and(Fetch::can_suspend))
            .count()
    }
}
