//! The frame tree of a page (ADR 0031 section 2): an arena of [`Frame`]
//! records addressed by [`FrameId`] (an index and a generation, so that an
//! id of a destroyed frame never finds another frame). [`FrameId::TOP`] is
//! the top-level traversable; its document is the page's document
//! (`Page::doc`, which script tasks borrow), so its record holds the tree
//! data only. Every other frame is the content of an `iframe` element (its
//! *container*) in its parent's document and owns its document.
//!
//! The page (`page/frames.rs`) makes, navigates and destroys frames; this
//! module keeps the records, the tree order and the lookup tables, and
//! the limits of ADR 0031 section 11.

mod budget;
pub(crate) mod paint;
pub(crate) mod registry;
mod sandbox;

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use swb_dom::NodeId;
use swb_layout::Size;
use swb_net::{ClientId, Origin, RequestId, Url, same_site};
use swb_web::DocumentKey;

use crate::document::DocumentState;

pub(crate) use budget::Charge;
pub use budget::{MAX_FONT_BYTES, MAX_IMAGE_BYTES, MAX_NODES, MAX_SOURCE_BYTES, MAX_STYLE_BYTES};
pub use sandbox::Sandbox;

use budget::Budget;

/// The most frames of a page besides the top-level one (Chromium 148,
/// measured in `tools/probes/web-frames.json`, case `limits`: the 1,001st
/// `iframe` has no `contentWindow`).
pub const MAX_FRAMES: usize = 1_000;

/// The deepest frame: the top-level frame has depth 0, its children 1.
/// Chromium has no limit of its own below its frame count; the limit
/// bounds the recursion of layout, paint and hit testing.
pub const MAX_DEPTH: u32 = 32;

/// The most navigations of one frame in [`NAVIGATION_WINDOW_MS`] of page
/// time; later ones are dropped (a frame that reloads itself).
pub const MAX_NAVIGATIONS: usize = 64;

/// The window of [`MAX_NAVIGATIONS`] in ms of page time.
pub const NAVIGATION_WINDOW_MS: f64 = 10_000.0;

/// The viewport of a frame before its container had a box: the default
/// object size of a replaced element (CSS 2.2 §10.3.2, §10.6.2).
pub const DEFAULT_FRAME_VIEWPORT: Size = Size {
    width: 300.0,
    height: 150.0,
};

/// Identifies a frame of a page. A stale id (of a destroyed frame) finds
/// nothing, not another frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId {
    index: u32,
    generation: u32,
}

impl FrameId {
    /// The top-level frame: the page's document.
    pub const TOP: FrameId = FrameId {
        index: 0,
        generation: 0,
    };

    /// A number that identifies the frame in logs and tools (index and
    /// generation).
    pub fn to_u64(self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.index)
    }

    /// The id of [`FrameId::to_u64`].
    pub fn from_u64(value: u64) -> FrameId {
        FrameId {
            index: (value & u64::from(u32::MAX)) as u32,
            generation: (value >> 32) as u32,
        }
    }

    /// The number of the frame in the script runtime ([`swb_web::FrameHost`]):
    /// below 2^31, and unique over the life of the arena, because a slot whose
    /// generation reaches [`MAX_GENERATION`] is not used again.
    pub fn number(self) -> u32 {
        (self.generation << INDEX_BITS) | self.index
    }

    /// The id of [`FrameId::number`].
    pub fn from_number(number: u32) -> FrameId {
        FrameId {
            index: number & ((1 << INDEX_BITS) - 1),
            generation: number >> INDEX_BITS,
        }
    }
}

/// The bits of the index in [`FrameId::number`]: an arena has at most
/// [`MAX_FRAMES`] live frames besides the top level, and a free slot is used
/// before a new one.
const INDEX_BITS: u32 = 10;

/// The first generation that a slot does not get; see [`FrameId::number`].
const MAX_GENERATION: u32 = 1 << (31 - INDEX_BITS);

impl std::fmt::Display for FrameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame {}.{}", self.index, self.generation)
    }
}

/// What a frame navigates to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FrameTarget {
    /// A new `about:blank` document.
    Blank,
    /// An `about:srcdoc` document with the markup of the container's
    /// `srcdoc` attribute (read when the navigation commits, so that a
    /// pending navigation holds no copy of it).
    Srcdoc,
    /// A document from the network (or a `data:` or `file:` URL).
    Url(Url),
}

/// The navigation of a frame whose document has not arrived.
#[derive(Clone, Debug)]
pub(crate) enum PendingNavigation {
    /// The request of the document is running.
    Request { url: Url, request: RequestId },
    /// A document without a request (`about:blank`, `about:srcdoc`); it
    /// commits in the next turn of the event loop (HTML queues it as a
    /// navigation task).
    Local(FrameTarget),
}

/// One frame (a navigable with its active document).
pub(crate) struct Frame {
    /// The parent frame; `None` for the top-level frame.
    pub(crate) parent: Option<FrameId>,
    /// The `iframe` element in the parent's document; `None` for the
    /// top-level frame.
    pub(crate) container: Option<NodeId>,
    /// The child frames, in the tree order of their containers.
    pub(crate) children: Vec<FrameId>,
    /// The name of the browsing context: the `name` attribute of the container
    /// when the frame was made, then what scripts write to `window.name`
    /// (measured: a later change of the attribute does not count).
    pub(crate) name: String,
    /// The origin of the current document.
    pub(crate) origin: Option<Origin>,
    /// The key of the current document in the script runtime, once it has a
    /// realm (ADR 0031 section 5).
    pub(crate) script: Option<DocumentKey>,
    /// True while a navigation that the insertion started can reuse the
    /// `Window` of the initial `about:blank` document (measured, case
    /// `scripted-initial-reuse`; HTML "initial about:blank").
    pub(crate) reuse_window: bool,
    /// True if the viewport changed since the last rendering update: the
    /// document's window gets `resize`.
    pub(crate) resized: bool,
    /// The sandboxing flags of the container's `sandbox` attribute when
    /// the current document was made (HTML takes them at navigation).
    pub(crate) sandbox: Sandbox,
    /// The sandboxing flags of the navigation that waits for its document,
    /// from when it started (HTML "snapshot target snapshot params"; measured,
    /// case `scripted-sandbox-scripts`).
    pub(crate) pending_sandbox: Sandbox,
    /// The document; `None` for the top-level frame (the page holds it).
    pub(crate) doc: Option<Box<DocumentState>>,
    /// A navigation whose document has not arrived.
    pub(crate) pending: Option<PendingNavigation>,
    /// The viewport: the content box of the container after the parent's
    /// last layout, or the last one if the container has no box.
    pub(crate) viewport: Size,
    /// The parent's layout serial of the last viewport update.
    pub(crate) viewport_serial: Option<u64>,
    /// True if the container has a box in the parent's last layout: the
    /// frame paints, so its document gets a display list.
    pub(crate) has_box: bool,
    /// The color of the canvas below the document if it is opaque; `None`
    /// for a transparent canvas (ADR 0031 section 4).
    pub(crate) canvas: Option<swb_style::Rgba>,
    /// 0 for the top-level frame.
    pub(crate) depth: u32,
    /// True once the current document has loaded (with its delaying child
    /// frames) and the container's `load` event fired; for the top-level
    /// frame, once the page's `load` event fired (or, with JavaScript off,
    /// the page was first fully loaded). A frame whose parent has this
    /// flag does not delay it.
    pub(crate) load_fired: bool,
    /// The page times of the recent navigations (at most
    /// [`MAX_NAVIGATIONS`] within [`NAVIGATION_WINDOW_MS`]).
    pub(crate) navigations: VecDeque<f64>,
    /// True after the warning about dropped navigations of the frame.
    pub(crate) navigation_warned: bool,
    /// The page time and URL of the declarative refresh of the document.
    pub(crate) refresh: Option<(f64, Url)>,
    /// What the current document costs the page's budget for frame
    /// documents.
    pub(crate) charge: Charge,
}

impl Frame {
    fn new(parent: Option<FrameId>, container: Option<NodeId>, depth: u32) -> Frame {
        Frame {
            parent,
            container,
            children: Vec::new(),
            name: String::new(),
            origin: None,
            script: None,
            reuse_window: false,
            resized: false,
            sandbox: Sandbox::default(),
            pending_sandbox: Sandbox::default(),
            doc: None,
            pending: None,
            viewport: DEFAULT_FRAME_VIEWPORT,
            viewport_serial: None,
            has_box: false,
            canvas: None,
            depth,
            load_fired: false,
            navigations: VecDeque::new(),
            navigation_warned: false,
            refresh: None,
            charge: Charge::default(),
        }
    }

    /// Records a navigation at page time `now`; false if the frame had
    /// [`MAX_NAVIGATIONS`] in the window before it (the navigation is
    /// dropped).
    pub(crate) fn note_navigation(&mut self, now: f64) -> bool {
        while self
            .navigations
            .front()
            .is_some_and(|&t| t <= now - NAVIGATION_WINDOW_MS)
        {
            self.navigations.pop_front();
        }
        if self.navigations.len() >= MAX_NAVIGATIONS {
            return false;
        }
        self.navigations.push_back(now);
        true
    }
}

struct Slot {
    generation: u32,
    frame: Option<Frame>,
}

/// The frames of a page. See the module documentation.
pub(crate) struct Frames {
    slots: Vec<Slot>,
    /// Indices of empty slots.
    free: Vec<u32>,
    /// The number of frames besides the top-level one.
    count: usize,
    /// The frame of each frame document's loader client.
    clients: HashMap<ClientId, FrameId>,
    /// The frame of each running request of a frame document.
    requests: HashMap<RequestId, FrameId>,
    /// The frame of each container: (parent frame, `iframe` element).
    containers: HashMap<(FrameId, NodeId), FrameId>,
    /// True after the warning about the frame limit (per top-level
    /// document).
    pub(crate) count_warned: bool,
    /// True after the warning about the depth limit (per top-level
    /// document).
    pub(crate) depth_warned: bool,
    /// The budget for the documents of frames.
    pub(crate) budget: Budget,
    /// The number of frames besides the top-level one, for the runtime: the
    /// paths of a page without frames read this one number.
    pub(crate) count_cell: Rc<Cell<usize>>,
    /// The nodes that the budget has room for, for the parser of a document
    /// that parses in steps.
    pub(crate) node_room: Rc<Cell<usize>>,
}

impl Frames {
    /// A tree with the top-level frame only.
    pub(crate) fn new() -> Frames {
        Frames {
            slots: vec![Slot {
                generation: 0,
                frame: Some(Frame::new(None, None, 0)),
            }],
            free: Vec::new(),
            count: 0,
            clients: HashMap::new(),
            requests: HashMap::new(),
            containers: HashMap::new(),
            count_warned: false,
            depth_warned: false,
            budget: Budget::default(),
            count_cell: Rc::default(),
            node_room: Rc::new(Cell::new(MAX_NODES)),
        }
    }

    /// Resets the top-level frame for a new top-level document (its
    /// children are gone): its load state and the warnings of the limits.
    pub(crate) fn reset_top(&mut self) {
        let top = self.top_mut();
        top.children.clear();
        top.load_fired = false;
        top.script = None;
        self.count_warned = false;
        self.depth_warned = false;
        self.budget.reset();
        self.node_room
            .set(self.budget.nodes_left(Charge::default()));
    }

    /// What frame `id`'s current document costs the budget.
    fn charge_of(&self, id: FrameId) -> Charge {
        self.get(id).map_or_else(Charge::default, |f| f.charge)
    }

    /// True if a document of `bytes` of source fits the budget of the page
    /// when it replaces the current document of frame `id`.
    pub(crate) fn fits_bytes(&self, id: FrameId, bytes: usize) -> bool {
        self.budget.fits_bytes(self.charge_of(id), bytes)
    }

    /// True if a document of `nodes` DOM nodes fits the budget of the page
    /// when it replaces the current document of frame `id`.
    pub(crate) fn fits_nodes(&self, id: FrameId, nodes: usize) -> bool {
        self.budget.fits_nodes(self.charge_of(id), nodes)
    }

    /// The most nodes that a document may have when it replaces the current
    /// document of frame `id`.
    pub(crate) fn nodes_left(&self, id: FrameId) -> usize {
        self.budget.nodes_left(self.charge_of(id))
    }

    /// Sets what frame `id`'s current document costs.
    pub(crate) fn set_charge(&mut self, id: FrameId, charge: Charge) {
        let old = self.charge_of(id);
        if let Some(frame) = self.get_mut(id) {
            frame.charge = charge;
            self.budget.replace(old, charge);
            self.node_room
                .set(self.budget.nodes_left(Charge::default()));
        }
    }

    /// A retired document (ADR 0031 section 5) stays in memory while the
    /// runtime lives, so its nodes stay in the budget until the next top-level
    /// document. Its source text is gone.
    pub(crate) fn charge_retired(&mut self, nodes: usize) {
        let charge = Charge { bytes: 0, nodes };
        self.budget.replace(Charge::default(), charge);
        self.node_room
            .set(self.budget.nodes_left(Charge::default()));
    }

    /// What the live frame documents cost together.
    pub(crate) fn charged(&self) -> Charge {
        self.budget.total()
    }

    /// The frame `id`; `None` if it was destroyed.
    pub(crate) fn get(&self, id: FrameId) -> Option<&Frame> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.frame.as_ref()
    }

    /// The frame `id` for changes; `None` if it was destroyed.
    pub(crate) fn get_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.frame.as_mut()
    }

    /// The top-level frame.
    pub(crate) fn top(&self) -> &Frame {
        self.get(FrameId::TOP)
            .expect("the top-level frame is never destroyed")
    }

    /// The top-level frame for changes.
    pub(crate) fn top_mut(&mut self) -> &mut Frame {
        self.get_mut(FrameId::TOP)
            .expect("the top-level frame is never destroyed")
    }

    /// The number of frames besides the top-level one.
    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// Adds a child frame of `parent` for `container` with `doc`, at
    /// position `at` of the parent's children (the tree order of the
    /// containers, [`Frames::child_position`]). `None` if `parent` is
    /// stale.
    pub(crate) fn insert(
        &mut self,
        parent: FrameId,
        container: NodeId,
        doc: Box<DocumentState>,
        at: usize,
    ) -> Option<FrameId> {
        let depth = self.get(parent)?.depth + 1;
        let client = doc.client;
        let mut frame = Frame::new(Some(parent), Some(container), depth);
        frame.doc = Some(doc);
        let id = if let Some(index) = self.free.pop() {
            let slot = self.slots.get_mut(index as usize)?;
            slot.frame = Some(frame);
            FrameId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = u32::try_from(self.slots.len()).ok()?;
            if index >= 1 << INDEX_BITS {
                // Each slot reached the last generation: about 2 million
                // frames per slot in one arena.
                log::warn!("the page has made too many frames; a further iframe gets no frame");
                return None;
            }
            self.slots.push(Slot {
                generation: 0,
                frame: Some(frame),
            });
            FrameId {
                index,
                generation: 0,
            }
        };
        self.count += 1;
        self.count_cell.set(self.count);
        self.clients.insert(client, id);
        self.containers.insert((parent, container), id);
        let record = self.get_mut(id)?;
        if let Some(doc) = record.doc.as_deref_mut() {
            doc.frame_id = id;
        }
        let children = &mut self.get_mut(parent)?.children;
        children.insert(at.min(children.len()), id);
        Some(id)
    }

    /// The position of a new child of `parent` whose container is
    /// `container`, in the tree order of the containers (`precedes(a, b)`:
    /// container `a` comes before `b` in the parent's document). Most
    /// frames are added in tree order, so the search starts at the end.
    pub(crate) fn child_position(
        &self,
        parent: FrameId,
        container: NodeId,
        precedes: impl Fn(NodeId, NodeId) -> bool,
    ) -> usize {
        let Some(frame) = self.get(parent) else {
            return 0;
        };
        frame
            .children
            .iter()
            .rposition(|&c| {
                self.get(c)
                    .and_then(|f| f.container)
                    .is_some_and(|n| precedes(n, container))
            })
            .map_or(0, |i| i + 1)
    }

    /// Removes frame `id` (not the top-level one) and returns its record;
    /// its children stay in the arena (the caller removes them first).
    pub(crate) fn remove(&mut self, id: FrameId) -> Option<Frame> {
        if id == FrameId::TOP {
            return None;
        }
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let frame = slot.frame.take()?;
        slot.generation += 1;
        if slot.generation < MAX_GENERATION {
            self.free.push(id.index);
        }
        self.count -= 1;
        self.count_cell.set(self.count);
        self.budget.release(frame.charge);
        self.node_room
            .set(self.budget.nodes_left(Charge::default()));
        if let Some(doc) = &frame.doc {
            self.clients.remove(&doc.client);
        }
        if let Some(PendingNavigation::Request { request, .. }) = &frame.pending {
            self.requests.remove(request);
        }
        if let (Some(parent), Some(container)) = (frame.parent, frame.container) {
            self.containers.remove(&(parent, container));
            if let Some(p) = self.get_mut(parent) {
                p.children.retain(|&c| c != id);
            }
        }
        Some(frame)
    }

    /// The frame of `container` in the document of `parent`.
    pub(crate) fn by_container(&self, parent: FrameId, container: NodeId) -> Option<FrameId> {
        self.containers.get(&(parent, container)).copied()
    }

    /// The frame whose document has loader client `client`.
    pub(crate) fn by_client(&self, client: ClientId) -> Option<FrameId> {
        self.clients.get(&client).copied()
    }

    /// Records that `frame`'s document now has client `client` (a new
    /// document replaced the old one, whose client goes).
    pub(crate) fn set_client(&mut self, frame: FrameId, old: Option<ClientId>, client: ClientId) {
        if let Some(old) = old {
            self.clients.remove(&old);
        }
        self.clients.insert(client, frame);
    }

    /// Records the request of `frame`'s document.
    pub(crate) fn add_request(&mut self, request: RequestId, frame: FrameId) {
        self.requests.insert(request, frame);
    }

    /// The frame whose document request is `request`; the entry goes.
    pub(crate) fn take_request(&mut self, request: RequestId) -> Option<FrameId> {
        self.requests.remove(&request)
    }

    /// Forgets a request of a frame document (cancelled).
    pub(crate) fn forget_request(&mut self, request: RequestId) {
        self.requests.remove(&request);
    }

    /// The descendants of `id` (without `id`), each before its parent
    /// (post-order), so that a frame is destroyed after its children.
    pub(crate) fn descendants_post_order(&self, id: FrameId) -> Vec<FrameId> {
        let mut out = Vec::new();
        let mut stack: Vec<(FrameId, bool)> = self
            .get(id)
            .map(|f| f.children.iter().rev().map(|&c| (c, false)).collect())
            .unwrap_or_default();
        while let Some((frame, expanded)) = stack.pop() {
            if expanded {
                out.push(frame);
                continue;
            }
            stack.push((frame, true));
            if let Some(f) = self.get(frame) {
                stack.extend(f.children.iter().rev().map(|&c| (c, false)));
            }
        }
        out
    }

    /// The top-level frame's origin, which is the origin of the runtime
    /// until feature 13 gives other origins runtimes of their own.
    pub(crate) fn top_origin(&self) -> Option<&Origin> {
        self.top().origin.as_ref()
    }

    /// True if frame `id`'s document has the origin of the runtime.
    pub(crate) fn in_agent(&self, id: FrameId) -> bool {
        let (Some(frame), Some(top)) = (self.get(id), self.top_origin()) else {
            return false;
        };
        frame.origin.as_ref() == Some(top)
    }

    /// The frames from the top level down to `frame`.
    pub(crate) fn chain(&self, frame: FrameId) -> Vec<FrameId> {
        let mut chain = vec![frame];
        let mut at = frame;
        while let Some(parent) = self.get(at).and_then(|f| f.parent) {
            chain.push(parent);
            at = parent;
            if chain.len() > MAX_DEPTH as usize + 2 {
                break;
            }
        }
        chain.reverse();
        chain
    }

    /// The site for cookies of the requests of a document of origin `own` in
    /// a child of `parent` (RFC 6265bis §5.2.1; ADR 0031 section 3), or of
    /// the request of a frame document of `parent` (`own` is `None`): the
    /// top-level origin if `own` and the documents of `parent` and its
    /// ancestors are same-site with it, else an opaque origin.
    pub(crate) fn site_for_cookies(&self, parent: FrameId, own: Option<&Origin>) -> Origin {
        let Some(top) = self.top_origin() else {
            return Origin::new_opaque();
        };
        let mut same = own.is_none_or(|o| same_site(o, top));
        let mut frame = Some(parent);
        while same && let Some(id) = frame {
            if id == FrameId::TOP {
                break;
            }
            same = self
                .get(id)
                .and_then(|f| f.origin.as_ref())
                .is_some_and(|o| same_site(o, top));
            frame = self.get(id).and_then(|f| f.parent);
        }
        if same {
            top.clone()
        } else {
            Origin::new_opaque()
        }
    }

    /// True if the document of `source` may navigate frame `target` (HTML
    /// "allowed by sandboxing to navigate", and the origin rule of Chromium;
    /// measured, cases `input-target-*`):
    ///
    /// - itself and its descendants, always;
    /// - a sandboxed frame: else only the top level, with a token that
    ///   `sandbox_allows` accepts (also from a nested frame, and the flags go
    ///   down to frames without `sandbox`);
    /// - else a frame whose document, or the document of one of whose
    ///   ancestors, has the origin of `source`. So a frame with the top
    ///   level's origin may navigate any frame, and a cross-origin frame may
    ///   not navigate its parent.
    pub(crate) fn may_navigate(
        &self,
        source: FrameId,
        target: FrameId,
        sandbox_allows: fn(Sandbox) -> bool,
    ) -> bool {
        let chain = self.chain(target);
        if chain.contains(&source) {
            return true;
        }
        let sandbox = self.get(source).map(|f| f.sandbox).unwrap_or_default();
        if sandbox.sandboxed {
            return target == FrameId::TOP && sandbox_allows(sandbox);
        }
        let Some(origin) = self.get(source).and_then(|f| f.origin.as_ref()) else {
            return false;
        };
        chain
            .iter()
            .any(|&id| self.get(id).and_then(|f| f.origin.as_ref()) == Some(origin))
    }

    /// All frames in tree order (pre-order, top first).
    pub(crate) fn tree_order(&self) -> Vec<FrameId> {
        let mut out = Vec::new();
        let mut stack = vec![FrameId::TOP];
        while let Some(frame) = stack.pop() {
            out.push(frame);
            if let Some(f) = self.get(frame) {
                stack.extend(f.children.iter().rev().copied());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
