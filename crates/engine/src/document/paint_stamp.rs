//! The inputs of a document's last frame, compared without running style or
//! layout (ADR 0040 section 2): [`PaintStamp`] holds what the pixels of the
//! document depend on that is not a display list, and the serial of the
//! display list; [`DocumentState::paint_pending`] tells whether the next
//! render would change the pipeline's state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use swb_dom::NodeId;
use swb_layout::Point;

use super::{DocumentState, View};

/// The serial of the next display list. One counter for all documents, so
/// that the lists of two documents (and of a frame's old and new document)
/// never share a serial. 0 means "no list".
static NEXT_LIST_SERIAL: AtomicU64 = AtomicU64::new(1);

/// A new display list serial.
pub(super) fn next_list_serial() -> u64 {
    NEXT_LIST_SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// What a rendering of a document drew with: two stamps that are equal
/// give the same pixels.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaintStamp {
    /// The serial of the display list (0: there was none).
    list: u64,
    /// The scroll offset of the viewport.
    scroll: Point,
    /// The scroll offsets of the scroll containers that are not zero. The
    /// display list is built with them, and a scroll drops it; the stamp
    /// holds them too, so that a scroll that skipped the drop still counts.
    offsets: HashMap<NodeId, Point>,
    /// The viewport, the scale, the indicators and the color scheme.
    view: View,
}

impl DocumentState {
    /// The inputs of a rendering of the document now.
    pub(crate) fn paint_stamp(&self) -> PaintStamp {
        PaintStamp {
            list: if self.display_list.is_some() {
                self.list_serial
            } else {
                0
            },
            scroll: self.scroll,
            offsets: self.scrollers.offsets().clone(),
            view: self.view,
        }
    }

    /// True if a rendering would do work that can change the pixels: the
    /// DOM has changes that the pipeline has not taken, the styles, the
    /// layout or the display list are outdated or missing (the styles of a
    /// document that waits for its stylesheets are missing and nothing can
    /// build them), or the selection changed.
    pub(crate) fn paint_pending(&self) -> bool {
        if self.retired {
            return false;
        }
        if self.drained != Some(self.dom.generation()) || !self.dom.journal().is_empty() {
            return true;
        }
        // `controls_stale` and `images.sources_outdated` do not count: a
        // render does not consume them (the next style computation does),
        // and every change that needs the styles recomputed also drops them.
        if self.styles_stale {
            return true;
        }
        if self.styles.is_none() {
            return self.stylesheets_settled();
        }
        if self.fragments.is_none() || self.display_list.is_none() {
            return true;
        }
        self.dom.selection_generation() != self.display_selection.0
    }
}
