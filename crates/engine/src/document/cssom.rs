//! The style sheets of a document for scripts (CSSOM, ADR 0028 section
//! 10): `document.styleSheets` and the `sheet` of `style` and `link`
//! elements. The CSSOM sheet of a slot is the one that the stylist is built
//! from (`resources::SheetSlot::sheet`), so a script's change is a change
//! of the rules that the next style flush uses.

use swb_dom::{NodeId, QuirksMode};
use swb_net::Url;
use swb_style::cssom::sheet::SheetRef;

use super::DocumentState;
use crate::resources::SheetSource;

impl DocumentState {
    /// The CSSOM sheet of the slot at `index`, made at its first use; `None`
    /// while a `link` loads.
    pub(crate) fn cssom_sheet(&mut self, index: usize) -> Option<SheetRef> {
        let quirks = self.dom.quirks_mode == QuirksMode::Quirks;
        let slot = self.sheets.cssom_slot(index)?;
        let clean = match &slot.source {
            SheetSource::Inline => true,
            SheetSource::Link(_) => origin_clean(&self.url, &slot.base_url),
        };
        slot.sheet(quirks, clean, &self.style_memory)
    }

    /// The number of sheets of `document.styleSheets`, after the DOM
    /// changes.
    pub(crate) fn script_sheet_count(&mut self) -> usize {
        self.process_changes();
        self.sheets.settled().len()
    }

    /// The sheet at `index` of `document.styleSheets`.
    pub(crate) fn script_sheet(&mut self, index: usize) -> Option<SheetRef> {
        self.process_changes();
        let slot = *self.sheets.settled().get(index)?;
        self.cssom_sheet(slot)
    }

    /// The number of sheets of `ShadowRoot.styleSheets` of the shadow root
    /// `scope`, after the DOM changes.
    pub(crate) fn script_sheet_count_in(&mut self, scope: NodeId) -> usize {
        self.process_changes();
        self.sheets.settled_in(scope).len()
    }

    /// The sheet at `index` of `ShadowRoot.styleSheets` of `scope`.
    pub(crate) fn script_sheet_in(&mut self, scope: NodeId, index: usize) -> Option<SheetRef> {
        self.process_changes();
        let slot = *self.sheets.settled_in(scope).get(index)?;
        self.cssom_sheet(slot)
    }

    /// The sheet of the element `owner`, after the DOM changes.
    pub(crate) fn script_sheet_of(&mut self, owner: NodeId) -> Option<SheetRef> {
        self.process_changes();
        let index = self.sheets.position(owner)?;
        self.cssom_sheet(index)
    }
}

/// The origin-clean flag of a sheet loaded from `sheet` by the document at
/// `document`: true for the same origin. Chromium measured: the rules of a
/// sheet of another origin throw `SecurityError`. A `file:` sheet of a
/// `file:` document and a `data:` sheet count as clean (swb's choice; the
/// `url` crate gives both opaque origins).
fn origin_clean(document: &Url, sheet: &Url) -> bool {
    match sheet.scheme() {
        "data" => true,
        "file" => document.scheme() == "file",
        _ => sheet.origin() == document.origin(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        let page = Url::parse("https://a.test/p").unwrap();
        let same = Url::parse("https://a.test/s.css").unwrap();
        let other = Url::parse("https://b.test/s.css").unwrap();
        assert!(origin_clean(&page, &same));
        assert!(!origin_clean(&page, &other));
        assert!(origin_clean(
            &page,
            &Url::parse("data:text/css,a{}").unwrap()
        ));
        let file = Url::parse("file:///tmp/p.html").unwrap();
        assert!(origin_clean(
            &file,
            &Url::parse("file:///tmp/s.css").unwrap()
        ));
        assert!(!origin_clean(
            &page,
            &Url::parse("file:///tmp/s.css").unwrap()
        ));
    }
}
