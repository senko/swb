//! Nodes between arenas (ADR 0031 section 5). Each document of a frame has its
//! own arena, so DOM "adopt" into another arena takes the subtree out of one
//! arena and puts a copy into the other, and `importNode` copies it.
//!
//! All walks are iterative: a script can build a tree of a million levels.
//!
//! A taken node stays in its arena as an empty detached comment, because a
//! `NodeId` stays valid for the lifetime of the arena: what still holds the id
//! reads nothing of the moved node.
//!
//! Shadow roots go with their hosts (all of them when a subtree is taken, the
//! clonable ones in a copy: DOM "clone a node"), and the arena that gets them
//! assigns their slots again (ADR 0036 section 1).

use std::collections::HashMap;

use html5ever::local_name;

use crate::mutation::{DomError, data_bytes};
use crate::tree::{Document, NodeData, NodeId, ShadowRootData};

/// How a node without a parent hangs on another node of a [`Subtree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attached {
    /// Not at all (the root of the subtree, or a node with a parent).
    No,
    /// The contents of this template element.
    Contents(NodeId),
    /// The shadow root of this host.
    Shadow(NodeId),
}

/// One node of a [`Subtree`].
#[derive(Debug)]
struct Entry {
    /// The id in the arena that the subtree comes from.
    old: NodeId,
    data: NodeData,
    /// The parent, in that arena; `None` for a root.
    parent: Option<NodeId>,
    /// For template contents and shadow roots: their element.
    attached: Attached,
}

/// The nodes of a subtree without an arena, in an order with parents before
/// their children, template elements before their contents and hosts before
/// their shadow roots.
#[derive(Debug, Default)]
pub struct Subtree {
    nodes: Vec<Entry>,
    bytes: usize,
    templates: usize,
}

impl Subtree {
    /// The number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True for a subtree without nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// The result of [`Document::put_subtree`].
#[derive(Debug)]
pub struct Placed {
    /// The copy of the root.
    pub root: NodeId,
    /// The pairs of the id in the old arena and the id in this one, parents
    /// before their children.
    pub pairs: Vec<(NodeId, NodeId)>,
}

/// What a subtree needs of an arena (see [`Document::subtree_size`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubtreeSize {
    nodes: usize,
    bytes: usize,
    templates: usize,
}

/// The nodes that an arena needs for a subtree of `nodes` nodes with `templates`
/// template elements: each template can need its contents and an inert document,
/// and the inert documents need one more.
fn nodes_needed(nodes: usize, templates: usize) -> usize {
    nodes + 2 * templates + usize::from(templates > 0)
}

impl Document {
    /// The nodes of the subtree of `root` with its template contents and
    /// shadow roots (with `all_roots` every root, else the clonable ones);
    /// without `deep`, only `root` and its clonable shadow root. Each has the
    /// element that it hangs on, if it is the root of template contents or
    /// a shadow root.
    fn subtree_nodes(&self, root: NodeId, deep: bool, all_roots: bool) -> Vec<(NodeId, Attached)> {
        let mut found = Vec::new();
        let mut roots = vec![(root, Attached::No)];
        while let Some((start, attached)) = roots.pop() {
            found.push((start, attached));
            if !deep && start == root {
                if let Some(shadow) = self.shadow_root_to_take(root, false) {
                    roots.push((shadow, Attached::Shadow(root)));
                }
                continue;
            }
            let mut at = start;
            loop {
                if let Some(contents) = self.element(at).and_then(|e| e.template_contents) {
                    roots.push((contents, Attached::Contents(at)));
                }
                if let Some(shadow) = self.shadow_root_to_take(at, all_roots) {
                    roots.push((shadow, Attached::Shadow(at)));
                }
                let Some(next) = self.next_in_tree_order(at, start) else {
                    break;
                };
                found.push((next, Attached::No));
                at = next;
            }
        }
        found
    }

    /// The shadow root of `host` that goes with it: any root with `all`,
    /// else a clonable one.
    fn shadow_root_to_take(&self, host: NodeId, all: bool) -> Option<NodeId> {
        let root = self.shadow_root(host)?;
        (all || self.shadow_root_data(root).is_some_and(|d| d.clonable)).then_some(root)
    }

    /// The size of the subtree of `root`, for [`Document::check_subtree_room`] of
    /// the arena that is to hold it.
    pub fn subtree_size(&self, root: NodeId) -> SubtreeSize {
        let found = self.subtree_nodes(root, true, true);
        let mut size = SubtreeSize {
            nodes: found.len(),
            bytes: 0,
            templates: 0,
        };
        for &(node, _) in &found {
            let data = &self.node(node).data;
            size.bytes += data_bytes(data);
            if matches!(data, NodeData::Element(e) if e.is_html_named(&local_name!("template"))) {
                size.templates += 1;
            }
        }
        size
    }

    /// True if [`Document::put_subtree`] of a subtree of this size succeeds: the
    /// same test, so that a caller can take the subtree from its arena afterwards.
    ///
    /// # Errors
    ///
    /// The limit that the arena would pass.
    pub fn check_subtree_room(&self, size: SubtreeSize) -> Result<(), DomError> {
        self.check_room(nodes_needed(size.nodes, size.templates), size.bytes)
    }

    /// Takes the subtree of the detached node `root`, with its template
    /// contents, out of the arena. Each taken node stays as an empty detached
    /// comment.
    pub fn take_subtree(&mut self, root: NodeId) -> Subtree {
        let found = self.subtree_nodes(root, true, true);
        let mut subtree = Subtree {
            nodes: Vec::with_capacity(found.len()),
            ..Subtree::default()
        };
        // The parents first: the links are still whole while they are read.
        let parents: Vec<Option<NodeId>> = found
            .iter()
            .map(|&(n, _)| if n == root { None } else { self.parent(n) })
            .collect();
        for (&(old, attached), parent) in found.iter().zip(parents) {
            let node = self.node_mut(old);
            node.parent = None;
            node.first_child = None;
            node.last_child = None;
            node.prev_sibling = None;
            node.next_sibling = None;
            node.child_count = 0;
            node.element_child_count = 0;
            let data = std::mem::replace(&mut node.data, NodeData::Comment(String::new()));
            self.text_bytes = self.text_bytes.saturating_sub(data_bytes(&data));
            subtree.push(old, data, parent, attached);
            if matches!(attached, Attached::Contents(_)) {
                self.template_hosts.remove(&old);
            }
        }
        if self.shadow.is_some() {
            let taken: Vec<NodeId> = found.iter().map(|&(n, _)| n).collect();
            self.forget_shadow_nodes(&taken);
        }
        self.generation += 1;
        self.structure_generation += 1;
        subtree
    }

    /// A copy of the subtree of `root` (with `deep`, else of `root` alone), for
    /// another arena.
    pub fn copy_subtree(&self, root: NodeId, deep: bool) -> Subtree {
        let found = self.subtree_nodes(root, deep, false);
        let mut subtree = Subtree {
            nodes: Vec::with_capacity(found.len()),
            ..Subtree::default()
        };
        for &(old, attached) in &found {
            let parent = if old == root { None } else { self.parent(old) };
            subtree.push(old, self.node(old).data.clone(), parent, attached);
        }
        subtree
    }

    /// Puts a subtree into the arena, detached, with the node document of
    /// `document` (any node of that document); template contents get the
    /// template contents owner of that document.
    ///
    /// # Errors
    ///
    /// The limit that the arena would pass; nothing changes then.
    pub fn put_subtree(&mut self, document: NodeId, subtree: Subtree) -> Result<Placed, DomError> {
        self.check_room(
            nodes_needed(subtree.nodes.len(), subtree.templates),
            subtree.bytes,
        )?;
        let owner = self.node_document(document);
        let mut map: HashMap<NodeId, NodeId> = HashMap::with_capacity(subtree.nodes.len());
        let mut pairs = Vec::with_capacity(subtree.nodes.len());
        let mut root = None;
        let mut shadow_roots = Vec::new();
        for entry in subtree.nodes {
            let new = match entry.attached {
                // `push_node` made the contents of the template.
                Attached::Contents(host) => map
                    .get(&host)
                    .and_then(|&h| self.element(h))
                    .and_then(|e| e.template_contents),
                Attached::Shadow(host) => match (map.get(&host), entry.data) {
                    (Some(&host), NodeData::ShadowRoot(data)) => {
                        let id = self.make_shadow_root(host, ShadowRootData { host, ..data });
                        shadow_roots.push(id);
                        Some(id)
                    }
                    _ => None,
                },
                Attached::No => {
                    let parent = entry.parent.and_then(|p| map.get(&p).copied());
                    let own = parent.map_or(owner, |p| self.node_document(p));
                    let id = self.push_node(entry.data, own);
                    if let Some(parent) = parent {
                        self.link_raw(parent, id, None);
                    }
                    Some(id)
                }
            };
            let Some(new) = new else {
                continue;
            };
            root.get_or_insert(new);
            map.insert(entry.old, new);
            pairs.push((entry.old, new));
        }
        for shadow in shadow_roots {
            self.assign_new_root(shadow);
        }
        self.generation += 1;
        self.structure_generation += 1;
        Ok(Placed {
            root: root.ok_or(DomError::NotFound)?,
            pairs,
        })
    }
}

impl Subtree {
    /// Adds a node: what refers to the old arena leaves its data.
    fn push(
        &mut self,
        old: NodeId,
        mut data: NodeData,
        parent: Option<NodeId>,
        attached: Attached,
    ) {
        if let NodeData::Element(e) = &mut data {
            e.template_contents = None;
            e.parser_form = None;
            e.shadow_flags = 0;
            if e.is_html_named(&local_name!("template")) {
                self.templates += 1;
            }
        }
        self.bytes += data_bytes(&data);
        self.nodes.push(Entry {
            old,
            data,
            parent,
            attached,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(doc: &Document, root: NodeId) -> String {
        crate::serialize::inner_html(doc, root)
    }

    #[test]
    fn a_taken_subtree_leaves_comments_and_arrives_whole() {
        let mut a = crate::parse_html("<body><div id=d class=k><b>x</b>y<!--c--></div><p id=p>");
        let mut b = crate::parse_html("<body><i id=i></i>");
        let d = a.element_by_id("d").expect("d");
        a.remove(d);
        let size = a.subtree_size(d);
        assert_eq!(size.nodes, 5);
        b.check_subtree_room(size).expect("room");
        let bytes = a.text_bytes();
        let subtree = a.take_subtree(d);
        assert_eq!(subtree.len(), 5);
        assert!(a.text_bytes() < bytes);
        assert!(matches!(&a.node(d).data, NodeData::Comment(c) if c.is_empty()));
        assert_eq!(a.node(d).first_child(), None);
        let placed = b.put_subtree(NodeId::DOCUMENT, subtree).expect("put");
        assert_eq!(placed.pairs.first(), Some(&(d, placed.root)));
        assert_eq!(placed.pairs.len(), 5);
        let body = b.body().expect("body");
        b.append_child(body, placed.root).expect("append");
        assert_eq!(b.element_by_id("d"), Some(placed.root));
        assert_eq!(a.element_by_id("d"), None);
        assert_eq!(
            tree(&b, placed.root),
            "<b>x</b>y<!--c-->",
            "the children keep their order"
        );
        assert_eq!(b.child_count(placed.root), 3);
        assert_eq!(b.element_child_count(placed.root), 1);
    }

    #[test]
    fn template_contents_go_with_their_template() {
        let mut a = crate::parse_html(
            "<body><template id=t><p>x<template id=u><i></i></template></p></template>",
        );
        let mut b = crate::parse_html("<body>");
        let t = a.element_by_id("t").expect("t");
        let old_contents = a.element(t).and_then(|e| e.template_contents);
        a.remove(t);
        let subtree = a.take_subtree(t);
        let placed = b.put_subtree(NodeId::DOCUMENT, subtree).expect("put");
        let contents = b
            .element(placed.root)
            .and_then(|e| e.template_contents)
            .expect("contents");
        assert!(
            placed
                .pairs
                .contains(&(old_contents.expect("old"), contents))
        );
        assert_eq!(b.child_count(contents), 1);
        assert_ne!(b.node_document(contents), NodeId::DOCUMENT);
        assert_eq!(b.node_document(placed.root), NodeId::DOCUMENT);
        let p = b.node(contents).first_child().expect("p");
        assert_eq!(b.child_count(p), 2);
        assert!(b.is_host_including_inclusive_ancestor(placed.root, p));
    }

    #[test]
    fn a_copy_leaves_the_source_and_a_shallow_copy_has_no_children() {
        let a = crate::parse_html("<body><div id=d><b>x</b></div>");
        let mut b = crate::parse_html("<body>");
        let d = a.element_by_id("d").expect("d");
        let deep = b
            .put_subtree(NodeId::DOCUMENT, a.copy_subtree(d, true))
            .expect("deep");
        let shallow = b
            .put_subtree(NodeId::DOCUMENT, a.copy_subtree(d, false))
            .expect("shallow");
        assert_eq!(a.element_by_id("d"), Some(d));
        assert_eq!(b.child_count(deep.root), 1);
        assert_eq!(b.child_count(shallow.root), 0);
        assert!(a.is_equal_to(d, &b, deep.root));
        assert!(!a.is_equal_to(d, &b, shallow.root));
    }

    #[test]
    fn a_deep_subtree_moves_without_recursion() {
        let mut a = Document::new();
        let mut b = Document::new();
        let name = html5ever::QualName::new(None, html5ever::ns!(html), local_name!("div"));
        let root = a.create_element(name.clone(), Vec::new()).expect("root");
        let mut at = root;
        for _ in 0..200_000 {
            let next = a.create_element(name.clone(), Vec::new()).expect("next");
            a.append_child(at, next).expect("append");
            at = next;
        }
        let subtree = a.take_subtree(root);
        assert_eq!(subtree.len(), 200_001);
        let placed = b.put_subtree(NodeId::DOCUMENT, subtree).expect("put");
        assert_eq!(b.descendants(placed.root).count(), 200_001);
    }

    #[test]
    fn a_full_arena_refuses_and_stays_as_it_was() {
        let a = crate::parse_html("<body><div id=d><b>x</b></div>");
        let mut b = Document::with_limits(crate::tree::Limits {
            max_nodes: 2,
            ..crate::tree::Limits::default()
        });
        let d = a.element_by_id("d").expect("d");
        let len = b.len();
        assert!(matches!(
            b.put_subtree(NodeId::DOCUMENT, a.copy_subtree(d, true)),
            Err(DomError::NodeLimit(_))
        ));
        assert_eq!(b.len(), len);
        assert!(b.check_subtree_room(a.subtree_size(d)).is_err());
    }

    /// A caller takes a subtree from its arena after the check, so the check and
    /// the put must agree at each size of the target, also with template contents
    /// and their inert documents.
    #[test]
    fn the_room_check_and_the_put_agree() {
        for source in [
            "<body><div id=d><b>x</b></div>",
            "<body><div id=d><template><p>x</p></template></div>",
            "<body><div id=d><template><template><i></i></template></template><template></template></div>",
        ] {
            let a = crate::parse_html(source);
            let d = a.element_by_id("d").expect("d");
            let size = a.subtree_size(d);
            let mut fitted = false;
            for max_nodes in 1..40 {
                let mut b = Document::with_limits(crate::tree::Limits {
                    max_nodes,
                    ..crate::tree::Limits::default()
                });
                let room = b.check_subtree_room(size).is_ok();
                let put = b.put_subtree(NodeId::DOCUMENT, a.copy_subtree(d, true));
                assert_eq!(room, put.is_ok(), "{source} with room for {max_nodes}");
                fitted |= room;
            }
            assert!(fitted, "{source} fits in the end");
        }
    }
}
