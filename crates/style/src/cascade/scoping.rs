//! Cascade contexts of shadow trees (CSS Scoping 1, CSS Cascade 5 §6.1; ADR
//! 0036 section 5): the matching of the rules of the stylists of an element's
//! own shadow tree, of the slots it is assigned to, of the `:host` rules of
//! its shadow root and of the `::part()` rules of the tree around its host,
//! each with the number that orders its cascade context. The rest of the
//! cascade is in `cascade.rs`.

use super::{
    Arc, DomElement, ElementData, MatchingContext, NodeId, RuleSet, RuleTarget, ScopeIndex, Styler,
    Stylist,
};

/// The evaluated `@media` conditions and the layer ranks of a scoped
/// stylist.
pub(super) struct ScopeCascade {
    pub(super) media: Vec<bool>,
    pub(super) ranks: Arc<[u32]>,
}

/// The stylist of a [`ScopedMatch`] that is the document's.
pub(super) const DOCUMENT_STYLIST: ScopeIndex = ScopeIndex::MAX;

/// The most slots, each assigned to the next, whose `::slotted()` rules an
/// element gets: the first slots of the chain. A chain of N slots passed on
/// through N nested trees would otherwise cost O(N²) in a style pass
/// (hostile case `shadow-slotted-passthrough-10000`); the bound equals the
/// box tree's depth limit (`MAX_BOX_DEPTH`), below which such content is
/// flattened anyway.
const MAX_SLOT_CHAIN: usize = 256;

/// A matched author rule of a shadow tree's stylist, or a `::part()` rule of
/// the document's, with its cascade context (CSS Cascade 5 §6.1; ADR 0036
/// section 5): a number that orders the contexts of one element by
/// shadow-including tree order of their roots (CSS Scoping 1 §3.1), so that
/// a smaller one is an outer context: the tree around the element's own
/// (`::part()` rules), its own tree, then the trees of the slots that it is
/// assigned to, its assigned slot's first (each later one is nested in the
/// one before), then its own shadow tree (`:host` rules).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ScopedMatch {
    pub(super) context: u32,
    pub(super) stylist: ScopeIndex,
    pub(super) rule: u32,
    /// The rank of the rule's layer in its stylist.
    pub(super) rank: u32,
}

impl Styler<'_> {
    /// The nesting depth of the tree scope `scope`: 0 for the document (and
    /// for the root of a detached tree), 1 for the shadow root of a host in
    /// the document, and so on. Memoized, so that a walk down nested trees
    /// costs linear time.
    pub(super) fn depth(&mut self, scope: NodeId) -> u32 {
        let doc = self.doc;
        let mut chain = Vec::new();
        let mut current = scope;
        let base = loop {
            if current == NodeId::DOCUMENT {
                break 0;
            }
            if let Some(&depth) = self.depths.get(&current) {
                break depth;
            }
            match doc.shadow_host(current) {
                Some(host) => {
                    chain.push(current);
                    current = doc.root(host);
                }
                None => break 0,
            }
        };
        let mut depth = base;
        for &root in chain.iter().rev() {
            depth = depth.saturating_add(1);
            self.depths.insert(root, depth);
        }
        depth
    }

    /// Makes the evaluated media and layer ranks of the scoped stylist
    /// `index` at its first use.
    pub(super) fn ensure_scope_cascade(&mut self, index: ScopeIndex) {
        let Some(stylist) = self.cx.stylist.scope(index) else {
            return;
        };
        let slot = index as usize;
        if self.scope_cascades.len() <= slot {
            self.scope_cascades.resize_with(slot + 1, || None);
        }
        if let Some(entry) = self.scope_cascades.get_mut(slot)
            && entry.is_none()
        {
            let media = stylist.evaluate_media(self.cx.env);
            let ranks = stylist.layer_ranks(&media);
            *entry = Some(ScopeCascade { media, ranks });
        }
    }

    /// Appends to `out` the rules of `set` of the stylist `index` that
    /// match, by `test`, with the cascade context `context`. `key` is the
    /// element whose keys the rules' buckets are looked up by.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn collect_scoped(
        &mut self,
        index: ScopeIndex,
        set: RuleSet,
        target: RuleTarget,
        key: &DomElement<'_>,
        context: u32,
        mut test: impl FnMut(&swb_css::Selector, &mut MatchingContext) -> bool,
        out: &mut Vec<ScopedMatch>,
    ) {
        let (stylist, media, ranks): (&Stylist, &[bool], &[u32]) = if index == DOCUMENT_STYLIST {
            (self.cx.stylist, &self.media, &self.cx.layer_ranks)
        } else {
            self.ensure_scope_cascade(index);
            match (
                self.cx.stylist.scope(index),
                self.scope_cascades
                    .get(index as usize)
                    .and_then(Option::as_ref),
            ) {
                (Some(stylist), Some(c)) => (stylist, &c.media, &c.ranks),
                _ => return,
            }
        };
        self.found.clear();
        let matching = &mut self.matching;
        stylist.collect(
            set,
            target,
            key,
            media,
            self.filter.as_ref(),
            &mut self.scratch,
            |selector| test(selector, matching),
            &mut self.containers,
            &mut self.found,
        );
        out.extend(self.found.iter().map(|&rule| {
            ScopedMatch {
                context,
                stylist: index,
                rule,
                rank: ranks
                    .get(stylist.rule(rule).layer as usize)
                    .copied()
                    .unwrap_or(0),
            }
        }));
    }

    /// Appends to `out` the rules of shadow trees, and the document's
    /// `::part()` rules, that match `el` (the element being styled, of the
    /// tree scope `self.scope`) or its pseudo-element `target`: steps 2 to 5
    /// of ADR 0036 section 5. The element's own tree's rules for an element
    /// of the document are the document stylist's (step 1).
    pub(super) fn match_scoped(
        &mut self,
        el: &DomElement<'_>,
        target: RuleTarget,
        out: &mut Vec<ScopedMatch>,
    ) {
        let doc = self.doc;
        let states = self.states;
        let stylist = self.cx.stylist;
        let scope = self.scope;
        let own = self.depth(scope);
        let node = el.node_id();
        let handle = |id: Option<NodeId>| id.and_then(|id| DomElement::new(doc, id, states));
        let host = handle(doc.shadow_host(scope));
        if let Some(host) = &host {
            // The rules of the element's own shadow tree.
            if let Some(index) = stylist.scope_of(scope) {
                self.collect_scoped(
                    index,
                    RuleSet::Tree,
                    target,
                    el,
                    own,
                    |s, cx| swb_css::matches_in_tree(s, el, Some(host), cx),
                    out,
                );
            }
            // `::part()` rules of the tree around the host, and `:host::part()`
            // rules of the element's own tree.
            if el.data().has_attr("part") {
                let outer = doc.root(host.node_id());
                let outer_host = handle(doc.shadow_host(outer));
                let outer_index = if outer == NodeId::DOCUMENT {
                    Some(DOCUMENT_STYLIST)
                } else {
                    stylist.scope_of(outer)
                };
                if let Some(index) = outer_index {
                    self.collect_scoped(
                        index,
                        RuleSet::Part,
                        target,
                        el,
                        own.saturating_sub(1),
                        |s, cx| swb_css::matches_part(s, el, host, outer_host.as_ref(), cx),
                        out,
                    );
                }
                if let Some(index) = stylist.scope_of(scope) {
                    self.collect_scoped(
                        index,
                        RuleSet::Part,
                        target,
                        el,
                        own,
                        |s, cx| swb_css::matches_part(s, el, host, Some(host), cx),
                        out,
                    );
                }
            }
        }
        let mut context = own.saturating_add(1);
        // `::slotted()` rules of the trees of the slots that the element is
        // assigned to: its slot, the slot that slot is assigned to, and so on
        // (measured: a nested slot's rules match, `slotted-nested-cascade`).
        let mut current = node;
        for _ in 0..MAX_SLOT_CHAIN {
            let assigned = doc
                .parent_element(current)
                .and_then(|p| doc.element(p))
                .is_some_and(ElementData::is_shadow_host);
            let Some(slot) = assigned.then(|| doc.assigned_slot(current)).flatten() else {
                break;
            };
            let root = doc.root(slot);
            if let (Some(index), Some(slot_el)) = (stylist.scope_of(root), handle(Some(slot))) {
                let slot_host = handle(doc.shadow_host(root));
                self.collect_scoped(
                    index,
                    RuleSet::Slotted,
                    target,
                    el,
                    context,
                    |s, cx| swb_css::matches_slotted(s, el, &slot_el, slot_host.as_ref(), cx),
                    out,
                );
            }
            context = context.saturating_add(1);
            current = slot;
        }
        // `:host` rules of the element's own shadow tree.
        if el.data().is_shadow_host()
            && let Some(root) = doc.shadow_root(node)
            && let Some(index) = stylist.scope_of(root)
        {
            self.collect_scoped(
                index,
                RuleSet::Host,
                target,
                el,
                context,
                |s, cx| swb_css::matches_in_tree(s, el, Some(el), cx),
                out,
            );
        }
    }
}
