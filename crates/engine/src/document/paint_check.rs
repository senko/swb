//! The check mode of the paint-only path (ADR 0044 section 9), for tests
//! and debug builds: when the restyle check is on, a frame whose layout
//! went through a paint-only refresh also runs a full layout and display
//! list and compares them with the refreshed ones. The fragment trees are
//! compared box by box (geometry, styles, scrollable overflow, the scroll
//! size: what scripts and hit testing read), the display lists item by
//! item. A difference panics with the first differing fragment or item.
//! The check is the oracle for the table of paint properties
//! (`swb_style::paint`): a property that the check finds different is a
//! bug of the table or of the refresh.

use swb_dom::{LiveRange, NodeId};
use swb_text::FontContext;

use super::DocumentState;

impl DocumentState {
    /// Compares the refreshed fragments and the display list that was just
    /// built from them with a full layout of the current styles, if the
    /// fragments were refreshed since the last check.
    pub(super) fn check_paint_refresh(
        &mut self,
        fonts: &mut FontContext,
        page: Option<LiveRange>,
        control: Option<(NodeId, u32, u32)>,
    ) {
        if !std::mem::take(&mut self.paint_refreshed) || !super::restyle_check::enabled() {
            return;
        }
        let (Some(styles), Some(refreshed), Some(list)) =
            (&self.styles, &self.fragments, &self.display_list)
        else {
            return;
        };
        let full = self.layout_with(styles, fonts);
        let difference = refreshed.first_difference(&full);
        assert!(
            difference.is_none(),
            "the paint-only refresh of {} differs from a full layout: {}",
            self.url.as_str(),
            difference.unwrap_or_default()
        );
        let full_list = self.display_list_of(&full, page, control);
        assert!(
            list.items.len() == full_list.items.len(),
            "the display list of the paint-only refresh of {} has {} items, a full layout \
             gives {}",
            self.url.as_str(),
            list.items.len(),
            full_list.items.len()
        );
        for (i, (a, b)) in list.items.iter().zip(&full_list.items).enumerate() {
            let (a, b) = (format!("{a:?}"), format!("{b:?}"));
            assert!(
                a == b,
                "the display list of the paint-only refresh of {} differs from a full layout \
                 at item {i}:\n  refreshed: {a}\n  full:      {b}",
                self.url.as_str()
            );
        }
        super::restyle_check::count_paint();
    }
}
