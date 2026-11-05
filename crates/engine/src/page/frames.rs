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
//! Frame documents run no scripts yet (with JavaScript on they parse with
//! the scripting flag set).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use swb_css::ColorScheme;
use swb_dom::{NodeId, QuirksMode};
use swb_layout::Size;
use swb_net::{Destination, NetError, Origin, Request, Response, Url, escape_html, same_site};
use swb_web::{DispatchTarget, TrustedEvent};

use super::loading::Parsed;
use super::{MAX_VIEWPORT_SIDE, Page, about_blank, is_loadable, strip_fragment};
use crate::document::frames::iframes_in;
use crate::document::{DocumentInit, DocumentState, ScrollTarget, View};
use crate::frames::{
    Charge, FrameId, FrameTarget, MAX_DEPTH, MAX_FRAMES, MAX_NAVIGATIONS, MAX_NODES,
    MAX_SOURCE_BYTES, PendingNavigation, Sandbox,
};
use crate::resources::Images;

/// The markup of an initial `about:blank` document (measured:
/// `<html><head></head><body></body></html>`, quirks mode).
const BLANK: &str = "";

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

    fn doc_of_mut(&mut self, id: FrameId) -> Option<&mut DocumentState> {
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
        for node in changes.removed {
            if let Some(child) = self.frames.by_container(frame, node) {
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
        for node in changes.names {
            if let Some(child) = self.frames.by_container(frame, node) {
                self.update_frame_name(child);
            }
        }
        for node in changes.attributes {
            let Some(child) = self.frames.by_container(frame, node) else {
                continue;
            };
            if made.contains(&child) || !self.in_frame_tree(frame, node) {
                continue;
            }
            self.process_frame_attributes(child, false);
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
        if self.frames.count() >= MAX_FRAMES {
            if !std::mem::replace(&mut self.frames.count_warned, true) {
                log::warn!(
                    "the page has {MAX_FRAMES} frames, the limit; further iframe elements get no frame"
                );
            }
            return None;
        }
        let depth = self.frames.get(parent)?.depth + 1;
        if depth > MAX_DEPTH {
            if !std::mem::replace(&mut self.frames.depth_warned, true) {
                log::warn!(
                    "frames are nested {MAX_DEPTH} deep, the limit; a deeper iframe gets no frame"
                );
            }
            return None;
        }
        let parent_doc = self.doc_of(parent)?;
        let element = parent_doc.dom().element(container)?;
        let name = element.attr("name").unwrap_or_default().to_owned();
        let sandbox = self.container_sandbox(parent, element.attr("sandbox"));
        let origin = self.inherited_origin(parent, sandbox)?;
        let site = Some(self.site_for_cookies(parent, Some(&origin)));
        let view = self.frame_view(crate::frames::DEFAULT_FRAME_VIEWPORT);
        let parsed = Parsed::complete(
            swb_dom::parse_html_with(BLANK, self.javascript),
            "text/html",
        );
        let doc = self.new_frame_document(
            parent,
            parsed,
            about_blank(),
            origin,
            site,
            view,
            Images::default(),
        );
        let at = {
            let dom = self.doc_of(parent)?.dom();
            self.frames
                .child_position(parent, container, |a, b| dom.precedes(a, b))
        };
        let id = self.frames.insert(parent, container, doc, at)?;
        if let Some(frame) = self.frames.get_mut(id) {
            frame.name = name;
            frame.sandbox = sandbox;
        }
        log::debug!("{id}: made for an iframe of {parent}");
        self.process_frame_attributes(id, true);
        Some(id)
    }

    /// The origin of an `about:blank` or `about:srcdoc` document in a
    /// child of `parent`: the parent document's, or an opaque one in a
    /// sandbox without `allow-same-origin`.
    fn inherited_origin(&self, parent: FrameId, sandbox: Sandbox) -> Option<Origin> {
        if sandbox.opaque_origin() {
            return Some(Origin::new_opaque());
        }
        Some(self.doc_of(parent)?.origin())
    }

    /// The site for cookies of the requests of a document of origin
    /// `own` in a child of `parent` (RFC 6265bis §5.2.1; ADR 0031 section
    /// 3), or of the request of a frame document of `parent` (`own` is
    /// `None`): the top-level origin if `own` and the documents of
    /// `parent` and its ancestors are same-site with it, else an opaque
    /// origin.
    fn site_for_cookies(&self, parent: FrameId, own: Option<&Origin>) -> Origin {
        let Some(top) = self.doc.as_ref().map(|d| d.origin()) else {
            return Origin::new_opaque();
        };
        let mut same = own.is_none_or(|o| same_site(o, &top));
        let mut frame = Some(parent);
        while same && let Some(id) = frame {
            if id == FrameId::TOP {
                break;
            }
            same = self
                .doc_of(id)
                .is_some_and(|d| same_site(&d.origin(), &top));
            frame = self.frames.get(id).and_then(|f| f.parent);
        }
        if same { top } else { Origin::new_opaque() }
    }

    /// The view of a frame document with `viewport`: the page's scale and
    /// color scheme, no scroll indicators.
    fn frame_view(&self, viewport: Size) -> View {
        View {
            viewport,
            scale: self.view.scale,
            indicators: false,
            color_scheme: self.view.color_scheme,
        }
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

    /// A new frame document of a child of `parent` (the parse is complete:
    /// frame documents run no scripts in feature 10a). An `about:` document
    /// resolves relative URLs against the parent's base URL (HTML
    /// "fallback base URL"; `about:srcdoc`: the container document's, which
    /// is the parent's).
    #[allow(clippy::too_many_arguments)]
    fn new_frame_document(
        &self,
        parent: FrameId,
        parsed: Parsed,
        url: Url,
        origin: Origin,
        site_for_cookies: Option<Origin>,
        view: View,
        images: Images,
    ) -> Box<DocumentState> {
        let scroll_target = url.fragment().map(|f| ScrollTarget::Fragment(f.to_owned()));
        let mut images = images;
        images.attach_budget(
            Arc::clone(&self.frame_images),
            Arc::clone(&self.frame_vectors),
        );
        let fallback_base = (url.scheme() == "about")
            .then(|| self.doc_of(parent).map(|d| d.base_url().clone()))
            .flatten();
        Box::new(DocumentState::new(DocumentInit {
            dom: parsed.dom,
            parser: None,
            url,
            encoding: parsed.encoding,
            content_type: parsed.content_type,
            images,
            loader: Rc::clone(&self.loader),
            storage: Rc::clone(&self.storage),
            view,
            scroll_target,
            parse: parsed.parse,
            client: self.loader.new_client(),
            origin: Some(origin),
            site_for_cookies,
            frame: true,
            fallback_base,
            style_budget: Some(Arc::clone(&self.frame_styles)),
            body_budget: Some(Arc::clone(&self.frame_bodies)),
            font_budget: Some(Arc::clone(&self.frame_fonts)),
        }))
    }

    /// HTML "process the iframe attributes" for frame `id`; `initial` at
    /// its insertion.
    fn process_frame_attributes(&mut self, id: FrameId, initial: bool) {
        let Some(target) = self.attribute_target(id) else {
            return;
        };
        match target {
            // HTML: `about:blank` at the insertion runs the iframe load
            // event steps; the initial document stays.
            FrameTarget::Blank if initial => {}
            target => self.navigate_frame(id, target),
        }
    }

    /// Copies the `name` of frame `id`'s container to the frame.
    fn update_frame_name(&mut self, id: FrameId) {
        let element = self
            .frames
            .get(id)
            .and_then(|f| Some((f.parent?, f.container?)))
            .and_then(|(p, c)| self.doc_of(p)?.dom().element(c));
        let name = element
            .and_then(|e| e.attr("name"))
            .unwrap_or_default()
            .to_owned();
        if let Some(frame) = self.frames.get_mut(id) {
            frame.name = name;
        }
    }

    /// What the attributes of frame `id`'s container ask for: `srcdoc`,
    /// else the URL of `src` (resolved against the parent's base URL),
    /// else `about:blank`. `None` if `src` does not parse (nothing
    /// happens).
    fn attribute_target(&self, id: FrameId) -> Option<FrameTarget> {
        let frame = self.frames.get(id)?;
        let (parent, container) = (frame.parent?, frame.container?);
        let doc = self.doc_of(parent)?;
        let element = doc.dom().element(container)?;
        if element.has_attr("srcdoc") {
            return Some(FrameTarget::Srcdoc);
        }
        let src = element
            .attr("src")
            .map(|s| s.trim_matches(swb_dom::is_html_whitespace))
            .filter(|s| !s.is_empty());
        let Some(src) = src else {
            return Some(FrameTarget::Blank);
        };
        match doc.base_url().join(src) {
            Ok(url) if url.as_str() == "about:blank" => Some(FrameTarget::Blank),
            Ok(url) => Some(FrameTarget::Url(url)),
            Err(_) => {
                log::debug!("{id}: the src {src:?} does not parse");
                None
            }
        }
    }

    /// Navigates frame `id` to `target` (ADR 0031 section 2). Dropped past
    /// [`MAX_NAVIGATIONS`] in the window of page time (one warning per
    /// frame), for URLs that cannot be loaded (`javascript:` comes with
    /// scripts in frames), for `file:` URLs from a document that is not a
    /// `file:` one, and when two or more ancestors' documents have the URL
    /// (Chromium's rule, case `self-embed`).
    fn navigate_frame(&mut self, id: FrameId, target: FrameTarget) {
        let now = self.clock.now();
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        if !frame.note_navigation(now) {
            if !std::mem::replace(&mut frame.navigation_warned, true) {
                log::warn!(
                    "{id} navigated {MAX_NAVIGATIONS} times in 10 s; its later navigations are dropped"
                );
            }
            return;
        }
        let Some(parent) = frame.parent else {
            return;
        };
        let target = match target {
            FrameTarget::Url(url) => match self.check_frame_url(id, parent, url) {
                Some(target) => target,
                None => return,
            },
            target => target,
        };
        self.cancel_frame_navigation(id);
        let pending = match target {
            FrameTarget::Url(url) => {
                let initiator = self.doc_of(parent).map(DocumentState::origin);
                let site = Some(self.site_for_cookies(parent, None));
                let mut request = Request::get(url.clone(), Destination::IFrame)
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
        if let Some(frame) = self.frames.get_mut(id) {
            frame.pending = Some(pending);
            frame.load_fired = false;
            frame.refresh = None;
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
        if !self.doc_of(parent).is_some_and(|d| d.may_load(&url)) {
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
    /// requests.
    fn drop_frame(&mut self, id: FrameId) {
        self.frame_destroyed = true;
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
            let sandbox = self.current_sandbox(id);
            let Some(origin) = self.inherited_origin(parent, sandbox) else {
                continue;
            };
            let (parsed, url, charge) = if target == FrameTarget::Srcdoc {
                let (parsed, charge) = self.parse_srcdoc(id);
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
    fn parse_srcdoc(&mut self, id: FrameId) -> (Parsed, Charge) {
        let markup = self.srcdoc_of(id).map(str::to_owned);
        let markup = markup.as_deref().unwrap_or_default();
        if !self.admit_bytes(id, markup.len()) {
            return (self.over_budget_document(), Charge::default());
        }
        // The parse stops one node past the room that the budget has left.
        let max_nodes = self.frames.nodes_left(id).saturating_add(1);
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

    /// The sandboxing flags of frame `id`'s container now (they apply to
    /// the document that the navigation makes).
    fn current_sandbox(&mut self, id: FrameId) -> Sandbox {
        let frame = self.frames.get(id);
        let parent = frame.and_then(|f| f.parent);
        let element = frame
            .and_then(|f| Some((f.parent?, f.container?)))
            .and_then(|(p, c)| self.doc_of(p)?.dom().element(c));
        let sandbox = match parent {
            Some(parent) => self.container_sandbox(parent, element.and_then(|e| e.attr("sandbox"))),
            None => Sandbox::default(),
        };
        if let Some(frame) = self.frames.get_mut(id) {
            frame.sandbox = sandbox;
        }
        sandbox
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
        let sandbox = self.current_sandbox(id);
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
        match self.parse_response(&response, false, max_nodes) {
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

    /// Replaces the document of frame `id` with a new one: the old one
    /// goes with its frames and requests. `charge` is what the document
    /// costs the page's budget for frame documents.
    fn commit_frame_document(
        &mut self,
        id: FrameId,
        parsed: Parsed,
        url: Url,
        origin: Origin,
        images: Images,
        charge: Charge,
    ) {
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
        let site = Some(self.site_for_cookies(parent, Some(&origin)));
        let view = self.frame_view(viewport);
        let doc = self.new_frame_document(parent, parsed, url, origin, site, view, images);
        let refresh = doc
            .declarative_refresh()
            .map(|r| (self.clock.now() + r.delay_s * 1000.0, r.url));
        let client = doc.client;
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        let mut old = frame.doc.replace(doc);
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
        log::debug!(
            "{id}: committed {}",
            self.doc_of(id).map_or("", |d| d.url().as_str())
        );
        self.update_frames(id);
    }

    /// Hands a completion of a request of frame `id`'s document to it.
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
        let Some(pending) = doc.requests.pending.remove(&completion.id) else {
            return false;
        };
        doc.with_own_fonts(&mut self.fonts, |doc, fonts| {
            doc.handle_completion(pending, completion.result, fonts);
        });
        // Without scripts in the frame nothing listens to its element
        // events.
        doc.take_element_events();
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
    /// event steps"): as a task of the page's runtime for a container in
    /// the page's document (frame documents run no scripts yet). True if
    /// any frame loaded.
    pub(super) fn check_frame_loads(&mut self) -> bool {
        if self.lent || self.frames.count() == 0 {
            return false;
        }
        let mut containers = Vec::new();
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
            let (parent, container) = (frame.parent, frame.container);
            if let Some(frame) = self.frames.get_mut(id) {
                frame.load_fired = true;
            }
            any = true;
            if parent == Some(FrameId::TOP)
                && let Some(container) = container
            {
                containers.push((id, container));
            }
        }
        if self.script.is_some() {
            for (id, container) in containers {
                // A listener of an earlier `load` may have removed the
                // container, or removed and inserted it again (a new frame,
                // which has not loaded).
                if self.frames.by_container(FrameId::TOP, container) == Some(id) {
                    self.dispatch_event(
                        DispatchTarget::Node(container),
                        &TrustedEvent::new("load"),
                    );
                }
            }
        }
        any
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
            let Some(frame) = self.frames.get(id) else {
                continue;
            };
            let (Some(parent), Some(container)) = (frame.parent, frame.container) else {
                continue;
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
            let view = self.frame_view(viewport);
            let Some(frame) = self.frames.get_mut(id) else {
                continue;
            };
            frame.viewport = viewport;
            frame.viewport_serial = viewport_serial;
            frame.has_box = has_box;
            if let Some(doc) = frame.doc.as_deref_mut() {
                doc.set_view(view);
                doc.with_own_fonts(&mut self.fonts, DocumentState::update_layout);
            }
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
fn clamp_viewport(size: Size) -> Size {
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
