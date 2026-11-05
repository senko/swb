//! The page's frame registry as natives see it (ADR 0031 section 5): during a
//! script task the page moves its frame tree here, and the runtime reaches it
//! through [`swb_web::FrameHost`]. So a native can read the tree, make and
//! destroy frames, and borrow the document of a frame, while the page itself
//! waits for the task to end. Outside a task the registry holds an empty tree.
//!
//! A native borrows the registry for one call and never across a call into
//! script. What only the page can do (the navigation of a new frame, the input
//! state of a destroyed one) waits in [`AfterTask`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use swb_dom::NodeId;
use swb_net::{ByteBudget, Loader, Origin, Url};
use swb_text::FontContext;
use swb_web::{DocumentKey, EngineHost, FrameHost, LentDocuments, NewFrame, RetiredFrame};

use super::{Charge, FrameId, FrameTarget, Frames, MAX_DEPTH, MAX_FRAMES, Sandbox};
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

/// What the page does after a task for the frames that the task made or
/// destroyed.
#[derive(Debug, Default)]
pub(crate) struct AfterTask {
    /// New frames whose attributes the page has yet to process.
    pub(crate) created: Vec<FrameId>,
    /// Destroyed frames with their parents.
    pub(crate) destroyed: Vec<(FrameId, Option<FrameId>)>,
}

/// The registry. See the module documentation.
pub(crate) struct FrameRegistry {
    /// The page's frames during a task, else an empty tree.
    pub(crate) frames: Box<Frames>,
    pub(crate) maker: Rc<DocumentMaker>,
    pub(crate) settings: FrameSettings,
    pub(crate) after_task: AfterTask,
}

impl FrameRegistry {
    fn frame(&self, number: u32) -> Option<&super::Frame> {
        self.frames.get(FrameId::from_number(number))
    }

    /// The document of `frame`: the runtime's, or the record's.
    fn document_mut<'a>(
        frames: &'a mut Frames,
        docs: &'a mut dyn LentDocuments,
        frame: FrameId,
    ) -> Option<&'a mut DocumentState> {
        let record = frames.get_mut(frame)?;
        if let Some(doc) = record.doc.as_deref_mut() {
            return Some(doc);
        }
        let key = record.script?;
        let host: &mut dyn std::any::Any = docs.lent_host(key)?;
        host.downcast_mut::<DocumentState>()
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
        let parent_doc = Self::document_mut(&mut self.frames, docs, parent)?;
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
        let child_doc = Self::document_mut(&mut self.frames, docs, child)?;
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
        Some(self.frames.by_container(parent, container)?.number())
    }

    fn child_containers(&self, frame: u32) -> Vec<(u32, NodeId)> {
        let Some(record) = self.frame(frame) else {
            return Vec::new();
        };
        record
            .children
            .iter()
            .filter_map(|&c| Some((c.number(), self.frames.get(c)?.container?)))
            .collect()
    }

    fn container(&self, frame: u32) -> Option<NodeId> {
        self.frame(frame)?.container
    }

    fn name(&self, frame: u32) -> String {
        self.frame(frame)
            .map(|f| f.name.clone())
            .unwrap_or_default()
    }

    fn set_name(&mut self, frame: u32, name: &str) {
        if let Some(record) = self.frames.get_mut(FrameId::from_number(frame)) {
            name.clone_into(&mut record.name);
        }
    }

    fn in_this_agent(&self, frame: u32) -> bool {
        self.frames.in_agent(FrameId::from_number(frame))
    }

    fn scripts_allowed(&self, frame: u32) -> bool {
        self.frame(frame)
            .is_some_and(|f| f.sandbox.scripts_allowed())
    }

    fn document_key(&self, frame: u32) -> Option<DocumentKey> {
        self.frame(frame)?.script
    }

    fn set_document_key(&mut self, frame: u32, key: Option<DocumentKey>) {
        if let Some(record) = self.frames.get_mut(FrameId::from_number(frame)) {
            record.script = key;
        }
    }

    fn document_loading(&self, frame: u32) -> bool {
        self.frame(frame)
            .and_then(|f| f.doc.as_deref())
            .is_some_and(DocumentState::is_loading)
    }

    fn take_document(&mut self, frame: u32) -> Option<Box<dyn EngineHost>> {
        let record = self.frames.get_mut(FrameId::from_number(frame))?;
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
            if record.script.is_none() {
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
            }
            self.frames.charge_retired(record.charge.nodes);
            retired.push(RetiredFrame {
                frame: one.number(),
                key: record.script,
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
        self.frames.may_navigate(
            FrameId::from_number(source),
            FrameId::from_number(target),
            Sandbox::script_top_navigation_allowed,
        )
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

/// A charge of a document whose nodes come while it parses in steps.
pub(crate) fn scripted_charge(bytes: usize) -> Charge {
    Charge { bytes, nodes: 0 }
}
