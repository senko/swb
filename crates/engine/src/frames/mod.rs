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
    /// below 2^30, and unique over the life of the arena, because a slot whose
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
const MAX_GENERATION: u32 = 1 << (30 - INDEX_BITS);

impl std::fmt::Display for FrameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frame {}.{}", self.index, self.generation)
    }
}

/// An agent of the page: a script runtime (ADR 0031 section 5). A number is not
/// used twice in a page, so a stale one finds no runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct AgentId(u32);

impl AgentId {
    /// The agent with the number `number`.
    pub(crate) fn new(number: u32) -> AgentId {
        AgentId(number)
    }
}

/// The place of a document in the page's runtimes: an agent and a key, which
/// mean something only together. The key comes out only for its agent, so that
/// no code can use it with another runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameScript {
    agent: AgentId,
    key: DocumentKey,
}

impl FrameScript {
    /// The document `key` of the runtime of `agent`.
    pub(crate) fn new(agent: AgentId, key: DocumentKey) -> FrameScript {
        FrameScript { agent, key }
    }

    /// The agent of the document.
    pub(crate) fn agent(self) -> AgentId {
        self.agent
    }

    /// The key of the document in the runtime of `agent`; `None` if the document
    /// is in another runtime.
    pub(crate) fn key_in(self, agent: AgentId) -> Option<DocumentKey> {
        (self.agent == agent).then_some(self.key)
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
    /// The site of the current document for storage keys ([`storage_site_of`]).
    pub(crate) storage_site: Option<Rc<str>>,
    /// The URL of the document that started the navigation that waits for its document,
    /// for `document.referrer`; `None`: the parent's (ADR 0031 "Done (feature 14b)").
    pub(crate) pending_source: Option<Url>,
    /// The runtime of the current document and its key there, once it has a
    /// realm (ADR 0031 section 5).
    pub(crate) script: Option<FrameScript>,
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
    /// The sandboxing flags at a script's write to the `location` of this frame:
    /// the page starts that navigation after the task, and it has the flags of the
    /// write (measured, case `xo-navigate-flags-at-the-write`). With the generation
    /// of the parent's DOM at the parse, so that a loop of writes parses once.
    pub(crate) location_sandbox: Option<(u64, Sandbox)>,
    /// When the frame last got a user's activation ([`Frames::note_activation`]). It
    /// stays through a commit of the same origin, and a commit of another origin
    /// clears it (measured, cases `activation-xo-after-commit*`).
    pub(crate) activation: Option<std::time::Instant>,
    /// The document. For the top-level frame the page holds it (`Page::doc`), and
    /// it is here only during a task of another runtime than the top level's,
    /// which needs it for layout across documents (ADR 0031 section 5).
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

/// The site of a document for storage keys: the scheme and registrable domain of its
/// origin, or `file://` for a `file:` document (all of them share one area, as one site).
/// `None` for a document whose origin is opaque otherwise, and for a document whose
/// sandbox has no `allow-same-origin`: it has no storage. `storage_url` is the URL that
/// gives the document its storage (the creator's for an `about:` document with its
/// creator's origin).
pub(crate) fn storage_site_of(
    origin: &Origin,
    storage_url: &Url,
    sandboxed: bool,
) -> Option<Rc<str>> {
    if sandboxed {
        return None;
    }
    if let Some(site) = swb_net::site_text(origin) {
        return Some(Rc::from(site));
    }
    (storage_url.scheme() == "file").then(|| Rc::from("file://"))
}

impl Frame {
    fn new(parent: Option<FrameId>, container: Option<NodeId>, depth: u32) -> Frame {
        Frame {
            parent,
            container,
            children: Vec::new(),
            name: String::new(),
            origin: None,
            storage_site: None,
            pending_source: None,
            script: None,
            reuse_window: false,
            resized: false,
            sandbox: Sandbox::default(),
            pending_sandbox: Sandbox::default(),
            location_sandbox: None,
            activation: None,
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

    /// True if the current document of the frame is the document `key` of the
    /// runtime of `agent`.
    pub(crate) fn has_document(&self, agent: AgentId, key: DocumentKey) -> bool {
        self.script.and_then(|s| s.key_in(agent)) == Some(key)
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
    /// The nodes of the retired documents, by runtime: they stay in the budget
    /// while their runtime lives.
    retired_nodes: HashMap<AgentId, usize>,
    /// The frame that holds the page's focus: what `document.hasFocus()` and a
    /// script's `focus()` read during a task.
    pub(crate) focused: FrameId,
}

/// How long a user's input counts as a transient activation (HTML "transient
/// activation duration"; Chromium's five seconds).
const ACTIVATION: std::time::Duration = std::time::Duration::from_secs(5);

impl Frames {
    /// The user pressed a button or a key in the document of `frame` (HTML
    /// "activation notification"). The frame, its ancestors and the frames of its
    /// origin get the activation. Chromium gives it to all frames of the origin,
    /// not only to descendants (measured, case `activation-xo-sibling-same`).
    pub(crate) fn note_activation(&mut self, frame: FrameId) {
        let now = std::time::Instant::now();
        let origin = self.get(frame).and_then(|f| f.origin.clone());
        for id in self.chain(frame) {
            if let Some(record) = self.get_mut(id) {
                record.activation = Some(now);
            }
        }
        let Some(origin) = origin else {
            return;
        };
        for id in self.tree_order() {
            if let Some(record) = self.get_mut(id)
                && record.origin.as_ref() == Some(&origin)
            {
                record.activation = Some(now);
            }
        }
    }

    /// True if `frame` has had an activation (HTML "sticky activation").
    fn has_sticky_activation(&self, frame: FrameId) -> bool {
        self.get(frame).is_some_and(|f| f.activation.is_some())
    }

    /// True if the last activation of `frame` is recent (HTML "transient
    /// activation"): a sandbox's `allow-top-navigation-by-user-activation` allows
    /// a script's navigation then (measured: also in a timer, also after a click
    /// in a parent of the same origin).
    pub(crate) fn has_transient_activation(&self, frame: FrameId) -> bool {
        self.get(frame)
            .and_then(|f| f.activation)
            .is_some_and(|time| time.elapsed() <= ACTIVATION)
    }

    /// A tree with the top-level frame only.
    pub(crate) fn new() -> Frames {
        Frames {
            slots: vec![Slot {
                generation: 0,
                frame: Some(Frame::new(None, None, 0)),
            }],
            free: Vec::new(),
            count: 0,
            focused: FrameId::TOP,
            clients: HashMap::new(),
            requests: HashMap::new(),
            containers: HashMap::new(),
            count_warned: false,
            depth_warned: false,
            budget: Budget::default(),
            count_cell: Rc::default(),
            node_room: Rc::new(Cell::new(MAX_NODES)),
            retired_nodes: HashMap::new(),
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
        self.retired_nodes.clear();
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

    /// A retired document (ADR 0031 section 5) stays in memory while its
    /// runtime lives, so its nodes stay in the budget until then. Its source text
    /// is gone.
    pub(crate) fn charge_retired(&mut self, agent: AgentId, nodes: usize) {
        let charge = Charge { bytes: 0, nodes };
        self.budget.replace(Charge::default(), charge);
        *self.retired_nodes.entry(agent).or_default() += nodes;
        self.node_room
            .set(self.budget.nodes_left(Charge::default()));
    }

    /// The runtime of `agent` has gone, with its retired documents.
    pub(crate) fn release_retired(&mut self, agent: AgentId) {
        if let Some(nodes) = self.retired_nodes.remove(&agent) {
            self.budget.release(Charge { bytes: 0, nodes });
            self.node_room
                .set(self.budget.nodes_left(Charge::default()));
        }
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

    /// The top-level frame's origin.
    pub(crate) fn top_origin(&self) -> Option<&Origin> {
        self.top().origin.as_ref()
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

    /// The partition of the storage key of a document of site `own` in a child of
    /// `parent` (ADR 0031 section 3, "Done (feature 14b)"): the key is the origin alone
    /// if the top-level document, the ancestors and the document are all of one site,
    /// else the origin and the site of the top-level document. Without a site of the
    /// document or of the top level the key cannot be computed.
    pub(crate) fn storage_partition(
        &self,
        parent: FrameId,
        own: Option<&str>,
    ) -> swb_web::StoragePartition {
        use swb_web::StoragePartition;
        let (Some(own), Some(top)) = (own, self.top().storage_site.clone()) else {
            return StoragePartition::Unavailable;
        };
        let mut frame = Some(parent);
        let mut first_party = &*top == own;
        while first_party && let Some(id) = frame {
            if id == FrameId::TOP {
                break;
            }
            first_party = self
                .get(id)
                .and_then(|f| f.storage_site.as_deref())
                .is_some_and(|site| site == own);
            frame = self.get(id).and_then(|f| f.parent);
        }
        if first_party {
            StoragePartition::FirstParty
        } else {
            StoragePartition::Top(top)
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
        // A cross-origin frame may navigate the top level after an activation of
        // its own, also a late one (measured, cases `activation-xo-*`). Departure:
        // Chromium also allows it while the top level has had no activation at all
        // (`xo-top-fresh-*`); the owner's decision in ADR 0031 refuses that.
        if target == FrameId::TOP && self.has_sticky_activation(source) {
            return true;
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
