//! The media element load algorithm and the resource selection that
//! follows it (HTML,
//! <https://html.spec.whatwg.org/multipage/media.html#media-element-load-algorithm>),
//! and the failures of a load.
//!
//! The synchronous part runs when the DOM change is processed
//! ([`Media::load_algorithm`]): `abort`, `emptied` and `timeupdate` are
//! queued, the state goes back to the start, and `networkState` is
//! `NETWORK_NO_SOURCE`. The part that HTML runs "in a stable state"
//! ([`Media::selection_step`]) runs at the next pump: it chooses the
//! resource, sets `NETWORK_LOADING`, queues `loadstart` and starts the
//! fetch (or defers it under `preload=none`).

use swb_dom::{Document, NodeId, local_name};
use swb_net::Url;
use swb_web::{MediaErrorInfo, MediaTask, PlayRejection};

use super::backend::{self, Failure, Fetch};
use super::playback::autoplay_attribute;
use super::select::{self, Candidate, Cx, Pointer};
use super::{
    Element, HAVE_ENOUGH_DATA, HAVE_NOTHING, MAX_ELEMENTS, Media, NETWORK_EMPTY, NETWORK_IDLE,
    NETWORK_LOADING, NETWORK_NO_SOURCE, Phase, PreloadAttr,
};

/// The `preload` attribute of `node`: `none`, `metadata` or `auto` (also the
/// empty string); anything else, and a missing attribute, is `metadata`
/// (Chromium, measured: a missing attribute loads like `metadata`).
pub(super) fn preload_of(dom: &Document, node: NodeId) -> PreloadAttr {
    match dom.element(node).and_then(|e| e.attr("preload")) {
        Some(value) if value.eq_ignore_ascii_case("none") => PreloadAttr::None,
        Some(value) if value.is_empty() || value.eq_ignore_ascii_case("auto") => PreloadAttr::Auto,
        _ => PreloadAttr::Metadata,
    }
}

impl Media {
    /// The element of `node`, made if there is none (the table is
    /// bounded). `None` when the table is full.
    pub(super) fn element_mut(&mut self, cx: &Cx<'_>, node: NodeId) -> Option<&mut Element> {
        if !self.ctx.enabled() {
            return None;
        }
        if !self.elements.contains_key(&node) {
            if self.elements.len() >= MAX_ELEMENTS {
                // The states of elements outside the document go first (not
                // those that play: a `new Audio()` lives while it plays).
                self.elements
                    .retain(|&n, e| cx.dom.is_connected(n) || !e.paused);
            }
            if self.elements.len() >= MAX_ELEMENTS {
                if !self.warned {
                    self.warned = true;
                    log::warn!(
                        "more than {MAX_ELEMENTS} media elements are in the document; further ones do not load"
                    );
                }
                return None;
            }
            let video = cx.dom.is_html_element(node, &local_name!("video"));
            // `muted` starts as the content attribute says (HTML: the
            // attribute at the creation of the element); it is a state of
            // its own after that.
            let muted = cx
                .dom
                .element(node)
                .is_some_and(|e| e.attr("muted").is_some());
            self.elements.insert(node, Element::new(video, muted));
        }
        self.elements.get_mut(&node)
    }

    /// `src` was set or changed on `node`, or `load()` was called
    /// (`forced`): the media element load algorithm.
    pub(crate) fn load_algorithm(&mut self, cx: &Cx<'_>, node: NodeId, forced: bool) {
        let Some(element) = self.element_mut(cx, node) else {
            return;
        };
        // 1 to 5 of the algorithm: what a running load queues.
        let mut queue = Vec::new();
        if matches!(element.network_state, NETWORK_LOADING | NETWORK_IDLE) {
            queue.push("abort");
        }
        if element.network_state != NETWORK_EMPTY {
            queue.push("emptied");
            // Chromium fires `timeupdate` also when the position is 0.
            queue.push("timeupdate");
        }
        // The fetch is dropped: its pipeline closes.
        element.fetch = None;
        element.ready_state = HAVE_NOTHING;
        element.meta = None;
        element.error = None;
        element.pointer = None;
        element.candidate = None;
        element.deferred = None;
        element.progress_sent = false;
        element.suspend_sent = false;
        element.idle_since = None;
        element.forced = forced;
        // Step 6: the tasks that are queued for the element are dropped (the
        // epoch), and its playback starts again: paused, nothing played.
        // `volume`, `muted` and `loop` stay; the rate goes back to the
        // default. The promises of `play()` that no one settled yet reject.
        element.epoch = element.epoch.wrapping_add(1);
        element.paused = true;
        element.can_autoplay = true;
        element.seeking = false;
        element.ended = false;
        element.loop_pending = false;
        element.autoplay_pending = false;
        element.failed_pending = false;
        element.first_data = false;
        element.muted_exempt = false;
        element.played.clear();
        element.run_start = None;
        element.damage = None;
        element.emulation = None;
        element.frozen = None;
        element.rate = element.default_rate;
        element.pending_upto = None;
        let unsettled = (element.play_last > element.play_delivered).then_some(element.play_last);
        // The resource selection algorithm, step 1.
        element.network_state = NETWORK_NO_SOURCE;
        element.phase = Phase::Selecting;
        for name in queue {
            self.queue(node, name);
        }
        if let Some(upto) = unsettled {
            self.push_task(node, node, MediaTask::RejectPlay(upto, PlayRejection::Load));
        }
        self.queue_select(node);
    }

    /// A `source` was inserted into the media element `parent`: HTML says a
    /// media element that has no `src` and is `NETWORK_EMPTY` runs the
    /// resource selection algorithm, and one that waits for a `source`
    /// looks at the new one.
    pub(crate) fn source_inserted(&mut self, cx: &Cx<'_>, parent: NodeId) {
        if select::src_attribute(cx.dom, parent).is_some() {
            return;
        }
        let Some(element) = self.element_mut(cx, parent) else {
            return;
        };
        if matches!(element.phase, Phase::Empty | Phase::Waiting) {
            element.network_state = NETWORK_NO_SOURCE;
            element.phase = Phase::Selecting;
            self.queue_select(parent);
        }
    }

    /// A media element was found (inserted, or walked): it loads if it has a
    /// `src` or a `source` child. It gets a state in any case (`muted` starts
    /// as its content attribute says); one that is known already keeps its
    /// load (a re-insertion starts nothing: a suspended pipeline opens again
    /// when `play()` or a seek needs it).
    pub(crate) fn discovered(&mut self, cx: &Cx<'_>, node: NodeId) {
        if self.elements.contains_key(&node) {
            return;
        }
        let has_source = select::next_source(cx.dom, node, Pointer::Start).is_some();
        if select::src_attribute(cx.dom, node).is_none() && !has_source {
            self.element_mut(cx, node);
            return;
        }
        self.load_algorithm(cx, node, false);
    }

    /// The children `removed` of the media element `parent` were removed;
    /// `before` was the child before them. A pointer after one of them
    /// moves to after `before` (HTML: the pointer does not move relative to
    /// the children that stay).
    pub(crate) fn children_removed(
        &mut self,
        parent: NodeId,
        removed: &[NodeId],
        before: Option<NodeId>,
    ) {
        if let Some(element) = self.elements.get_mut(&parent)
            && let Some(Pointer::After(node)) = element.pointer
            && removed.contains(&node)
        {
            element.pointer = Some(before.map_or(Pointer::Start, Pointer::After));
        }
    }

    /// The `preload` attribute of `node` changed: a fetch that
    /// `preload=none` held back starts; a running fetch reads ahead as the
    /// value says.
    pub(crate) fn preload_changed(&mut self, cx: &Cx<'_>, node: NodeId) {
        let preload = preload_of(cx.dom, node);
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        if let Some(fetch) = &mut element.fetch {
            fetch.set_preload(preload.read_ahead());
        } else if element.phase == Phase::Deferred
            && preload != PreloadAttr::None
            && let Some(url) = element.deferred.take()
        {
            self.begin_fetch(cx, node, &url);
        }
    }

    /// The media element `node` left the document: HTML runs the internal
    /// pause steps ("When a media element is removed from a Document", as
    /// Chromium does: `timeupdate`, `pause`), and a pipeline that has all
    /// its data suspends at once. A load that runs goes on (HTML does not stop
    /// its load); the rule of the idle pipeline suspends it 15 s after it has
    /// its data.
    pub(crate) fn removed(&mut self, node: NodeId) {
        self.pause_steps(node, PlayRejection::Pause);
        let now = self.now_us;
        if let Some(element) = self.elements.get_mut(&node)
            && element.ready_state == HAVE_ENOUGH_DATA
            && let Some(fetch) = &mut element.fetch
            && fetch.can_suspend()
        {
            fetch.suspend(now);
            element.idle_since = None;
        }
    }

    /// The stable-state part of the resource selection algorithm
    /// ("await a stable state"): chooses the mode, sets `NETWORK_LOADING`,
    /// queues `loadstart` and goes on with the resource.
    pub(super) fn selection_step(&mut self, cx: &Cx<'_>, node: NodeId) {
        let resumed = {
            let Some(element) = self.elements.get_mut(&node) else {
                return;
            };
            element.phase = Phase::Empty;
            // A resumed wait goes on with the next candidate; `loadstart`
            // was queued before.
            element.network_state == NETWORK_NO_SOURCE && element.pointer.is_some()
        };
        let attribute = select::src_attribute(cx.dom, node);
        let children =
            attribute.is_none() && select::next_source(cx.dom, node, Pointer::Start).is_some();
        if attribute.is_none() && !children {
            if let Some(element) = self.elements.get_mut(&node) {
                element.network_state = NETWORK_EMPTY;
            }
            return;
        }
        if let Some(element) = self.elements.get_mut(&node) {
            element.network_state = NETWORK_LOADING;
        }
        if !resumed {
            self.queue(node, "loadstart");
        }
        match attribute {
            Some(src) => self.attribute_mode(cx, node, &src),
            None => self.children_mode(cx, node),
        }
    }

    /// The `src` attribute: resolve it and load it, or fail.
    fn attribute_mode(&mut self, cx: &Cx<'_>, node: NodeId, src: &str) {
        if src.is_empty() {
            self.fail_attribute(node, "MEDIA_ELEMENT_ERROR: Empty src attribute");
            return;
        }
        let url = match cx.base.join(src) {
            Ok(url) if cx.may_load(&url) => url,
            Ok(url) => {
                log::warn!("not allowed to load the media {url}");
                self.fail_attribute(node, backend::FORMAT_ERROR);
                return;
            }
            Err(error) => {
                log::warn!("media URL {src:?}: {error}");
                self.fail_attribute(node, backend::FORMAT_ERROR);
                return;
            }
        };
        if let Some(element) = self.elements.get_mut(&node) {
            element.current_src = url.to_string();
        }
        self.start_resource(cx, node, url);
    }

    /// The `source` children: the next candidate that fits, with an `error`
    /// at each that does not; when none is left, the element waits.
    pub(super) fn children_mode(&mut self, cx: &Cx<'_>, node: NodeId) {
        loop {
            let pointer = self
                .elements
                .get(&node)
                .and_then(|e| e.pointer)
                .unwrap_or(Pointer::Start);
            let Some(source) = select::next_source(cx.dom, node, pointer) else {
                if let Some(element) = self.elements.get_mut(&node) {
                    element.network_state = NETWORK_NO_SOURCE;
                    element.phase = Phase::Waiting;
                    element.candidate = None;
                }
                return;
            };
            if let Some(element) = self.elements.get_mut(&node) {
                element.pointer = Some(Pointer::After(source));
            }
            match select::candidate(cx, source) {
                Candidate::Skip => {}
                Candidate::Fail => self.queue_for(node, source, "error"),
                Candidate::Use(url) => {
                    if let Some(element) = self.elements.get_mut(&node) {
                        element.current_src = url.to_string();
                        element.candidate = Some(source);
                    }
                    self.start_resource(cx, node, url);
                    return;
                }
            }
        }
    }

    /// The resource is chosen: fetch it, or keep it back under
    /// `preload=none` (`loadstart` is followed by `suspend`, the network is
    /// idle) until a load is asked for.
    fn start_resource(&mut self, cx: &Cx<'_>, node: NodeId, url: Url) {
        let preload = preload_of(cx.dom, node);
        // An element that `play()` started loads whatever `preload` says
        // (the deferred fetch began then), and so does an `autoplay`
        // element that the policy lets play (Chromium, measured: the
        // `autoplay` attribute wins over `preload=none`).
        let wanted = self.elements.get(&node).is_some_and(|e| {
            e.forced
                || !e.paused
                || (autoplay_attribute(cx, node) && super::playback::allowed_to_play(cx, e))
        });
        if preload == PreloadAttr::None && !wanted {
            if let Some(element) = self.elements.get_mut(&node) {
                element.network_state = NETWORK_IDLE;
                element.suspend_sent = true;
                element.phase = Phase::Deferred;
                element.deferred = Some(url);
            }
            self.queue(node, "suspend");
            return;
        }
        self.begin_fetch(cx, node, &url);
    }

    /// Starts the fetch of `url`.
    pub(super) fn begin_fetch(&mut self, cx: &Cx<'_>, node: NodeId, url: &Url) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let crossorigin = cx.dom.element(node).and_then(|e| e.attr("crossorigin"));
        let mut fetch_url = url.clone();
        fetch_url.set_fragment(None);
        let request = backend::request_for(
            fetch_url,
            element.video,
            crossorigin,
            cx.initiator.clone(),
            cx.site_for_cookies.clone(),
        );
        log::debug!("media request: {url}");
        let mut fetch = Fetch::start(
            &self.ctx,
            cx.loader,
            request,
            element.video,
            preload_of(cx.dom, node).read_ahead(),
        );
        fetch.set_gain(if element.muted {
            0.0
        } else {
            element.volume as f32
        });
        element.fetch = Some(fetch);
        element.phase = Phase::Fetching;
        element.network_state = NETWORK_LOADING;
        element.progress_sent = false;
        element.suspend_sent = false;
    }

    /// A new `MediaError` for `node`.
    pub(super) fn new_error(&mut self, code: u16, message: &str) -> MediaErrorInfo {
        self.error_serial += 1;
        MediaErrorInfo {
            code,
            message: message.to_owned(),
            serial: self.error_serial,
        }
    }

    /// The `src` attribute failed (an empty value, a URL that does not
    /// resolve): `MEDIA_ERR_SRC_NOT_SUPPORTED`, `NETWORK_NO_SOURCE`, `error`.
    fn fail_attribute(&mut self, node: NodeId, message: &str) {
        let error = self.new_error(4, message);
        if let Some(element) = self.elements.get_mut(&node) {
            element.error = Some(error);
            element.network_state = NETWORK_NO_SOURCE;
            element.phase = Phase::Failed;
        }
        self.source_failed(node);
    }

    /// The source failed (HTML "dedicated media source failure steps"): the
    /// `error` event and the end of the playback that was asked for come in
    /// the next unit, after the events that this unit queued were delivered
    /// (Chromium: `play`, `waiting`, `loadstart`, `progress` and `suspend`
    /// see `paused` false, `error` sees it true; probe rows
    /// `play-pending-src-*`).
    fn source_failed(&mut self, node: NodeId) {
        if let Some(element) = self.elements.get_mut(&node) {
            element.failed_pending = true;
            self.failed.insert(node);
        }
    }

    /// The unit after a failed source: `error`; an element that was playing
    /// or about to pauses (`timeupdate`, `pause`) and the pending `play()`
    /// promises reject with `NotSupportedError`. True if it did the unit.
    pub(super) fn step_failed(&mut self, node: NodeId) -> bool {
        let Some(element) = self.elements.get_mut(&node) else {
            return false;
        };
        if !std::mem::take(&mut element.failed_pending) {
            return false;
        }
        self.queue(node, "error");
        self.pause_steps(node, PlayRejection::NotSupported);
        true
    }

    /// The load of the element failed before its metadata. With bytes that
    /// arrived (the demuxer could not open them), `progress` and `suspend`
    /// come first. In attribute mode the element gets the error; in
    /// children mode the `source` gets an `error` event and the next
    /// candidate is tried.
    pub(super) fn load_failed(&mut self, cx: &Cx<'_>, node: NodeId, failure: &Failure) {
        let Some(element) = self.elements.get_mut(&node) else {
            return;
        };
        let (progress, suspend) = (element.progress_sent, element.suspend_sent);
        element.fetch = None;
        element.progress_sent = false;
        element.suspend_sent = false;
        let candidate = element.candidate.take();
        if failure.bytes_arrived {
            if !progress {
                self.queue(node, "progress");
            }
            if !suspend {
                self.queue(node, "suspend");
            }
        }
        match candidate {
            None => {
                let error = self.new_error(failure.code, &failure.message);
                if let Some(element) = self.elements.get_mut(&node) {
                    element.error = Some(error);
                    element.network_state = NETWORK_NO_SOURCE;
                    element.phase = Phase::Failed;
                }
                self.source_failed(node);
            }
            Some(source) => {
                self.queue_for(node, source, "error");
                self.children_mode(cx, node);
            }
        }
    }
}
