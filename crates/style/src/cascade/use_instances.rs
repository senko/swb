//! The instance trees of SVG `use` elements (SVG 2 §5.6,
//! <https://svgwg.org/svg2-draft/struct.html#UseElement>).
//!
//! A `use` element with a local reference (`href="#id"` or
//! `xlink:href`) draws a copy of the referenced element. The copy's rules
//! come from the original: selectors match the original element in its
//! place in the document. Only inheritance differs: the copy inherits from
//! the `use` element, not from the original's parent. So after the normal
//! cascade, this module computes the styles of the referenced subtree again
//! with the `use` element's style as the parent, once for each `use`
//! element (and again for `use` elements inside such a copy, which inherit
//! from their own `use`).
//!
//! Limits for hostile documents (a `use` of a group of `use` elements can
//! grow exponentially): at most [`MAX_INSTANCE_ELEMENTS`] styled elements
//! in all instance trees of a document, and references nest at most
//! [`MAX_USE_DEPTH`] deep. A reference to an ancestor of the `use` element
//! or to an element that an enclosing instance already copies is a cycle
//! and has no instance. The layout draws nothing for a `use` without an
//! instance.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use swb_dom::{NodeId, ns};

use super::Styler;
use crate::ComputedStyle;
use crate::element::DomElement;
use crate::style_map::{InstanceId, StyleMap, UseInstance};
use crate::values::Display;

/// The most elements that the instance trees of one document hold.
pub const MAX_INSTANCE_ELEMENTS: usize = 20_000;

/// The deepest nesting of `use` instances (a `use` inside the copy made by
/// another `use`, ...).
pub const MAX_USE_DEPTH: usize = 16;

/// Whether the element is an SVG `use` element.
fn is_svg_use(e: &swb_dom::ElementData) -> bool {
    e.name.ns == ns!(svg) && &**e.local_name() == "use"
}

/// A `use` element waiting for its instance.
struct Pending {
    /// The tree that holds the `use` element: the document (`None`) or an
    /// instance tree.
    scope: Option<InstanceId>,
    node: NodeId,
    /// The elements copied by the enclosing instances, outermost first.
    chain: Arc<[NodeId]>,
}

impl Styler<'_> {
    /// Computes the instance trees of all `use` elements of the document.
    pub(super) fn expand_uses(&mut self, map: &mut StyleMap) {
        let mut queue: VecDeque<Pending> = self
            .doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| self.doc.element(n).is_some_and(is_svg_use))
            .map(|node| Pending {
                scope: None,
                node,
                chain: Arc::from([]),
            })
            .collect();
        if queue.is_empty() {
            return;
        }
        let ids = self.doc.element_ids();
        let mut budget = MAX_INSTANCE_ELEMENTS;
        // The Bloom filter holds the ancestors of the element that the
        // main pass styles: it cannot serve an instance tree.
        let filter = self.filter.take();
        while let Some(pending) = queue.pop_front() {
            if budget == 0 {
                log::warn!(
                    "SVG: more than {MAX_INSTANCE_ELEMENTS} elements in the instances of `use` \
                     elements; the rest are not drawn"
                );
                break;
            }
            self.expand_use(map, &ids, pending, &mut budget, &mut queue);
        }
        self.filter = filter;
    }

    /// Makes the instance tree of one `use` element, if it is valid, and
    /// queues the `use` elements inside it.
    fn expand_use(
        &mut self,
        map: &mut StyleMap,
        ids: &HashMap<&str, NodeId>,
        pending: Pending,
        budget: &mut usize,
        queue: &mut VecDeque<Pending>,
    ) {
        let Pending { scope, node, chain } = pending;
        let Some(use_style) = map.style_in(scope, node).cloned() else {
            return;
        };
        if use_style.display == Display::None || chain.len() >= MAX_USE_DEPTH {
            return;
        }
        let Some(target) = self.use_target(node, ids) else {
            return;
        };
        // A reference to the `use` element itself or to an ancestor of it,
        // or to an element that an enclosing instance copies, is a cycle.
        if target == node
            || chain.contains(&target)
            || self.doc.ancestors(node).any(|a| a == target)
        {
            return;
        }
        let mut instance = UseInstance {
            target,
            styles: HashMap::new(),
            uses: HashMap::new(),
        };
        let uses = self.style_instance(target, &use_style, &mut instance, budget);
        let id = map.add_instance(instance);
        map.set_use_instance(scope, node, id);
        let inner: Arc<[NodeId]> = chain.iter().copied().chain([target]).collect();
        for node in uses {
            queue.push_back(Pending {
                scope: Some(id),
                node,
                chain: Arc::clone(&inner),
            });
        }
    }

    /// The element that `use` element `node` references: an SVG element
    /// found by the fragment of a local `href`.
    fn use_target(&self, node: NodeId, ids: &HashMap<&str, NodeId>) -> Option<NodeId> {
        let href = self.doc.element(node)?.svg_href()?.trim();
        let id = href.strip_prefix('#')?;
        let target = *ids.get(id)?;
        let e = self.doc.element(target)?;
        (e.name.ns == ns!(svg)).then_some(target)
    }

    /// Styles `target` and its descendants with parent style `parent`
    /// into `instance`, up to the budget. Elements in `display: none`
    /// subtrees get no style, as in the main pass. Returns the `use`
    /// elements that got a style, in tree order. Only styled elements are
    /// visited, so the budget bounds the work.
    fn style_instance(
        &mut self,
        target: NodeId,
        parent: &Arc<ComputedStyle>,
        instance: &mut UseInstance,
        budget: &mut usize,
    ) -> Vec<NodeId> {
        let mut uses = Vec::new();
        let mut stack: Vec<(NodeId, Arc<ComputedStyle>, Option<Display>)> =
            vec![(target, Arc::clone(parent), None)];
        let mut children = Vec::new();
        while let Some((node, parent, layout_parent)) = stack.pop() {
            if *budget == 0 {
                return uses;
            }
            let Some(el) = DomElement::new(self.doc, node, self.states) else {
                continue;
            };
            *budget -= 1;
            let style = self.style_element(&el, &parent, layout_parent, false);
            if style.display != Display::None {
                let child_layout_parent = if style.display == Display::Contents {
                    layout_parent
                } else {
                    Some(style.display)
                };
                children.clear();
                children.extend(self.doc.element_children(node));
                for &child in children.iter().rev() {
                    stack.push((child, Arc::clone(&style), child_layout_parent));
                }
            }
            if is_svg_use(el.data()) {
                uses.push(node);
            }
            instance.styles.insert(node, style);
        }
        uses
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use swb_css::MediaEnvironment;
    use swb_dom::{Document, NodeId, parse_html};

    use crate::element::ElementStates;
    use crate::style_map::StyleMap;
    use crate::{Stylist, compute_styles};

    fn styles(html: &str) -> (Document, StyleMap) {
        let doc = parse_html(html);
        let stylist = Stylist::new(doc.quirks_mode);
        let base = url::Url::parse("https://example.com/").expect("a URL");
        let map = compute_styles(
            &doc,
            &stylist,
            &MediaEnvironment::default(),
            &ElementStates::default(),
            &base,
        );
        (doc, map)
    }

    fn node(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id).expect("an element with the id")
    }

    #[test]
    fn the_copy_inherits_from_the_use_element() {
        let (doc, map) = styles(
            "<svg fill=blue><defs><g id=g fill=red><rect id=r /></g><rect id=plain /></defs>\
             <use id=u href='#plain' fill=green /></svg>",
        );
        let u = node(&doc, "u");
        let instance = map.use_instance(None, u).expect("an instance");
        let plain = node(&doc, "plain");
        let green = map.style_in(Some(instance), plain).expect("a style");
        let original = map.get(plain).expect("a style");
        assert_ne!(green.fill, original.fill);
        assert_eq!(green.fill, map.get(u).expect("a style").fill);
    }

    #[test]
    fn xlink_href_and_missing_targets() {
        let (doc, map) = styles(
            "<svg><rect id=r /><use id=a xlink:href='#r' /><use id=b href='#nope' />\
             <use id=c href='r' /><use id=d /></svg>",
        );
        assert!(map.use_instance(None, node(&doc, "a")).is_some());
        for id in ["b", "c", "d"] {
            assert!(map.use_instance(None, node(&doc, id)).is_none(), "#{id}");
        }
    }

    #[test]
    fn cycles_have_no_instance() {
        let (doc, map) = styles(
            "<svg><g id=g><use id=self href='#g' /></g><use id=a href='#b' /><use id=b href='#a' />\
             </svg>",
        );
        assert!(map.use_instance(None, node(&doc, "self")).is_none());
        // `a` references `b`, which references `a`: the chain stops where it
        // closes, so each of them has an instance at most once deep.
        assert!(map.use_instance(None, node(&doc, "a")).is_some());
    }

    #[test]
    fn nested_uses_get_their_own_instance() {
        let (doc, map) = styles(
            "<svg><rect id=r /><g id=g><use id=inner href='#r' fill=red /></g>\
             <use id=outer href='#g' fill=blue /></svg>",
        );
        let outer = map
            .use_instance(None, node(&doc, "outer"))
            .expect("an instance");
        let inner = map
            .use_instance(Some(outer), node(&doc, "inner"))
            .expect("an instance");
        // The inner `use` sets its own fill: the copy of `r` gets red.
        let r = map.style_in(Some(inner), node(&doc, "r")).expect("a style");
        assert_eq!(
            r.fill,
            map.style_in(Some(outer), node(&doc, "inner"))
                .expect("a style")
                .fill
        );
    }

    #[test]
    fn unstyled_subtrees_of_the_target_are_not_visited() {
        // The `use` inside the `display: none` child has no style in the
        // instance, so it gets no instance and costs nothing.
        let (doc, map) = styles(
            "<svg><rect id=r /><g id=g><g style='display:none'><use id=hid href='#r' /></g>\
             <use id=shown href='#r' /></g><use id=u href='#g' /></svg>",
        );
        let u = map
            .use_instance(None, node(&doc, "u"))
            .expect("an instance");
        assert!(map.use_instance(Some(u), node(&doc, "shown")).is_some());
        assert!(map.use_instance(Some(u), node(&doc, "hid")).is_none());
        // Styled: `g`, the hidden `g` and `shown` in the instance of `u`;
        // `r` in the instance of `shown` in the document; `r` in the
        // instance of `shown` in the instance of `u`.
        assert_eq!(map.instance_element_count(), 5);
    }

    #[test]
    fn exponential_expansion_stops_at_the_budget() {
        // 20 levels, each `use`s the level below twice: 2^20 copies.
        let mut levels = String::new();
        for i in 1..=20 {
            let p = i - 1;
            let _ = write!(
                levels,
                "<g id=l{i}><use href='#l{p}'/><use href='#l{p}'/></g>"
            );
        }
        let html = format!("<svg><rect id=l0 />{levels}<use href='#l20' /></svg>");
        let (_, map) = styles(&html);
        let count: usize = map.instance_element_count();
        assert!(count <= super::MAX_INSTANCE_ELEMENTS, "{count}");
    }
}
