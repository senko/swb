//! The adopted style sheets of the document and of its shadow roots (CSSOM
//! `adoptedStyleSheets`, ADR 0036 section 6). The bindings keep each list
//! equal to its script array ([`swb_web::EngineHost::adopted_sheets`]); the
//! stylist build reads them: the document's adopted sheets come after the
//! document's other sheets (and join its layer order), a shadow root's after
//! the sheets of its tree's elements (`shadow_styles.rs`).
//!
//! A sheet that an array holds more than once applies once, at its last
//! place (measured in Chromium 148, case `adopted-array`: in `[C, D, C]`
//! the rules of C win). The list of a shadow root holds the root's adoption
//! count of the last write ([`swb_dom::ShadowRootData::adoptions`]): a root
//! that moved to another document since (and maybe back) has no adopted
//! sheets, as the bindings' array, which they empty at its next use.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId};
use swb_style::cssom::sheet::SheetRef;

/// The adopted sheets of a document's scopes.
#[derive(Debug, Default)]
pub(crate) struct AdoptedSheets {
    /// The document's list.
    document: Vec<SheetRef>,
    /// The lists of shadow roots whose node document is the document.
    roots: HashMap<NodeId, RootSheets>,
    /// The connected roots with adopted sheets at the last stylist build,
    /// sorted.
    applied: Vec<NodeId>,
    /// The structure generation of the arena when the stylist was last
    /// built or found up to date ([`AdoptedSheets::out_of_date`]): a root
    /// is connected, or moves to another document while connected, only
    /// with a change of a child list, which changes the generation.
    checked: Option<u64>,
}

/// The list of a shadow root.
#[derive(Debug)]
struct RootSheets {
    /// The root's adoption count at the last write.
    adoptions: u32,
    sheets: Vec<SheetRef>,
}

impl AdoptedSheets {
    /// The list of `scope` for a change by the bindings: the document's, or
    /// the list of a shadow root whose node document is the document (an
    /// empty one if the root moved since the last write). `None` for
    /// another node.
    pub(crate) fn list_mut(&mut self, dom: &Document, scope: NodeId) -> Option<&mut Vec<SheetRef>> {
        if scope == NodeId::DOCUMENT {
            return Some(&mut self.document);
        }
        let adoptions = dom.shadow_root_data(scope)?.adoptions;
        if dom.node_document(scope) != NodeId::DOCUMENT {
            return None;
        }
        let entry = self.roots.entry(scope).or_insert(RootSheets {
            adoptions,
            sheets: Vec::new(),
        });
        if entry.adoptions != adoptions {
            entry.adoptions = adoptions;
            entry.sheets.clear();
        }
        Some(&mut entry.sheets)
    }

    /// True if a stylist that the last [`AdoptedSheets::root_sheets`] fed
    /// is out of date: a shadow root with adopted sheets moved to another
    /// document since their last write, or a root with adopted sheets is
    /// connected now that was not then (a DOM change keeps the stylist
    /// otherwise; a root that leaves the document needs no new stylist).
    /// The roots are looked at only after a change of a child list since
    /// the last look: a style query in a loop costs nothing here.
    pub(crate) fn out_of_date(&mut self, dom: &Document) -> bool {
        let generation = dom.structure_generation();
        if self.roots.is_empty() || self.checked == Some(generation) {
            return false;
        }
        // `is_connected` walks the hosts up to the document, so it runs
        // only for the roots that the last build did not apply.
        let stale = self.roots.iter().any(|(&root, entry)| {
            !entry.sheets.is_empty()
                && (!current(dom, root, entry.adoptions)
                    || (self.applied.binary_search(&root).is_err() && dom.is_connected(root)))
        });
        if !stale {
            self.checked = Some(generation);
        }
        stale
    }

    /// The document's sheets that apply in `env`, each once at its last
    /// place.
    pub(crate) fn document_sheets(&self, env: &MediaEnvironment) -> Vec<SheetRef> {
        applying(&self.document, env)
    }

    /// The connected shadow roots of the document that have sheets that
    /// apply in `env`, with those sheets, in the order of their ids. The
    /// lists of nodes that are no shadow root of the document any more, or
    /// of roots that moved since their last write, go.
    pub(crate) fn root_sheets(
        &mut self,
        dom: &Document,
        env: &MediaEnvironment,
    ) -> Vec<(NodeId, Vec<SheetRef>)> {
        self.applied.clear();
        self.checked = Some(dom.structure_generation());
        if self.roots.is_empty() {
            return Vec::new();
        }
        self.roots
            .retain(|&root, entry| current(dom, root, entry.adoptions));
        self.applied = self
            .roots
            .iter()
            .filter(|(root, entry)| !entry.sheets.is_empty() && dom.is_connected(**root))
            .map(|(&root, _)| root)
            .collect();
        self.applied.sort_unstable();
        self.applied
            .iter()
            .filter_map(|root| {
                let sheets = applying(&self.roots.get(root)?.sheets, env);
                (!sheets.is_empty()).then_some((*root, sheets))
            })
            .collect()
    }
}

/// True if `root` is still a shadow root of the document with the adoption
/// count `adoptions`.
fn current(dom: &Document, root: NodeId, adoptions: u32) -> bool {
    dom.get(root).is_some()
        && dom.node_document(root) == NodeId::DOCUMENT
        && dom
            .shadow_root_data(root)
            .is_some_and(|data| data.adoptions == adoptions)
}

/// The sheets of `list` that apply in `env` (not disabled, their media
/// match), each once at its last place, in the order of the list.
fn applying(list: &[SheetRef], env: &MediaEnvironment) -> Vec<SheetRef> {
    let mut seen = HashSet::new();
    let mut sheets: Vec<SheetRef> = list
        .iter()
        .rev()
        .filter(|sheet| seen.insert(Rc::as_ptr(sheet)))
        .filter(|sheet| {
            let sheet = sheet.borrow();
            !sheet.disabled && sheet.media.matches(env)
        })
        .cloned()
        .collect();
    sheets.reverse();
    sheets
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::MediaQueryList;
    use swb_style::cssom::sheet::{StyleMemory, StyleSheet};

    fn sheet(memory: &StyleMemory, media: &str) -> SheetRef {
        let base = swb_net::Url::parse("https://a.test/").unwrap();
        let media = MediaQueryList::parse_str(media);
        StyleSheet::construct(memory, false, base, media, false)
    }

    #[test]
    fn each_sheet_applies_once_at_its_last_place() {
        let memory = StyleMemory::default();
        let (a, b) = (sheet(&memory, ""), sheet(&memory, ""));
        let print = sheet(&memory, "print");
        let disabled = sheet(&memory, "");
        disabled.borrow_mut().disabled = true;
        let list = [
            Rc::clone(&a),
            Rc::clone(&b),
            Rc::clone(&print),
            Rc::clone(&a),
            Rc::clone(&disabled),
        ];
        let got = applying(&list, &MediaEnvironment::default());
        assert_eq!(got.len(), 2);
        assert!(Rc::ptr_eq(&got[0], &b) && Rc::ptr_eq(&got[1], &a));
    }
}
