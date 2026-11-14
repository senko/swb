//! The contents operations of `Range` (DOM §5.5,
//! <https://dom.spec.whatwg.org/#interface-range>; ADR 0039 section 2): clone
//! the contents, extract them, delete them, insert a node, and the checks of
//! `surroundContents`, which the bindings compose from these.
//!
//! They are built on the change functions, so the mutation records, the
//! journal, the steps of the node iterators and the live ranges and the limits
//! come with them. The standard's recursion over the partially contained
//! children is a loop down each boundary point's path below the common
//! ancestor: each partially contained node gets a shallow copy (a node of
//! character data a copy of its selected data), and the children between the
//! paths are moved, copied or removed level by level. A range at the bottom of
//! a deep chain costs its depth, without recursion.
//!
//! The order of the changes is Chromium's (measured, case `contents-records`
//! of `tools/probes/web-range.json`; the standard's order differs, the result
//! is the same): the start's side (its data, then the children after the path,
//! from the bottom up), the end's side likewise, then the contained children
//! of the common ancestor. Each child that leaves its parent gets its own
//! record; the live range steps run once for each run of consecutive children
//! that leave together (`removing_run` in `ranges.rs`), so that one call costs
//! nodes plus boundary points.

use super::{Boundary, LiveRange};
use crate::algorithms::Cloned;
use crate::mutation::{DomError, Validity};
use crate::tree::{Document, NodeData, NodeId};

/// What `cloneContents` or `extractContents` made.
#[derive(Debug)]
pub struct Contents {
    /// The new document fragment with the contents.
    pub fragment: NodeId,
    /// The copies of the partially contained nodes, in tree order: the
    /// elements among them get upgrade reactions (extractContents; the copies
    /// of cloneContents are the whole fragment).
    pub copies: Vec<NodeId>,
    /// Pairs of the original and the copy of each `script` element.
    pub scripts: Vec<(NodeId, NodeId)>,
    /// Pairs of the original and the copy of each HTML `input` and `textarea`
    /// element.
    pub controls: Vec<(NodeId, NodeId)>,
}

impl Contents {
    fn new(fragment: NodeId) -> Contents {
        Contents {
            fragment,
            copies: Vec::new(),
            scripts: Vec::new(),
            controls: Vec::new(),
        }
    }
}

/// Which contents operation runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Clone,
    Extract,
    Delete,
}

/// The paths of a range's boundary points below their common ancestor.
struct Paths {
    common: NodeId,
    /// The partially contained nodes on the start's side, from the child of
    /// `common` down to the start node; empty if the start node is an
    /// inclusive ancestor of the end node.
    left: Vec<NodeId>,
    /// The same on the end's side.
    right: Vec<NodeId>,
}

impl Document {
    /// The inclusive ancestors of `node`, the root first.
    fn path_from_root(&self, node: NodeId) -> Vec<NodeId> {
        let mut path: Vec<NodeId> = std::iter::once(node).chain(self.ancestors(node)).collect();
        path.reverse();
        path
    }

    /// The paths of `range` (both boundary points share a root).
    fn range_paths(&self, range: LiveRange) -> Paths {
        let mut left = self.path_from_root(range.start.node);
        let mut right = self.path_from_root(range.end.node);
        self.charge_range_work(left.len() + right.len());
        let shared = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
        let common = shared
            .checked_sub(1)
            .and_then(|i| left.get(i).copied())
            .unwrap_or(range.start.node);
        let left = left.split_off(shared.min(left.len()));
        let right = right.split_off(shared.min(right.len()));
        Paths {
            common,
            left,
            right,
        }
    }

    /// The child of `parent` at `index`, if any.
    fn child_at(&self, parent: NodeId, index: u32) -> Option<NodeId> {
        let child = self.children(parent).nth(index as usize);
        self.charge_range_work(index as usize);
        child
    }

    /// The siblings from `first` up to (not including) `stop`.
    fn siblings_until(&self, first: Option<NodeId>, stop: Option<NodeId>) -> Vec<NodeId> {
        let mut run = Vec::new();
        let mut current = first;
        while let Some(n) = current {
            if Some(n) == stop {
                break;
            }
            run.push(n);
            current = self.node(n).next_sibling();
        }
        self.charge_range_work(run.len());
        run
    }

    /// The children of `paths.common` that `range` contains.
    fn contained_children(&self, range: LiveRange, paths: &Paths) -> Vec<NodeId> {
        let first = match paths.left.first() {
            Some(&s) => self.node(s).next_sibling(),
            None => self.child_at(paths.common, range.start.offset),
        };
        let stop = match paths.right.first() {
            Some(&e) => Some(e),
            None => self.child_at(paths.common, range.end.offset),
        };
        self.siblings_until(first, stop)
    }

    /// True if `range` contains a doctype (only a child of a document can be
    /// one, so only the contained children of the common ancestor are read).
    pub fn range_contains_doctype(&self, range: LiveRange) -> bool {
        if range.is_collapsed()
            || range.start.node == range.end.node && !self.is_document(range.start.node)
        {
            return false;
        }
        let paths = self.range_paths(range);
        self.is_document(paths.common)
            && self
                .contained_children(range, &paths)
                .iter()
                .any(|&n| matches!(self.node(n).data, NodeData::Doctype { .. }))
    }

    /// True if `range` partially contains a node that is not a `Text` node
    /// (`surroundContents` refuses such a range).
    /// <https://dom.spec.whatwg.org/#dom-range-surroundcontents>
    pub fn range_partly_selects_non_text(&self, range: LiveRange) -> bool {
        if range.start.node == range.end.node {
            return false;
        }
        let paths = self.range_paths(range);
        paths
            .left
            .iter()
            .chain(&paths.right)
            .any(|&n| !matches!(self.node(n).data, NodeData::Text(_)))
    }

    /// True if `range` contains `node` (DOM "contained": the node is in the
    /// range's root, `(node, 0)` is after the start and `(node, length)`
    /// before the end).
    /// <https://dom.spec.whatwg.org/#contained>
    pub fn range_contains_node(&self, range: LiveRange, node: NodeId) -> bool {
        self.root(node) == self.root(range.start.node)
            && self.compare_boundaries(Boundary::new(node, 0), range.start)
                == std::cmp::Ordering::Greater
            && self.compare_boundaries(Boundary::new(node, self.node_length(node)), range.end)
                == std::cmp::Ordering::Less
    }

    fn is_document(&self, node: NodeId) -> bool {
        matches!(self.node(node).data, NodeData::Document)
    }

    /// A new node of character data with the data of `node` from `from` to
    /// `to` (bytes), in the document of `node`.
    fn data_copy(&mut self, node: NodeId, from: u32, to: u32) -> Result<NodeId, DomError> {
        let slice = |t: &str| {
            t.get(from as usize..(to as usize).min(t.len()))
                .unwrap_or_default()
                .to_owned()
        };
        let data = match &self.node(node).data {
            NodeData::Text(t) => NodeData::Text(slice(t)),
            NodeData::Comment(t) => NodeData::Comment(slice(t)),
            NodeData::ProcessingInstruction { target, data } => NodeData::ProcessingInstruction {
                target: target.clone(),
                data: slice(data),
            },
            _ => return Err(DomError::InvalidNodeType),
        };
        self.create_node_in(node, data)
    }

    /// A clone of `node` (with `deep` its subtree) for `out`.
    fn copy_for(
        &mut self,
        node: NodeId,
        deep: bool,
        out: &mut Contents,
    ) -> Result<NodeId, DomError> {
        let owner = self.node_document(node);
        let Cloned {
            root,
            scripts,
            controls,
        } = self.clone_node(node, owner, deep)?;
        out.scripts.extend(scripts);
        out.controls.extend(controls);
        Ok(root)
    }

    /// The copies of the partially contained nodes of one side, each linked
    /// into the copy above it (the first into the fragment). The copy of a
    /// boundary node of character data holds its selected data.
    fn spine_copies(
        &mut self,
        spine: &[NodeId],
        point: Boundary,
        start: bool,
        out: &mut Option<Contents>,
    ) -> Result<Vec<NodeId>, DomError> {
        let Some(out) = out else {
            return Ok(Vec::new());
        };
        let mut parent = out.fragment;
        let mut copies = Vec::with_capacity(spine.len());
        for (i, &node) in spine.iter().enumerate() {
            let copy = if i + 1 == spine.len() && self.is_character_data(node) {
                let (from, to) = if start {
                    (point.offset, self.node_length(node))
                } else {
                    (0, point.offset)
                };
                self.data_copy(node, from, to)?
            } else {
                self.copy_for(node, false, out)?
            };
            self.link(parent, copy, None);
            out.copies.push(copy);
            copies.push(copy);
            parent = copy;
        }
        Ok(copies)
    }

    /// Copies (clone), moves (extract) or removes (delete) the consecutive
    /// children `run` of `parent`; copies and moved nodes go into `target`
    /// before its child `before` (at the end without). Each removal has its
    /// record; the live range steps run once for the run.
    fn take_run(
        &mut self,
        parent: NodeId,
        run: &[NodeId],
        target: Option<(NodeId, Option<NodeId>)>,
        mode: Mode,
        out: &mut Option<Contents>,
    ) -> Result<(), DomError> {
        if run.is_empty() {
            return Ok(());
        }
        if mode == Mode::Clone {
            let (Some((into, before)), Some(out)) = (target, out.as_mut()) else {
                return Ok(());
            };
            for &n in run {
                let copy = self.copy_for(n, true, out)?;
                self.link(into, copy, before);
            }
            return Ok(());
        }
        self.range_remove_run_steps(parent, run);
        for &n in run {
            self.remove_node_with(n, false, true, false);
            self.reset_form_owners(n);
            if let Some((into, before)) = target {
                self.link(into, n, before);
            }
        }
        Ok(())
    }

    /// The point where extract and delete collapse a range: the start if the
    /// start node is an inclusive ancestor of the end node, else after the
    /// child of the common ancestor that holds the start.
    fn collapse_point(&self, range: LiveRange, paths: &Paths) -> Boundary {
        match paths.left.first() {
            None => range.start,
            Some(&top) => Boundary::new(paths.common, self.index_in_parent(top) + 1),
        }
    }

    /// The common steps of clone, extract and delete: the new fragment (none
    /// for delete) and the point where the range collapses (none if the
    /// standard does not set it).
    /// <https://dom.spec.whatwg.org/#concept-range-clone>
    /// <https://dom.spec.whatwg.org/#concept-range-extract>
    /// <https://dom.spec.whatwg.org/#dom-range-deletecontents>
    fn range_contents(
        &mut self,
        range: LiveRange,
        mode: Mode,
    ) -> Result<(Option<Contents>, Option<Boundary>), DomError> {
        let mut out = match mode {
            Mode::Delete => None,
            Mode::Clone | Mode::Extract => {
                let owner = self.node_document(range.start.node);
                let fragment = self.create_node_in(owner, NodeData::DocumentFragment)?;
                Some(Contents::new(fragment))
            }
        };
        if range.is_collapsed() {
            return Ok((out, None));
        }
        let (start, end) = (range.start, range.end);
        if start.node == end.node && self.is_character_data(start.node) {
            if let Some(out) = &mut out {
                let copy = self.data_copy(start.node, start.offset, end.offset)?;
                self.link(out.fragment, copy, None);
            }
            if mode != Mode::Clone {
                let count = end.offset.saturating_sub(start.offset);
                self.replace_data(start.node, start.offset as usize, count as usize, "")?;
            }
            return Ok((out, None));
        }
        let paths = self.range_paths(range);
        let contained = self.contained_children(range, &paths);
        if mode != Mode::Delete
            && contained
                .iter()
                .any(|&n| matches!(self.node(n).data, NodeData::Doctype { .. }))
        {
            return Err(DomError::HierarchyRequest);
        }
        let point = (mode != Mode::Clone).then(|| self.collapse_point(range, &paths));
        let left_copies = self.spine_copies(&paths.left, start, true, &mut out)?;
        let right_copies = self.spine_copies(&paths.right, end, false, &mut out)?;
        self.take_start_side(&paths, start, &left_copies, mode, &mut out)?;
        self.take_end_side(&paths, end, &right_copies, mode, &mut out)?;
        // The contained children of the common ancestor.
        let target = out
            .as_ref()
            .map(|o| (o.fragment, right_copies.first().copied()));
        self.take_run(paths.common, &contained, target, mode, &mut out)?;
        Ok((out, point))
    }

    /// The start's side of clone, extract and delete: the selected data of
    /// the start node, then the children after the path `paths.left`, from
    /// the bottom up; `copies` are the copies of the path's nodes.
    fn take_start_side(
        &mut self,
        paths: &Paths,
        start: Boundary,
        copies: &[NodeId],
        mode: Mode,
        out: &mut Option<Contents>,
    ) -> Result<(), DomError> {
        if let Some(&node) = paths.left.last()
            && mode != Mode::Clone
            && self.is_character_data(node)
        {
            let count = self.node_length(node).saturating_sub(start.offset);
            self.replace_data(node, start.offset as usize, count as usize, "")?;
        }
        for level in (0..paths.left.len()).rev() {
            let Some(&node) = paths.left.get(level) else {
                continue;
            };
            let first = match paths.left.get(level + 1) {
                Some(&below) => self.node(below).next_sibling(),
                None if self.is_character_data(node) => continue,
                None => self.child_at(node, start.offset),
            };
            let run = self.siblings_until(first, None);
            let target = copies.get(level).map(|&copy| (copy, None));
            self.take_run(node, &run, target, mode, out)?;
        }
        Ok(())
    }

    /// The end's side of clone, extract and delete: the selected data of the
    /// end node, then the children before the path `paths.right`, from the
    /// bottom up; `copies` are the copies of the path's nodes.
    fn take_end_side(
        &mut self,
        paths: &Paths,
        end: Boundary,
        copies: &[NodeId],
        mode: Mode,
        out: &mut Option<Contents>,
    ) -> Result<(), DomError> {
        if let Some(&node) = paths.right.last()
            && mode != Mode::Clone
            && self.is_character_data(node)
        {
            self.replace_data(node, 0, end.offset as usize, "")?;
        }
        for level in (0..paths.right.len()).rev() {
            let Some(&node) = paths.right.get(level) else {
                continue;
            };
            let stop = match paths.right.get(level + 1) {
                Some(&below) => Some(below),
                None if self.is_character_data(node) => continue,
                None => self.child_at(node, end.offset),
            };
            let run = self.siblings_until(self.node(node).first_child(), stop);
            let before = copies.get(level + 1).copied();
            let target = copies.get(level).map(|&copy| (copy, before));
            self.take_run(node, &run, target, mode, out)?;
        }
        Ok(())
    }

    /// Sets live range `id` to the collapsed `point` at the end of extract
    /// and delete. If that changes the selection's range, the selection
    /// reports the type `Range` and the direction `forward` until its next
    /// change (Chromium, measured: `delete-from-document-more`).
    fn collapse_after_contents(&mut self, id: u32, point: Option<Boundary>) {
        let Some(point) = point else {
            return;
        };
        let range = LiveRange::collapsed_at(point);
        if self.live_range(id) == Some(range) {
            return;
        }
        self.set_live_range(id, range);
        if let Some(ranges) = self.ranges.as_deref_mut()
            && ranges.selection_id() == Some(id)
        {
            ranges.mark_selection_type_stale();
        }
    }

    /// `Range.cloneContents`: a new fragment with copies of the contents of
    /// `range`.
    ///
    /// # Errors
    ///
    /// [`DomError::HierarchyRequest`] if the range contains a doctype; the
    /// limits.
    /// <https://dom.spec.whatwg.org/#dom-range-clonecontents>
    pub fn clone_contents(&mut self, range: LiveRange) -> Result<Contents, DomError> {
        let (out, _) = self.range_contents(range, Mode::Clone)?;
        out.ok_or(DomError::NotFound)
    }

    /// `Range.extractContents` of live range `id`: the contents move into a
    /// new fragment (partially contained nodes stay, with copies in the
    /// fragment) and the range collapses.
    ///
    /// # Errors
    ///
    /// [`DomError::NotFound`] for an unknown range;
    /// [`DomError::HierarchyRequest`] if the range contains a doctype; the
    /// limits.
    /// <https://dom.spec.whatwg.org/#dom-range-extractcontents>
    pub fn extract_contents(&mut self, id: u32) -> Result<Contents, DomError> {
        let range = self.live_range(id).ok_or(DomError::NotFound)?;
        let (out, point) = self.range_contents(range, Mode::Extract)?;
        self.collapse_after_contents(id, point);
        out.ok_or(DomError::NotFound)
    }

    /// `Range.deleteContents` of live range `id`: the contained nodes leave,
    /// the selected data of the boundary nodes goes, and the range
    /// collapses.
    ///
    /// # Errors
    ///
    /// [`DomError::NotFound`] for an unknown range.
    /// <https://dom.spec.whatwg.org/#dom-range-deletecontents>
    pub fn delete_contents(&mut self, id: u32) -> Result<(), DomError> {
        let range = self.live_range(id).ok_or(DomError::NotFound)?;
        let (_, point) = self.range_contents(range, Mode::Delete)?;
        self.collapse_after_contents(id, point);
        Ok(())
    }

    /// `Range.insertNode` of `node` at the start of live range `id`: a text
    /// node at the start is split, and a collapsed range grows over the new
    /// node. The parent is returned (its new children are the node, or the
    /// former children of a fragment).
    ///
    /// # Errors
    ///
    /// [`DomError::HierarchyRequest`] if the start is in a comment or a
    /// processing instruction, in a text node without a parent, at `node`
    /// itself, or the insertion would make an invalid tree;
    /// [`DomError::NotFound`] for an unknown range; the limits.
    /// <https://dom.spec.whatwg.org/#dom-range-insertnode>
    pub fn insert_node(&mut self, id: u32, node: NodeId) -> Result<NodeId, DomError> {
        let start = self.live_range(id).ok_or(DomError::NotFound)?.start;
        let is_text = matches!(self.node(start.node).data, NodeData::Text(_));
        if matches!(
            self.node(start.node).data,
            NodeData::Comment(_) | NodeData::ProcessingInstruction { .. }
        ) || (is_text && self.parent(start.node).is_none())
            || start.node == node
        {
            return Err(DomError::HierarchyRequest);
        }
        let mut reference = if is_text {
            Some(start.node)
        } else {
            self.child_at(start.node, start.offset)
        };
        let parent = match reference {
            Some(r) => self.parent(r).ok_or(DomError::HierarchyRequest)?,
            None => start.node,
        };
        self.check_insertion(parent, node, reference, Validity::PreInsert)?;
        if is_text {
            reference = Some(self.split_text(start.node, start.offset as usize)?);
        }
        if reference == Some(node) {
            reference = self.node(node).next_sibling();
        }
        if self.parent(node).is_some() {
            self.remove(node);
        }
        let offset = match reference {
            Some(r) => self.index_in_parent(r),
            None => self.node_length(parent),
        };
        let added = if self.node(node).data.is_fragment() {
            self.node_length(node)
        } else {
            1
        };
        self.insert_before(parent, node, reference)?;
        if let Some(now) = self.live_range(id)
            && now.is_collapsed()
        {
            let end = Boundary::new(parent, offset.saturating_add(added));
            self.set_live_range(
                id,
                LiveRange {
                    start: now.start,
                    end,
                },
            );
        }
        Ok(parent)
    }
}

#[cfg(test)]
mod tests;
