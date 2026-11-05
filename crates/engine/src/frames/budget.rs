//! The page-wide budget for the documents of frames (ADR 0031 section 11).
//! Each frame has its own DOM, style and layout, and the per-document
//! limits (64 MiB of body, 1 Mi nodes) multiply by the frame count. The
//! budget caps what the frame documents that are alive at the same time
//! cost together: the bytes of their source text (checked before the parse)
//! and their DOM nodes (checked after it). A document past the budget
//! becomes an error document, which costs nothing. A document leaves the
//! budget when its frame is destroyed or navigates.

/// The most source text of frame documents that are alive at the same time
/// in a page: the `srcdoc` text and the bodies of network documents.
pub const MAX_SOURCE_BYTES: usize = 64 << 20;

/// The most DOM nodes of frame documents that are alive at the same time in
/// a page: as many as the parser accepts for one document. A document of
/// 1 Mi nodes (500,000 `p` elements with text) cost 1.5 GiB of DOM, style
/// and layout (measured, ADR 0031 section 11), so the frame documents of a
/// page cost at most about as much as a second top-level document.
pub const MAX_NODES: usize = 1 << 20;

/// The most decoded pixels (RGBA, 4 bytes each) that the raster images of
/// frame documents that are alive at the same time hold together. An image
/// past it fails. (The top-level document has no such budget; a raster image
/// of 64 Mpx is 256 MiB.)
pub const MAX_IMAGE_BYTES: usize = 512 << 20;

/// The most bytes of external style sheets of frame documents that are alive
/// at the same time. A sheet of 13 MB (1,000,000 rules) cost 1.4 GiB of
/// parsed rules (measured), so the budget bounds them to about 1 GiB. A
/// sheet past it fails. (The top-level document has no such budget.)
pub const MAX_STYLE_BYTES: usize = 8 << 20;

/// The most bytes of decoded fonts of frame documents that are alive at the
/// same time: the decoded files and a share for each face that loads
/// (`crate::web_fonts::FACE_COST`). One font file may decode to 64 MiB, and
/// each document decodes its own files. A font past it fails, and the text
/// uses the next family. (The top-level document has no such budget.)
pub const MAX_FONT_BYTES: usize = 256 << 20;

/// What a frame document costs the budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Charge {
    /// Bytes of source text.
    pub(crate) bytes: usize,
    /// DOM nodes.
    pub(crate) nodes: usize,
}

/// The most nodes that a document may have after a document did not fit,
/// until the room has grown by [`SQUEEZE_MARGIN`]: a document that is
/// rejected after its parse has cost the parse, so the following ones stop
/// early (a page of 1,000 frames with documents just too big for the room
/// that is left would otherwise parse each of them in full). A document that
/// reaches this limit shows the error document, not its first nodes.
const SQUEEZED_NODES: usize = 16 * 1024;

/// How many nodes must go since the last rejection to lift
/// [`SQUEEZED_NODES`]. A frame with a small document that navigates to a big
/// one gives its small document back when it is rejected, and that must not
/// pay for the full parse of the next big one.
const SQUEEZE_MARGIN: usize = 64 * 1024;

/// The sum of the charges of the live frame documents.
#[derive(Debug, Default)]
pub(crate) struct Budget {
    total: Charge,
    /// True after the warning (per top-level document).
    pub(crate) warned: bool,
    /// The nodes in use at the last rejection, until the room has grown by
    /// [`SQUEEZE_MARGIN`].
    squeezed_at: Option<usize>,
}

impl Budget {
    /// True if `bytes` of source fit when they replace a document that costs
    /// `old`.
    pub(crate) fn fits_bytes(&self, old: Charge, bytes: usize) -> bool {
        self.total
            .bytes
            .saturating_sub(old.bytes)
            .saturating_add(bytes)
            <= MAX_SOURCE_BYTES
    }

    /// True if `nodes` DOM nodes fit when they replace a document that costs
    /// `old`.
    pub(crate) fn fits_nodes(&self, old: Charge, nodes: usize) -> bool {
        self.total
            .nodes
            .saturating_sub(old.nodes)
            .saturating_add(nodes)
            <= MAX_NODES
    }

    /// How many nodes a document that replaces one that costs `old` may
    /// have.
    pub(crate) fn nodes_left(&self, old: Charge) -> usize {
        let left = MAX_NODES.saturating_sub(self.total.nodes.saturating_sub(old.nodes));
        if self.squeezed_at.is_some() {
            left.min(SQUEEZED_NODES)
        } else {
            left
        }
    }

    /// Notes that a document did not fit.
    pub(crate) fn note_rejection(&mut self) {
        self.squeezed_at = Some(self.total.nodes);
    }

    /// Replaces the charge `old` of a document with `new`.
    pub(crate) fn replace(&mut self, old: Charge, new: Charge) {
        self.take(old);
        self.total.bytes = self.total.bytes.saturating_add(new.bytes);
        self.total.nodes = self.total.nodes.saturating_add(new.nodes);
        self.lift_squeeze();
    }

    /// Takes the charge `old` of a document that goes.
    pub(crate) fn release(&mut self, old: Charge) {
        self.take(old);
        self.lift_squeeze();
    }

    fn take(&mut self, old: Charge) {
        self.total.bytes = self.total.bytes.saturating_sub(old.bytes);
        self.total.nodes = self.total.nodes.saturating_sub(old.nodes);
    }

    /// Ends the limit of the parse once enough nodes have gone since the
    /// last rejection.
    fn lift_squeeze(&mut self) {
        if self
            .squeezed_at
            .is_some_and(|at| at.saturating_sub(self.total.nodes) >= SQUEEZE_MARGIN)
        {
            self.squeezed_at = None;
        }
    }

    /// The sum of the charges.
    pub(crate) fn total(&self) -> Charge {
        self.total
    }

    /// Resets for a new top-level document.
    pub(crate) fn reset(&mut self) {
        self.warned = false;
        self.squeezed_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_document_pays_the_difference() {
        let mut budget = Budget::default();
        let big = Charge {
            bytes: MAX_SOURCE_BYTES,
            nodes: MAX_NODES,
        };
        assert!(budget.fits_bytes(Charge::default(), MAX_SOURCE_BYTES));
        assert!(!budget.fits_bytes(Charge::default(), MAX_SOURCE_BYTES + 1));
        budget.replace(Charge::default(), big);
        assert!(!budget.fits_bytes(Charge::default(), 1));
        assert!(!budget.fits_nodes(Charge::default(), 1));
        // The same frame navigates: its own charge leaves first.
        assert!(budget.fits_bytes(big, MAX_SOURCE_BYTES));
        assert!(budget.fits_nodes(big, MAX_NODES));
        assert!(!budget.fits_nodes(big, MAX_NODES + 1));
        budget.release(big);
        assert_eq!(budget.total(), Charge::default());
        // A release without a charge does not underflow.
        budget.release(big);
        assert_eq!(budget.total(), Charge::default());
    }

    #[test]
    fn a_rejection_limits_the_parse_of_the_next_documents() {
        let mut budget = Budget::default();
        let half = Charge {
            bytes: 10,
            nodes: MAX_NODES / 2,
        };
        budget.replace(Charge::default(), half);
        assert_eq!(budget.nodes_left(Charge::default()), MAX_NODES / 2);
        budget.note_rejection();
        assert_eq!(budget.nodes_left(Charge::default()), SQUEEZED_NODES);
        // Releasing nodes lifts the limit.
        budget.release(half);
        assert_eq!(budget.nodes_left(Charge::default()), MAX_NODES);
    }

    /// Frames with small documents that navigate to big ones: each rejected
    /// frame gives its small document back, and the parse stays limited.
    #[test]
    fn small_releases_do_not_lift_the_limit_of_the_parse() {
        let mut budget = Budget::default();
        let small = Charge {
            bytes: 10,
            nodes: 100,
        };
        for _ in 0..1000 {
            budget.replace(Charge::default(), small);
        }
        for _ in 0..1000 {
            budget.note_rejection();
            budget.replace(small, Charge::default());
            assert_eq!(budget.nodes_left(Charge::default()), SQUEEZED_NODES);
        }
        assert_eq!(budget.total(), Charge::default());
    }

    /// A replacement that gives back as many nodes as it takes does not make
    /// room.
    #[test]
    fn a_replacement_lifts_the_limit_only_by_what_it_makes_free() {
        let mut budget = Budget::default();
        let big = Charge {
            bytes: 10,
            nodes: 2 * SQUEEZE_MARGIN,
        };
        budget.replace(Charge::default(), big);
        budget.note_rejection();
        budget.replace(big, big);
        assert_eq!(budget.nodes_left(Charge::default()), SQUEEZED_NODES);
        budget.replace(big, Charge::default());
        assert_eq!(budget.nodes_left(Charge::default()), MAX_NODES);
    }
}
