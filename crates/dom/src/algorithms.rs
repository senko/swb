//! More DOM algorithms for scripts (ADR 0028 section 3): clone, node
//! equality, document position, `normalize`, and the reset of form owners
//! after a move (<https://dom.spec.whatwg.org/>).
//!
//! All walks are iterative: a script can build a tree of 100,000 levels.

use crate::mutation::DomError;
use crate::tree::{Document, NodeData, NodeId};

/// `compareDocumentPosition` bits.
pub mod position {
    /// The nodes are in different trees.
    pub const DISCONNECTED: u16 = 0x01;
    /// The other node precedes this one.
    pub const PRECEDING: u16 = 0x02;
    /// The other node follows this one.
    pub const FOLLOWING: u16 = 0x04;
    /// The other node contains this one.
    pub const CONTAINS: u16 = 0x08;
    /// The other node is contained by this one.
    pub const CONTAINED_BY: u16 = 0x10;
    /// The order of disconnected nodes is decided by the implementation.
    pub const IMPLEMENTATION_SPECIFIC: u16 = 0x20;
}

/// The result of a clone: the copy of the node and, for each cloned HTML
/// `script` element, the pair of the original and the copy (HTML's cloning
/// steps copy the "already started" flag; the bindings keep that flag).
#[derive(Clone, Debug)]
pub struct Cloned {
    /// The copy of the node.
    pub root: NodeId,
    /// Pairs of the original and the copy of each `script` element.
    pub scripts: Vec<(NodeId, NodeId)>,
    /// Pairs of the original and the copy of each HTML `input` and
    /// `textarea` element (their cloning steps copy the value and the
    /// checkedness).
    pub controls: Vec<(NodeId, NodeId)>,
}

/// The pairs that a clone collects.
#[derive(Default)]
struct Pairs {
    scripts: Vec<(NodeId, NodeId)>,
    controls: Vec<(NodeId, NodeId)>,
}

impl Document {
    /// DOM "clone a node" with the node document `document` (any node of
    /// that document): a copy of `node`, and with `deep` of its
    /// descendants (and the contents of template elements). The copy has no
    /// parent. Form element pointers are not copied.
    /// <https://dom.spec.whatwg.org/#concept-node-clone>
    pub fn clone_node(
        &mut self,
        node: NodeId,
        document: NodeId,
        deep: bool,
    ) -> Result<Cloned, DomError> {
        let document = self.node_document(document);
        let mut pairs = Pairs::default();
        let root = self.clone_one(node, document, &mut pairs)?;
        if deep {
            // (original, copy): the children of the original are copied
            // next. Template contents go with their template element.
            let mut stack = vec![(node, root)];
            while let Some((src, dst)) = stack.pop() {
                let contents = self
                    .element(src)
                    .and_then(|e| e.template_contents)
                    .zip(self.element(dst).and_then(|e| e.template_contents));
                for (from, to) in std::iter::once((src, dst)).chain(contents) {
                    let owner = self.node_document(to);
                    let children: Vec<NodeId> = self.children(from).collect();
                    for child in children {
                        let copy = self.clone_one(child, owner, &mut pairs)?;
                        self.link_raw(to, copy, None);
                        stack.push((child, copy));
                    }
                }
            }
        }
        Ok(Cloned {
            root,
            scripts: pairs.scripts,
            controls: pairs.controls,
        })
    }

    /// Creates the copy of one node, without children.
    fn clone_one(
        &mut self,
        src: NodeId,
        owner: NodeId,
        pairs: &mut Pairs,
    ) -> Result<NodeId, DomError> {
        let mut data = self.node(src).data.clone();
        let mut nodes = 1;
        if let NodeData::Element(e) = &mut data {
            nodes += 2 * usize::from(e.template_contents.take().is_some());
            e.parser_form = None;
        }
        self.check_room(nodes, crate::mutation::data_bytes(&data))?;
        let (is_script, is_control) = match &data {
            NodeData::Element(e) => (
                e.is_html_named(&html5ever::local_name!("script")),
                e.is_html_named(&html5ever::local_name!("input"))
                    || e.is_html_named(&html5ever::local_name!("textarea")),
            ),
            _ => (false, false),
        };
        let copy = self.push_node(data, owner);
        if is_script {
            pairs.scripts.push((src, copy));
        }
        if is_control {
            pairs.controls.push((src, copy));
        }
        Ok(copy)
    }

    /// DOM "equals": the nodes are of the same kind with the same data,
    /// attributes (in any order) and equal children.
    /// <https://dom.spec.whatwg.org/#concept-node-equals>
    pub fn is_equal_node(&self, a: NodeId, b: NodeId) -> bool {
        let mut stack = vec![(a, b)];
        while let Some((x, y)) = stack.pop() {
            if !self.same_data(x, y) {
                return false;
            }
            let (mut cx, mut cy) = (self.node(x).first_child, self.node(y).first_child);
            loop {
                match (cx, cy) {
                    (None, None) => break,
                    (Some(p), Some(q)) => {
                        stack.push((p, q));
                        cx = self.node(p).next_sibling;
                        cy = self.node(q).next_sibling;
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    /// True if two nodes are of the same kind and have the same data (not
    /// the children).
    fn same_data(&self, x: NodeId, y: NodeId) -> bool {
        match (&self.node(x).data, &self.node(y).data) {
            (NodeData::Document, NodeData::Document)
            | (NodeData::DocumentFragment, NodeData::DocumentFragment) => true,
            (
                NodeData::Doctype {
                    name,
                    public_id,
                    system_id,
                },
                NodeData::Doctype {
                    name: n2,
                    public_id: p2,
                    system_id: s2,
                },
            ) => name == n2 && public_id == p2 && system_id == s2,
            (NodeData::Element(a), NodeData::Element(b)) => {
                a.name.ns == b.name.ns
                    && a.name.prefix == b.name.prefix
                    && a.name.local == b.name.local
                    && a.attrs.len() == b.attrs.len()
                    && a.attrs.iter().all(|attr| {
                        b.attrs.iter().any(|o| {
                            o.name.ns == attr.name.ns
                                && o.name.local == attr.name.local
                                && o.value == attr.value
                        })
                    })
            }
            (NodeData::Text(a), NodeData::Text(b))
            | (NodeData::Comment(a), NodeData::Comment(b)) => a == b,
            (
                NodeData::ProcessingInstruction { target, data },
                NodeData::ProcessingInstruction {
                    target: t2,
                    data: d2,
                },
            ) => target == t2 && data == d2,
            _ => false,
        }
    }

    /// `compareDocumentPosition`: the position of `other` relative to
    /// `this` (a combination of the [`position`] bits).
    /// <https://dom.spec.whatwg.org/#dom-node-comparedocumentposition>
    pub fn compare_position(&self, this: NodeId, other: NodeId) -> u16 {
        use position::{
            CONTAINED_BY, CONTAINS, DISCONNECTED, FOLLOWING, IMPLEMENTATION_SPECIFIC, PRECEDING,
        };
        if this == other {
            return 0;
        }
        if self.root(this) != self.root(other) {
            // A consistent order for disconnected nodes: by creation.
            let order = if other.index() < this.index() {
                PRECEDING
            } else {
                FOLLOWING
            };
            return DISCONNECTED | IMPLEMENTATION_SPECIFIC | order;
        }
        if self.ancestors(this).any(|a| a == other) {
            return CONTAINS | PRECEDING;
        }
        if self.ancestors(other).any(|a| a == this) {
            return CONTAINED_BY | FOLLOWING;
        }
        if self.precedes(other, this) {
            PRECEDING
        } else {
            FOLLOWING
        }
    }

    /// DOM `normalize`: removes the empty text nodes under `node` and merges
    /// each run of adjacent text nodes into its first.
    /// <https://dom.spec.whatwg.org/#dom-node-normalize>
    pub fn normalize(&mut self, node: NodeId) -> Result<(), DomError> {
        let texts: Vec<NodeId> = self
            .descendants(node)
            .filter(|&n| n != node && matches!(self.node(n).data, NodeData::Text(_)))
            .collect();
        for text in texts {
            if self.node(text).parent.is_none() {
                // Merged into an earlier node.
                continue;
            }
            if self.char_data(text).is_some_and(str::is_empty) {
                self.remove_node(text, false);
                continue;
            }
            // One node at a time: append its data (a "characterData"
            // record), then remove it (a "childList" record). The standard
            // appends the data of the whole run at once; Chromium works
            // node by node (measured, feature 9a), and so do we.
            let mut next = self.node(text).next_sibling;
            while let Some(n) = next {
                let NodeData::Text(data) = &self.node(n).data else {
                    break;
                };
                let data = data.clone();
                next = self.node(n).next_sibling;
                if !data.is_empty() {
                    self.append_data(text, &data)?;
                }
                self.remove_node(n, false);
            }
        }
        Ok(())
    }

    /// Sets the "already started" flag of the script element `node` (the
    /// cloning steps of HTML copy it).
    pub fn mark_script_started(&mut self, node: NodeId) {
        if let Some(e) = self.element_mut(node) {
            e.script_already_started = true;
        }
    }

    /// HTML "reset the form owner" after the ancestors of the subtree of
    /// `root` changed: the elements lose the form pointer that the parser
    /// gave them (the form owner is then the form attribute or the nearest
    /// ancestor `form`).
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#reset-the-form-owner>
    pub(crate) fn reset_form_owners(&mut self, root: NodeId) {
        if !self.parser_forms {
            return;
        }
        let subtree: Vec<NodeId> = self.descendants(root).collect();
        for n in subtree {
            if let Some(e) = self.element_mut(n) {
                e.parser_form = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::position::{
        CONTAINED_BY, CONTAINS, DISCONNECTED, FOLLOWING, IMPLEMENTATION_SPECIFIC, PRECEDING,
    };
    use super::*;
    use crate::parse_html;

    fn id(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id).expect("element")
    }

    #[test]
    fn clone_equal_and_position() {
        let mut doc = parse_html(
            "<div id=a class=x><p id=p>t<b>u</b></p><!--c--></div><div id=z></div>\
             <template id=t><i>q</i></template><script id=s>1</script>",
        );
        let (div, para, other) = (id(&doc, "a"), id(&doc, "p"), id(&doc, "z"));
        let shallow = doc.clone_node(div, div, false).unwrap().root;
        assert_eq!(doc.children(shallow).count(), 0);
        assert!(doc.node(shallow).parent.is_none());
        let deep = doc.clone_node(div, div, true).unwrap().root;
        assert!(doc.is_equal_node(div, deep));
        assert!(!doc.is_equal_node(div, shallow));
        assert!(!doc.is_equal_node(div, other));
        assert_eq!(doc.text_content(deep), "tu");
        let template = id(&doc, "t");
        let copy = doc.clone_node(template, template, true).unwrap().root;
        let contents = doc.element(copy).unwrap().template_contents.unwrap();
        assert_eq!(doc.children(contents).count(), 1);
        let script = id(&doc, "s");
        let cloned = doc.clone_node(script, script, false).unwrap();
        assert_eq!(cloned.scripts, vec![(script, cloned.root)]);

        assert_eq!(doc.compare_position(div, div), 0);
        assert_eq!(doc.compare_position(para, div), CONTAINS | PRECEDING);
        assert_eq!(doc.compare_position(div, para), CONTAINED_BY | FOLLOWING);
        assert_eq!(doc.compare_position(div, other), FOLLOWING);
        assert_eq!(doc.compare_position(other, div), PRECEDING);
        assert_eq!(
            doc.compare_position(div, deep) & (DISCONNECTED | IMPLEMENTATION_SPECIFIC),
            DISCONNECTED | IMPLEMENTATION_SPECIFIC
        );
    }

    #[test]
    fn clone_of_a_deep_chain_is_iterative() {
        let mut doc = Document::new();
        let root = doc
            .create_element(
                crate::QualName::new(None, crate::ns!(html), "div".into()),
                vec![],
            )
            .unwrap();
        let mut parent = root;
        for _ in 0..50_000 {
            let child = doc
                .create_element(
                    crate::QualName::new(None, crate::ns!(html), "div".into()),
                    vec![],
                )
                .unwrap();
            doc.append_child(parent, child).unwrap();
            parent = child;
        }
        let copy = doc.clone_node(root, root, true).unwrap().root;
        assert!(doc.is_equal_node(root, copy));
    }

    #[test]
    fn moved_form_controls_lose_the_parser_form_pointer() {
        let mut doc = parse_html(
            "<form id=f><table><tr><td><input id=i></td></tr></table></form><div id=d></div><div id=e></div>",
        );
        let (i, d, e) = (id(&doc, "i"), id(&doc, "d"), id(&doc, "e"));
        let form = id(&doc, "f");
        assert_eq!(doc.element(i).unwrap().parser_form, Some(form));
        // Moving it anywhere resets the owner: the form owner then comes
        // from the ancestors and the `form` attribute.
        doc.append_child(d, i).unwrap();
        assert_eq!(doc.element(i).unwrap().parser_form, None);
        doc.append_child(e, d).unwrap();
        doc.remove(e);
        assert_eq!(doc.element(i).unwrap().parser_form, None);
    }

    #[test]
    fn normalize_merges_and_removes() {
        let mut doc = parse_html("<div id=d></div>");
        let d = id(&doc, "d");
        let mut texts = Vec::new();
        for s in ["a", "", "b"] {
            let t = doc.create_text(s).unwrap();
            doc.append_child(d, t).unwrap();
            texts.push(t);
        }
        let e = doc
            .create_element(
                crate::QualName::new(None, crate::ns!(html), "i".into()),
                vec![],
            )
            .unwrap();
        doc.append_child(d, e).unwrap();
        let t = doc.create_text("").unwrap();
        doc.append_child(d, t).unwrap();
        doc.normalize(d).unwrap();
        let kids: Vec<NodeId> = doc.children(d).collect();
        assert_eq!(kids, vec![texts[0], e]);
        assert_eq!(doc.char_data(texts[0]), Some("ab"));
    }
}
