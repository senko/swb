//! The inputs of a document's last frame, compared without running style or
//! layout (ADR 0040 section 2): [`PaintStamp`] holds what the pixels of the
//! document depend on that is not a display list, and the serial of the
//! display list; [`DocumentState::paint_pending`] tells whether the next
//! render would change the pipeline's state.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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
    /// The generation bumps of the document's images (media frames).
    media: u64,
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
            media: self.images.media_generation,
        }
    }

    /// A hash of [`DocumentState::paint_stamp`]: equal stamps give equal
    /// numbers. For the media key of the document's frame in the damage of
    /// its parent (ADR 0044 section 18).
    pub(crate) fn paint_generation(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let stamp = self.paint_stamp();
        stamp.list.hash(&mut hasher);
        stamp.scroll.x.to_bits().hash(&mut hasher);
        stamp.scroll.y.to_bits().hash(&mut hasher);
        let mut offsets: Vec<(NodeId, u32, u32)> = stamp
            .offsets
            .iter()
            .map(|(node, p)| (*node, p.x.to_bits(), p.y.to_bits()))
            .collect();
        offsets.sort_unstable_by_key(|(node, _, _)| *node);
        offsets.hash(&mut hasher);
        format!("{:?}", stamp.view).hash(&mut hasher);
        stamp.media.hash(&mut hasher);
        hasher.finish()
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
        // Animated values changed (ADR 0044 section 8), also by the
        // catch-up of the timeline that tasks asked for; only those that
        // can change visible pixels count (the visibility test), so the
        // tasks of a page with an offscreen animation draw no frames.
        if self.styles.is_some() {
            self.settle_animations();
            if self.animations.borrow().has_shown_marks() {
                return true;
            }
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
