//! The drain of the change journal (ADR 0028 sections 3 and 9): resource
//! discovery, the removal of the sheets and images of removed elements,
//! invalidation, and the derived data that the page reads without `&mut`
//! (the title and the base URL).
//!
//! A complete journal lists each change since the last drain. The drain
//! collects the elements that matter for loads (`style`, `link`, `img`,
//! `video`, the `img` elements of a `picture` whose sources changed,
//! `base`, `title`) from the inserted subtrees, from attribute changes and
//! from changes of their children, and *reconciles* each one in tree
//! order: an element gets the sheet or image that its attributes ask for,
//! and keeps what it has if nothing changed (a moved `link` does not load
//! again). An incomplete journal (the parse without scripts, or an
//! overflow) reconciles every element of the document in one walk; for a
//! new document this is the walk that the engine did before JavaScript
//! support.
//!
//! The loads of one drain start in this order: stylesheets, video posters,
//! images (as the walk does). Across drains, each kind starts in the order
//! of insertion.
//!
//! Invalidation: any change marks the styles dirty (a full restyle, M8); a
//! change of a stylesheet (added, removed, moved, its text, `media` or
//! URL) or of the base URL marks the stylist dirty.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use swb_dom::{Change, Document, ElementData, Journal, NodeId, is_html_whitespace, local_name, ns};
use swb_net::Url;

use super::DocumentState;
use super::derived::{base_url, document_title};
use crate::image_source::SelectedImage;
use crate::resources::{SheetSlot, SheetSource};

/// What an element is, for resource discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// An HTML or SVG `style` element: an inline stylesheet.
    Style,
    /// An HTML `link`: maybe an external stylesheet.
    Link,
    /// An HTML `video`: its poster.
    Video,
    /// An HTML `img`.
    Img,
    /// An HTML `picture`: a change of its children selects its images
    /// again.
    Picture,
    /// An HTML `source`: a change of its attributes selects the images of
    /// its `picture` again.
    Source,
    /// An HTML `base`: the base URL.
    Base,
    /// An HTML `title`.
    Title,
}

impl Kind {
    fn of(e: &ElementData) -> Option<Kind> {
        if e.name.ns == ns!(svg) {
            // `<style>` elements of the SVG namespace are document style
            // sheets like the HTML ones (SVG 2 §6.4: they apply to the
            // whole document, also to HTML elements).
            return (*e.local_name() == local_name!("style")).then_some(Kind::Style);
        }
        if !e.is_html() {
            return None;
        }
        Some(match *e.local_name() {
            local_name!("style") => Kind::Style,
            local_name!("link") => Kind::Link,
            local_name!("video") => Kind::Video,
            local_name!("img") => Kind::Img,
            local_name!("picture") => Kind::Picture,
            local_name!("source") => Kind::Source,
            local_name!("base") => Kind::Base,
            local_name!("title") => Kind::Title,
            _ => return None,
        })
    }
}

/// The loads that one drain starts, in this order.
#[derive(Default)]
pub(super) struct LoadPlan {
    /// External stylesheets: owner, load serial, URL.
    pub(super) sheets: Vec<(NodeId, u64, Url)>,
    /// Video posters.
    pub(super) posters: Vec<Url>,
    /// The `img` elements whose sources are selected, in tree order.
    pub(super) images: Vec<NodeId>,
}

/// What the entries of one drain touched.
#[derive(Default)]
struct Batch {
    /// Elements to reconcile, each once.
    candidates: Vec<NodeId>,
    seen: HashSet<NodeId>,
    /// Elements of interest in removed subtrees.
    removed: Vec<NodeId>,
    /// `style` elements whose text may have changed.
    sheet_text: HashSet<NodeId>,
    /// True if a `base` element was inserted, removed or changed.
    base: bool,
    /// True if a `title` element or its text changed.
    title: bool,
    /// True if any node was removed.
    removal: bool,
    /// True if a removed subtree has a `style` or `link` element.
    sheet_removed: bool,
    /// True if a change was in the document (not in a detached subtree or
    /// in template contents): only those can change the styles.
    in_document: bool,
}

impl Batch {
    fn add(&mut self, node: NodeId) {
        if self.seen.insert(node) {
            self.candidates.push(node);
        }
    }

    fn note(&mut self, node: NodeId, kind: Kind, removed: bool) {
        match kind {
            Kind::Base => self.base = true,
            Kind::Title => self.title = true,
            _ => {}
        }
        if removed {
            self.sheet_removed |= matches!(kind, Kind::Style | Kind::Link);
            self.removed.push(node);
        } else {
            self.add(node);
        }
    }
}

impl DocumentState {
    /// Drains the journal: starts the loads of inserted and changed
    /// elements, drops the sheets and images of removed ones, marks the
    /// styles (and for stylesheet changes the stylist) dirty, and keeps the
    /// title and the base URL current. Does nothing if the DOM did not
    /// change since the last drain.
    pub fn process_changes(&mut self) {
        let generation = self.dom.generation();
        if self.drained == Some(generation) && self.dom.journal().is_empty() {
            return;
        }
        let journal = self.dom.take_journal();
        self.drained = Some(generation);
        if journal.incomplete() {
            self.rediscover();
        } else if !journal.is_empty() {
            self.apply_journal(&journal);
        }
    }

    /// Reconciles every element of the document (an incomplete journal).
    fn rediscover(&mut self) {
        #[cfg(test)]
        {
            self.walks += 1;
        }
        let generation = self.dom.generation();
        let (dom, url) = (&self.dom, &self.url);
        self.base_url.rebuild(generation, |_| base_url(dom, url));
        self.title.rebuild(generation, |_| document_title(dom));
        let mut owners = Vec::new();
        let mut videos = Vec::new();
        for node in dom.descendants(NodeId::DOCUMENT) {
            match dom.element(node).and_then(Kind::of) {
                Some(Kind::Style | Kind::Link) => owners.push(node),
                Some(Kind::Video) => videos.push(node),
                _ => {}
            }
        }
        let mut plan = LoadPlan::default();
        let mut old: HashMap<NodeId, SheetSlot> = self
            .sheets
            .take()
            .into_iter()
            .map(|s| (s.owner, s))
            .collect();
        for owner in owners {
            let (slot, _) = self.wanted_sheet(owner, old.remove(&owner), &mut plan);
            if let Some(slot) = slot {
                self.sheets.push(slot);
            }
        }
        for node in videos {
            self.reconcile_poster(node, &mut plan);
        }
        let dom = &self.dom;
        self.images.by_node.retain(|&n, _| in_document(dom, n));
        self.start_plan(plan, true);
        self.clean_up_states();
        self.invalidate_style();
    }

    /// Applies the entries of a complete journal.
    fn apply_journal(&mut self, journal: &Journal) {
        let mut batch = Batch::default();
        let mut visited = HashSet::new();
        let mut removed_visited = HashSet::new();
        let mut connected = Connected::new(&self.dom);
        for change in journal.iter() {
            if !batch.in_document {
                let target = match &change {
                    Change::Children { parent, .. } => *parent,
                    Change::Attribute { element, .. } => *element,
                    Change::Text { node, .. } => *node,
                };
                batch.in_document = connected.check(target);
            }
            match change {
                Change::Children {
                    parent,
                    added,
                    removed,
                    ..
                } => {
                    for &root in added {
                        self.collect_subtree(root, &mut visited, &mut batch, false);
                    }
                    batch.removal |= !removed.is_empty();
                    for &root in removed {
                        self.collect_subtree(root, &mut removed_visited, &mut batch, true);
                    }
                    self.children_changed(parent, &mut batch);
                }
                Change::Attribute { element, .. } => {
                    if let Some(kind) = self.dom.element(element).and_then(Kind::of) {
                        if kind == Kind::Source {
                            self.picture_images(element, &mut batch);
                        }
                        batch.note(element, kind, false);
                    }
                }
                Change::Text { node, .. } => {
                    if let Some(parent) = self.dom.parent(node) {
                        self.children_changed(parent, &mut batch);
                    }
                }
            }
        }
        self.finish_batch(batch);
    }

    /// Notes the elements of interest in the subtree of `root`, as it is
    /// now. A subtree that this drain walked before is skipped: the DOM is
    /// in its final state, so a walk of an ancestor saw it already.
    fn collect_subtree(
        &self,
        root: NodeId,
        visited: &mut HashSet<NodeId>,
        batch: &mut Batch,
        removed: bool,
    ) {
        if !visited.insert(root) {
            return;
        }
        let dom = &self.dom;
        // Pre-order, with an explicit stack. A node that an earlier walk
        // visited is skipped with its subtree: a chain that a script built
        // bottom-up has one entry per level, and walking the whole subtree
        // again at each level would be quadratic.
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if let Some(kind) = dom.element(node).and_then(Kind::of) {
                batch.note(node, kind, removed);
            }
            let first = stack.len();
            for child in dom.children(node) {
                if visited.insert(child) {
                    stack.push(child);
                }
            }
            stack[first..].reverse();
        }
    }

    /// Notes a change of the children (or of the text of a child) of
    /// `parent`: the text of a `style` or `title` element, the sources of a
    /// `picture`. The text of an element counts for its parent; inside SVG
    /// elements also for the SVG ancestors (an SVG `style` can have element
    /// children). The walk stops at the first other element, so that a
    /// change deep in a tree does not walk all its ancestors.
    fn children_changed(&self, parent: NodeId, batch: &mut Batch) {
        let dom = &self.dom;
        let mut node = parent;
        loop {
            let element = dom.element(node);
            match element.and_then(Kind::of) {
                Some(Kind::Style) => {
                    batch.sheet_text.insert(node);
                }
                Some(Kind::Title) => batch.title = true,
                Some(Kind::Picture) if node == parent => self.picture_images(node, batch),
                _ => {}
            }
            match dom.parent(node) {
                Some(next) if element.is_some_and(|e| e.name.ns == ns!(svg)) => node = next,
                _ => break,
            }
        }
    }

    /// Adds the `img` children of the `picture` that is `node` or the
    /// parent of `node` (a `source`).
    fn picture_images(&self, node: NodeId, batch: &mut Batch) {
        let dom = &self.dom;
        let is_picture = |n: NodeId| dom.is_html_element(n, &local_name!("picture"));
        let picture = if is_picture(node) {
            Some(node)
        } else {
            dom.parent(node).filter(|&p| is_picture(p))
        };
        let Some(picture) = picture else {
            return;
        };
        for child in dom.element_children(picture) {
            if dom.is_html_element(child, &local_name!("img")) {
                batch.add(child);
            }
        }
    }

    /// Reconciles the elements that a drain touched, in tree order, then
    /// starts their loads and invalidates.
    fn finish_batch(&mut self, mut batch: Batch) {
        let mut stylist_dirty = self.refresh_base_and_title(&batch);
        batch.candidates.retain(|&n| in_document(&self.dom, n));
        sort_in_tree_order(&self.dom, &mut batch.candidates);
        let mut plan = LoadPlan::default();
        // The owners of the sheets that were placed in the list (new, or
        // reconciled because they were inserted or changed).
        let mut placed = Vec::new();
        for &node in &batch.candidates {
            match self.dom.element(node).and_then(Kind::of) {
                Some(Kind::Style | Kind::Link) => {
                    let index = self.sheets.position(node);
                    let old = index.map(|i| self.sheets.remove(i));
                    let had = old.is_some();
                    let (slot, changed) = self.wanted_sheet(node, old, &mut plan);
                    stylist_dirty |= changed || (had && slot.is_none());
                    if let Some(slot) = slot {
                        // A moved element is sorted below.
                        let at = index.unwrap_or(self.sheets.len());
                        self.sheets.insert(at, slot);
                        placed.push(node);
                    }
                    batch.sheet_text.remove(&node);
                }
                Some(Kind::Video) => self.reconcile_poster(node, &mut plan),
                Some(Kind::Img) => plan.images.push(node),
                _ => {}
            }
        }
        // The text of inline sheets that were not reconciled above.
        for owner in batch.sheet_text {
            stylist_dirty |= self.refresh_sheet_text(owner);
        }
        if batch.removal {
            if batch.sheet_removed {
                let before = self.sheets.len();
                let dom = &self.dom;
                self.sheets.retain(|s| in_document(dom, s.owner));
                stylist_dirty |= self.sheets.len() != before;
            }
            for node in batch.removed {
                if !in_document(&self.dom, node) {
                    self.forget_image(node);
                }
            }
            self.clean_up_states();
        }
        if !self.placed_in_order(&placed) {
            let dom = &self.dom;
            self.sheets
                .sort_by(|a, b| tree_order_cmp(dom, a.owner, b.owner));
            stylist_dirty = true;
        }
        self.start_plan(plan, false);
        if stylist_dirty {
            self.invalidate_style();
        } else if batch.in_document {
            self.invalidate_styles();
        }
    }

    /// True if the sheets of `placed` are in tree order with their
    /// neighbours in the list. The list was in tree order before the drain,
    /// and every sheet whose owner moved was placed again, so this is true
    /// if the whole list is in tree order. (A check of the neighbours only
    /// keeps a parse that adds one sheet per step linear.)
    fn placed_in_order(&self, placed: &[NodeId]) -> bool {
        let dom = &self.dom;
        let sheets = &self.sheets;
        placed.iter().all(|&owner| {
            let Some(i) = sheets.position(owner) else {
                return true;
            };
            let before = i.checked_sub(1).and_then(|j| sheets.get(j));
            before.is_none_or(|b| dom.precedes(b.owner, owner))
                && sheets
                    .get(i + 1)
                    .is_none_or(|a| dom.precedes(owner, a.owner))
        })
    }

    /// Brings the base URL and the title up to date after a drain. Returns
    /// true if the base URL changed (inline sheets resolve against it).
    fn refresh_base_and_title(&mut self, batch: &Batch) -> bool {
        let generation = self.dom.generation();
        let dom = &self.dom;
        let mut changed = false;
        if batch.base || self.base_url.is_unbuilt() {
            let old = self.base_url.value().clone();
            let url = &self.url;
            self.base_url.rebuild(generation, |_| base_url(dom, url));
            if *self.base_url.value() != old {
                let base = self.base_url.value();
                for slot in self.sheets.iter_mut() {
                    if slot.source == SheetSource::Inline {
                        slot.base_url = base.clone();
                    }
                }
                changed = true;
            }
        } else {
            self.base_url.confirm(generation);
        }
        if batch.title || self.title.is_unbuilt() {
            self.title.rebuild(generation, |_| document_title(dom));
        } else {
            self.title.confirm(generation);
        }
        changed
    }

    /// The sheet that element `owner` (a `style` or `link`) asks for,
    /// given its current one `old`, and whether it differs from `old`: an
    /// unchanged sheet is `old` itself, a new external sheet is added to
    /// `plan`.
    fn wanted_sheet(
        &mut self,
        owner: NodeId,
        old: Option<SheetSlot>,
        plan: &mut LoadPlan,
    ) -> (Option<SheetSlot>, bool) {
        let Some(element) = self.dom.element(owner) else {
            return (None, old.is_some());
        };
        let media = element.attr("media").map(str::to_owned);
        match Kind::of(element) {
            Some(Kind::Style) => {
                // Chromium (measured, `tools/probes/inline-svg.json`, case
                // `style-element`) loads only those whose `type` is empty
                // or `text/css`, in both namespaces.
                if !element.attr("type").is_none_or(is_css_type) {
                    return (None, old.is_some());
                }
                let css = self.dom.text_content(owner);
                let base_url = self.base_url.value().clone();
                match old {
                    Some(mut slot) if slot.source == SheetSource::Inline => {
                        let mut changed = slot.media != media;
                        if slot.css.as_deref() != Some(css.as_str()) {
                            slot.css = Some(css);
                            slot.parsed = None;
                            changed = true;
                        }
                        slot.media = media;
                        slot.base_url = base_url;
                        (Some(slot), changed)
                    }
                    _ => (Some(SheetSlot::inline(owner, css, base_url, media)), true),
                }
            }
            Some(Kind::Link) => {
                let url = element
                    .attr("rel")
                    .filter(|rel| is_stylesheet_rel(rel))
                    .and_then(|_| element.attr("href"))
                    .and_then(|h| self.base_url.value().join(h.trim()).ok());
                let Some(url) = url else {
                    return (None, old.is_some());
                };
                if let Some(mut slot) = old
                    && slot.source == SheetSource::Link(url.clone())
                {
                    let changed = slot.media != media;
                    slot.media = media;
                    return (Some(slot), changed);
                }
                self.sheet_serial += 1;
                let serial = self.sheet_serial;
                plan.sheets.push((owner, serial, url.clone()));
                let mut slot = SheetSlot::link(owner, url, serial, media);
                slot.blocks_scripts = self.parser_drain;
                (Some(slot), true)
            }
            _ => (None, old.is_some()),
        }
    }

    /// Reads the text of the inline sheet of `owner` again. Returns true
    /// if it changed.
    fn refresh_sheet_text(&mut self, owner: NodeId) -> bool {
        let Some(slot) = self
            .sheets
            .of(owner)
            .filter(|s| s.source == SheetSource::Inline)
        else {
            return false;
        };
        let css = self.dom.text_content(owner);
        if slot.css.as_deref() == Some(css.as_str()) {
            return false;
        }
        slot.css = Some(css);
        slot.parsed = None;
        true
    }

    /// Gives a `video` the poster that its `poster` attribute names. The
    /// poster loads like an image; the media resource itself is never
    /// fetched (swb plays no media).
    fn reconcile_poster(&mut self, node: NodeId, plan: &mut LoadPlan) {
        let poster = self
            .dom
            .element(node)
            .and_then(|e| e.attr("poster"))
            .map(|s| s.trim_matches(is_html_whitespace))
            .filter(|s| !s.is_empty())
            .and_then(|src| self.base_url.value().join(src).ok());
        match poster {
            Some(url) => {
                plan.posters.push(url.clone());
                self.images
                    .by_node
                    .insert(node, SelectedImage { url, density: 1.0 });
            }
            None => {
                self.images.by_node.remove(&node);
            }
        }
    }

    /// Clears the element states and the selection that refer to nodes
    /// that left the document. (The HTML focus fixup rule; the selection
    /// goes away instead of moving its boundary points.)
    fn clean_up_states(&mut self) {
        let dom = &self.dom;
        let states = &mut self.states;
        for state in [
            &mut states.hover,
            &mut states.active,
            &mut states.focus,
            &mut states.target,
        ] {
            if state.is_some_and(|n| !in_document(dom, n)) {
                *state = None;
            }
        }
        if self
            .selection
            .is_some_and(|s| !in_document(dom, s.anchor.node) || !in_document(dom, s.focus.node))
        {
            self.selection = None;
        }
    }
}

/// Answers whether nodes are in the document, with the answers for the
/// ancestors that it walked kept for the next question. A drain of many
/// entries in one detached tree (a script that builds a deep chain before
/// it inserts it) walks each ancestor once.
struct Connected<'a> {
    dom: &'a Document,
    known: HashMap<NodeId, bool>,
    path: Vec<NodeId>,
}

impl<'a> Connected<'a> {
    fn new(dom: &'a Document) -> Self {
        Connected {
            dom,
            known: HashMap::new(),
            path: Vec::new(),
        }
    }

    /// True if `node` is in the document (its root is the document node).
    fn check(&mut self, node: NodeId) -> bool {
        if self.dom.get(node).is_none() {
            return false;
        }
        self.path.clear();
        let mut current = node;
        let answer = loop {
            if let Some(&known) = self.known.get(&current) {
                break known;
            }
            self.path.push(current);
            match self.dom.parent(current) {
                Some(parent) => current = parent,
                None => break current == NodeId::DOCUMENT,
            }
        };
        for &n in &self.path {
            self.known.insert(n, answer);
        }
        answer
    }
}

/// True if `node` is in the document tree of `dom` (not detached, not in
/// another document of the arena such as template contents).
pub(super) fn in_document(dom: &Document, node: NodeId) -> bool {
    dom.get(node).is_some() && dom.root(node) == NodeId::DOCUMENT
}

/// Compares two nodes of the document tree in tree order.
fn tree_order_cmp(dom: &Document, a: NodeId, b: NodeId) -> Ordering {
    if a == b {
        Ordering::Equal
    } else if dom.precedes(a, b) {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Sorts nodes of the document tree in tree order. The parser inserts in
/// tree order, so the list is usually sorted already.
fn sort_in_tree_order(dom: &Document, nodes: &mut [NodeId]) {
    if !nodes.is_sorted_by(|&a, &b| !dom.precedes(b, a)) {
        nodes.sort_by(|&a, &b| tree_order_cmp(dom, a, b));
    }
}

/// True for a `type` of a `<style>` element that Chromium loads: empty or
/// `text/css`, ASCII case-insensitive (HTML Standard §4.2.6).
fn is_css_type(value: &str) -> bool {
    value.is_empty() || value.eq_ignore_ascii_case("text/css")
}

/// True for a `rel` value that makes a `link` a primary stylesheet:
/// `stylesheet` without `alternate`.
/// <https://html.spec.whatwg.org/multipage/links.html#link-type-stylesheet>
fn is_stylesheet_rel(rel: &str) -> bool {
    let has = |word: &str| {
        rel.split_ascii_whitespace()
            .any(|r| r.eq_ignore_ascii_case(word))
    };
    has("stylesheet") && !has("alternate")
}
