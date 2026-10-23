//! The change journal and the mutation sink (ADR 0028 section 3).
//!
//! Every primitive change of a document's arena (a tree change, an
//! attribute change, a change of text data) is reported twice:
//!
//! - it appends an entry to the [`Journal`] of the document, which the
//!   engine drains later (resource discovery, style invalidation, derived
//!   data);
//! - it calls the installed [`MutationSink`] at once, while the document
//!   is in the state right after the change. Mutation observers need this:
//!   DOM "queue a mutation record" finds the interested observers on the
//!   tree at each change.
//!
//! The changes correspond to the DOM standard's mutation records
//! (<https://dom.spec.whatwg.org/#queueing-a-mutation-record>): one tree
//! change can add and remove several nodes (`replaceChildren`, the
//! insertion of a fragment), as one record does.
//!
//! A journal can be incomplete ([`Journal::incomplete`]): when it
//! overflowed, or when recording was off ([`Document::set_journaling`]).
//! Its consumer then treats the whole document as changed. The parse
//! without scripts (`parse_html*`) records nothing: recording costs 2 to
//! 3 % of the parse time, and the engine walks the new document once
//! anyway.

use std::any::Any;

use html5ever::QualName;

use crate::tree::{Document, NodeId};

/// The most entries that a journal holds. Past it, the journal drops its
/// entries and becomes [incomplete](Journal::incomplete). A script that
/// changes the tree in a loop cannot make the journal grow without bound.
pub const MAX_JOURNAL_ENTRIES: usize = 1 << 20;

/// The most node ids that the entries of a journal list (added and
/// removed nodes). Past it, the journal overflows as above.
const MAX_JOURNAL_NODES: usize = 1 << 22;

/// The most bytes of old attribute values that a journal keeps. Past it,
/// the journal overflows as above: the byte limit of the arena does not
/// count a replaced value, so a script that sets a long attribute in a
/// loop could otherwise fill the memory through the journal.
const MAX_JOURNAL_VALUE_BYTES: usize = 16 << 20;

/// One primitive change, as the journal and the mutation sink report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change<'a> {
    /// Nodes were added to or removed from the children of `parent`
    /// (a "childList" mutation record). An added node brings its subtree
    /// with it; a removed node takes its subtree away.
    Children {
        /// The parent whose children changed.
        parent: NodeId,
        /// The added nodes, in tree order.
        added: &'a [NodeId],
        /// The removed nodes, in their former tree order.
        removed: &'a [NodeId],
        /// The sibling before the added or removed nodes.
        prev_sibling: Option<NodeId>,
        /// The sibling after the added or removed nodes.
        next_sibling: Option<NodeId>,
    },
    /// An attribute was added, changed or removed (an "attributes" record).
    Attribute {
        /// The element.
        element: NodeId,
        /// The attribute's name (namespace and local name).
        name: &'a QualName,
        /// The value before the change; `None` if the attribute was added.
        old_value: Option<&'a str>,
    },
    /// The data of a text, comment or processing instruction node changed
    /// (a "characterData" record).
    Text {
        /// The node.
        node: NodeId,
        /// The data before the change. The mutation sink always gets it;
        /// the journal does not keep it (always `None` there), because the
        /// parser appends text in many small steps.
        old_value: Option<&'a str>,
    },
}

/// A receiver of each primitive change at the moment it happens.
///
/// The document calls it after the change, with the document in its new
/// state: for a removal, the removed node is already detached, and its
/// former parent and that parent's ancestors are where they were. Install
/// one with [`Document::set_mutation_sink`]; without one, nothing is
/// called. The sink cannot change the document.
///
/// The sink holds only Rust data (for mutation observers: the registered
/// observers per node and the queued records); it is `'static` so that it
/// can live in the document while the document is lent to a script
/// runtime, and `Any` so that its owner can reach its concrete type
/// (`doc.mutation_sink_mut()` upcast to `&mut dyn Any`).
pub trait MutationSink: Any {
    /// Reports one change of `doc`.
    fn change(&mut self, doc: &Document, change: &Change<'_>);
}

/// A range in [`Journal::nodes`].
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    start: u32,
    len: u32,
}

#[derive(Clone, Debug)]
enum Entry {
    /// One node added to or removed from `parent`: the parser's changes,
    /// kept without the shared node list.
    One {
        parent: NodeId,
        node: NodeId,
        removed: bool,
        prev_sibling: Option<NodeId>,
        next_sibling: Option<NodeId>,
    },
    Children {
        parent: NodeId,
        added: Span,
        removed: Span,
        prev_sibling: Option<NodeId>,
        next_sibling: Option<NodeId>,
    },
    /// Boxed: attribute changes are rare during a parse, and the box keeps
    /// the other entries small.
    Attribute(Box<AttributeEntry>),
    Text {
        node: NodeId,
    },
}

#[derive(Clone, Debug)]
struct AttributeEntry {
    element: NodeId,
    name: QualName,
    old_value: Option<String>,
}

/// The changes of a document since the last drain, in the order in which
/// they happened. See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct Journal {
    entries: Vec<Entry>,
    /// The added and removed nodes of all entries.
    nodes: Vec<NodeId>,
    /// The bytes of the old attribute values of all entries.
    value_bytes: usize,
    /// True if changes are missing.
    incomplete: bool,
}

impl Journal {
    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if nothing changed: no entries, and none missing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && !self.incomplete
    }

    /// True if changes are missing from the journal: more happened than it
    /// can hold ([`MAX_JOURNAL_ENTRIES`]; the entries are then dropped), or
    /// some happened while recording was off. The consumer must treat the
    /// whole document as changed.
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// Notes that a change was not recorded: the entries are of no use
    /// any more.
    pub(crate) fn skip(&mut self) {
        if !self.incomplete {
            self.clear();
        }
    }

    /// Drops the entries and marks the journal incomplete.
    fn clear(&mut self) {
        self.entries = Vec::new();
        self.nodes = Vec::new();
        self.value_bytes = 0;
        self.incomplete = true;
    }

    /// The entries, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = Change<'_>> + '_ {
        self.entries.iter().map(|entry| self.view(entry))
    }

    fn view<'a>(&'a self, entry: &'a Entry) -> Change<'a> {
        match entry {
            Entry::One {
                parent,
                node,
                removed,
                prev_sibling,
                next_sibling,
            } => {
                let node = std::slice::from_ref(node);
                let (added, removed) = if *removed {
                    (&[][..], node)
                } else {
                    (node, &[][..])
                };
                Change::Children {
                    parent: *parent,
                    added,
                    removed,
                    prev_sibling: *prev_sibling,
                    next_sibling: *next_sibling,
                }
            }
            Entry::Children {
                parent,
                added,
                removed,
                prev_sibling,
                next_sibling,
            } => Change::Children {
                parent: *parent,
                added: self.slice(*added),
                removed: self.slice(*removed),
                prev_sibling: *prev_sibling,
                next_sibling: *next_sibling,
            },
            Entry::Attribute(a) => Change::Attribute {
                element: a.element,
                name: &a.name,
                old_value: a.old_value.as_deref(),
            },
            Entry::Text { node } => Change::Text {
                node: *node,
                old_value: None,
            },
        }
    }

    fn slice(&self, span: Span) -> &[NodeId] {
        let start = span.start as usize;
        self.nodes
            .get(start..start + span.len as usize)
            .unwrap_or_default()
    }

    /// Appends an entry for `change`.
    #[inline]
    pub(crate) fn record(&mut self, change: &Change<'_>) {
        if self.incomplete {
            // The consumer treats everything as changed anyway.
            return;
        }
        let (nodes, value_bytes) = match change {
            Change::Children { added, removed, .. } => (added.len() + removed.len(), 0),
            Change::Attribute { old_value, .. } => (0, old_value.map_or(0, str::len)),
            Change::Text { .. } => (0, 0),
        };
        self.value_bytes += value_bytes;
        if self.entries.len() >= MAX_JOURNAL_ENTRIES
            || self.nodes.len() + nodes > MAX_JOURNAL_NODES
            || self.value_bytes > MAX_JOURNAL_VALUE_BYTES
        {
            self.overflow();
            return;
        }
        let entry = match *change {
            Change::Children {
                parent,
                added: &[node],
                removed: &[],
                prev_sibling,
                next_sibling,
            }
            | Change::Children {
                parent,
                added: &[],
                removed: &[node],
                prev_sibling,
                next_sibling,
            } => Entry::One {
                parent,
                node,
                removed: !matches!(change, Change::Children { added: [_], .. }),
                prev_sibling,
                next_sibling,
            },
            Change::Children {
                parent,
                added,
                removed,
                prev_sibling,
                next_sibling,
            } => Entry::Children {
                parent,
                added: self.push_nodes(added),
                removed: self.push_nodes(removed),
                prev_sibling,
                next_sibling,
            },
            Change::Attribute {
                element,
                name,
                old_value,
            } => Entry::Attribute(Box::new(AttributeEntry {
                element,
                name: name.clone(),
                old_value: old_value.map(str::to_owned),
            })),
            Change::Text { node, .. } => Entry::Text { node },
        };
        self.entries.push(entry);
    }

    #[cold]
    fn overflow(&mut self) {
        log::warn!(
            "the change journal overflowed after {} entries; the document counts as changed",
            self.entries.len()
        );
        self.clear();
    }

    fn push_nodes(&mut self, nodes: &[NodeId]) -> Span {
        // Both values fit: the node count is bounded by MAX_JOURNAL_NODES.
        let span = Span {
            start: u32::try_from(self.nodes.len()).unwrap_or(u32::MAX),
            len: u32::try_from(nodes.len()).unwrap_or(0),
        };
        self.nodes.extend_from_slice(nodes);
        span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use html5ever::{LocalName, ns};

    fn node(i: usize) -> NodeId {
        NodeId::from_index(i).unwrap()
    }

    #[test]
    fn entries_keep_their_data() {
        let mut journal = Journal::default();
        assert!(journal.is_empty());
        journal.record(&Change::Children {
            parent: node(1),
            added: &[node(2), node(3)],
            removed: &[node(4)],
            prev_sibling: Some(node(5)),
            next_sibling: None,
        });
        let name = QualName::new(None, ns!(), LocalName::from("id"));
        journal.record(&Change::Attribute {
            element: node(2),
            name: &name,
            old_value: Some("a"),
        });
        journal.record(&Change::Text {
            node: node(6),
            old_value: Some("dropped"),
        });
        let entries: Vec<_> = journal.iter().collect();
        assert_eq!(
            entries,
            [
                Change::Children {
                    parent: node(1),
                    added: &[node(2), node(3)],
                    removed: &[node(4)],
                    prev_sibling: Some(node(5)),
                    next_sibling: None,
                },
                Change::Attribute {
                    element: node(2),
                    name: &name,
                    old_value: Some("a"),
                },
                // The journal does not keep old text.
                Change::Text {
                    node: node(6),
                    old_value: None,
                },
            ]
        );
    }

    #[test]
    fn overflow_drops_the_entries() {
        let mut journal = Journal::default();
        for i in 0..=MAX_JOURNAL_ENTRIES {
            journal.record(&Change::Text {
                node: node(i),
                old_value: None,
            });
        }
        assert!(journal.incomplete());
        assert_eq!(journal.len(), 0);
        assert!(!journal.is_empty());
        journal.record(&Change::Text {
            node: node(1),
            old_value: None,
        });
        assert_eq!(journal.len(), 0, "an overflowed journal stays empty");
    }

    #[test]
    fn old_attribute_values_overflow_at_their_byte_limit() {
        let mut journal = Journal::default();
        let name = QualName::new(None, ns!(), LocalName::from("a"));
        let value = "x".repeat(1 << 20);
        for _ in 0..16 {
            journal.record(&Change::Attribute {
                element: node(1),
                name: &name,
                old_value: Some(&value),
            });
        }
        assert_eq!(journal.len(), 16);
        journal.record(&Change::Attribute {
            element: node(1),
            name: &name,
            old_value: Some("y"),
        });
        assert!(journal.incomplete());
        assert_eq!(journal.len(), 0);
    }
}
