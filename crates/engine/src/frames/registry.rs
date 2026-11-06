//! The page's frame registry as natives see it (ADR 0031 section 5): during a
//! script task the page moves its frame tree here, and the runtime reaches it
//! through [`swb_web::FrameHost`]. So a native can read the tree, make and
//! destroy frames, and borrow the document of a frame, while the page itself
//! waits for the task to end. Outside a task the registry holds an empty tree.
//!
//! A native borrows the registry for one call and never across a call into
//! script. What only the page can do (the navigation of a new frame, the input
//! state of a destroyed one) waits in [`AfterTask`].
//!
//! The page's frames share one registry, and it knows the agent of the running
//! task ([`FrameRegistry::enter`]). Each member answers for that agent: a frame of
//! another agent has no key and no document here. When an agent or a key does not
//! match, the answer is "no document"; it never forwards. The members that read
//! the frame tree (the hooks of cross-origin objects) answer for every frame.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use swb_dom::NodeId;
use swb_net::{ByteBudget, Loader, Origin, Url};
use swb_text::FontContext;
use swb_web::{DocumentKey, EngineHost, FrameHost, LentDocuments, NewFrame, RetiredFrame};

use super::{
    AgentId, Charge, FrameId, FrameScript, FrameTarget, Frames, MAX_DEPTH, MAX_FRAMES, Sandbox,
};
use crate::document::{DocumentInit, DocumentState, ScrollTarget, View};
use crate::resources::Images;

/// The page's font context while a task runs: the documents that a task
/// lays out use it, whichever is current (ADR 0028 section 2).
pub(crate) type LentFonts = RefCell<Option<FontContext>>;

/// The markup of an initial `about:blank` document (measured:
/// `<html><head></head><body></body></html>`, quirks mode).
pub(crate) const BLANK: &str = "";

/// What a new frame document needs from the page.
pub(crate) struct DocumentMaker {
    pub(crate) loader: Rc<Loader>,
    pub(crate) storage: swb_web::SharedStorage,
    pub(crate) fonts: Rc<LentFonts>,
    pub(crate) images: Arc<ByteBudget>,
    pub(crate) vectors: Arc<swb_paint::VectorCache>,
    pub(crate) styles: Arc<ByteBudget>,
    pub(crate) bodies: Arc<ByteBudget>,
    pub(crate) font_files: Arc<ByteBudget>,
}

/// A parsed frame document and what it starts with.
pub(crate) struct FrameDocument {
    pub(crate) dom: swb_dom::Document,
    pub(crate) parser: Option<swb_dom::HtmlParser>,
    pub(crate) encoding: &'static encoding_rs::Encoding,
    pub(crate) content_type: String,
    pub(crate) parse: std::time::Duration,
    pub(crate) url: Url,
    pub(crate) origin: Origin,
    pub(crate) site_for_cookies: Option<Origin>,
    pub(crate) view: View,
    pub(crate) images: Images,
    pub(crate) fallback_base: Option<Url>,
    pub(crate) creator_url: Option<Url>,
    pub(crate) history: crate::document::HistoryInfo,
    pub(crate) node_room: Rc<Cell<usize>>,
    /// The frame's sandbox has no `allow-same-origin`.
    pub(crate) sandboxed_origin: bool,
}

impl DocumentMaker {
    /// A new frame document.
    pub(crate) fn make(&self, init: FrameDocument) -> Box<DocumentState> {
        let scroll_target = init
            .url
            .fragment()
            .map(|f| ScrollTarget::Fragment(f.to_owned()));
        let mut images = init.images;
        images.attach_budget(Arc::clone(&self.images), Arc::clone(&self.vectors));
        let mut doc = Box::new(DocumentState::new(DocumentInit {
            dom: init.dom,
            parser: init.parser,
            url: init.url,
            encoding: init.encoding,
            content_type: init.content_type,
            images,
            loader: Rc::clone(&self.loader),
            storage: Rc::clone(&self.storage),
            view: init.view,
            scroll_target,
            parse: init.parse,
            client: self.loader.new_client(),
            origin: Some(init.origin),
            site_for_cookies: init.site_for_cookies,
            frame: true,
            fallback_base: init.fallback_base,
            creator_url: init.creator_url,
            style_budget: Some(Arc::clone(&self.styles)),
            body_budget: Some(Arc::clone(&self.bodies)),
            font_budget: Some(Arc::clone(&self.font_files)),
            fonts: Rc::clone(&self.fonts),
            node_room: Some(init.node_room),
        }));
        doc.history = init.history;
        doc.sandboxed_origin = init.sandboxed_origin;
        doc
    }
}

/// What a new frame takes from its parent's document and its container.
pub(crate) struct ChildInit {
    pub(crate) container: NodeId,
    pub(crate) name: String,
    pub(crate) sandbox_attr: Option<String>,
    pub(crate) base_url: Url,
    /// The URL of the cookies and the storage of the parent's document.
    pub(crate) creator_url: Url,
    /// What the attributes ask for at the insertion.
    pub(crate) target: Option<FrameTarget>,
    pub(crate) history: crate::document::HistoryInfo,
}

impl ChildInit {
    /// What the `iframe` element `container` of `doc` gives its frame; `None`
    /// if it is no element.
    pub(crate) fn of(doc: &DocumentState, container: NodeId) -> Option<ChildInit> {
        let element = doc.dom().element(container)?;
        Some(ChildInit {
            container,
            name: element.attr("name").unwrap_or_default().to_owned(),
            sandbox_attr: element.attr("sandbox").map(str::to_owned),
            base_url: doc.base_url().clone(),
            creator_url: doc.cookie_url().clone(),
            target: attribute_target(doc, container),
            history: doc.history.clone(),
        })
    }
}

/// What the attributes of the `iframe` element `container` of `doc` ask for:
/// `srcdoc`, else the URL of `src` (resolved against the base URL), else
/// `about:blank`. `None` if `src` does not parse (nothing happens).
pub(crate) fn attribute_target(doc: &DocumentState, container: NodeId) -> Option<FrameTarget> {
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
            log::debug!("the src {src:?} does not parse");
            None
        }
    }
}

/// The settings of the page that a new frame document takes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameSettings {
    /// The page's scripting flag.
    pub(crate) javascript: bool,
    /// The page's view: scale and color scheme.
    pub(crate) view: View,
}

impl FrameSettings {
    /// The view of a frame document with `viewport`.
    pub(crate) fn view(&self, viewport: swb_layout::Size) -> View {
        View {
            viewport,
            scale: self.view.scale,
            indicators: false,
            color_scheme: self.view.color_scheme,
        }
    }
}

impl Frames {
    /// Makes the frame of an `iframe` element in `parent`'s document with its
    /// initial `about:blank` document, at position `at` of the parent's
    /// children. `None` past the limits of frames and depth (ADR 0031 section
    /// 11; a warning per top-level document).
    pub(crate) fn create_child(
        &mut self,
        maker: &DocumentMaker,
        settings: FrameSettings,
        parent: FrameId,
        init: ChildInit,
        at: usize,
    ) -> Option<FrameId> {
        if self.count() >= MAX_FRAMES {
            if !std::mem::replace(&mut self.count_warned, true) {
                log::warn!(
                    "the page has {MAX_FRAMES} frames, the limit; further iframe elements get no frame"
                );
            }
            return None;
        }
        let record = self.get(parent)?;
        if record.depth + 1 > MAX_DEPTH {
            if !std::mem::replace(&mut self.depth_warned, true) {
                log::warn!(
                    "frames are nested {MAX_DEPTH} deep, the limit; a deeper iframe gets no frame"
                );
            }
            return None;
        }
        let sandbox = Sandbox::parse(init.sandbox_attr.as_deref()).inherit(record.sandbox);
        let origin = if sandbox.opaque_origin() {
            Origin::new_opaque()
        } else {
            record.origin.clone()?
        };
        let site = Some(self.site_for_cookies(parent, Some(&origin)));
        let parsed = swb_dom::parse_html_with(BLANK, settings.javascript);
        let doc = maker.make(FrameDocument {
            dom: parsed,
            parser: None,
            encoding: encoding_rs::UTF_8,
            content_type: "text/html".to_owned(),
            parse: std::time::Duration::ZERO,
            url: crate::page::about_blank(),
            origin: origin.clone(),
            site_for_cookies: site,
            view: settings.view(super::DEFAULT_FRAME_VIEWPORT),
            images: Images::default(),
            fallback_base: Some(init.base_url),
            creator_url: (!sandbox.opaque_origin()).then_some(init.creator_url),
            history: init.history,
            node_room: Rc::clone(&self.node_room),
            sandboxed_origin: sandbox.opaque_origin(),
        });
        let id = self.insert(parent, init.container, doc, at)?;
        let frame = self.get_mut(id)?;
        frame.name = init.name;
        frame.sandbox = sandbox;
        frame.origin = Some(origin);
        frame.reuse_window = matches!(init.target, Some(FrameTarget::Srcdoc | FrameTarget::Url(_)));
        log::debug!("{id}: made for an iframe of {parent}");
        Some(id)
    }
}

/// The most moves of the page's focus that the page replays after a task: a loop
/// of `focus()` calls must cost no memory.
const MAX_FOCUS_MOVES: usize = 16;

/// What the page does after a task for the frames that the task made or
/// destroyed.
#[derive(Debug, Default)]
pub(crate) struct AfterTask {
    /// New frames whose attributes the page has yet to process.
    pub(crate) created: Vec<FrameId>,
    /// Destroyed frames with their parents.
    pub(crate) destroyed: Vec<(FrameId, Option<FrameId>)>,
    /// The frame that a script moved the page's focus to.
    pub(crate) focused: Option<FrameId>,
    /// The moves of the page's focus (from, to) that the task made: the documents
    /// of other runtimes get theirs after the task. At most [`MAX_FOCUS_MOVES`];
    /// past that, the last one holds the end of the rest.
    pub(crate) focus_moves: Vec<(FrameId, FrameId)>,
    /// The navigations that scripts asked for through the `Location` of a frame
    /// of another origin: one per frame, the last request.
    pub(crate) navigations: BTreeMap<FrameId, CrossOriginNavigation>,
    /// The documents of other agents in the frames that the task destroyed: their
    /// `pagehide` and `unload` run after the task, in their own runtimes, children
    /// first; then they retire.
    pub(crate) retired_elsewhere: Vec<RetiredElsewhere>,
}

impl AfterTask {
    /// True if the task left nothing for the page, as most tasks do.
    pub(crate) fn is_empty(&self) -> bool {
        self.created.is_empty()
            && self.destroyed.is_empty()
            && self.focused.is_none()
            && self.navigations.is_empty()
            && self.retired_elsewhere.is_empty()
    }
}

/// A document of another agent in a frame that a task destroyed.
pub(crate) struct RetiredElsewhere {
    pub(crate) frame: FrameId,
    pub(crate) script: FrameScript,
    pub(crate) doc: Option<Box<DocumentState>>,
}

impl std::fmt::Debug for RetiredElsewhere {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RetiredElsewhere({})", self.frame)
    }
}

/// A navigation of a frame of another origin that a script asked for.
#[derive(Debug)]
pub(crate) struct CrossOriginNavigation {
    pub(crate) url: Url,
    /// The origin of the document that asked.
    pub(crate) initiator: Option<Origin>,
    /// True for `location.replace()`.
    pub(crate) replace: bool,
}

/// The registry. See the module documentation.
pub(crate) struct FrameRegistry {
    /// The page's frames during a task, else an empty tree.
    pub(crate) frames: Box<Frames>,
    pub(crate) maker: Rc<DocumentMaker>,
    pub(crate) settings: FrameSettings,
    pub(crate) after_task: AfterTask,
    /// The agent of the running task.
    pub(crate) agent: Option<AgentId>,
    /// The origin of [`FrameRegistry::agent`].
    pub(crate) origin: Option<Origin>,
}

impl FrameRegistry {
    fn frame(&self, number: u32) -> Option<&super::Frame> {
        self.frames.get(FrameId::from_number(number))
    }

    /// The running task belongs to `agent`, of `origin`; `origin` gives the
    /// origin when the agent differs from the last one, so that a page with one
    /// runtime does not copy it for each task.
    pub(crate) fn enter(&mut self, agent: AgentId, origin: impl FnOnce() -> Option<Origin>) {
        if self.agent != Some(agent) {
            self.agent = Some(agent);
            self.origin = origin();
        }
    }

    /// True if frame `id`'s document has the origin of the running agent.
    fn has_agent_origin(&self, id: FrameId) -> bool {
        let (Some(frame), Some(origin)) = (self.frames.get(id), self.origin.as_ref()) else {
            return false;
        };
        frame.origin.as_ref() == Some(origin)
    }

    /// The document of `frame` for layout across documents and the attributes of
    /// its elements, whatever its agent: the runtime's, or the record's. No script
    /// of another runtime runs on it.
    fn document_mut<'a>(
        frames: &'a mut Frames,
        docs: &'a mut dyn LentDocuments,
        agent: Option<AgentId>,
        frame: FrameId,
    ) -> Option<&'a mut DocumentState> {
        let record = frames.get_mut(frame)?;
        if let Some(doc) = record.doc.as_deref_mut() {
            return Some(doc);
        }
        let key = record.script?.key_in(agent?)?;
        let host: &mut dyn std::any::Any = docs.lent_host(key)?;
        host.downcast_mut::<DocumentState>()
    }

    /// The document of `frame` if it has the origin of the running agent; `None`
    /// for another agent's.
    fn own_document_mut<'a>(
        &'a mut self,
        docs: &'a mut dyn LentDocuments,
        frame: FrameId,
    ) -> Option<&'a mut DocumentState> {
        if !self.has_agent_origin(frame) {
            return None;
        }
        let agent = self.agent;
        Self::document_mut(&mut self.frames, docs, agent, frame)
    }

    /// Lays out the document of `parent` and gives its child `child` the
    /// content box of its container as its viewport.
    fn update_viewport(
        &mut self,
        parent: FrameId,
        child: FrameId,
        docs: &mut dyn LentDocuments,
    ) -> Option<()> {
        let record = self.frames.get(child)?;
        let (container, old, old_serial) =
            (record.container?, record.viewport, record.viewport_serial);
        let agent = self.agent;
        let parent_doc = Self::document_mut(&mut self.frames, docs, agent, parent)?;
        parent_doc.script_layout_flush();
        let serial = parent_doc
            .fragments()
            .is_some()
            .then(|| parent_doc.layout_serial());
        let mut viewport = old;
        let mut has_box = None;
        if serial.is_some() && serial != old_serial {
            let size = parent_doc.frame_box(container);
            has_box = Some(size.is_some());
            if let Some(size) = size {
                viewport = crate::page::clamp_viewport(size);
            }
        }
        let view = self.settings.view(viewport);
        let record = self.frames.get_mut(child)?;
        if viewport != record.viewport && record.viewport_serial.is_some() {
            record.resized = true;
        }
        record.viewport = viewport;
        if serial.is_some() {
            record.viewport_serial = serial;
        }
        if let Some(has_box) = has_box {
            record.has_box = has_box;
        }
        let child_doc = Self::document_mut(&mut self.frames, docs, agent, child)?;
        child_doc.set_view(view);
        Some(())
    }
}

impl FrameHost for FrameRegistry {
    fn is_alive(&self, frame: u32) -> bool {
        self.frame(frame).is_some()
    }

    fn parent(&self, frame: u32) -> Option<u32> {
        Some(self.frame(frame)?.parent?.number())
    }

    fn child_count(&self, frame: u32) -> u32 {
        self.frame(frame)
            .map_or(0, |f| u32::try_from(f.children.len()).unwrap_or(u32::MAX))
    }

    fn child_at(&self, frame: u32, index: u32) -> Option<u32> {
        let child = self.frame(frame)?.children.get(index as usize)?;
        Some(child.number())
    }

    fn child_named(&self, frame: u32, name: &str) -> Option<u32> {
        let children = &self.frame(frame)?.children;
        children
            .iter()
            .find(|&&c| self.frames.get(c).is_some_and(|f| f.name == name))
            .map(|c| c.number())
    }

    fn child_of_container(&self, parent: u32, container: NodeId) -> Option<u32> {
        let parent = FrameId::from_number(parent);
        if !self.has_agent_origin(parent) {
            return None;
        }
        Some(self.frames.by_container(parent, container)?.number())
    }

    fn child_containers(&self, frame: u32) -> Vec<(u32, NodeId)> {
        let Some(record) = self.frame(frame) else {
            return Vec::new();
        };
        if !self.has_agent_origin(FrameId::from_number(frame)) {
            return Vec::new();
        }
        record
            .children
            .iter()
            .filter_map(|&c| Some((c.number(), self.frames.get(c)?.container?)))
            .collect()
    }

    fn container(&self, frame: u32) -> Option<NodeId> {
        let record = self.frame(frame)?;
        if !self.has_agent_origin(record.parent?) {
            return None;
        }
        record.container
    }

    fn name(&self, frame: u32) -> String {
        if !self.has_agent_origin(FrameId::from_number(frame)) {
            return String::new();
        }
        self.frame(frame)
            .map(|f| f.name.clone())
            .unwrap_or_default()
    }

    fn set_name(&mut self, frame: u32, name: &str) {
        let id = FrameId::from_number(frame);
        if !self.has_agent_origin(id) {
            return;
        }
        if let Some(record) = self.frames.get_mut(id) {
            name.clone_into(&mut record.name);
        }
    }

    fn in_this_agent(&self, frame: u32) -> bool {
        self.has_agent_origin(FrameId::from_number(frame))
    }

    fn scripts_allowed(&self, frame: u32) -> bool {
        self.frame(frame)
            .is_some_and(|f| f.sandbox.scripts_allowed())
    }

    fn document_key(&self, frame: u32) -> Option<DocumentKey> {
        self.frame(frame)?.script?.key_in(self.agent?)
    }

    fn set_document_key(&mut self, frame: u32, key: Option<DocumentKey>) {
        let id = FrameId::from_number(frame);
        let Some(agent) = self.agent else {
            return;
        };
        if !self.has_agent_origin(id) {
            return;
        }
        if let Some(record) = self.frames.get_mut(id) {
            record.script = key.map(|key| FrameScript::new(agent, key));
        }
    }

    fn document_loading(&self, frame: u32) -> bool {
        if !self.has_agent_origin(FrameId::from_number(frame)) {
            return false;
        }
        self.frame(frame)
            .and_then(|f| f.doc.as_deref())
            .is_some_and(DocumentState::is_loading)
    }

    fn take_document(&mut self, frame: u32, key: DocumentKey) -> Option<Box<dyn EngineHost>> {
        let agent = self.agent?;
        let record = self.frames.get_mut(FrameId::from_number(frame))?;
        if !record.has_document(agent, key) {
            return None;
        }
        let doc: Box<dyn EngineHost> = record.doc.take()?;
        Some(doc)
    }

    fn connect(
        &mut self,
        parent: u32,
        container: NodeId,
        parent_doc: &mut dyn EngineHost,
    ) -> Option<NewFrame> {
        let host: &mut dyn std::any::Any = parent_doc;
        let doc = host.downcast_mut::<DocumentState>()?;
        doc.process_changes();
        let parent = FrameId::from_number(parent);
        let init = ChildInit::of(doc, container)?;
        let loaded_at_once = matches!(init.target, Some(FrameTarget::Blank) | None);
        let at = {
            let dom = doc.dom();
            self.frames
                .child_position(parent, container, |a, b| dom.precedes(a, b))
        };
        let maker = Rc::clone(&self.maker);
        let id = self
            .frames
            .create_child(&maker, self.settings, parent, init, at)?;
        self.after_task.created.push(id);
        Some(NewFrame {
            frame: id.number(),
            loaded_at_once,
        })
    }

    fn subtree(&self, frame: u32) -> Vec<u32> {
        let id = FrameId::from_number(frame);
        if self.frames.get(id).is_none() {
            return Vec::new();
        }
        let mut all: Vec<u32> = self
            .frames
            .descendants_post_order(id)
            .into_iter()
            .map(FrameId::number)
            .collect();
        all.push(frame);
        all
    }

    fn disconnect(&mut self, frame: u32) -> Vec<RetiredFrame> {
        let id = FrameId::from_number(frame);
        if self.frames.get(id).is_none() {
            return Vec::new();
        }
        let mut doomed = self.frames.descendants_post_order(id);
        doomed.push(id);
        let mut retired = Vec::with_capacity(doomed.len());
        for one in doomed {
            let Some(mut record) = self.frames.remove(one) else {
                continue;
            };
            self.after_task.destroyed.push((one, record.parent));
            if let Some(super::PendingNavigation::Request { request, .. }) = &record.pending {
                self.maker.loader.cancel(*request);
            }
            let doc = record.doc.take();
            let Some(script) = record.script else {
                // Nothing can reach the document: it goes now.
                if let Some(mut doc) = doc {
                    self.maker.loader.cancel_client(doc.client);
                    doc.release_fonts_in_task();
                }
                retired.push(RetiredFrame {
                    frame: one.number(),
                    key: None,
                    doc: None,
                });
                continue;
            };
            self.frames
                .charge_retired(script.agent(), record.charge.nodes);
            let key = self.agent.and_then(|agent| script.key_in(agent));
            if key.is_none() {
                // Another runtime's: the page runs its unload after the task. The
                // removal holds for this task from here on.
                self.after_task.retired_elsewhere.push(RetiredElsewhere {
                    frame: one,
                    script,
                    doc,
                });
                retired.push(RetiredFrame {
                    frame: one.number(),
                    key: None,
                    doc: None,
                });
                continue;
            }
            retired.push(RetiredFrame {
                frame: one.number(),
                key,
                doc: doc.map(|d| {
                    let doc: Box<dyn EngineHost> = d;
                    doc
                }),
            });
        }
        retired
    }

    fn frame_loaded(&mut self, frame: u32) {
        if let Some(record) = self.frames.get_mut(FrameId::from_number(frame)) {
            record.load_fired = true;
            record.reuse_window = false;
        }
    }

    fn may_navigate(&self, source: u32, target: u32) -> bool {
        let source = FrameId::from_number(source);
        self.frames.may_navigate(
            source,
            FrameId::from_number(target),
            self.script_navigation_rule(source),
        )
    }

    fn note_location_flags(&mut self, target: u32, docs: &mut dyn LentDocuments) {
        let target = FrameId::from_number(target);
        // Whatever an earlier write left goes: if this write finds no flags, its
        // navigation reads them when it starts, and it never takes old ones.
        let Some(noted) = self
            .frames
            .get_mut(target)
            .map(|record| record.location_sandbox.take())
        else {
            return;
        };
        let Some((parent, container)) = self
            .frames
            .get(target)
            .and_then(|record| Some((record.parent?, record.container?)))
        else {
            return;
        };
        let inherited = self
            .frames
            .get(parent)
            .map(|record| record.sandbox)
            .unwrap_or_default();
        let agent = self.agent;
        let Some(doc) = Self::document_mut(&mut self.frames, docs, agent, parent) else {
            return;
        };
        let generation = doc.dom().generation();
        let flags = match noted {
            Some((at, flags)) if at == generation => flags,
            _ => {
                let attr = doc.dom().element(container).and_then(|e| e.attr("sandbox"));
                Sandbox::parse(attr).inherit(inherited)
            }
        };
        if let Some(record) = self.frames.get_mut(target) {
            record.location_sandbox = Some((generation, flags));
        }
    }

    fn navigate_frame(&mut self, target: u32, url: &str, source: u32, replace: bool) {
        let Ok(url) = Url::parse(url) else {
            return;
        };
        let target = FrameId::from_number(target);
        if self.frames.get(target).is_none() {
            return;
        }
        let initiator = self
            .frames
            .get(FrameId::from_number(source))
            .and_then(|frame| frame.origin.clone());
        self.after_task.navigations.insert(
            target,
            CrossOriginNavigation {
                url,
                initiator,
                replace,
            },
        );
    }

    fn focused_frame(&self) -> u32 {
        self.frames.focused.number()
    }

    fn holds_focus(&self, frame: u32) -> bool {
        self.frames
            .chain(self.frames.focused)
            .contains(&FrameId::from_number(frame))
    }

    fn set_focused_frame(&mut self, frame: u32, docs: &mut dyn LentDocuments) {
        let new = FrameId::from_number(frame);
        let old = self.frames.focused;
        let old_chain = self.frames.chain(old);
        let new_chain = self.frames.chain(new);
        for &id in old_chain.iter().filter(|id| !new_chain.contains(id)) {
            if let Some(doc) = self.own_document_mut(docs, id) {
                doc.frame_focus = None;
                doc.script_set_focus(None, false);
            }
        }
        for (index, &id) in new_chain.iter().enumerate() {
            let container = new_chain
                .get(index + 1)
                .and_then(|&next| self.frames.get(next))
                .and_then(|record| record.container);
            if let Some(doc) = self.own_document_mut(docs, id) {
                if container.is_some() {
                    doc.script_set_focus(None, false);
                }
                doc.frame_focus = container;
                doc.window_blurred = false;
            }
        }
        self.frames.focused = new;
        let moves = &mut self.after_task.focus_moves;
        if moves.len() < MAX_FOCUS_MOVES {
            moves.push((old, new));
        } else if let Some(last) = moves.last_mut() {
            last.1 = new;
        }
        self.after_task.focused = Some(new);
    }

    fn focus_after_disconnect(&mut self, docs: &mut dyn LentDocuments) {
        if self.frames.get(self.frames.focused).is_some() {
            return;
        }
        // The arena has no parent of a dead frame; the page knows it from the
        // destruction, and until then the focus is nowhere.
        // A top level of another agent: the page does this after the task.
        self.frames.focused = FrameId::TOP;
        if let Some(doc) = self.own_document_mut(docs, FrameId::TOP) {
            doc.frame_focus = None;
            doc.window_blurred = true;
        }
    }

    fn choose_frame(&self, source: u32, name: &str) -> Option<u32> {
        let source = FrameId::from_number(source);
        let allows = self.script_navigation_rule(source);
        let chosen = if name.eq_ignore_ascii_case("_top") {
            FrameId::TOP
        } else if name.eq_ignore_ascii_case("_parent") {
            self.frames.get(source)?.parent.unwrap_or(source)
        } else if name.eq_ignore_ascii_case("_blank") {
            return None;
        } else {
            // The first frame in tree order with the name that `source` may
            // navigate (as `Page::frame_named`).
            return self
                .frames
                .tree_order()
                .into_iter()
                .skip(1)
                .filter(|&id| self.frames.get(id).is_some_and(|f| f.name == name))
                .find(|&id| self.frames.may_navigate(source, id, allows))
                .map(FrameId::number);
        };
        self.frames
            .may_navigate(source, chosen, allows)
            .then(|| chosen.number())
    }

    fn update_viewports(&mut self, frame: u32, docs: &mut dyn LentDocuments) {
        let chain = self.frames.chain(FrameId::from_number(frame));
        for pair in chain.windows(2) {
            if let [parent, child] = *pair
                && self.update_viewport(parent, child, docs).is_none()
            {
                return;
            }
        }
    }
}

impl FrameRegistry {
    /// The sandbox's rule for a navigation of the top level by a script of `source`:
    /// with a recent activation of that frame, the token
    /// `allow-top-navigation-by-user-activation` counts too (measured, cases
    /// `activation-*`).
    fn script_navigation_rule(&self, source: FrameId) -> fn(Sandbox) -> bool {
        if self.frames.has_transient_activation(source) {
            Sandbox::top_navigation_allowed
        } else {
            Sandbox::script_top_navigation_allowed
        }
    }
}

/// A charge of a document whose nodes come while it parses in steps.
pub(crate) fn scripted_charge(bytes: usize) -> Charge {
    Charge { bytes, nodes: 0 }
}
