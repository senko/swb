//! The budget of retained frames (M14 feature 10a, ADR 0047
//! "Implementation notes (M14 feature 10a)"): a video element whose
//! pipeline holds no place (suspended, waiting, failed) keeps its current
//! frame as the image that paint draws. The frame budget of the pipelines
//! (`swb_media::pipeline::PipelineSlots`) no longer counts these frames, so
//! the document bounds them: at most [`RETAINED_FRAME_BUDGET`] bytes. Over
//! the budget (review fixes of 10a):
//!
//! 1. the frames of elements without a painted box go;
//! 2. the frames of elements with a box, least recently painted first (and
//!    among equals the oldest retained frame), are replaced by a reduced
//!    copy at the size of the box in device pixels
//!    ([`swb_paint::reduced_copy`]: the level that paint draws the frame
//!    from at that size, so the picture differs by at most 1 in a color
//!    channel), when the copy is smaller;
//! 3. while the total is still over the budget, frames go in the order of
//!    step 2.
//!
//! One sync converts at most [`REDUCE_PIXELS_PER_SYNC`] source pixels (step
//! 2): the page thread does not block for seconds when many large frames
//! are over the budget. The sync that stops at the cap leaves a backlog:
//! [`Media::next_due`] reports a due time [`REDUCE_RETRY_MS`] later, so the
//! loops (real and virtual time) run the next sync without another trigger.
//! While the backlog waits, step 3 holds the total at the budget plus
//! [`RETAINED_MARGIN`] only (and keeps the reduced frames), so the memory
//! stays bounded.
//!
//! A dropped frame comes back through the painted flag's quiet seek
//! (`swb_media::pipeline::Pipeline::set_painted`) when its element is
//! painted again; one that is dropped while it is painted (more painted
//! videos than the budget holds, even reduced) does not come back until the
//! element plays, seeks or is painted again, so the budget never makes a
//! cycle of drops and seeks. A reduced frame whose box grows past the copy
//! is decoded again at full size by the same quiet seek
//! ([`Element::reduced_frame_too_small`]); it is reduced again only for a
//! box that it does not cover, so this makes no cycle either.
//!
//! The document keeps the bytes of each element up to date for the elements
//! that its sync looks at (the touched ones, or all), so a page with
//! thousands of elements does not count all of them after each unit of
//! media work.

use std::collections::HashMap;

use swb_dom::NodeId;

use super::Media;

/// The bytes of retained frames that a document keeps at most: 256 MiB
/// (thirty 1080p frames, seven 4K frames, one frame at the size limit; 870
/// frames of 320 x 240).
pub(crate) const RETAINED_FRAME_BUDGET: u64 = 256 << 20;

/// The source pixels that the reductions of one sync convert at most: 8 Mpx
/// (4 frames of 1080p, one of 4K; a frame larger than that goes alone). The
/// page thread blocks for one sync's worth at most: about 15 ms in a
/// release build (a 1080p frame 3.5 to 4 ms, a 4K frame 14.5 ms, measured;
/// see ADR 0047, notes of 10a). A count of pixels, not a
/// time: virtual time stays deterministic.
pub(crate) const REDUCE_PIXELS_PER_SYNC: u64 = 8 << 20;

/// The bytes over the budget that the retained frames may hold while
/// reductions wait for later syncs: 64 MiB, the bytes that two full syncs
/// free (2 x 8 Mpx of 4-byte pixels). Past it, frames go (in the order of
/// step 3) instead, so the memory stays bounded at budget + margin.
pub(crate) const RETAINED_MARGIN: u64 = 64 << 20;

/// The page time (ms) from a sync that left reductions to the next one: no
/// spin, and the page thread has time for input between two syncs.
const REDUCE_RETRY_MS: f64 = 10.0;

/// What one try to reduce a frame did.
enum Reduction {
    /// Nothing to do (no box, already small, not smaller).
    Skipped,
    /// Replaced by a copy; the source pixels that it converted.
    Done(u64),
    /// The sync's pixels are spent: stop.
    Capped,
}

/// The retained frames of a document.
#[derive(Default)]
pub(crate) struct Retained {
    /// The bytes of each element that keeps a frame, and the serial of the
    /// moment it started to keep it.
    entries: HashMap<NodeId, (u64, u64)>,
    total: u64,
    serial: u64,
    /// Frames dropped for the budget (tests).
    dropped: u64,
    /// Frames replaced by a reduced copy for the budget (tests).
    reduced: u64,
    /// The last sync stopped at [`REDUCE_PIXELS_PER_SYNC`] with the total
    /// still over the budget: the next sync continues
    /// ([`Media::reduce_due`]).
    backlog: bool,
    /// The most source pixels that one sync reduced, and the number of
    /// syncs that reduced a frame (tests).
    max_sync_pixels: u64,
    reducing_syncs: u64,
}

impl Retained {
    /// Forgets every entry (the elements went).
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.total = 0;
        self.backlog = false;
    }

    /// The most source pixels that one sync reduced (tests).
    pub(crate) fn max_sync_pixels(&self) -> u64 {
        self.max_sync_pixels
    }

    /// The number of syncs that reduced at least one frame (tests).
    pub(crate) fn reducing_syncs(&self) -> u64 {
        self.reducing_syncs
    }

    /// Records that `node` keeps `bytes` of a retained frame (0: none).
    fn set(&mut self, node: NodeId, bytes: u64) {
        let old = match self.entries.get_mut(&node) {
            Some(entry) if bytes > 0 => std::mem::replace(&mut entry.0, bytes),
            Some(_) => self.entries.remove(&node).map_or(0, |(b, _)| b),
            None if bytes > 0 => {
                self.serial += 1;
                self.entries.insert(node, (bytes, self.serial));
                0
            }
            None => 0,
        };
        self.total = self.total.saturating_sub(old).saturating_add(bytes);
    }

    /// The bytes of retained frames now.
    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    /// Frames dropped for the budget so far.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Frames replaced by a reduced copy for the budget so far.
    pub(crate) fn reduced(&self) -> u64 {
        self.reduced
    }
}

impl Media {
    /// Looks at the retained frame of `node` again (after a unit of media
    /// work, a script call, a change of the painted flag).
    pub(crate) fn note_retained(&mut self, node: NodeId) {
        let bytes = self.retained_bytes_of(node);
        self.retained.set(node, bytes);
    }

    /// Looks at the retained frames of all elements again.
    pub(crate) fn rebuild_retained(&mut self) {
        let serials: HashMap<NodeId, u64> = self
            .retained
            .entries
            .iter()
            .map(|(&n, &(_, s))| (n, s))
            .collect();
        let mut entries = HashMap::new();
        let mut total = 0u64;
        for (&node, element) in &self.elements {
            let bytes = element
                .fetch
                .as_ref()
                .filter(|_| element.video)
                .map_or(0, super::Fetch::retained_bytes);
            if bytes > 0 {
                let serial = serials.get(&node).copied().unwrap_or_else(|| {
                    self.retained.serial += 1;
                    self.retained.serial
                });
                entries.insert(node, (bytes, serial));
                total = total.saturating_add(bytes);
            }
        }
        self.retained.entries = entries;
        self.retained.total = total;
    }

    fn retained_bytes_of(&self, node: NodeId) -> u64 {
        self.elements
            .get(&node)
            .filter(|e| e.video)
            .and_then(|e| e.fetch.as_ref())
            .map_or(0, super::Fetch::retained_bytes)
    }

    /// Brings the retained frames within the budget, in the steps of the
    /// module comment; returns the elements whose image changed.
    pub(crate) fn trim_retained(&mut self) -> Vec<NodeId> {
        let budget = self.ctx.retained_budget();
        self.retained.backlog = false;
        if self.retained.total <= budget {
            return Vec::new();
        }
        let mut order: Vec<(bool, u64, u64, NodeId)> = self
            .retained
            .entries
            .iter()
            .map(|(&node, &(_, serial))| {
                let (has_box, painted_at) = self
                    .elements
                    .get(&node)
                    .map_or((false, 0), |e| (e.has_box, e.painted_at));
                (has_box, painted_at, serial, node)
            })
            .collect();
        order.sort_unstable();
        let mut changed = Vec::new();
        // 1. The frames of elements without a box (first in the order).
        for &(has_box, _, _, node) in &order {
            if self.retained.total <= budget || has_box {
                break;
            }
            self.drop_retained(node, &mut changed);
        }
        // 2. Reduced copies for the boxes, up to the pixels of one sync.
        let mut pixels = 0u64;
        let mut capped = false;
        for &(_, _, _, node) in &order {
            if self.retained.total <= budget {
                break;
            }
            match self.reduce_retained(node, pixels, &mut changed) {
                Reduction::Skipped => {}
                Reduction::Done(done) => pixels += done,
                Reduction::Capped => {
                    capped = true;
                    break;
                }
            }
        }
        if pixels > 0 {
            self.retained.reducing_syncs += 1;
            self.retained.max_sync_pixels = self.retained.max_sync_pixels.max(pixels);
        }
        // 3. Drops. While reductions wait for the next sync the total may
        // stay over the budget by the margin; frames that are reduced
        // already free little and stay.
        let limit = if capped {
            budget.saturating_add(RETAINED_MARGIN)
        } else {
            budget
        };
        for &(_, _, _, node) in &order {
            if self.retained.total <= limit {
                break;
            }
            if capped && self.is_reduced(node) {
                continue;
            }
            self.drop_retained(node, &mut changed);
        }
        self.retained.backlog = capped && self.retained.total > budget;
        changed
    }

    /// True if the element shows a reduced copy of its frame.
    fn is_reduced(&self, node: NodeId) -> bool {
        self.elements
            .get(&node)
            .and_then(|e| e.fetch.as_ref())
            .is_some_and(|f| f.reduced_from().is_some())
    }

    /// The page time (ms) of the next sync that continues the reductions
    /// (a sync stopped at its pixels with the total over the budget), if
    /// any. The page's loops wake for it, in virtual time too.
    pub(crate) fn reduce_due(&self, now_us: i64) -> Option<f64> {
        self.retained
            .backlog
            .then(|| now_us as f64 / 1000.0 + REDUCE_RETRY_MS)
    }

    /// Drops the retained frame of `node` (if it still has one).
    fn drop_retained(&mut self, node: NodeId, changed: &mut Vec<NodeId>) {
        if !self.retained.entries.contains_key(&node) {
            return;
        }
        let gone = self
            .elements
            .get_mut(&node)
            .and_then(|e| e.fetch.as_mut())
            .is_some_and(super::Fetch::drop_retained_frame);
        // An entry whose pipeline holds a place again (or lost its frame) no
        // longer counts.
        self.retained.set(node, 0);
        if gone {
            self.retained.dropped += 1;
            changed.push(node);
        }
    }

    /// Replaces the retained frame of `node` by a reduced copy for its box,
    /// when the element has a box that bounds its image and the copy is
    /// smaller than the frame.
    ///
    /// `spent` is the source pixels that this sync converted so far: a
    /// conversion that would pass [`REDUCE_PIXELS_PER_SYNC`] waits for the
    /// next sync (the first one of a sync always runs, so each sync that
    /// stops at the cap converted a frame).
    fn reduce_retained(
        &mut self,
        node: NodeId,
        spent: u64,
        changed: &mut Vec<NodeId>,
    ) -> Reduction {
        if !self.retained.entries.contains_key(&node) {
            return Reduction::Skipped;
        }
        let Some(element) = self.elements.get_mut(&node) else {
            return Reduction::Skipped;
        };
        let (Some(box_size), true) = (element.box_size, element.has_box) else {
            return Reduction::Skipped;
        };
        let Some(fetch) = element.fetch.as_mut() else {
            return Reduction::Skipped;
        };
        let Some(image) = fetch.frame_image() else {
            return Reduction::Skipped;
        };
        let full = fetch
            .reduced_from()
            .unwrap_or((image.width(), image.height()));
        let (width, height) = copy_size(full, box_size);
        // `reduced_copy` converts only for a scale below 0.5 in an axis; the
        // other cases cost nothing and do not count.
        let (iw, ih) = (image.width(), image.height());
        if width / (iw as f32) >= 0.5 && height / (ih as f32) >= 0.5 {
            return Reduction::Skipped;
        }
        let source_pixels = u64::from(iw) * u64::from(ih);
        if spent > 0 && spent.saturating_add(source_pixels) > REDUCE_PIXELS_PER_SYNC {
            return Reduction::Capped;
        }
        let Some(copy) = swb_paint::reduced_copy(&image, width, height) else {
            return Reduction::Skipped;
        };
        drop(image);
        if !fetch.reduce_retained_frame(std::sync::Arc::new(copy)) {
            return Reduction::Skipped;
        }
        let bytes = fetch.retained_bytes();
        self.retained.set(node, bytes);
        self.retained.reduced += 1;
        changed.push(node);
        Reduction::Done(source_pixels)
    }
}

/// The size for a reduced copy of a frame of `full` pixels in a box of
/// `box_size` device pixels: the frame at the one scale that covers the box
/// in both axes. Paint draws the frame into the box (`fill`), into a
/// rectangle inside it (`contain`, `scale-down`), or at that scale
/// (`cover`), so never at a larger scale in an axis.
fn copy_size(full: (u32, u32), box_size: (f32, f32)) -> (f32, f32) {
    let (w, h) = (full.0.max(1) as f32, full.1.max(1) as f32);
    let scale = (box_size.0 / w).max(box_size.1 / h);
    (w * scale, h * scale)
}

impl super::Element {
    /// Whether the element shows a reduced copy of its frame that is
    /// smaller than its box needs now (the box grew, or the image is no
    /// longer bounded by it): the full frame must come back.
    pub(crate) fn reduced_frame_too_small(&self) -> bool {
        let Some(fetch) = &self.fetch else {
            return false;
        };
        let Some(full) = fetch.reduced_from() else {
            return false;
        };
        let Some(copy) = fetch.frame_image() else {
            return false;
        };
        let Some(box_size) = self.box_size else {
            return self.has_box;
        };
        let (width, height) = copy_size(full, box_size);
        // The copy is the level that paint draws from for a size up to its
        // own (the halvings stop at a scale of 0.5); a little slack for the
        // rounding of the float sizes.
        width > copy.width() as f32 + 0.01 || height > copy.height() as f32 + 0.01
    }
}
