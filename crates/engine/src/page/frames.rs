//! The frames of the page (ADR 0031 sections 2 to 4 and 11): the life of a
//! frame, the navigation of frames, the layout of frame documents and the
//! preparation of their paint. The records are in `crate::frames`; the arrival
//! and commit of frame documents are in `frame_commit.rs`, the load state and
//! the refresh in `frame_load.rs`.
//!
//! - *Insertion* of an `iframe` into a document of the page (the journal
//!   drain reports it, [`Page::update_frames`]) makes a frame with an
//!   initial `about:blank` document: the parent's origin, quirks mode,
//!   UTF-8. Then its attributes navigate it (HTML "process the iframe
//!   attributes"): `srcdoc` to an `about:srcdoc` document (no-quirks, the
//!   parent's origin), else a `src` that parses to its URL; with neither
//!   (or `about:blank`) the frame is loaded at once. A change of `src` or
//!   `srcdoc` navigates again, and so does the frame document's declarative
//!   refresh. *Removal* of the `iframe` destroys the frame and its
//!   descendants and cancels their requests; so does a new document in the
//!   parent.
//! - *Loading*: a frame document's request has the destination `iframe`,
//!   the parent's origin as initiator and the parent's site for cookies.
//!   A navigation commits when its document arrives; documents without a
//!   request commit in the next turn of the event loop
//!   ([`Page::run_frame_navigations`]). Each document has its loader
//!   client, so its subresources complete into it.
//! - *Load state*: a frame delays its parent's `load` until its document
//!   and its own frames have loaded; then the container's `load` event
//!   fires (as a task of the parent's runtime when the parent has scripts)
//!   ([`Page::check_frame_loads`]). Frames that a document gets after its
//!   `load` do not delay it.
//! - *Viewports*: after the parent's layout, each frame's viewport is the
//!   content box of its container, clamped to [`MAX_VIEWPORT_SIDE`]; a
//!   frame without a box keeps its last viewport ([`Page::layout_frames`]).
//!
//! - *Paint*: [`Page::prepare_frames`] brings the display lists up to date
//!   before a rendering; the rasterizer draws them in place of the `iframe`
//!   contents (`crate::frames::paint`, `swb_paint`).
//!
//! A frame document that has a script gets a realm when it commits, in the
//! runtime of its origin (`page/agents.rs`), and the runtime drives its parse as
//! for the top-level document; one that no script touches costs no realm. With a
//! script runtime, a task makes and destroys frames itself
//! (`crate::frames::registry`); the page does what is left after the task
//! ([`Page::run_task_in`]).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use swb_css::ColorScheme;
use swb_dom::NodeId;
use swb_layout::Size;
use swb_net::{Destination, Origin, Request, Url};
use swb_web::ScriptRuntime;

use super::frame_commit::srcdoc_url;
use super::loading::Parsed;
use super::{MAX_VIEWPORT_SIDE, Page, about_blank, is_loadable, strip_fragment};
use crate::document::DocumentState;
use crate::document::frames::iframes_in;
use crate::frames::registry::{BLANK, ChildInit, FrameSettings, attribute_target};
use crate::frames::{
    Charge, FrameId, FrameScript, FrameTarget, MAX_NAVIGATIONS, PendingNavigation, Sandbox,
};
use crate::history::PostData;
use crate::resources::Images;

/// The document that starts a navigation of a frame: its origin, which the request
/// carries as its initiator, and the URL that gives it that origin, which is the source of
/// the referrer of the new document ([`crate::referrer`]).
#[derive(Clone, Debug)]
pub(super) struct Initiator {
    pub(super) origin: Origin,
    pub(super) url: Url,
}

impl Page {
    /// The document of `frame` as the initiator of a navigation.
    pub(super) fn initiator_of(&self, frame: FrameId) -> Option<Initiator> {
        let doc = self.doc_of(frame)?;
        Some(Initiator {
            origin: doc.origin(),
            url: doc.cookie_url().clone(),
        })
    }

    // ----- The frame tree for tests and tools -----

    /// The frames of the page in tree order, the top-level frame first.
    pub fn frame_ids(&self) -> Vec<FrameId> {
        self.frames.tree_order()
    }

    /// The number of frames besides the top-level one (`page.info`).
    pub fn frame_count(&self) -> usize {
        self.frames.count()
    }

    /// The document of frame `id` ([`FrameId::TOP`]: the page's document).
    /// `None` for a destroyed frame.
    pub fn frame_document(&self, id: FrameId) -> Option<&DocumentState> {
        self.doc_of(id)
    }

    /// The parent of frame `id`; `None` for the top-level frame and for a
    /// destroyed one.
    pub fn frame_parent(&self, id: FrameId) -> Option<FrameId> {
        self.frames.get(id)?.parent
    }

    /// The child frames of `id` in the tree order of their containers.
    pub fn frame_children(&self, id: FrameId) -> Vec<FrameId> {
        self.frames
            .get(id)
            .map(|f| f.children.clone())
            .unwrap_or_default()
    }

    /// The `iframe` element of frame `id` in its parent's document.
    pub fn frame_container(&self, id: FrameId) -> Option<NodeId> {
        self.frames.get(id)?.container
    }

    /// The frame of the `iframe` element `container` of `parent`'s
    /// document, if it has one.
    pub fn frame_of(&self, parent: FrameId, container: NodeId) -> Option<FrameId> {
        self.frames.by_container(parent, container)
    }

    /// The viewport of frame `id` in CSS px (the page's viewport for the
    /// top-level frame).
    pub fn frame_viewport(&self, id: FrameId) -> Option<Size> {
        if id == FrameId::TOP {
            return Some(self.view.viewport);
        }
        Some(self.frames.get(id)?.viewport)
    }

    /// The bytes of source text of the live frame documents, which count
    /// against [`MAX_SOURCE_BYTES`].
    pub fn frame_source_bytes(&self) -> usize {
        self.frames.charged().bytes
    }

    /// The DOM nodes of the live frame documents, which count against
    /// [`MAX_NODES`].
    pub fn frame_node_count(&self) -> usize {
        self.frames.charged().nodes
    }

    /// The name of frame `id` (its container's `name` attribute).
    pub fn frame_name(&self, id: FrameId) -> Option<&str> {
        Some(self.frames.get(id)?.name.as_str())
    }

    /// The sandboxing flags of frame `id`'s document.
    pub fn frame_sandbox(&self, id: FrameId) -> Option<Sandbox> {
        Some(self.frames.get(id)?.sandbox)
    }

    /// True if the document of frame `id` has a realm, in the runtime of its
    /// origin.
    pub fn frame_has_realm(&self, id: FrameId) -> bool {
        self.frames.get(id).is_some_and(|f| f.script.is_some())
    }

    /// True while frame `id` has a navigation whose document has not
    /// arrived, or its document loads.
    pub fn frame_is_loading(&self, id: FrameId) -> bool {
        let Some(frame) = self.frames.get(id) else {
            return false;
        };
        frame.pending.is_some() || self.doc_of(id).is_some_and(DocumentState::is_loading)
    }

    /// True once frame `id`'s document and its delaying frames loaded and
    /// its container's `load` event fired.
    pub fn frame_load_fired(&self, id: FrameId) -> bool {
        self.frames.get(id).is_some_and(|f| f.load_fired)
    }

    // ----- Documents of frames -----

    /// The document of frame `id`.
    pub(super) fn doc_of(&self, id: FrameId) -> Option<&DocumentState> {
        if id == FrameId::TOP {
            return self.doc.as_deref();
        }
        self.frames.get(id)?.doc.as_deref()
    }

    pub(super) fn doc_of_mut(&mut self, id: FrameId) -> Option<&mut DocumentState> {
        if id == FrameId::TOP {
            return self.doc.as_deref_mut();
        }
        self.frames.get_mut(id)?.doc.as_deref_mut()
    }

    // ----- Life of frames -----

    /// Destroys all frames: a new top-level document (or none).
    pub(super) fn reset_frames(&mut self) {
        for id in self.frames.descendants_post_order(FrameId::TOP) {
            self.drop_frame(id);
        }
        // The arena stays, so that the ids of the old frames stay stale.
        self.frames.reset_top();
        self.frame_inputs.clear();
        self.deferred_navigations.clear();
        self.frame_navigations.clear();
        self.frame_fonts.reset_warning();
        self.frame_bodies.reset_warning();
    }

    /// Acts on the `iframe` changes that the drain of `frame`'s document
    /// found: destroys the frames of removed containers, makes frames for
    /// inserted ones and navigates those whose attributes changed.
    pub(super) fn update_frames(&mut self, frame: FrameId) {
        if self.lent && frame == FrameId::TOP {
            return;
        }
        let Some(doc) = self.doc_of_mut(frame) else {
            return;
        };
        if !doc.has_frame_changes() {
            return;
        }
        let changes = doc.take_frame_changes();
        if changes.rescan {
            self.rescan_frames(frame);
        }
        // With a runtime the natives destroyed and made the frames of what
        // the task moved; the frame of an element that is in the document
        // again is the new one.
        let scripted = self.frames.get(frame).is_some_and(|f| f.script.is_some());
        for node in changes.removed {
            if let Some(child) = self.frames.by_container(frame, node)
                && !(scripted && self.in_frame_tree(frame, node))
            {
                self.destroy_frame(child);
            }
        }
        let mut made = HashSet::new();
        for node in changes.inserted {
            if self.frames.by_container(frame, node).is_none()
                && self.in_frame_tree(frame, node)
                && let Some(child) = self.create_frame(frame, node)
            {
                made.insert(child);
            }
        }
        for set in changes.attributes {
            let Some(child) = self.frames.by_container(frame, set.node) else {
                continue;
            };
            if made.contains(&child)
                || self.made_in_task.contains(&child)
                || !self.in_frame_tree(frame, set.node)
            {
                continue;
            }
            if self.process_frame_attributes(child, false) {
                let parent_flags = self
                    .frames
                    .get(frame)
                    .map(|f| f.sandbox)
                    .unwrap_or_default();
                if let Some(record) = self.frames.get_mut(child) {
                    record.pending_sandbox = set.sandbox.inherit(parent_flags);
                }
            }
        }
    }

    /// Compares the frames of `parent` with the `iframe` elements of its
    /// document after a walk of the whole document: destroys the frames of
    /// containers that left it, makes the missing ones.
    fn rescan_frames(&mut self, parent: FrameId) {
        let Some(doc) = self.doc_of(parent) else {
            return;
        };
        let present = iframes_in(doc.dom());
        let set: HashSet<NodeId> = present.iter().copied().collect();
        let gone: Vec<FrameId> = self
            .frames
            .get(parent)
            .map(|f| f.children.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|&c| {
                self.frames
                    .get(c)
                    .and_then(|f| f.container)
                    .is_none_or(|n| !set.contains(&n))
            })
            .collect();
        for child in gone {
            self.destroy_frame(child);
        }
        for node in present {
            if self.frames.by_container(parent, node).is_none() {
                self.create_frame(parent, node);
            }
        }
    }

    /// True if `node` is in the document tree of `frame`'s document.
    fn in_frame_tree(&self, frame: FrameId, node: NodeId) -> bool {
        self.doc_of(frame).is_some_and(|d| {
            let dom = d.dom();
            dom.get(node).is_some() && dom.root(node) == NodeId::DOCUMENT
        })
    }

    /// Makes the frame of `container`, an `iframe` element in `parent`'s
    /// document, with its initial `about:blank` document, and processes
    /// its attributes. `None` past the limits of frames and depth
    /// (ADR 0031 section 11; a warning per top-level document).
    fn create_frame(&mut self, parent: FrameId, container: NodeId) -> Option<FrameId> {
        let doc = self.doc_of(parent)?;
        let init = ChildInit::of(doc, container)?;
        let at = {
            let dom = doc.dom();
            self.frames
                .child_position(parent, container, |a, b| dom.precedes(a, b))
        };
        let settings = self.frame_settings();
        let maker = Rc::clone(&self.maker);
        let id = self
            .frames
            .create_child(&maker, settings, parent, init, at)?;
        self.process_frame_attributes(id, true);
        Some(id)
    }

    /// The settings of the page that a new frame document takes.
    pub(super) fn frame_settings(&self) -> FrameSettings {
        FrameSettings {
            javascript: self.javascript,
            view: self.view,
        }
    }

    /// The origin of an `about:blank` or `about:srcdoc` document in a
    /// child of `parent`: the parent document's, or an opaque one in a
    /// sandbox without `allow-same-origin`.
    fn inherited_origin(&self, parent: FrameId, sandbox: Sandbox) -> Option<Origin> {
        if sandbox.opaque_origin() {
            return Some(Origin::new_opaque());
        }
        self.frames.get(parent)?.origin.clone()
    }

    /// The sandboxing flags of a child of `parent` whose container has the
    /// `sandbox` attribute `attr`: the attribute's flags and the parent
    /// frame's (HTML "determining the creation sandboxing flags").
    pub(super) fn container_sandbox(&self, parent: FrameId, attr: Option<&str>) -> Sandbox {
        let parent_flags = self
            .frames
            .get(parent)
            .map(|f| f.sandbox)
            .unwrap_or_default();
        Sandbox::parse(attr).inherit(parent_flags)
    }

    /// The attributes of a frame that a task made.
    pub(super) fn process_new_frame(&mut self, id: FrameId) {
        self.process_frame_attributes(id, true);
    }

    /// HTML "process the iframe attributes" for frame `id`; `initial` at
    /// its insertion. True if a navigation started.
    fn process_frame_attributes(&mut self, id: FrameId, initial: bool) -> bool {
        let Some(target) = self.attribute_target(id) else {
            return false;
        };
        match target {
            // HTML: `about:blank` at the insertion runs the iframe load
            // event steps; the initial document stays.
            FrameTarget::Blank if initial => false,
            target => {
                let started = self.navigate_frame(id, target);
                // The page reads the attributes after the task that inserted
                // the frame. If they already asked for a document at the
                // insertion, the navigation started then, and a later `sandbox`
                // does not count (measured).
                if started
                    && initial
                    && let Some(frame) = self.frames.get_mut(id)
                    && frame.reuse_window
                {
                    frame.pending_sandbox = frame.sandbox;
                }
                started
            }
        }
    }

    /// What the attributes of frame `id`'s container ask for (see
    /// [`attribute_target`]).
    fn attribute_target(&self, id: FrameId) -> Option<FrameTarget> {
        let frame = self.frames.get(id)?;
        let (parent, container) = (frame.parent?, frame.container?);
        attribute_target(self.doc_of(parent)?, container)
    }

    /// Navigates frame `id` to `target` (ADR 0031 section 2). Dropped past
    /// [`MAX_NAVIGATIONS`] in the window of page time (one warning per
    /// frame), for URLs that cannot be loaded, for `file:` URLs from a document that is not a
    /// `file:` one, and when two or more ancestors' documents have the URL
    /// (Chromium's rule, case `self-embed`). True if a navigation started.
    pub(super) fn navigate_frame(&mut self, id: FrameId, target: FrameTarget) -> bool {
        self.navigate_frame_with(id, target, None, None)
    }

    /// A navigation of frame `id` that a script's write to its `location` asked for
    /// in the task that ended: it has the sandboxing flags of the write.
    pub(super) fn navigate_frame_by_location(
        &mut self,
        id: FrameId,
        url: Url,
        initiator: Option<Initiator>,
    ) {
        let flags = self
            .frames
            .get_mut(id)
            .and_then(|frame| frame.location_sandbox.take())
            .map(|(_, flags)| flags);
        let started = self.navigate_frame_with(id, FrameTarget::Url(url), None, initiator);
        if started
            && let Some(flags) = flags
            && let Some(frame) = self.frames.get_mut(id)
        {
            frame.pending_sandbox = flags;
        }
    }

    /// [`Page::navigate_frame`] with a `POST` body, and the origin of the
    /// document that asks (the parent's if `None`).
    pub(super) fn navigate_frame_with(
        &mut self,
        id: FrameId,
        target: FrameTarget,
        post: Option<PostData>,
        initiator: Option<Initiator>,
    ) -> bool {
        let now = self.clock.now();
        let Some(frame) = self.frames.get_mut(id) else {
            return false;
        };
        if !frame.note_navigation(now) {
            if !std::mem::replace(&mut frame.navigation_warned, true) {
                log::warn!(
                    "{id} navigated {MAX_NAVIGATIONS} times in 10 s; its later navigations are dropped"
                );
            }
            return false;
        }
        let Some(parent) = frame.parent else {
            return false;
        };
        if let FrameTarget::Url(url) = &target
            && url.scheme() == "javascript"
        {
            let url = url.to_string();
            let source = initiator
                .as_ref()
                .map(|i| i.origin.clone())
                .or_else(|| self.frames.get(parent).and_then(|p| p.origin.clone()));
            if source.is_some()
                && self.frames.get(id).and_then(|f| f.origin.as_ref()) == source.as_ref()
            {
                self.run_frame_javascript_url(id, &url);
            }
            return false;
        }
        let target = match target {
            FrameTarget::Url(url) => match self.check_frame_url(id, parent, url) {
                Some(target) => target,
                None => return false,
            },
            target => target,
        };
        self.cancel_frame_navigation(id);
        let source_url = initiator.as_ref().map(|i| i.url.clone());
        let pending = match target {
            FrameTarget::Url(url) => {
                let initiator = initiator
                    .map(|i| i.origin)
                    .or_else(|| self.doc_of(parent).map(DocumentState::origin));
                let site = Some(self.frames.site_for_cookies(parent, None));
                let request = match post {
                    Some(post) => Request::post(
                        url.clone(),
                        post.body.to_vec(),
                        &post.content_type,
                        Destination::IFrame,
                    ),
                    None => Request::get(url.clone(), Destination::IFrame),
                };
                let mut request = request
                    .with_initiator(initiator)
                    .with_site_for_cookies(site);
                // A reservation of 0 bytes always fits; the worker grows it
                // by the body, and a body past the budget fails the request.
                request.reservation = swb_net::Reservation::new(&self.frame_bodies, 0);
                let request = self.loader.start(request);
                self.frames.add_request(request, id);
                log::debug!("{id}: navigating to {url}");
                PendingNavigation::Request { url, request }
            }
            local => {
                self.frame_navigations.push(id);
                PendingNavigation::Local(local)
            }
        };
        let sandbox = self.sandbox_flags_now(id);
        if let Some(frame) = self.frames.get_mut(id) {
            frame.pending = Some(pending);
            frame.pending_source = source_url;
            frame.pending_sandbox = sandbox;
            frame.load_fired = false;
            frame.refresh = None;
        }
        true
    }

    /// A `javascript:` URL in a frame runs as a task in the frame's realm; a
    /// string result does not replace the document (ADR 0031 section 2). An
    /// empty one does nothing and needs no realm (measured, core-js's
    /// `src = "javascript:"`). The caller checked that the document that asks has
    /// the frame's origin (HTML "navigate to a javascript: URL"; measured).
    pub(super) fn run_frame_javascript_url(&mut self, id: FrameId, url: &str) {
        if url.len() <= "javascript:".len()
            || !self
                .frames
                .get(id)
                .is_some_and(|f| f.sandbox.scripts_allowed())
        {
            return;
        }
        self.ensure_frame_realm(id);
        self.run_task_in(id, |script| script.queue_javascript_url(url));
    }

    /// The target of a navigation of frame `id` (child of `parent`) to
    /// `url`, or `None` if the navigation does not happen.
    fn check_frame_url(&self, id: FrameId, parent: FrameId, url: Url) -> Option<FrameTarget> {
        if url.scheme() == "about" {
            // `about:blank` and the other `about:` URLs give an empty
            // document.
            return Some(FrameTarget::Blank);
        }
        if !is_loadable(&url) {
            log::info!("{id}: not navigating to a {}: URL", url.scheme());
            return None;
        }
        if !self.doc_of(parent).is_none_or(|d| d.may_load(&url)) {
            log::warn!("{id}: not allowed to load local resource {url}");
            return None;
        }
        let wanted = strip_fragment(&url);
        let mut same = 0;
        let mut frame = Some(parent);
        while let Some(f) = frame {
            if self
                .doc_of(f)
                .is_some_and(|d| strip_fragment(d.url()) == wanted)
            {
                same += 1;
            }
            frame = self.frames.get(f).and_then(|r| r.parent);
        }
        if same >= 2 {
            log::warn!("{id}: not loading {url}: two ancestor documents have this URL (recursion)");
            return None;
        }
        Some(FrameTarget::Url(url))
    }

    /// Cancels the navigation of frame `id` whose document has not
    /// arrived.
    fn cancel_frame_navigation(&mut self, id: FrameId) {
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        if let Some(PendingNavigation::Request { request, .. }) = frame.pending.take() {
            self.frames.forget_request(request);
            self.loader.cancel(request);
        }
    }

    /// Destroys frame `id` and its descendants (HTML "destroy a child
    /// navigable"): their requests are cancelled and their documents go.
    pub(super) fn destroy_frame(&mut self, id: FrameId) {
        let mut doomed = self.frames.descendants_post_order(id);
        doomed.push(id);
        for frame in doomed {
            self.drop_frame(frame);
        }
    }

    /// Fires the `load` event if the removal of a frame left nothing that
    /// delays it (a removed frame no longer delays its parent's `load`).
    /// Called at the points where the page is not inside a script task.
    pub(super) fn check_load_after_frame_removal(&mut self) {
        if std::mem::take(&mut self.frame_destroyed) {
            self.check_load_event();
        }
    }

    /// Removes one frame record (its children are gone) and cancels its
    /// requests. A document with a realm unloads and stays in the runtime
    /// (ADR 0031 section 5).
    fn drop_frame(&mut self, id: FrameId) {
        if self.frames.get(id).is_some_and(|f| f.script.is_some()) {
            self.run_task_in(id, ScriptRuntime::unload_document);
        }
        self.frame_destroyed = true;
        let parent = self.frames.get(id).and_then(|f| f.parent);
        self.forget_frame_input(id, parent);
        if let Some(PendingNavigation::Request { request, .. }) =
            self.frames.get(id).and_then(|f| f.pending.clone())
        {
            self.loader.cancel(request);
        }
        if let Some(mut frame) = self.frames.remove(id) {
            if let Some(doc) = frame.doc.as_deref_mut() {
                self.loader.cancel_client(doc.client);
                doc.release_own_fonts(&mut self.fonts);
            }
            self.forget_frame_everywhere(id);
            if let Some(script) = frame.script {
                let nodes = frame.doc.as_ref().map_or(0, |d| d.node_count());
                self.retire_in_agent(script, frame.doc.take(), nodes);
            }
            log::debug!("{id}: destroyed");
        }
    }

    /// The document `script` left its frame: it stays in its runtime while the
    /// runtime lives (ADR 0031 section 5).
    pub(super) fn retire_in_agent(
        &mut self,
        script: FrameScript,
        doc: Option<Box<DocumentState>>,
        nodes: usize,
    ) {
        let agent = script.agent();
        self.frames.charge_retired(agent, nodes);
        self.agents.dirty = true;
        let (Some(key), Some(runtime)) = (script.key_in(agent), self.runtime_of_mut(agent)) else {
            return;
        };
        let doc: Option<Box<dyn swb_web::EngineHost>> = match doc {
            Some(doc) => Some(doc),
            None => None,
        };
        runtime.retire_document(key, doc);
    }

    /// The requests of all frames were cancelled ([`swb_net::Loader::cancel_all`]
    /// for a navigation of the page): the frames stop waiting for them.
    pub(super) fn abandon_frame_loads(&mut self) {
        for id in self.frames.tree_order() {
            if id == FrameId::TOP {
                continue;
            }
            if let Some(frame) = self.frames.get_mut(id)
                && let Some(PendingNavigation::Request { request, .. }) = frame.pending.take()
            {
                self.frames.forget_request(request);
            }
            if let Some(doc) = self.doc_of_mut(id) {
                doc.requests = crate::resources::Requests::default();
            }
        }
        self.frame_navigations.clear();
    }

    /// Stops the loads of all frames ([`Page::stop`]).
    pub(super) fn stop_frames(&mut self) {
        self.abandon_frame_loads();
        for id in self.frames.tree_order() {
            if id == FrameId::TOP {
                continue;
            }
            if let Some(frame) = self.frames.get_mut(id)
                && let Some(doc) = frame.doc.as_deref_mut()
            {
                doc.with_own_fonts(&mut self.fonts, DocumentState::stop_loading);
            }
        }
    }

    // ----- Loading -----

    /// Commits the navigations of frames that need no request
    /// (`about:blank`, `about:srcdoc`). True if any committed.
    pub(super) fn run_frame_navigations(&mut self) -> bool {
        if self.lent || self.frame_navigations.is_empty() {
            return false;
        }
        let mut committed = false;
        for id in std::mem::take(&mut self.frame_navigations) {
            let Some(frame) = self.frames.get_mut(id) else {
                continue;
            };
            let Some(PendingNavigation::Local(target)) = frame.pending.take() else {
                continue;
            };
            let Some(parent) = frame.parent else {
                continue;
            };
            let sandbox = frame.pending_sandbox;
            let Some(origin) = self.inherited_origin(parent, sandbox) else {
                continue;
            };
            self.take_pending_sandbox(id);
            let (parsed, url, charge) = if target == FrameTarget::Srcdoc {
                let (parsed, charge) = self.parse_srcdoc(id, &origin);
                (parsed, srcdoc_url(), charge)
            } else {
                let dom = swb_dom::parse_html_with(BLANK, self.javascript);
                (
                    Parsed::complete(dom, "text/html"),
                    about_blank(),
                    Charge::default(),
                )
            };
            self.commit_frame_document(id, parsed, url, origin, Images::default(), charge);
            committed = true;
        }
        if committed {
            // The frame may be the last load that the page waits for.
            self.check_load_event();
        }
        committed
    }

    // ----- Viewports and layout -----

    /// Lays out the frame documents, parents first, each in its viewport:
    /// the content box of its container in the parent's layout (clamped
    /// to [`MAX_VIEWPORT_SIDE`]); a frame whose container has no box keeps
    /// its viewport. A changed viewport or view restyles the document.
    pub(super) fn layout_frames(&mut self) {
        if self.lent || self.frames.count() == 0 {
            return;
        }
        let mut boxes: HashMap<FrameId, HashMap<NodeId, Size>> = HashMap::new();
        for id in self.frames.tree_order().into_iter().skip(1) {
            self.layout_frame(id, &mut boxes);
        }
    }

    /// Lays out frame `id` alone, for a hit test that goes into it: its
    /// parent's layout is current (the hit test used it).
    pub(super) fn layout_frame_alone(&mut self, id: FrameId) {
        if self.lent {
            return;
        }
        self.layout_frame(id, &mut HashMap::new());
    }

    /// Lays out frame `id` in its viewport (see [`Page::layout_frames`]);
    /// `boxes` caches the frame boxes of the parents.
    pub(super) fn layout_frame(
        &mut self,
        id: FrameId,
        boxes: &mut HashMap<FrameId, HashMap<NodeId, Size>>,
    ) {
        let Some(frame) = self.frames.get(id) else {
            return;
        };
        let (Some(parent), Some(container)) = (frame.parent, frame.container) else {
            return;
        };
        let serial = self
            .doc_of(parent)
            .filter(|d| d.fragments().is_some())
            .map(DocumentState::layout_serial);
        let mut viewport = frame.viewport;
        let mut viewport_serial = frame.viewport_serial;
        let mut has_box = frame.has_box;
        if serial.is_some() && serial != frame.viewport_serial {
            let parent_boxes = boxes.entry(parent).or_insert_with(|| {
                self.doc_of(parent)
                    .map(DocumentState::frame_boxes)
                    .unwrap_or_default()
            });
            let size = parent_boxes.get(&container);
            has_box = size.is_some();
            if let Some(size) = size {
                viewport = clamp_viewport(*size);
            }
            viewport_serial = serial;
        }
        let view = self.frame_settings().view(viewport);
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        // The first size that a layout gives is no resize (measured).
        if frame.viewport != viewport && frame.viewport_serial.is_some() {
            frame.resized = true;
        }
        frame.viewport = viewport;
        frame.viewport_serial = viewport_serial;
        frame.has_box = has_box;
        if let Some(doc) = frame.doc.as_deref_mut() {
            doc.set_view(view);
            doc.with_own_fonts(&mut self.fonts, DocumentState::update_layout);
        }
    }

    /// Brings the frame documents up to date for a rendering (ADR 0031
    /// section 4): their layouts, parents first, and for the frames with a
    /// box their display lists and canvases. A list is built only when its
    /// document changed. The frame's canvas is transparent when the used
    /// color schemes of its container and of its root are the same (CSS
    /// Color Adjustment 1 §2.3); else it is opaque in the frame's color.
    pub(super) fn prepare_frames(&mut self) {
        if self.lent || self.frames.count() == 0 {
            return;
        }
        self.layout_frames();
        for id in self.frames.tree_order().into_iter().skip(1) {
            let Some(frame) = self.frames.get(id).filter(|f| f.has_box) else {
                continue;
            };
            let (Some(parent), Some(container)) = (frame.parent, frame.container) else {
                continue;
            };
            let container_scheme = self
                .doc_of_mut(parent)
                .map(|d| d.used_color_scheme(container));
            let Some(frame) = self.frames.get_mut(id) else {
                continue;
            };
            let Some(doc) = frame.doc.as_deref_mut() else {
                continue;
            };
            doc.with_own_fonts(&mut self.fonts, DocumentState::update_display_list);
            let root_scheme = doc.root_color_scheme();
            let color = match root_scheme {
                ColorScheme::Dark => DARK_CANVAS,
                ColorScheme::Light => swb_style::Rgba::WHITE,
            };
            frame.canvas = (container_scheme != Some(root_scheme)).then_some(color);
        }
    }
}

/// The canvas color of a document with the dark color scheme (measured:
/// case `transparency`).
const DARK_CANVAS: swb_style::Rgba = swb_style::Rgba::rgb(18, 18, 18);

/// Clamps a frame's viewport to the accepted viewport sizes.
pub(crate) fn clamp_viewport(size: Size) -> Size {
    // NaN (not possible from layout) becomes 0.
    let side = |v: f32| {
        if v.is_nan() {
            0.0
        } else {
            v.clamp(0.0, MAX_VIEWPORT_SIDE)
        }
    };
    Size::new(side(size.width), side(size.height))
}
