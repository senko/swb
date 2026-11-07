//! The cascade layers of the author origin (CSS Cascade 5, section 6,
//! <https://www.w3.org/TR/css-cascade-5/#layering>).
//!
//! The [`LayerTree`] has one node per layer name path (`a`, `a.b`) and one
//! per anonymous `@layer` block or `@import ... layer`. Node 0 is the root:
//! the implicit layer of the unlayered rules, which comes after all named
//! layers. Every rule of the stylist refers to the node of the layer it is
//! in.
//!
//! The order of the layers is not stored in the tree: the declarations
//! (`@layer` statements, blocks and `@import ... layer`, in document order
//! across all sheets) are kept as a list of events, and [`LayerTree::ranks`]
//! turns them into one rank per node. The ranks depend on the media
//! conditions: a declaration inside an `@media` that does not match does not
//! count (measured in Chromium 148, `tools/probes/cascade-layers.json`,
//! `cond-*` cases), so the stylist computes the ranks when the media are
//! evaluated, not when the sheets are added. `@supports` is evaluated when
//! the sheets are added, so a declaration inside a false `@supports` is not
//! recorded.
//!
//! A layer's rank is its position in the traversal in which the children of
//! a layer come in the order of their first declaration and before the
//! layer's own rules; so a sublayer ranks below its parent's own rules, and
//! the unlayered rules (the root) rank last. Normal declarations of a higher
//! rank win; `!important` declarations reverse the layer order.
//!
//! Every style pass and every `getComputedStyle` asks for the ranks, so the
//! tree keeps the last result, keyed by the values of the `@media` chains
//! that hold declarations (usually none).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use swb_css::LayerName;

/// The node of the unlayered rules.
pub(crate) const UNLAYERED: u32 = 0;

/// The most layers (nodes) that one document can have. Layers beyond it are
/// ignored with their rules. Real pages have a few dozen; the bound keeps
/// the work and memory of hostile pages linear in the input.
pub(crate) const MAX_LAYERS: usize = 65_536;

/// The values of the `@media` chains that hold declarations, and the ranks
/// for them.
type CachedRanks = (Vec<bool>, Arc<[u32]>);

/// The layers of a document's author origin and the order of their
/// declarations.
#[derive(Debug)]
pub(crate) struct LayerTree {
    /// The parent of each node; node 0 is its own parent.
    parents: Vec<u32>,
    /// The named child of a node by its name.
    named: HashMap<(u32, Box<str>), u32>,
    /// The declarations in document order: the layer, and the `@media`
    /// chain that it is inside (see the stylist), if any.
    events: Vec<(u32, Option<u32>)>,
    /// The `@media` chains of the events, each once, in order of first use.
    chains: Vec<u32>,
    chain_set: HashSet<u32>,
    /// The last result of [`LayerTree::ranks`] and the values of `chains`
    /// that it was computed for.
    cache: Mutex<Option<CachedRanks>>,
    /// True after the warning about [`MAX_LAYERS`].
    warned: bool,
}

impl Default for LayerTree {
    fn default() -> Self {
        LayerTree {
            parents: vec![UNLAYERED],
            named: HashMap::new(),
            events: Vec::new(),
            chains: Vec::new(),
            chain_set: HashSet::new(),
            cache: Mutex::new(None),
            warned: false,
        }
    }
}

impl LayerTree {
    /// The node of the layer `name` inside the layer `parent`, made if it
    /// does not exist, with the layers on the path. `None` if the tree is
    /// full ([`MAX_LAYERS`]).
    pub(crate) fn node(&mut self, parent: u32, name: &LayerName) -> Option<u32> {
        let mut current = parent;
        for segment in name.segments() {
            let key = (current, segment.clone());
            current = if let Some(&node) = self.named.get(&key) {
                node
            } else {
                let node = self.push(current)?;
                self.named.insert(key, node);
                node
            };
        }
        Some(current)
    }

    /// A new anonymous layer inside `parent`. `None` if the tree is full.
    pub(crate) fn anonymous(&mut self, parent: u32) -> Option<u32> {
        self.push(parent)
    }

    fn push(&mut self, parent: u32) -> Option<u32> {
        if self.parents.len() >= MAX_LAYERS {
            if !self.warned {
                log::warn!("more than {MAX_LAYERS} cascade layers; ignoring the rest");
                self.warned = true;
            }
            return None;
        }
        self.clear_cache();
        self.parents.push(parent);
        u32::try_from(self.parents.len() - 1).ok()
    }

    /// Records that `node` is declared here (by a statement, a block or an
    /// `@import`), inside the `@media` chain `media`.
    pub(crate) fn declare(&mut self, node: u32, media: Option<u32>) {
        // A repeat of the last declaration changes nothing.
        if self.events.last() != Some(&(node, media)) {
            self.clear_cache();
            self.events.push((node, media));
            if let Some(chain) = media
                && self.chain_set.insert(chain)
            {
                self.chains.push(chain);
            }
        }
    }

    fn clear_cache(&mut self) {
        *self.cache.get_mut().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// The rank of each node for the `@media` conditions `media` (indexed by
    /// chain): 0 for the first layer, the number of layers for the unlayered
    /// root. A layer that no matching declaration names gets 0 (no matching
    /// rule is in it). The result of the last call is reused while the
    /// chains that hold declarations have the same values.
    pub(crate) fn ranks(&self, media: &[bool]) -> Arc<[u32]> {
        let key: Vec<bool> = self
            .chains
            .iter()
            .map(|&c| media.get(c as usize).copied().unwrap_or(false))
            .collect();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached_key, ranks)) = cache.as_ref()
            && *cached_key == key
        {
            return Arc::clone(ranks);
        }
        let ranks: Arc<[u32]> = self.compute_ranks(media).into();
        *cache = Some((key, Arc::clone(&ranks)));
        ranks
    }

    fn compute_ranks(&self, media: &[bool]) -> Vec<u32> {
        let count = self.parents.len();
        if count == 1 {
            return vec![0];
        }
        // The children of each node in the order of their first matching
        // declaration. A declaration of a nested layer declares its
        // ancestors first.
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); count];
        let mut seen = vec![false; count];
        seen[UNLAYERED as usize] = true;
        let mut path = Vec::new();
        for &(node, chain) in &self.events {
            if chain.is_some_and(|c| !media.get(c as usize).copied().unwrap_or(false)) {
                continue;
            }
            let mut current = node;
            while !seen[current as usize] {
                seen[current as usize] = true;
                path.push(current);
                current = self.parents[current as usize];
            }
            while let Some(declared) = path.pop() {
                children[self.parents[declared as usize] as usize].push(declared);
            }
        }
        // Post-order traversal: the children, then the node itself. An
        // explicit stack, because dotted names make the tree as deep as
        // the input is long.
        let mut ranks = vec![0; count];
        let mut next = 0;
        let mut stack: Vec<(u32, usize)> = vec![(UNLAYERED, 0)];
        while let Some(top) = stack.last_mut() {
            let (node, index) = *top;
            if let Some(&child) = children[node as usize].get(index) {
                top.1 += 1;
                stack.push((child, 0));
            } else {
                ranks[node as usize] = next;
                next += 1;
                stack.pop();
            }
        }
        ranks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::{ComponentValue, parse_component_values};

    fn name(text: &str) -> LayerName {
        let values: Vec<ComponentValue> = parse_component_values(text);
        LayerName::parse(&values).expect("a valid layer name")
    }

    /// Declares the layers in order and gives the ranks of `lookup`.
    fn ranks_of(declared: &[&str], lookup: &[&str]) -> Vec<u32> {
        let mut tree = LayerTree::default();
        for text in declared {
            let node = tree.node(UNLAYERED, &name(text)).expect("room");
            tree.declare(node, None);
        }
        let ranks = tree.ranks(&[]);
        lookup
            .iter()
            .map(|text| ranks[tree.node(UNLAYERED, &name(text)).expect("room") as usize])
            .collect()
    }

    #[test]
    fn first_declaration_orders_layers() {
        assert_eq!(ranks_of(&["a", "b", "a", "c"], &["a", "b", "c"]), [0, 1, 2]);
        assert_eq!(ranks_of(&["b", "a"], &["a", "b"]), [1, 0]);
    }

    #[test]
    fn sublayers_rank_below_their_parent() {
        // x, then x.b, x.a (declared late), then y: x.b < x.a < x < y.
        let order = ["x", "x.b", "y", "x.a"];
        assert_eq!(ranks_of(&order, &["x.b", "x.a", "x", "y"]), [0, 1, 2, 3]);
        // A sublayer declared before its parent brings the parent along.
        assert_eq!(ranks_of(&["p.q", "r"], &["p.q", "p", "r"]), [0, 1, 2]);
    }

    #[test]
    fn unlayered_ranks_last() {
        let mut tree = LayerTree::default();
        let a = tree.node(UNLAYERED, &name("a")).expect("room");
        tree.declare(a, None);
        let anonymous = tree.anonymous(UNLAYERED).expect("room");
        tree.declare(anonymous, None);
        let ranks = tree.ranks(&[]);
        assert_eq!(ranks[a as usize], 0);
        assert_eq!(ranks[anonymous as usize], 1);
        assert_eq!(ranks[UNLAYERED as usize], 2);
    }

    #[test]
    fn declarations_in_a_false_media_do_not_count() {
        let mut tree = LayerTree::default();
        let a = tree.node(UNLAYERED, &name("a")).expect("room");
        let b = tree.node(UNLAYERED, &name("b")).expect("room");
        tree.declare(a, Some(0));
        tree.declare(b, None);
        tree.declare(a, None);
        // Chain 0 is true: a, b. Chain 0 is false: b, a.
        let on = tree.ranks(&[true]);
        assert!(on[a as usize] < on[b as usize]);
        let off = tree.ranks(&[false]);
        assert!(off[b as usize] < off[a as usize]);
    }

    #[test]
    fn ranks_are_reused_until_a_chain_or_the_tree_changes() {
        let mut tree = LayerTree::default();
        let a = tree.node(UNLAYERED, &name("a")).expect("room");
        let b = tree.node(UNLAYERED, &name("b")).expect("room");
        tree.declare(b, Some(1));
        tree.declare(a, None);
        tree.declare(b, None);
        // Chain 0 holds no declaration: its value does not matter.
        let first = tree.ranks(&[true, true]);
        assert!(Arc::ptr_eq(&first, &tree.ranks(&[false, true])));
        // Chain 1 changes: b is no longer declared first.
        let off = tree.ranks(&[true, false]);
        assert!(off[a as usize] < off[b as usize]);
        assert!(first[b as usize] < first[a as usize]);
        // A new layer clears the cache.
        let c = tree.node(UNLAYERED, &name("c")).expect("room");
        tree.declare(c, None);
        let ranks = tree.ranks(&[true, false]);
        assert_eq!(ranks.len(), 4);
        assert_eq!(ranks[c as usize], 2);
    }

    #[test]
    fn long_dotted_names_and_many_layers_are_cheap() {
        let mut tree = LayerTree::default();
        let long = (0..50_000).map(|_| "a").collect::<Vec<_>>().join(".");
        let node = tree.node(UNLAYERED, &name(&long)).expect("room");
        tree.declare(node, None);
        let ranks = tree.ranks(&[]);
        assert_eq!(ranks[node as usize], 0);
        assert_eq!(ranks[UNLAYERED as usize], 50_000);
        // Past the bound no node is made.
        let mut tree = LayerTree::default();
        let mut made = 0;
        for i in 0..MAX_LAYERS + 10 {
            if tree.node(UNLAYERED, &name(&format!("l{i}"))).is_some() {
                made += 1;
            }
        }
        assert_eq!(made, MAX_LAYERS - 1);
    }
}
