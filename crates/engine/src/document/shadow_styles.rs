//! The stylists of the sheets of shadow trees (ADR 0036 section 5): the
//! `<style>` and `<link rel=stylesheet>` elements of a connected shadow
//! tree, and then the root's adopted sheets (section 6), apply to that tree
//! only. Each shadow root that has sheets gets a
//! scoped stylist ([`Stylist::new_scoped`]) inside the document's stylist;
//! roots with the same list of sheets share one, so that a page that puts
//! the same `<style>` into every root, or adopts the same sheet in every
//! root (GitHub's popover polyfill), builds
//! one. A sheet is the same as another when no script changed its rules and
//! it has the same text and base URL; a sheet that a script changed, and an
//! adopted sheet, is only the same as itself.
//!
//! The scoped stylists are built with the document's stylist and dropped
//! with it (`invalidate_style`). Their estimated bytes count in the
//! document's style memory (ADR 0028 section 16, ADR 0036 section 12), as
//! do the parsed sheets of shadow trees (`SheetSlot::sheet`): past the
//! budget, sheets are not parsed and stylists not built, with one warning.

use std::collections::HashMap;
use std::rc::Rc;

use swb_css::MediaEnvironment;
use swb_dom::NodeId;
use swb_net::Url;
use swb_style::Stylist;
use swb_style::cssom::sheet::SheetRef;

use super::DocumentState;

/// What makes two sheets of shadow trees give the same rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SheetKey<'a> {
    /// A sheet whose rules are the parse of its text: the text and the base
    /// URL of its `url()` values.
    Text(&'a str, &'a Url),
    /// A sheet that a script changed, or an adopted sheet: its identity.
    Object(usize),
}

/// A sheet that applies to a shadow root.
#[derive(Clone, Debug)]
enum ScopeSheet {
    /// The sheet of the slot at this index (an element of the tree).
    Slot(usize),
    /// An adopted sheet that applies.
    Adopted(SheetRef),
}

impl DocumentState {
    /// Adds the scoped stylists of the sheets at `slots` (all of shadow
    /// trees, in tree order) and of the roots' adopted sheets that apply
    /// (`adopted`, from `AdoptedSheets::root_sheets`) to `stylist` and
    /// assigns them to their roots.
    pub(super) fn add_shadow_scopes(
        &mut self,
        stylist: &mut Stylist,
        slots: &[usize],
        adopted: Vec<(NodeId, Vec<SheetRef>)>,
        env: &MediaEnvironment,
    ) {
        // The applying sheets of each root, and the roots of each distinct
        // list of sheets (keyed by the list).
        let mut lists: Vec<(Vec<ScopeSheet>, Vec<NodeId>)> = Vec::new();
        {
            let mut roots: Vec<(NodeId, Vec<ScopeSheet>)> = Vec::new();
            let mut root_index: HashMap<NodeId, usize> = HashMap::new();
            let mut list_of = |root: NodeId, roots: &mut Vec<(NodeId, Vec<ScopeSheet>)>| {
                *root_index.entry(root).or_insert_with(|| {
                    roots.push((root, Vec::new()));
                    roots.len() - 1
                })
            };
            for &index in slots {
                let Some(slot) = self.sheets.get(index) else {
                    continue;
                };
                if !self.shadow_sheet_applies(index, env) {
                    continue;
                }
                let i = list_of(slot.scope, &mut roots);
                if let Some((_, list)) = roots.get_mut(i) {
                    list.push(ScopeSheet::Slot(index));
                }
            }
            // A root's adopted sheets come after its elements' sheets.
            for (root, sheets) in adopted {
                let i = list_of(root, &mut roots);
                if let Some((_, list)) = roots.get_mut(i) {
                    list.extend(sheets.into_iter().map(ScopeSheet::Adopted));
                }
            }
            let mut by_key: HashMap<Vec<SheetKey<'_>>, usize> = HashMap::new();
            for (root, list) in roots {
                let key: Vec<SheetKey<'_>> =
                    list.iter().filter_map(|s| self.sheet_key(s)).collect();
                let i = *by_key.entry(key).or_insert_with(|| {
                    lists.push((list, Vec::new()));
                    lists.len() - 1
                });
                if let Some((_, roots)) = lists.get_mut(i) {
                    roots.push(root);
                }
            }
        }
        for (list, roots) in lists {
            if let Some(index) = self.build_scope(stylist, &list, env) {
                for root in roots {
                    stylist.set_root_scope(root, index);
                }
            }
        }
    }

    /// True if the sheet at `index` applies: not disabled, and its media
    /// match (the `media` attribute, or the CSSOM sheet's media once it was
    /// made).
    fn shadow_sheet_applies(&self, index: usize, env: &MediaEnvironment) -> bool {
        let Some(slot) = self.sheets.get(index) else {
            return false;
        };
        if !slot.is_settled() {
            return false;
        }
        match slot.existing_sheet() {
            Some(sheet) => {
                let sheet = sheet.borrow();
                !sheet.disabled && sheet.media.matches(env)
            }
            None => slot
                .media
                .as_deref()
                .is_none_or(|m| swb_css::MediaQueryList::parse_str(m).matches(env)),
        }
    }

    /// The key of `sheet` (see [`SheetKey`]).
    fn sheet_key(&self, sheet: &ScopeSheet) -> Option<SheetKey<'_>> {
        let index = match sheet {
            ScopeSheet::Slot(index) => *index,
            ScopeSheet::Adopted(sheet) => {
                return Some(SheetKey::Object(Rc::as_ptr(sheet) as usize));
            }
        };
        let slot = self.sheets.get(index)?;
        if let Some(sheet) = slot.existing_sheet()
            && sheet.borrow().is_modified()
        {
            return Some(SheetKey::Object(Rc::as_ptr(sheet) as usize));
        }
        Some(SheetKey::Text(slot.css().unwrap_or(""), &slot.base_url))
    }

    /// Builds the scoped stylist of the sheets of `list` and adds it to
    /// `stylist`; `None` if it has no rules or its estimated bytes do not
    /// fit in the document's style memory.
    fn build_scope(
        &mut self,
        stylist: &mut Stylist,
        list: &[ScopeSheet],
        env: &MediaEnvironment,
    ) -> Option<u32> {
        let mut scoped = Stylist::new_scoped(self.dom.quirks_mode);
        for entry in list {
            match entry {
                ScopeSheet::Slot(index) => {
                    let Some(sheet) = self.cssom_sheet(*index) else {
                        continue;
                    };
                    let sheet = sheet.borrow();
                    if sheet.disabled || !sheet.media.matches(env) {
                        continue;
                    }
                    if let Some(slot) = self.sheets.get(*index) {
                        scoped.add_cssom_sheet(&sheet, &slot.base_url);
                    }
                }
                ScopeSheet::Adopted(sheet) => {
                    let sheet = sheet.borrow();
                    if let Some(base) = sheet.constructed_base_url() {
                        scoped.add_cssom_sheet(&sheet, base);
                    }
                }
            }
        }
        if scoped.is_empty() {
            return None;
        }
        let bytes = swb_style::SCOPE_BYTES
            .saturating_add(scoped.len().saturating_mul(swb_style::SCOPE_RULE_BYTES));
        let hold = self
            .style_memory
            .reserve(bytes, "the style rules of a shadow tree")?;
        Some(stylist.add_scope(scoped, hold))
    }
}
