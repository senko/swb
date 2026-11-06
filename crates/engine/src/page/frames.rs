//! The frames of the page (ADR 0031 sections 2 to 4 and 11; M9 feature
//! 10a): the life of a frame, the loading of frame documents, the load
//! state and the viewports. The records are in `crate::frames`.
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
//! A frame document of the page's origin that has a script gets a realm when
//! it commits, and the runtime drives its parse as for the top-level document;
//! one that no script touches costs no realm. With a script runtime, a task
//! makes and destroys frames itself (`crate::frames::registry`); the page does
//! what is left after the task ([`Page::run_task_in`]).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use swb_css::ColorScheme;
use swb_dom::{NodeId, QuirksMode};
use swb_layout::Size;
use swb_net::{Destination, NetError, Origin, Request, Response, Url, escape_html};
use swb_web::{DispatchTarget, DocumentKey, ScriptRuntime, TrustedEvent};

use super::loading::Parsed;
use super::{MAX_VIEWPORT_SIDE, Page, about_blank, is_loadable, strip_fragment};
use crate::document::DocumentState;
use crate::document::frames::iframes_in;
use crate::frames::registry::{BLANK, ChildInit, FrameDocument, FrameSettings, attribute_target};
use crate::frames::{
    Charge, FrameId, FrameTarget, MAX_NAVIGATIONS, MAX_NODES, MAX_SOURCE_BYTES, PendingNavigation,
    Sandbox,
};
use crate::history::PostData;
use crate::resources::{Images, Pending};

impl Page {
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

    /// True if the document of frame `id` has a realm in the page's runtime.
    pub fn frame_has_realm(&self, id: FrameId) -> bool {
        self.script.is_some() && self.frames.get(id).is_some_and(|f| f.script.is_some())
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
        let scripted = self.script.is_some();
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
    fn container_sandbox(&self, parent: FrameId, attr: Option<&str>) -> Sandbox {
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

    /// [`Page::navigate_frame`] with a `POST` body, and the origin of the
    /// document that asks (the parent's if `None`).
    pub(super) fn navigate_frame_with(
        &mut self,
        id: FrameId,
        target: FrameTarget,
        post: Option<PostData>,
        initiator: Option<Origin>,
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
            self.run_frame_javascript_url(id, &url);
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
        let pending = match target {
            FrameTarget::Url(url) => {
                let initiator =
                    initiator.or_else(|| self.doc_of(parent).map(DocumentState::origin));
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
        let sandbox = self.container_flags(id);
        if let Some(frame) = self.frames.get_mut(id) {
            frame.pending = Some(pending);
            frame.pending_sandbox = sandbox;
            frame.load_fired = false;
            frame.refresh = None;
        }
        true
    }

    /// A `javascript:` URL in a frame runs as a task in the frame's realm; a
    /// string result does not replace the document (ADR 0031 section 2). An
    /// empty one does nothing and needs no realm (measured, core-js's
    /// `src = "javascript:"`). A frame of another origin runs nothing until
    /// feature 13.
    pub(super) fn run_frame_javascript_url(&mut self, id: FrameId, url: &str) {
        if url.len() <= "javascript:".len()
            || !self
                .frames
                .get(id)
                .is_some_and(|f| f.sandbox.scripts_allowed())
        {
            return;
        }
        let number = id.number();
        if self.frames.get(id).is_some_and(|f| f.script.is_none()) {
            self.with_registry(|script| script.ensure_frame_window(number));
        }
        if self.frames.in_agent(id) {
            self.run_task_in(id, |script| script.queue_javascript_url(url));
        }
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
            if let Some(script) = self.script.as_mut() {
                script.forget_frame(id.number());
                if let Some(key) = frame.script {
                    let nodes = frame.doc.as_ref().map_or(0, |d| d.node_count());
                    self.frames.charge_retired(nodes);
                    let doc: Option<Box<dyn swb_web::EngineHost>> = match frame.doc.take() {
                        Some(doc) => Some(doc),
                        None => None,
                    };
                    script.retire_document(key, doc);
                }
            }
            log::debug!("{id}: destroyed");
        }
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
            self.commit_sandbox(id);
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

    /// Parses the `srcdoc` attribute of frame `id`'s container, within the
    /// budget for frame documents (an error document otherwise).
    fn parse_srcdoc(&mut self, id: FrameId, origin: &Origin) -> (Parsed, Charge) {
        let markup = self.srcdoc_of(id).map(str::to_owned);
        let markup = markup.as_deref().unwrap_or_default();
        if !self.admit_bytes(id, markup.len()) {
            return (self.over_budget_document(), Charge::default());
        }
        // The parse stops one node past the room that the budget has left.
        let max_nodes = self.frames.nodes_left(id).saturating_add(1);
        if self.frame_runs_scripts(id, origin) && may_have_script(markup.as_bytes(), None) {
            let mut dom = swb_dom::Document::new();
            dom.set_max_nodes(max_nodes);
            let parser = swb_dom::HtmlParser::new(&mut dom, markup, true);
            let mut parsed = Parsed::complete(dom, "text/html");
            parsed.parser = Some(parser);
            return (
                parsed,
                crate::frames::registry::scripted_charge(markup.len()),
            );
        }
        let mut dom = swb_dom::parse_html_limited(markup, self.javascript, max_nodes);
        // An iframe srcdoc document is never in quirks mode (HTML "the
        // initial insertion mode").
        dom.quirks_mode = QuirksMode::NoQuirks;
        let parsed = Parsed::complete(dom, "text/html");
        self.admit_parsed(id, parsed, markup.len(), max_nodes)
    }

    /// The `srcdoc` attribute of frame `id`'s container.
    fn srcdoc_of(&self, id: FrameId) -> Option<&str> {
        let frame = self.frames.get(id)?;
        let doc = self.doc_of(frame.parent?)?;
        doc.dom().element(frame.container?)?.attr("srcdoc")
    }

    /// The sandboxing flags of frame `id`'s container now.
    fn container_flags(&self, id: FrameId) -> Sandbox {
        let frame = self.frames.get(id);
        let parent = frame.and_then(|f| f.parent);
        let element = frame
            .and_then(|f| Some((f.parent?, f.container?)))
            .and_then(|(p, c)| self.doc_of(p)?.dom().element(c));
        match parent {
            Some(parent) => self.container_sandbox(parent, element.and_then(|e| e.attr("sandbox"))),
            None => Sandbox::default(),
        }
    }

    /// The document of frame `id`'s navigation is here: it has the flags of
    /// the navigation.
    fn commit_sandbox(&mut self, id: FrameId) -> Sandbox {
        let Some(frame) = self.frames.get_mut(id) else {
            return Sandbox::default();
        };
        frame.sandbox = frame.pending_sandbox;
        frame.sandbox
    }

    /// The document of frame `id`'s navigation arrived (or failed).
    pub(super) fn frame_document_arrived(
        &mut self,
        id: FrameId,
        result: Result<Response, NetError>,
    ) {
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        let url = match frame.pending.take() {
            Some(PendingNavigation::Request { url, .. }) => url,
            other => {
                frame.pending = other;
                return;
            }
        };
        let sandbox = self.commit_sandbox(id);
        let opaque = |o: Origin| {
            if sandbox.opaque_origin() {
                Origin::new_opaque()
            } else {
                o
            }
        };
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                log::warn!("{id}: cannot load {url}: {error}");
                let parsed =
                    error_document(&format!("Cannot load {url}: {error}"), self.javascript);
                self.commit_frame_document(
                    id,
                    parsed,
                    url,
                    Origin::new_opaque(),
                    Images::default(),
                    Charge::default(),
                );
                return;
            }
        };
        if !response.is_success() {
            log::debug!("{id}: {}: HTTP {}", response.url, response.status);
        }
        let origin = opaque(response.url.origin());
        let size = response.body.len();
        if !self.admit_bytes(id, size) {
            let parsed = self.over_budget_document();
            let none = Charge::default();
            self.commit_frame_document(id, parsed, response.url, origin, Images::default(), none);
            return;
        }
        // The parse stops one node past the room that the budget has left,
        // so that a document that does not fit costs little.
        let max_nodes = self.frames.nodes_left(id).saturating_add(1);
        let charset = response.content_type().and_then(|c| c.charset);
        let steps = self.frame_runs_scripts(id, &origin)
            && may_have_script(&response.body, charset.as_deref());
        match self.parse_response(&response, steps, max_nodes) {
            Ok((parsed, images)) if parsed.parser.is_some() => {
                let charge = crate::frames::registry::scripted_charge(size);
                self.commit_frame_document(id, parsed, response.url, origin, images, charge);
            }
            Ok((parsed, images)) => {
                let (parsed, charge) = self.admit_parsed(id, parsed, size, max_nodes);
                // An over-budget document has no images either.
                let images = if charge == Charge::default() {
                    Images::default()
                } else {
                    images
                };
                self.commit_frame_document(id, parsed, response.url, origin, images, charge);
            }
            Err(essence) => {
                let parsed = error_document(
                    &format!("Cannot display content of type {essence}"),
                    self.javascript,
                );
                let none = Charge::default();
                self.commit_frame_document(
                    id,
                    parsed,
                    response.url,
                    origin,
                    Images::default(),
                    none,
                );
            }
        }
    }

    /// True if a document of `len` bytes of source fits the page's budget
    /// for frame documents when it replaces frame `id`'s (ADR 0031 section
    /// 11). One warning per top-level document otherwise.
    fn admit_bytes(&mut self, id: FrameId, len: usize) -> bool {
        self.frames.fits_bytes(id, len) || self.warn_over_budget()
    }

    /// Admits the parsed document of `bytes` of source if its nodes fit the
    /// budget, else gives an error document. A parse that reached
    /// `max_nodes` stopped early, and the page must not show a part of the
    /// document, even if the part would fit. Returns the document and what it
    /// costs.
    fn admit_parsed(
        &mut self,
        id: FrameId,
        parsed: Parsed,
        bytes: usize,
        max_nodes: usize,
    ) -> (Parsed, Charge) {
        let nodes = parsed.dom.len();
        if nodes < max_nodes && self.frames.fits_nodes(id, nodes) {
            return (parsed, Charge { bytes, nodes });
        }
        self.frames.budget.note_rejection();
        self.warn_over_budget();
        (self.over_budget_document(), Charge::default())
    }

    /// Warns once per top-level document that a frame document did not fit
    /// the budget. Returns false.
    fn warn_over_budget(&mut self) -> bool {
        if !std::mem::replace(&mut self.frames.budget.warned, true) {
            log::warn!(
                "the frame documents of the page have {} MiB of source or {} million nodes, the limit; later ones show an error",
                MAX_SOURCE_BYTES >> 20,
                MAX_NODES >> 20
            );
        }
        false
    }

    /// The document of a frame that did not fit the budget.
    fn over_budget_document(&self) -> Parsed {
        error_document("Too many frame documents on this page", self.javascript)
    }

    /// True if a document of origin `origin` in frame `id` may run scripts:
    /// the page has a runtime, the document has the runtime's origin, and
    /// its sandbox has `allow-scripts`.
    fn frame_runs_scripts(&self, id: FrameId, origin: &Origin) -> bool {
        self.javascript
            && self.script.is_some()
            && self.frames.top_origin() == Some(origin)
            && self
                .frames
                .get(id)
                .is_some_and(|f| f.sandbox.scripts_allowed())
    }

    /// Replaces the document of frame `id` with a new one: the old one
    /// unloads and goes with its frames and requests. `charge` is what the
    /// document costs the page's budget for frame documents.
    fn commit_frame_document(
        &mut self,
        id: FrameId,
        parsed: Parsed,
        url: Url,
        origin: Origin,
        images: Images,
        charge: Charge,
    ) {
        if self.frames.get(id).is_some_and(|f| f.script.is_some()) {
            self.run_task_in(id, ScriptRuntime::unload_document);
        }
        let Some(frame) = self.frames.get(id) else {
            return;
        };
        let Some(parent) = frame.parent else {
            return;
        };
        let viewport = frame.viewport;
        for child in frame.children.clone() {
            self.destroy_frame(child);
        }
        let creator = (url.scheme() == "about")
            .then(|| self.doc_of(parent))
            .flatten();
        let fallback_base = creator.map(|d| d.base_url().clone());
        let same_origin = self
            .frames
            .get(parent)
            .is_some_and(|p| p.origin.as_ref() == Some(&origin));
        let creator_url = creator
            .filter(|_| same_origin)
            .map(|d| d.cookie_url().clone());
        let history = self
            .doc
            .as_deref()
            .map(|d| d.history.clone())
            .unwrap_or_default();
        let mut doc = self.maker.make(FrameDocument {
            dom: parsed.dom,
            parser: parsed.parser,
            encoding: parsed.encoding,
            content_type: parsed.content_type,
            parse: parsed.parse,
            url,
            origin: origin.clone(),
            site_for_cookies: Some(self.frames.site_for_cookies(parent, Some(&origin))),
            view: self.frame_settings().view(viewport),
            images,
            fallback_base,
            creator_url,
            history,
            node_room: Rc::clone(&self.frames.node_room),
        });
        doc.frame_id = id;
        let refresh = doc
            .declarative_refresh()
            .map(|r| (self.clock.now() + r.delay_s * 1000.0, r.url));
        let client = doc.client;
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        let mut old = frame.doc.replace(doc);
        let old_key = frame.script.take();
        let reuse = frame.reuse_window && frame.origin.as_ref() == Some(&origin);
        frame.reuse_window = false;
        frame.origin = Some(origin);
        // The new document has its own pointer and keyboard state; the
        // focus stays with the frame (measured: a navigated frame that
        // held the focus keeps it, with no focused element in it).
        self.frame_inputs.remove(&id);
        if self
            .input
            .press_frame
            .as_ref()
            .is_some_and(|p| p.frame == id)
        {
            self.input.press_frame = None;
        }
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        frame.load_fired = false;
        frame.refresh = refresh;
        frame.canvas = None;
        let old_client = old.as_ref().map(|d| d.client);
        if let Some(old) = old_client {
            self.loader.cancel_client(old);
        }
        if let Some(old) = old.as_deref_mut() {
            old.release_own_fonts(&mut self.fonts);
        }
        self.frames.set_client(id, old_client, client);
        self.frames.set_charge(id, charge);
        let kept = self.retire_old_document(old, old_key, reuse);
        log::debug!(
            "{id}: committed {}",
            self.doc_of(id).map_or("", |d| d.url().as_str())
        );
        self.start_frame_document(id, kept);
        self.update_frames(id);
    }

    /// The document that a commit replaced: with a realm it stays in the
    /// runtime (ADR 0031 section 5). With `reuse`, the new document takes over
    /// its `Window` (HTML "initial about:blank"; measured, case
    /// `scripted-initial-reuse`); returns its key.
    fn retire_old_document(
        &mut self,
        old: Option<Box<DocumentState>>,
        key: Option<DocumentKey>,
        reuse: bool,
    ) -> Option<DocumentKey> {
        let (Some(key), Some(script)) = (key, self.script.as_mut()) else {
            return None;
        };
        let kept = if reuse {
            script.reuse_window(key)
        } else {
            None
        };
        let nodes = old.as_ref().map_or(0, |d| d.node_count());
        let old: Option<Box<dyn swb_web::EngineHost>> = match old {
            Some(old) => Some(old),
            None => None,
        };
        script.retire_document(key, old);
        self.frames.charge_retired(nodes);
        kept
    }

    /// A new document of frame `id` starts. With a script to run it gets a
    /// realm, and the runtime drives its parse. A script may hold the frame's
    /// proxy, so the proxy needs a new target: a realm for a document of the
    /// agent, none for another (ADR 0031 section 5). Else the document needs
    /// no realm yet.
    fn start_frame_document(&mut self, id: FrameId, kept: Option<DocumentKey>) {
        let number = id.number();
        let parsing = self.doc_of(id).is_some_and(DocumentState::is_parsing);
        let Some(script) = self.script.as_ref() else {
            self.finish_frame_parse(id);
            return;
        };
        let known = script.knows_frame(number);
        let agent = self.frames.in_agent(id);
        let key = if kept.is_some() {
            kept
        } else if parsing || (known && agent) {
            self.with_registry(|s| s.add_frame_document(number))
                .flatten()
        } else {
            None
        };
        let scripts_off = !self
            .frames
            .get(id)
            .is_some_and(|f| f.sandbox.scripts_allowed());
        if let Some(frame) = self.frames.get_mut(id) {
            frame.script = key;
        }
        if let (Some(key), Some(script)) = (key, self.script.as_mut()) {
            script.set_scripts_off(key, scripts_off);
        }
        match key {
            Some(_) if parsing => {
                self.run_task_in(id, ScriptRuntime::start_document);
            }
            Some(key) => {
                // Its `load` is to come: the window that a script holds gets
                // it (measured, case `scripted-initial-reuse`).
                if let Some(script) = self.script.as_mut() {
                    script.adopt_complete_document(key, true);
                }
            }
            None => {
                self.finish_frame_parse(id);
                if known && let Some(script) = self.script.as_mut() {
                    script.clear_frame_target(number);
                }
            }
        }
    }

    /// A parse in steps of frame `id`'s document goes to its end without
    /// scripts; the document is charged for its nodes.
    fn finish_frame_parse(&mut self, id: FrameId) {
        let Some(doc) = self.doc_of_mut(id) else {
            return;
        };
        if !doc.is_parsing() {
            return;
        }
        doc.finish_parse_without_scripts();
        self.update_frame_charge(id);
        self.update_frames(id);
    }

    /// Charges the nodes that frame `id`'s document has now (a document that
    /// a script or a parse in steps grew).
    pub(super) fn update_frame_charge(&mut self, id: FrameId) {
        let Some(frame) = self.frames.get(id) else {
            return;
        };
        let Some(doc) = frame.doc.as_deref() else {
            return;
        };
        let charge = Charge {
            bytes: frame.charge.bytes,
            nodes: doc.node_count(),
        };
        if charge != frame.charge {
            self.frames.set_charge(id, charge);
        }
    }

    /// Hands a completion of a request of frame `id`'s document to it. Its
    /// scripts and events follow, as for the top-level document.
    pub(super) fn frame_completion(
        &mut self,
        id: FrameId,
        completion: swb_net::Completion,
    ) -> bool {
        let Some(frame) = self.frames.get_mut(id) else {
            return false;
        };
        let Some(doc) = frame.doc.as_deref_mut() else {
            return false;
        };
        if doc.is_api_fetch(completion.id) {
            doc.api_fetch_completed(
                completion.id,
                completion.result,
                completion.kind,
                completion.reservation,
            );
            self.deliver_api_fetches_in(id);
            return true;
        }
        let Some(pending) = doc.requests.pending.remove(&completion.id) else {
            return false;
        };
        let sheet = matches!(pending, Pending::Stylesheet { .. });
        doc.with_own_fonts(&mut self.fonts, |doc, fonts| {
            doc.handle_completion(pending, completion.result, fonts);
        });
        self.deliver_element_events_in(id);
        self.deliver_scripts_in(id, sheet);
        true
    }

    // ----- The declarative refresh of frame documents -----

    /// The page time of the next refresh of a frame document.
    pub(super) fn next_frame_refresh(&self) -> Option<f64> {
        if self.frames.count() == 0 {
            return None;
        }
        self.frames
            .tree_order()
            .into_iter()
            .filter_map(|id| self.frames.get(id)?.refresh.as_ref().map(|(t, _)| *t))
            .reduce(f64::min)
    }

    /// Navigates the frames whose refresh is due. True if any did.
    pub(super) fn run_frame_refreshes(&mut self) -> bool {
        if self.lent || self.frames.count() == 0 {
            return false;
        }
        let now = self.clock.now();
        let due: Vec<(FrameId, Url)> = self
            .frames
            .tree_order()
            .into_iter()
            .filter_map(|id| {
                let frame = self.frames.get_mut(id)?;
                if frame.refresh.as_ref().is_some_and(|(t, _)| *t <= now) {
                    frame.refresh.take().map(|(_, url)| (id, url))
                } else {
                    None
                }
            })
            .collect();
        let any = !due.is_empty();
        for (id, url) in due {
            log::debug!("{id}: refresh to {url}");
            self.navigate_frame(id, FrameTarget::Url(url));
        }
        any
    }

    // ----- Load state -----

    /// True if frame `id` has loaded: its `load` fired, or its document
    /// and its frames have loaded (without a check of its layout).
    fn frame_settled(&self, id: FrameId) -> bool {
        let Some(frame) = self.frames.get(id) else {
            return true;
        };
        frame.load_fired
            || (frame.pending.is_none()
                && !self.doc_of(id).is_some_and(DocumentState::is_loading)
                && frame.children.iter().all(|&c| self.frame_settled(c)))
    }

    /// True while frames delay the page's load (JavaScript off, or before
    /// the `load` event with JavaScript on).
    pub(super) fn frames_loading(&self) -> bool {
        let top = self.frames.top();
        !top.load_fired && top.children.iter().any(|&c| !self.frame_settled(c))
    }

    /// True if a child frame has not fired its container's `load`: the
    /// page's `load` event waits.
    pub(super) fn frames_delay_load(&self) -> bool {
        let top = self.frames.top();
        !top.load_fired
            && top
                .children
                .iter()
                .any(|&c| self.frames.get(c).is_some_and(|f| !f.load_fired))
    }

    /// True if any frame loads (requests, a navigation): the loops of the
    /// event loop do not jump in virtual time meanwhile.
    pub(super) fn frames_busy(&self) -> bool {
        !self.frame_navigations.is_empty()
            || self
                .frames
                .tree_order()
                .into_iter()
                .skip(1)
                .any(|id| self.frame_is_loading(id))
    }

    /// Marks the frames whose document and frames loaded, children first,
    /// and fires the `load` event at their containers (HTML "iframe load
    /// event steps"): as a task of the parent's document when it has a
    /// realm. A document that the runtime drives fires its own `load` first;
    /// that task also fires the container's. True if any frame loaded.
    pub(super) fn check_frame_loads(&mut self) -> bool {
        if self.lent || self.frames.count() == 0 {
            return false;
        }
        let mut any = false;
        for id in self.frames.descendants_post_order(FrameId::TOP) {
            let Some(frame) = self.frames.get(id) else {
                continue;
            };
            if frame.load_fired
                || frame.pending.is_some()
                || self.doc_of(id).is_none_or(DocumentState::is_loading)
                || frame
                    .children
                    .iter()
                    .any(|&c| self.frames.get(c).is_some_and(|f| !f.load_fired))
            {
                continue;
            }
            if let (Some(key), Some(script)) = (frame.script, self.script.as_ref()) {
                if script.load_event_due_in(key) {
                    // Layout can find more resources (ADR 0031 section 2).
                    self.layout_frame_alone(id);
                    if self.doc_of(id).is_none_or(DocumentState::is_loading) {
                        continue;
                    }
                    self.run_task_in(id, ScriptRuntime::fire_load_event);
                    any = true;
                    self.finish_load_after_limit(id, key);
                    continue;
                }
                if !script.document_loaded_in(key) {
                    continue;
                }
            }
            any = true;
            self.mark_frame_loaded(id);
        }
        any
    }

    /// A limit can end the task of the `load` event of frame `id`'s document
    /// before the container's `load`, which marks the frame. No later event has
    /// to come, and the page's `load` waits for the frame, so the frame is done
    /// here. A listener that navigated the frame left a pending navigation or
    /// another document.
    fn finish_load_after_limit(&mut self, id: FrameId, key: DocumentKey) {
        let waiting = self
            .frames
            .get(id)
            .is_some_and(|f| !f.load_fired && f.pending.is_none() && f.script == Some(key));
        if waiting
            && self
                .script
                .as_ref()
                .is_some_and(|s| s.document_loaded_in(key))
        {
            self.mark_frame_loaded(id);
        }
    }

    /// Frame `id` has loaded: its container gets `load`.
    fn mark_frame_loaded(&mut self, id: FrameId) {
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        frame.load_fired = true;
        frame.reuse_window = false;
        let (parent, container) = (frame.parent, frame.container);
        if let (Some(parent), Some(container)) = (parent, container) {
            self.fire_container_load(id, parent, container);
        }
    }

    /// Fires `load` at the container of frame `id`, if its parent's document
    /// has a realm. A listener of an earlier `load` may have removed the
    /// container, or removed and inserted it again (a new frame, which has
    /// not loaded).
    fn fire_container_load(&mut self, id: FrameId, parent: FrameId, container: NodeId) {
        if self.frames.by_container(parent, container) != Some(id) {
            return;
        }
        let has_realm =
            parent == FrameId::TOP || self.frames.get(parent).is_some_and(|f| f.script.is_some());
        if self.script.is_some() && has_realm {
            let event = TrustedEvent::new("load");
            self.run_task_in(parent, |script| {
                script.dispatch_event(DispatchTarget::Node(container), &event)
            });
        }
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

/// True if the source of a document can have a script to run: a `script`
/// element, or an attribute `on...` with a value. A false positive costs a realm.
/// A document counts as a positive where the bytes do not tell: a byte order
/// mark or a `charset` (of the HTTP header) that names an encoding that is not
/// compatible with ASCII, and an escape byte, which can hide markup in
/// ISO-2022-JP (a `meta` can name that encoding; measured).
fn may_have_script(bytes: &[u8], charset: Option<&str>) -> bool {
    let wide = charset
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .is_some_and(|encoding| !encoding.is_ascii_compatible());
    if wide
        || bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
        || bytes.contains(&0x1B)
    {
        return true;
    }
    let lower = |b: &u8| b.to_ascii_lowercase();
    bytes
        .windows(7)
        .any(|w| w[0] == b'<' && w[1..].iter().map(lower).eq(b"script".iter().copied()))
        // A link or a form that runs a script when the user activates it.
        || bytes
            .windows(11)
            .any(|w| w.eq_ignore_ascii_case(b"javascript:"))
        || bytes.iter().enumerate().any(|(index, &byte)| {
            if byte != b'=' {
                return false;
            }
            let head = bytes[..index].trim_ascii_end();
            let letters = head
                .iter()
                .rev()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            letters >= 3 && {
                let name = &head[head.len() - letters..];
                name[..2].eq_ignore_ascii_case(b"on")
            }
        })
}

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

/// The URL of an `about:srcdoc` document.
fn srcdoc_url() -> Url {
    Url::parse("about:srcdoc").expect("about:srcdoc is a valid URL")
}

/// A frame document that shows a load error.
fn error_document(message: &str, scripting: bool) -> Parsed {
    let html = format!(
        "<!DOCTYPE html><title>Error</title><body style=\"font-family:sans-serif\"><p>{}</p>",
        escape_html(message)
    );
    Parsed::complete(swb_dom::parse_html_with(&html, scripting), "text/html")
}

#[cfg(test)]
mod tests {
    use super::may_have_script;

    #[test]
    fn the_source_scan_finds_each_form_of_a_script() {
        for source in [
            "<script>",
            "<SCRIPT src=a>",
            "<svg><script>",
            "<body onload=a>",
            "<body\nONLOAD\n=\na>",
            "<img/onerror=a>",
        ] {
            assert!(may_have_script(source.as_bytes(), None), "{source}");
        }
        for source in ["", "<p>on</p>", "<a href=b>", "<a on=b>", "<scrip>"] {
            assert!(!may_have_script(source.as_bytes(), None), "{source}");
        }
    }

    #[test]
    fn the_source_scan_cannot_read_a_wide_encoding() {
        let wide: Vec<u8> = "<script>".bytes().flat_map(|b| [b, 0]).collect();
        assert!(!may_have_script(&wide, None));
        assert!(!may_have_script(&wide, Some("utf-8")));
        assert!(may_have_script(&wide, Some("utf-16le")));
        assert!(may_have_script(&[0xFF, 0xFE, b'x', 0], None));
        assert!(may_have_script(&[0xFE, 0xFF, 0, b'x'], None));
        assert!(may_have_script(b"<scr\x1b(Bipt>", None));
    }
}
