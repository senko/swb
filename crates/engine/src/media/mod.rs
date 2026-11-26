//! The state of the `video` and `audio` elements (M14 features 6a and 6b,
//! ADR 0047 section 10; HTML "media elements",
//! <https://html.spec.whatwg.org/multipage/media.html>): one [`Element`]
//! per media element that has started to load or that a script used, in
//! [`Media`] (a field of the document's state).
//!
//! **The algorithms.** `select.rs` has the choice of a resource
//! ("resource selection algorithm": the `src` attribute, else the `source`
//! children whose `type` and `media` fit), `load.rs` the "media element
//! load algorithm" (`abort`, `emptied`, the reset of the state) and the
//! failures of a load, `step.rs` what the page's pump does with a fetch:
//! the notes of its pipeline become state changes and events,
//! `playback.rs` the playback API (`play()`, `pause()`, seeks, `volume`,
//! `muted`, autoplay, the end, the late failure).
//!
//! **Lockstep.** The state that scripts read changes only in `step` (and in
//! the calls of scripts), one unit for each element at a time: a unit changes
//! the state and queues its events, and the page delivers the events (each a
//! task) before the next unit. So the values that a listener reads
//! (`readyState`, `networkState`) are the same on every run, whatever the
//! speed of the threads. The reads of scripts first process the DOM changes
//! ([`crate::DocumentState`]'s `media_info`), so `v.src = x` shows in
//! `networkState` (`NETWORK_NO_SOURCE`) at once.
//!
//! **The stable state.** The part of the resource selection that HTML runs
//! "in a stable state" is a marker in the queue of tasks ([`Entry::Select`]):
//! the events that were queued before it are delivered in the state that
//! the synchronous part left, then `step` handles the marker (the step runs),
//! and the events that scripts queued after the marker (`play`,
//! `volumechange`) see the state that it made (`NETWORK_LOADING`), as in
//! Chromium. The step queues `loadstart` at the end.
//!
//! **Tasks** are queued as [`Entry::Task`] (a [`MediaTask`]: an event at a
//! node, or the settling of the `play()` promises of an element) and
//! delivered in order with `swb_web::ScriptRuntime::media_events`. They
//! follow Chromium's order (probes `media-states.json` and
//! `media-playback.json`): `loadstart`, `progress`, `suspend`,
//! `durationchange`, `resize`, `loadedmetadata`, then `loadeddata`,
//! `canplay`, `canplaythrough` together when the data is enough (Chromium
//! goes from `HAVE_METADATA` to `HAVE_ENOUGH_DATA` in one step), and
//! `error` (at the element, or at a `source`). `HAVE_CURRENT_DATA` and
//! `HAVE_FUTURE_DATA` show only after an underrun in real time. A load
//! drops the tasks that are queued for its element (HTML "media element
//! load algorithm", step 6): each entry carries the epoch of its element,
//! which a load increases, and an entry of an older epoch is skipped at
//! delivery.
//!
//! **Fetches.** A load that starts (not under `preload=none`, until
//! `load()`, `play()` or an `autoplay` that is allowed) opens a pipeline
//! through [`Fetch`] (`backend.rs`); without the Cargo feature `media` it
//! fails with code 4 and no request. A pipeline that has been idle for 15 s
//! with all its data suspends (ADR 0047 section 6); an element that leaves
//! the document pauses (HTML) and suspends at once if it has all its data
//! (one that still loads goes on). `play()` and a seek open a suspended
//! pipeline again.

mod backend;
mod frames;
mod load;
mod playback;
mod select;
mod step;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use swb_dom::NodeId;
use swb_net::Url;
use swb_web::{MediaDelivery, MediaErrorInfo, MediaInfo, MediaTask};

pub(crate) use backend::MediaContext;
use backend::{Failure, Fetch, Preload};
pub(crate) use frames::VideoView;
pub(crate) use select::Cx;

/// `networkState` values (HTML "network states").
pub(crate) const NETWORK_EMPTY: u16 = 0;
pub(crate) const NETWORK_IDLE: u16 = 1;
pub(crate) const NETWORK_LOADING: u16 = 2;
pub(crate) const NETWORK_NO_SOURCE: u16 = 3;

/// `readyState` values (HTML "ready states").
pub(crate) const HAVE_NOTHING: u16 = 0;
pub(crate) const HAVE_METADATA: u16 = 1;
pub(crate) const HAVE_CURRENT_DATA: u16 = 2;
pub(crate) const HAVE_ENOUGH_DATA: u16 = 4;

/// An idle pipeline suspends after this many microseconds of page time (ADR
/// 0047 section 6: swb's choice). Due times are whole microseconds on both
/// sides of the page clock, so they compare without a margin.
pub(crate) const SUSPEND_AFTER_US: i64 = 15_000_000;

/// The most media elements that have a state. At the limit, the states of
/// elements outside the document that do not play go first; with none to
/// drop, a new element does not load (one warning).
pub(crate) const MAX_ELEMENTS: usize = 16_384;

/// The most tasks that one delivery hands over.
pub(crate) const MAX_DELIVERY: usize = 4096;

/// The most undelivered entries in the queue; a task that would come after
/// them is dropped (the settling of `play()` promises is not: it is one
/// entry for each run of promises). A page that makes more than this in one
/// task (a flood of `pause()` and `play()`) loses the events past the bound.
pub(crate) const MAX_QUEUED: usize = 65_536;

/// The most `play()` promises of one element that wait for their settling.
pub(crate) const MAX_PENDING_PLAYS: u64 = 65_536;

/// Page time in microseconds, the unit of the pipeline's clock, from page
/// time in milliseconds. It rounds: the time of a due point comes back from
/// `Pipeline::next_due` in microseconds, goes through the page clock as
/// milliseconds, and must give the same microsecond again (truncating gave
/// one too few, and virtual time jumped to a point that was not yet due, again
/// and again).
pub(crate) fn ms_to_us(ms: f64) -> i64 {
    (ms * 1000.0).round() as i64
}

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
    /// runs in a stable state waits for its marker in the queue.
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

/// An entry of the queue of media element tasks.
#[derive(Clone, Copy)]
pub(super) enum Entry {
    /// A task for the node `target` of the media element `owner`, queued
    /// in the epoch of the element.
    Task {
        target: NodeId,
        owner: NodeId,
        epoch: u32,
        task: MediaTask,
    },
    /// The stable-state step of the resource selection of `node` waits here
    /// (see the module comment).
    Select { node: NodeId, epoch: u32 },
}

/// The playback that runs without a pipeline after a late failure (see
/// `playback.rs`): the pipeline is gone, so the element counts the time
/// itself until the damage. All times are whole microseconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Emulation {
    /// The media time and the page time where it started.
    pub(crate) anchor_media: i64,
    pub(crate) anchor: i64,
    /// The media time of the next `timeupdate` and of the damage.
    pub(crate) next_tick: i64,
    pub(crate) deadline: i64,
}

impl Emulation {
    /// The media time at page time `now`.
    pub(crate) fn position(&self, now: i64) -> i64 {
        self.anchor_media
            .saturating_add(now.saturating_sub(self.anchor))
            .min(self.deadline)
    }

    /// The page time of the next unit of work.
    pub(crate) fn next_due(&self) -> i64 {
        self.anchor.saturating_add(
            self.next_tick
                .min(self.deadline)
                .saturating_sub(self.anchor_media),
        )
    }
}

/// What an element knows of its load and its playback.
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
    /// The page time (microseconds) since which the element has been idle
    /// with all its data, for the suspend of its pipeline.
    pub(crate) idle_since: Option<i64>,
    /// The epoch of the element: a load increases it, and the queued tasks
    /// of older epochs are dropped.
    pub(crate) epoch: u32,
    // ----- Playback (feature 6b) -----
    /// `paused`.
    pub(crate) paused: bool,
    /// HTML "can autoplay flag": cleared by `play()` and `pause()`.
    pub(crate) can_autoplay: bool,
    /// `seeking`.
    pub(crate) seeking: bool,
    /// Playback reached the end (`ended`, with `loop` off).
    pub(crate) ended: bool,
    /// A looping media reached its end: the next unit seeks to the start.
    pub(crate) loop_pending: bool,
    /// The first data of an `autoplay` element that the policy allows as a
    /// muted `video` came: the next unit starts it.
    pub(crate) autoplay_pending: bool,
    /// The source failed (no metadata): the next unit queues `error` and
    /// ends the playback that was asked for.
    pub(crate) failed_pending: bool,
    /// The first data of the load was reported.
    pub(crate) first_data: bool,
    /// Where playback starts when the metadata arrives: a `currentTime` that
    /// a script set before it (HTML "default playback start position").
    pub(crate) default_start: Option<f64>,
    /// `volume`, `muted`, `playbackRate`, `defaultPlaybackRate`: they stay
    /// when the element loads again.
    pub(crate) volume: f64,
    pub(crate) muted: bool,
    pub(crate) rate: f64,
    pub(crate) default_rate: f64,
    /// The element plays because the policy lets a muted `video` play
    /// without an activation: it pauses when a script unmutes it before the
    /// user has done anything (Chromium).
    pub(crate) muted_exempt: bool,
    /// `played`: the ranges that playback covered (sorted, not touching).
    pub(crate) played: Vec<(f64, f64)>,
    /// Where the current run of playback began (`played` takes it at the
    /// next stop).
    pub(crate) run_start: Option<f64>,
    /// The numbers of the `play()` promises: the last one made, the highest
    /// one that waits for the next `playing` (or a rejection), and the
    /// highest one whose settling was delivered.
    pub(crate) play_last: u64,
    pub(crate) pending_upto: Option<u64>,
    pub(crate) play_delivered: u64,
    /// The failure that came after the metadata (see `playback.rs`), and the
    /// playback that counts the time until it comes out.
    pub(crate) damage: Option<Failure>,
    pub(crate) emulation: Option<Emulation>,
    /// The position of an element whose pipeline failed.
    pub(crate) frozen: Option<f64>,
    /// HTML "show poster flag": set at each load, cleared by `play()` and by
    /// a seek after the metadata. While it is set a video with a `poster`
    /// attribute shows its poster, else its frame (ADR 0047 section 9).
    pub(crate) show_poster: bool,
    /// The element has a box (`false`: `display: none`, or not in the
    /// document): its frames are not converted (ADR 0047 section 6).
    pub(crate) painted: bool,
}

impl Element {
    fn new(video: bool, muted: bool) -> Element {
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
            epoch: 0,
            paused: true,
            can_autoplay: true,
            seeking: false,
            ended: false,
            loop_pending: false,
            autoplay_pending: false,
            failed_pending: false,
            first_data: false,
            default_start: None,
            volume: 1.0,
            muted,
            rate: 1.0,
            default_rate: 1.0,
            muted_exempt: false,
            played: Vec::new(),
            run_start: None,
            play_last: 0,
            pending_upto: None,
            play_delivered: 0,
            damage: None,
            emulation: None,
            frozen: None,
            show_poster: true,
            painted: true,
        }
    }

    /// The playback position (seconds) at page time `now_ms`: the number
    /// that `currentTime` reads. Before the metadata it is the position that
    /// a script set, or 0.
    pub(crate) fn position(&self, now_ms: f64) -> f64 {
        if let Some(emulation) = &self.emulation {
            return emulation.position(ms_to_us(now_ms)) as f64 / 1e6;
        }
        if let Some(frozen) = self.frozen {
            return frozen;
        }
        match (&self.fetch, &self.meta) {
            (Some(fetch), Some(_)) => fetch.position_at(ms_to_us(now_ms)),
            _ => self.default_start.unwrap_or(0.0),
        }
    }

    /// The silent state that the autoplay policy accepts for a `video`:
    /// muted, or a volume of 0.
    pub(crate) fn silent(&self) -> bool {
        self.muted || self.volume == 0.0
    }

    /// `played` as it reads now: the ranges that playback covered, and the
    /// run that goes on.
    fn played_now(&self, now_ms: f64) -> Vec<(f64, f64)> {
        let mut ranges = self.played.clone();
        if let Some(start) = self.run_start {
            playback::add_range(&mut ranges, start, self.position(now_ms).max(start));
        }
        ranges
    }

    /// What the element reports to scripts at page time `now_ms`.
    fn info(&self, now_ms: f64) -> MediaInfo {
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
            current_time: self.position(now_ms),
            paused: self.paused,
            seeking: self.seeking,
            video_width: size.map_or(0, |s| s.0),
            video_height: size.map_or(0, |s| s.1),
            buffered,
            seekable,
            played: self.played_now(now_ms),
            ended: self.ended,
            volume: self.volume,
            muted: self.muted,
            playback_rate: self.rate,
            default_playback_rate: self.default_rate,
        }
    }

    /// Whether virtual time must wait for this element: a selection step, a
    /// fetch that opens or has notes that the page has not handled, data
    /// that is not enough yet.
    fn pending(&self) -> bool {
        if self.failed_pending {
            return true;
        }
        match self.phase {
            Phase::Selecting => true,
            Phase::Fetching => {
                let Some(fetch) = &self.fetch else {
                    return false;
                };
                if fetch.is_suspended() {
                    return false;
                }
                self.loop_pending
                    || self.autoplay_pending
                    || fetch.has_notes()
                    || fetch.pending()
                    || (self.meta.is_some()
                        && self.ready_state < HAVE_ENOUGH_DATA
                        && self.damage.is_none())
                    || (self.meta.is_some() && !self.suspend_sent && !fetch.net_idle())
            }
            Phase::Empty | Phase::Deferred | Phase::Waiting | Phase::Failed => false,
        }
    }

    /// The page time (ms) of the next unit of work that time brings: the
    /// next frame, `timeupdate` point or end of the pipeline, the next tick
    /// or the damage of a playback without a pipeline, the suspend of an
    /// idle pipeline.
    fn next_due(&self) -> Option<f64> {
        let idle = self
            .idle_since
            .map(|since| since.saturating_add(SUSPEND_AFTER_US) as f64 / 1000.0);
        let pipeline = self.fetch.as_ref().and_then(Fetch::next_due);
        let emulation = self
            .emulation
            .as_ref()
            .map(|e| e.next_due() as f64 / 1000.0);
        [idle, pipeline, emulation]
            .into_iter()
            .flatten()
            .reduce(f64::min)
    }
}

/// The media elements of a document.
pub(crate) struct Media {
    /// The page's shared state (the bound on open pipelines).
    pub(super) ctx: MediaContext,
    /// By node: the iteration order, and so the order of the events of
    /// different elements, is the same on every run.
    pub(super) elements: BTreeMap<NodeId, Element>,
    /// The tasks that wait for the script runtime, in order.
    pub(super) events: VecDeque<Entry>,
    /// The elements whose next unit is the one after a failed source
    /// (`Element::failed_pending`), in node order; an entry of an element
    /// that a load reset does nothing. A set, so that a page with 16,000
    /// failing videos does not search all elements at each step.
    pub(super) failed: BTreeSet<NodeId>,
    /// The number of the last `MediaError` that an element got.
    pub(super) error_serial: u64,
    /// The page time of the last step, in microseconds.
    pub(super) now_us: i64,
    pub(super) warned: bool,
    /// The elements whose images (the natural size, the frame, whether the
    /// poster shows) may have changed since the document last took them
    /// ([`Media::take_touched`]): the node of each unit of a step and of each
    /// call of a script. A document with thousands of media elements does not
    /// look at all of them after each unit.
    pub(super) touched: BTreeSet<NodeId>,
    /// The states of elements went (the table purged, the document left):
    /// the next sync looks at all of them.
    pub(super) resync_all: bool,
}

impl Media {
    /// No elements.
    pub(crate) fn new(ctx: MediaContext) -> Media {
        Media {
            ctx,
            elements: BTreeMap::new(),
            events: VecDeque::new(),
            failed: BTreeSet::new(),
            error_serial: 0,
            now_us: 0,
            warned: false,
            touched: BTreeSet::new(),
            resync_all: false,
        }
    }

    /// Forgets every element: the pipelines close (a document that left its
    /// frame).
    pub(crate) fn clear(&mut self) {
        self.elements.clear();
        self.events.clear();
        self.resync_all = true;
    }

    /// Notes that the images of the element `node` may have changed.
    pub(crate) fn touch(&mut self, node: NodeId) {
        self.touched.insert(node);
    }

    /// The elements noted by [`Media::touch`] since the last call.
    pub(crate) fn take_touched(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.touched).into_iter().collect()
    }

    /// Whether the document has to look at all elements ([`Media::resync_all`]);
    /// clears the flag.
    pub(crate) fn take_resync_all(&mut self) -> bool {
        std::mem::take(&mut self.resync_all)
    }

    /// True if no media element has a state.
    pub(crate) fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// The number of entries that wait for delivery.
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    /// True if tasks wait for delivery (the next entry is a task, not the
    /// marker of a step: [`Media::step`] handles that).
    pub(crate) fn has_events(&self) -> bool {
        matches!(self.events.front(), Some(Entry::Task { .. }))
    }

    /// The page time of the media state, in milliseconds.
    pub(crate) fn now_ms(&self) -> f64 {
        self.now_us as f64 / 1000.0
    }

    /// Takes the next tasks for the script runtime (at most
    /// [`MAX_DELIVERY`]), up to the next stable-state marker, which stays at
    /// the front: the page runs [`Media::step`], which handles it, before
    /// it asks again. Tasks of an older epoch of their element are dropped.
    pub(crate) fn take_events(&mut self) -> Vec<MediaDelivery> {
        let mut out = Vec::new();
        while out.len() < MAX_DELIVERY {
            // A marker that a later load replaced does nothing: it goes
            // here, or a flood of loads would cost a round for each.
            if let Some(&Entry::Select { node, epoch }) = self.events.front()
                && !self
                    .elements
                    .get(&node)
                    .is_some_and(|e| e.epoch == epoch && e.phase == Phase::Selecting)
            {
                self.events.pop_front();
                continue;
            }
            let Some(&Entry::Task {
                target,
                owner,
                epoch,
                task,
            }) = self.events.front()
            else {
                break;
            };
            self.events.pop_front();
            // `task_begin` drops a task of an older epoch at its delivery
            // too, so this check changes no result; it only saves the
            // budget of `MAX_DELIVERY` and the round trip for tasks that a
            // load dropped (a flood of loads queues many).
            if self.elements.get(&owner).is_some_and(|e| e.epoch != epoch) {
                continue;
            }
            out.push(MediaDelivery {
                target,
                owner,
                epoch,
                task,
            });
        }
        out
    }

    /// A task is about to run ([`swb_web::EngineHost::media_task_begin`]):
    /// false if a load of its element dropped it since it was handed over.
    /// A settling of promises counts as delivered.
    pub(crate) fn task_begin(&mut self, owner: NodeId, epoch: u32, task: MediaTask) -> bool {
        let Some(element) = self.elements.get_mut(&owner) else {
            return true;
        };
        if element.epoch != epoch {
            return false;
        }
        if let MediaTask::ResolvePlay(upto) | MediaTask::RejectPlay(upto, _) = task {
            element.play_delivered = element.play_delivered.max(upto);
        }
        true
    }

    /// Queues the event `name` at `node`, a media element.
    pub(super) fn queue(&mut self, node: NodeId, name: &'static str) {
        self.queue_for(node, node, name);
    }

    /// Queues the event `name` at `target` (a media element or one of its
    /// `source` children) for the media element `owner`.
    pub(super) fn queue_for(&mut self, owner: NodeId, target: NodeId, name: &'static str) {
        self.push_task(owner, target, MediaTask::Event(name));
    }

    /// Queues a task for the media element `owner`. A settling of promises
    /// joins the one before it when that is the last entry (a flood of
    /// `play()` calls makes one entry); a bound on the queue drops the
    /// events past it.
    pub(super) fn push_task(&mut self, owner: NodeId, target: NodeId, task: MediaTask) {
        let epoch = self.elements.get(&owner).map_or(0, |e| e.epoch);
        if let Some(Entry::Task {
            target: last_target,
            owner: last_owner,
            epoch: last_epoch,
            task: last_task,
        }) = self.events.back_mut()
            && *last_owner == owner
            && *last_target == target
            && *last_epoch == epoch
        {
            match (&*last_task, task) {
                (MediaTask::ResolvePlay(_), MediaTask::ResolvePlay(upto)) => {
                    *last_task = MediaTask::ResolvePlay(upto);
                    return;
                }
                (MediaTask::RejectPlay(_, a), MediaTask::RejectPlay(upto, b)) if *a == b => {
                    *last_task = MediaTask::RejectPlay(upto, b);
                    return;
                }
                _ => {}
            }
        }
        let settles = !matches!(task, MediaTask::Event(_));
        if self.events.len() >= MAX_QUEUED && !settles {
            return;
        }
        self.events.push_back(Entry::Task {
            target,
            owner,
            epoch,
            task,
        });
    }

    /// Queues the stable-state step of the resource selection of `node`.
    pub(super) fn queue_select(&mut self, node: NodeId) {
        let epoch = self.elements.get(&node).map_or(0, |e| e.epoch);
        self.events.push_back(Entry::Select { node, epoch });
    }

    /// What `node` reports to scripts: its state, or the default of an
    /// element that has none (`muted_attribute`: the `muted` content
    /// attribute that it was made with).
    pub(crate) fn info(&self, node: NodeId, muted_attribute: bool) -> MediaInfo {
        self.elements.get(&node).map_or_else(
            || MediaInfo {
                muted: muted_attribute,
                ..MediaInfo::default()
            },
            |element| element.info(self.now_ms()),
        )
    }

    /// Whether virtual time must wait for a media element.
    pub(crate) fn pending(&self) -> bool {
        self.elements.values().any(Element::pending)
    }

    /// The page time (ms) of the next media due time: a frame or
    /// `timeupdate` point of a playing pipeline, the damage of a playback
    /// that counts its own time, the suspend of an idle pipeline.
    pub(crate) fn next_due(&self) -> Option<f64> {
        self.elements
            .values()
            .filter_map(Element::next_due)
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

#[cfg(test)]
mod tests {
    use super::Emulation;

    /// The emulated playback after a late failure counts from its anchor and
    /// stops at the place of the failure; its next unit is the next
    /// `timeupdate` or the failure, whichever comes first.
    #[test]
    fn the_emulation_stops_at_the_failure() {
        let e = Emulation {
            anchor_media: 500_000,
            anchor: 10_000_000,
            next_tick: 750_000,
            deadline: 900_000,
        };
        assert_eq!(e.position(10_000_000), 500_000);
        assert_eq!(e.position(10_300_000), 800_000);
        assert_eq!(e.position(10_400_000), 900_000);
        assert_eq!(e.position(99_000_000), 900_000);
        assert_eq!(e.next_due(), 10_250_000);
        let late = Emulation {
            next_tick: 1_000_000,
            ..e
        };
        assert_eq!(late.next_due(), 10_400_000);
        let far = Emulation {
            anchor: i64::MAX - 10,
            next_tick: i64::MAX,
            deadline: i64::MAX,
            ..e
        };
        // Far times saturate and do not overflow.
        assert_eq!(far.position(i64::MAX), 500_010);
        assert_eq!(far.next_due(), i64::MAX);
    }
}
